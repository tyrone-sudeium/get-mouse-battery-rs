use hidapi::{HidApi, HidDevice};
use std::thread;
use std::time::Duration;

const RAZER_VID: u16 = 0x1532;
const PID_BASILISK_V3_WIRED: u16 = 0x00AA;
const PID_BASILISK_V3_WIRELESS: u16 = 0x00AB;

const REPORT_INDEX: u8 = 0x00;
const COMMAND_CLASS_MISC: u8 = 0x07;
const TRANSACTION_ID: u8 = 0x1F;

#[repr(u8)]
#[derive(Clone, Copy)]
enum RazerCommand {
    GetBattery = 0x80,
    GetChargingStatus = 0x84,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
enum ReportStatus {
    NewCommand = 0x00,
    Busy = 0x01,
    Success = 0x02,
    Failure = 0x03,
    Timeout = 0x04,
    Unsupported = 0x05,
}

struct InvalidReportStatusError {
    invalid_byte: u8,
}

impl TryFrom<u8> for ReportStatus {
    type Error = InvalidReportStatusError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(ReportStatus::NewCommand),
            0x01 => Ok(ReportStatus::Busy),
            0x02 => Ok(ReportStatus::Success),
            0x03 => Ok(ReportStatus::Failure),
            0x04 => Ok(ReportStatus::Timeout),
            0x05 => Ok(ReportStatus::Unsupported),
            _ => Err(InvalidReportStatusError {
                invalid_byte: value,
            }),
        }
    }
}

#[derive(Debug, Clone)]
struct RazerReport {
    status: ReportStatus,
    transaction_id: u8,
    remaining_packets: u16,
    protocol_type: u8,
    data_size: u8,
    command_class: u8,
    command_id: u8,
    arguments: [u8; 80],
    #[allow(dead_code)]
    crc: u8,
    reserved: u8,
}

struct ReportParseError {
    message: String,
}

impl RazerReport {
    const SIZE: usize = 1 + 1 + 2 + 1 + 1 + 1 + 1 + 80 + 1 + 1; // 90 bytes

    /// Converts the RazerReport into a byte vector (raw report data).
    fn to_bytes(&self) -> Vec<u8> {
        let mut buffer = vec![0u8; Self::SIZE];
        let bytes = &mut buffer[..];

        bytes[0] = self.status as u8;
        bytes[1] = self.transaction_id;

        // remaining_packets must be converted back to Big Endian for the report
        let remaining_packets_be = self.remaining_packets.to_be_bytes();
        bytes[2..4].copy_from_slice(&remaining_packets_be);

        bytes[4] = self.protocol_type;
        bytes[5] = self.data_size;
        bytes[6] = self.command_class;
        bytes[7] = self.command_id;

        // Copy arguments array
        bytes[8..88].copy_from_slice(&self.arguments);

        // Calculate the CRC from the other bytes
        let mut checksum: u8 = 0;
        for i in 2..88 {
            checksum ^= bytes[i];
        }
        bytes[88] = checksum;
        bytes[89] = self.reserved;

        buffer
    }
}

impl TryFrom<[u8; 90]> for RazerReport {
    type Error = ReportParseError;

    /// Creates a RazerReport from a byte slice (raw report data).
    fn try_from(bytes: [u8; 90]) -> Result<Self, Self::Error> {
        if bytes.len() != Self::SIZE {
            return Err(ReportParseError {
                message: "Input byte slice is not the correct size (expected 90 bytes)."
                    .to_string(),
            });
        }

        // Remaining packets (indices 2-3) are Big Endian in the report
        let remaining_packets_be: [u8; 2] = bytes[2..4].try_into().unwrap();
        let remaining_packets = u16::from_be_bytes(remaining_packets_be);

        // Arguments (indices 8-87)
        let arguments: [u8; 80] = bytes[8..88].try_into().unwrap();
        let status: ReportStatus =
            bytes[0]
                .try_into()
                .map_err(|e: InvalidReportStatusError| ReportParseError {
                    message: format!("Unexpected status: {}", e.invalid_byte),
                })?;

        Ok(RazerReport {
            status: status,
            transaction_id: bytes[1],
            remaining_packets,
            protocol_type: bytes[4],
            data_size: bytes[5],
            command_class: bytes[6],
            command_id: bytes[7],
            arguments,
            crc: bytes[88],
            reserved: bytes[89],
        })
    }
}

macro_rules! debug_eprintln {
    ($($arg:tt)*) => (if ::std::cfg!(debug_assertions) { ::std::eprintln!($($arg)*); })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = HidApi::new()?;

    // Try Wired First, then Wireless
    let device = find_device(&api, PID_BASILISK_V3_WIRED, "Wired")
        .or_else(|| find_device(&api, PID_BASILISK_V3_WIRELESS, "Wireless"));

    match device {
        Some((dev, _name)) => {
            let levels_report = get_razer_report(&dev, RazerCommand::GetBattery)?;
            let charging_report = get_razer_report(&dev, RazerCommand::GetChargingStatus)?;

            // Timeouts *usually* indicate the device is switched off. The dongle can still report a timeout.
            if levels_report.status == ReportStatus::Timeout
                || charging_report.status == ReportStatus::Timeout
            {
                println!("N/A");
                return Ok(());
            }
            let level = (levels_report.arguments[1] as f32 / 255.0 * 100.0) as u8;
            let charging = charging_report.arguments[1] == 1;
            let charge_status = if charging { " ⚡" } else { "" };
            println!("{}%{}", level, charge_status);
        }
        None => {
            debug_eprintln!("Error: Razer Basilisk V3 Pro not found.");
            debug_eprintln!(
                "Checked Wired (0x{:04X}) and Wireless (0x{:04X}) on Interface 0.",
                PID_BASILISK_V3_WIRED,
                PID_BASILISK_V3_WIRELESS
            );
            println!("N/A");
        }
    }

    Ok(())
}

fn find_device(api: &HidApi, pid: u16, name: &str) -> Option<(HidDevice, String)> {
    let device_info = api
        .device_list()
        .find(|d| d.vendor_id() == RAZER_VID && d.product_id() == pid && d.interface_number() == 0);

    if let Some(info) = device_info {
        if let Ok(dev) = info.open_device(api) {
            return Some((dev, name.to_string()));
        }
    }
    None
}

fn get_razer_report(device: &HidDevice, cmd: RazerCommand) -> Result<RazerReport, String> {
    // Razer HID Report Structure (90 bytes + 1 byte Report ID)
    let req_report = RazerReport {
        status: ReportStatus::NewCommand,
        transaction_id: TRANSACTION_ID,
        remaining_packets: 0,
        protocol_type: 0,
        data_size: 0,
        command_class: COMMAND_CLASS_MISC,
        command_id: cmd as u8,
        arguments: [0u8; 80],
        crc: 0,
        reserved: 0,
    };
    let mut buf = [0u8; 91];

    buf[0] = REPORT_INDEX;
    buf[1..].copy_from_slice(&req_report.to_bytes());

    if cfg!(debug_assertions) {
        println!("Raw Request Dump:");
        print_hex_dump(&buf);
    }

    // Send
    device
        .send_feature_report(&buf)
        .map_err(|e| format!("Write failed: {}", e))?;

    // Wait for firmware to process the command
    thread::sleep(Duration::from_millis(50));

    let mut response_buf = [0u8; 91];
    response_buf[0] = REPORT_INDEX;

    let len = device
        .get_feature_report(&mut response_buf)
        .map_err(|e| format!("Read failed: {}", e))?;

    if cfg!(debug_assertions) {
        println!("Raw Response Dump:");
        print_hex_dump(&response_buf);
    }

    if len < 90 {
        return Err("Response too short".to_string());
    }

    if response_buf[2] != TRANSACTION_ID {
        return Err("Transaction ID mismatch".to_string());
    }

    let slice: [u8; 90] = response_buf[1..91].try_into().unwrap();
    let report = RazerReport::try_from(slice).map_err(|e| e.message)?;
    Ok(report)
}

fn print_hex_dump(data: &[u8]) {
    for (i, byte) in data.iter().enumerate() {
        if i % 16 == 0 {
            print!("\n{:02x}: ", i);
        }
        print!("{:02x} ", byte);
    }
    println!();
}
