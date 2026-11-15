//! `layer-shell` surfaces for Waylo.
//!
//! This crate takes care of everything Wayland: registering globals, creating one
//! surface per monitor, reserving space and delivering the pixels through `wl_shm`. It does not know
//! *what* is drawn: whoever implements [`Painter`] decides that.
//!
//! [`run_panels`] creates one panel per monitor with the shape given by [`Layout`]: a strip
//! anchored at the top (the bar), a centered box, or a full-screen surface with exclusive keyboard
//! (the Exposé and other overlays).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use smithay_client_toolkit::{
    reexports::{
        calloop::{EventLoop, LoopHandle, channel},
        calloop_wayland_source::WaylandSource,
    },
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};
use waylo_render::Canvas;

/// Clickable zones of a panel. The program registers them while painting with [`Hits::add`] and the shell
/// uses them to know what the pointer is over.
#[derive(Default)]
pub struct Hits {
    regions: Vec<(f32, f32, f32, f32, u32)>,
    hover: Option<u32>,
}

impl Hits {
    /// Registers a zone (x, y, width, height) with an identifier chosen by the program.
    pub fn add(&mut self, id: u32, x: f32, y: f32, w: f32, h: f32) {
        self.regions.push((x, y, w, h, id));
    }

    /// Zone the pointer is over right now (according to the last event).
    pub fn hovered(&self) -> Option<u32> {
        self.hover
    }

    fn at(&self, px: f32, py: f32) -> Option<u32> {
        // The last one registered ends up on top.
        self.regions.iter().rev().find(|(x, y, w, h, _)| px >= *x && px < x + w && py >= *y && py < y + h).map(|r| r.4)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
    Other,
}

/// What the user did with the pointer on a panel. `id` is the [`Hits`] zone under the
/// pointer, or `None` if it was over a gap in the bar.
#[derive(Debug, Clone, Copy)]
pub enum Interaction {
    Click { id: Option<u32>, button: Button },
    /// `delta` > 0 means down; one wheel step is 1.
    Scroll { id: Option<u32>, delta: f32 },
}

/// Key pressed on a panel with keyboard (overlays only; the bar does not request the keyboard).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Escape,
    Enter,
    Tab,
    /// Shift + Tab.
    BackTab,
    Left,
    Right,
    Up,
    Down,
    Backspace,
    Delete,
    Home,
    End,
    Char(char),
}

/// What the shell must do after handling an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Nothing to do.
    Idle,
    /// Repaint all panels.
    Redraw,
    /// Close the panels and finish [`run_panels`].
    Quit,
}

/// Whoever draws the content of each panel.
pub trait Painter {
    /// Called once before repainting all the panels: the system is queried here, so the
    /// query is not repeated for each monitor.
    fn refresh(&mut self) {}

    /// Draws the panel of monitor `output` (e.g. `"HDMI-A-1"`) on `canvas` and registers in
    /// `hits` the zones that can be interacted with.
    fn paint(&mut self, output: &str, canvas: &mut Canvas, hits: &mut Hits);

    /// The user clicked or turned the wheel on the panel of `output`.
    fn interact(&mut self, _output: &str, _action: Interaction) -> Flow {
        Flow::Idle
    }

    /// The user pressed a key (only arrives if the panel requests the keyboard).
    fn key(&mut self, _key: Key) -> Flow {
        Flow::Idle
    }

    /// Queried after each `paint`: if it returns `true`, the shell asks the compositor for another
    /// frame and repaints (in sync with the display) until it returns `false`. This way an
    /// animation costs no CPU once it ends. The painter keeps its own clock.
    fn animating(&self) -> bool {
        false
    }
}

/// Shape of the surfaces.
#[derive(Debug, Clone)]
pub enum Layout {
    /// Strip anchored to the top edge, full width minus the margins.
    Strip {
        height: u32,
        /// Margin (top, right, bottom, left).
        margin: (i32, i32, i32, i32),
        /// Space that is reserved so windows do not cover it.
        exclusive_zone: i32,
    },
    /// The whole screen, above everything, with exclusive keyboard.
    Fullscreen,
    /// Fixed-size box centered on the monitor, with exclusive keyboard (Spotlight). Being
    /// small, repainting it is almost free (a full-screen overlay costs ~25 ms per frame).
    Centered { width: u32, height: u32 },
}

#[derive(Debug, Clone)]
pub struct PanelSpec {
    /// Name of the surface; Hyprland uses it for its layer rules.
    pub namespace: &'static str,
    pub layout: Layout,
    /// If `Some`, only that monitor's panel is created (e.g. the one that has focus).
    pub only_monitor: Option<String>,
}

struct Panel {
    layer: LayerSurface,
    output: wl_output::WlOutput,
    monitor: String,
    /// Physical pixels per logical unit (2 on HiDPI).
    scale: u32,
    /// Logical size assigned by the compositor (0 until the first `configure`).
    width: u32,
    height: u32,
    hits: Hits,
    /// Pointer position inside the panel, if it is over it.
    pointer: Option<(f32, f32)>,
    /// There is a frame callback pending (two are never requested at once).
    frame_pending: bool,
    /// Canvas reused between frames (avoids requesting and zeroing ~15 MB on each one).
    canvas: Option<Canvas>,
}

/// Requests a repaint from another thread. Requests that arrive together are merged into a single
/// repaint.
#[derive(Clone)]
pub struct Waker {
    tx: channel::Sender<()>,
    pending: Arc<AtomicBool>,
}

impl Waker {
    pub fn wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            let _ = self.tx.send(());
        }
    }
}

struct App {
    registry: RegistryState,
    output: OutputState,
    seat: SeatState,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    shift: bool,
    exit: bool,
    compositor: CompositorState,
    layer_shell: LayerShell,
    spec: PanelSpec,
    /// Forces the scale (`WAYLO_FORCE_SCALE` variable); only to test HiDPI drawing.
    force_scale: Option<u32>,
    shm: Shm,
    pool: SlotPool,
    painter: Box<dyn Painter>,
    panels: Vec<Panel>,
    pending: Arc<AtomicBool>,
    qh: QueueHandle<App>,
    loop_handle: LoopHandle<'static, App>,
}

impl App {
    /// Creates a monitor's panel if it does not have one yet (the name may arrive after
    /// `new_output`, which is why it is also called from `update_output`).
    fn create_panel(&mut self, qh: &QueueHandle<Self>, output: &wl_output::WlOutput) {
        if self.panels.iter().any(|p| p.output == *output) {
            return;
        }
        let Some(info) = self.output.info(output) else { return };
        let Some(monitor) = info.name.clone() else { return };
        if self.spec.only_monitor.as_ref().is_some_and(|only| *only != monitor) {
            return;
        }
        if std::env::var_os("WAYLO_DEBUG").is_some() {
            eprintln!("waylo-shell: panel nuevo para {monitor} (escala {})", info.scale_factor);
        }
        let scale = self.force_scale.unwrap_or(info.scale_factor.max(1) as u32);

        let surface = self.compositor.create_surface(qh);
        let level = if matches!(self.spec.layout, Layout::Fullscreen | Layout::Centered { .. }) { Layer::Overlay } else { Layer::Top };
        let layer = self.layer_shell.create_layer_surface(qh, surface, level, Some(self.spec.namespace), Some(output));
        let height = match &self.spec.layout {
            Layout::Strip { height, margin, exclusive_zone } => {
                layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
                layer.set_size(0, *height);
                layer.set_margin(margin.0, margin.1, margin.2, margin.3);
                layer.set_exclusive_zone(*exclusive_zone);
                layer.set_keyboard_interactivity(KeyboardInteractivity::None);
                *height
            }
            Layout::Centered { width, height } => {
                layer.set_anchor(Anchor::empty());
                layer.set_size(*width, *height);
                layer.set_exclusive_zone(-1);
                layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
                *height
            }
            Layout::Fullscreen => {
                layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
                layer.set_size(0, 0);
                // -1: covers the whole screen without letting other bars reserve space.
                layer.set_exclusive_zone(-1);
                layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
                0
            }
        };
        layer.commit();
        self.panels.push(Panel {
            layer,
            output: output.clone(),
            monitor,
            scale,
            width: 0,
            height,
            hits: Hits::default(),
            pointer: None,
            frame_pending: false,
            canvas: None,
        });
    }

    /// Translates a keyboard key and hands it to the painter (key repeat uses it too).
    fn handle_key(&mut self, event: &KeyEvent) {
        let key = match event.keysym {
            Keysym::Escape => Key::Escape,
            Keysym::Return | Keysym::KP_Enter => Key::Enter,
            Keysym::Tab | Keysym::ISO_Left_Tab => if self.shift { Key::BackTab } else { Key::Tab },
            Keysym::Left => Key::Left,
            Keysym::Right => Key::Right,
            Keysym::Up => Key::Up,
            Keysym::Down => Key::Down,
            Keysym::BackSpace => Key::Backspace,
            Keysym::Delete => Key::Delete,
            Keysym::Home => Key::Home,
            Keysym::End => Key::End,
            _ => match event.utf8.as_deref().and_then(|t| t.chars().next()) {
                Some(c) if !c.is_control() => Key::Char(c),
                _ => return,
            },
        };
        let flow = self.painter.key(key);
        self.apply(flow);
    }

    /// Repaints all the panels that are already configured.
    fn redraw_all(&mut self) {
        self.painter.refresh();
        for index in 0..self.panels.len() {
            let width = self.panels[index].width;
            if width > 0 {
                self.draw(index, width);
            }
        }
    }

    fn apply(&mut self, flow: Flow) {
        match flow {
            Flow::Idle => {}
            Flow::Redraw => self.redraw_all(),
            Flow::Quit => self.exit = true,
        }
    }

    /// Recomputes the zone under the pointer and repaints only if it changed.
    fn update_hover(&mut self, index: usize) {
        let panel = &mut self.panels[index];
        let hover = panel.pointer.and_then(|(x, y)| panel.hits.at(x, y));
        if hover != panel.hits.hover {
            panel.hits.hover = hover;
            let width = panel.width;
            if width > 0 {
                self.draw(index, width);
            }
        }
    }

    fn draw(&mut self, index: usize, width: u32) {
        let t0 = std::time::Instant::now();
        let height = self.panels[index].height;
        let scale = self.panels[index].scale;
        let reuse = self.panels[index].canvas.take().filter(|c| c.pixel_size() == (width * scale, height * scale));
        let Some(mut canvas) = reuse.or_else(|| Canvas::new(width, height, scale)) else { return };
        canvas.clear();
        let t_clear = t0.elapsed();
        let mut hits = std::mem::take(&mut self.panels[index].hits);
        hits.regions.clear();
        self.painter.paint(&self.panels[index].monitor, &mut canvas, &mut hits);
        self.panels[index].hits = hits;

        let t_paint = t0.elapsed();
        let (pw, ph) = canvas.pixel_size();
        let Ok((buffer, pixels)) =
            self.pool.create_buffer(pw as i32, ph as i32, pw as i32 * 4, wl_shm::Format::Argb8888)
        else {
            return;
        };
        let t_buf = t0.elapsed();
        canvas.write_argb8888(pixels);
        self.panels[index].canvas = Some(canvas);
        let animating = self.painter.animating();
        let surface = self.panels[index].layer.wl_surface();
        if animating && !self.panels[index].frame_pending {
            surface.frame(&self.qh, surface.clone());
            self.panels[index].frame_pending = true;
        }
        let surface = self.panels[index].layer.wl_surface();
        surface.set_buffer_scale(scale as i32);
        surface.damage_buffer(0, 0, pw as i32, ph as i32);
        if buffer.attach_to(surface).is_ok() {
            self.panels[index].layer.commit();
        }
        if std::env::var_os("WAYLO_TRACE").is_some() {
            eprintln!("waylo-shell: draw total {:?} (clear {t_clear:?}, paint {t_paint:?}, buffer {t_buf:?})", t0.elapsed());
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, factor: i32) {
        if self.force_scale.is_some() {
            return;
        }
        let Some(index) = self.panels.iter().position(|p| p.layer.wl_surface() == surface) else { return };
        let factor = factor.max(1) as u32;
        if self.panels[index].scale != factor {
            self.panels[index].scale = factor;
            let width = self.panels[index].width;
            if width > 0 {
                self.draw(index, width);
            }
        }
    }
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _: u32) {
        let Some(index) = self.panels.iter().position(|p| p.layer.wl_surface() == surface) else { return };
        self.panels[index].frame_pending = false;
        let width = self.panels[index].width;
        if self.painter.animating() && width > 0 {
            self.draw(index, width);
        }
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output
    }
    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.create_panel(qh, &output);
    }
    fn update_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        // The monitor name usually arrives here, after `new_output`.
        self.create_panel(qh, &output);
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        // Dropping the panel destroys its surface.
        if std::env::var_os("WAYLO_DEBUG").is_some() {
            eprintln!("waylo-shell: monitor desconectado");
        }
        self.panels.retain(|p| p.output != output);
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        // The compositor closes the surface when its monitor is disconnected: only that
        // panel is discarded and the program keeps running (the monitor may come back).
        if std::env::var_os("WAYLO_DEBUG").is_some() {
            eprintln!("waylo-shell: superficie cerrada por el compositor");
        }
        self.panels.retain(|p| p.layer != *layer);
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        let Some(index) = self.panels.iter().position(|p| p.layer == *layer) else { return };
        let width = configure.new_size.0.max(1);
        if configure.new_size.1 > 0 {
            self.panels[index].height = configure.new_size.1;
        }
        let first = self.panels[index].width == 0;
        self.panels[index].width = width;
        if first {
            self.painter.refresh();
        }
        self.draw(index, width);
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat.get_pointer(qh, &seat).ok();
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            let handle = self.loop_handle.clone();
            self.keyboard = self
                .seat
                .get_keyboard_with_repeat(
                    qh,
                    &seat,
                    None,
                    handle,
                    Box::new(|app: &mut App, _, event: KeyEvent| app.handle_key(&event)),
                )
                .ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
        if capability == Capability::Keyboard {
            if let Some(keyboard) = self.keyboard.take() {
                keyboard.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for event in events {
            let Some(index) = self.panels.iter().position(|p| p.layer.wl_surface() == &event.surface) else { continue };
            let position = (event.position.0 as f32, event.position.1 as f32);
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.panels[index].pointer = Some(position);
                    self.update_hover(index);
                }
                PointerEventKind::Leave { .. } => {
                    self.panels[index].pointer = None;
                    self.update_hover(index);
                }
                PointerEventKind::Press { button, .. } => {
                    let id = self.panels[index].hits.at(position.0, position.1);
                    let button = match button {
                        0x110 => Button::Left,
                        0x111 => Button::Right,
                        0x112 => Button::Middle,
                        _ => Button::Other,
                    };
                    let monitor = self.panels[index].monitor.clone();
                    let flow = self.painter.interact(&monitor, Interaction::Click { id, button });
                    self.apply(flow);
                }
                PointerEventKind::Axis { vertical, .. } => {
                    // Wheel: discrete steps; touchpad: continuous scrolling (≈10 px per step).
                    let delta = if vertical.discrete != 0 { vertical.discrete as f32 } else { vertical.absolute as f32 / 10. };
                    if delta != 0. {
                        let id = self.panels[index].hits.at(position.0, position.1);
                        let monitor = self.panels[index].monitor.clone();
                        let flow = self.painter.interact(&monitor, Interaction::Scroll { id, delta });
                        self.apply(flow);
                    }
                }
                PointerEventKind::Release { .. } => {}
            }
        }
    }
}

impl KeyboardHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}

    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.handle_key(&event);
    }

    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}

    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, modifiers: Modifiers, _: u32) {
        self.shift = modifiers.shift;
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState];
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_pointer!(App);
delegate_keyboard!(App);
delegate_layer!(App);
delegate_registry!(App);

/// Creates one panel per monitor and enters the event loop. Only returns on error.
///
/// `start` receives a [`Waker`] so the program can spawn its own threads (events, clock)
/// that request repaints.
pub fn run_panels(
    spec: PanelSpec,
    painter: impl Painter + 'static,
    start: impl FnOnce(Waker),
) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("sin sesión Wayland: {e}"))?;
    let (globals, mut queue) = registry_queue_init(&conn).map_err(|e| format!("registro Wayland: {e}"))?;
    let qh = queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|e| format!("layer-shell no disponible: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;
    let pool = SlotPool::new(1 << 20, &shm).map_err(|e| format!("pool de memoria: {e}"))?;

    let mut event_loop: EventLoop<'static, App> = EventLoop::try_new().map_err(|e| e.to_string())?;
    let mut app = App {
        registry: RegistryState::new(&globals),
        output: OutputState::new(&globals, &qh),
        seat: SeatState::new(&globals, &qh),
        pointer: None,
        keyboard: None,
        shift: false,
        exit: false,
        compositor,
        layer_shell,
        spec,
        force_scale: std::env::var("WAYLO_FORCE_SCALE").ok().and_then(|v| v.parse().ok()),
        shm,
        pool,
        painter: Box::new(painter),
        panels: Vec::new(),
        pending: Arc::new(AtomicBool::new(false)),
        qh: qh.clone(),
        loop_handle: event_loop.handle(),
    };
    // Monitors arrive through events and their panels are created in the handlers
    // (`new_output`/`update_output`), both the ones present at startup and those connected later.
    queue.roundtrip(&mut app).map_err(|e| e.to_string())?;
    queue.roundtrip(&mut app).map_err(|e| e.to_string())?;

    let handle = event_loop.handle();
    WaylandSource::new(conn, queue).insert(handle.clone()).map_err(|e| e.to_string())?;

    let (tx, rx) = channel::channel::<()>();
    handle
        .insert_source(rx, |event, _, app| {
            if let channel::Event::Msg(()) = event {
                // The flag is lowered first: an event that arrives during the repaint will request another one.
                app.pending.store(false, Ordering::Release);
                app.redraw_all();
            }
        })
        .map_err(|e| e.to_string())?;
    start(Waker { tx, pending: app.pending.clone() });

    let signal = event_loop.get_signal();
    event_loop
        .run(None, &mut app, |app| {
            if app.exit {
                signal.stop();
            }
        })
        .map_err(|e| e.to_string())
}
