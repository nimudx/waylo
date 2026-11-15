//! Widgets on the right side: network, brightness, volume, battery and bluetooth.
//!
//! Each widget computes its width and draws itself; `layout` places them from right to left and
//! registers each one's clickable area with its id.

use waylo_core::sys::network::{Kind, Net};
use waylo_render::{Canvas, Fonts, Weight, rgba, theme};
use waylo_shell::Hits;

use crate::view::View;

/// Zone identifiers (those below 100 are workspaces).
pub mod id {
    pub const NETWORK: u32 = 1000;
    pub const BRIGHTNESS: u32 = 1001;
    pub const VOLUME: u32 = 1002;
    pub const BATTERY: u32 = 1003;
    pub const BLUETOOTH: u32 = 1004;
}

const PAD: f32 = 10.;
const GAP: f32 = 8.;
const SIZE: f32 = 12.5;

enum Widget {
    Network(Option<Net>),
    Brightness(u8),
    Volume(u32, bool),
    Battery(u32, bool),
    Bluetooth(bool),
}

impl Widget {
    fn id(&self) -> u32 {
        match self {
            Widget::Network(_) => id::NETWORK,
            Widget::Brightness(_) => id::BRIGHTNESS,
            Widget::Volume(..) => id::VOLUME,
            Widget::Battery(..) => id::BATTERY,
            Widget::Bluetooth(_) => id::BLUETOOTH,
        }
    }
}

fn network_label(net: &Net) -> String {
    let mut name: String = net.name.chars().take(18).collect();
    if net.name.chars().count() > 18 {
        name.push('…');
    }
    name
}

fn level_label(level: u32) -> String {
    format!("{level}%")
}

/// Widget content: (icon, icon width, text, text color).
fn content(widget: &Widget) -> (f32, String, u32) {
    match widget {
        Widget::Network(Some(net)) if net.kind == Kind::Wifi => (18., network_label(net), theme::TEXT),
        Widget::Network(Some(_)) => (0., "Ethernet".into(), theme::TEXT),
        Widget::Network(None) => (0., "Sin red".into(), theme::TEXT_MUTED),
        Widget::Brightness(l) => (30., level_label(*l as u32), theme::TEXT),
        Widget::Volume(l, muted) => (30., if *muted { "Mute".into() } else { level_label(*l) }, if *muted { theme::TEXT_MUTED } else { theme::TEXT }),
        Widget::Battery(l, charging) => (22., level_label(*l), if *l <= 20 && !*charging { 0xff6b6bff } else { theme::TEXT }),
        Widget::Bluetooth(on) => (6., "Bluetooth".into(), if *on { theme::TEXT } else { theme::TEXT_MUTED }),
    }
}

fn draw_icon(canvas: &mut Canvas, widget: &Widget, x: f32, cy: f32) {
    match widget {
        Widget::Network(Some(net)) if net.kind == Kind::Wifi => {
            // Four bars that fill up according to the signal.
            for (i, (height, min)) in [(4., 1), (7., 25), (10., 50), (13., 75)].into_iter().enumerate() {
                let color = if net.signal >= min { 0xffffffe6 } else { 0xffffff40 };
                canvas.fill_rrect(x + i as f32 * 5., cy + 6.5 - height, 3., height, 1., rgba(color));
            }
        }
        Widget::Brightness(l) => meter(canvas, x, cy, *l as u32, 0xffd76aff),
        Widget::Volume(l, muted) => meter(canvas, x, cy, if *muted { 0 } else { *l }, 0xffffffd9),
        Widget::Battery(l, charging) => {
            let color = if *charging { 0x7bd88fff } else if *l <= 20 { 0xff6b6bff } else { 0xffffffd9 };
            canvas.stroke_rrect(x, cy - 5., 20., 10., 3., rgba(0xffffff80));
            let fill = 16. * (*l).min(100) as f32 / 100.;
            canvas.fill_rrect(x + 2., cy - 3., fill, 6., 1.5, rgba(color));
            canvas.fill_rrect(x + 20., cy - 2., 2., 4., 1., rgba(0xffffff80));
        }
        Widget::Bluetooth(on) => {
            let color = if *on { theme::ACCENT } else { 0xffffff4d };
            canvas.fill_circle(x + 3., cy, 3., rgba(color));
        }
        Widget::Network(_) => {}
    }
}

/// Small horizontal bar from 0 to 100.
fn meter(canvas: &mut Canvas, x: f32, cy: f32, level: u32, color: u32) {
    canvas.fill_rrect(x, cy - 2., 30., 4., 2., rgba(0xffffff26));
    let fill = 30. * level.min(100) as f32 / 100.;
    if fill >= 1. {
        canvas.fill_rrect(x, cy - 2., fill, 4., 2., rgba(color));
    }
}

fn widgets(view: &View<'_>) -> Vec<Widget> {
    let mut list = vec![Widget::Network(view.sys.network.clone())];
    if let Some(level) = view.brightness() {
        list.push(Widget::Brightness(level));
    }
    if let Some((level, muted)) = view.sys.volume {
        list.push(Widget::Volume(level, muted));
    }
    if let Some((level, charging)) = view.sys.battery {
        list.push(Widget::Battery(level, charging));
    }
    list.push(Widget::Bluetooth(view.sys.bluetooth));
    list
}

/// Draws the widgets flush against the right edge and registers their zones.
pub fn layout(canvas: &mut Canvas, fonts: &Fonts, view: &View<'_>, hits: &mut Hits, w: f32, h: f32) {
    let list = widgets(view);
    // Widths
    let sizes: Vec<(f32, String, u32, f32)> = list
        .iter()
        .map(|widget| {
            let (icon, text, color) = content(widget);
            let tw = canvas.text_width(fonts, Weight::Regular, &text, SIZE);
            let width = PAD * 2. + if icon > 0. { icon + GAP } else { 0. } + tw;
            (icon, text, color, width)
        })
        .collect();
    let cy = h / 2.;
    let mut right = w - 4.;
    for (widget, (icon, text, color, width)) in list.iter().zip(&sizes).rev() {
        let x = right - width;
        if hits.hovered() == Some(widget.id()) {
            canvas.fill_rrect(x, (h - 24.) / 2., *width, 24., 12., rgba(theme::HOVER_BG));
        }
        hits.add(widget.id(), x, 0., *width, h);
        let mut cursor = x + PAD;
        if *icon > 0. {
            draw_icon(canvas, widget, cursor, cy);
            cursor += icon + GAP;
        } else {
            draw_icon(canvas, widget, cursor, cy);
        }
        canvas.text(fonts, Weight::Regular, text, cursor, cy + 4.5, SIZE, rgba(*color));
        right = x;
    }
}
