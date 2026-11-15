//! `waylo-expose`: window overview (F3).
//!
//! It is an **on-demand** process: the shortcut launches it, it shows up, and once closed nothing
//! stays in memory. One card per window with keyboard navigation and click.

mod thumbs;

use std::time::{Duration, Instant};

use waylo_core::hypr::{self, Client};
use waylo_render::{Canvas, Fonts, Weight, rgba, theme};
use waylo_shell::{Button, Flow, Hits, Interaction, Key, Layout, Painter, PanelSpec};

/// Click zones of the cards are `CARD_BASE + index`.
const CARD_BASE: u32 = 100;
const PAD: f32 = 56.;
const GAP: f32 = 22.;
const LABEL_H: f32 = 46.;
const MAX_CARD_W: f32 = 380.;

/// Animation durations.
const OPEN: Duration = Duration::from_millis(170);
const SELECT: Duration = Duration::from_millis(120);
const FADE: Duration = Duration::from_millis(220);

/// Progress 0..=1 of an animation that started at `since`.
fn progress(since: Instant, total: Duration) -> f32 {
    (since.elapsed().as_secs_f32() / total.as_secs_f32()).clamp(0., 1.)
}

fn ease_out(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

struct Expose {
    fonts: Fonts,
    clients: Vec<Client>,
    selected: usize,
    /// Columns of the last grid painted (the up/down arrows need them).
    cols: usize,
    thumbs: thumbs::Shared,
    /// Instant of the first frame: starts the opening animation.
    opened: Option<Instant>,
    /// Card that had the selection before the last change, and when it changed.
    sel_from: Option<usize>,
    sel_at: Instant,
    started: Instant,
    reported: bool,
}

/// Trims `text` with "…" so it fits in `max` logical pixels.
fn fit(canvas: &Canvas, fonts: &Fonts, weight: Weight, text: &str, size: f32, max: f32) -> String {
    if canvas.text_width(fonts, weight, text, size) <= max {
        return text.to_string();
    }
    let mut out: String = text.to_string();
    while !out.is_empty() {
        out.pop();
        let cand = format!("{}…", out.trim_end());
        if canvas.text_width(fonts, weight, &cand, size) <= max {
            return cand;
        }
    }
    String::new()
}

/// Picks the number of columns that gives the biggest cards without leaving the screen.
fn grid(n: usize, w: f32, h: f32) -> (usize, f32, f32) {
    let mut best = (1, 0., 0.);
    for cols in 1..=n.max(1) {
        let rows = n.max(1).div_ceil(cols);
        let cw = ((w - 2. * PAD - (cols - 1) as f32 * GAP) / cols as f32).min(MAX_CARD_W);
        let ch = cw * 0.62 + LABEL_H;
        let total = rows as f32 * ch + (rows - 1) as f32 * GAP;
        if total <= h - 2. * PAD && cw >= best.1 {
            best = (cols, cw, ch);
        }
    }
    if best.1 == 0. {
        // Too many windows: shrink as much as needed with a single row of small cards.
        let cols = n.max(1);
        let cw = (w - 2. * PAD - (cols - 1) as f32 * GAP) / cols as f32;
        best = (cols, cw, cw * 0.62 + LABEL_H);
    }
    best
}

impl Expose {
    fn activate(&self, index: usize) -> Flow {
        if let Some(c) = self.clients.get(index) {
            hypr::focus_window(&c.address);
        }
        Flow::Quit
    }
}

impl Painter for Expose {
    fn paint(&mut self, _output: &str, canvas: &mut Canvas, hits: &mut Hits) {
        if !self.reported && std::env::var_os("WAYLO_DEBUG").is_some() {
            self.reported = true;
            eprintln!("waylo-expose: primer fotograma a los {:?}", self.started.elapsed());
        }
        let (w, h) = (canvas.width(), canvas.height());
        // The blur comes from Hyprland (layer rule); here just a dark veil.
        let opened = *self.opened.get_or_insert_with(Instant::now);
        let open = ease_out(progress(opened, OPEN));
        canvas.fill(rgba((0xa6 as f32 * open) as u32));
        let sel_q = ease_out(progress(self.sel_at, SELECT));

        if self.clients.is_empty() {
            let msg = "No hay ventanas abiertas";
            let tw = canvas.text_width(&self.fonts, Weight::Regular, msg, 18.);
            canvas.text(&self.fonts, Weight::Regular, msg, (w - tw) / 2., h / 2., 18., rgba(theme::TEXT_MUTED));
            return;
        }

        let n = self.clients.len();
        let (cols, cw, ch) = grid(n, w, h);
        self.cols = cols;
        let rows = n.div_ceil(cols);
        let total_h = rows as f32 * ch + (rows - 1) as f32 * GAP;
        let top = (h - total_h) / 2.;

        for (i, c) in self.clients.iter().enumerate() {
            let (row, col) = (i / cols, i % cols);
            // The last row is centered if it is not full.
            let in_row = if row == rows - 1 { n - row * cols } else { cols };
            let row_w = in_row as f32 * cw + (in_row - 1) as f32 * GAP;
            let x = (w - row_w) / 2. + (col as f32) * (cw + GAP);
            let y = top + row as f32 * (ch + GAP);
            hits.add(CARD_BASE + i as u32, x, y, cw, ch);

            // On open the cards grow from 92 %; the selected one grows a little and the
            // one that lost the selection returns to its size.
            let bump = if i == self.selected {
                0.035 * sel_q
            } else if self.sel_from == Some(i) {
                0.035 * (1. - sel_q)
            } else {
                0.
            };
            let s = (0.92 + 0.08 * open) * (1. + bump);
            let (x, y, cw, ch) = (x + cw * (1. - s) / 2., y + ch * (1. - s) / 2., cw * s, ch * s);

            let hovered = hits.hovered() == Some(CARD_BASE + i as u32);
            let selected = i == self.selected;
            let bg = if selected || hovered { 0x2a2a30f2 } else { 0x1c1c20e6 };
            canvas.fill_rrect(x, y, cw, ch, theme::RADIUS, rgba(bg));
            if selected {
                canvas.stroke_rrect(x, y, cw, ch, theme::RADIUS, rgba(theme::ACCENT));
                canvas.stroke_rrect(x + 1., y + 1., cw - 2., ch - 2., theme::RADIUS - 1., rgba(theme::ACCENT));
            } else {
                canvas.stroke_rrect(x, y, cw, ch, theme::RADIUS, rgba(theme::BORDER));
            }

            // Preview area (thumbnails arrive later): until then nothing is drawn.
            let (px, py, pw, ph) = (x + 8., y + 8., cw - 16., ch - LABEL_H - 8.);
            canvas.fill_rrect(px, py, pw, ph, theme::RADIUS - 4., rgba(0x0000004d));
            let thumbs = self.thumbs.lock().ok();
            if let Some((img, arrived)) = thumbs.as_ref().and_then(|t| t[i].as_ref()) {
                // Fit into the box without distortion and center.
                let k = (pw / img.width() as f32).min(ph / img.height() as f32);
                let (iw, ih) = (img.width() as f32 * k, img.height() as f32 * k);
                let (ix, iy) = (px + (pw - iw) / 2., py + (ph - ih) / 2.);
                canvas.draw_image(img, ix, iy, iw, ih, theme::RADIUS - 6.);
                // Fades in: a layer of the background color that gradually clears.
                let fade = 1. - ease_out(progress(*arrived, FADE));
                if fade > 0. {
                    canvas.fill_rrect(ix, iy, iw, ih, theme::RADIUS - 6., rgba(0x0e0e12_00 | (255. * fade) as u32));
                }
            }
            // No thumbnail yet: the dark box is left as it is, with no decoration.

            // Badge with the workspace number.
            let badge = c.workspace.id.to_string();
            let bw = canvas.text_width(&self.fonts, Weight::Bold, &badge, 12.) + 14.;
            canvas.fill_rrect(px + pw - bw - 6., py + 6., bw, 20., 10., rgba(theme::ACTIVE_BG));
            canvas.text(&self.fonts, Weight::Bold, &badge, px + pw - bw - 6. + 7., py + 20., 12., rgba(theme::ACTIVE_FG));

            // Title and class.
            let title = if c.title.is_empty() { &c.class } else { &c.title };
            let t = fit(canvas, &self.fonts, Weight::Bold, title, 13., cw - 24.);
            canvas.text(&self.fonts, Weight::Bold, &t, x + 12., y + ch - 22., 13., rgba(theme::TEXT));
            let k = fit(canvas, &self.fonts, Weight::Regular, &c.class, 11., cw - 24.);
            canvas.text(&self.fonts, Weight::Regular, &k, x + 12., y + ch - 8., 11., rgba(theme::TEXT_MUTED));
        }
    }

    fn interact(&mut self, _output: &str, action: Interaction) -> Flow {
        match action {
            Interaction::Click { id: Some(id), button: Button::Left } if id >= CARD_BASE => {
                self.activate((id - CARD_BASE) as usize)
            }
            // Click outside the cards: closes.
            Interaction::Click { id: None, button: Button::Left } => Flow::Quit,
            _ => Flow::Idle,
        }
    }

    fn key(&mut self, key: Key) -> Flow {
        let n = self.clients.len();
        if n == 0 {
            return if key == Key::Escape { Flow::Quit } else { Flow::Idle };
        }
        let cols = self.cols.max(1);
        let old = self.selected;
        match key {
            Key::Escape => return Flow::Quit,
            Key::Enter => return self.activate(self.selected),
            Key::Right | Key::Tab => self.selected = (self.selected + 1) % n,
            Key::Left | Key::BackTab => self.selected = (self.selected + n - 1) % n,
            Key::Down => {
                if self.selected + cols < n {
                    self.selected += cols;
                }
            }
            Key::Up => self.selected = self.selected.saturating_sub(cols).min(self.selected),
            Key::Char(c) if c.is_ascii_digit() => {
                // 1–9 jump to that card.
                if let Some(d) = c.to_digit(10).filter(|d| *d >= 1) {
                    if (d as usize) <= n {
                        self.selected = d as usize - 1;
                    }
                }
            }
            _ => {}
        }
        if self.selected != old {
            self.sel_from = Some(old);
            self.sel_at = Instant::now();
            Flow::Redraw
        } else {
            Flow::Idle
        }
    }

    fn animating(&self) -> bool {
        let Some(opened) = self.opened else { return true };
        if progress(opened, OPEN) < 1. || progress(self.sel_at, SELECT) < 1. {
            return true;
        }
        self.thumbs.lock().is_ok_and(|t| t.iter().flatten().any(|(_, at)| progress(*at, FADE) < 1.))
    }
}

fn main() {
    let started = Instant::now();
    let fonts = Fonts::system().unwrap_or_else(|e| {
        eprintln!("waylo-expose: {e}");
        std::process::exit(1);
    });
    let clients = hypr::clients().unwrap_or_else(|e| {
        eprintln!("waylo-expose: {e}");
        Vec::new()
    });
    // Start by selecting the window that has focus now (`focusHistoryID` 0 is approximated
    // with the first one for now).
    let monitors = hypr::monitors().unwrap_or_default();
    let focused = monitors.iter().find(|m| m.focused).map(|m| m.name.clone());
    let active_ws = monitors.iter().find(|m| m.focused).map(|m| m.active_workspace.id);
    let selected = active_ws.and_then(|ws| clients.iter().position(|c| c.workspace.id == ws)).unwrap_or(0);

    let spec = PanelSpec { namespace: "waylo-expose", layout: Layout::Fullscreen, only_monitor: focused };
    let shared = thumbs::shared(clients.len());
    let to_capture = clients.clone();
    let app = Expose { fonts, clients, selected, cols: 1, thumbs: shared.clone(), opened: None, sel_from: None, sel_at: started, started, reported: false };
    // Physical size of the thumbnail box according to the grid that is going to be painted.
    let (fit_w, fit_h) = match monitors.iter().find(|m| m.focused).or(monitors.first()) {
        Some(m) => {
            let (lw, lh) = (m.width as f32 / m.scale as f32, m.height as f32 / m.scale as f32);
            let (_, cw, ch) = grid(to_capture.len(), lw, lh);
            ((cw - 16.) * m.scale as f32, (ch - LABEL_H - 8.) * m.scale as f32)
        }
        None => (MAX_CARD_W, MAX_CARD_W * 0.62),
    };
    let start = move |waker| thumbs::start(&to_capture, selected, (fit_w, fit_h), &shared, &waker);
    if let Err(e) = waylo_shell::run_panels(spec, app, start) {
        eprintln!("waylo-expose: {e}");
        std::process::exit(1);
    }
}
