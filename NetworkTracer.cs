/*
 * DISCLAIMER:
 * This code is provided "as-is" without any express or implied warranties, including but not limited to implied warranties of merchantability or fitness for a particular purpose.
 * 
 * WARNING: 
 * Do NOT use this class in production environments. This code is intended for development and testing purposes only.
 * 
 * By using this code, you agree that you are doing so at your own risk. The author(s) will not be held liable for any damages,
 * issues, or losses incurred as a result of using, modifying, or distributing this code, whether directly or indirectly.
 * 
 * Ensure that you thoroughly test this code in your development environment to validate its suitability for your use cases.
 */


using System.Buffers.Binary;
using System.Diagnostics;

using System.IO;
using System.Threading;
using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;

namespace Microsoft.Data.SqlClient.Tracer
{

    public class NetworkTracer
    {
        internal enum NTPacketType : byte
        {
            Unknown = 0x00,
            SqlBatch = 0x01,
            RpcRequest = 0x03,
            TabularResult = 0x04,
            Attention = 0x06,
            BulkLoad = 0x07,
            FedAuthToken = 0x08,
            TransactionManager = 0x0E,
            Login7 = 0x10,
            SSPI = 0x11,
            PreLogin = 0x12
        }

        [Flags]
        internal enum NTPacketStatusFlags
        {
            Normal = 0x00,
            EOM = 0x01,
            Ignore = 0x02,
            ResetConnection = 0x08,
            ResetConnectionSkipTran = 0x10
        }

        internal class NTTDSHeader
        {
            public NTPacketType Type { get; set; }
            public NTPacketStatusFlags Status { get; set; }
            public int Length { get; set; }
            public byte PId { get; set; }

            public void Hydrate(Span<byte> content)
            {
                Type = (NTPacketType)content[0];
                Status = (NTPacketStatusFlags)content[1];
                Length = content[2] << 8 | content[3]; // Corrected Length calculation
                PId = content[6];
            }

            public override string ToString()
            {
                return $"# Type:{Type}, Status:{Status}, Length:{Length}, PId:{PId}";
            }
        }

        internal class NTMarsPacket
        {
            public NTMarsHeader Header { get; set; }
            public byte[] Buffer { get; set; }

            public NTMarsPacket(NTMarsHeader header, byte[] buffer, int offset, int length)
            {
                Header = header;
                Buffer = new byte[length];
                Array.Copy(buffer, offset, Buffer, 0, length);
            }

            public void Merge(byte[] buffer, int offset, int length)
            {
                byte[] newBuffer = new byte[Buffer.Length + length];
                List<byte> bytes = new List<byte>(length + Buffer.Length);
                bytes.AddRange(Buffer);
                bytes.AddRange(buffer.Skip(offset).Take(length));
                Buffer = bytes.ToArray();
            }
        }



        internal class NTMarsHeader
        {
            internal enum NTMarsMessageType
            {
                SYN = 1,
                ACK = 2,
                FIN = 4,
                DATA = 8
            }

            internal NTMarsHeader(bool sending)
            {
                this.sending = sending;

            }

            public byte SMID;
            public NTMarsMessageType Type;
            public ushort sessionId;
            public uint length;
            public uint sequenceNumber;
            public uint highwater;
            private bool sending;

            public void Read(byte[] bytes)
            {
                SMID = bytes[0];
                Type = (NTMarsMessageType)bytes[1];


                Span<byte> span = bytes.AsSpan();
                sessionId = BinaryPrimitives.ReadUInt16LittleEndian(span.Slice(2));
                length = BinaryPrimitives.ReadUInt32LittleEndian(span.Slice(4));
                sequenceNumber = BinaryPrimitives.ReadUInt32LittleEndian(span.Slice(8));
                highwater = BinaryPrimitives.ReadUInt32LittleEndian(span.Slice(12));
            }

            public override string ToString()
            {
                if (sending == false)
                {
                    return $"# [MARS-In] SMID:{SMID}, Type:{Type}, Sid:{sessionId}, Length:{length}, Recv-SN:{sequenceNumber}, SendWaterMark: {highwater}";
                }
                else
                {
                    return $"# [MARS-Out] SMID:{SMID}, Type:{Type}, Sid:{sessionId}, Length:{length}, Send-SN:{sequenceNumber}, RecvWaterMark: {highwater}";
                }


            }
        }

        private StreamWriter writer;
        private bool consoleEnabled;
        private volatile int byteCounter = 0;
        private bool printContent = false;
        private volatile bool marsEnabled;

        public NetworkTracer(bool consoleEnabled = true)
        {
            var fileName = Guid.NewGuid().ToString().Split('-')[0] + ".txt";
            writer = new StreamWriter(File.Create(fileName));
            this.consoleEnabled = consoleEnabled;
            printContent = true;
            marsEnabled = false;
        }


        public ValueTask<int> GetDataFromNetworkStream(byte[] buffer, int offset, int length, CancellationToken ct = default)
        {
            ProcessData(isSending: false, buffer, offset, length);
            return new ValueTask<int>(length);
        }

        public ValueTask SendDataToNetworkStream(byte[] buffer, int offset, int length, CancellationToken ct = default)
        {
            ProcessData(isSending: true, buffer, offset, length);
            return ValueTask.CompletedTask;
        }


        private void GetHeaders(bool isSending, byte[] buffer, int offset, int length, out NTTDSHeader tdsHeader, out NTMarsHeader marsHeader)
        {
            marsHeader = null;
            tdsHeader = null;
            var isMarsStartMarker = buffer[offset] == 0x53;
            if (isMarsStartMarker)
            {
                marsHeader = new NTMarsHeader(isSending);
                marsHeader.Read(buffer);
                if (!Enum.IsDefined(typeof(NTMarsHeader.NTMarsMessageType), marsHeader.Type))
                {
                    marsHeader = null;
                }
                else
                {
                    marsEnabled = true;
                }
            }

            if (marsHeader == null)
            {
                tdsHeader = new NTTDSHeader();
                tdsHeader.Hydrate(buffer.AsSpan(offset, 8));
                if (!Enum.IsDefined(typeof(NTPacketType), tdsHeader.Type) || tdsHeader.Type == NTPacketType.Unknown)
                {
                    tdsHeader = null;
                }
            }
            else if (buffer.Length > 16)
            {
                tdsHeader = new NTTDSHeader();
                tdsHeader.Hydrate(buffer.AsSpan(offset + 16, 8));
                if (!Enum.IsDefined(typeof(NTPacketType), tdsHeader.Type) || tdsHeader.Type == NTPacketType.Unknown)
                {
                    tdsHeader = null;
                }
            }
        }

        private void ProcessData(bool isSending, byte[] buffer, int offset, int length)
        {
            lock (this)
            {
                GetHeaders(isSending, buffer, offset, length, out var tdsHeader, out var marsHeader);
                var currentForeColor = Console.ForegroundColor;
                Console.ForegroundColor = currentForeColor;

                if (tdsHeader == null && marsHeader == null)
                {
                    throw new Exception("Why we are getting partial data which has no mars header nor tds header");
                    //PrettyPrintPartialData(buffer, offset, length);
                }

                try
                {
                    if (!marsEnabled)
                    {
                        HandleTdsPacket(buffer, offset, length, tdsHeader);
                    }
                    else
                    {
                        if (marsHeader != null && marsHeader.Type != NTMarsHeader.NTMarsMessageType.DATA)
                        {
                            Debug.Assert(capturePacket == null);

                            HandleMarsControlPacket(buffer, offset, length, marsHeader);
                        }
                        else if (marsHeader != null && tdsHeader != null)
                        {
                            Debug.Assert(capturePacket == null);

                            HandleMarsPacket(buffer, offset, length, tdsHeader, marsHeader);
                        }
                        else if (marsHeader != null)
                        {
                            Debug.Assert(capturePacket == null);
                            Debug.Assert(tdsHeader == null);

                            // lets bookkeep the header and wait for tdsHeader
                            capturePacket = new NTMarsPacket(marsHeader, buffer, offset, length);
                        }
                        else if (tdsHeader != null)
                        {
                            Debug.Assert(marsHeader == null);
                            Debug.Assert(capturePacket != null);

                            marsHeader = capturePacket.Header;
                            capturePacket.Merge(buffer, offset, length);
                            HandleMarsPacket(capturePacket.Buffer, 0, length, tdsHeader, marsHeader);
                        }
                        else
                        {
                            throw new Exception("Not supported Trace scenario");
                        }
                    }
                }
                finally
                {
                    Console.ForegroundColor = currentForeColor;
                }
            }
        }


        private void HandleMarsControlPacket(byte[] buffer, int offset, int length, NTMarsHeader marsHeader)
        {
            var currentForeColor = Console.ForegroundColor;
            try
            {
                byteCounter = 0;
                WriteLine();
                WriteLine();
                WriteLine(new string('=', 80), ConsoleColor.DarkGreen);
                WriteLine(marsHeader.ToString(), ConsoleColor.DarkYellow);
                WriteLine(new string('=', 80), ConsoleColor.DarkGreen);
                Console.ForegroundColor = ConsoleColor.DarkYellow;
                if (printContent)
                {
                    for (int x = offset; x < length + offset; x++)
                    {
                        if (byteCounter % 16 == 0)
                        {
                            if (byteCounter > 0)
                            {
                                WriteLine();
                            }

                            Write($"{byteCounter.ToString("X2")}\t\t\t {buffer[x]:X2} ");
                        }
                        else
                        {
                            Write($"{buffer[x]:X2} ");
                        }

                        byteCounter++;
                    }
                    WriteLine();
                    WriteLine(new string('-', 80), ConsoleColor.White);
                }
            }
            finally
            {
                Console.ForegroundColor = currentForeColor;
            }
        }

        private NTMarsPacket capturePacket = null;


        private void HandleMarsPacket(byte[] buffer, int offset, int length, NTTDSHeader tdsHeader, NTMarsHeader marsHeader)
        {
            var currentForeColor = Console.ForegroundColor;
            capturePacket = null;
            try
            {

                byteCounter = 0;
                WriteLine();
                WriteLine();
                WriteLine(new string('=', 80), ConsoleColor.DarkGreen);
                WriteLine(marsHeader.ToString(), ConsoleColor.DarkYellow);
                WriteLine(tdsHeader.ToString(), ConsoleColor.Magenta);
                WriteLine(new string('=', 80), ConsoleColor.DarkGreen);

                if (printContent)
                {
                    for (int x = offset; x < length + offset; x++)
                    {
                        if (byteCounter >= 0 && byteCounter < 16)
                        {
                            Console.ForegroundColor = ConsoleColor.DarkYellow;
                        }
                        else if (byteCounter >= 16 && byteCounter < 24)
                        {
                            Console.ForegroundColor = ConsoleColor.Magenta;
                        }
                        else
                        {
                            Console.ForegroundColor = ConsoleColor.White;
                        }


                        if (byteCounter % 16 == 0)
                        {
                            if (byteCounter > 0)
                            {
                                WriteLine();
                            }

                            Write($"{byteCounter.ToString("X2")}\t\t\t {buffer[x]:X2} ");
                        }
                        else
                        {
                            Write($"{buffer[x]:X2} ");
                        }

                        byteCounter++;
                    }
                }

                WriteLine();
                WriteLine(new string('-', 80));
                WriteLine();
                writer.Flush();
            }
            finally
            {
                Console.ForegroundColor = currentForeColor;
            }
        }


        private void HandleTdsPacket(byte[] buffer, int offset, int length, NTTDSHeader tdsHeader)
        {
            var currentForeColor = Console.ForegroundColor;
            try
            {
                byteCounter = 0;

                WriteLine();
                WriteLine(new string('=', 80), ConsoleColor.DarkGreen);
                WriteLine(tdsHeader.ToString(), ConsoleColor.Magenta);
                WriteLine(new string('=', 80), ConsoleColor.DarkGreen);

                if (printContent)
                {
                    for (int x = offset; x < length + offset; x++)
                    {
                        if (byteCounter >= 0 && byteCounter < 8)
                        {
                            Console.ForegroundColor = ConsoleColor.Magenta;
                        }
                        else
                        {
                            Console.ForegroundColor = currentForeColor;
                        }

                        if (byteCounter % 16 == 0)
                        {
                            if (byteCounter > 0)
                            {
                                WriteLine();
                            }

                            Write($"{byteCounter.ToString("X2")}\t\t\t {buffer[x]:X2} ");
                        }
                        else
                        {
                            Write($"{buffer[x]:X2} ");
                        }

                        byteCounter++;
                    }
                }

                WriteLine();
                WriteLine(new string('-', 80));
                WriteLine();
                writer.Flush();
            }
            finally
            {
                Console.ForegroundColor = currentForeColor;
            }
        }



        private void PrettyPrintPartialData(byte[] buffer, int offset, int length)
        {
            // This should not happen 
            var currentForeColor = Console.ForegroundColor;
            try
            {
                Console.ForegroundColor = ConsoleColor.DarkYellow;
                if (printContent)
                {
                    for (int x = 0; x < length; x++)
                    {
                        if (byteCounter % 16 == 0)
                        {
                            if (byteCounter > 0)
                            {
                                WriteLine();
                            }

                            Write($"{byteCounter.ToString("X2")}\t\t\t {buffer[x]:X2} ");
                        }
                        else
                        {
                            Write($"{buffer[x]:X2} ");
                        }

                        byteCounter++;
                    }
                }

                WriteLine();
                WriteLine(new string('-', 80));
                WriteLine();
            }
            finally
            {
                Console.ForegroundColor = currentForeColor;
            }
        }

        private void WriteLine(string input = "")
        {
            writer.WriteLine(input);
            if (consoleEnabled)
            {
                Console.WriteLine(input);
            }
        }

        private void WriteLine(string input, ConsoleColor color)
        {
            writer.WriteLine(input);
            if (consoleEnabled)
            {
                var currentForeColor = Console.ForegroundColor;
                Console.ForegroundColor = color;
                Console.WriteLine(input);
                Console.ForegroundColor = currentForeColor;
            }
        }


        private void Write(string input = "")
        {
            writer.Write(input);
            if (consoleEnabled)
            {
                Console.Write(input);
            }
        }
    }
        

}
