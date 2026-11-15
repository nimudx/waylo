//! `waylo-bar`: top bar, one per monitor.
//!
//! This binary only wires the pieces together: data (`view`, from `waylo-core`), drawing (`draw` and
//! `widgets`, with `waylo-render`) and windows (`waylo-shell`). It repaints on Hyprland events and
//! on system changes (volume, network, brightness...), never on a fast poll.

mod draw;
mod view;
mod widgets;

use std::sync::{Arc, OnceLock};

use waylo_core::sys::{Sys, volume};
use waylo_core::{exec, hypr};
use waylo_render::{Canvas, Fonts};
use waylo_shell::{Button, Flow, Hits, Interaction, Layout, Painter, PanelSpec, Waker};

use view::Snapshot;
use widgets::id;

const HEIGHT: u32 = 34;
const GAP_TOP: i32 = 6;
const GAP_SIDE: i32 = 10;

/// Hyprland events that change what the bar shows.
const RELEVANT: &[&str] = &[
    "workspace",
    "workspacev2",
    "focusedmon",
    "focusedmonv2",
    "createworkspace",
    "createworkspacev2",
    "destroyworkspace",
    "destroyworkspacev2",
    "moveworkspace",
    "moveworkspacev2",
    "openwindow",
    "closewindow",
    "movewindow",
    "movewindowv2",
    "monitoradded",
    "monitoraddedv2",
    "monitorremoved",
    "monitorremovedv2",
];

struct Bar {
    fonts: Fonts,
    /// Filled in when the loop starts (it needs the `Waker`).
    sys: Arc<OnceLock<Sys>>,
    snapshot: Snapshot,
}

impl Bar {
    fn sys_state(&self) -> waylo_core::sys::State {
        self.sys.get().map(Sys::state).unwrap_or_default()
    }
}

impl Painter for Bar {
    fn refresh(&mut self) {
        self.snapshot = Snapshot::collect(self.sys_state());
    }

    fn paint(&mut self, output: &str, canvas: &mut Canvas, hits: &mut Hits) {
        draw::bar(canvas, &self.fonts, &self.snapshot.view(output), hits);
    }

    fn interact(&mut self, output: &str, action: Interaction) -> Flow {
        if std::env::var_os("WAYLO_DEBUG").is_some() {
            eprintln!("waylo-bar: {output}: {action:?}");
        }
        match action {
            Interaction::Click { id: Some(ws), button: Button::Left } if ws < 100 => {
                hypr::focus_monitor(output);
                hypr::focus_workspace(ws);
            }
            Interaction::Click { id: Some(id::NETWORK), button: Button::Left } => exec::spawn("kitty -e nmtui"),
            Interaction::Click { id: Some(id::VOLUME), button: Button::Left } => volume::toggle_mute(),
            Interaction::Click { id: Some(id::VOLUME), button: Button::Right } => exec::spawn("pavucontrol"),
            Interaction::Click { id: Some(id::BLUETOOTH), button: Button::Left } => exec::spawn("blueman-manager"),
            // Wheel over a widget: adjusts it. Delta > 0 means down.
            Interaction::Scroll { id: Some(id::VOLUME), delta } => volume::step(if delta > 0. { -1 } else { 1 }),
            Interaction::Scroll { id: Some(id::BRIGHTNESS), delta } => {
                if let Some(sys) = self.sys.get() {
                    sys.adjust_brightness(hypr::is_laptop_output(output), if delta > 0. { -5 } else { 5 });
                }
            }
            // Wheel anywhere else: cycles through this monitor's workspaces.
            Interaction::Scroll { delta, .. } => {
                let view = self.snapshot.view(output);
                let Some(pos) = view.workspaces.iter().position(|&w| w == view.active) else { return Flow::Idle };
                let next = if delta > 0. { pos + 1 } else { pos.wrapping_sub(1) };
                if let Some(&ws) = view.workspaces.get(next) {
                    hypr::focus_monitor(output);
                    hypr::focus_workspace(ws);
                }
            }
            _ => {}
        }
        Flow::Idle
    }
}

fn main() {
    let fonts = Fonts::system().unwrap_or_else(|e| {
        eprintln!("waylo-bar: {e}");
        std::process::exit(1);
    });
    let sys = Arc::new(OnceLock::new());
    let spec = PanelSpec {
        namespace: "waylo-bar",
        layout: Layout::Strip {
            height: HEIGHT,
            margin: (GAP_TOP, GAP_SIDE, 0, GAP_SIDE),
            exclusive_zone: HEIGHT as i32 + GAP_TOP,
        },
        only_monitor: None,
    };
    let bar = Bar { fonts, sys: sys.clone(), snapshot: Snapshot::collect(Default::default()) };

    // Repaint sources: Hyprland events and system state.
    let start = move |waker: Waker| {
        let on_hypr = waker.clone();
        let sys_for_events = sys.clone();
        std::thread::spawn(move || {
            let Ok(events) = hypr::events() else {
                eprintln!("waylo-bar: sin eventos de Hyprland; los workspaces no se actualizarán solos");
                return;
            };
            for event in events {
                if RELEVANT.contains(&event.name.as_str()) {
                    on_hypr.wake();
                }
                // When a monitor is added or removed the devices with brightness control change (only the
                // version 2 event is handled; Hyprland emits both).
                if event.name == "monitoraddedv2" || event.name == "monitorremovedv2" {
                    if let Some(sys) = sys_for_events.get() {
                        sys.rescan_brightness();
                    }
                }
            }
        });
        let _ = sys.set(Sys::start(move || waker.wake()));
    };

    if let Err(e) = waylo_shell::run_panels(spec, bar, start) {
        eprintln!("waylo-bar: {e}");
        std::process::exit(1);
    }
}
