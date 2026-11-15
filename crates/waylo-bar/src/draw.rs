//! What the bar looks like: takes a [`View`] and paints it with `waylo-render`.

use waylo_render::{Canvas, Fonts, Weight, rgba, theme};
use waylo_shell::Hits;

use crate::view::View;
use crate::widgets;

pub fn bar(canvas: &mut Canvas, fonts: &Fonts, view: &View<'_>, hits: &mut Hits) {
    let (w, h) = (canvas.width(), canvas.height());
    canvas.fill_rrect(0., 0., w, h, theme::RADIUS, rgba(theme::SURFACE));
    canvas.stroke_rrect(0., 0., w, h, theme::RADIUS, rgba(theme::BORDER));

    workspaces(canvas, fonts, view, hits, h);
    clock(canvas, fonts, view, w, h);
    widgets::layout(canvas, fonts, view, hits, w, h);
}

/// Three states: active = white pill, occupied = faint capsule, free = just the number.
fn workspaces(canvas: &mut Canvas, fonts: &Fonts, view: &View<'_>, hits: &mut Hits, h: f32) {
    let mut x = 10.;
    for ws in &view.workspaces {
        let label = ws.to_string();
        let active = *ws == view.active;
        let occupied = view.occupied.contains(ws);
        let (weight, color) = if active {
            (Weight::Bold, theme::ACTIVE_FG)
        } else if occupied {
            (Weight::Bold, 0xffffffff)
        } else {
            (Weight::Regular, theme::TEXT_FAINT)
        };
        let tw = canvas.text_width(fonts, weight, &label, 12.5);
        let bw = (tw + 18.).max(26.);
        let (by, bh) = ((h - 24.) / 2., 24.);
        if active {
            canvas.fill_rrect(x, by, bw, bh, 12., rgba(theme::ACTIVE_BG));
        } else if hits.hovered() == Some(*ws) {
            canvas.fill_rrect(x, by, bw, bh, 12., rgba(theme::HOVER_BG));
        } else if occupied {
            canvas.fill_rrect(x, by, bw, bh, 12., rgba(theme::OCCUPIED_BG));
        }
        // The clickable area spans the whole bar height, which makes it easier to hit.
        hits.add(*ws, x - 2., 0., bw + 4., h);
        canvas.text(fonts, weight, &label, x + (bw - tw) / 2., by + 16.5, 12.5, rgba(color));
        x += bw + 4.;
    }
}

fn clock(canvas: &mut Canvas, fonts: &Fonts, view: &View<'_>, w: f32, h: f32) {
    let tw = canvas.text_width(fonts, Weight::Bold, view.time, 13.5);
    let dw = canvas.text_width(fonts, Weight::Regular, view.date, 13.5);
    let mut x = (w - (tw + 8. + 3. + 8. + dw)) / 2.;
    canvas.text(fonts, Weight::Bold, view.time, x, h / 2. + 5., 13.5, rgba(theme::TEXT));
    x += tw + 8.;
    canvas.fill_circle(x + 1.5, h / 2., 1.5, rgba(0xffffff4d));
    x += 11.;
    canvas.text(fonts, Weight::Regular, view.date, x, h / 2. + 5., 13.5, rgba(theme::TEXT_MUTED));
}
