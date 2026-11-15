//! Screen positions: the laptop on one side of the external monitors.
//!
//! Only what differs from the current state is touched, so that reconnecting a monitor that
//! Hyprland already placed correctly does not cause mode changes (flicker).

use std::fs;

use serde::{Deserialize, Serialize};

use crate::hypr::{self, Monitor};
use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
}

/// Hyprland monitor rule: mode, position and scale.
#[derive(Debug, Clone, PartialEq)]
struct Rule {
    name: String,
    mode: String,
    x: i32,
    scale: f64,
}

impl Rule {
    fn for_monitor(m: &Monitor, x: i32) -> Self {
        Self { name: m.name.clone(), mode: format!("{}x{}@{:.2}", m.width, m.height, m.refresh_rate), x, scale: m.scale }
    }

    fn lua(&self) -> String {
        format!(
            "hl.monitor({{ output = \"{}\", mode = \"{}\", position = \"{}x0\", scale = \"{}\" }})",
            self.name, self.mode, self.x, self.scale
        )
    }
}

fn logical_width(m: &Monitor) -> i32 {
    (m.width as f64 / m.scale).round() as i32
}

/// Current side of the laptop relative to the external monitors (`None` if unknown).
pub fn current_side() -> Option<Side> {
    let all = hypr::monitors().ok()?;
    let laptop = all.iter().find(|m| m.is_laptop())?;
    let external = all.iter().find(|m| !m.is_laptop())?;
    Some(if laptop.x < external.x { Side::Left } else { Side::Right })
}

/// Far-away area where screens are parked while they are rearranged.
const PARK_X: i32 = 20_000;

/// Puts the screens with the laptop on side `side`. Returns `true` if anything had to change.
pub fn apply(side: Side) -> Result<bool, String> {
    let all = hypr::monitors()?;
    let (laptops, externals): (Vec<_>, Vec<_>) = all.iter().partition(|m| m.is_laptop());
    if laptops.is_empty() || externals.is_empty() {
        return Err("necesito al menos una laptop y un monitor externo".into());
    }
    let ordered: Vec<&Monitor> = match side {
        Side::Left => laptops.into_iter().chain(externals).collect(),
        Side::Right => externals.into_iter().chain(laptops).collect(),
    };

    // Final positions.
    let mut x = 0;
    let mut wanted = Vec::new();
    for m in &ordered {
        wanted.push((*m, Rule::for_monitor(m, x)));
        x += logical_width(m);
    }
    let wrong: Vec<&(&Monitor, Rule)> = wanted.iter().filter(|(m, r)| m.x != r.x || m.y != 0).collect();

    // The persistent file is kept up to date even if there is nothing to move.
    let text = persisted(&wanted.iter().map(|(_, r)| r.clone()).collect::<Vec<_>>());
    persist(&text)?;

    if wrong.is_empty() {
        return Ok(false);
    }
    // Hyprland warns if two screens overlap for an instant. With only one out of place it is enough
    // to move it; with several they are parked far away in a row before being placed.
    if wrong.len() > 1 {
        let mut park = PARK_X;
        for (m, _) in &wanted {
            run(&Rule::for_monitor(m, park))?;
            park += logical_width(m);
        }
        for (_, r) in &wanted {
            run(r)?;
        }
    } else {
        run(&wrong[0].1)?;
    }
    Ok(true)
}

fn run(rule: &Rule) -> Result<(), String> {
    hypr::hyprctl(&["eval", &rule.lua()]).map(|_| ()).map_err(|e| format!("hyprctl respondió: {e}"))
}

fn persisted(rules: &[Rule]) -> String {
    let mut text = String::from("-- Generado por Waylo (layout de pantallas). No editar a mano.\n");
    for r in rules {
        text.push_str(&r.lua());
        text.push('\n');
    }
    text
}

/// Writes the rules so the layout survives `hyprctl reload` and restarts (they are loaded by
/// `core/monitors.lua`). Does not rewrite the file if the content is unchanged.
fn persist(text: &str) -> Result<(), String> {
    let path = paths::legacy_file("monitors.lua").ok_or("HOME no está definido")?;
    // The first line (header) is ignored when comparing.
    let body = |t: &str| t.lines().skip(1).collect::<Vec<_>>().join("\n");
    if fs::read_to_string(&path).is_ok_and(|old| body(&old) == body(text)) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}
