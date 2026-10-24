//! GDI fonts: the cached type ramp and face lookup (Segoe UI Variable where installed).

use super::sc;
use crate::util::wide;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::LPARAM;
use windows_sys::Win32::Graphics::Gdi::{
    CreateFontW, DeleteObject, EnumFontFamiliesExW, GetDC, ReleaseDC, CLEARTYPE_QUALITY, DEFAULT_CHARSET,
    HFONT, HGDIOBJ, LOGFONTW, TEXTMETRICW,
};

/// The type ramp from `design.rs`: caption, body, body strong, subtitle and display.
pub struct Fonts {
    pub caption: HFONT,
    pub body: HFONT,
    /// Semibold body size, for a title line above `body` text.
    pub strong: HFONT,
    /// Section headings.
    pub subtitle: HFONT,
    /// Monitor numbers.
    pub display: HFONT,
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

pub fn font_exists(face: &str) -> bool {
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
        use crate::ui::design::{FONT_BODY, FONT_CAPTION, FONT_DISPLAY, FONT_SUBTITLE};
        let (text, display) = *faces();
        let px = |size: i32, face: &str, weight: i32| {
            make_font_px(face, -sc(size, dpi), weight, CLEARTYPE_QUALITY)
        };
        Fonts {
            caption: px(FONT_CAPTION, text, 400),
            body: px(FONT_BODY, text, 400),
            strong: px(FONT_BODY, text, 600),
            subtitle: px(FONT_SUBTITLE, display, 600),
            display: px(FONT_DISPLAY, display, 600),
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        // SAFETY: handles created by CreateFontW and owned here.
        unsafe {
            for f in [self.caption, self.body, self.strong, self.subtitle, self.display] {
                if !f.is_null() {
                    DeleteObject(f as HGDIOBJ);
                }
            }
        }
    }
}
