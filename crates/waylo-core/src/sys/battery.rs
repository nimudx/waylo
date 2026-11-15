//! Battery (sysfs).

use std::fs;

/// (percentage, charging)
pub type Battery = (u32, bool);

/// `None` if the machine has no battery.
pub fn read() -> Option<Battery> {
    let entry = fs::read_dir("/sys/class/power_supply").ok()?.flatten().find(|e| {
        fs::read_to_string(e.path().join("type")).map(|t| t.trim() == "Battery").unwrap_or(false)
            && e.path().join("capacity").exists()
    })?;
    let level = fs::read_to_string(entry.path().join("capacity")).ok()?.trim().parse().ok()?;
    let status = fs::read_to_string(entry.path().join("status")).unwrap_or_default();
    Some((level, status.trim() == "Charging"))
}
