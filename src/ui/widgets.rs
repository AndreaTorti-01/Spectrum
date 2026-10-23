//! Custom-drawn controls (Button, Toggle, Checkbox, Slider, Dropdown field + popup list) with a small
//! retained widget list for hit-testing and hover/pressed/focus state, plus the slider value mappings.

#![allow(clippy::too_many_arguments, clippy::not_unsafe_ptr_arg_deref)]

use super::design;
use super::draw::{
    apply_window_theme, colorref, mix, sc, scf, window_dpi, Canvas, Fonts, Theme, R, TA_CENTER, TA_LEFT,
};
use crate::util::wide;
use std::cell::RefCell;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromWindow, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SetFocus, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SPACE,
    VK_TAB, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, LoadCursorW, PostMessageW,
    RegisterClassExW, SetWindowPos, ShowWindow, CS_DROPSHADOW, IDC_ARROW, SWP_NOACTIVATE, SW_SHOW,
    WM_ACTIVATE, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT, WNDCLASSEXW,
    WS_EX_TOOLWINDOW, WS_POPUP,
};

// ---------------------------------------------------------------------------------------------
// Widget list
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Button,
    TextButton,
    Toggle,
    Checkbox,
    Slider,
    Dropdown,
    /// Non-interactive-looking hit area drawn by the owner (monitor tiles).
    Area,
}

#[derive(Clone, Copy, Debug)]
pub struct Widget<I> {
    pub id: I,
    pub kind: Kind,
    pub rect: R,
    pub enabled: bool,
    pub focusable: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WState {
    pub hover: bool,
    pub pressed: bool,
    pub focus: bool,
    pub enabled: bool,
}

pub struct WidgetList<I: Copy + PartialEq> {
    items: Vec<Widget<I>>,
    pub hover: Option<I>,
    pub pressed: Option<I>,
    pub focus: Option<I>,
    pub focus_visible: bool,
}

impl<I: Copy + PartialEq> Default for WidgetList<I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: Copy + PartialEq> WidgetList<I> {
    pub fn new() -> Self {
        WidgetList {
            items: Vec::new(),
            hover: None,
            pressed: None,
            focus: None,
            focus_visible: false,
        }
    }

    /// Drops all widgets (interaction state is kept by id).
    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn add(&mut self, id: I, kind: Kind, rect: R, enabled: bool, focusable: bool) {
        self.items.push(Widget {
            id,
            kind,
            rect,
            enabled,
            focusable,
        });
    }

    pub fn get(&self, id: I) -> Option<&Widget<I>> {
        self.items.iter().find(|w| w.id == id)
    }

    pub fn items(&self) -> &[Widget<I>] {
        &self.items
    }

    /// Topmost widget at the point (later additions win).
    pub fn hit(&self, x: i32, y: i32) -> Option<I> {
        self.items
            .iter()
            .rev()
            .find(|w| w.rect.contains(x, y))
            .map(|w| w.id)
    }

    pub fn state(&self, id: I) -> WState {
        WState {
            hover: self.hover == Some(id),
            pressed: self.pressed == Some(id),
            focus: self.focus == Some(id) && self.focus_visible,
            enabled: self.get(id).map(|w| w.enabled).unwrap_or(false),
        }
    }

    /// Moves keyboard focus to the next/previous enabled focusable widget (wraps).
    pub fn focus_next(&mut self, backwards: bool) -> Option<I> {
        let order: Vec<I> = self
            .items
            .iter()
            .filter(|w| w.focusable && w.enabled)
            .map(|w| w.id)
            .collect();
        if order.is_empty() {
            self.focus = None;
            return None;
        }
        let cur = self.focus.and_then(|f| order.iter().position(|&i| i == f));
        let next = match (cur, backwards) {
            (None, false) => 0,
            (None, true) => order.len() - 1,
            (Some(i), false) => (i + 1) % order.len(),
            (Some(i), true) => (i + order.len() - 1) % order.len(),
        };
        self.focus = Some(order[next]);
        self.focus_visible = true;
        self.focus
    }
}

// ---------------------------------------------------------------------------------------------
// Slider math
// ---------------------------------------------------------------------------------------------

/// Mapping between a normalized slider position `t` in 0..=1 and a value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scale {
    /// `value = max * t^2` (fine control near zero).
    Quadratic {
        max: f64,
    },
    /// `value = min * (max/min)^t`.
    Log {
        min: f64,
        max: f64,
    },
    Linear {
        min: f64,
        max: f64,
    },
}

impl Scale {
    pub fn to_value(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match *self {
            Scale::Quadratic { max } => max * t * t,
            Scale::Log { min, max } => {
                if max <= min {
                    min
                } else {
                    min * (max / min).powf(t)
                }
            }
            Scale::Linear { min, max } => min + (max - min) * t,
        }
    }

    pub fn to_t(&self, v: f64) -> f64 {
        match *self {
            Scale::Quadratic { max } => (v / max).clamp(0.0, 1.0).sqrt(),
            Scale::Log { min, max } => {
                if max <= min {
                    0.0
                } else {
                    ((v.clamp(min, max) / min).ln() / (max / min).ln()).clamp(0.0, 1.0)
                }
            }
            Scale::Linear { min, max } => {
                if max <= min {
                    0.0
                } else {
                    ((v - min) / (max - min)).clamp(0.0, 1.0)
                }
            }
        }
    }
}

pub const BLACK_MAX_NITS: f64 = 0.5;
pub const PEAK_MIN_NITS: f64 = 100.0;
pub const PEAK_MAX_NITS: f64 = 4000.0;

pub fn black_scale() -> Scale {
    Scale::Quadratic { max: BLACK_MAX_NITS }
}
pub fn peak_scale() -> Scale {
    Scale::Log {
        min: PEAK_MIN_NITS,
        max: PEAK_MAX_NITS,
    }
}
/// Full-frame uses the same fixed 100 to 4000 range as Peak, so moving Peak never rescales it.
pub fn full_scale() -> Scale {
    peak_scale()
}

/// New (peak, full-frame) when Peak moves to `peak`: a lower peak pushes Full-frame down with it.
pub fn move_peak(peak: f64, full: f64) -> (f64, f64) {
    (peak, full.min(peak))
}

/// New full-frame when it is dragged to `full`: it stops at the current peak.
pub fn move_full(full: f64, peak: f64) -> f64 {
    full.min(peak)
}

/// The SDR content value the peak tick sits on: the highest 4-nit step not above the peak. `None` when the
/// peak is outside the slider's range.
pub fn sdr_peak_target(peak: f64) -> Option<f64> {
    use crate::monitors::{SDR_WHITE_MAX_NITS as MAX, SDR_WHITE_MIN_NITS as MIN};
    if !(MIN..MAX).contains(&peak.round()) {
        return None;
    }
    Some(MIN + ((peak.round() - MIN) / 4.0).floor() * 4.0)
}
/// Windows "SDR content brightness": 80 nits up to `max` (the EDID cap, or 480).
pub fn white_scale(max: f64) -> Scale {
    Scale::Linear {
        min: crate::monitors::SDR_WHITE_MIN_NITS,
        max,
    }
}

/// Slider value snapped to the 4-nit grid Windows uses.
pub fn round_sdr_white_nits(v: f64) -> f64 {
    crate::monitors::round_sdr_white_nits(v)
}

pub fn sat_scale() -> Scale {
    Scale::Linear { min: 0.0, max: 100.0 }
}

/// Black level shown/applied in 0.01-nit steps.
pub fn round_black(v: f64) -> f64 {
    ((v * 100.0).round() / 100.0).clamp(0.0, BLACK_MAX_NITS)
}

/// Luminance shown/applied in 5-nit steps (10 above 1000), clamped to `lo..=hi`.
pub fn round_nits(v: f64, lo: f64, hi: f64) -> f64 {
    let step = if v >= 1000.0 { 10.0 } else { 5.0 };
    ((v / step).round() * step).clamp(lo, hi.max(lo))
}

/// New `t` for a slider key press, or None when the key is not a slider key.
pub fn key_to_t(t: f64, vk: u16) -> Option<f64> {
    const STEP: f64 = 0.02;
    const PAGE: f64 = 0.1;
    let n = match vk {
        VK_LEFT | VK_DOWN => t - STEP,
        VK_RIGHT | VK_UP => t + STEP,
        VK_PRIOR => t + PAGE,
        VK_NEXT => t - PAGE,
        VK_HOME => 0.0,
        VK_END => 1.0,
        _ => return None,
    };
    Some(n.clamp(0.0, 1.0))
}

/// Horizontal travel of the thumb center for a slider occupying `r`.
pub fn slider_span(r: R, dpi: u32) -> (f32, f32) {
    let pad = scf(THUMB_R, dpi) + 1.0;
    (r.l as f32 + pad, r.r as f32 - pad)
}

pub fn slider_t_at(r: R, x: i32, dpi: u32) -> f64 {
    let (a, b) = slider_span(r, dpi);
    (((x as f32 - a) / (b - a).max(1.0)) as f64).clamp(0.0, 1.0)
}

const THUMB_R: f32 = 9.0;

// ---------------------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------------------

pub struct Ctx<'a> {
    pub th: Theme,
    pub f: &'a Fonts,
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

fn dis(ctx: &Ctx, c: u32) -> u32 {
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

pub fn draw_button(c: &mut Canvas, ctx: &Ctx, r: R, label: &str, st: WState, style: BtnStyle) {
    let th = &ctx.th;
    let rad = scf(design::RADIUS_CONTROL, ctx.dpi);
    let filled = |base: u32| -> u32 {
        if !st.enabled {
            mix(base, th.bg, 0.6)
        } else if st.pressed && st.hover {
            mix(base, 0x000000, 0.2)
        } else if st.hover {
            mix(base, 0xFFFFFF, 0.14)
        } else {
            base
        }
    };
    let outlined_fill = if !st.enabled {
        mix(th.surface, th.bg, 0.5)
    } else if st.pressed && st.hover {
        th.pressed_fill()
    } else if st.hover {
        th.hover_fill()
    } else {
        th.surface
    };
    let outlined_border = if st.hover && st.enabled {
        mix(th.line, th.text, 0.18)
    } else {
        th.line
    };
    let (fill, border, txt) = match style {
        BtnStyle::Secondary => (
            outlined_fill,
            Some(outlined_border),
            if st.enabled { th.text } else { dis(ctx, th.text) },
        ),
        BtnStyle::Danger => {
            let d = design::danger(th);
            (
                outlined_fill,
                Some(if st.hover && st.enabled {
                    mix(th.line, d, 0.6)
                } else {
                    outlined_border
                }),
                if st.enabled { d } else { dis(ctx, th.text) },
            )
        }
        BtnStyle::Primary => (filled(th.accent), None, th.on_accent()),
        BtnStyle::DangerConfirm => {
            let d = if th.dark { 0xC42B1C } else { design::danger(th) };
            (filled(d), None, 0xFFFFFF)
        }
    };
    let fill = Some(fill);
    if let Some(f) = fill {
        c.fill_rrect(r, rad, f, 1.0);
    }
    if let Some(b) = border {
        c.stroke_rrect(r, rad, 1.0_f32.max(scf(1.0, ctx.dpi).floor()), b, 1.0);
    }
    c.text(r, label, ctx.f.body, txt, TA_CENTER);
    if st.focus {
        focus_ring(c, ctx, r, rad);
    }
}

/// A full-width disclosure row: chevron, label, hover fill. Opens and closes a group of advanced controls.
pub fn draw_expander(c: &mut Canvas, ctx: &Ctx, r: R, label: &str, open: bool, st: WState) {
    let th = &ctx.th;
    let rad = scf(design::RADIUS_CONTROL, ctx.dpi);
    if st.enabled && (st.hover || st.pressed) {
        let f = if st.pressed {
            th.pressed_fill()
        } else {
            th.hover_fill()
        };
        c.fill_rrect(r, rad, f, 1.0);
    }
    let col = if st.enabled { th.text } else { dis(ctx, th.text) };
    let s = scf(5.0, ctx.dpi);
    let (cx, cy) = (r.l as f32 + scf(12.0, ctx.dpi), r.cy());
    let pts = if open {
        [(cx - s, cy - s * 0.5), (cx, cy + s * 0.5), (cx + s, cy - s * 0.5)]
    } else {
        [(cx - s * 0.5, cy - s), (cx + s * 0.5, cy), (cx - s * 0.5, cy + s)]
    };
    c.polyline(&pts, false, scf(1.6, ctx.dpi), col, 1.0);
    let tr = R::new(r.l + sc(28, ctx.dpi), r.t, r.r, r.b);
    c.text(tr, label, ctx.f.strong, col, TA_LEFT);
    if st.focus {
        focus_ring(c, ctx, r, rad);
    }
}

pub fn draw_toggle(c: &mut Canvas, ctx: &Ctx, r: R, on: bool, st: WState) {
    let th = &ctx.th;
    let h = r.h() as f32;
    let rad = h / 2.0;
    let (l, t) = (r.l as f32, r.t as f32);
    let bw = scf(1.5, ctx.dpi);
    if on {
        let fill = if !st.enabled {
            mix(th.accent, th.bg, 0.6)
        } else if st.hover {
            mix(th.accent, 0xFFFFFF, 0.1)
        } else {
            th.accent
        };
        c.fill_rrect_f(l, t, r.r as f32, r.b as f32, rad, fill, 1.0);
        let kr = rad - scf(4.5, ctx.dpi) + if st.hover { scf(0.8, ctx.dpi) } else { 0.0 };
        let knob = if st.enabled {
            th.on_accent()
        } else {
            dis(ctx, th.on_accent())
        };
        c.fill_circle(r.r as f32 - rad, t + rad, kr, knob, 1.0);
    } else {
        let col = if st.enabled {
            mix(th.text_dim, th.text, if st.hover { 0.4 } else { 0.0 })
        } else {
            dis(ctx, th.text_dim)
        };
        c.stroke_rrect_f(l, t, r.r as f32, r.b as f32, rad, bw, col, 1.0);
        let kr = rad - scf(6.0, ctx.dpi) + if st.hover { scf(0.8, ctx.dpi) } else { 0.0 };
        c.fill_circle(l + rad, t + rad, kr, col, 1.0);
    }
    if st.focus {
        focus_ring(c, ctx, r, rad);
    }
}

pub fn draw_checkbox(c: &mut Canvas, ctx: &Ctx, r: R, label: &str, checked: bool, st: WState) {
    let th = &ctx.th;
    let b = sc(design::CHECK, ctx.dpi);
    let bx = R::xywh(r.l, r.t + (r.h() - b) / 2, b, b);
    let rad = scf(design::RADIUS_CONTROL, ctx.dpi);
    let txt = if st.enabled { th.text } else { dis(ctx, th.text) };
    if checked {
        let fill = if !st.enabled {
            mix(th.accent, th.bg, 0.6)
        } else if st.pressed {
            mix(th.accent, 0x000000, 0.2)
        } else if st.hover {
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
        let f = if st.pressed {
            th.pressed_fill()
        } else if st.hover {
            th.hover_fill()
        } else {
            th.surface
        };
        c.fill_rrect(bx, rad, f, 1.0);
        let bc = if st.enabled {
            mix(th.text_dim, th.text, if st.hover { 0.35 } else { 0.0 })
        } else {
            dis(ctx, th.text_dim)
        };
        c.stroke_rrect(bx, rad, scf(1.2, ctx.dpi), bc, 1.0);
    }
    let tr = R::new(bx.r + sc(10, ctx.dpi), r.t, r.r, r.b);
    c.text(tr, label, ctx.f.body, txt, TA_LEFT);
    if st.focus {
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
pub fn draw_slider(c: &mut Canvas, ctx: &Ctx, r: R, t: f64, st: WState, dragging: bool) {
    let th = &ctx.th;
    let (a, b) = slider_span(r, ctx.dpi);
    let cy = r.cy();
    let tw = scf(4.0, ctx.dpi);
    let x = a + (b - a) * t as f32;
    let track = if st.enabled {
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
    if st.enabled {
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
    if st.enabled {
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
        } else if st.hover {
            0.58
        } else {
            0.50
        };
        c.fill_circle(x, cy, tr * inner, th.accent, 1.0);
    } else {
        c.fill_circle(x, cy, tr * 0.75, mix(th.text_dim, th.bg, 0.55), 1.0);
    }
    if st.focus {
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

pub fn draw_dropdown(c: &mut Canvas, ctx: &Ctx, r: R, text: &str, st: WState, open: bool) {
    let th = &ctx.th;
    let rad = scf(design::RADIUS_CONTROL, ctx.dpi);
    let fill = if !st.enabled {
        mix(th.surface, th.bg, 0.5)
    } else if st.pressed || open {
        th.pressed_fill()
    } else if st.hover {
        th.hover_fill()
    } else {
        th.surface
    };
    c.fill_rrect(r, rad, fill, 1.0);
    let border = if st.hover && st.enabled {
        mix(th.line, th.text, 0.18)
    } else {
        th.line
    };
    c.stroke_rrect(r, rad, 1.0, border, 1.0);
    let pad = sc(12, ctx.dpi);
    let txt = if st.enabled { th.text } else { dis(ctx, th.text) };
    c.text(
        R::new(r.l + pad, r.t, r.r - sc(34, ctx.dpi), r.b),
        text,
        ctx.f.body,
        txt,
        TA_LEFT,
    );
    // chevron
    let (cx, cy) = (r.r as f32 - scf(18.0, ctx.dpi), r.cy());
    let s = scf(4.0, ctx.dpi);
    let col = if st.enabled {
        th.text_dim
    } else {
        dis(ctx, th.text_dim)
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
    if st.focus {
        focus_ring(c, ctx, r, rad);
    }
}

// ---------------------------------------------------------------------------------------------
// Dropdown popup list
// ---------------------------------------------------------------------------------------------

struct Pop {
    owner: HWND,
    msg: u32,
    items: Vec<String>,
    sel: Option<usize>,
    hover: Option<usize>,
    th: Theme,
    dpi: u32,
    fonts: Fonts,
    canvas: Option<Canvas>,
    row_h: i32,
    pad: i32,
    done: bool,
}

thread_local! {
    static POP: RefCell<Option<Pop>> = const { RefCell::new(None) };
}

const POP_CLASS: &str = "SpectrumDropdownList";

/// Value of `wParam` posted to the owner when the popup was dismissed without a choice.
pub const DROPDOWN_DISMISSED: usize = usize::MAX;

pub fn dropdown_is_open() -> bool {
    POP.with(|p| p.try_borrow().map(|b| b.is_some()).unwrap_or(true))
}

/// Opens the list popup under (or above) `anchor` (screen coordinates). On close the owner receives `msg`
/// with `wParam` = chosen index or `DROPDOWN_DISMISSED`.
pub fn open_dropdown(owner: HWND, anchor: R, items: Vec<String>, sel: Option<usize>, th: Theme, msg: u32) {
    if items.is_empty() || dropdown_is_open() {
        return;
    }
    let dpi = window_dpi(owner);
    let row_h = sc(34, dpi);
    let margin = sc(4, dpi);
    let h = row_h * items.len() as i32 + margin * 2;
    let w = anchor.w();
    // SAFETY: standard window creation; state is installed before the window can receive messages
    // that read it (WM_PAINT arrives only after ShowWindow below).
    unsafe {
        register_pop_class();
        let hinst = GetModuleHandleW(std::ptr::null());
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST), &mut mi);
        let mut y = anchor.b + sc(2, dpi);
        if y + h > mi.rcWork.bottom && anchor.t - h - sc(2, dpi) >= mi.rcWork.top {
            y = anchor.t - h - sc(2, dpi);
        }
        POP.with(|p| {
            *p.borrow_mut() = Some(Pop {
                owner,
                msg,
                items,
                sel,
                hover: sel,
                th,
                dpi,
                fonts: Fonts::new(dpi),
                canvas: None,
                row_h,
                pad: margin,
                done: false,
            })
        });
        let cls = wide(POP_CLASS);
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            cls.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            anchor.l,
            y,
            w,
            h,
            owner,
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            POP.with(|p| *p.borrow_mut() = None);
            return;
        }
        apply_window_theme(hwnd, th.dark);
        SetWindowPos(hwnd, std::ptr::null_mut(), anchor.l, y, w, h, SWP_NOACTIVATE);
        ShowWindow(hwnd, SW_SHOW);
        SetFocus(hwnd);
    }
}

fn register_pop_class() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let cls = wide(POP_CLASS);
        // SAFETY: class data outlives the call; wndproc has the right signature.
        unsafe {
            let mut wc: WNDCLASSEXW = std::mem::zeroed();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.style = CS_DROPSHADOW;
            wc.lpfnWndProc = Some(pop_proc);
            wc.hInstance = GetModuleHandleW(std::ptr::null());
            wc.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
            wc.lpszClassName = cls.as_ptr();
            RegisterClassExW(&wc);
        }
    });
}

fn pop_finish(hwnd: HWND, pick: Option<usize>) {
    let info = POP.with(|p| {
        let mut b = match p.try_borrow_mut() {
            Ok(b) => b,
            Err(_) => return None,
        };
        match b.as_mut() {
            Some(s) if !s.done => {
                s.done = true;
                Some((s.owner, s.msg))
            }
            _ => None,
        }
    });
    if let Some((owner, msg)) = info {
        // SAFETY: valid window handles; PostMessage never blocks.
        unsafe {
            DestroyWindow(hwnd);
            PostMessageW(owner, msg, pick.unwrap_or(DROPDOWN_DISMISSED), 0);
        }
    }
}

fn pop_item_at(s: &Pop, y: i32) -> Option<usize> {
    let i = (y - s.pad).div_euclid(s.row_h);
    if y >= s.pad && i >= 0 && (i as usize) < s.items.len() {
        Some(i as usize)
    } else {
        None
    }
}

unsafe extern "system" fn pop_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            POP.with(|p| {
                if let Ok(mut b) = p.try_borrow_mut() {
                    if let Some(s) = b.as_mut() {
                        let mut rc: RECT = std::mem::zeroed();
                        GetClientRect(hwnd, &mut rc);
                        let (w, h) = (rc.right, rc.bottom);
                        if s.canvas.as_ref().map(|c| (c.width(), c.height())) != Some((w, h)) {
                            s.canvas = Canvas::new(w, h);
                        }
                        let (th, dpi, row_h, pad) = (s.th, s.dpi, s.row_h, s.pad);
                        if let Some(cv) = s.canvas.as_mut() {
                            cv.clear(th.surface);
                            for (i, it) in s.items.iter().enumerate() {
                                let r = R::xywh(pad, pad + i as i32 * row_h, w - pad * 2, row_h);
                                if s.hover == Some(i) {
                                    cv.fill_rrect(r, scf(4.0, dpi), th.hover_fill(), 1.0);
                                    cv.fill_rrect(r, scf(4.0, dpi), th.text, 0.04);
                                }
                                if s.sel == Some(i) {
                                    let bar = R::xywh(
                                        r.l + sc(2, dpi),
                                        r.t + sc(9, dpi),
                                        sc(3, dpi),
                                        r.h() - sc(18, dpi),
                                    );
                                    cv.fill_rrect(bar, scf(1.5, dpi), th.accent, 1.0);
                                }
                                let tr = R::new(r.l + sc(14, dpi), r.t, r.r - sc(8, dpi), r.b);
                                cv.text(tr, it, s.fonts.body, th.text, TA_LEFT);
                            }
                            cv.stroke_rrect(R::new(0, 0, w, h), scf(8.0, dpi), 1.0, th.line, 1.0);
                            cv.present(hdc, 0, 0);
                        }
                    }
                }
            });
            EndPaint(hwnd, &ps);
            0
        }
        WM_MOUSEMOVE => {
            let y = ((lp >> 16) & 0xFFFF) as i16 as i32;
            let changed = POP.with(|p| {
                let Ok(mut b) = p.try_borrow_mut() else {
                    return false;
                };
                if let Some(s) = b.as_mut() {
                    let h = pop_item_at(s, y);
                    if h != s.hover {
                        s.hover = h;
                        return true;
                    }
                }
                false
            });
            if changed {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_LBUTTONUP => {
            let y = ((lp >> 16) & 0xFFFF) as i16 as i32;
            let pick = POP.with(|p| {
                p.try_borrow()
                    .ok()
                    .and_then(|b| b.as_ref().and_then(|s| pop_item_at(s, y)))
            });
            if pick.is_some() {
                pop_finish(hwnd, pick);
            }
            0
        }
        WM_KEYDOWN => {
            let vk = wp as u16;
            let mut finish: Option<Option<usize>> = None;
            let changed = POP.with(|p| {
                let Ok(mut b) = p.try_borrow_mut() else {
                    return false;
                };
                let Some(s) = b.as_mut() else { return false };
                let n = s.items.len();
                match vk {
                    VK_ESCAPE => {
                        finish = Some(None);
                        false
                    }
                    VK_RETURN | VK_SPACE => {
                        finish = Some(s.hover);
                        false
                    }
                    VK_DOWN => {
                        s.hover = Some(s.hover.map(|h| (h + 1) % n).unwrap_or(0));
                        true
                    }
                    VK_UP => {
                        s.hover = Some(s.hover.map(|h| (h + n - 1) % n).unwrap_or(n - 1));
                        true
                    }
                    VK_HOME => {
                        s.hover = Some(0);
                        true
                    }
                    VK_END => {
                        s.hover = Some(n - 1);
                        true
                    }
                    VK_TAB => {
                        finish = Some(None);
                        false
                    }
                    _ => false,
                }
            });
            if let Some(f) = finish {
                pop_finish(hwnd, f);
            } else if changed {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_ACTIVATE => {
            if (wp & 0xFFFF) == 0 {
                pop_finish(hwnd, None);
            }
            0
        }
        WM_NCDESTROY => {
            POP.with(|p| {
                if let Ok(mut b) = p.try_borrow_mut() {
                    *b = None;
                }
            });
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

// keep `colorref` linked for downstream modules that want to build GDI brushes
#[allow(dead_code)]
pub(crate) fn _colorref(c: u32) -> u32 {
    colorref(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn black_is_quadratic() {
        let s = black_scale();
        assert!(close(s.to_value(0.0), 0.0));
        assert!(close(s.to_value(1.0), 0.5));
        assert!(close(s.to_value(0.5), 0.125));
        assert!(close(s.to_t(0.125), 0.5));
    }

    #[test]
    fn peak_is_log() {
        let s = peak_scale();
        assert!(close(s.to_value(0.0), 100.0));
        assert!((s.to_value(1.0) - 4000.0).abs() < 1e-6);
        assert!((s.to_value(0.5) - (100.0f64 * 4000.0).sqrt()).abs() < 1e-6);
        assert!((s.to_t(1000.0) - (10.0f64).ln() / (40.0f64).ln()).abs() < 1e-9);
    }

    #[test]
    fn full_frame_keeps_its_range_and_stops_at_peak() {
        assert_eq!(full_scale(), peak_scale());
        // dragging full-frame past the peak stops at the peak's position
        assert!(close(move_full(800.0, 600.0), 600.0));
        assert!(close(move_full(300.0, 600.0), 300.0));
        assert!(close(
            full_scale().to_t(move_full(800.0, 600.0)),
            peak_scale().to_t(600.0)
        ));
        // lowering the peak below full-frame pushes it down; raising it leaves full-frame alone
        assert_eq!(move_peak(250.0, 400.0), (250.0, 250.0));
        assert_eq!(move_peak(1000.0, 400.0), (1000.0, 400.0));
    }

    #[test]
    fn sdr_peak_tick_sits_on_the_grid() {
        assert_eq!(sdr_peak_target(455.0), Some(452.0));
        assert_eq!(sdr_peak_target(452.0), Some(452.0));
        assert_eq!(sdr_peak_target(1000.0), None);
        assert_eq!(sdr_peak_target(60.0), None);
    }

    #[test]
    fn saturation_is_linear() {
        let s = sat_scale();
        assert!(close(s.to_value(0.25), 25.0));
        assert!(close(s.to_t(75.0), 0.75));
    }

    #[test]
    fn roundtrip_all_scales() {
        let scales = [black_scale(), peak_scale(), full_scale(), sat_scale()];
        for s in scales {
            for i in 0..=20 {
                let t = i as f64 / 20.0;
                assert!((s.to_t(s.to_value(t)) - t).abs() < 1e-9, "{s:?} t={t}");
            }
        }
    }

    #[test]
    fn rounding() {
        assert!(close(round_black(0.123), 0.12));
        assert!(close(round_black(0.9), 0.5));
        assert!(close(round_nits(632.5, 100.0, 4000.0), 635.0));
        assert!(close(round_nits(1234.0, 100.0, 4000.0), 1230.0));
        assert!(close(round_nits(5000.0, 100.0, 4000.0), 4000.0));
        assert!(close(round_nits(3.0, 100.0, 4000.0), 100.0));
    }

    #[test]
    fn keys_move_slider() {
        assert!(close(key_to_t(0.5, VK_RIGHT).unwrap(), 0.52));
        assert!(close(key_to_t(0.5, VK_NEXT).unwrap(), 0.4));
        assert!(close(key_to_t(0.5, VK_HOME).unwrap(), 0.0));
        assert!(close(key_to_t(0.99, VK_PRIOR).unwrap(), 1.0));
        assert!(key_to_t(0.5, 0x41).is_none());
    }

    #[test]
    fn slider_hit_math() {
        let r = R::xywh(100, 0, 200, 24);
        assert!(close(slider_t_at(r, 0, 96), 0.0));
        assert!(close(slider_t_at(r, 1000, 96), 1.0));
        let mid = slider_t_at(r, 200, 96);
        assert!((mid - 0.5).abs() < 0.02);
    }

    #[test]
    fn widget_list_hit_and_focus() {
        let mut w: WidgetList<u8> = WidgetList::new();
        w.add(1, Kind::Button, R::xywh(0, 0, 10, 10), true, true);
        w.add(2, Kind::Button, R::xywh(20, 0, 10, 10), false, true);
        w.add(3, Kind::Slider, R::xywh(40, 0, 10, 10), true, true);
        assert_eq!(w.hit(5, 5), Some(1));
        assert_eq!(w.hit(25, 5), Some(2));
        assert_eq!(w.hit(500, 5), None);
        assert_eq!(w.focus_next(false), Some(1));
        assert_eq!(w.focus_next(false), Some(3)); // 2 is disabled
        assert_eq!(w.focus_next(false), Some(1));
        assert_eq!(w.focus_next(true), Some(3));
    }
}
