//! Drawing foundation: theme tokens (light/dark), DPI scaling, cached fonts, anti-aliased shape painting
//! on a 32-bit DIB back buffer, and GDI text.
//!
//! Shapes (rounded rects, circles, lines) are rasterised here with signed-distance coverage so they are
//! anti-aliased without GDI+; text goes through GDI (ClearType) onto the same DIB.

#![allow(clippy::too_many_arguments, clippy::not_unsafe_ptr_arg_deref)]

use crate::util::wide;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI8, AtomicU32, AtomicU8, Ordering};
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW,
    EnumFontFamiliesExW, GdiFlush, GetDC, GetTextExtentPoint32W, ReleaseDC, SelectObject, SetBkMode,
    SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLEARTYPE_QUALITY, DEFAULT_CHARSET, DIB_RGB_COLORS,
    HBITMAP, HDC, HFONT, HGDIOBJ, LOGFONTW, SRCCOPY, TEXTMETRICW, TRANSPARENT,
};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};

// ---------------------------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub dark: bool,
    pub bg: u32,
    pub surface: u32,
    pub text: u32,
    pub text_dim: u32,
    pub line: u32,
    pub accent: u32,
}

/// 0 = follow system, 1 = force dark, 2 = force light.
static FORCED: AtomicU8 = AtomicU8::new(0);
/// -1 = unknown, 0 = light, 1 = dark.
static SYSTEM_DARK: AtomicI8 = AtomicI8::new(-1);

/// WM_MOUSELEAVE, re-exported for the UI modules.
pub use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;

pub const DEFAULT_ACCENT: u32 = 0xFF5A3C;

fn read_system_dark() -> bool {
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let val = wide("AppsUseLightTheme");
    let mut data: u32 = 1;
    let mut size: u32 = 4;
    // SAFETY: valid NUL-terminated strings and a 4-byte output buffer.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            val.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut data as *mut u32 as *mut c_void,
            &mut size,
        )
    };
    rc == 0 && data == 0
}

/// Follows AppsUseLightTheme unless overridden by `set_forced_theme`.
pub fn theme() -> Theme {
    let dark = match FORCED.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let mut c = SYSTEM_DARK.load(Ordering::Relaxed);
            if c < 0 {
                c = read_system_dark() as i8;
                SYSTEM_DARK.store(c, Ordering::Relaxed);
            }
            c == 1
        }
    };
    if dark {
        Theme {
            dark,
            bg: 0x1C1B1A,
            surface: 0x262523,
            text: 0xEDE9E3,
            text_dim: 0xA39D94,
            line: 0x3A3733,
            accent: DEFAULT_ACCENT,
        }
    } else {
        Theme {
            dark,
            bg: 0xF7F5F2,
            surface: 0xFFFFFF,
            text: 0x1A1917,
            text_dim: 0x6B655D,
            line: 0xE2DDD5,
            accent: DEFAULT_ACCENT,
        }
    }
}

pub fn set_forced_theme(dark: Option<bool>) {
    FORCED.store(
        match dark {
            None => 0,
            Some(true) => 1,
            Some(false) => 2,
        },
        Ordering::Relaxed,
    );
}

/// Forget the cached system theme (call on WM_SETTINGCHANGE "ImmersiveColorSet").
pub fn invalidate_theme() {
    SYSTEM_DARK.store(-1, Ordering::Relaxed);
}

impl Theme {
    pub fn with_accent(mut self, accent: u32) -> Theme {
        self.accent = accent;
        self
    }
    /// Slightly lighter (dark) / tinted (light) control fill on hover.
    pub fn hover_fill(&self) -> u32 {
        if self.dark {
            mix(self.surface, 0xFFFFFF, 0.07)
        } else {
            mix(self.surface, self.text, 0.05)
        }
    }
    /// Darker control fill while pressed.
    pub fn pressed_fill(&self) -> u32 {
        if self.dark {
            mix(self.surface, 0x000000, 0.22)
        } else {
            mix(self.surface, self.text, 0.11)
        }
    }
    /// Text/glyph color to use on top of the accent color.
    pub fn on_accent(&self) -> u32 {
        on_color(self.accent)
    }
    /// Accent suitable for text on the window background (darkened on light themes for contrast).
    pub fn accent_text(&self) -> u32 {
        if self.dark {
            self.accent
        } else {
            mix(self.accent, 0x000000, 0.38)
        }
    }
}

/// 0xRRGGBB to GDI COLORREF.
pub fn colorref(c: u32) -> u32 {
    ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)
}

pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |s: u32| {
        let x = ((a >> s) & 0xFF) as f32;
        let y = ((b >> s) & 0xFF) as f32;
        ((x + (y - x) * t).round() as u32) & 0xFF
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// Relative luminance 0..1 of a 0xRRGGBB color (sRGB approximation).
pub fn luminance(c: u32) -> f32 {
    let f = |s: u32| {
        let v = ((c >> s) & 0xFF) as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(16) + 0.7152 * f(8) + 0.0722 * f(0)
}

/// Near-black or white, whichever reads on `bg`.
pub fn on_color(bg: u32) -> u32 {
    if luminance(bg) > 0.38 {
        0x1A1917
    } else {
        0xFFFFFF
    }
}

// ---------------------------------------------------------------------------------------------
// DPI and DWM helpers
// ---------------------------------------------------------------------------------------------

/// Logical px to physical px.
pub fn sc(v: i32, dpi: u32) -> i32 {
    ((v as f32) * dpi as f32 / 96.0).round() as i32
}

pub fn scf(v: f32, dpi: u32) -> f32 {
    v * dpi as f32 / 96.0
}

/// UI scale override in thousandths (`--debug-scale`, screenshots only): 1000 = the real DPI.
static UI_SCALE_MILLI: AtomicU32 = AtomicU32::new(1000);

/// Makes every window lay out and draw at `f` times the real DPI scale (screenshots only).
pub fn set_ui_scale(f: f64) {
    if f.is_finite() && (0.25..=4.0).contains(&f) {
        UI_SCALE_MILLI.store((f * 1000.0).round() as u32, Ordering::Relaxed);
    }
}

fn scaled(d: u32) -> u32 {
    let m = UI_SCALE_MILLI.load(Ordering::Relaxed);
    if m == 1000 {
        d
    } else {
        (d as f64 * m as f64 / 1000.0).round() as u32
    }
}

/// The DPI the system actually uses for window frames and title bars, given a (possibly scaled) UI DPI.
pub fn frame_dpi(dpi: u32) -> u32 {
    let m = UI_SCALE_MILLI.load(Ordering::Relaxed);
    if m == 1000 {
        dpi
    } else {
        (dpi as f64 * 1000.0 / m as f64).round() as u32
    }
}

pub fn system_dpi() -> u32 {
    // SAFETY: no preconditions.
    let d = unsafe { GetDpiForSystem() };
    scaled(if d == 0 { 96 } else { d })
}

pub fn window_dpi(hwnd: HWND) -> u32 {
    // SAFETY: plain query.
    let d = unsafe { GetDpiForWindow(hwnd) };
    if d == 0 {
        system_dpi()
    } else {
        scaled(d)
    }
}

/// The theme forced with `--theme`, if any (true = dark).
pub fn forced_theme() -> Option<bool> {
    match FORCED.load(Ordering::Relaxed) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// Dark title bar for the theme and rounded corners (Windows 11); ignored where unsupported.
pub fn apply_window_theme(hwnd: HWND, dark: bool) {
    let d: i32 = dark as i32;
    let round: i32 = 2; // DWMWCP_ROUND
                        // SAFETY: attribute pointers reference live 4-byte locals.
    unsafe {
        DwmSetWindowAttribute(hwnd, 20, &d as *const i32 as *const c_void, 4);
        DwmSetWindowAttribute(hwnd, 33, &round as *const i32 as *const c_void, 4);
    }
}

/// Releases pages the process no longer needs (called after a window and its GDI caches are freed).
pub fn trim_working_set() {
    // SAFETY: pseudo-handle of the current process.
    unsafe {
        windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet(
            windows_sys::Win32::System::Threading::GetCurrentProcess(),
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Integer rectangle
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct R {
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
}

impl R {
    pub fn new(l: i32, t: i32, r: i32, b: i32) -> R {
        R { l, t, r, b }
    }
    pub fn xywh(x: i32, y: i32, w: i32, h: i32) -> R {
        R {
            l: x,
            t: y,
            r: x + w,
            b: y + h,
        }
    }
    pub fn w(&self) -> i32 {
        self.r - self.l
    }
    pub fn h(&self) -> i32 {
        self.b - self.t
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.l && x < self.r && y >= self.t && y < self.b
    }
    pub fn inset(&self, d: i32) -> R {
        R {
            l: self.l + d,
            t: self.t + d,
            r: self.r - d,
            b: self.b - d,
        }
    }
    pub fn offset(&self, dx: i32, dy: i32) -> R {
        R {
            l: self.l + dx,
            t: self.t + dy,
            r: self.r + dx,
            b: self.b + dy,
        }
    }
    pub fn cx(&self) -> f32 {
        (self.l + self.r) as f32 / 2.0
    }
    pub fn cy(&self) -> f32 {
        (self.t + self.b) as f32 / 2.0
    }
    fn to_rect(self) -> RECT {
        RECT {
            left: self.l,
            top: self.t,
            right: self.r,
            bottom: self.b,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------------------------

pub struct Fonts {
    pub body: HFONT,
    pub dim: HFONT,
    pub heading: HFONT,
    /// Semibold body size, for a title line above `body` text.
    pub title: HFONT,
    pub big: HFONT,
}

unsafe extern "system" fn enum_font_cb(
    _lf: *const LOGFONTW,
    _tm: *const TEXTMETRICW,
    _ft: u32,
    lp: LPARAM,
) -> i32 {
    // SAFETY: lp points at a bool owned by font_exists.
    *(lp as *mut bool) = true;
    0
}

fn font_exists(face: &str) -> bool {
    // SAFETY: LOGFONTW zero-init is valid; callback only writes the bool.
    unsafe {
        let hdc = GetDC(std::ptr::null_mut());
        let mut lf: LOGFONTW = std::mem::zeroed();
        lf.lfCharSet = DEFAULT_CHARSET;
        for (i, c) in face.encode_utf16().take(31).enumerate() {
            lf.lfFaceName[i] = c;
        }
        let mut found = false;
        EnumFontFamiliesExW(hdc, &lf, Some(enum_font_cb), &mut found as *mut bool as LPARAM, 0);
        ReleaseDC(std::ptr::null_mut(), hdc);
        found
    }
}

/// (text face, display face)
fn faces() -> &'static (&'static str, &'static str) {
    static F: OnceLock<(&'static str, &'static str)> = OnceLock::new();
    F.get_or_init(|| {
        let text = if font_exists("Segoe UI Variable Text") {
            "Segoe UI Variable Text"
        } else {
            "Segoe UI"
        };
        let display = if font_exists("Segoe UI Variable Display") {
            "Segoe UI Variable Display"
        } else {
            "Segoe UI"
        };
        (text, display)
    })
}

pub fn display_face() -> &'static str {
    faces().1
}

pub fn make_font(face: &str, pt: f32, weight: i32, dpi: u32, quality: u8) -> HFONT {
    let h = -((pt * dpi as f32 / 72.0).round() as i32);
    make_font_px(face, h, weight, quality)
}

/// `height` negative = em height in pixels.
pub fn make_font_px(face: &str, height: i32, weight: i32, quality: u8) -> HFONT {
    let f = wide(face);
    // SAFETY: valid NUL-terminated face name.
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            quality as u32,
            0,
            f.as_ptr(),
        )
    }
}

impl Fonts {
    pub fn new(dpi: u32) -> Fonts {
        let (text, display) = *faces();
        Fonts {
            body: make_font(text, 10.0, 400, dpi, CLEARTYPE_QUALITY),
            dim: make_font(text, 9.0, 400, dpi, CLEARTYPE_QUALITY),
            heading: make_font(display, 13.0, 600, dpi, CLEARTYPE_QUALITY),
            title: make_font(text, 10.0, 600, dpi, CLEARTYPE_QUALITY),
            big: make_font(display, 28.0, 600, dpi, CLEARTYPE_QUALITY),
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        // SAFETY: handles created by CreateFontW and owned here.
        unsafe {
            for f in [self.body, self.dim, self.heading, self.title, self.big] {
                if !f.is_null() {
                    DeleteObject(f as HGDIOBJ);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// DIB section
// ---------------------------------------------------------------------------------------------

/// A top-down 32-bit DIB section. Pixels are `0x00RRGGBB` (or ARGB where the caller says so).
pub struct Dib {
    pub hbm: HBITMAP,
    pub bits: *mut u32,
    pub w: i32,
    pub h: i32,
}

impl Dib {
    pub fn new(w: i32, h: i32) -> Option<Dib> {
        if w <= 0 || h <= 0 {
            return None;
        }
        // SAFETY: standard DIB-section creation; bits pointer is valid while hbm lives.
        unsafe {
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let hbm = CreateDIBSection(
                std::ptr::null_mut(),
                &bmi,
                DIB_RGB_COLORS,
                &mut bits,
                std::ptr::null_mut(),
                0,
            );
            if hbm.is_null() || bits.is_null() {
                return None;
            }
            Some(Dib {
                hbm,
                bits: bits as *mut u32,
                w,
                h,
            })
        }
    }

    pub fn pixels(&mut self) -> &mut [u32] {
        // SAFETY: the DIB owns w*h u32s at `bits`.
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.w * self.h) as usize) }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        // SAFETY: owned bitmap handle.
        unsafe {
            DeleteObject(self.hbm as HGDIOBJ);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Canvas
// ---------------------------------------------------------------------------------------------

pub const TA_LEFT: u32 = 0;
pub const TA_CENTER: u32 = 1;
pub const TA_RIGHT: u32 = 2;

/// Back buffer: a memory DC with a DIB selected. All painting happens here, then `present` blits it.
pub struct Canvas {
    pub dc: HDC,
    dib: Dib,
    old: HGDIOBJ,
    dirty_gdi: bool,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Option<Canvas> {
        let dib = Dib::new(w, h)?;
        // SAFETY: memory DC compatible with the screen.
        unsafe {
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            if dc.is_null() {
                return None;
            }
            let old = SelectObject(dc, dib.hbm as HGDIOBJ);
            SetBkMode(dc, TRANSPARENT as i32);
            Some(Canvas {
                dc,
                dib,
                old,
                dirty_gdi: false,
            })
        }
    }

    pub fn width(&self) -> i32 {
        self.dib.w
    }
    pub fn height(&self) -> i32 {
        self.dib.h
    }

    /// Ensure GDI text drawn earlier is visible in the pixel memory.
    fn sync(&mut self) {
        if self.dirty_gdi {
            // SAFETY: no preconditions.
            unsafe {
                GdiFlush();
            }
            self.dirty_gdi = false;
        }
    }

    pub fn present(&mut self, dst: HDC, x: i32, y: i32) {
        self.sync();
        // SAFETY: both DCs valid.
        unsafe {
            BitBlt(dst, x, y, self.dib.w, self.dib.h, self.dc, 0, 0, SRCCOPY);
        }
    }

    pub fn clear(&mut self, color: u32) {
        self.sync();
        self.dib.pixels().fill(color & 0xFFFFFF);
    }

    pub fn fill_rect(&mut self, r: R, color: u32) {
        self.sync();
        let (w, h) = (self.dib.w, self.dib.h);
        let (l, t, rr, b) = (r.l.max(0), r.t.max(0), r.r.min(w), r.b.min(h));
        if l >= rr || t >= b {
            return;
        }
        let px = self.dib.pixels();
        for y in t..b {
            px[(y * w + l) as usize..(y * w + rr) as usize].fill(color & 0xFFFFFF);
        }
    }

    pub fn blend_rect(&mut self, r: R, color: u32, alpha: f32) {
        let (w, h) = (self.dib.w, self.dib.h);
        self.sync();
        let px = self.dib.pixels();
        for y in r.t.max(0)..r.b.min(h) {
            for x in r.l.max(0)..r.r.min(w) {
                blend_px(&mut px[(y * w + x) as usize], color, alpha);
            }
        }
    }

    /// Generic signed-distance painter: `cov(x, y)` returns coverage 0..1 for the pixel center.
    fn paint_sdf(
        &mut self,
        bbox: (f32, f32, f32, f32),
        color: u32,
        alpha: f32,
        cov: impl Fn(f32, f32) -> f32,
    ) {
        self.sync();
        let (w, h) = (self.dib.w, self.dib.h);
        let x0 = (bbox.0.floor() as i32 - 1).max(0);
        let y0 = (bbox.1.floor() as i32 - 1).max(0);
        let x1 = (bbox.2.ceil() as i32 + 1).min(w);
        let y1 = (bbox.3.ceil() as i32 + 1).min(h);
        let px = self.dib.pixels();
        for y in y0..y1 {
            for x in x0..x1 {
                let c = cov(x as f32 + 0.5, y as f32 + 0.5);
                if c > 0.0 {
                    blend_px(&mut px[(y * w + x) as usize], color, c * alpha);
                }
            }
        }
    }

    pub fn fill_rrect(&mut self, r: R, radius: f32, color: u32, alpha: f32) {
        self.fill_rrect_f(
            r.l as f32, r.t as f32, r.r as f32, r.b as f32, radius, color, alpha,
        );
    }

    pub fn fill_rrect_f(&mut self, l: f32, t: f32, r: f32, b: f32, radius: f32, color: u32, alpha: f32) {
        self.paint_sdf((l, t, r, b), color, alpha, move |x, y| {
            (0.5 - sd_rrect(x, y, l, t, r, b, radius)).clamp(0.0, 1.0)
        });
    }

    /// Border drawn inside the rect.
    pub fn stroke_rrect(&mut self, r: R, radius: f32, width: f32, color: u32, alpha: f32) {
        self.stroke_rrect_f(
            r.l as f32, r.t as f32, r.r as f32, r.b as f32, radius, width, color, alpha,
        );
    }

    pub fn stroke_rrect_f(
        &mut self,
        l: f32,
        t: f32,
        r: f32,
        b: f32,
        radius: f32,
        width: f32,
        color: u32,
        alpha: f32,
    ) {
        self.paint_sdf((l, t, r, b), color, alpha, move |x, y| {
            let d = sd_rrect(x, y, l, t, r, b, radius);
            (0.5 - ((d + width / 2.0).abs() - width / 2.0)).clamp(0.0, 1.0)
        });
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, rad: f32, color: u32, alpha: f32) {
        self.paint_sdf(
            (cx - rad, cy - rad, cx + rad, cy + rad),
            color,
            alpha,
            move |x, y| (0.5 - (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - rad)).clamp(0.0, 1.0),
        );
    }

    pub fn stroke_circle(&mut self, cx: f32, cy: f32, rad: f32, width: f32, color: u32, alpha: f32) {
        self.paint_sdf(
            (cx - rad, cy - rad, cx + rad, cy + rad),
            color,
            alpha,
            move |x, y| {
                let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - rad;
                (0.5 - ((d + width / 2.0).abs() - width / 2.0)).clamp(0.0, 1.0)
            },
        );
    }

    /// Anti-aliased line with round caps.
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: u32, alpha: f32) {
        let hw = width / 2.0;
        let bbox = (x0.min(x1) - hw, y0.min(y1) - hw, x0.max(x1) + hw, y0.max(y1) + hw);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        self.paint_sdf(bbox, color, alpha, move |x, y| {
            let t = (((x - x0) * dx + (y - y0) * dy) / len2).clamp(0.0, 1.0);
            let (px, py) = (x0 + t * dx, y0 + t * dy);
            let d = ((x - px).powi(2) + (y - py).powi(2)).sqrt() - hw;
            (0.5 - d).clamp(0.0, 1.0)
        });
    }

    pub fn polyline(&mut self, pts: &[(f32, f32)], closed: bool, width: f32, color: u32, alpha: f32) {
        for i in 0..pts.len().saturating_sub(1) {
            self.line(
                pts[i].0,
                pts[i].1,
                pts[i + 1].0,
                pts[i + 1].1,
                width,
                color,
                alpha,
            );
        }
        if closed && pts.len() > 2 {
            let (a, b) = (pts[pts.len() - 1], pts[0]);
            self.line(a.0, a.1, b.0, b.1, width, color, alpha);
        }
    }

    /// Composite premultiplied BGRA (top-down) at (x, y).
    pub fn blit_premul_bgra(&mut self, x: i32, y: i32, w: i32, h: i32, bgra: &[u8]) {
        self.sync();
        let (cw, ch) = (self.dib.w, self.dib.h);
        let px = self.dib.pixels();
        for j in 0..h {
            let dy = y + j;
            if dy < 0 || dy >= ch {
                continue;
            }
            for i in 0..w {
                let dx = x + i;
                if dx < 0 || dx >= cw {
                    continue;
                }
                let o = ((j * w + i) * 4) as usize;
                if o + 3 >= bgra.len() {
                    return;
                }
                let a = bgra[o + 3] as u32;
                if a == 0 {
                    continue;
                }
                let d = &mut px[(dy * cw + dx) as usize];
                let inv = 255 - a;
                let r = bgra[o + 2] as u32 + ((*d >> 16) & 0xFF) * inv / 255;
                let g = bgra[o + 1] as u32 + ((*d >> 8) & 0xFF) * inv / 255;
                let b = bgra[o] as u32 + (*d & 0xFF) * inv / 255;
                *d = (r.min(255) << 16) | (g.min(255) << 8) | b.min(255);
            }
        }
    }

    /// Composite an ARGB source (alpha byte 0 = transparent, otherwise opaque) at (x, y).
    pub fn blit_masked(&mut self, x: i32, y: i32, src: &mut Dib) {
        self.sync();
        let (cw, ch) = (self.dib.w, self.dib.h);
        let (sw, sh) = (src.w, src.h);
        let sp = src.pixels();
        let px = self.dib.pixels();
        for j in 0..sh {
            let dy = y + j;
            if dy < 0 || dy >= ch {
                continue;
            }
            for i in 0..sw {
                let dx = x + i;
                if dx < 0 || dx >= cw {
                    continue;
                }
                let s = sp[(j * sw + i) as usize];
                if s >> 24 != 0 {
                    px[(dy * cw + dx) as usize] = s & 0xFFFFFF;
                }
            }
        }
    }

    // ---- text -------------------------------------------------------------------------------

    /// Single-line text in `r`, vertically centered, with ellipsis. `align` is TA_*.
    pub fn text(&mut self, r: R, s: &str, font: HFONT, color: u32, align: u32) {
        let flags = 0x0004 /*DT_VCENTER*/ | 0x0020 /*DT_SINGLELINE*/ | 0x0800 /*DT_NOPREFIX*/
            | 0x8000 /*DT_END_ELLIPSIS*/
            | match align {
                TA_CENTER => 0x0001,
                TA_RIGHT => 0x0002,
                _ => 0,
            };
        self.draw_text(r, s, font, color, flags);
    }

    /// Word-wrapped text from the top of `r`.
    pub fn text_wrapped(&mut self, r: R, s: &str, font: HFONT, color: u32) {
        self.draw_text(r, s, font, color, 0x0010 /*DT_WORDBREAK*/ | 0x0800);
    }

    /// Height of `s` wrapped at `width`.
    pub fn measure_wrapped(&mut self, width: i32, s: &str, font: HFONT) -> i32 {
        let w: Vec<u16> = s.encode_utf16().collect();
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: 0,
        };
        // SAFETY: valid DC, font and buffers.
        unsafe {
            let old = SelectObject(self.dc, font as HGDIOBJ);
            DrawTextW(
                self.dc,
                w.as_ptr(),
                w.len() as i32,
                &mut rc,
                0x0010 | 0x0400 /*DT_CALCRECT*/ | 0x0800,
            );
            SelectObject(self.dc, old);
        }
        rc.bottom - rc.top
    }

    fn draw_text(&mut self, r: R, s: &str, font: HFONT, color: u32, flags: u32) {
        if s.is_empty() {
            return;
        }
        let w: Vec<u16> = s.encode_utf16().collect();
        let mut rc = r.to_rect();
        // SAFETY: valid DC, font and buffers.
        unsafe {
            let old = SelectObject(self.dc, font as HGDIOBJ);
            SetTextColor(self.dc, colorref(color));
            DrawTextW(self.dc, w.as_ptr(), w.len() as i32, &mut rc, flags);
            SelectObject(self.dc, old);
        }
        self.dirty_gdi = true;
    }

    pub fn text_width(&mut self, s: &str, font: HFONT) -> i32 {
        let w: Vec<u16> = s.encode_utf16().collect();
        let mut sz = windows_sys::Win32::Foundation::SIZE { cx: 0, cy: 0 };
        // SAFETY: valid DC, font and buffers.
        unsafe {
            let old = SelectObject(self.dc, font as HGDIOBJ);
            GetTextExtentPoint32W(self.dc, w.as_ptr(), w.len() as i32, &mut sz);
            SelectObject(self.dc, old);
        }
        sz.cx
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        // SAFETY: restore the original bitmap, then free the DC (the DIB drops after).
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteDC(self.dc);
        }
    }
}

/// Signed distance to a rounded rect (negative inside).
fn sd_rrect(x: f32, y: f32, l: f32, t: f32, r: f32, b: f32, radius: f32) -> f32 {
    let radius = radius.min((r - l) / 2.0).min((b - t) / 2.0).max(0.0);
    let (cx, cy) = ((l + r) / 2.0, (t + b) / 2.0);
    let (hx, hy) = ((r - l) / 2.0 - radius, (b - t) / 2.0 - radius);
    let (qx, qy) = ((x - cx).abs() - hx, (y - cy).abs() - hy);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - radius
}

#[inline]
pub fn blend_px(d: &mut u32, c: u32, a: f32) {
    if a <= 0.0 {
        return;
    }
    let a = a.min(1.0);
    let ch = |s: u32| {
        let dv = ((*d >> s) & 0xFF) as f32;
        let cv = ((c >> s) & 0xFF) as f32;
        ((dv + (cv - dv) * a) + 0.5) as u32 & 0xFF
    };
    *d = (ch(16) << 16) | (ch(8) << 8) | ch(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_swaps_channels() {
        assert_eq!(colorref(0xFF5A3C), 0x3C5AFF);
    }

    #[test]
    fn mix_endpoints() {
        assert_eq!(mix(0x102030, 0xF0E0D0, 0.0), 0x102030);
        assert_eq!(mix(0x102030, 0xF0E0D0, 1.0), 0xF0E0D0);
        assert_eq!(mix(0x000000, 0xFFFFFF, 0.5), 0x808080);
    }

    #[test]
    fn scaling() {
        assert_eq!(sc(100, 96), 100);
        assert_eq!(sc(100, 144), 150);
        assert_eq!(sc(3, 120), 4);
    }

    #[test]
    fn contrast_choice() {
        assert_eq!(on_color(0xFFC93C), 0x1A1917);
        assert_eq!(on_color(0x1C1B1A), 0xFFFFFF);
    }

    #[test]
    fn rrect_sdf_signs() {
        assert!(sd_rrect(50.0, 50.0, 0.0, 0.0, 100.0, 100.0, 8.0) < 0.0);
        assert!(sd_rrect(150.0, 50.0, 0.0, 0.0, 100.0, 100.0, 8.0) > 0.0);
        // corner pixel is outside a rounded rect
        assert!(sd_rrect(0.5, 0.5, 0.0, 0.0, 100.0, 100.0, 12.0) > 0.0);
    }
}
