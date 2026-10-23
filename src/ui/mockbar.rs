//! Mock Windows 11 taskbar segment for the README demo (`--render demo`): a rounded
//! strip in the app's theme colors with a few dim app placeholders, the Spectrum tray icon and clock-like
//! bars. It draws no real content (no time, no real icons).

use super::draw::{sc, Canvas, Theme, R};
use crate::icon_render::render_bgra;

/// Height of the strip at 100% scale.
pub const BAR_H: i32 = 48;

/// Paints the strip filling `c` (the canvas is the strip's size and already holds the stage color).
/// `icon_cx` is the tray icon's center in canvas pixels; `hover` draws the hover highlight behind it.
pub fn paint(c: &mut Canvas, th: &Theme, dpi: u32, icon_cx: i32, hover: bool) {
    let bar = R::new(0, 0, c.width(), c.height());
    let cy = bar.cy().round() as i32;
    c.fill_rrect(bar, sc(9, dpi) as f32, th.surface, 1.0);
    c.stroke_rrect(bar, sc(9, dpi) as f32, 1.0, th.line, 1.0);

    // App placeholders, left of the tray: dim rounded squares, the first one a little stronger.
    let (size, gap) = (sc(26, dpi), sc(14, dpi));
    let cluster = 4 * size + 3 * gap;
    let mut x = (bar.w() as f32 * 0.34) as i32 - cluster / 2;
    for i in 0..4 {
        let r = R::xywh(x, cy - size / 2, size, size);
        c.fill_rrect(
            r,
            sc(7, dpi) as f32,
            th.text_dim,
            if i == 0 { 0.34 } else { 0.18 },
        );
        x += size + gap;
    }

    // Tray icon with its hover highlight.
    if hover {
        let s = sc(36, dpi);
        c.fill_rrect(
            R::xywh(icon_cx - s / 2, cy - s / 2, s, s),
            sc(6, dpi) as f32,
            th.text,
            0.10,
        );
    }
    let isz = sc(20, dpi);
    c.blit_premul_bgra(
        icon_cx - isz / 2,
        cy - isz / 2,
        isz,
        isz,
        &render_bgra(isz as u32),
    );

    // Clock placeholder: two dim bars, right-aligned.
    let right = bar.r - sc(18, dpi);
    let (bh, tw, dw) = (sc(6, dpi), sc(44, dpi), sc(58, dpi));
    c.fill_rrect(
        R::xywh(right - tw, cy - sc(9, dpi), tw, bh),
        bh as f32 / 2.0,
        th.text_dim,
        0.55,
    );
    c.fill_rrect(
        R::xywh(right - dw, cy + sc(3, dpi), dw, bh),
        bh as f32 / 2.0,
        th.text_dim,
        0.36,
    );
}
