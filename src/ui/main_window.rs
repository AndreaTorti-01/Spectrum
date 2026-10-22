//! The main window: monitor map, SDR profile picker, HDR sliders. Destroyed on close to free memory.

#![allow(clippy::too_many_arguments)]

use super::draw::{
    apply_window_theme, invalidate_theme, mix, sc, scf, system_dpi, theme, trim_working_set, window_dpi,
    Canvas, Fonts, Theme, R, TA_CENTER, TA_LEFT, TA_RIGHT, WM_MOUSELEAVE,
};
use super::widgets::{
    self, black_scale, draw_button, draw_checkbox, draw_dropdown, draw_slider, draw_toggle, full_scale,
    key_to_t, peak_scale, round_black, round_nits, round_sdr_white_nits, sat_scale, slider_t_at, white_scale,
    BtnStyle, Ctx, Kind, WidgetList, DROPDOWN_DISMISSED,
};
use crate::app::{UiAction, UiMonitor, UiState};
use crate::icc_write::HdrParams;
use crate::util::wide;
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::AdjustWindowRectExForDpi;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_DOWN,
    VK_ESCAPE, VK_RETURN, VK_SHIFT, VK_SPACE, VK_TAB,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, GetClientRect, GetCursorPos, IsIconic,
    KillTimer, LoadCursorW, RegisterClassExW, SendMessageW, SetCursor, SetForegroundWindow, SetTimer,
    SetWindowPos, ShowWindow, SystemParametersInfoW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, HICON, ICON_BIG,
    ICON_SMALL, IDC_ARROW, IDC_HAND, SM_CXICON, SM_CXSMICON, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOZORDER,
    SW_RESTORE, SW_SHOW, WM_ACTIVATE, WM_APP, WM_CAPTURECHANGED, WM_CLOSE, WM_CREATE, WM_DESTROY,
    WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT, WM_SETCURSOR, WM_SETFOCUS, WM_SETICON, WM_SETTINGCHANGE, WM_TIMER,
    WNDCLASSEXW, WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
};

const CLASS: &str = "SpectrumMainWindow";
const CLIENT_W: i32 = 440;
const CLIENT_H: i32 = 700;
const MARGIN: i32 = 20;
const WM_DROPDOWN_DONE: u32 = WM_APP + 10;
const TIMER_COMMIT: usize = 1;
const COMMIT_DELAY_MS: u32 = 400;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Id {
    Tile(usize),
    Identify,
    Sdr,
    Read,
    Compare,
    Manage,
    HdrToggle,
    Black,
    Peak,
    Full,
    Sat,
    SdrWhite,
    Reset,
    Autostart,
}

const SLIDERS: [Id; 5] = [Id::Black, Id::Peak, Id::Full, Id::Sat, Id::SdrWhite];

enum Pending {
    Dispatch(UiAction),
    OpenDropdown,
    Close,
}

struct Main {
    hwnd: HWND,
    dpi: u32,
    fonts: Fonts,
    canvas: Option<Canvas>,
    ui: WidgetList<Id>,
    st: UiState,
    sel: usize,
    /// Raw (unrounded) HDR edit values: nits / nits / nits / percent.
    black: f64,
    peak: f64,
    full: f64,
    sat: f64,
    /// Windows "SDR content brightness" in nits. Not part of the MHC2 profile; applied through DisplayConfig.
    white: f64,
    white_dirty: bool,
    drag: Option<Id>,
    dirty: bool,
    pending: Vec<Pending>,
    tracking: bool,
    highlight: Option<usize>,
    dd_closed_at: Option<Instant>,
    icons: [HICON; 2],
    geo: Geo,
}

thread_local! {
    static MAIN: RefCell<Option<Main>> = const { RefCell::new(None) };
    static HWND_CELL: Cell<isize> = const { Cell::new(0) };
    /// Where the next `open` puts the window's top-left corner (screenshots); `None` centers it.
    static OPEN_AT: Cell<Option<(i32, i32)>> = const { Cell::new(None) };
}

fn with<T>(f: impl FnOnce(&mut Main) -> T) -> Option<T> {
    MAIN.with(|m| m.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

/// Fixed (non-monitor-dependent) layout in physical pixels.
#[derive(Clone, Copy, Default)]
struct Geo {
    map: R,
    identify: R,
    info: R,
    sdr_label: R,
    sdr: R,
    read: R,
    compare: R,
    manage: R,
    div1: i32,
    hdr_head: R,
    hdr_hint: R,
    toggle: R,
    /// (label, slider, value) per slider row
    rows: [(R, R, R); 5],
    reset: R,
    div2: i32,
    autostart: R,
    status: R,
}

fn geo(dpi: u32) -> Geo {
    let s = |v: i32| sc(v, dpi);
    let right = CLIENT_W - MARGIN;
    let mut rows = [(R::default(), R::default(), R::default()); 5];
    for (i, row) in rows.iter_mut().enumerate() {
        let y = 398 + i as i32 * 38;
        *row = (
            R::new(s(MARGIN), s(y), s(134), s(y + 38)),
            R::new(s(138), s(y + 5), s(322), s(y + 33)),
            R::new(s(326), s(y), s(right), s(y + 38)),
        );
    }
    Geo {
        map: R::new(s(MARGIN), s(16), s(330), s(128)),
        identify: R::new(s(right - 70), s(14), s(right), s(40)),
        info: R::new(s(MARGIN), s(134), s(right), s(158)),
        sdr_label: R::new(s(MARGIN), s(170), s(122), s(204)),
        sdr: R::new(s(126), s(170), s(right), s(204)),
        read: R::new(s(MARGIN), s(216), s(right), s(250)),
        compare: R::new(s(MARGIN), s(258), s(right), s(292)),
        manage: R::new(s(MARGIN), s(300), s(right), s(334)),
        div1: s(348),
        hdr_head: R::new(s(MARGIN), s(358), s(120), s(390)),
        hdr_hint: R::new(s(120), s(358), s(right - 58), s(390)),
        toggle: R::new(s(right - 44), s(363), s(right), s(385)),
        rows,
        reset: R::new(s(MARGIN), s(594), s(MARGIN + 92), s(622)),
        div2: s(632),
        autostart: R::new(s(MARGIN), s(642), s(right), s(666)),
        status: R::new(s(MARGIN), s(672), s(right), s(692)),
    }
}

impl Main {
    fn mon(&self) -> Option<&UiMonitor> {
        self.st.monitors.get(self.sel)
    }

    fn theme(&self) -> Theme {
        let t = theme();
        match self.mon() {
            Some(m) => t.with_accent(m.accent),
            None => t,
        }
    }

    fn hdr_enabled(&self) -> bool {
        self.mon().map(|m| m.hdr_supported && m.hdr_on).unwrap_or(false)
    }

    fn fix_sel(&mut self) {
        if self.sel >= self.st.monitors.len() {
            self.sel = 0;
        }
    }

    /// Pull HDR slider values from the snapshot unless the user is editing.
    fn sync_hdr(&mut self) {
        if self.drag.is_some() || self.dirty || self.white_dirty {
            return;
        }
        if let Some(m) = self.mon() {
            let (h, white) = (m.hdr, m.sdr_white_nits);
            if let Some(n) = white {
                self.white = round_sdr_white_nits(n);
            }
            self.black = h.min_nits.clamp(0.0, widgets::BLACK_MAX_NITS);
            self.peak = h.peak_nits.clamp(widgets::PEAK_MIN_NITS, widgets::PEAK_MAX_NITS);
            self.full = h.full_frame_nits.clamp(widgets::PEAK_MIN_NITS, self.peak);
            self.sat = (h.saturation * 100.0).clamp(0.0, 100.0);
        }
    }

    fn slider_scale(&self, id: Id) -> widgets::Scale {
        match id {
            Id::Black => black_scale(),
            Id::Peak => peak_scale(),
            Id::Full => full_scale(self.peak),
            Id::SdrWhite => white_scale(),
            _ => sat_scale(),
        }
    }

    fn slider_value(&self, id: Id) -> f64 {
        match id {
            Id::Black => self.black,
            Id::Peak => self.peak,
            Id::Full => self.full,
            Id::SdrWhite => self.white,
            _ => self.sat,
        }
    }

    fn set_slider_t(&mut self, id: Id, t: f64) {
        let v = self.slider_scale(id).to_value(t);
        if id == Id::SdrWhite {
            self.white = round_sdr_white_nits(v);
            self.white_dirty = true;
            return;
        }
        match id {
            Id::Black => self.black = v,
            Id::Peak => {
                self.peak = v;
                if self.full > v {
                    self.full = v;
                }
            }
            Id::Full => self.full = v.min(self.peak),
            _ => self.sat = v,
        }
        self.dirty = true;
    }

    fn slider_text(&self, id: Id) -> String {
        match id {
            Id::Black => format!("{:.2} nits", round_black(self.black)),
            Id::Peak => format!("{} nits", round_nits(self.peak, 100.0, 4000.0) as i64),
            Id::Full => {
                let peak = round_nits(self.peak, 100.0, 4000.0);
                format!("{} nits", round_nits(self.full, 100.0, peak) as i64)
            }
            Id::SdrWhite => format!("{} nits", self.white.round() as i64),
            _ => format!("{}%", self.sat.round() as i64),
        }
    }

    fn params(&self) -> HdrParams {
        let peak = round_nits(self.peak, 100.0, 4000.0);
        HdrParams {
            min_nits: round_black(self.black),
            peak_nits: peak,
            full_frame_nits: round_nits(self.full, 100.0, peak),
            saturation: self.sat.round().clamp(0.0, 100.0) / 100.0,
        }
    }

    fn commit(&mut self) {
        if self.white_dirty {
            self.white_dirty = false;
            self.pending.push(Pending::Dispatch(UiAction::SetSdrWhite {
                mon: self.sel,
                nits: self.white,
            }));
        }
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let params = self.params();
        self.pending.push(Pending::Dispatch(UiAction::ApplyHdr {
            mon: self.sel,
            params,
        }));
    }

    fn layout(&mut self) {
        let dpi = self.dpi;
        let g = geo(dpi);
        self.geo = g;
        let mon = self.mon().cloned();
        let hdr_on = self.hdr_enabled();
        self.ui.clear();
        // monitor tiles
        if !self.st.monitors.is_empty() {
            for (i, r) in tile_rects(&self.st.monitors, g.map, dpi).into_iter().enumerate() {
                self.ui.add(Id::Tile(i), Kind::Area, r, true, false);
            }
        }
        self.ui.add(
            Id::Identify,
            Kind::TextButton,
            g.identify,
            !self.st.monitors.is_empty(),
            true,
        );
        let has_opts = mon.as_ref().map(|m| !m.sdr_options.is_empty()).unwrap_or(false);
        self.ui.add(Id::Sdr, Kind::Dropdown, g.sdr, has_opts, true);
        self.ui.add(
            Id::Read,
            Kind::Button,
            g.read,
            mon.as_ref().map(|m| m.has_edid).unwrap_or(false),
            true,
        );
        self.ui.add(Id::Compare, Kind::Button, g.compare, true, true);
        self.ui.add(Id::Manage, Kind::Button, g.manage, true, true);
        self.ui.add(
            Id::HdrToggle,
            Kind::Toggle,
            g.toggle,
            mon.as_ref().map(|m| m.hdr_supported).unwrap_or(false),
            true,
        );
        for (i, id) in SLIDERS.iter().enumerate() {
            let live = *id != Id::SdrWhite || mon.as_ref().is_some_and(|m| m.sdr_white_nits.is_some());
            self.ui.add(*id, Kind::Slider, g.rows[i].1, hdr_on && live, true);
        }
        self.ui.add(
            Id::Reset,
            Kind::TextButton,
            g.reset,
            mon.as_ref()
                .map(|m| m.hdr_supported && m.hdr_custom_active)
                .unwrap_or(false),
            true,
        );
        self.ui
            .add(Id::Autostart, Kind::Checkbox, g.autostart, true, true);
        // drop focus that points at something now disabled
        if let Some(f) = self.ui.focus {
            if !self.ui.get(f).map(|w| w.enabled).unwrap_or(false) && !matches!(f, Id::Tile(_)) {
                self.ui.focus = None;
            }
        }
    }

    fn invalidate(&self) {
        if !self.hwnd.is_null() {
            // SAFETY: valid window handle.
            unsafe {
                InvalidateRect(self.hwnd, std::ptr::null(), 0);
            }
        }
    }

    fn set_hover(&mut self, h: Option<Id>) {
        if self.ui.hover != h {
            self.ui.hover = h;
            self.invalidate();
        }
        let tile = match h {
            Some(Id::Tile(i)) => Some(i),
            _ => None,
        };
        if tile != self.highlight {
            self.highlight = tile;
            self.pending.push(Pending::Dispatch(UiAction::Highlight(tile)));
        }
    }

    fn activate(&mut self, id: Id) {
        let sel = self.sel;
        match id {
            Id::Tile(i) => {
                if i != self.sel {
                    self.commit();
                    self.sel = i;
                    self.dirty = false;
                    self.white_dirty = false;
                    self.sync_hdr();
                    self.layout();
                    self.invalidate();
                }
            }
            Id::Identify => self.pending.push(Pending::Dispatch(UiAction::Identify)),
            Id::Sdr => self.pending.push(Pending::OpenDropdown),
            Id::Read => self
                .pending
                .push(Pending::Dispatch(UiAction::ReadEdid { mon: sel })),
            Id::Compare => self.pending.push(Pending::Dispatch(UiAction::OpenGamut)),
            Id::Manage => self.pending.push(Pending::Dispatch(UiAction::OpenProfiles)),
            Id::HdrToggle => {
                let on = !self.mon().map(|m| m.hdr_on).unwrap_or(false);
                self.pending
                    .push(Pending::Dispatch(UiAction::SetHdrEnabled { mon: sel, on }));
            }
            Id::Reset => self
                .pending
                .push(Pending::Dispatch(UiAction::ResetHdr { mon: sel })),
            Id::Autostart => {
                self.pending
                    .push(Pending::Dispatch(UiAction::SetAutostart(!self.st.autostart)));
            }
            _ => {}
        }
    }

    fn slider_at_x(&mut self, id: Id, x: i32) {
        if let Some(w) = self.ui.get(id) {
            let t = slider_t_at(w.rect, x, self.dpi);
            self.set_slider_t(id, t);
            self.invalidate();
        }
    }

    fn arm_commit_timer(&self) {
        // SAFETY: valid window handle; re-arming replaces the pending one-shot.
        unsafe {
            SetTimer(self.hwnd, TIMER_COMMIT, COMMIT_DELAY_MS, None);
        }
    }

    // ---- painting ---------------------------------------------------------------------------

    fn paint(&mut self, hdc: windows_sys::Win32::Graphics::Gdi::HDC) {
        let mut rc: RECT = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: valid window handle and out pointer.
        unsafe {
            GetClientRect(self.hwnd, &mut rc);
        }
        let (w, h) = (rc.right, rc.bottom);
        if w <= 0 || h <= 0 {
            return;
        }
        let mut cv = match self.canvas.take() {
            Some(c) if c.width() == w && c.height() == h => c,
            _ => match Canvas::new(w, h) {
                Some(c) => c,
                None => return,
            },
        };
        self.draw(&mut cv);
        cv.present(hdc, 0, 0);
        self.canvas = Some(cv);
    }

    fn draw(&self, c: &mut Canvas) {
        let th = self.theme();
        let dpi = self.dpi;
        let ctx = Ctx {
            th,
            f: &self.fonts,
            dpi,
        };
        let g = &self.geo;
        let ui = &self.ui;
        c.clear(th.bg);

        let mon = self.mon();
        if self.st.monitors.is_empty() {
            c.text(
                g.map,
                "No active displays found",
                self.fonts.body,
                th.text_dim,
                TA_LEFT,
            );
        }
        // monitor map
        for (i, m) in self.st.monitors.iter().enumerate() {
            let Some(w) = ui.get(Id::Tile(i)) else { continue };
            let r = w.rect;
            let st = ui.state(Id::Tile(i));
            let rad = scf(8.0, dpi);
            let selected = i == self.sel;
            let fill = if selected {
                mix(th.surface, m.accent, if st.hover { 0.28 } else { 0.18 })
            } else if st.hover {
                th.hover_fill()
            } else {
                th.surface
            };
            c.fill_rrect(r, rad, fill, 1.0);
            c.stroke_rrect(r, rad, scf(2.0, dpi), m.accent, 1.0);
            let num = if th.dark {
                m.accent
            } else {
                mix(m.accent, 0x000000, 0.38)
            };
            let font = if r.h() < sc(46, dpi) {
                self.fonts.heading
            } else {
                self.fonts.big
            };
            c.text(r, &m.num.to_string(), font, num, TA_CENTER);
        }
        // identify
        draw_button(
            c,
            &ctx,
            g.identify,
            "Identify",
            ui.state(Id::Identify),
            BtnStyle::Text,
        );

        // info line
        if let Some(m) = mon {
            c.fill_circle(
                g.info.l as f32 + scf(4.0, dpi),
                g.info.cy(),
                scf(4.0, dpi),
                m.accent,
                1.0,
            );
            let hdr = if !m.hdr_supported {
                "HDR not supported"
            } else if m.hdr_on {
                "HDR on"
            } else {
                "HDR off"
            };
            let line = format!("{} \u{b7} {} \u{d7} {} \u{b7} {}", m.name, m.width, m.height, hdr);
            c.text(
                R::new(g.info.l + sc(18, dpi), g.info.t, g.info.r, g.info.b),
                &line,
                self.fonts.body,
                th.text,
                TA_LEFT,
            );
        }

        // SDR profile
        c.text(g.sdr_label, "SDR profile", self.fonts.body, th.text, TA_LEFT);
        let sdr_text = mon
            .and_then(|m| {
                m.sdr_current
                    .and_then(|i| m.sdr_options.get(i))
                    .map(|o| o.0.clone())
            })
            .unwrap_or_else(|| "Not set".to_string());
        draw_dropdown(
            c,
            &ctx,
            g.sdr,
            &sdr_text,
            ui.state(Id::Sdr),
            widgets::dropdown_is_open(),
        );
        draw_button(
            c,
            &ctx,
            g.read,
            "Read profile from monitor",
            ui.state(Id::Read),
            BtnStyle::Normal,
        );
        draw_button(
            c,
            &ctx,
            g.compare,
            "Compare gamuts\u{2026}",
            ui.state(Id::Compare),
            BtnStyle::Normal,
        );
        draw_button(
            c,
            &ctx,
            g.manage,
            "Manage profiles\u{2026}",
            ui.state(Id::Manage),
            BtnStyle::Normal,
        );

        // HDR
        c.fill_rect(
            R::new(sc(MARGIN, dpi), g.div1, sc(CLIENT_W - MARGIN, dpi), g.div1 + 1),
            th.line,
        );
        c.text(g.hdr_head, "HDR", self.fonts.heading, th.text, TA_LEFT);
        let hint = match mon {
            Some(m) if !m.hdr_supported => Some("Not supported by this display"),
            Some(m) if !m.hdr_on => Some("Turn on HDR to tune it"),
            None => None,
            _ => None,
        };
        if let Some(h) = hint {
            c.text(g.hdr_hint, h, self.fonts.dim, th.text_dim, TA_RIGHT);
        }
        draw_toggle(
            c,
            &ctx,
            g.toggle,
            mon.map(|m| m.hdr_on).unwrap_or(false),
            ui.state(Id::HdrToggle),
        );
        let labels = [
            "Black level",
            "Peak brightness",
            "Full-frame",
            "Saturation",
            "SDR content",
        ];
        let enabled = self.hdr_enabled();
        for (i, id) in SLIDERS.iter().enumerate() {
            let (lab, sl, val) = g.rows[i];
            let tcol = if enabled {
                th.text
            } else {
                mix(th.text, th.bg, 0.55)
            };
            c.text(lab, labels[i], self.fonts.body, tcol, TA_LEFT);
            let t = self.slider_scale(*id).to_t(self.slider_value(*id));
            draw_slider(c, &ctx, sl, t, ui.state(*id), self.drag == Some(*id));
            let vcol = if enabled {
                th.text
            } else {
                mix(th.text_dim, th.bg, 0.5)
            };
            c.text(val, &self.slider_text(*id), self.fonts.body, vcol, TA_RIGHT);
        }
        draw_button(c, &ctx, g.reset, "Reset HDR", ui.state(Id::Reset), BtnStyle::Text);

        // footer
        c.fill_rect(
            R::new(sc(MARGIN, dpi), g.div2, sc(CLIENT_W - MARGIN, dpi), g.div2 + 1),
            th.line,
        );
        draw_checkbox(
            c,
            &ctx,
            g.autostart,
            "Start with Windows",
            self.st.autostart,
            ui.state(Id::Autostart),
        );
        if let Some(s) = &self.st.status {
            c.text(g.status, s, self.fonts.dim, th.text_dim, TA_LEFT);
        }
    }
}

/// Scaled desktop layout of the monitors fitted into `area`; one tile (with gap) per monitor.
fn tile_rects(mons: &[UiMonitor], area: R, dpi: u32) -> Vec<R> {
    let left = mons.iter().map(|m| m.rect.left).min().unwrap_or(0);
    let top = mons.iter().map(|m| m.rect.top).min().unwrap_or(0);
    let right = mons.iter().map(|m| m.rect.right).max().unwrap_or(1);
    let bottom = mons.iter().map(|m| m.rect.bottom).max().unwrap_or(1);
    let (bw, bh) = (((right - left).max(1)) as f32, ((bottom - top).max(1)) as f32);
    let scale = (area.w() as f32 / bw).min(area.h() as f32 / bh);
    let (ow, oh) = (bw * scale, bh * scale);
    let (ox, oy) = (
        area.l as f32 + (area.w() as f32 - ow) / 2.0,
        area.t as f32 + (area.h() as f32 - oh) / 2.0,
    );
    let gap = sc(3, dpi);
    mons.iter()
        .map(|m| {
            let l = ox + (m.rect.left - left) as f32 * scale;
            let t = oy + (m.rect.top - top) as f32 * scale;
            let r = ox + (m.rect.right - left) as f32 * scale;
            let b = oy + (m.rect.bottom - top) as f32 * scale;
            R::new(
                l.round() as i32 + gap,
                t.round() as i32 + gap,
                r.round() as i32 - gap,
                b.round() as i32 - gap,
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------------------------

/// Opens the window with its top-left corner at (x, y) in screen pixels (screenshots).
pub fn open_at(x: i32, y: i32) {
    OPEN_AT.with(|c| c.set(Some((x, y))));
    open();
    OPEN_AT.with(|c| c.set(None));
}

pub fn open() {
    if is_open() {
        let hwnd = hwnd();
        // SAFETY: valid window handle.
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
        }
        return;
    }
    register_class();
    let dpi = system_dpi();
    let st = super::snapshot();
    let main = Main {
        hwnd: std::ptr::null_mut(),
        dpi,
        fonts: Fonts::new(dpi),
        canvas: None,
        ui: WidgetList::new(),
        st,
        sel: 0,
        black: 0.0,
        peak: 1000.0,
        full: 400.0,
        sat: 0.0,
        white: 240.0,
        white_dirty: false,
        drag: None,
        dirty: false,
        pending: Vec::new(),
        tracking: false,
        highlight: None,
        dd_closed_at: None,
        icons: [std::ptr::null_mut(); 2],
        geo: geo(dpi),
    };
    MAIN.with(|m| *m.borrow_mut() = Some(main));
    with(|m| {
        m.sync_hdr();
        m.layout();
    });

    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    let (cw, ch) = (sc(CLIENT_W, dpi), sc(CLIENT_H, dpi));
    let mut r = RECT {
        left: 0,
        top: 0,
        right: cw,
        bottom: ch,
    };
    let mut wa = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    // SAFETY: plain Win32 calls with valid pointers; MAIN is not borrowed during window creation.
    unsafe {
        AdjustWindowRectExForDpi(&mut r, style, 0, 0, super::draw::frame_dpi(dpi));
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut RECT as *mut _, 0);
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let (x, y) = OPEN_AT.with(|c| c.get()).unwrap_or((
            wa.left + ((wa.right - wa.left) - w) / 2,
            wa.top + ((wa.bottom - wa.top) - h) / 2,
        ));
        let cls = wide(CLASS);
        let title = wide("Spectrum");
        let hwnd = CreateWindowExW(
            0,
            cls.as_ptr(),
            title.as_ptr(),
            style,
            x.max(wa.left),
            y.max(wa.top),
            if w <= 0 { CW_USEDEFAULT } else { w },
            h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        if hwnd.is_null() {
            MAIN.with(|m| *m.borrow_mut() = None);
            return;
        }
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        let actual = window_dpi(hwnd);
        if actual != dpi {
            on_dpi_changed(hwnd, actual, None);
        }
    }
}

pub fn close() {
    let h = hwnd();
    if !h.is_null() {
        // SAFETY: valid window handle owned by this thread.
        unsafe {
            DestroyWindow(h);
        }
    }
}

/// Closes through `WM_CLOSE`, so pending slider edits are committed first.
pub fn request_close() {
    let h = hwnd();
    if !h.is_null() {
        // SAFETY: valid window handle owned by this thread; handled synchronously.
        unsafe {
            SendMessageW(h, WM_CLOSE, 0, 0);
        }
    }
}

pub fn refresh() {
    with(|m| {
        m.st = super::snapshot();
        m.fix_sel();
        m.sync_hdr();
        m.layout();
        m.invalidate();
    });
}

pub fn is_open() -> bool {
    !hwnd().is_null()
}

/// Index of the monitor currently selected in the open window (0 when closed).
pub fn selected() -> usize {
    with(|m| m.sel).unwrap_or(0)
}

/// Demo hook: shows `nits` on the peak slider (and lowers full-frame to fit) without applying anything.
pub fn demo_set_peak(nits: f64) {
    with(|m| {
        m.peak = nits.clamp(widgets::PEAK_MIN_NITS, widgets::PEAK_MAX_NITS);
        m.full = m.full.min(m.peak);
        m.invalidate();
    });
}

/// Demo hook: screen position of the peak slider's thumb when it shows `nits`.
pub fn demo_peak_thumb(nits: f64) -> Option<(i32, i32)> {
    with(|m| {
        let r = m.ui.get(Id::Peak)?.rect;
        let (a, b) = widgets::slider_span(r, m.dpi);
        let x = a + (b - a) * peak_scale().to_t(nits) as f32;
        let mut p = POINT {
            x: x.round() as i32,
            y: r.cy().round() as i32,
        };
        // SAFETY: valid window handle and POINT.
        unsafe { windows_sys::Win32::Graphics::Gdi::ClientToScreen(m.hwnd, &mut p) };
        Some((p.x, p.y))
    })
    .flatten()
}

pub fn hwnd() -> HWND {
    HWND_CELL.with(|c| c.get()) as HWND
}

// ---------------------------------------------------------------------------------------------
// Window procedure
// ---------------------------------------------------------------------------------------------

fn register_class() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let cls = wide(CLASS);
        // SAFETY: class data outlives the call.
        unsafe {
            let mut wc: WNDCLASSEXW = std::mem::zeroed();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            wc.lpfnWndProc = Some(wnd_proc);
            wc.hInstance = GetModuleHandleW(std::ptr::null());
            wc.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
            wc.lpszClassName = cls.as_ptr();
            RegisterClassExW(&wc);
        }
    });
}

fn lo(lp: LPARAM) -> i32 {
    (lp & 0xFFFF) as i16 as i32
}
fn hi(lp: LPARAM) -> i32 {
    ((lp >> 16) & 0xFFFF) as i16 as i32
}

fn flush_pending() {
    loop {
        let items = with(|m| std::mem::take(&mut m.pending)).unwrap_or_default();
        if items.is_empty() {
            break;
        }
        for p in items {
            match p {
                Pending::Dispatch(a) => super::dispatch(a),
                Pending::OpenDropdown => open_dropdown_now(),
                Pending::Close => close_requested(),
            }
        }
    }
}

fn open_dropdown_now() {
    let info = with(|m| {
        if m.dd_closed_at
            .map(|t| t.elapsed() < Duration::from_millis(300))
            .unwrap_or(false)
        {
            return None;
        }
        let mon = m.mon()?;
        let w = m.ui.get(Id::Sdr)?;
        let mut tl = POINT {
            x: w.rect.l,
            y: w.rect.t,
        };
        let mut br = POINT {
            x: w.rect.r,
            y: w.rect.b,
        };
        // SAFETY: valid window handle and points.
        unsafe {
            windows_sys::Win32::Graphics::Gdi::ClientToScreen(m.hwnd, &mut tl);
            windows_sys::Win32::Graphics::Gdi::ClientToScreen(m.hwnd, &mut br);
        }
        let items: Vec<String> = mon.sdr_options.iter().map(|o| o.0.clone()).collect();
        Some((
            m.hwnd,
            R::new(tl.x, tl.y, br.x, br.y),
            items,
            mon.sdr_current,
            m.theme(),
        ))
    })
    .flatten();
    if let Some((h, anchor, items, sel, th)) = info {
        widgets::open_dropdown(h, anchor, items, sel, th, WM_DROPDOWN_DONE);
        with(|m| m.invalidate());
    }
}

/// Esc / close box: commit pending edits, tell the app, destroy.
fn close_requested() {
    with(|m| m.commit());
    let acts = with(|m| std::mem::take(&mut m.pending)).unwrap_or_default();
    for p in acts {
        if let Pending::Dispatch(a) = p {
            super::dispatch(a);
        }
    }
    super::dispatch(UiAction::CloseMain);
    close();
}

fn on_dpi_changed(hwnd: HWND, dpi: u32, suggested: Option<RECT>) {
    with(|m| {
        m.dpi = dpi;
        m.fonts = Fonts::new(dpi);
        m.canvas = None;
        m.layout();
    });
    refresh_icons(hwnd, dpi);
    // SAFETY: valid window; nested messages are handled with MAIN unborrowed.
    unsafe {
        let r = match suggested {
            Some(r) => r,
            None => {
                let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
                let mut r = RECT {
                    left: 0,
                    top: 0,
                    right: sc(CLIENT_W, dpi),
                    bottom: sc(CLIENT_H, dpi),
                };
                AdjustWindowRectExForDpi(&mut r, style, 0, 0, super::draw::frame_dpi(dpi));
                let mut cur: RECT = std::mem::zeroed();
                windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut cur);
                RECT {
                    left: cur.left,
                    top: cur.top,
                    right: cur.left + r.right - r.left,
                    bottom: cur.top + r.bottom - r.top,
                }
            }
        };
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

fn shift_down() -> bool {
    // SAFETY: plain query.
    unsafe { GetKeyState(VK_SHIFT as i32) < 0 }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let r = handle(hwnd, msg, wp, lp);
    if msg != WM_DESTROY && msg != WM_NCDESTROY {
        flush_pending();
    }
    r
}

unsafe fn handle(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            HWND_CELL.with(|c| c.set(hwnd as isize));
            let dark = theme().dark;
            apply_window_theme(hwnd, dark);
            let big = icon_for(hwnd, SM_CXICON);
            let small = icon_for(hwnd, SM_CXSMICON);
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big as LPARAM);
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as LPARAM);
            with(|m| {
                m.hwnd = hwnd;
                m.icons = [big, small];
            });
            0
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            with(|m| m.paint(hdc));
            EndPaint(hwnd, &ps);
            0
        }
        WM_DPICHANGED => {
            let dpi = (wp & 0xFFFF) as u32;
            let rc = *(lp as *const RECT);
            on_dpi_changed(hwnd, dpi, Some(rc));
            0
        }
        WM_SETTINGCHANGE => {
            // A string lParam only comes with wParam 0; anything else may not be a pointer.
            if wp == 0 && lp != 0 {
                let p = lp as *const u16;
                let mut n = 0;
                while n < 64 && *p.add(n) != 0 {
                    n += 1;
                }
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
                if s == "ImmersiveColorSet" {
                    invalidate_theme();
                    apply_window_theme(hwnd, theme().dark);
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            }
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = (lo(lp), hi(lp));
            with(|m| {
                if !m.tracking {
                    let mut t = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut t);
                    m.tracking = true;
                }
                if let Some(id) = m.drag {
                    if wp & 1 == 0 {
                        // MK_LBUTTON is gone: the button-up was missed, so end the drag here.
                        m.drag = None;
                        m.ui.pressed = None;
                        m.commit();
                    } else {
                        m.slider_at_x(id, x);
                    }
                } else {
                    let h = m.ui.hit(x, y);
                    m.set_hover(h);
                }
            });
            0
        }
        WM_MOUSELEAVE => {
            with(|m| {
                m.tracking = false;
                if m.drag.is_none() {
                    m.set_hover(None);
                }
            });
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = (lo(lp), hi(lp));
            SetFocus(hwnd);
            with(|m| {
                let hit = m.ui.hit(x, y);
                m.ui.focus_visible = false;
                if let Some(id) = hit {
                    let w = *m.ui.get(id).unwrap();
                    if w.enabled {
                        m.ui.pressed = Some(id);
                        if w.focusable {
                            m.ui.focus = Some(id);
                        }
                        if w.kind == Kind::Slider {
                            m.drag = Some(id);
                            m.slider_at_x(id, x);
                        }
                        SetCapture(hwnd);
                    }
                } else {
                    m.ui.focus = None;
                }
                m.invalidate();
            });
            0
        }
        WM_LBUTTONUP => {
            let (x, y) = (lo(lp), hi(lp));
            with(|m| {
                let pressed = m.ui.pressed.take();
                if let Some(id) = m.drag.take() {
                    m.slider_at_x(id, x);
                    m.commit();
                } else if let Some(p) = pressed {
                    if m.ui.hit(x, y) == Some(p) && m.ui.get(p).map(|w| w.enabled).unwrap_or(false) {
                        m.activate(p);
                    }
                }
                let h = m.ui.hit(x, y);
                m.set_hover(h);
                m.invalidate();
            });
            // After the state is taken: ReleaseCapture sends WM_CAPTURECHANGED.
            if GetCapture_is(hwnd) {
                ReleaseCapture();
            }
            0
        }
        WM_CAPTURECHANGED => {
            // Capture stolen mid-press (Alt+Tab, a menu, a notification): end the press and drag.
            with(|m| {
                m.ui.pressed = None;
                if m.drag.take().is_some() {
                    m.commit();
                }
                m.invalidate();
            });
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp >> 16) & 0xFFFF) as i16 as i32;
            let mut pt = POINT { x: lo(lp), y: hi(lp) };
            windows_sys::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut pt);
            with(|m| {
                if let Some(id) = m.ui.hit(pt.x, pt.y) {
                    if SLIDERS.contains(&id) && m.ui.get(id).map(|w| w.enabled).unwrap_or(false) {
                        let t = m.slider_scale(id).to_t(m.slider_value(id));
                        let n = (t + 0.03 * (delta as f64 / 120.0)).clamp(0.0, 1.0);
                        m.set_slider_t(id, n);
                        m.arm_commit_timer();
                        m.invalidate();
                    }
                }
            });
            0
        }
        WM_KEYDOWN => {
            let vk = wp as u16;
            with(|m| match vk {
                VK_ESCAPE => m.pending.push(Pending::Close),
                VK_TAB => {
                    m.ui.focus_next(shift_down());
                    m.invalidate();
                }
                VK_SPACE | VK_RETURN => {
                    m.ui.focus_visible = true;
                    if let Some(f) = m.ui.focus {
                        m.activate(f);
                    }
                    m.invalidate();
                }
                _ => {
                    m.ui.focus_visible = true;
                    if let Some(f) = m.ui.focus {
                        if SLIDERS.contains(&f) {
                            let t = m.slider_scale(f).to_t(m.slider_value(f));
                            if let Some(n) = key_to_t(t, vk) {
                                m.set_slider_t(f, n);
                                m.arm_commit_timer();
                            }
                        } else if f == Id::Sdr && vk == VK_DOWN {
                            m.pending.push(Pending::OpenDropdown);
                        }
                    }
                    m.invalidate();
                }
            });
            0
        }
        WM_TIMER => {
            if wp == TIMER_COMMIT {
                KillTimer(hwnd, TIMER_COMMIT);
                with(|m| m.commit());
            }
            0
        }
        WM_DROPDOWN_DONE => {
            with(|m| {
                m.dd_closed_at = Some(Instant::now());
                if wp != DROPDOWN_DISMISSED {
                    let sel = m.sel;
                    m.pending
                        .push(Pending::Dispatch(UiAction::SetSdr { mon: sel, idx: wp }));
                }
                m.invalidate();
            });
            0
        }
        WM_ACTIVATE => {
            // Brightness changed in Settings while this window was in the background.
            if wp & 0xFFFF != 0 {
                refresh();
            }
            0
        }
        WM_SETFOCUS | WM_KILLFOCUS => {
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_SETCURSOR => {
            if (lp & 0xFFFF) as u32 == 1 {
                // HTCLIENT
                let mut pt = POINT { x: 0, y: 0 };
                GetCursorPos(&mut pt);
                windows_sys::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut pt);
                let hand = with(|m| {
                    m.ui.hit(pt.x, pt.y)
                        .and_then(|id| m.ui.get(id).copied())
                        .map(|w| w.enabled && w.kind != Kind::Slider)
                        .unwrap_or(false)
                })
                .unwrap_or(false);
                let cur = LoadCursorW(std::ptr::null_mut(), if hand { IDC_HAND } else { IDC_ARROW });
                SetCursor(cur);
                return 1;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_CLOSE => {
            close_requested();
            0
        }
        WM_DESTROY => {
            KillTimer(hwnd, TIMER_COMMIT);
            let had_highlight = with(|m| m.highlight.take().is_some()).unwrap_or(false);
            HWND_CELL.with(|c| c.set(0));
            let main = MAIN.with(|m| m.try_borrow_mut().ok().and_then(|mut b| b.take()));
            if let Some(m) = &main {
                for i in m.icons {
                    if !i.is_null() {
                        DestroyIcon(i);
                    }
                }
            }
            drop(main); // fonts, canvas (DIB + DC)
            if had_highlight {
                super::dispatch(UiAction::Highlight(None));
            }
            trim_working_set();
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[allow(non_snake_case)]
fn GetCapture_is(hwnd: HWND) -> bool {
    // SAFETY: plain query.
    unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetCapture() == hwnd }
}

fn icon_for(hwnd: HWND, metric: i32) -> HICON {
    icon_at(window_dpi(hwnd), metric)
}

/// Rebuilds the title bar icons for a new DPI and frees the old ones.
fn refresh_icons(hwnd: HWND, dpi: u32) {
    if with(|_| ()).is_none() {
        return;
    }
    let big = icon_at(dpi, SM_CXICON);
    let small = icon_at(dpi, SM_CXSMICON);
    // SAFETY: valid window; the old icons are released only after the window stops using them.
    unsafe {
        SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big as LPARAM);
        SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as LPARAM);
        if let Some(old) = with(|x| std::mem::replace(&mut x.icons, [big, small])) {
            for i in old {
                if !i.is_null() {
                    DestroyIcon(i);
                }
            }
        }
    }
}

fn icon_at(dpi: u32, metric: i32) -> HICON {
    // SAFETY: plain query.
    let px = unsafe { windows_sys::Win32::UI::HiDpi::GetSystemMetricsForDpi(metric, dpi) };
    crate::icon::create_hicon(px.max(16) as u32)
}
