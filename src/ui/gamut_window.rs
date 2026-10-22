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
    ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_DOWN, VK_ESCAPE,
    VK_RETURN, VK_SPACE, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, GetClientRect, IsIconic, LoadCursorW,
    RegisterClassExW, SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow, SystemParametersInfoW,
    CS_HREDRAW, CS_VREDRAW, HICON, ICON_BIG, ICON_SMALL, IDC_ARROW, SM_CXICON, SM_CXSMICON, SPI_GETWORKAREA,
    SWP_NOACTIVATE, SWP_NOZORDER, SW_RESTORE, SW_SHOW, WM_CAPTURECHANGED, WM_CLOSE, WM_CREATE, WM_DESTROY,
    WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_PAINT, WM_SETICON, WM_SETTINGCHANGE, WNDCLASSEXW, WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED,
    WS_SYSMENU,
};

const CLASS: &str = "SpectrumGamutWindow";
const CLIENT_W: i32 = 890;
const CLIENT_H: i32 = 610;
/// Plot domain.
const X_MIN: f64 = -0.05;
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
    /// Keyboard cursor (arrow keys); drawn only while `kbd` is set.
    selected: Option<usize>,
    kbd: bool,
    hover: Option<usize>,
    tracking: bool,
    /// Physical height of the info block (measured while painting).
    info_h: i32,
    /// Scrollbar thumb drag: offset of the mouse from the thumb top.
    drag: Option<i32>,
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
    /// The list container (surface fill, border, rounded corners).
    panel: R,
    /// The list viewport inside the container's padding: a whole number of rows tall.
    list: R,
    /// The info block (name, detail, coverage), anchored to the bottom of the right panel.
    info: R,
    head_l: R,
    head_r: R,
}

/// The list never shrinks below this many rows.
const MIN_LIST_ROWS: i32 = 3;
const PANEL_L: i32 = 598;
/// Clear space between the list container and the info block.
const INFO_GAP: i32 = 12;
/// Inner padding of the list container.
const LIST_PAD: i32 = 4;
/// Width reserved at the right edge of the container for the scrollbar.
const SCROLLBAR_W: i32 = 12;

/// `info_h` is the physical height of the info block; the list container fills the rest of the right
/// column. Whole rows are visible; the sub-row remainder is bottom padding inside the container.
fn geo(dpi: u32, info_h: i32) -> Geo {
    let s = |v: i32| sc(v, dpi);
    let pad = s(LIST_PAD);
    let row = s(ROW_H);
    let info_b = s(CLIENT_H - 16);
    let info_t = info_b - info_h;
    let top = s(40);
    let bottom = info_t - s(INFO_GAP);
    let rows = ((bottom - top - 2 * pad) / row).max(MIN_LIST_ROWS);
    let panel = R::new(
        s(PANEL_L),
        top,
        s(CLIENT_W - 20),
        (top + 2 * pad + rows * row).max(bottom),
    );
    Geo {
        plot: R::new(s(56), s(40), s(566), s(580)),
        head_l: R::new(s(56), s(10), s(566), s(34)),
        head_r: R::new(s(PANEL_L), s(10), s(CLIENT_W - 20), s(34)),
        panel,
        list: R::new(
            panel.l + pad,
            panel.t + pad,
            panel.r - pad,
            panel.t + pad + rows * row,
        ),
        info: R::new(s(PANEL_L), info_t, s(CLIENT_W - 20), info_b),
    }
}

/// Every list row (entry or header) has the same height, so scrolling and the viewport stay on whole rows.
const ROW_H: i32 = 30;
const HEAD_H: i32 = ROW_H;

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
        f if f.eq_ignore_ascii_case("Spectrum ProPhoto RGB.icm") => Some(3),
        f if f.eq_ignore_ascii_case("Spectrum Rec. 2020.icm") => Some(4),
        f if f.eq_ignore_ascii_case("Spectrum DCI-P3.icm") => Some(5),
        _ => None,
    }
}

/// Screenshot request: what to tick, and an in-memory entry for the monitor's EDID primaries when the
/// profile file is not installed (nothing is written anywhere).
struct Showcase {
    file: String,
    label: String,
    prims: Option<Primaries>,
}

thread_local! {
    static SHOWCASE: RefCell<Option<Showcase>> = const { RefCell::new(None) };
}

/// Opens the window with the monitor's EDID-derived profile, sRGB and Display P3 ticked (screenshots).
/// `mon` is the 0-based monitor index whose associations tag the profiles.
pub fn open_showcase(file: String, label: String, prims: Option<Primaries>, mon: usize) {
    SHOWCASE.with(|s| {
        *s.borrow_mut() = Some(Showcase {
            file: file.clone(),
            label,
            prims,
        })
    });
    open(Some(file), mon);
    SHOWCASE.with(|s| *s.borrow_mut() = None);
}

fn build_entries(selected_file: Option<&str>, mon: usize) -> (Vec<Entry>, Option<usize>) {
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
    let showcase = SHOWCASE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|s| (s.file.clone(), s.label.clone(), s.prims))
    });
    let mut profiles = if showcase.is_some() {
        crate::showcase::gamut_profiles()
    } else if super::is_mock() {
        mock_profiles()
    } else {
        installed_profiles(mon)
    };
    // Spectrum's EDID profile, then SDR-associated, then other plottable, then HDR, then not plottable.
    profiles.sort_by_key(|p| {
        if p.2.is_none() {
            4
        } else {
            match p.3.as_str() {
                "Spectrum" => 0,
                "SDR" => 1,
                "HDR" => 3,
                _ => 2,
            }
        }
    });
    if let Some((file, label, prims)) = &showcase {
        // The monitor's own profile goes first; build it from the EDID when it is not installed.
        match profiles.iter().position(|p| p.0.eq_ignore_ascii_case(file)) {
            Some(i) => {
                let p = profiles.remove(i);
                profiles.insert(0, p);
            }
            None => profiles.insert(0, (file.clone(), label.clone(), *prims, "Spectrum".to_string())),
        }
        entries[1].checked = true; // Display P3
    }
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
            detail: if plottable && file.starts_with("Spectrum") && file.ends_with("EDID.icm") {
                "Built by Spectrum from the monitor's EDID".to_string()
            } else if plottable {
                format!("{file} \u{b7} matrix/TRC profile")
            } else {
                format!("{file} \u{b7} LUT-based profile, no triangle to plot")
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

/// (file, name, primaries when matrix/TRC, tag). Tags follow the associations of monitor `mon_idx` (0-based).
fn installed_profiles(mon_idx: usize) -> Vec<(String, String, Option<Primaries>, String)> {
    let mut out = Vec::new();
    let mon = crate::app::current_monitors().into_iter().nth(mon_idx);
    let assoc = |hdr: bool| {
        mon.as_ref()
            .and_then(|m| crate::wcs::associated(m, hdr).ok())
            .unwrap_or_default()
    };
    let (sdr_assoc, hdr_assoc) = (assoc(false), assoc(true));
    for file in crate::wcs::list_installed() {
        let Some(bytes) = crate::wcs::read_profile(&file) else {
            continue;
        };
        let Ok(info) = crate::icc_read::parse(&bytes) else {
            continue;
        };
        if !info.is_display_rgb() {
            continue;
        }
        let name = if info.desc.trim().is_empty() {
            file.clone()
        } else {
            info.desc.clone()
        };
        let prims = if info.is_matrix_trc { info.primaries } else { None };
        // Tag by association with the current monitor, not by what is inside the file.
        let listed = |l: &[String]| l.iter().any(|f| f.eq_ignore_ascii_case(&file));
        let tag = if file.starts_with("Spectrum") && file.ends_with("EDID.icm") {
            "Spectrum"
        } else if listed(&hdr_assoc) {
            "HDR"
        } else if listed(&sdr_assoc) {
            "SDR"
        } else {
            ""
        }
        .to_string();
        out.push((file, name, prims, tag));
    }
    out
}

fn mock_profiles() -> Vec<(String, String, Option<Primaries>, String)> {
    let xy = |x, y| Xy { x, y };
    let w = xy(0.3127, 0.3290);
    vec![
        (
            "Spectrum QD-OLED 27 EDID.icm".to_string(),
            "QD-OLED 27 (EDID)".to_string(),
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
            "QD-OLED 27 HDR 450".to_string(),
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
        self.rows.push(Row::Header("Reference spaces"));
        for (i, e) in self.entries.iter().enumerate() {
            if e.reference {
                self.rows.push(Row::Entry(i));
            }
        }
        if self.entries.iter().any(|e| !e.reference) {
            self.rows.push(Row::Header("Installed profiles"));
            for (i, e) in self.entries.iter().enumerate() {
                if !e.reference {
                    self.rows.push(Row::Entry(i));
                }
            }
        }
    }

    fn geo(&self) -> Geo {
        geo(self.dpi, self.info_h)
    }

    /// Scrollbar geometry: (track, thumb top, thumb length, max scroll). None when the list fits.
    fn scrollbar(&self) -> Option<(R, i32, i32, i32)> {
        let g = self.geo();
        let content = self.content_h();
        if content <= g.list.h() {
            return None;
        }
        let max_scroll = content - g.list.h();
        let dpi = self.dpi;
        let tx = g.list.r - sc(7, dpi);
        let track = R::new(tx, g.list.t + sc(2, dpi), tx + sc(4, dpi), g.list.b - sc(2, dpi));
        let len = ((track.h() as f32 * g.list.h() as f32 / content as f32).max(scf(24.0, dpi))) as i32;
        let t = track.t + ((track.h() - len) as f32 * self.scroll as f32 / max_scroll as f32) as i32;
        Some((track, t, len, max_scroll))
    }

    /// Height of the info block: the tallest text it can show, so the list does not resize on hover.
    fn info_height(&self, c: &mut Canvas, width: i32) -> i32 {
        let dpi = self.dpi;
        let dim = self.fonts.dim;
        let block = |c: &mut Canvas, body: &str| c.measure_wrapped(width, body, dim) + sc(18, dpi);
        let mut tallest = 0;
        for i in 0..self.entries.len() {
            let (_, body) = self.row_info(i);
            tallest = tallest.max(block(c, &body));
            if let Some((_, body)) = self.coverage_info(i) {
                tallest = tallest.max(block(c, &body));
            }
        }
        tallest + sc(16, dpi)
    }

    /// Mouse press at (x, y): thumb starts a drag, track pages by the visible row count. True if handled.
    fn scrollbar_press(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some((track, t, len, _)) = self.scrollbar() else {
            return false;
        };
        let g = self.geo();
        let zone = R::new(g.list.r - sc(SCROLLBAR_W, self.dpi), g.list.t, g.list.r, g.list.b);
        if !zone.contains(x, y) {
            return false;
        }
        let _ = track;
        if y >= t && y < t + len {
            self.drag = Some(y - t);
            // SAFETY: valid window handle; released on button up or capture loss.
            unsafe { SetCapture(hwnd) };
        } else {
            let page = g.list.h() / sc(ROW_H, self.dpi) * sc(ROW_H, self.dpi);
            self.scroll += if y < t { -page } else { page };
            self.clamp_scroll();
        }
        true
    }

    /// Moves the list so the thumb top is at `thumb_top` (snapped to whole rows by `clamp_scroll`).
    fn scrollbar_drag_to(&mut self, thumb_top: i32) {
        let Some((track, _, len, max_scroll)) = self.scrollbar() else {
            return;
        };
        let span = (track.h() - len).max(1);
        let f = ((thumb_top - track.t) as f32 / span as f32).clamp(0.0, 1.0);
        self.scroll = (f * max_scroll as f32).round() as i32;
        self.clamp_scroll();
    }

    fn content_h(&self) -> i32 {
        self.rows.iter().map(|r| row_h(r, self.dpi)).sum()
    }

    fn clamp_scroll(&mut self) {
        let g = self.geo();
        let row = sc(ROW_H, self.dpi);
        let max = (self.content_h() - g.list.h()).max(0);
        let snapped = (self.scroll + row / 2) / row * row;
        self.scroll = snapped.clamp(0, max);
    }

    /// Entry index and its on-screen rect at a client point.
    fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        let g = self.geo();
        let sb = if self.content_h() > g.list.h() {
            sc(SCROLLBAR_W, self.dpi)
        } else {
            0
        };
        if !g.list.contains(x, y) || x >= g.list.r - sb {
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
        self.kbd = true;
        self.ensure_visible(order[next as usize]);
    }

    fn ensure_visible(&mut self, entry: usize) {
        let g = self.geo();
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

    /// Name and description of row `i`. An installed profile also lists its primaries.
    fn row_info(&self, i: usize) -> (String, String) {
        let e = &self.entries[i];
        let mut body = e.detail.clone();
        if let (false, Some(p)) = (e.reference, e.prims) {
            body.push_str(&format!(
                "\nR {:.3}, {:.3}  G {:.3}, {:.3}  B {:.3}, {:.3}",
                p.r.x, p.r.y, p.g.x, p.g.y, p.b.x, p.b.y
            ));
        }
        (e.name.clone(), body)
    }

    /// Coverage of sRGB, Display P3 and Adobe RGB by installed profile `i`, and its closest reference space.
    fn coverage_info(&self, i: usize) -> Option<(String, String)> {
        let e = &self.entries[i];
        if e.reference {
            return None;
        }
        let p = e.prims?;
        let cov = |idx: usize| (math_coverage(&p, &STANDARD_SPACES[idx].prims) * 100.0).round() as i32;
        let best = closest_space(&p);
        // No-break spaces inside each "Name NN%" unit: a line may only break between units.
        let unit = |idx: usize| {
            format!("{} {}%", short_name(STANDARD_SPACES[idx].name), cov(idx)).replace(' ', "\u{a0}")
        };
        Some((
            e.name.clone(),
            format!(
                "{} \u{b7} {} \u{b7} {}\nClosest match: {}",
                unit(0),
                unit(1),
                unit(2),
                short_name(STANDARD_SPACES[best].name),
            ),
        ))
    }

    /// Info block for the current state: the hovered row, else the first ticked installed profile.
    /// None when neither applies.
    fn info(&self) -> Option<(String, String)> {
        match self.hover {
            Some(i) => Some(self.row_info(i)),
            None => {
                let i = self
                    .entries
                    .iter()
                    .position(|e| !e.reference && e.checked && e.prims.is_some())?;
                self.coverage_info(i)
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

    fn ensure_horseshoe(&mut self) {
        let g = self.geo();
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
        let g = self.geo();
        (
            g.plot.l as f32 + ((x - X_MIN) / (X_MAX - X_MIN)) as f32 * g.plot.w() as f32,
            g.plot.t as f32 + ((Y_MAX - y) / Y_MAX) as f32 * g.plot.h() as f32,
        )
    }

    fn draw(&mut self, c: &mut Canvas) {
        let th = theme();
        let dpi = self.dpi;
        let width = self.geo().info.w();
        let need = self.info_height(c, width);
        if need != self.info_h {
            self.info_h = need;
            self.clamp_scroll();
            if let Some(i) = self.selected {
                self.ensure_visible(i);
            }
        }
        let g = self.geo();
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
            let label = nm.to_string();
            let (tw, thh) = (c.text_width(&label, self.fonts.dim), sc(14, dpi));
            let (hw, hh) = (tw / 2, thh / 2);
            let margin = sc(4, dpi);
            // On the outward normal, then clamped inside the plot so it never reaches the axis labels.
            let lx = (p.0 + nx * lr).clamp((g.plot.l + margin + hw) as f32, (g.plot.r - margin - hw) as f32);
            let ly = (p.1 + ny * lr).clamp((g.plot.t + margin + hh) as f32, (g.plot.b - margin - hh) as f32);
            let at = |dx: i32, dy: i32| {
                R::new(
                    lx as i32 - hw - sc(2, dpi) + dx,
                    ly as i32 - hh - sc(2, dpi) + dy,
                    lx as i32 + hw + sc(2, dpi) + dx,
                    ly as i32 + hh + sc(2, dpi) + dy,
                )
            };
            // 2 px halo in the plot background color, so the label reads over the locus fill
            let d = sc(2, dpi).max(2);
            for (dx, dy) in [
                (-d, 0),
                (d, 0),
                (0, -d),
                (0, d),
                (-d, -d),
                (d, -d),
                (-d, d),
                (d, d),
            ] {
                c.text(at(dx, dy), &label, self.fonts.dim, th.surface, TA_CENTER);
            }
            c.text(at(0, 0), &label, self.fonts.dim, th.text_dim, TA_CENTER);
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

        // --- right: list container ---
        let ctx = Ctx {
            th,
            f: &self.fonts,
            dpi,
        };
        let radius = scf(6.0, dpi);
        c.fill_rrect(g.panel, radius, th.surface, 1.0);
        let content = self.content_h();
        let overflow = content > g.list.h();
        let sb_w = if overflow { sc(SCROLLBAR_W, dpi) } else { 0 };
        let rows_r = g.list.r - sb_w;
        // Rows are whole and aligned with the viewport, so they clip exactly at its edges.
        let mut top = g.list.t - self.scroll;
        for r in &self.rows {
            let h = row_h(r, dpi);
            if top >= g.list.t && top + h <= g.list.b {
                match r {
                    Row::Header(t) => {
                        c.text(
                            R::new(g.list.l + sc(8, dpi), top, rows_r, top + h),
                            t,
                            self.fonts.dim,
                            th.text_dim,
                            TA_LEFT,
                        );
                    }
                    Row::Entry(i) => self.draw_row(c, &ctx, *i, R::new(g.list.l, top, rows_r, top + h)),
                }
            }
            top += h;
        }
        if overflow {
            let fade_h = sc(24, dpi);
            let max_scroll = content - g.list.h();
            let fade = |c: &mut Canvas, from_top: bool| {
                for k in 0..fade_h {
                    // 0 at the inner end of the fade, ~1 at the container edge
                    let t = (k + 1) as f32 / fade_h as f32;
                    let alpha = 0.96 * t * t;
                    let y = if from_top {
                        g.list.t + fade_h - 1 - k
                    } else {
                        g.list.b - fade_h + k
                    };
                    c.blend_rect(R::new(g.list.l, y, rows_r, y + 1), th.surface, alpha);
                }
            };
            if self.scroll > 0 {
                fade(c, true);
            }
            if self.scroll < max_scroll {
                fade(c, false);
                let first_hidden_top = self.scroll + g.list.h();
                let mut y = 0;
                let mut more = 0;
                for r in &self.rows {
                    if y >= first_hidden_top && matches!(r, Row::Entry(_)) {
                        more += 1;
                    }
                    y += row_h(r, dpi);
                }
                if more > 0 {
                    let label = format!("{more} more");
                    let w = c.text_width(&label, self.fonts.dim) + sc(14, dpi);
                    let pill = R::new(
                        rows_r - sc(6, dpi) - w,
                        g.list.b - sc(22, dpi),
                        rows_r - sc(6, dpi),
                        g.list.b - sc(4, dpi),
                    );
                    c.fill_rrect(pill, scf(9.0, dpi), th.surface, 1.0);
                    c.text(pill, &label, self.fonts.dim, th.text_dim, TA_CENTER);
                }
            }
            // scrollbar: the track stays visible while the list overflows
            if let Some((track, t, len, _)) = self.scrollbar() {
                c.fill_rrect(track, scf(2.0, dpi), mix(th.surface, th.text_dim, 0.22), 1.0);
                let alpha = if self.drag.is_some() { 1.0 } else { 0.85 };
                c.fill_rrect(
                    R::new(track.l, t, track.r, t + len),
                    scf(2.0, dpi),
                    th.text_dim,
                    alpha,
                );
            }
        }
        c.stroke_rrect(g.panel, radius, scf(1.0, dpi).floor().max(1.0), th.line, 1.0);
        c.text(g.head_r, "Compare gamuts", self.fonts.heading, th.text, TA_LEFT);

        // --- right: one info block anchored to the bottom, under a divider ---
        let width = g.info.w();
        let mut y = g.info.b;
        if let Some((title, body)) = self.info() {
            let h = c.measure_wrapped(width, &body, self.fonts.dim);
            y -= h;
            c.text_wrapped(
                R::new(g.info.l, y, g.info.r, y + h),
                &body,
                self.fonts.dim,
                th.text_dim,
            );
            y -= sc(18, dpi);
            c.text(
                R::new(g.info.l, y, g.info.r, y + sc(18, dpi)),
                &title,
                self.fonts.title,
                th.text,
                TA_LEFT,
            );
        }
        let line_y = g.info.t + sc(8, dpi);
        c.fill_rect(
            R::new(g.info.l, line_y, g.info.r, line_y + sc(1, dpi).max(1)),
            th.line,
        );
    }

    fn draw_row(&self, c: &mut Canvas, ctx: &Ctx, i: usize, r: R) {
        let th = &ctx.th;
        let dpi = self.dpi;
        let e = &self.entries[i];
        let plottable = e.prims.is_some();
        let rr = r.inset(sc(1, dpi));
        if self.hover == Some(i) {
            c.fill_rrect(rr, scf(6.0, dpi), th.hover_fill(), 0.5);
        }
        if self.kbd && self.selected == Some(i) {
            c.stroke_rrect(rr, scf(6.0, dpi), scf(1.5, dpi), th.text_dim, 1.0);
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

/// Index of the reference space closest to `p`: highest overlap (IoU); spaces within 0.005 of the best
/// (Display P3 and DCI-P3 share primaries) are separated by the white-point distance.
fn closest_space(p: &Primaries) -> usize {
    let ious: Vec<f64> = STANDARD_SPACES.iter().map(|s| math_iou(p, &s.prims)).collect();
    let top = ious.iter().cloned().fold(f64::MIN, f64::max);
    let dist =
        |s: &color::StandardSpace| ((p.w.x - s.prims.w.x).powi(2) + (p.w.y - s.prims.w.y).powi(2)).sqrt();
    (0..STANDARD_SPACES.len())
        .filter(|&i| ious[i] >= top - 0.005)
        .min_by(|&a, &b| {
            dist(&STANDARD_SPACES[a])
                .partial_cmp(&dist(&STANDARD_SPACES[b]))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0)
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
            let c0 = (((pair[0] - X_MIN) / (X_MAX - X_MIN)) * w as f64 - 0.5)
                .ceil()
                .max(0.0) as i32;
            let c1 = (((pair[1] - X_MIN) / (X_MAX - X_MIN)) * w as f64 - 0.5)
                .floor()
                .min(w as f64 - 1.0) as i32;
            for col in c0..=c1 {
                let x = X_MIN + (col as f64 + 0.5) / w as f64 * (X_MAX - X_MIN);
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

/// `mon` is the 0-based index of the monitor selected in the main window: its SDR and HDR associations
/// decide which profiles are tagged SDR and HDR.
pub fn open(selected_monitor_profile: Option<String>, mon: usize) {
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
    let (entries, monitor_entry) = build_entries(selected_monitor_profile.as_deref(), mon);
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
        kbd: false,
        hover: None,
        tracking: false,
        info_h: sc(110, dpi),
        drag: None,
        icons: [std::ptr::null_mut(); 2],
    };
    g.rebuild_rows();
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
        AdjustWindowRectExForDpi(&mut r, style, 0, 0, super::draw::frame_dpi(dpi));
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut RECT as *mut _, 0);
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let x = (wa.left + ((wa.right - wa.left) - w) / 2).max(wa.left);
        let y = (wa.top + ((wa.bottom - wa.top) - h) / 2).max(wa.top);
        let cls = wide(CLASS);
        let title = wide("Spectrum: compare gamuts");
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
    refresh_icons(hwnd, dpi);
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

fn lo(lp: LPARAM) -> i32 {
    (lp & 0xFFFF) as i16 as i32
}
fn hi(lp: LPARAM) -> i32 {
    ((lp >> 16) & 0xFFFF) as i16 as i32
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
            if wp == 0 && lp != 0 {
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
                if let Some(off) = g.drag {
                    g.scrollbar_drag_to(y - off);
                    g.invalidate();
                    return;
                }
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
                let h = g.row_at(x, y);
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
                g.kbd = false;
                if g.scrollbar_press(hwnd, x, y) {
                    g.invalidate();
                } else if let Some(i) = g.row_at(x, y) {
                    g.toggle(i);
                    g.invalidate();
                }
            });
            0
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED => {
            with(|g| {
                if g.drag.take().is_some() {
                    g.invalidate();
                }
            });
            if msg == WM_LBUTTONUP {
                ReleaseCapture();
            }
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp >> 16) & 0xFFFF) as i16 as i32;
            let mut pt = POINT { x: lo(lp), y: hi(lp) };
            ScreenToClient(hwnd, &mut pt);
            with(|g| {
                // one row per wheel notch
                let steps = (delta.abs() / 120).max(1) * delta.signum();
                g.scroll -= steps * sc(ROW_H, g.dpi);
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
            let g = GAMUT.with(|m| m.try_borrow_mut().ok().and_then(|mut b| b.take()));
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
    fn closest_match_breaks_primary_ties_by_white_point() {
        // The QD-OLED 27 EDID: Display P3 primaries with a near-D65 white. DCI-P3 shares the primaries
        // (a tie on overlap), but its white is far greener, so Display P3 must win.
        let xy = |x, y| Xy { x, y };
        let mag = Primaries {
            r: xy(0.6855, 0.3037),
            g: xy(0.2402, 0.7119),
            b: xy(0.1436, 0.0576),
            w: xy(0.3135, 0.3291),
        };
        assert_eq!(STANDARD_SPACES[closest_space(&mag)].name, "Display P3");
        let p3 = STANDARD_SPACES[1].prims;
        assert_eq!(STANDARD_SPACES[closest_space(&p3)].name, "Display P3");
        let dci = STANDARD_SPACES[5].prims;
        assert_eq!(STANDARD_SPACES[closest_space(&dci)].name, "DCI-P3");
    }

    #[test]
    fn reference_files_map() {
        assert_eq!(reference_file_id("sRGB Color Space Profile.icm"), Some(0));
        assert_eq!(reference_file_id("Spectrum Display P3.icm"), Some(1));
        assert_eq!(reference_file_id("foo.icc"), None);
    }
}
