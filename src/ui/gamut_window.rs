//! CIE 1931 xy gamut comparison window with a checklist of reference spaces and installed profiles.

#![allow(clippy::too_many_arguments)]

use super::draw::{
    apply_window_theme, invalidate_theme, mix, sc, scf, system_dpi, theme, trim_working_set, window_dpi,
    Canvas, Dib, Fonts, Theme, R, TA_CENTER, TA_LEFT, TA_RIGHT, WM_MOUSELEAVE,
};
use super::widgets::{draw_checkbox, Ctx, WState};
use crate::color::{self, Primaries, Xy, Xyz, SPECTRAL_LOCUS, STANDARD_SPACES};
use crate::util::wide;
use std::cell::RefCell;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, ScreenToClient, PAINTSTRUCT};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::AdjustWindowRectExForDpi;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_SPACE, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, GetClientRect, IsIconic, LoadCursorW,
    RegisterClassExW, SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow, SystemParametersInfoW,
    CS_HREDRAW, CS_VREDRAW, HICON, ICON_BIG, ICON_SMALL, IDC_ARROW, SM_CXICON, SM_CXSMICON, SPI_GETWORKAREA,
    SWP_NOACTIVATE, SWP_NOZORDER, SW_RESTORE, SW_SHOW, WM_CLOSE, WM_CREATE, WM_DESTROY, WM_DPICHANGED,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SETICON,
    WM_SETTINGCHANGE, WNDCLASSEXW, WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
};

const CLASS: &str = "SpectrumGamutWindow";
const CLIENT_W: i32 = 780;
const CLIENT_H: i32 = 520;
/// Plot domain.
const X_MAX: f64 = 0.8;
const Y_MAX: f64 = 0.9;
const PROFILE_COLORS: [u32; 8] = [
    0xFFFFFF, 0xB57BFF, 0x4D9DE0, 0xFF9F43, 0x00E5FF, 0xFF4D8D, 0xC6F135, 0xA0522D,
];
const WAVELENGTHS: [u16; 9] = [460, 480, 500, 520, 540, 560, 580, 600, 620];

struct Entry {
    name: String,
    /// Right-aligned dim tag in the list.
    tag: String,
    detail: String,
    prims: Option<Primaries>,
    color: u32,
    checked: bool,
    reference: bool,
}

#[derive(Clone, Copy)]
enum Row {
    Header(&'static str),
    Entry(usize),
}

struct Gamut {
    hwnd: HWND,
    dpi: u32,
    fonts: Fonts,
    canvas: Option<Canvas>,
    horseshoe: Option<Dib>,
    entries: Vec<Entry>,
    rows: Vec<Row>,
    scroll: i32,
    selected: Option<usize>,
    hover: Option<usize>,
    tracking: bool,
    /// Profile (by entry index) the stats line talks about when nothing else is ticked.
    monitor_entry: Option<usize>,
    stats: (String, String),
    icons: [HICON; 2],
}

thread_local! {
    static GAMUT: RefCell<Option<Gamut>> = const { RefCell::new(None) };
    static HWND_CELL: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

fn with<T>(f: impl FnOnce(&mut Gamut) -> T) -> Option<T> {
    GAMUT.with(|m| m.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

// ---------------------------------------------------------------------------------------------
// Geometry (physical px)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Geo {
    plot: R,
    list: R,
    desc: R,
    stats: R,
    head_l: R,
    head_r: R,
}

fn geo(dpi: u32) -> Geo {
    let s = |v: i32| sc(v, dpi);
    Geo {
        plot: R::new(s(56), s(40), s(456), s(490)),
        head_l: R::new(s(56), s(10), s(456), s(34)),
        head_r: R::new(s(488), s(10), s(CLIENT_W - 20), s(34)),
        list: R::new(s(488), s(40), s(CLIENT_W - 20), s(318)),
        desc: R::new(s(488), s(330), s(CLIENT_W - 20), s(410)),
        stats: R::new(s(488), s(424), s(CLIENT_W - 20), s(506)),
    }
}

const ROW_H: i32 = 30;
const HEAD_H: i32 = 24;

fn row_h(r: &Row, dpi: u32) -> i32 {
    sc(
        match r {
            Row::Header(_) => HEAD_H,
            Row::Entry(_) => ROW_H,
        },
        dpi,
    )
}

// ---------------------------------------------------------------------------------------------
// Data
// ---------------------------------------------------------------------------------------------

fn short_name(n: &str) -> &str {
    n.split(" (").next().unwrap_or(n)
}

fn reference_file_id(file: &str) -> Option<usize> {
    match file {
        f if f.eq_ignore_ascii_case("sRGB Color Space Profile.icm") => Some(0),
        f if f.eq_ignore_ascii_case("Spectrum Display P3.icm") => Some(1),
        f if f.eq_ignore_ascii_case("Spectrum Adobe RGB compatible.icm") => Some(2),
        _ => None,
    }
}

fn build_entries(selected_file: Option<&str>) -> (Vec<Entry>, Option<usize>) {
    let mut entries: Vec<Entry> = STANDARD_SPACES
        .iter()
        .map(|s| Entry {
            name: s.name.to_string(),
            tag: String::new(),
            detail: s.description.to_string(),
            prims: Some(s.prims),
            color: s.color,
            checked: false,
            reference: true,
        })
        .collect();
    // sRGB is always ticked by default.
    entries[0].checked = true;
    let mut monitor_entry = None;
    let profiles = if super::is_mock() {
        mock_profiles()
    } else {
        installed_profiles()
    };
    let mut n = 0usize;
    for (file, name, prims, tag) in profiles {
        if let Some(i) = reference_file_id(&file) {
            if selected_file == Some(file.as_str()) {
                entries[i].checked = true;
            }
            continue;
        }
        let plottable = prims.is_some();
        let color = PROFILE_COLORS[n % PROFILE_COLORS.len()];
        n += 1;
        let is_sel = selected_file == Some(file.as_str());
        let idx = entries.len();
        entries.push(Entry {
            name,
            tag: if plottable {
                tag
            } else {
                "not plottable".to_string()
            },
            detail: if plottable {
                format!("{file} \u{b7} matrix/TRC profile")
            } else {
                format!("{file} \u{b7} LUT-based profile, so there is no triangle to draw")
            },
            prims,
            color,
            checked: is_sel && plottable,
            reference: false,
        });
        if is_sel {
            monitor_entry = Some(idx);
        }
    }
    (entries, monitor_entry)
}

/// (file, name, primaries when matrix/TRC, tag)
fn installed_profiles() -> Vec<(String, String, Option<Primaries>, String)> {
    let mut out = Vec::new();
    for file in crate::wcs::list_installed() {
        let Some(bytes) = crate::wcs::read_profile(&file) else {
            continue;
        };
        if bytes.len() > 8 << 20 {
            continue;
        }
        let Ok(info) = crate::icc_read::parse(&bytes) else {
            continue;
        };
        let name = if info.desc.trim().is_empty() {
            file.clone()
        } else {
            info.desc.clone()
        };
        let prims = if info.is_matrix_trc { info.primaries } else { None };
        let tag = if info.mhc2.is_some() {
            "HDR".to_string()
        } else if file.starts_with("Spectrum") {
            "Spectrum".to_string()
        } else {
            String::new()
        };
        out.push((file, name, prims, tag));
    }
    out
}

fn mock_profiles() -> Vec<(String, String, Option<Primaries>, String)> {
    let xy = |x, y| Xy { x, y };
    let w = xy(0.3127, 0.3290);
    vec![
        (
            "Spectrum MAG 271QPX E2 EDID.icm".to_string(),
            "MAG 271QPX E2 (from EDID)".to_string(),
            Some(Primaries {
                r: xy(0.6855, 0.3047),
                g: xy(0.2373, 0.7041),
                b: xy(0.1396, 0.0508),
                w,
            }),
            "Spectrum".to_string(),
        ),
        (
            "Spectrum HDR 1.icc".to_string(),
            "MAG 271QPX E2 HDR 450".to_string(),
            Some(Primaries {
                r: xy(0.6855, 0.3047),
                g: xy(0.2373, 0.7041),
                b: xy(0.1396, 0.0508),
                w,
            }),
            "HDR".to_string(),
        ),
        (
            "VENDOR_sRGB_mode.icm".to_string(),
            "Vendor sRGB mode".to_string(),
            Some(Primaries {
                r: xy(0.6406, 0.3301),
                g: xy(0.2998, 0.5996),
                b: xy(0.1504, 0.0596),
                w,
            }),
            String::new(),
        ),
        (
            "Office_CMYK_proof.icc".to_string(),
            "Office CMYK proof".to_string(),
            None,
            String::new(),
        ),
    ]
}

impl Gamut {
    fn rebuild_rows(&mut self) {
        self.rows.clear();
        self.rows.push(Row::Header("REFERENCE SPACES"));
        for (i, e) in self.entries.iter().enumerate() {
            if e.reference {
                self.rows.push(Row::Entry(i));
            }
        }
        if self.entries.iter().any(|e| !e.reference) {
            self.rows.push(Row::Header("INSTALLED PROFILES"));
            for (i, e) in self.entries.iter().enumerate() {
                if !e.reference {
                    self.rows.push(Row::Entry(i));
                }
            }
        }
    }

    fn content_h(&self) -> i32 {
        self.rows.iter().map(|r| row_h(r, self.dpi)).sum()
    }

    fn clamp_scroll(&mut self) {
        let g = geo(self.dpi);
        let max = (self.content_h() - g.list.h()).max(0);
        self.scroll = self.scroll.clamp(0, max);
    }

    /// Entry index and its on-screen rect at a client point.
    fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        let g = geo(self.dpi);
        if !g.list.contains(x, y) {
            return None;
        }
        let mut top = g.list.t - self.scroll;
        for r in &self.rows {
            let h = row_h(r, self.dpi);
            if y >= top && y < top + h {
                return match r {
                    Row::Entry(i) => Some(*i),
                    Row::Header(_) => None,
                };
            }
            top += h;
        }
        None
    }

    fn toggle(&mut self, i: usize) {
        if self.entries[i].prims.is_some() {
            self.entries[i].checked = !self.entries[i].checked;
        }
        self.selected = Some(i);
        self.recompute();
    }

    fn move_selection(&mut self, dir: i32) {
        let n = self.entries.len() as i32;
        if n == 0 {
            return;
        }
        let order: Vec<usize> = self
            .rows
            .iter()
            .filter_map(|r| match r {
                Row::Entry(i) => Some(*i),
                _ => None,
            })
            .collect();
        let cur = self
            .selected
            .and_then(|s| order.iter().position(|&i| i == s))
            .map(|p| p as i32);
        let next = match cur {
            None => 0,
            Some(c) => (c + dir).clamp(0, order.len() as i32 - 1),
        };
        self.selected = Some(order[next as usize]);
        self.ensure_visible(order[next as usize]);
        self.recompute();
    }

    fn ensure_visible(&mut self, entry: usize) {
        let g = geo(self.dpi);
        let mut top = 0;
        for r in &self.rows {
            let h = row_h(r, self.dpi);
            if matches!(r, Row::Entry(i) if *i == entry) {
                if top < self.scroll {
                    self.scroll = top;
                } else if top + h > self.scroll + g.list.h() {
                    self.scroll = top + h - g.list.h();
                }
                break;
            }
            top += h;
        }
        self.clamp_scroll();
    }

    /// Stats line: coverage of sRGB / P3 / Adobe by the first ticked non-reference profile (or the monitor's).
    fn recompute(&mut self) {
        let pick = self
            .entries
            .iter()
            .position(|e| !e.reference && e.checked && e.prims.is_some())
            .or(self.monitor_entry.filter(|&i| self.entries[i].prims.is_some()));
        self.stats = match pick.and_then(|i| self.entries[i].prims.map(|p| (i, p))) {
            None => (
                "Tick an installed profile to see its coverage.".to_string(),
                String::new(),
            ),
            Some((i, p)) => {
                let cov =
                    |idx: usize| (math_coverage(&p, &STANDARD_SPACES[idx].prims) * 100.0).round() as i32;
                let best = (0..STANDARD_SPACES.len())
                    .max_by(|&a, &b| {
                        math_iou(&p, &STANDARD_SPACES[a].prims)
                            .partial_cmp(&math_iou(&p, &STANDARD_SPACES[b].prims))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .unwrap_or(0);
                (
                    self.entries[i].name.clone(),
                    format!(
                        "Coverage of {} {}% \u{b7} {} {}% \u{b7} {} {}% \u{2014} closest: {}",
                        short_name(STANDARD_SPACES[0].name),
                        cov(0),
                        short_name(STANDARD_SPACES[1].name),
                        cov(1),
                        short_name(STANDARD_SPACES[2].name),
                        cov(2),
                        short_name(STANDARD_SPACES[best].name),
                    ),
                )
            }
        };
    }

    fn invalidate(&self) {
        if !self.hwnd.is_null() {
            // SAFETY: valid window handle.
            unsafe {
                InvalidateRect(self.hwnd, std::ptr::null(), 0);
            }
        }
    }

    fn ensure_horseshoe(&mut self) {
        let g = geo(self.dpi);
        let (w, h) = (g.plot.w(), g.plot.h());
        if self.horseshoe.as_ref().map(|d| (d.w, d.h)) != Some((w, h)) {
            self.horseshoe = build_horseshoe(w, h);
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
        self.ensure_horseshoe();
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

    fn plot_xy(&self, x: f64, y: f64) -> (f32, f32) {
        let g = geo(self.dpi);
        (
            g.plot.l as f32 + (x / X_MAX) as f32 * g.plot.w() as f32,
            g.plot.t as f32 + ((Y_MAX - y) / Y_MAX) as f32 * g.plot.h() as f32,
        )
    }

    fn draw(&mut self, c: &mut Canvas) {
        let th = theme();
        let dpi = self.dpi;
        let g = geo(dpi);
        c.clear(th.bg);

        // --- left: plot ---
        c.text(
            g.head_l,
            "CIE 1931 xy chromaticity",
            self.fonts.heading,
            th.text,
            TA_LEFT,
        );
        c.fill_rect(g.plot, th.surface);
        if let Some(h) = self.horseshoe.as_mut() {
            c.blit_masked(g.plot.l, g.plot.t, h);
        }
        // grid
        let lw = 1.0_f32.max(scf(1.0, dpi).floor());
        for i in 0..=8 {
            let v = i as f64 * 0.1;
            let (px, _) = self.plot_xy(v, 0.0);
            c.line(px, g.plot.t as f32, px, g.plot.b as f32, lw, th.line, 0.9);
            let lab = format!("{v:.1}");
            c.text(
                R::new(
                    px as i32 - sc(16, dpi),
                    g.plot.b + sc(3, dpi),
                    px as i32 + sc(16, dpi),
                    g.plot.b + sc(19, dpi),
                ),
                &lab,
                self.fonts.dim,
                th.text_dim,
                TA_CENTER,
            );
        }
        for i in 0..=9 {
            let v = i as f64 * 0.1;
            let (_, py) = self.plot_xy(0.0, v);
            c.line(g.plot.l as f32, py, g.plot.r as f32, py, lw, th.line, 0.9);
            let lab = format!("{v:.1}");
            c.text(
                R::new(
                    g.plot.l - sc(40, dpi),
                    py as i32 - sc(9, dpi),
                    g.plot.l - sc(6, dpi),
                    py as i32 + sc(9, dpi),
                ),
                &lab,
                self.fonts.dim,
                th.text_dim,
                TA_RIGHT,
            );
        }
        c.stroke_rrect(g.plot, 0.0, lw, mix(th.line, th.text_dim, 0.3), 1.0);

        // spectral locus outline and wavelength ticks
        let pts: Vec<(f32, f32)> = SPECTRAL_LOCUS
            .iter()
            .map(|&(_, x, y)| self.plot_xy(x, y))
            .collect();
        c.polyline(&pts, true, scf(1.5, dpi), mix(th.text_dim, th.text, 0.35), 0.9);
        let centroid = self.plot_xy(0.33, 0.33);
        for nm in WAVELENGTHS {
            let Some(i) = SPECTRAL_LOCUS.iter().position(|p| p.0 == nm) else {
                continue;
            };
            let (p, a, b) = (pts[i], pts[i.saturating_sub(1)], pts[(i + 1).min(pts.len() - 1)]);
            let (tx, ty) = (b.0 - a.0, b.1 - a.1);
            let len = (tx * tx + ty * ty).sqrt().max(1e-3);
            let (mut nx, mut ny) = (-ty / len, tx / len);
            if nx * (p.0 - centroid.0) + ny * (p.1 - centroid.1) < 0.0 {
                nx = -nx;
                ny = -ny;
            }
            let tick = scf(5.0, dpi);
            c.line(
                p.0,
                p.1,
                p.0 + nx * tick,
                p.1 + ny * tick,
                scf(1.2, dpi),
                th.text_dim,
                1.0,
            );
            let lr = scf(17.0, dpi);
            let (lx, ly) = (p.0 + nx * lr, p.1 + ny * lr);
            c.text(
                R::new(
                    lx as i32 - sc(16, dpi),
                    ly as i32 - sc(8, dpi),
                    lx as i32 + sc(16, dpi),
                    ly as i32 + sc(8, dpi),
                ),
                &nm.to_string(),
                self.fonts.dim,
                th.text_dim,
                TA_CENTER,
            );
        }
        c.text(
            R::new(
                g.plot.r - sc(70, dpi),
                g.plot.t + sc(2, dpi),
                g.plot.r - sc(4, dpi),
                g.plot.t + sc(18, dpi),
            ),
            "nm",
            self.fonts.dim,
            th.text_dim,
            TA_RIGHT,
        );

        // triangles
        for (i, e) in self.entries.iter().enumerate() {
            let (true, Some(p)) = (e.checked, e.prims) else {
                continue;
            };
            let t = [p.r, p.g, p.b].map(|v| self.plot_xy(v.x, v.y));
            let w = if self.selected == Some(i) { 2.8 } else { 2.0 };
            c.polyline(&t, true, scf(w + 1.5, dpi), 0x000000, 0.35);
            c.polyline(&t, true, scf(w, dpi), e.color, 1.0);
            for v in t {
                c.fill_circle(v.0, v.1, scf(2.6, dpi), e.color, 1.0);
            }
            let (wx, wy) = self.plot_xy(p.w.x, p.w.y);
            let s = scf(5.0, dpi);
            c.line(wx - s, wy, wx + s, wy, scf(2.5, dpi), 0x000000, 0.4);
            c.line(wx, wy - s, wx, wy + s, scf(2.5, dpi), 0x000000, 0.4);
            c.line(wx - s, wy, wx + s, wy, scf(1.4, dpi), e.color, 1.0);
            c.line(wx, wy - s, wx, wy + s, scf(1.4, dpi), e.color, 1.0);
        }

        // --- right: list ---
        let ctx = Ctx {
            th,
            f: &self.fonts,
            dpi,
        };
        let mut top = g.list.t - self.scroll;
        for r in &self.rows {
            let h = row_h(r, dpi);
            if top + h > g.list.t - h && top < g.list.b + h {
                match r {
                    Row::Header(t) => {
                        c.text(
                            R::new(g.list.l + sc(4, dpi), top, g.list.r, top + h),
                            t,
                            self.fonts.dim,
                            th.text_dim,
                            TA_LEFT,
                        );
                    }
                    Row::Entry(i) => self.draw_row(c, &ctx, *i, R::new(g.list.l, top, g.list.r, top + h)),
                }
            }
            top += h;
        }
        // mask everything outside the list rect (rows are not clipped)
        c.fill_rect(R::new(g.list.l - sc(8, dpi), 0, c.width(), g.list.t), th.bg);
        c.fill_rect(
            R::new(g.list.l - sc(8, dpi), g.list.b, c.width(), c.height()),
            th.bg,
        );
        c.text(g.head_r, "Compare gamuts", self.fonts.heading, th.text, TA_LEFT);
        // scrollbar
        let content = self.content_h();
        if content > g.list.h() {
            let track = g.list.h() as f32;
            let th_len = (track * g.list.h() as f32 / content as f32).max(scf(24.0, dpi));
            let y0 = g.list.t as f32 + (track - th_len) * self.scroll as f32 / (content - g.list.h()) as f32;
            let x = g.list.r as f32 - scf(2.0, dpi);
            c.line(
                x,
                y0 + scf(2.0, dpi),
                x,
                y0 + th_len - scf(2.0, dpi),
                scf(3.0, dpi),
                th.text_dim,
                0.5,
            );
        }
        // description of the selected row
        if let Some(i) = self.selected {
            let e = &self.entries[i];
            c.text(
                R::new(g.desc.l, g.desc.t, g.desc.r, g.desc.t + sc(20, dpi)),
                &e.name,
                self.fonts.body,
                th.text,
                TA_LEFT,
            );
            let mut detail = e.detail.clone();
            if let (false, Some(p)) = (e.reference, e.prims) {
                detail.push_str(&format!(
                    "\nR {:.3}, {:.3}  G {:.3}, {:.3}  B {:.3}, {:.3}",
                    p.r.x, p.r.y, p.g.x, p.g.y, p.b.x, p.b.y
                ));
            }
            c.text_wrapped(
                R::new(g.desc.l, g.desc.t + sc(22, dpi), g.desc.r, g.desc.b),
                &detail,
                self.fonts.dim,
                th.text_dim,
            );
        } else {
            c.text_wrapped(
                g.desc,
                "Click a row to tick it and see what it is.",
                self.fonts.dim,
                th.text_dim,
            );
        }
        c.fill_rect(
            R::new(
                g.stats.l,
                g.stats.t - sc(6, dpi),
                g.stats.r,
                g.stats.t - sc(5, dpi),
            ),
            th.line,
        );
        c.text(
            R::new(g.stats.l, g.stats.t, g.stats.r, g.stats.t + sc(20, dpi)),
            &self.stats.0,
            self.fonts.body,
            th.text,
            TA_LEFT,
        );
        if !self.stats.1.is_empty() {
            c.text_wrapped(
                R::new(g.stats.l, g.stats.t + sc(22, dpi), g.stats.r, g.stats.b),
                &self.stats.1,
                self.fonts.dim,
                th.text_dim,
            );
        }
    }

    fn draw_row(&self, c: &mut Canvas, ctx: &Ctx, i: usize, r: R) {
        let th = &ctx.th;
        let dpi = self.dpi;
        let e = &self.entries[i];
        let plottable = e.prims.is_some();
        let rr = r.inset(sc(1, dpi));
        if self.selected == Some(i) {
            c.fill_rrect(rr, scf(6.0, dpi), th.surface, 1.0);
            c.stroke_rrect(rr, scf(6.0, dpi), 1.0, th.line, 1.0);
        } else if self.hover == Some(i) {
            c.fill_rrect(rr, scf(6.0, dpi), th.hover_fill(), 0.6);
        }
        let b = sc(18, dpi);
        let cb = R::xywh(r.l + sc(10, dpi), r.t + (r.h() - b) / 2, b, b);
        let cctx = Ctx {
            th: th.with_accent(e.color),
            f: ctx.f,
            dpi,
        };
        let cst = WState {
            hover: self.hover == Some(i),
            pressed: false,
            focus: false,
            enabled: plottable,
        };
        if plottable {
            draw_checkbox(c, &cctx, cb, "", e.checked, cst);
        } else {
            c.stroke_rrect(cb, scf(4.0, dpi), 1.2, mix(th.text_dim, th.bg, 0.5), 1.0);
        }
        let tag_w = if e.tag.is_empty() {
            0
        } else {
            c.text_width(&e.tag, ctx.f.dim) + sc(10, dpi)
        };
        let name_col = if plottable {
            th.text
        } else {
            mix(th.text_dim, th.bg, 0.3)
        };
        c.text(
            R::new(cb.r + sc(10, dpi), r.t, r.r - tag_w - sc(10, dpi), r.b),
            &e.name,
            ctx.f.body,
            name_col,
            TA_LEFT,
        );
        if tag_w > 0 {
            c.text(
                R::new(r.r - tag_w - sc(8, dpi), r.t, r.r - sc(10, dpi), r.b),
                &e.tag,
                ctx.f.dim,
                mix(th.text_dim, th.bg, if plottable { 0.0 } else { 0.3 }),
                TA_RIGHT,
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Math: thin wrappers over `crate::color`
// ---------------------------------------------------------------------------------------------

fn xy_rgb8(x: f64, y: f64) -> [u8; 3] {
    color::xyz_to_srgb8(color::xy_to_xyz(Xy { x, y }, 1.0))
}

fn math_coverage(a: &Primaries, r: &Primaries) -> f64 {
    color::coverage(a, r)
}

fn math_iou(a: &Primaries, b: &Primaries) -> f64 {
    color::iou(a, b)
}

/// Fills the horseshoe once: ARGB pixels (alpha 0xFF inside the locus, 0 outside), colors dimmed to 55 %.
fn build_horseshoe(w: i32, h: i32) -> Option<Dib> {
    let mut dib = Dib::new(w, h)?;
    let px = dib.pixels();
    px.fill(0);
    let poly: Vec<(f64, f64)> = SPECTRAL_LOCUS.iter().map(|&(_, x, y)| (x, y)).collect();
    let n = poly.len();
    for row in 0..h {
        let y = Y_MAX - (row as f64 + 0.5) / h as f64 * Y_MAX;
        let mut xs: Vec<f64> = Vec::with_capacity(8);
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            if (a.1 <= y && b.1 > y) || (b.1 <= y && a.1 > y) {
                let t = (y - a.1) / (b.1 - a.1);
                xs.push(a.0 + t * (b.0 - a.0));
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for pair in xs.chunks(2) {
            if pair.len() < 2 {
                break;
            }
            let c0 = ((pair[0] / X_MAX) * w as f64 - 0.5).ceil().max(0.0) as i32;
            let c1 = ((pair[1] / X_MAX) * w as f64 - 0.5).floor().min(w as f64 - 1.0) as i32;
            for col in c0..=c1 {
                let x = (col as f64 + 0.5) / w as f64 * X_MAX;
                let rgb = xy_rgb8(x, y);
                let d = |v: u8| (v as f32 * 0.55).round() as u32;
                px[(row * w + col) as usize] = 0xFF00_0000 | (d(rgb[0]) << 16) | (d(rgb[1]) << 8) | d(rgb[2]);
            }
        }
    }
    let _ = Xyz::default; // keep the import used when the fallback path is the only one compiled in tests
    Some(dib)
}

// ---------------------------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------------------------

/// The window handle (null when closed).
pub fn hwnd() -> HWND {
    HWND_CELL.with(|c| c.get()) as HWND
}

/// True while the window exists.
pub fn is_open() -> bool {
    !(HWND_CELL.with(|c| c.get()) as HWND).is_null()
}

pub fn open(selected_monitor_profile: Option<String>) {
    let existing = HWND_CELL.with(|c| c.get()) as HWND;
    if !existing.is_null() {
        // SAFETY: valid window handle.
        unsafe {
            if IsIconic(existing) != 0 {
                ShowWindow(existing, SW_RESTORE);
            }
            SetForegroundWindow(existing);
        }
        return;
    }
    register_class();
    let dpi = system_dpi();
    let (entries, monitor_entry) = build_entries(selected_monitor_profile.as_deref());
    let selected = monitor_entry.or(Some(0));
    let mut g = Gamut {
        hwnd: std::ptr::null_mut(),
        dpi,
        fonts: Fonts::new(dpi),
        canvas: None,
        horseshoe: None,
        entries,
        rows: Vec::new(),
        scroll: 0,
        selected,
        hover: None,
        tracking: false,
        monitor_entry,
        stats: (String::new(), String::new()),
        icons: [std::ptr::null_mut(); 2],
    };
    g.rebuild_rows();
    g.recompute();
    GAMUT.with(|m| *m.borrow_mut() = Some(g));

    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    let mut r = RECT {
        left: 0,
        top: 0,
        right: sc(CLIENT_W, dpi),
        bottom: sc(CLIENT_H, dpi),
    };
    let mut wa = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    // SAFETY: plain Win32 calls with valid pointers; GAMUT is not borrowed during window creation.
    unsafe {
        AdjustWindowRectExForDpi(&mut r, style, 0, 0, dpi);
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut RECT as *mut _, 0);
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let x = (wa.left + ((wa.right - wa.left) - w) / 2).max(wa.left);
        let y = (wa.top + ((wa.bottom - wa.top) - h) / 2).max(wa.top);
        let cls = wide(CLASS);
        let title = wide("Spectrum \u{2014} Compare gamuts");
        let hwnd = CreateWindowExW(
            0,
            cls.as_ptr(),
            title.as_ptr(),
            style,
            x,
            y,
            w,
            h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        if hwnd.is_null() {
            GAMUT.with(|m| *m.borrow_mut() = None);
            return;
        }
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        let actual = window_dpi(hwnd);
        if actual != dpi {
            on_dpi_changed(hwnd, actual, None);
        }
    }
}

pub fn close() {
    let h = HWND_CELL.with(|c| c.get()) as HWND;
    if !h.is_null() {
        // SAFETY: valid window handle owned by this thread.
        unsafe {
            DestroyWindow(h);
        }
    }
}

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

fn on_dpi_changed(hwnd: HWND, dpi: u32, suggested: Option<RECT>) {
    with(|g| {
        g.dpi = dpi;
        g.fonts = Fonts::new(dpi);
        g.canvas = None;
        g.horseshoe = None;
        g.clamp_scroll();
    });
    // SAFETY: valid window; GAMUT is not borrowed here.
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
                AdjustWindowRectExForDpi(&mut r, style, 0, 0, dpi);
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

fn lo(lp: LPARAM) -> i32 {
    (lp & 0xFFFF) as i16 as i32
}
fn hi(lp: LPARAM) -> i32 {
    ((lp >> 16) & 0xFFFF) as i16 as i32
}

fn icon_for(hwnd: HWND, metric: i32) -> HICON {
    let dpi = window_dpi(hwnd);
    // SAFETY: plain query.
    let px = unsafe { windows_sys::Win32::UI::HiDpi::GetSystemMetricsForDpi(metric, dpi) };
    crate::icon::create_hicon(px.max(16) as u32)
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            HWND_CELL.with(|c| c.set(hwnd as isize));
            apply_window_theme(hwnd, theme().dark);
            let big = icon_for(hwnd, SM_CXICON);
            let small = icon_for(hwnd, SM_CXSMICON);
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big as LPARAM);
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as LPARAM);
            with(|g| {
                g.hwnd = hwnd;
                g.icons = [big, small];
            });
            0
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            with(|g| g.paint(hdc));
            EndPaint(hwnd, &ps);
            0
        }
        WM_DPICHANGED => {
            let rc = *(lp as *const RECT);
            on_dpi_changed(hwnd, (wp & 0xFFFF) as u32, Some(rc));
            0
        }
        WM_SETTINGCHANGE => {
            if lp != 0 {
                let p = lp as *const u16;
                let mut n = 0;
                while n < 64 && *p.add(n) != 0 {
                    n += 1;
                }
                if String::from_utf16_lossy(std::slice::from_raw_parts(p, n)) == "ImmersiveColorSet" {
                    invalidate_theme();
                    apply_window_theme(hwnd, theme().dark);
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            }
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = (lo(lp), hi(lp));
            with(|g| {
                if !g.tracking {
                    let mut t = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut t);
                    g.tracking = true;
                }
                let h = g.row_at(x, y).filter(|_| true);
                if h != g.hover {
                    g.hover = h;
                    g.invalidate();
                }
            });
            0
        }
        WM_MOUSELEAVE => {
            with(|g| {
                g.tracking = false;
                if g.hover.take().is_some() {
                    g.invalidate();
                }
            });
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = (lo(lp), hi(lp));
            SetFocus(hwnd);
            with(|g| {
                if let Some(i) = g.row_at(x, y) {
                    g.toggle(i);
                    g.invalidate();
                }
            });
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp >> 16) & 0xFFFF) as i16 as i32;
            let mut pt = POINT { x: lo(lp), y: hi(lp) };
            ScreenToClient(hwnd, &mut pt);
            with(|g| {
                g.scroll -= delta * sc(ROW_H, g.dpi) / 120;
                g.clamp_scroll();
                g.invalidate();
            });
            0
        }
        WM_KEYDOWN => {
            let vk = wp as u16;
            if vk == VK_ESCAPE {
                DestroyWindow(hwnd);
                return 0;
            }
            with(|g| {
                match vk {
                    VK_DOWN => g.move_selection(1),
                    VK_UP => g.move_selection(-1),
                    VK_SPACE | VK_RETURN => {
                        if let Some(i) = g.selected {
                            g.toggle(i);
                        }
                    }
                    _ => {}
                }
                g.invalidate();
            });
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            HWND_CELL.with(|c| c.set(0));
            let g = GAMUT.with(|m| m.borrow_mut().take());
            if let Some(g) = &g {
                for i in g.icons {
                    if !i.is_null() {
                        DestroyIcon(i);
                    }
                }
            }
            drop(g); // fonts, canvas, horseshoe DIB
            trim_working_set();
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[allow(dead_code)]
fn _unused(_: Theme) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_names() {
        assert_eq!(short_name("Adobe RGB (1998)"), "Adobe RGB");
        assert_eq!(short_name("sRGB"), "sRGB");
    }

    #[test]
    fn reference_files_map() {
        assert_eq!(reference_file_id("sRGB Color Space Profile.icm"), Some(0));
        assert_eq!(reference_file_id("Spectrum Display P3.icm"), Some(1));
        assert_eq!(reference_file_id("foo.icc"), None);
    }
}
