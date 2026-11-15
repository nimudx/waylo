//! Queries to Hyprland through `hyprctl -j`.

use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub refresh_rate: f64,
    pub active_workspace: WorkspaceRef,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceRef {
    pub id: i64,
}

impl Monitor {
    /// A laptop's built-in screen (eDP).
    pub fn is_laptop(&self) -> bool {
        is_laptop_output(&self.name)
    }
}

/// Is this the output name of a built-in screen (eDP)?
pub fn is_laptop_output(name: &str) -> bool {
    name.starts_with("eDP")
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    /// `None` for special workspaces (scratchpad).
    pub id: Option<i64>,
    pub windows: u32,
    pub monitor: String,
}

fn query<T: for<'de> Deserialize<'de>>(what: &str) -> Result<T, String> {
    let out = Command::new("hyprctl")
        .args([what, "-j"])
        .output()
        .map_err(|e| format!("no pude ejecutar hyprctl: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("JSON inválido de hyprctl {what}: {e}"))
}

pub fn monitors() -> Result<Vec<Monitor>, String> {
    query("monitors")
}

/// Existing workspaces and the monitor they are on (special ones have no id).
pub fn workspace_monitors() -> Vec<(i64, String)> {
    workspaces().unwrap_or_default().into_iter().filter_map(|w| Some((w.id?, w.monitor))).collect()
}

pub fn workspaces() -> Result<Vec<Workspace>, String> {
    query("workspaces")
}

/// Ids of the workspaces that currently have at least one window.
pub fn occupied() -> HashSet<u32> {
    workspaces()
        .unwrap_or_default()
        .into_iter()
        .filter(|w| w.windows > 0)
        .filter_map(|w| w.id)
        .map(|id| id as u32)
        .collect()
}

/// An open window.
#[derive(Debug, Clone, Deserialize)]
pub struct Client {
    pub address: String,
    #[serde(default)]
    pub mapped: bool,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub title: String,
    pub workspace: WorkspaceRef,
    /// Stable identifier to capture the window with `grim -T`.
    #[serde(default, rename = "stableId")]
    pub stable_id: String,
    #[serde(default)]
    pub size: [i32; 2],
}

/// Visible windows (mapped and outside special workspaces), sorted by workspace.
pub fn clients() -> Result<Vec<Client>, String> {
    let mut all: Vec<Client> = query("clients")?;
    all.retain(|c| c.mapped && c.workspace.id > 0);
    all.sort_by_key(|c| c.workspace.id);
    Ok(all)
}

/// Focuses a window and waits for Hyprland to apply it (the caller usually exits right after).
pub fn focus_window(address: &str) {
    let expr = format!("hl.dsp.focus({{ window = \"address:{address}\" }})");
    match Command::new("hyprctl").args(["dispatch", &expr]).output() {
        Ok(out) if out.status.success() => {}
        Ok(out) => eprintln!("waylo: focus_window falló: {}", String::from_utf8_lossy(&out.stdout).trim()),
        Err(e) => eprintln!("waylo: no pude ejecutar hyprctl: {e}"),
    }
}

/// Runs `hyprctl` and requires it to answer `ok`. On failure returns what it answered.
pub fn hyprctl(args: &[&str]) -> Result<String, String> {
    let out = Command::new("hyprctl").args(args).output().map_err(|e| format!("no pude ejecutar hyprctl: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() && stdout == "ok" { Ok(stdout) } else { Err(stdout) }
}

/// Runs a Hyprland action (the config's Lua syntax) without blocking the caller.
fn dispatch(expr: String) {
    std::thread::spawn(move || match Command::new("hyprctl").args(["dispatch", &expr]).output() {
        Ok(out) if out.status.success() && !String::from_utf8_lossy(&out.stdout).contains("error") => {}
        Ok(out) => eprintln!(
            "waylo: hyprctl dispatch {expr} falló: {} {}",
            String::from_utf8_lossy(&out.stdout).trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(e) => eprintln!("waylo: no pude ejecutar hyprctl: {e}"),
    });
}

/// Goes to workspace `id` on whichever monitor it is.
pub fn focus_workspace(id: u32) {
    dispatch(format!("hl.dsp.focus({{ workspace = {id} }})"));
}

/// Focuses monitor `name`.
pub fn focus_monitor(name: &str) {
    dispatch(format!("hl.dsp.focus({{ monitor = \"{name}\" }})"));
}

/// Event from Hyprland's socket (`name>>data`).
#[derive(Debug, Clone)]
pub struct Event {
    pub name: String,
    pub data: String,
}

/// Connects to Hyprland's event socket. The iterator blocks until one arrives and
/// ends if Hyprland closes the connection.
pub fn events() -> Result<impl Iterator<Item = Event>, String> {
    let dir = std::env::var("XDG_RUNTIME_DIR").map_err(|_| "XDG_RUNTIME_DIR no está definido")?;
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").map_err(|_| "no hay una sesión de Hyprland")?;
    let stream = UnixStream::connect(format!("{dir}/hypr/{sig}/.socket2.sock"))
        .map_err(|e| format!("no pude conectar con los eventos de Hyprland: {e}"))?;
    Ok(BufReader::new(stream).lines().map_while(Result::ok).filter_map(|line| {
        let (name, data) = line.split_once(">>")?;
        Some(Event { name: name.to_string(), data: data.to_string() })
    }))
}
