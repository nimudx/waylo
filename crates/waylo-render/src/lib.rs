//! 2D drawing on the CPU for Waylo.
//!
//! Offers a [`Canvas`] (shapes and text) and a [`theme`] with the colors. It knows nothing about Wayland
//! or Hyprland: it only produces pixels, so it serves the bar, the Exposé or whatever comes next.

mod canvas;
mod text;
pub mod theme;

pub use canvas::{Canvas, Image};
pub use text::{Fonts, Weight};
pub use tiny_skia::Color;

/// Color from `0xRRGGBBAA`.
pub fn rgba(hex: u32) -> Color {
    Color::from_rgba8((hex >> 24) as u8, (hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}
