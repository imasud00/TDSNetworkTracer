/*
 * DISCLAIMER:
 * This code is provided "as-is" without any express or implied warranties, including but not limited to implied warranties of merchantability or fitness for a particular purpose.
 * 
 * WARNING: 
 * Do NOT use this class in production environments. This code is intended for development and testing purposes only.
 * 
 * By using this code, you agree that you are doing so at your own risk. The author(s) will not be held liable for any damages, issues, or losses incurred as a result of using, modifying, or distributing this code, whether directly or indirectly.
 * 
 * Ensure that you thoroughly test this code in your development environment to validate its suitability for your use cases.
 *
 * 






 *  NOTE THE BELOW CODE IS AUTO TRANSLATED VIA CHATGPT form C# to Java and I have not tested it.
    For out packets call this sendDataToNetworkStream before writing to the stream and call getDataFromNetworkStream after receiving the bytes
 */




import java.io.*;
import java.nio.ByteBuffer;
import java.util.*;
import java.util.concurrent.atomic.AtomicInteger;

public class NetworkTracer {

    // Enums translated from C#
    public enum NTPacketType {
        Unknown(0x00),
        SqlBatch(0x01),
        RpcRequest(0x03),
        TabularResult(0x04),
        Attention(0x06),
        BulkLoad(0x07),
        FedAuthToken(0x08),
        TransactionManager(0x0E),
        Login7(0x10),
        SSPI(0x11),
        PreLogin(0x12);

        private final byte value;

        NTPacketType(int value) {
            this.value = (byte) value;
        }

        public static NTPacketType fromByte(byte b) {
            for (NTPacketType type : values()) {
                if (type.value == b) {
                    return type;
                }
            }
            return Unknown;
        }
    }

    public enum NTPacketStatusFlags {
        Normal(0x00),
        EOM(0x01),
        Ignore(0x02),
        ResetConnection(0x08),
        ResetConnectionSkipTran(0x10);

        private final int value;

        NTPacketStatusFlags(int value) {
            this.value = value;
        }

        public static NTPacketStatusFlags fromByte(byte b) {
            for (NTPacketStatusFlags flag : values()) {
                if (flag.value == b) {
                    return flag;
                }
            }
            return Normal;
        }
    }

    public class NTTDSHeader {
        public NTPacketType type;
        public NTPacketStatusFlags status;
        public int length;
        public byte pId;

        public void hydrate(byte[] content) {
            type = NTPacketType.fromByte(content[0]);
            status = NTPacketStatusFlags.fromByte(content[1]);
            length = (content[2] << 8) | content[3];
            pId = content[6];
        }

        @Override
        public String toString() {
            return "# Type:" + type + ", Status:" + status + ", Length:" + length + ", PId:" + pId;
        }
    }

    public class NTMarsPacket {
        public NTMarsHeader header;
        public byte[] buffer;

        public NTMarsPacket(NTMarsHeader header, byte[] buffer, int offset, int length) {
            this.header = header;
            this.buffer = Arrays.copyOfRange(buffer, offset, offset + length);
        }

        public void merge(byte[] buffer, int offset, int length) {
            byte[] newBuffer = Arrays.copyOf(this.buffer, this.buffer.length + length);
            System.arraycopy(buffer, offset, newBuffer, this.buffer.length, length);
            this.buffer = newBuffer;
        }
    }

    public class NTMarsHeader {
        public enum NTMarsMessageType {
            SYN(1),
            ACK(2),
            FIN(4),
            DATA(8);

            private final int value;

            NTMarsMessageType(int value) {
                this.value = value;
            }

            public static NTMarsMessageType fromByte(byte b) {
                for (NTMarsMessageType type : values()) {
                    if (type.value == b) {
                        return type;
                    }
                }
                return null;
            }
        }

        public byte SMID;
        public NTMarsMessageType type;
        public short sessionId;
        public int length;
        public int sequenceNumber;
        public int highwater;
        private boolean sending;

        public NTMarsHeader(boolean sending) {
            this.sending = sending;
        }

        public void read(byte[] bytes) {
            SMID = bytes[0];
            type = NTMarsMessageType.fromByte(bytes[1]);
            ByteBuffer buffer = ByteBuffer.wrap(bytes);
            sessionId = buffer.getShort(2);
            length = buffer.getInt(4);
            sequenceNumber = buffer.getInt(8);
            highwater = buffer.getInt(12);
        }

        @Override
        public String toString() {
            return (sending ? "# [MARS-Out]" : "# [MARS-In]") + " SMID:" + SMID + ", Type:" + type +
                    ", Sid:" + sessionId + ", Length:" + length + ", Recv-SN:" + sequenceNumber + ", SendWaterMark: " + highwater;
        }
    }

    private BufferedWriter writer;
    private boolean consoleEnabled;
    private AtomicInteger byteCounter = new AtomicInteger(0);
    private boolean printContent = true;
    private volatile boolean marsEnabled;

    public NetworkTracer(boolean consoleEnabled) throws IOException {
        String fileName = UUID.randomUUID().toString().split("-")[0] + ".txt";
        writer = new BufferedWriter(new FileWriter(fileName));
        this.consoleEnabled = consoleEnabled;
        marsEnabled = false;
    }

    public int getDataFromNetworkStream(byte[] buffer, int offset, int length) {
        processData(false, buffer, offset, length);
        return length;
    }

    public void sendDataToNetworkStream(byte[] buffer, int offset, int length) {
        processData(true, buffer, offset, length);
    }

    private void getHeaders(boolean isSending, byte[] buffer, int offset, int length, 
                            NTTDSHeader tdsHeader, NTMarsHeader marsHeader) {
        boolean isMarsStartMarker = buffer[offset] == 0x53;
        if (isMarsStartMarker) {
            marsHeader = new NTMarsHeader(isSending);
            marsHeader.read(buffer);
            if (!EnumSet.allOf(NTMarsHeader.NTMarsMessageType.class).contains(marsHeader.type)) {
                marsHeader = null;
            } else {
                marsEnabled = true;
            }
        }

        if (marsHeader == null) {
            tdsHeader = new NTTDSHeader();
            tdsHeader.hydrate(Arrays.copyOfRange(buffer, offset, offset + 8));
            if (!EnumSet.allOf(NTPacketType.class).contains(tdsHeader.type) || tdsHeader.type == NTPacketType.Unknown) {
                tdsHeader = null;
            }
        } else if (buffer.length > 16) {
            tdsHeader = new NTTDSHeader();
            tdsHeader.hydrate(Arrays.copyOfRange(buffer, offset + 16, offset + 24));
            if (!EnumSet.allOf(NTPacketType.class).contains(tdsHeader.type) || tdsHeader.type == NTPacketType.Unknown) {
                tdsHeader = null;
            }
        }
    }

    private void processData(boolean isSending, byte[] buffer, int offset, int length) {
        synchronized (this) {
            NTTDSHeader tdsHeader = null;
            NTMarsHeader marsHeader = null;
            getHeaders(isSending, buffer, offset, length, tdsHeader, marsHeader);

            if (tdsHeader == null && marsHeader == null) {
                throw new IllegalStateException("Partial data with no MARS or TDS headers");
            }

            if (!marsEnabled) {
                handleTdsPacket(buffer, offset, length, tdsHeader);
            } else if (marsHeader != null && marsHeader.type != NTMarsHeader.NTMarsMessageType.DATA) {
                handleMarsControlPacket(buffer, offset, length, marsHeader);
            } else if (marsHeader != null && tdsHeader != null) {
                handleMarsPacket(buffer, offset, length, tdsHeader, marsHeader);
            }
        }
    }

    private void handleMarsControlPacket(byte[] buffer, int offset, int length, NTMarsHeader marsHeader) {
        // Content printing logic would go here. This is mostly for logging/printing bytes to console or file.
        printPacket(marsHeader.toString(), buffer, offset, length);
    }

    private void handleMarsPacket(byte[] buffer, int offset, int length, NTTDSHeader tdsHeader, NTMarsHeader marsHeader) {
        // Content printing logic for Mars + TDS packets.
        printPacket(marsHeader.toString() + "\n" + tdsHeader.toString(), buffer, offset, length);
    }

    private void handleTdsPacket(byte[] buffer, int offset, int length, NTTDSHeader tdsHeader) {
        // Content printing logic for TDS packets.
        printPacket(tdsHeader.toString(), buffer, offset, length);
    }

    private void printPacket(String header, byte[] buffer, int offset, int length) {
        // Simplified printing logic to simulate packet data being logged to console/file
        try {
            writer.write(header + "\n");
            if (consoleEnabled) {
                System.out.println(header);
            }
            // Simulate hex dump of packet data
            for (int i = offset; i < offset + length; i++) {
                if (i % 16 == 0 && i != 0) {
                    writer.write("\n");
                    if (consoleEnabled) System.out.println();
                }
                String hexByte = String.format("%02X ", buffer[i]);
                writer.write(hexByte);
                if (consoleEnabled) System.out.print(hexByte);
            }
            writer.write("\n");
            if (consoleEnabled) System.out.println();
            writer.flush();
        } catch (IOException e) {
            e.printStackTrace();
        }
    }
}
