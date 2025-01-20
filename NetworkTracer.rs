use std::fs::File;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use std::fmt;
use std::str;

#[derive(Debug, Copy, Clone)]
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

#[derive(Debug, Copy, Clone)]
struct NTTDSHeader {
    packet_type: NTPacketType,
    status_flags: u8,
    length: u16,
    pid: u8,
}

impl NTTDSHeader {
    fn hydrate(data: &[u8]) -> Self {
        let packet_type = NTPacketType::from(data[0]);
        let status_flags = data[1];
        let length = u16::from_be_bytes([data[2], data[3]]);
        let pid = data[6];
        
        Self {
            packet_type,
            status_flags,
            length,
            pid,
        }
    }
}

impl fmt::Display for NTTDSHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "# Type: {:?}, Status: {:#04x}, Length: {}, PId: {}",
            self.packet_type, self.status_flags, self.length, self.pid
        )
    }
}

struct NetworkTracer {
    writer: Arc<Mutex<File>>,
    console_enabled: bool,
    print_content: bool,
    mars_enabled: Arc<Mutex<bool>>,
}

impl NetworkTracer {
    fn new(console_enabled: bool) -> io::Result<Self> {
        let file = File::create("trace_log.txt")?;
        Ok(Self {
            writer: Arc::new(Mutex::new(file)),
            console_enabled,
            print_content: true,
            mars_enabled: Arc::new(Mutex::new(false)),
        })
    }

    fn process_data(&self, is_sending: bool, buffer: &[u8]) {
        let mut mars_header = None;
        let mut tds_header = None;

        if buffer[0] == 0x53 {
            mars_header = Some(self.parse_mars_header(buffer));
            let mut mars_enabled = self.mars_enabled.lock().unwrap();
            *mars_enabled = true;
        }

        if mars_header.is_none() {
            tds_header = Some(NTTDSHeader::hydrate(&buffer[..8]));
        } else if buffer.len() > 16 {
            tds_header = Some(NTTDSHeader::hydrate(&buffer[16..24]));
        }

        if tds_header.is_none() && mars_header.is_none() {
            eprintln!("No valid headers found in data.");
        }

        if let Some(tds) = tds_header {
            self.handle_tds_packet(&buffer, tds);
        }
    }

    fn parse_mars_header(&self, _buffer: &[u8]) -> Option<()> {
        // Implement MARS header parsing logic
        None
    }

    fn handle_tds_packet(&self, buffer: &[u8], header: NTTDSHeader) {
        println!("Processing TDS Packet: {}", header);
        if self.print_content {
            for (i, byte) in buffer.iter().enumerate() {
                if i % 16 == 0 {
                    println!();
                }
                print!("{:02X} ", byte);
            }
            println!();
        }
    }

    fn write_log(&self, message: &str) {
        if let Ok(mut writer) = self.writer.lock() {
            writeln!(writer, "{}", message).unwrap_or_else(|e| eprintln!("Failed to write to log: {}", e));
        }

        if self.console_enabled {
            println!("{}", message);
        }
    }
}

fn main() -> io::Result<()> {
    let tracer = NetworkTracer::new(true)?;
    let sample_data = vec![0x12, 0x01, 0x00, 0x10, 0x00, 0x00, 0x01, 0x00];
    tracer.process_data(false, &sample_data);
    Ok(())
}
