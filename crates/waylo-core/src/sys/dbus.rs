//! Bluetooth and power changes (charger, battery) through D-Bus signals.
//!
//! A single `dbus-monitor` is used (BlueZ and UPower share the process), so the bluetooth icon
//! and the charging state change instantly instead of waiting for the 60 s poll.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Properties whose change matters to us: bluetooth power and battery state.
const INTERESTING: &[&str] = &["\"Powered\"", "\"State\"", "\"Percentage\"", "\"Online\""];

/// Blocks, calling `on_event` whenever something relevant changes. If `dbus-monitor` exits,
/// it is launched again; if it does not exist, it just returns (the slow poll remains).
pub fn watch(mut on_event: impl FnMut()) {
    loop {
        let child = Command::new("dbus-monitor")
            .args([
                "--system",
                "type='signal',sender='org.bluez',interface='org.freedesktop.DBus.Properties'",
                "type='signal',sender='org.freedesktop.UPower',interface='org.freedesktop.DBus.Properties'",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else { return };
        let Some(stdout) = child.stdout.take() else { return };
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.trim_start().starts_with("string ") && INTERESTING.iter().any(|p| line.contains(p)) {
                // A change usually brings several properties together: let them settle.
                std::thread::sleep(Duration::from_millis(120));
                on_event();
            }
        }
        let _ = child.wait();
        std::thread::sleep(Duration::from_secs(5));
    }
}
