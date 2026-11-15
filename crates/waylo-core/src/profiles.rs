//! Display profiles: laptop side and brightness per screen.
//!
//! Stored in `~/.config/xdisplay/profiles.toml` (shared with `xdisplay`'s UI).

use std::collections::BTreeMap;
use std::fs;

use serde::{Deserialize, Serialize};

use crate::brightness;
use crate::layout::{self, Side};
use crate::paths;

/// A saved profile: laptop side and brightness per screen (by name).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub side: Side,
    #[serde(default)]
    pub brightness: BTreeMap<String, u8>,
}

#[derive(Default, Serialize, Deserialize)]
struct Store {
    /// Last profile saved or applied: the one that is reapplied when a monitor is connected.
    #[serde(default)]
    last: Option<String>,
    #[serde(default)]
    profiles: BTreeMap<String, Profile>,
}

fn load() -> Result<Store, String> {
    let path = paths::legacy_file("profiles.toml").ok_or("HOME no está definido")?;
    match fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn store(s: &Store) -> Result<(), String> {
    let path = paths::legacy_file("profiles.toml").ok_or("HOME no está definido")?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = toml::to_string_pretty(s).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Saves the current state (layout + brightness levels) under that name.
pub fn save(name: &str) -> Result<(), String> {
    let side = layout::current_side().ok_or("no pude detectar el lado de la laptop")?;
    let mut brightness = BTreeMap::new();
    for mut d in brightness::devices() {
        if let Ok(p) = d.get() {
            brightness.insert(d.name(), p);
        }
    }
    let mut s = load()?;
    s.profiles.insert(name.to_string(), Profile { side, brightness });
    s.last = Some(name.to_string());
    store(&s)
}

/// Name of the last profile used (if it still exists).
pub fn last() -> Result<Option<String>, String> {
    let s = load()?;
    Ok(s.last.filter(|n| s.profiles.contains_key(n)))
}

pub fn get(name: &str) -> Result<Profile, String> {
    load()?.profiles.get(name).cloned().ok_or_else(|| format!("no existe el perfil '{name}'"))
}

pub fn list() -> Result<Vec<(String, Profile)>, String> {
    Ok(load()?.profiles.into_iter().collect())
}

pub fn delete(name: &str) -> Result<(), String> {
    let mut s = load()?;
    s.profiles.remove(name).ok_or_else(|| format!("no existe el perfil '{name}'"))?;
    if s.last.as_deref() == Some(name) {
        s.last = None;
    }
    store(&s)
}

/// Marks `name` as the last profile used.
pub fn mark_last(name: &str) -> Result<(), String> {
    let mut s = load()?;
    s.last = Some(name.to_string());
    store(&s)
}

/// Sets each screen's brightness to the profile value, only where it differs. Returns how many
/// screens still do not have the requested value (e.g. a DDC monitor that does not answer yet).
pub fn apply_brightness(profile: &Profile) -> usize {
    let mut pending = 0;
    for mut d in brightness::devices() {
        let Some(&target) = profile.brightness.get(&d.name()) else { continue };
        match d.get() {
            Ok(now) if now == target => {}
            _ => {
                if d.set(target).is_err() {
                    pending += 1;
                }
            }
        }
    }
    pending
}
