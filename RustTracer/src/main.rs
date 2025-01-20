use std::fs::File;
use std::io::{self, Write};
use std::sync::Mutex;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Equivalent to C#:
/// internal enum NTPacketType : byte
#[derive(Copy, Clone, Debug, PartialEq)]
enum NTPacketType {
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
    PreLogin = 0x12,
}

impl From<u8> for NTPacketType {
    fn from(value: u8) -> Self {
        match value {
            0x01 => Self::SqlBatch,
            0x03 => Self::RpcRequest,
            0x04 => Self::TabularResult,
            0x06 => Self::Attention,
            0x07 => Self::BulkLoad,
            0x08 => Self::FedAuthToken,
            0x0E => Self::TransactionManager,
            0x10 => Self::Login7,
            0x11 => Self::SSPI,
            0x12 => Self::PreLogin,
            _ => Self::Unknown,
        }
    }
}

/// Equivalent to C#:
/// [Flags]
/// internal enum NTPacketStatusFlags
/// {
///     Normal = 0x00,
///     EOM = 0x01,
///     Ignore = 0x02,
///     ResetConnection = 0x08,
///     ResetConnectionSkipTran = 0x10
/// }
#[derive(Copy, Clone, Debug)]
struct NTPacketStatusFlags(u8);

impl NTPacketStatusFlags {
    fn from_byte(b: u8) -> Self {
        NTPacketStatusFlags(b)
    }
}

impl fmt::Display for NTPacketStatusFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // For simplicity, just display the raw hex of the flags
        write!(f, "{:#04x}", self.0)
    }
}

/// Equivalent to C#:
/// internal class NTTDSHeader
#[derive(Debug, Clone)]
struct NTTDSHeader {
    pub packet_type: NTPacketType,
    pub status: NTPacketStatusFlags,
    pub length: u16,
    pub pid: u8,
}

impl NTTDSHeader {
    pub fn hydrate(content: &[u8]) -> Self {
        let packet_type = NTPacketType::from(content[0]);
        let status = NTPacketStatusFlags::from_byte(content[1]);
        let length = u16::from_be_bytes([content[2], content[3]]);
        let pid = content[6];
        Self {
            packet_type,
            status,
            length,
            pid,
        }
    }
}

impl fmt::Display for NTTDSHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "# Type:{:?}, Status:{}, Length:{}, PId:{}",
            self.packet_type, self.status, self.length, self.pid
        )
    }
}

/// Equivalent to C#:
/// internal class NTMarsHeader
#[derive(Clone, Debug)]
struct NTMarsHeader {
    pub smid: u8,
    pub msg_type: NTMarsMessageType,
    pub session_id: u16,
    pub length: u32,
    pub sequence_number: u32,
    pub highwater: u32,
    pub sending: bool,
}

#[derive(Clone, Debug)]

enum NTMarsMessageType {
    SYN = 0x1,
    ACK = 0x2,
    FIN = 0x3,
    DATA = 0x4,
    Unknown = 0x10,
}

impl From<u8> for NTMarsMessageType {
    fn from(value: u8) -> Self {
        match value {
            1 => NTMarsMessageType::SYN,
            2 => NTMarsMessageType::ACK,
            3 => NTMarsMessageType::FIN,
            4 => NTMarsMessageType::DATA,
            // If none of these match, go with Unknown (no data stored)
            _ => NTMarsMessageType::Unknown,
        }
    }
}

impl fmt::Display for NTMarsHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.sending == false {
            write!(
                f,
                "# [MARS-In] SMID:{}, Type:{:?}, Sid:{}, Length:{}, Recv-SN:{}, SendWaterMark:{}",
                self.smid, self.msg_type, self.session_id, self.length, self.sequence_number, self.highwater
            )
        } else {
            write!(
                f,
                "# [MARS-Out] SMID:{}, Type:{:?}, Sid:{}, Length:{}, Send-SN:{}, RecvWaterMark:{}",
                self.smid, self.msg_type, self.session_id, self.length, self.sequence_number, self.highwater
            )
        }
    }
}

impl NTMarsHeader {
    pub fn new(sending: bool) -> Self {
        Self {
            smid: 0,
            msg_type: NTMarsMessageType::Unknown,
            session_id: 0,
            length: 0,
            sequence_number: 0,
            highwater: 0,
            sending,
        }
    }

    pub fn read(&mut self, bytes: &[u8]) {
        self.smid = bytes[0];
        self.msg_type = NTMarsMessageType::from(bytes[1]);
        self.session_id = u16::from_le_bytes([bytes[2], bytes[3]]);
        self.length = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        self.sequence_number = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        self.highwater = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    }
}

/// Equivalent to C#:
/// internal class NTMarsPacket
#[derive(Debug, Clone)]
struct NTMarsPacket {
    pub header: NTMarsHeader,
    pub buffer: Vec<u8>,
}

impl NTMarsPacket {
    pub fn new(header: NTMarsHeader, buffer: &[u8], offset: usize, length: usize) -> Self {
        let mut data = vec![0u8; length];
        data.copy_from_slice(&buffer[offset..offset + length]);
        Self {
            header,
            buffer: data,
        }
    }

    pub fn merge(&mut self, buffer: &[u8], offset: usize, length: usize) {
        self.buffer
            .extend_from_slice(&buffer[offset..offset + length]);
    }
}

/// This is the main "NetworkTracer" class in Rust form
pub struct NetworkTracer {
    writer: Mutex<File>,
    console_enabled: bool,
    byte_counter: Mutex<usize>,
    print_content: bool,
    mars_enabled: Mutex<bool>,
    capture_packet: Mutex<Option<NTMarsPacket>>,
}

impl NetworkTracer {
    /// The constructor.
    pub fn new(console_enabled: bool) -> io::Result<Self> {
        let file_name = format!(
            "{}.txt",
            uuid_like_random_string().unwrap_or_else(|| "trace".to_string())
        );
        let file = File::create(file_name)?;
        Ok(Self {
            writer: Mutex::new(file),
            console_enabled,
            byte_counter: Mutex::new(0),
            print_content: true,
            mars_enabled: Mutex::new(false),
            capture_packet: Mutex::new(None),
        })
    }

    /// In .NET: ValueTask<int> GetDataFromNetworkStream(...)
    /// We'll simply replicate by returning the length read.
    pub fn get_data_from_network_stream(
        &self,
        buffer: &[u8],
        offset: usize,
        length: usize,
    ) -> usize {
        self.process_data(false, buffer, offset, length);
        length
    }

    /// In .NET: ValueTask SendDataToNetworkStream(...)
    pub fn send_data_to_network_stream(&self, buffer: &[u8], offset: usize, length: usize) {
        self.process_data(true, buffer, offset, length);
    }

    fn process_data(&self, is_sending: bool, buffer: &[u8], offset: usize, length: usize) {
        //let _guard = self.writer.lock().unwrap(); // replicate lock(this)

        let (mut tds_header, mut mars_header) = self.get_headers(is_sending, buffer, offset, length);

        if tds_header.is_none() && mars_header.is_none() {
            self.write_line("Exception: partial data with no TDS/MARS header.");
            self.pretty_print_partial_data(buffer, offset, length);
            return;
        }

        //eprintln!("Here3");
        let mars_en = *self.mars_enabled.lock().unwrap();
        if !mars_en {
            if let Some(th) = tds_header.take() {
                self.handle_tds_packet(buffer, offset, length, &th);
            }
            return;
        }

        //eprintln!("Here4");
        // MARS is enabled
        match (mars_header.take(), tds_header.take()) {
            (Some(mh), None) => {
                // Possibly partial
                let mut cap = self.capture_packet.lock().unwrap();
                assert!(cap.is_none());
                *cap = Some(NTMarsPacket::new(mh, buffer, offset, length));
            }
            (None, Some(th)) => {
                // We might have leftover partial MARS
                let mut cap = self.capture_packet.lock().unwrap();
                if let Some(ref mut existing) = *cap {
                    existing.merge(buffer, offset, length);
                    let final_buf = existing.buffer.clone();
                    let mars_header_2 = existing.header.clone();
                    *cap = None;
                    self.handle_mars_packet(&final_buf, 0, final_buf.len(), &th, &mars_header_2);
                } else {
                    self.write_line("Error: MARS is enabled, but TDS header arrived without partial MARS header.");
                }
            }
            (Some(mh), Some(th)) => {
                // Complete MARS + TDS in one shot
                let cap = self.capture_packet.lock().unwrap();
                assert!(cap.is_none());
                self.handle_mars_packet(buffer, offset, length, &th, &mh);
            }
            (None, None) => {
                self.write_line("No recognized MARS or TDS header??");
            }
        }
    }

    fn get_headers(
        &self,
        is_sending: bool,
        buffer: &[u8],
        offset: usize,
        length: usize,
    ) -> (Option<NTTDSHeader>, Option<NTMarsHeader>) {
        let mut mars_header: Option<NTMarsHeader> = None;
        let mut tds_header: Option<NTTDSHeader> = None;

        // MARS start marker = 0x53
        if length > 0 && buffer[offset] == 0x53 && length >= 16 {
            let mut mh = NTMarsHeader::new(is_sending);
            mh.read(&buffer[offset..offset + 16]);
            match mh.msg_type {
                NTMarsMessageType::SYN
                | NTMarsMessageType::ACK
                | NTMarsMessageType::FIN
                | NTMarsMessageType::DATA => {
                    let mut me = self.mars_enabled.lock().unwrap();
                    *me = true;
                    mars_header = Some(mh);
                }
                _ => {
                    mars_header = None;
                }
            }
        }

        if mars_header.is_none() {
            // Then treat it as TDS if we have 8 bytes
            if length >= 8 {
                let th = NTTDSHeader::hydrate(&buffer[offset..offset + 8]);
                if th.packet_type != NTPacketType::Unknown {
                    tds_header = Some(th);
                }
            }
        } else {
            // Possibly TDS at offset + 16
            if length >= 24 {
                let th = NTTDSHeader::hydrate(&buffer[offset + 16..offset + 24]);
                if th.packet_type != NTPacketType::Unknown {
                    tds_header = Some(th);
                }
            }
        }

        (tds_header, mars_header)
    }

    fn handle_tds_packet(&self, buffer: &[u8], offset: usize, length: usize, tds_header: &NTTDSHeader) {
        //eprintln!("Here3aa");
        self.reset_byte_counter();
        //eprintln!("Here3ab");
        self.write_line("");
        //eprintln!("Here3ac");
        self.write_line(&"=".repeat(80));
        self.write_line(&tds_header.to_string());
        self.write_line(&"=".repeat(80));
        //eprintln!("Here3a");
        if self.print_content {
            let mut byte_counter = self.byte_counter.lock().unwrap();
            for i in 0..length {
                let x = buffer[offset + i];
                if *byte_counter % 16 == 0 {
                    if *byte_counter > 0 {
                        self.write_line("");
                    }
                    self.write_no_nl(&format!("{:02X}\t\t\t {:02X} ", *byte_counter, x));
                } else {
                    self.write_no_nl(&format!("{:02X} ", x));
                }
                *byte_counter += 1;
            }
        }
       // eprintln!("Here3b");
        self.write_line("");
        self.write_line(&"-".repeat(80));
        self.write_line("");
        self.flush_writer();
    }

    fn handle_mars_control_packet(&self, buffer: &[u8], offset: usize, length: usize, mars_header: &NTMarsHeader) {
        self.reset_byte_counter();
        self.write_line("");
        self.write_line("");
        self.write_line(&"=".repeat(80));
        self.write_line(&mars_header.to_string());
        self.write_line(&"=".repeat(80));

        if self.print_content {
            let mut byte_counter = self.byte_counter.lock().unwrap();
            for i in 0..length {
                let x = buffer[offset + i];
                if *byte_counter % 16 == 0 {
                    if *byte_counter > 0 {
                        self.write_line("");
                    }
                    self.write_no_nl(&format!("{:02X}\t\t\t {:02X} ", *byte_counter, x));
                } else {
                    self.write_no_nl(&format!("{:02X} ", x));
                }
                *byte_counter += 1;
            }
        }
        self.write_line("");
        self.write_line(&"-".repeat(80));
        self.write_line("");
    }

    fn handle_mars_packet(
        &self,
        buffer: &[u8],
        offset: usize,
        length: usize,
        tds_header: &NTTDSHeader,
        mars_header: &NTMarsHeader,
    ) {
        match mars_header.msg_type {
            NTMarsMessageType::DATA => {
                self.reset_byte_counter();
                self.write_line("");
                self.write_line("");
                self.write_line(&"=".repeat(80));
                self.write_line(&mars_header.to_string());
                self.write_line(&tds_header.to_string());
                self.write_line(&"=".repeat(80));

                if self.print_content {
                    let mut byte_counter = self.byte_counter.lock().unwrap();
                    for i in 0..length {
                        let x = buffer[offset + i];
                        if *byte_counter % 16 == 0 {
                            if *byte_counter > 0 {
                                self.write_line("");
                            }
                            self.write_no_nl(&format!("{:02X}\t\t\t {:02X} ", *byte_counter, x));
                        } else {
                            self.write_no_nl(&format!("{:02X} ", x));
                        }
                        *byte_counter += 1;
                    }
                }
                self.write_line("");
                self.write_line(&"-".repeat(80));
                self.write_line("");
                self.flush_writer();
            }
            _ => {
                // Control packet
                self.handle_mars_control_packet(buffer, offset, length, mars_header);
            }
        }
    }

    fn pretty_print_partial_data(&self, buffer: &[u8], offset: usize, length: usize) {
        self.reset_byte_counter();
        if self.print_content {
            let mut byte_counter = self.byte_counter.lock().unwrap();
            for i in 0..length {
                let x = buffer[offset + i];
                if *byte_counter % 16 == 0 {
                    if *byte_counter > 0 {
                        self.write_line("");
                    }
                    self.write_no_nl(&format!("{:02X}\t\t\t {:02X} ", *byte_counter, x));
                } else {
                    self.write_no_nl(&format!("{:02X} ", x));
                }
                *byte_counter += 1;
            }
            self.write_line("");
            self.write_line(&"-".repeat(80));
            self.write_line("");
        }
    }

    fn reset_byte_counter(&self) {
        let mut bc = self.byte_counter.lock().unwrap();
        *bc = 0;
    }

    fn write_line(&self, s: &str) {
        {
            //eprintln!("Here3aba");
            let mut writer = self.writer.lock().unwrap();
            //eprintln!("Here3abb");
            let _ = writeln!(writer, "{}", s);
        }
        if self.console_enabled {
            println!("{}", s);
        }
    }

    fn write_no_nl(&self, s: &str) {
        {
            let mut writer = self.writer.lock().unwrap();
            let _ = write!(writer, "{}", s);
        }
        if self.console_enabled {
            print!("{}", s);
        }
    }

    fn flush_writer(&self) {
        let mut writer = self.writer.lock().unwrap();
        let _ = writer.flush();
    }
}

/// Generate a pseudo-random 8-byte integer in place of a real GUID segment
fn rand_8bytes() -> u32 {
    // We do a trivial approach using system time. 
    // If you want a real RNG, include the `rand` crate and use that.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id() as u128;
    (now ^ pid).wrapping_add(12345) as u32
}

/// Mimic the "Guid.NewGuid().ToString().Split('-')[0]" from C#
fn uuid_like_random_string() -> Option<String> {
    Some(format!("{:08X}", rand_8bytes()))
}

fn main() -> io::Result<()> {
    // Example usage:
    let tracer = NetworkTracer::new(true)?;

    // TDS example (PreLogin = 0x12)
    let sample_tds_data = vec![0x12, 0x01, 0x00, 0x10, 0x00, 0x00, 0x01, 0x00];
    tracer.get_data_from_network_stream(&sample_tds_data, 0, sample_tds_data.len());

    // Partial MARS step 1
    let partial_mars_data = vec![
        0x53,       // 'S'
        0x08,       // msg_type = DATA
        0x34, 0x12, // session_id = 0x1234 (le)
        0x10, 0x00, 0x00, 0x00, // length=16
        0x01, 0x00, 0x00, 0x00, // sequence=1
        0x02, 0x00, 0x00, 0x00, // highwater=2
    ];
    tracer.get_data_from_network_stream(&partial_mars_data, 0, partial_mars_data.len());

    // Partial MARS step 2 + TDS
    let partial_tds = vec![
        // TDS header: type=0x04, ...
        0x04, 0x01, 0x00, 0x10, 0x00, 0x00, 0x05, 0x00,
        0xAA, 0xBB, 0xCC,
    ];
    tracer.get_data_from_network_stream(&partial_tds, 0, partial_tds.len());

    // Simulate sending data
    let send_data = vec![0x10, 0x03, 0x00, 0x15, 0x00, 0x00, 0x02, 0x00];
    tracer.send_data_to_network_stream(&send_data, 0, send_data.len());

    Ok(())
}
