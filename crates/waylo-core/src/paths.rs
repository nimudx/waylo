//! Configuration paths (`~/.config/waylo` or `$XDG_CONFIG_HOME/waylo`).

use std::path::PathBuf;

pub fn config_dir() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("waylo"))
}

pub fn config_file(name: &str) -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(name))
}

/// Directory where, for compatibility with `xdisplay`, the files read by the Hyprland config
/// (`monitors.lua`, `workspaces.lua`) and the profiles are stored. It will be renamed to `waylo` once
/// `xdisplay` is gone and `core/monitors.lua` is updated.
pub fn legacy_dir() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("xdisplay"))
}

pub fn legacy_file(name: &str) -> Option<PathBuf> {
    legacy_dir().map(|dir| dir.join(name))
}
