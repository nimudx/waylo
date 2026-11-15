//! Launch external commands without blocking.

use std::process::{Command, Stdio};

/// Runs `command` with `sh -c` and does not wait for its result.
pub fn spawn(command: &str) {
    let command = command.to_string();
    std::thread::spawn(move || {
        let _ = Command::new("sh")
            .args(["-c", &command])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    });
}
