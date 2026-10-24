//! The custom-drawn controls: button, expander, toggle, checkbox, slider and dropdown field. Each draws
//! itself from a [`WState`] with the design tokens of `design.rs`.

use super::super::design;
use super::super::draw::{mix, sc, scf, Canvas, Fonts, Theme, R, TA_CENTER, TA_LEFT};
use super::slider::{slider_span, THUMB_R};
use super::WState;

/// What every control needs to draw: the theme (with the window's accent), the fonts and the DPI.
pub struct Ctx<'a> {
    pub th: Theme,
    pub fonts: &'a Fonts,
    pub dpi: u32,
}

/// Button variants from the design language (`design.rs`). There is no text-only variant: every action
/// looks like a button.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BtnStyle {
    /// The single most important action of a view.
    Primary,
    /// Every other action.
    Secondary,
    /// A destructive action, first click.
    Danger,
    /// A destructive action waiting for its confirming click.
    DangerConfirm,
}

/// c faded toward the background, for a disabled control.
fn disabled(ctx: &Ctx, c: u32) -> u32 {
    mix(c, ctx.th.bg, 0.55)
}

pub fn focus_ring(c: &mut Canvas, ctx: &Ctx, r: R, radius: f32) {
    let o = scf(2.0, ctx.dpi);
    c.stroke_rrect(
        r.inset(-(o.round() as i32)),
        radius + o,
        scf(2.0, ctx.dpi),
        ctx.th.accent,
        1.0,
    );
}

pub fn draw_button(c: &mut Canvas, ctx: &Ctx, r: R, label: &str, state: WState, style: BtnStyle) {
    let th = &ctx.th;
    let rad = scf(design::RADIUS_CONTROL, ctx.dpi);
    let filled = |base: u32| -> u32 {
        if !state.enabled {
            mix(base, th.bg, 0.6)
        } else if state.pressed && state.hover {
            mix(base, 0x000000, 0.2)
        } else if state.hover {
            mix(base, 0xFFFFFF, 0.14)
        } else {
            base
        }
    };
    let outlined_fill = if !state.enabled {
        mix(th.surface, th.bg, 0.5)
    } else if state.pressed && state.hover {
        th.pressed_fill()
    } else if state.hover {
        th.hover_fill()
    } else {
        th.surface
    };
    let outlined_border = if state.hover && state.enabled {
        mix(th.line, th.text, 0.18)
    } else {
        th.line
    };
    let (fill, border, txt) = match style {
        BtnStyle::Secondary => (
            outlined_fill,
            Some(outlined_border),
            if state.enabled {
                th.text
            } else {
                disabled(ctx, th.text)
            },
        ),
        BtnStyle::Danger => {
            let d = design::danger(th);
            (
                outlined_fill,
                Some(if state.hover && state.enabled {
                    mix(th.line, d, 0.6)
                } else {
                    outlined_border
                }),
                if state.enabled { d } else { disabled(ctx, th.text) },
            )
        }
        BtnStyle::Primary => (filled(th.accent), None, th.on_accent()),
        BtnStyle::DangerConfirm => {
            let d = if th.dark { 0xC42B1C } else { design::danger(th) };
            (filled(d), None, 0xFFFFFF)
        }
    };
    c.fill_rrect(r, rad, fill, 1.0);
    if let Some(b) = border {
        c.stroke_rrect(r, rad, 1.0_f32.max(scf(1.0, ctx.dpi).floor()), b, 1.0);
    }
    c.text(r, label, ctx.fonts.body, txt, TA_CENTER);
    if state.focus {
        focus_ring(c, ctx, r, rad);
    }
}

/// A full-width disclosure row: chevron, label, hover fill. Opens and closes a group of advanced controls.
pub fn draw_expander(c: &mut Canvas, ctx: &Ctx, r: R, label: &str, open: bool, state: WState) {
    let th = &ctx.th;
    let rad = scf(design::RADIUS_CONTROL, ctx.dpi);
    if state.enabled && (state.hover || state.pressed) {
        let f = if state.pressed {
            th.pressed_fill()
        } else {
            th.hover_fill()
        };
        c.fill_rrect(r, rad, f, 1.0);
    }
    let col = if state.enabled {
        th.text
    } else {
        disabled(ctx, th.text)
    };
    let s = scf(5.0, ctx.dpi);
    let (cx, cy) = (r.l as f32 + scf(12.0, ctx.dpi), r.cy());
    let pts = if open {
        [(cx - s, cy - s * 0.5), (cx, cy + s * 0.5), (cx + s, cy - s * 0.5)]
    } else {
        [(cx - s * 0.5, cy - s), (cx + s * 0.5, cy), (cx - s * 0.5, cy + s)]
    };
    c.polyline(&pts, false, scf(1.6, ctx.dpi), col, 1.0);
    let tr = R::new(r.l + sc(28, ctx.dpi), r.t, r.r, r.b);
    c.text(tr, label, ctx.fonts.strong, col, TA_LEFT);
    if state.focus {
        focus_ring(c, ctx, r, rad);
    }
}

pub fn draw_toggle(c: &mut Canvas, ctx: &Ctx, r: R, on: bool, state: WState) {
    let th = &ctx.th;
    let h = r.h() as f32;
    let rad = h / 2.0;
    let (l, t) = (r.l as f32, r.t as f32);
    let bw = scf(1.5, ctx.dpi);
    if on {
        let fill = if !state.enabled {
            mix(th.accent, th.bg, 0.6)
        } else if state.hover {
            mix(th.accent, 0xFFFFFF, 0.1)
        } else {
            th.accent
        };
        c.fill_rrect_f(l, t, r.r as f32, r.b as f32, rad, fill, 1.0);
        let kr = rad - scf(4.5, ctx.dpi) + if state.hover { scf(0.8, ctx.dpi) } else { 0.0 };
        let knob = if state.enabled {
            th.on_accent()
        } else {
            disabled(ctx, th.on_accent())
        };
        c.fill_circle(r.r as f32 - rad, t + rad, kr, knob, 1.0);
    } else {
        let col = if state.enabled {
            mix(th.text_dim, th.text, if state.hover { 0.4 } else { 0.0 })
        } else {
            disabled(ctx, th.text_dim)
        };
        c.stroke_rrect_f(l, t, r.r as f32, r.b as f32, rad, bw, col, 1.0);
        let kr = rad - scf(6.0, ctx.dpi) + if state.hover { scf(0.8, ctx.dpi) } else { 0.0 };
        c.fill_circle(l + rad, t + rad, kr, col, 1.0);
    }
    if state.focus {
        focus_ring(c, ctx, r, rad);
    }
}

pub fn draw_checkbox(c: &mut Canvas, ctx: &Ctx, r: R, label: &str, checked: bool, state: WState) {
    let th = &ctx.th;
    let b = sc(design::CHECK, ctx.dpi);
    let bx = R::xywh(r.l, r.t + (r.h() - b) / 2, b, b);
    let rad = scf(design::RADIUS_CONTROL, ctx.dpi);
    let txt = if state.enabled {
        th.text
    } else {
        disabled(ctx, th.text)
    };
    if checked {
        let fill = if !state.enabled {
            mix(th.accent, th.bg, 0.6)
        } else if state.pressed {
            mix(th.accent, 0x000000, 0.2)
        } else if state.hover {
            mix(th.accent, 0xFFFFFF, 0.12)
        } else {
            th.accent
        };
        c.fill_rrect(bx, rad, fill, 1.0);
        c.stroke_rrect(bx, rad, 1.0, mix(fill, 0x000000, 0.3), 1.0);
        let ink = th.on_accent();
        let (x, y, s) = (bx.l as f32, bx.t as f32, b as f32);
        let pts = [
            (x + s * 0.26, y + s * 0.53),
            (x + s * 0.43, y + s * 0.69),
            (x + s * 0.75, y + s * 0.32),
        ];
        c.polyline(&pts, false, scf(1.8, ctx.dpi), ink, 1.0);
    } else {
        let f = if state.pressed {
            th.pressed_fill()
        } else if state.hover {
            th.hover_fill()
        } else {
            th.surface
        };
        c.fill_rrect(bx, rad, f, 1.0);
        let bc = if state.enabled {
            mix(th.text_dim, th.text, if state.hover { 0.35 } else { 0.0 })
        } else {
            disabled(ctx, th.text_dim)
        };
        c.stroke_rrect(bx, rad, scf(1.2, ctx.dpi), bc, 1.0);
    }
    let tr = R::new(bx.r + sc(10, ctx.dpi), r.t, r.r, r.b);
    c.text(tr, label, ctx.fonts.body, txt, TA_LEFT);
    if state.focus {
        focus_ring(c, ctx, bx, rad);
    }
}

/// A small mark across the track at `t` (the monitor's peak on the SDR content slider). Drawn before the
/// slider so the thumb covers it.
pub fn draw_slider_tick(c: &mut Canvas, ctx: &Ctx, r: R, t: f64, enabled: bool) {
    let (a, b) = slider_span(r, ctx.dpi);
    let x = (a + (b - a) * t as f32).round();
    let h = scf(6.0, ctx.dpi);
    let w = scf(2.0, ctx.dpi).max(1.0);
    let col = if enabled {
        ctx.th.text_dim
    } else {
        mix(ctx.th.text_dim, ctx.th.bg, 0.5)
    };
    c.fill_rrect_f(
        x - w / 2.0,
        r.cy() - h,
        x + w / 2.0,
        r.cy() + h,
        w / 2.0,
        col,
        1.0,
    );
}

/// `r` is the full hit area of the slider (thumb travel is inset by the thumb radius).
pub fn draw_slider(c: &mut Canvas, ctx: &Ctx, r: R, t: f64, state: WState, dragging: bool) {
    let th = &ctx.th;
    let (a, b) = slider_span(r, ctx.dpi);
    let cy = r.cy();
    let tw = scf(4.0, ctx.dpi);
    let x = a + (b - a) * t as f32;
    let track = if state.enabled {
        th.line
    } else {
        mix(th.line, th.bg, 0.4)
    };
    c.fill_rrect_f(
        a - tw / 2.0,
        cy - tw / 2.0,
        b + tw / 2.0,
        cy + tw / 2.0,
        tw / 2.0,
        mix(track, th.text_dim, 0.28),
        1.0,
    );
    if state.enabled {
        c.fill_rrect_f(
            a - tw / 2.0,
            cy - tw / 2.0,
            x,
            cy + tw / 2.0,
            tw / 2.0,
            th.accent,
            1.0,
        );
    }
    let tr = scf(THUMB_R, ctx.dpi);
    if state.enabled {
        let outer = if th.dark {
            mix(th.surface, th.text, 0.88)
        } else {
            0xFFFFFF
        };
        c.fill_circle(x, cy + 1.0, tr + 0.5, 0x000000, if th.dark { 0.40 } else { 0.14 });
        c.fill_circle(x, cy, tr, outer, 1.0);
        if !th.dark {
            c.stroke_circle(x, cy, tr, 1.0, mix(th.line, th.text, 0.3), 1.0);
        }
        let inner = if dragging {
            0.42
        } else if state.hover {
            0.58
        } else {
            0.50
        };
        c.fill_circle(x, cy, tr * inner, th.accent, 1.0);
    } else {
        c.fill_circle(x, cy, tr * 0.75, mix(th.text_dim, th.bg, 0.55), 1.0);
    }
    if state.focus {
        focus_ring(
            c,
            ctx,
            R::new(
                (x - tr) as i32,
                (cy - tr) as i32,
                (x + tr) as i32,
                (cy + tr) as i32,
            ),
            tr,
        );
    }
}

pub fn draw_dropdown(c: &mut Canvas, ctx: &Ctx, r: R, text: &str, state: WState, open: bool) {
    let th = &ctx.th;
    let rad = scf(design::RADIUS_CONTROL, ctx.dpi);
    let fill = if !state.enabled {
        mix(th.surface, th.bg, 0.5)
    } else if state.pressed || open {
        th.pressed_fill()
    } else if state.hover {
        th.hover_fill()
    } else {
        th.surface
    };
    c.fill_rrect(r, rad, fill, 1.0);
    let border = if state.hover && state.enabled {
        mix(th.line, th.text, 0.18)
    } else {
        th.line
    };
    c.stroke_rrect(r, rad, 1.0, border, 1.0);
    let pad = sc(12, ctx.dpi);
    let txt = if state.enabled {
        th.text
    } else {
        disabled(ctx, th.text)
    };
    c.text(
        R::new(r.l + pad, r.t, r.r - sc(34, ctx.dpi), r.b),
        text,
        ctx.fonts.body,
        txt,
        TA_LEFT,
    );
    // chevron
    let (cx, cy) = (r.r as f32 - scf(18.0, ctx.dpi), r.cy());
    let s = scf(4.0, ctx.dpi);
    let col = if state.enabled {
        th.text_dim
    } else {
        disabled(ctx, th.text_dim)
    };
    let (y0, y1) = if open {
        (cy + s * 0.5, cy - s * 0.5)
    } else {
        (cy - s * 0.5, cy + s * 0.5)
    };
    c.polyline(
        &[(cx - s, y0), (cx, y1), (cx + s, y0)],
        false,
        scf(1.5, ctx.dpi),
        col,
        1.0,
    );
    if state.focus {
        focus_ring(c, ctx, r, rad);
    }
}
