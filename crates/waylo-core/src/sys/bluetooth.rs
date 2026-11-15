//! Bluetooth state (`bluetoothctl`).

use std::process::Command;

pub fn is_on() -> bool {
    Command::new("bluetoothctl")
        .arg("show")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("Powered: yes"))
        .unwrap_or(false)
}
