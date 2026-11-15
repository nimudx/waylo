//! Network (NetworkManager, through `nmcli`).

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Wifi,
    Ethernet,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Net {
    pub kind: Kind,
    /// Network name (SSID) or connection name.
    pub name: String,
    /// Signal 0–100 (wifi only).
    pub signal: u32,
}

/// `None` if there is no active wifi or ethernet connection.
pub fn read() -> Option<Net> {
    let out = Command::new("nmcli").args(["-t", "-f", "TYPE,STATE,CONNECTION", "device"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    for line in text.lines() {
        let mut parts = line.splitn(3, ':');
        let (Some(kind), Some("connected"), Some(name)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let name = name.replace("\\:", ":");
        match kind {
            "wifi" => {
                let out = Command::new("nmcli").args(["-t", "-f", "IN-USE,SIGNAL", "dev", "wifi"]).output().ok()?;
                let signal = String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .find_map(|l| l.strip_prefix("*:").and_then(|s| s.trim().parse().ok()))
                    .unwrap_or(0);
                return Some(Net { kind: Kind::Wifi, name, signal });
            }
            "ethernet" => return Some(Net { kind: Kind::Ethernet, name, signal: 100 }),
            _ => {}
        }
    }
    None
}

/// Blocks listening to `nmcli monitor`; calls `on_event` on every line.
pub fn watch(mut on_event: impl FnMut()) {
    let Ok(mut child) = Command::new("nmcli").arg("monitor").stdout(Stdio::piped()).spawn() else { return };
    let Some(stdout) = child.stdout.take() else { return };
    for _ in BufReader::new(stdout).lines().map_while(Result::ok) {
        on_event();
    }
}
