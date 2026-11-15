//! Workspaces per monitor: each screen has its own range.
//!
//! Read from `~/.config/waylo/workspaces.toml` (`laptop = [6,7,..]`, `external = [1,2,..]`).
//! With only one monitor connected, that monitor gets all of them (1..=10).

use std::fs;

use serde::{Deserialize, Serialize};

use crate::hypr::Monitor;
use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Workspaces of the laptop when an external monitor is connected.
    pub laptop: Vec<u32>,
    /// Workspaces of the external monitor.
    pub external: Vec<u32>,
}

impl Default for Config {
    fn default() -> Self {
        Self { external: (1..=5).collect(), laptop: (6..=10).collect() }
    }
}

/// Reads the configuration; if it does not exist or is invalid, uses the defaults.
pub fn load() -> Config {
    // Waylo's config first; if it does not exist, `xdisplay`'s (compatibility).
    let path = paths::config_file("workspaces.toml").filter(|p| p.exists()).or_else(|| paths::legacy_file("workspaces.toml"));
    let Some(path) = path else { return Config::default() };
    match fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("{}: {e} (uso los valores por defecto)", path.display());
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

/// Workspaces that belong to `monitor` given the connected monitors.
pub fn for_monitor(config: &Config, monitors: &[Monitor], monitor: &str) -> Vec<u32> {
    let has_laptop = monitors.iter().any(Monitor::is_laptop);
    let has_external = monitors.iter().any(|m| !m.is_laptop());
    if !(has_laptop && has_external) {
        return (1..=10).collect();
    }
    if crate::hypr::is_laptop_output(monitor) { config.laptop.clone() } else { config.external.clone() }
}

/// Assignment (monitor, workspaces) for the connected monitors.
pub fn assignments(config: &Config, monitors: &[Monitor]) -> Vec<(String, Vec<u32>)> {
    // The external one first: it holds the low workspaces.
    let mut ordered: Vec<&Monitor> = monitors.iter().filter(|m| !m.is_laptop()).collect();
    ordered.extend(monitors.iter().filter(|m| m.is_laptop()));
    ordered.into_iter().map(|m| (m.name.clone(), for_monitor(config, monitors, &m.name))).collect()
}

fn table(assign: &[(String, Vec<u32>)]) -> String {
    let rows: Vec<String> = assign
        .iter()
        .map(|(m, list)| {
            let nums: Vec<String> = list.iter().map(u32::to_string).collect();
            format!("[\"{m}\"] = {{ {} }}", nums.join(", "))
        })
        .collect();
    // The name `XDISPLAY_WS` is used by the Super+N shortcuts in the Hyprland config.
    format!("XDISPLAY_WS = {{ {} }}", rows.join(", "))
}

fn rules(assign: &[(String, Vec<u32>)]) -> Vec<String> {
    let mut out = Vec::new();
    for (monitor, list) in assign {
        for (i, ws) in list.iter().enumerate() {
            let extra = if i == 0 { ", default = true, persistent = true" } else { "" };
            out.push(format!("hl.workspace_rule({{ workspace = \"{ws}\", monitor = \"{monitor}\"{extra} }})"));
        }
    }
    out
}

/// Lua loaded by `core/monitors.lua`: the table and the rules.
fn lua(assign: &[(String, Vec<u32>)]) -> String {
    let mut text = String::from("-- Generado por Waylo (workspaces por monitor). No editar a mano.\n");
    text.push_str(&table(assign));
    text.push('\n');
    for r in rules(assign) {
        text.push_str(&r);
        text.push('\n');
    }
    text
}

/// Leaves the workspaces as the config says for the connected monitors, touching only what
/// is needed: if the Lua file is already the same, Hyprland already has the rules (it loaded them at
/// startup or at the last apply) and none are resent; only the workspaces
/// on the wrong monitor are moved. Returns a summary of what changed (empty if nothing).
pub fn apply() -> Result<Vec<String>, String> {
    let config = load();
    let monitors = crate::hypr::monitors()?;
    let assign = assignments(&config, &monitors);
    if assign.is_empty() {
        return Err("no hay monitores".into());
    }
    let mut log = Vec::new();

    let path = paths::legacy_file("workspaces.lua").ok_or("HOME no está definido")?;
    let wanted = lua(&assign);
    let current = fs::read_to_string(&path).unwrap_or_default();
    // The header changed from "xdisplay" to "Waylo": ignored when comparing.
    let body = |t: &str| t.lines().skip(1).collect::<Vec<_>>().join("\n");
    if body(&current) != body(&wanted) {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        fs::write(&path, &wanted).map_err(|e| format!("{}: {e}", path.display()))?;
        crate::hypr::hyprctl(&["eval", &table(&assign)]).map_err(|e| format!("no pude actualizar XDISPLAY_WS: {e}"))?;
        for rule in rules(&assign) {
            crate::hypr::hyprctl(&["eval", &rule]).map_err(|e| format!("regla de workspace: {e}"))?;
        }
        for (m, l) in &assign {
            log.push(format!("{m}: workspaces {}", format_range(l)));
        }
    }

    // Moves only those that are on another monitor.
    for (id, current) in crate::hypr::workspace_monitors() {
        let Some((target, _)) = assign.iter().find(|(_, l)| l.contains(&(id as u32))) else { continue };
        if *target != current && id > 0 {
            let cmd = format!("hl.dsp.workspace.move({{ workspace = {id}, monitor = \"{target}\" }})");
            match crate::hypr::hyprctl(&["dispatch", &cmd]) {
                Ok(_) => log.push(format!("workspace {id}: {current} -> {target}")),
                Err(e) => log.push(format!("workspace {id}: no pude moverlo ({e})")),
            }
        }
    }
    Ok(log)
}

/// Compact text: [1,2,3,4,5] -> "1-5".
pub fn format_range(list: &[u32]) -> String {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < list.len() {
        let mut j = i;
        while j + 1 < list.len() && list[j + 1] == list[j] + 1 {
            j += 1;
        }
        parts.push(if j > i { format!("{}-{}", list[i], list[j]) } else { list[i].to_string() });
        i = j + 1;
    }
    parts.join(",")
}
