use std::path::Path;

use tiny_skia::{Color, FilterQuality, FillRule, Paint, PathBuilder, Pattern, Pixmap, PixmapPaint, SpreadMode, Stroke, Transform};

use crate::text::{self, Fonts, Weight};

/// Drawing surface in memory (premultiplied RGBA).
///
/// Drawing uses **logical** coordinates (the compositor's); `scale` says how many physical
/// pixels there are per logical unit (2 on a HiDPI screen) and the canvas scales itself.
pub struct Canvas {
    pixmap: Pixmap,
    scale: f32,
}

fn paint(color: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color);
    p.anti_alias = true;
    p
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.).min(h / 2.);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Opaque, already decoded image, ready to draw on a [`Canvas`].
pub struct Image {
    pixmap: Pixmap,
}

impl Image {
    /// Creates the image from 8-bit RGB pixels (`w * h * 3` bytes).
    pub fn from_rgb(w: u32, h: u32, rgb: &[u8]) -> Option<Self> {
        if rgb.len() < (w as usize) * (h as usize) * 3 {
            return None;
        }
        let mut pixmap = Pixmap::new(w, h)?;
        for (dst, src) in pixmap.data_mut().chunks_exact_mut(4).zip(rgb.chunks_exact(3)) {
            dst.copy_from_slice(&[src[0], src[1], src[2], 255]);
        }
        Some(Self { pixmap })
    }

    /// Loads an icon (PNG or SVG) and leaves it in a `px` by `px` square, with transparency and
    /// keeping the aspect ratio. It is scaled down here, once, so that painting it later is cheap.
    pub fn from_file(path: &Path, px: u32) -> Option<Self> {
        let data = std::fs::read(path).ok()?;
        let mut out = Pixmap::new(px, px)?;
        let target = px as f32;
        if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("svg")) {
            let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).ok()?;
            let size = tree.size();
            let s = (target / size.width()).min(target / size.height());
            let (tx, ty) = ((target - size.width() * s) / 2., (target - size.height() * s) / 2.);
            resvg::render(&tree, Transform::from_row(s, 0., 0., s, tx, ty), &mut out.as_mut());
        } else {
            let mut src = Pixmap::decode_png(&data).ok()?;
            // Halve it while it is more than twice too big: a single big jump gives jagged edges.
            while src.width() >= 2 * px && src.height() >= 2 * px {
                let mut half = Pixmap::new(src.width() / 2, src.height() / 2)?;
                let paint = PixmapPaint { quality: FilterQuality::Bilinear, ..Default::default() };
                half.draw_pixmap(0, 0, src.as_ref(), &paint, Transform::from_scale(0.5, 0.5), None);
                src = half;
            }
            let s = (target / src.width() as f32).min(target / src.height() as f32);
            let (tx, ty) = ((target - src.width() as f32 * s) / 2., (target - src.height() as f32 * s) / 2.);
            let paint = PixmapPaint { quality: FilterQuality::Bicubic, ..Default::default() };
            out.draw_pixmap(0, 0, src.as_ref(), &paint, Transform::from_row(s, 0., 0., s, tx, ty), None);
        }
        Some(Self { pixmap: out })
    }

    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }
}

impl Canvas {
    /// Draws `img` stretched (no smoothing: it is much faster; the image must already come at a
    /// size close to the destination) into the rectangle `(x, y, w, h)` with rounded corners.
    pub fn draw_image(&mut self, img: &Image, x: f32, y: f32, w: f32, h: f32, radius: f32) {
        let Some(path) = rounded_rect(x, y, w, h, radius) else { return };
        let sx = w / img.pixmap.width() as f32;
        let sy = h / img.pixmap.height() as f32;
        let pattern_t = Transform::from_row(sx, 0., 0., sy, x, y);
        let mut p = Paint::default();
        p.anti_alias = true;
        p.shader = Pattern::new(img.pixmap.as_ref(), SpreadMode::Pad, FilterQuality::Nearest, 1.0, pattern_t);
        let t = self.transform();
        self.pixmap.fill_path(&path, &p, FillRule::Winding, t, None);
    }

    /// `width` and `height` are logical; the real buffer is `width * scale` by `height * scale`.
    pub fn new(width: u32, height: u32, scale: u32) -> Option<Self> {
        let scale = scale.max(1);
        Pixmap::new(width * scale, height * scale).map(|pixmap| Self { pixmap, scale: scale as f32 })
    }

    /// Buffer size in physical pixels.
    pub fn pixel_size(&self) -> (u32, u32) {
        (self.pixmap.width(), self.pixmap.height())
    }

    /// Logical width.
    pub fn width(&self) -> f32 {
        self.pixmap.width() as f32 / self.scale
    }

    /// Physical pixels per logical unit.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Logical height.
    pub fn height(&self) -> f32 {
        self.pixmap.height() as f32 / self.scale
    }

    fn transform(&self) -> Transform {
        Transform::from_scale(self.scale, self.scale)
    }

    pub fn clear(&mut self) {
        self.pixmap.fill(Color::TRANSPARENT);
    }

    /// Fills the whole canvas with `color` (much faster than a path-based rectangle).
    pub fn fill(&mut self, color: Color) {
        self.pixmap.fill(color);
    }

    pub fn fill_rrect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
        let t = self.transform();
        if let Some(path) = rounded_rect(x, y, w, h, radius) {
            self.pixmap.fill_path(&path, &paint(color), FillRule::Winding, t, None);
        }
    }

    /// 1 px border that stays inside the rectangle.
    pub fn stroke_rrect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
        let t = self.transform();
        if let Some(path) = rounded_rect(x + 0.5, y + 0.5, w - 1., h - 1., radius) {
            let stroke = Stroke { width: 1.0, ..Default::default() };
            self.pixmap.stroke_path(&path, &paint(color), &stroke, t, None);
        }
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, color: Color) {
        let t = self.transform();
        if let Some(path) = PathBuilder::from_circle(cx, cy, r) {
            self.pixmap.fill_path(&path, &paint(color), FillRule::Winding, t, None);
        }
    }

    pub fn text_width(&self, fonts: &Fonts, weight: Weight, text: &str, size: f32) -> f32 {
        text::width(fonts.get(weight), text, size * self.scale) / self.scale
    }

    /// Draws text with the baseline at `baseline`. Returns the width used.
    /// Trims `text` adding "…" so it fits in `max` logical units.
    pub fn ellipsize(&self, fonts: &Fonts, weight: Weight, text: &str, size: f32, max: f32) -> String {
        if self.text_width(fonts, weight, text, size) <= max {
            return text.to_string();
        }
        let mut out = text.to_string();
        while !out.is_empty() {
            out.pop();
            let cand = format!("{}…", out.trim_end());
            if self.text_width(fonts, weight, &cand, size) <= max {
                return cand;
            }
        }
        String::new()
    }

    pub fn text(&mut self, fonts: &Fonts, weight: Weight, text: &str, x: f32, baseline: f32, size: f32, color: Color) -> f32 {
        let s = self.scale;
        text::draw(&mut self.pixmap, fonts.get(weight), text, x * s, baseline * s, size * s, color) / s
    }

    /// Copies the pixels into a `wl_shm` Argb8888 buffer (BGRA in little endian).
    pub fn write_argb8888(&self, out: &mut [u8]) {
        // RGBA -> BGRA (Argb8888 little-endian): R and B are swapped a word at a time, which
        // the compiler vectorizes; byte by byte it was several times slower at 2560x1440.
        for (dst, src) in out.chunks_exact_mut(4).zip(self.pixmap.data().chunks_exact(4)) {
            let x = u32::from_le_bytes([src[0], src[1], src[2], src[3]]);
            let y = (x & 0xff00ff00) | ((x >> 16) & 0xff) | ((x & 0xff) << 16);
            dst.copy_from_slice(&y.to_le_bytes());
        }
    }
}
