use std::process::Command;

use ab_glyph::{Font as _, FontRef, PxScale, ScaleFont as _, point};
use tiny_skia::{Color, Pixmap};

pub(crate) type Font = FontRef<'static>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Bold,
}

/// System fonts, loaded with `fc-match` (regular sans and bold).
pub struct Fonts {
    regular: Font,
    bold: Font,
}

fn load(pattern: &str) -> Result<Font, String> {
    let out = Command::new("fc-match")
        .args(["-f", "%{file}", pattern])
        .output()
        .map_err(|e| format!("no pude ejecutar fc-match: {e}"))?;
    let path = String::from_utf8_lossy(&out.stdout).into_owned();
    let data = std::fs::read(&path).map_err(|e| format!("no pude leer la fuente {path}: {e}"))?;
    // The font lives for the whole program: it is leaked to 'static to avoid copying it.
    FontRef::try_from_slice(Box::leak(data.into_boxed_slice())).map_err(|e| format!("fuente inválida {path}: {e}"))
}

impl Fonts {
    pub fn system() -> Result<Self, String> {
        Ok(Self { regular: load("sans")?, bold: load("sans:bold")? })
    }

    pub(crate) fn get(&self, weight: Weight) -> &Font {
        match weight {
            Weight::Regular => &self.regular,
            Weight::Bold => &self.bold,
        }
    }
}

/// ab_glyph measures the size by the font's total height; here the em size is used,
/// like CSS, so that 13 px looks like 13 px.
fn scale(font: &Font, size: f32) -> PxScale {
    let em = font.units_per_em().unwrap_or(1000.);
    PxScale::from(size * font.height_unscaled() / em)
}

pub(crate) fn width(font: &Font, text: &str, size: f32) -> f32 {
    let scaled = font.as_scaled(scale(font, size));
    let mut total = 0.;
    let mut prev = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(p) = prev {
            total += scaled.kern(p, id);
        }
        total += scaled.h_advance(id);
        prev = Some(id);
    }
    total
}

pub(crate) fn draw(pm: &mut Pixmap, font: &Font, text: &str, x: f32, baseline: f32, size: f32, color: Color) -> f32 {
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    let data = pm.data_mut();
    let [sr, sg, sb, sa] = [color.red(), color.green(), color.blue(), color.alpha()];
    let scaled = font.as_scaled(scale(font, size));
    let mut pen = x;
    let mut prev = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(p) = prev {
            pen += scaled.kern(p, id);
        }
        let glyph = id.with_scale_and_position(scale(font, size), point(pen, baseline));
        if let Some(outline) = font.outline_glyph(glyph) {
            let b = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let cov = coverage * sa;
                if cov <= 0. {
                    return;
                }
                let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                if px < 0 || py < 0 || px >= w || py >= h {
                    return;
                }
                let i = ((py * w + px) * 4) as usize;
                // Premultiplied pixels: dst = src·cov + dst·(1-cov)
                let blend = |d: u8, s: f32| (s * cov * 255.0 + d as f32 * (1.0 - cov)).round().min(255.0) as u8;
                data[i] = blend(data[i], sr);
                data[i + 1] = blend(data[i + 1], sg);
                data[i + 2] = blend(data[i + 2], sb);
                data[i + 3] = (cov * 255.0 + data[i + 3] as f32 * (1.0 - cov)).round().min(255.0) as u8;
            });
        }
        pen += scaled.h_advance(id);
        prev = Some(id);
    }
    pen - x
}
