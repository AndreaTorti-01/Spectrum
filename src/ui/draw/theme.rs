//! Theme tokens (light/dark, following the Windows setting unless `--theme` forces one) and color math.

use crate::util::wide;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI8, AtomicU8, Ordering};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

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

/// The Windows app theme, read once and cached until [`invalidate_theme`].
fn system_dark() -> bool {
    let mut cached = SYSTEM_DARK.load(Ordering::Relaxed);
    if cached < 0 {
        cached = read_system_dark() as i8;
        SYSTEM_DARK.store(cached, Ordering::Relaxed);
    }
    cached == 1
}

/// Follows AppsUseLightTheme unless overridden by `set_forced_theme`.
pub fn theme() -> Theme {
    palette(forced_theme().unwrap_or_else(system_dark))
}

/// The light or dark token set.
fn palette(dark: bool) -> Theme {
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

/// The theme forced with `--theme`, if any (true = dark).
pub fn forced_theme() -> Option<bool> {
    match FORCED.load(Ordering::Relaxed) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
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
        // Out-of-range factors clamp.
        assert_eq!(mix(0x102030, 0xF0E0D0, 2.0), 0xF0E0D0);
    }

    #[test]
    fn contrast_choice() {
        assert_eq!(on_color(0xFFC93C), 0x1A1917);
        assert_eq!(on_color(0x1C1B1A), 0xFFFFFF);
        assert!(luminance(0x000000).abs() < 1e-6);
        assert!((luminance(0xFFFFFF) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn palettes_keep_text_readable() {
        for dark in [false, true] {
            let t = palette(dark);
            assert_eq!(t.dark, dark);
            assert_eq!(t.accent, DEFAULT_ACCENT);
            assert_eq!(t.with_accent(0x2EC4B6).accent, 0x2EC4B6);
            let (bg, text) = (luminance(t.bg), luminance(t.text));
            assert!((bg - text).abs() > 0.6, "dark={dark}");
            // Hover and pressed fills sit between the surface and the text.
            assert_ne!(t.hover_fill(), t.surface);
            assert_ne!(t.pressed_fill(), t.hover_fill());
        }
        let light = palette(false).with_accent(0xFF5A3C);
        assert_eq!(light.accent_text(), mix(0xFF5A3C, 0x000000, 0.38));
        assert_eq!(palette(true).accent_text(), DEFAULT_ACCENT);
    }
}
