//! `waylo-watch`: reapplies the display profile at startup and when a monitor is connected or removed.
//!
//! Only what differs from the current state is changed: if Hyprland placed the screen correctly, it is not
//! rearranged (every mode change is a flicker); layout and workspaces are applied
//! instantly and brightness (DDC, slow) afterwards and only if needed.

use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use waylo_core::{hypr, layout, profiles, workspaces};

/// Quiet time without new events before acting: merges bursts (one monitor generates several events).
const QUIET: Duration = Duration::from_millis(350);
/// Brightness attempts and pause between them: a DDC monitor takes a while to answer after being connected.
const BRIGHTNESS_TRIES: u32 = 6;
const BRIGHTNESS_PAUSE: Duration = Duration::from_millis(1500);

/// DDC access does not support two attempts at the same time.
static DDC: Mutex<()> = Mutex::new(());

fn log(started: Instant, msg: &str) {
    println!("[{:>6.2}s] {msg}", started.elapsed().as_secs_f32());
}

/// Layout and workspaces: immediate. Returns the profile used.
fn reconcile(profile: Option<&str>, started: Instant) -> Option<profiles::Profile> {
    let name = match profile.map(str::to_string).or_else(|| profiles::last().ok().flatten()) {
        Some(n) => n,
        None => {
            log(started, "aviso: no hay perfil que aplicar (guarda uno con `xdisplay profile save`)");
            return None;
        }
    };
    let profile = match profiles::get(&name) {
        Ok(p) => p,
        Err(e) => {
            log(started, &format!("error: {e}"));
            return None;
        }
    };
    match layout::apply(profile.side) {
        Ok(true) => log(started, "layout: pantallas recolocadas"),
        Ok(false) => log(started, "layout: ya estaba bien"),
        Err(e) => log(started, &format!("layout: {e}")),
    }
    apply_workspaces(started);
    let _ = profiles::mark_last(&name);
    Some(profile)
}

fn apply_workspaces(started: Instant) {
    match workspaces::apply() {
        Ok(changes) if changes.is_empty() => log(started, "workspaces: ya estaban bien"),
        Ok(changes) => changes.iter().for_each(|c| log(started, &format!("workspaces: {c}"))),
        Err(e) => log(started, &format!("workspaces: {e}")),
    }
}

/// Brightness in the background: retries while any monitor does not answer.
fn apply_brightness(profile: profiles::Profile, started: Instant) {
    thread::spawn(move || {
        let _guard = DDC.lock().unwrap_or_else(|e| e.into_inner());
        for attempt in 1..=BRIGHTNESS_TRIES {
            let pending = profiles::apply_brightness(&profile);
            if pending == 0 {
                log(started, &format!("brillo: listo (intento {attempt})"));
                return;
            }
            thread::sleep(BRIGHTNESS_PAUSE);
        }
        log(started, "brillo: algún monitor no respondió");
    });
}

fn main() {
    let started = Instant::now();
    let profile_arg = std::env::args().nth(1);
    let events = match hypr::events() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("waylo-watch: {e}");
            std::process::exit(1);
        }
    };
    log(started, "vigilando monitores...");

    // Events are read in one thread and sent to the main one, which merges them before acting.
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        for ev in events {
            // Hyprland emits every change twice (`monitoradded` and `monitoraddedv2`): only
            // version 2 is handled.
            if ev.name == "monitoraddedv2" || ev.name == "monitorremovedv2" {
                if tx.send(ev.name).is_err() {
                    return;
                }
            }
        }
    });

    // At login the monitor may already be connected: no event will announce it.
    if let Some(profile) = reconcile(profile_arg.as_deref(), started) {
        apply_brightness(profile, started);
    }

    while let Ok(first) = rx.recv() {
        let mut added = first == "monitoraddedv2";
        // Merge the events of the burst.
        while let Ok(next) = rx.recv_timeout(QUIET) {
            added |= next == "monitoraddedv2";
        }
        log(started, if added { "monitor conectado" } else { "monitor desconectado" });
        if added {
            if let Some(profile) = reconcile(profile_arg.as_deref(), started) {
                apply_brightness(profile, started);
            }
        } else {
            // Without the external one, the laptop gets workspaces 1-10; there is no layout to arrange.
            apply_workspaces(started);
        }
    }
}
