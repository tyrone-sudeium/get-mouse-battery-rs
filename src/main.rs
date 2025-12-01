use hidapi::{HidApi, HidDevice};
use std::thread;
use std::time::Duration;

const RAZER_VID: u16 = 0x1532;
const PID_BASILISK_V3_WIRED: u16 = 0x00AA;
const PID_BASILISK_V3_WIRELESS: u16 = 0x00AB;

const REPORT_INDEX: u8 = 0x00;
const STATUS_NEW_COMMAND: u8 = 0x00;
const COMMAND_CLASS_MISC: u8 = 0x07;
const TRANSACTION_ID: u8 = 0x1F;

#[repr(u8)]
#[derive(Clone, Copy)]
enum RazerCommand {
    GetBattery = 0x80,
    GetChargingStatus = 0x84
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = HidApi::new()?;

    // Try Wired First, then Wireless
    let device = find_device(&api, PID_BASILISK_V3_WIRED, "Wired")
        .or_else(|| find_device(&api, PID_BASILISK_V3_WIRELESS, "Wireless"));

    match device {
        Some((dev, _name)) => {
            let raw_level = get_battery_level(&dev)?;
            let charging = get_charging_status(&dev)?;
            let level = (raw_level as f32 / 255.0 * 100.0) as u8;
            let charge_status = if charging {
                " ⚡"
            } else {
                ""
            };
            println!("{}%{}", level, charge_status);
        }
        None => {
            eprintln!("Error: Razer Basilisk V3 Pro not found.");
            eprintln!("Checked Wired (0x{:04X}) and Wireless (0x{:04X}) on Interface 0.", PID_BASILISK_V3_WIRED, PID_BASILISK_V3_WIRELESS);
        }
    }

    Ok(())
}

fn find_device(api: &HidApi, pid: u16, name: &str) -> Option<(HidDevice, String)> {
    let device_info = api.device_list().find(|d| {
        d.vendor_id() == RAZER_VID && d.product_id() == pid && d.interface_number() == 0 
    });

    if let Some(info) = device_info {
        if let Ok(dev) = info.open_device(api) {
            return Some((dev, name.to_string()));
        }
    }
    None
}

fn get_battery_level(device: &HidDevice) -> Result<u8, String> {
    get_razer_report(device, RazerCommand::GetBattery)
}

fn get_charging_status(device: &HidDevice) -> Result<bool, String> {
    get_razer_report(device, RazerCommand::GetChargingStatus).map(|byte| byte == 1)
}

fn get_razer_report(device: &HidDevice, cmd: RazerCommand) -> Result<u8, String> {
    // Razer HID Report Structure (90 bytes + 1 byte Report ID)
    let mut buf = [0u8; 91];

    buf[0] = REPORT_INDEX;
    buf[1] = STATUS_NEW_COMMAND;
    buf[2] = TRANSACTION_ID;
    buf[7] = COMMAND_CLASS_MISC; // 0x07
    buf[8] = cmd as u8;

    // Calculate Checksum (XOR bytes 2..88)
    // Note: In the 0-indexed buffer, this is indices 3..89 (buf[89] is where checksum goes)
    let mut checksum: u8 = 0;
    for i in 3..89 {
        checksum ^= buf[i];
    }
    buf[89] = checksum;

    if cfg!(debug_assertions) {
        println!("Raw Request Dump:");
        print_hex_dump(&buf);
    }

    // Send
    device.send_feature_report(&buf).map_err(|e| format!("Write failed: {}", e))?;
    
    // Wait for firmware to process the command
    thread::sleep(Duration::from_millis(50));

    let mut response_buf = [0u8; 91];
    response_buf[0] = REPORT_INDEX;
    
    let len = device.get_feature_report(&mut response_buf).map_err(|e| format!("Read failed: {}", e))?;

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

    // By amazing coincidence for both commands I care about the response byte is #10. Nice.
    Ok(response_buf[10])
}

fn print_hex_dump(data: &[u8]) {
    for (i, byte) in data.iter().enumerate() {
        if i % 16 == 0 { print!("\n{:02x}: ", i); }
        print!("{:02x} ", byte);
    }
    println!();
}
