//! Audio volume (PipeWire, through `wpctl`).

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use crate::exec;

/// (percentage, muted)
pub type Volume = (u32, bool);

pub fn read() -> Option<Volume> {
    let out = Command::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let value: f32 = text.split_whitespace().nth(1)?.parse().ok()?;
    Some(((value * 100.).round() as u32, text.contains("MUTED")))
}

pub fn toggle_mute() {
    exec::spawn("wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle");
}

/// Raises or lowers by `steps` steps of 3 % (never above 100 %).
pub fn step(steps: i32) {
    if steps > 0 {
        exec::spawn("wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 3%+");
    } else if steps < 0 {
        exec::spawn("wpctl set-volume @DEFAULT_AUDIO_SINK@ 3%-");
    }
}

/// Blocks listening to `pactl subscribe` and calls `on_event` on every sink change.
pub fn watch(mut on_event: impl FnMut()) {
    let Ok(mut child) = Command::new("pactl").arg("subscribe").stdout(Stdio::piped()).spawn() else { return };
    let Some(stdout) = child.stdout.take() else { return };
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if line.contains("sink") || line.contains("server") {
            on_event();
        }
    }
}
