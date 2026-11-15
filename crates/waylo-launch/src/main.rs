//! `waylo-launch`: application launcher.
//!
//! **On-demand** process: it opens, searches, launches the application and exits. Three forms:
//! - `waylo-launch`       compact icon grid, centered (Super+Space)
//! - `waylo-launch grid`  full-screen grid (F4)
//! - `waylo-launch list`  list with a search box

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use waylo_core::apps::{self, App};
use waylo_render::{Canvas, Color, Fonts, Image, Weight, rgba, theme};
use waylo_shell::{Button, Flow, Hits, Interaction, Key, Layout, Painter, PanelSpec};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Compact grid, centered.
    Compact,
    /// Full-screen grid.
    Full,
    /// List with a search box.
    List,
}

const PAD: f32 = 10.;
const HEADER_H: f32 = 68.;
/// Click zones of the apps are `CELL_BASE + visible position`.
const CELL_BASE: u32 = 100;

// List
const LIST_W: u32 = 680;
const LIST_ROWS: usize = 7;
const LIST_ROW_H: f32 = 48.;
const LIST_ICON: f32 = 30.;

// Compact grid (Super+Space)
const COMPACT_COLS: usize = 6;
const COMPACT_ROWS: usize = 3;
const COMPACT_CELL: (f32, f32) = (108., 98.);
const COMPACT_ICON: f32 = 48.;
const GAP: f32 = 8.;

// Full grid (F4)
const FULL_CELL: (f32, f32) = (150., 132.);
const FULL_ICON: f32 = 76.;

/// Size of the compact surface.
fn compact_size() -> (u32, u32) {
    let grid_w = COMPACT_COLS as f32 * COMPACT_CELL.0 + (COMPACT_COLS - 1) as f32 * GAP;
    let grid_h = COMPACT_ROWS as f32 * COMPACT_CELL.1 + (COMPACT_ROWS - 1) as f32 * GAP;
    ((grid_w + 2. * (PAD + 8.)) as u32, (HEADER_H + grid_h + 2. * PAD + 6.) as u32)
}

/// Where and how big the grid cells are.
struct Geo {
    cols: usize,
    rows: usize,
    cell: (f32, f32),
    icon: f32,
    x0: f32,
    y0: f32,
}

fn geo(mode: Mode, w: f32, h: f32) -> Geo {
    match mode {
        Mode::Full => {
            // Columns that fit in 74 % of the width; rows in 72 % of the height.
            let (cw, ch) = FULL_CELL;
            let cols = (((w * 0.74 + GAP) / (cw + GAP)).floor() as usize).max(1);
            let rows = (((h * 0.72 + GAP) / (ch + GAP)).floor() as usize).max(1);
            let grid_w = cols as f32 * cw + (cols - 1) as f32 * GAP;
            Geo { cols, rows, cell: FULL_CELL, icon: FULL_ICON, x0: (w - grid_w) / 2., y0: (h * 0.07).max(24.) + 46. + 36. }
        }
        _ => {
            let grid_w = COMPACT_COLS as f32 * COMPACT_CELL.0 + (COMPACT_COLS - 1) as f32 * GAP;
            Geo {
                cols: COMPACT_COLS,
                rows: COMPACT_ROWS,
                cell: COMPACT_CELL,
                icon: COMPACT_ICON,
                x0: (w - grid_w) / 2.,
                y0: HEADER_H + PAD,
            }
        }
    }
}

fn hue_color(name: &str) -> Color {
    let hue = name.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)) % 360;
    // HSL (s = 0.45, l = 0.42) to RGB.
    let (s, l) = (0.45f32, 0.42f32);
    let c = (1. - (2. * l - 1.).abs()) * s;
    let h = hue as f32 / 60.;
    let x = c * (1. - (h % 2. - 1.).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    let m = l - c / 2.;
    Color::from_rgba(r + m, g + m, b + m, 1.).unwrap_or(Color::BLACK)
}

struct Launcher {
    mode: Mode,
    fonts: Fonts,
    apps: Vec<App>,
    usage: BTreeMap<String, u32>,
    query: String,
    /// Cursor position, in characters.
    cursor: usize,
    /// Indices into `apps` that match, already sorted.
    results: Vec<usize>,
    selected: usize,
    /// First visible row (of the list, or of cells in the grid).
    top: usize,
    /// Visible columns and rows of the last grid painted (the arrows need them).
    cols: usize,
    rows: usize,
    /// Icons already loaded (index in `apps` -> image, or `None` if there is none).
    icons: HashMap<usize, Option<Image>>,
    started: Instant,
    reported: bool,
}

impl Launcher {
    fn new(mode: Mode, fonts: Fonts, apps: Vec<App>, usage: BTreeMap<String, u32>, started: Instant) -> Self {
        let (cols, rows) = match mode {
            Mode::List => (1, LIST_ROWS),
            _ => (COMPACT_COLS, COMPACT_ROWS),
        };
        let mut l = Self {
            mode,
            fonts,
            apps,
            usage,
            query: String::new(),
            cursor: 0,
            results: Vec::new(),
            selected: 0,
            top: 0,
            cols,
            rows,
            icons: HashMap::new(),
            started,
            reported: false,
        };
        l.search();
        l
    }

    /// Recomputes the results: best match first, and among equal matches the most used first.
    fn search(&mut self) {
        let q = self.query.trim().to_lowercase();
        let mut found: Vec<(u8, u32, usize)> = self
            .apps
            .iter()
            .enumerate()
            .filter_map(|(i, a)| a.rank(&q).map(|r| (r, self.usage.get(&a.id).copied().unwrap_or(0), i)))
            .collect();
        found.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(b.1.cmp(&a.1))
                .then_with(|| self.apps[a.2].name.to_lowercase().cmp(&self.apps[b.2].name.to_lowercase()))
        });
        self.results = found.into_iter().map(|(_, _, i)| i).collect();
        self.selected = 0;
        self.top = 0;
    }

    /// Moves the selection `delta` positions (without wrapping if `wrap` is false) and scrolls the
    /// view so it stays visible.
    fn move_selection(&mut self, delta: isize, wrap: bool) {
        let n = self.results.len() as isize;
        if n == 0 {
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = if wrap { next.rem_euclid(n) } else { next.clamp(0, n - 1) } as usize;
        let row = self.selected / self.cols.max(1);
        if row < self.top {
            self.top = row;
        } else if row >= self.top + self.rows {
            self.top = row + 1 - self.rows;
        }
    }

    fn launch(&self, result: usize) -> Flow {
        if let Some(&i) = self.results.get(result) {
            if let Err(e) = apps::launch(&self.apps[i]) {
                eprintln!("waylo-launch: {e}");
            }
        }
        Flow::Quit
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.query.char_indices().nth(chars).map_or(self.query.len(), |(b, _)| b)
    }

    /// Loads the icon of `apps[i]` the first time it is needed.
    fn ensure_icon(&mut self, i: usize, px: u32) {
        let app = &self.apps[i];
        self.icons.entry(i).or_insert_with(|| app.icon_path().and_then(|p| Image::from_file(&p, px)));
    }

    /// Draws the icon of `apps[i]` (or its initial on a colored square) at `(x, y, size, size)`.
    fn draw_icon(&mut self, canvas: &mut Canvas, i: usize, x: f32, y: f32, size: f32) {
        self.ensure_icon(i, (size * canvas.scale()).round() as u32);
        match self.icons.get(&i).and_then(|o| o.as_ref()) {
            Some(img) => canvas.draw_image(img, x, y, size, size, 0.),
            None => {
                let name = &self.apps[i].name;
                canvas.fill_rrect(x + size * 0.06, y + size * 0.06, size * 0.88, size * 0.88, size * 0.24, hue_color(name));
                let initial: String = name.chars().next().unwrap_or('?').to_uppercase().collect();
                let fs = size * 0.46;
                let iw = canvas.text_width(&self.fonts, Weight::Bold, &initial, fs);
                canvas.text(&self.fonts, Weight::Bold, &initial, x + (size - iw) / 2., y + size / 2. + fs * 0.34, fs, rgba(theme::TEXT));
            }
        }
    }

    /// Typed text and cursor, or the help text if empty.
    fn draw_query(&self, canvas: &mut Canvas, x: f32, baseline: f32, size: f32, cursor_h: f32) {
        if self.query.is_empty() {
            canvas.text(&self.fonts, Weight::Regular, "Buscar aplicaciones…", x, baseline, size, rgba(theme::TEXT_FAINT));
        } else {
            canvas.text(&self.fonts, Weight::Regular, &self.query, x, baseline, size, rgba(theme::TEXT));
        }
        let before: String = self.query.chars().take(self.cursor).collect();
        let cx = x + canvas.text_width(&self.fonts, Weight::Regular, &before, size);
        canvas.fill_rrect(cx + 1., baseline - cursor_h * 0.78, 2., cursor_h, 1., rgba(theme::ACCENT));
    }

    fn paint_list(&mut self, canvas: &mut Canvas, hits: &mut Hits) {
        let (w, h) = (canvas.width(), canvas.height());
        canvas.fill_rrect(0., 0., w, h, theme::RADIUS + 4., rgba(0x1a1a1ef2));
        canvas.stroke_rrect(0., 0., w, h, theme::RADIUS + 4., rgba(theme::BORDER));
        let tx = PAD + 14.;
        self.draw_query(canvas, tx, 42., 20., 28.);
        canvas.fill_rrect(PAD, HEADER_H - 4., w - 2. * PAD, 1., 0., rgba(theme::BORDER));
        if self.results.is_empty() {
            canvas.text(&self.fonts, Weight::Regular, "Sin resultados", tx, HEADER_H + 34., 15., rgba(theme::TEXT_MUTED));
            return;
        }
        let visible: Vec<usize> = self.results.iter().copied().skip(self.top).take(LIST_ROWS).collect();
        for (slot, i) in visible.into_iter().enumerate() {
            let result = self.top + slot;
            let y = HEADER_H + PAD + slot as f32 * LIST_ROW_H;
            let (x, rw) = (PAD, w - 2. * PAD);
            hits.add(CELL_BASE + slot as u32, x, y, rw, LIST_ROW_H);
            let selected = result == self.selected;
            if selected {
                canvas.fill_rrect(x, y, rw, LIST_ROW_H - 4., 10., rgba(0x6cb6ff33));
            } else if hits.hovered() == Some(CELL_BASE + slot as u32) {
                canvas.fill_rrect(x, y, rw, LIST_ROW_H - 4., 10., rgba(theme::HOVER_BG));
            }
            self.draw_icon(canvas, i, x + 12., y + (LIST_ROW_H - 4. - LIST_ICON) / 2., LIST_ICON);
            let weight = if selected { Weight::Bold } else { Weight::Regular };
            let name = canvas.ellipsize(&self.fonts, weight, &self.apps[i].name, 15., rw - 120.);
            canvas.text(&self.fonts, weight, &name, x + 12. + LIST_ICON + 14., y + 26., 15., rgba(theme::TEXT));
            if selected {
                canvas.text(&self.fonts, Weight::Regular, "Enter", x + rw - 52., y + 26., 12., rgba(theme::TEXT_MUTED));
            }
        }
    }

    fn paint_grid(&mut self, canvas: &mut Canvas, hits: &mut Hits) {
        let (w, h) = (canvas.width(), canvas.height());
        let g = geo(self.mode, w, h);
        self.cols = g.cols;
        self.rows = g.rows;
        // If the size changed, the view must not end up out of range.
        let total_rows = self.results.len().div_ceil(g.cols).max(1);
        self.top = self.top.min(total_rows.saturating_sub(g.rows));

        if self.mode == Mode::Full {
            canvas.fill(rgba(0x0000007a));
            // Search box as a pill, top center.
            let (pw, ph) = (440., 46.);
            let (px, py) = ((w - pw) / 2., (h * 0.07).max(24.));
            canvas.fill_rrect(px, py, pw, ph, ph / 2., rgba(0xffffff1c));
            canvas.stroke_rrect(px, py, pw, ph, ph / 2., rgba(0xffffff26));
            self.draw_query(canvas, px + 22., py + 30., 16., 20.);
        } else {
            canvas.fill_rrect(0., 0., w, h, theme::RADIUS + 4., rgba(0x1a1a1ef2));
            canvas.stroke_rrect(0., 0., w, h, theme::RADIUS + 4., rgba(theme::BORDER));
            self.draw_query(canvas, PAD + 14., 42., 20., 28.);
            canvas.fill_rrect(PAD, HEADER_H - 4., w - 2. * PAD, 1., 0., rgba(theme::BORDER));
        }

        if self.results.is_empty() {
            let msg = "Sin resultados";
            let mw = canvas.text_width(&self.fonts, Weight::Regular, msg, 15.);
            canvas.text(&self.fonts, Weight::Regular, msg, (w - mw) / 2., g.y0 + 50., 15., rgba(theme::TEXT_MUTED));
            return;
        }

        let (cw, ch) = g.cell;
        let first = self.top * g.cols;
        let visible: Vec<usize> = self.results.iter().copied().skip(first).take(g.rows * g.cols).collect();
        for (slot, i) in visible.into_iter().enumerate() {
            let (row, col) = (slot / g.cols, slot % g.cols);
            let x = g.x0 + col as f32 * (cw + GAP);
            let y = g.y0 + row as f32 * (ch + GAP);
            let id = CELL_BASE + slot as u32;
            hits.add(id, x, y, cw, ch);
            if first + slot == self.selected {
                canvas.fill_rrect(x, y, cw, ch, 16., rgba(0xffffff26));
            } else if hits.hovered() == Some(id) {
                canvas.fill_rrect(x, y, cw, ch, 16., rgba(0xffffff14));
            }
            let icon_y = y + (ch - g.icon - 24.) / 2.;
            self.draw_icon(canvas, i, x + (cw - g.icon) / 2., icon_y, g.icon);
            let name = canvas.ellipsize(&self.fonts, Weight::Regular, &self.apps[i].name, 12.5, cw - 16.);
            let nw = canvas.text_width(&self.fonts, Weight::Regular, &name, 12.5);
            canvas.text(&self.fonts, Weight::Regular, &name, x + (cw - nw) / 2., icon_y + g.icon + 20., 12.5, rgba(theme::TEXT));
        }

        // Indicator that there are more rows below/above.
        if total_rows > g.rows {
            let (bx, bw) = (w - 8., 3.);
            let track = g.rows as f32 * (ch + GAP) - GAP;
            let thumb = (track * g.rows as f32 / total_rows as f32).max(24.);
            let ty = g.y0 + (track - thumb) * self.top as f32 / (total_rows - g.rows) as f32;
            if self.mode == Mode::Full {
                canvas.fill_rrect(bx, ty, bw, thumb, 1.5, rgba(0xffffff40));
            }
        }
    }
}

impl Painter for Launcher {
    fn paint(&mut self, _output: &str, canvas: &mut Canvas, hits: &mut Hits) {
        if !self.reported && std::env::var_os("WAYLO_DEBUG").is_some() {
            self.reported = true;
            eprintln!("waylo-launch: primer fotograma a los {:?}", self.started.elapsed());
        }
        match self.mode {
            Mode::List => self.paint_list(canvas, hits),
            _ => self.paint_grid(canvas, hits),
        }
    }

    fn interact(&mut self, _output: &str, action: Interaction) -> Flow {
        match action {
            Interaction::Click { id: Some(id), button: Button::Left } if id >= CELL_BASE => {
                let first = if self.mode == Mode::List { self.top } else { self.top * self.cols };
                self.launch(first + (id - CELL_BASE) as usize)
            }
            // In full screen, a click on the background closes.
            Interaction::Click { id: None, button: Button::Left } if self.mode == Mode::Full => Flow::Quit,
            Interaction::Scroll { delta, .. } => {
                let steps = delta.round() as isize;
                if steps == 0 {
                    return Flow::Idle;
                }
                self.move_selection(steps.signum() * self.cols as isize, false);
                Flow::Redraw
            }
            _ => Flow::Idle,
        }
    }

    fn key(&mut self, key: Key) -> Flow {
        let len = self.query.chars().count();
        let cols = self.cols as isize;
        let grid = self.mode != Mode::List;
        match key {
            Key::Escape => {
                // First clear the search; with an empty field, close.
                if self.query.is_empty() {
                    return Flow::Quit;
                }
                self.query.clear();
                self.cursor = 0;
                self.search();
            }
            Key::Enter => return self.launch(self.selected),
            Key::Tab => self.move_selection(1, true),
            Key::BackTab => self.move_selection(-1, true),
            Key::Down => self.move_selection(if grid { cols } else { 1 }, !grid),
            Key::Up => self.move_selection(if grid { -cols } else { -1 }, !grid),
            // In the grid the left/right arrows move the selection; the text cursor is
            // moved only with Home/End (the search box is short).
            Key::Right if grid => self.move_selection(1, false),
            Key::Left if grid => self.move_selection(-1, false),
            Key::Left => self.cursor = self.cursor.saturating_sub(1),
            Key::Right => self.cursor = (self.cursor + 1).min(len),
            Key::Home => self.cursor = 0,
            Key::End => self.cursor = len,
            Key::Backspace if self.cursor > 0 => {
                let (a, b) = (self.byte_at(self.cursor - 1), self.byte_at(self.cursor));
                self.query.replace_range(a..b, "");
                self.cursor -= 1;
                self.search();
            }
            Key::Delete if self.cursor < len => {
                let (a, b) = (self.byte_at(self.cursor), self.byte_at(self.cursor + 1));
                self.query.replace_range(a..b, "");
                self.search();
            }
            Key::Char(c) => {
                let at = self.byte_at(self.cursor);
                self.query.insert(at, c);
                self.cursor += 1;
                self.search();
            }
            _ => return Flow::Idle,
        }
        Flow::Redraw
    }
}

fn main() {
    let started = Instant::now();
    let mode = match std::env::args().nth(1).as_deref() {
        None | Some("compact") => Mode::Compact,
        Some("grid") | Some("full") => Mode::Full,
        Some("list") => Mode::List,
        Some(other) => {
            eprintln!("waylo-launch: modo desconocido '{other}' (usa: compact, grid o list)");
            std::process::exit(2);
        }
    };
    let fonts = Fonts::system().unwrap_or_else(|e| {
        eprintln!("waylo-launch: {e}");
        std::process::exit(1);
    });
    let apps = apps::load();
    if std::env::var_os("WAYLO_DEBUG").is_some() {
        eprintln!("waylo-launch: {} aplicaciones leídas en {:?}", apps.len(), started.elapsed());
    }
    let focused = waylo_core::hypr::monitors().ok().and_then(|m| m.into_iter().find(|m| m.focused)).map(|m| m.name);
    let layout = match mode {
        Mode::Full => Layout::Fullscreen,
        Mode::Compact => {
            let (width, height) = compact_size();
            Layout::Centered { width, height }
        }
        Mode::List => Layout::Centered { width: LIST_W, height: (HEADER_H + LIST_ROWS as f32 * LIST_ROW_H + PAD * 2.) as u32 },
    };
    let spec = PanelSpec { namespace: "waylo-launch", layout, only_monitor: focused };
    let app = Launcher::new(mode, fonts, apps, apps::usage(), started);
    if let Err(e) = waylo_shell::run_panels(spec, app, |_| {}) {
        eprintln!("waylo-launch: {e}");
        std::process::exit(1);
    }
}
