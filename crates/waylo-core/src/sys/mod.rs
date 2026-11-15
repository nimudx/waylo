//! System state shown by the bars: volume, battery, network, bluetooth and brightness.
//!
//! [`Sys::start`] spawns the threads that watch each source. Every change updates a shared
//! state and notifies the program (`wake`), which repaints. Reading the state ([`Sys::state`]) is
//! just copying data in memory: it spawns no processes, so it can be called on every repaint.

pub mod battery;
pub mod bluetooth;
pub mod dbus;
pub mod network;
pub mod volume;

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::brightness::{self, Brightness};

#[derive(Clone, Default)]
pub struct State {
    pub volume: Option<volume::Volume>,
    pub battery: Option<battery::Battery>,
    pub network: Option<network::Net>,
    pub bluetooth: bool,
    /// Brightness (laptop, external monitor), as a percentage.
    pub brightness: (Option<u8>, Option<u8>),
}

enum BrightCmd {
    Adjust { laptop: bool, delta: i32 },
    /// Detect the monitors with brightness control again (one was connected or removed).
    Rescan,
}

pub struct Sys {
    state: Arc<Mutex<State>>,
    bright: Sender<BrightCmd>,
}

type Wake = Arc<dyn Fn() + Send + Sync>;

/// Applies a change to the state and notifies only if something really changed.
fn update(state: &Mutex<State>, wake: &Wake, change: impl FnOnce(&mut State) -> bool) {
    let changed = change(&mut state.lock().unwrap());
    if changed {
        wake();
    }
}

fn set<T: PartialEq>(slot: &mut T, value: T) -> bool {
    if *slot == value {
        false
    } else {
        *slot = value;
        true
    }
}

impl Sys {
    /// Spawns the watcher threads. `wake` is called (from another thread) when something changes.
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let wake: Wake = Arc::new(wake);
        let state = Arc::new(Mutex::new(State::default()));

        // Volume: PipeWire events.
        {
            let (state, wake) = (state.clone(), wake.clone());
            std::thread::spawn(move || {
                let refresh = || update(&state, &wake, |s| set(&mut s.volume, volume::read()));
                refresh();
                volume::watch(refresh);
            });
        }
        // Network: NetworkManager events. The wifi signal changes without events: the poll refreshes it.
        {
            let (state, wake) = (state.clone(), wake.clone());
            std::thread::spawn(move || {
                let refresh = || update(&state, &wake, |s| set(&mut s.network, network::read()));
                refresh();
                network::watch(|| {
                    // A connection emits several lines in a row: wait for it to settle.
                    std::thread::sleep(Duration::from_millis(600));
                    refresh();
                });
            });
        }
        // Bluetooth and power: D-Bus signals, to reflect changes instantly.
        {
            let (state, wake) = (state.clone(), wake.clone());
            std::thread::spawn(move || {
                dbus::watch(|| {
                    update(&state, &wake, |s| {
                        let a = set(&mut s.battery, battery::read());
                        let b = set(&mut s.bluetooth, bluetooth::is_on());
                        a | b
                    });
                });
            });
        }
        // Slow poll, once a minute: battery, bluetooth and wifi signal. It also wakes
        // the bar so the clock advances.
        {
            let (state, wake) = (state.clone(), wake.clone());
            std::thread::spawn(move || {
                loop {
                    update(&state, &wake, |s| {
                        let a = set(&mut s.battery, battery::read());
                        let b = set(&mut s.bluetooth, bluetooth::is_on());
                        let c = set(&mut s.network, network::read());
                        a | b | c
                    });
                    let secs = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs() % 60);
                    wake(); // the clock advances every minute even if nothing else changes
                    std::thread::sleep(Duration::from_millis((60 - secs) * 1000 + 50));
                }
            });
        }
        // Brightness: one thread that owns the devices (DDC is slow). Wheel commands
        // are grouped and applied at once; it also rereads every 60 s.
        let (bright, rx) = mpsc::channel::<BrightCmd>();
        {
            let (state, wake) = (state.clone(), wake.clone());
            std::thread::spawn(move || brightness_worker(&state, &wake, rx));
        }
        Sys { state, bright }
    }

    /// Copy of the current state (spawns no processes).
    pub fn state(&self) -> State {
        self.state.lock().unwrap().clone()
    }

    /// Changes the brightness of the laptop monitor (`laptop = true`) or the external one, in
    /// percentage points.
    /// Detects the monitors with brightness control again (when one is connected or removed).
    pub fn rescan_brightness(&self) {
        let _ = self.bright.send(BrightCmd::Rescan);
    }

    pub fn adjust_brightness(&self, laptop: bool, delta: i32) {
        let _ = self.bright.send(BrightCmd::Adjust { laptop, delta });
    }
}

fn brightness_worker(state: &Mutex<State>, wake: &Wake, rx: mpsc::Receiver<BrightCmd>) {
    let mut devices = brightness::devices();
    let read = |devices: &mut Vec<Box<dyn Brightness>>| {
        let (mut laptop, mut external) = (None, None);
        for d in devices.iter_mut() {
            let value = d.get().ok();
            if d.name() == "laptop" {
                laptop = value;
            } else if external.is_none() {
                external = value;
            }
        }
        (laptop, external)
    };
    update(state, wake, |s| set(&mut s.brightness, read(&mut devices)));
    loop {
        let mut delta = (0i32, 0i32); // (laptop, external)
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(BrightCmd::Adjust { laptop, delta: d }) => {
                if laptop { delta.0 += d } else { delta.1 += d }
                std::thread::sleep(Duration::from_millis(80));
                while let Ok(BrightCmd::Adjust { laptop, delta: d }) = rx.try_recv() {
                    if laptop { delta.0 += d } else { delta.1 += d }
                }
            }
            Ok(BrightCmd::Rescan) => {
                // A DDC monitor takes a while to answer after being connected: wait, detect and, if
                // the external one does not show up yet, retry once more.
                std::thread::sleep(Duration::from_millis(1500));
                while rx.try_recv().is_ok() {}
                devices = brightness::devices();
                let mut now = read(&mut devices);
                if now.1.is_none() && devices.len() > 1 {
                    std::thread::sleep(Duration::from_millis(3500));
                    devices = brightness::devices();
                    now = read(&mut devices);
                }
                update(state, wake, |s| set(&mut s.brightness, now));
                continue;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        for d in devices.iter_mut() {
            let is_laptop = d.name() == "laptop";
            let change = if is_laptop { delta.0 } else { delta.1 };
            if change == 0 {
                continue;
            }
            if let Ok(current) = d.get() {
                let min = if is_laptop { 1 } else { 0 };
                let _ = d.set((current as i32 + change).clamp(min, 100) as u8);
            }
        }
        update(state, wake, |s| set(&mut s.brightness, read(&mut devices)));
    }
}
