//! Data shown by the bar. Taken **once per repaint** (`Snapshot`) and each monitor picks
//! its own part from it (`View`). Spawns no processes: the system state is already in memory.

use std::collections::HashSet;

use waylo_core::hypr::{self, Monitor};
use waylo_core::sys::State;
use waylo_core::workspaces::{self, Config};

/// State shared by all monitors.
pub struct Snapshot {
    monitors: Vec<Monitor>,
    occupied: HashSet<u32>,
    config: Config,
    sys: State,
    time: String,
    date: String,
}

impl Snapshot {
    pub fn collect(sys: State) -> Self {
        let now = chrono::Local::now();
        Self {
            monitors: hypr::monitors().unwrap_or_default(),
            occupied: hypr::occupied(),
            config: workspaces::load(),
            sys,
            time: now.format("%H:%M").to_string(),
            date: now.format("%a %d %b").to_string(),
        }
    }

    /// What `monitor`'s bar must show.
    pub fn view(&self, monitor: &str) -> View<'_> {
        View {
            workspaces: workspaces::for_monitor(&self.config, &self.monitors, monitor),
            active: self.monitors.iter().find(|m| m.name == monitor).map_or(0, |m| m.active_workspace.id as u32),
            occupied: &self.occupied,
            laptop: hypr::is_laptop_output(monitor),
            sys: &self.sys,
            time: &self.time,
            date: &self.date,
        }
    }
}

pub struct View<'a> {
    /// Workspaces that belong to this monitor.
    pub workspaces: Vec<u32>,
    pub active: u32,
    pub occupied: &'a HashSet<u32>,
    /// Is this bar on the built-in screen? Decides which brightness it shows.
    pub laptop: bool,
    pub sys: &'a State,
    pub time: &'a str,
    pub date: &'a str,
}

impl View<'_> {
    /// Brightness of this bar's monitor.
    pub fn brightness(&self) -> Option<u8> {
        if self.laptop { self.sys.brightness.0 } else { self.sys.brightness.1 }
    }
}
