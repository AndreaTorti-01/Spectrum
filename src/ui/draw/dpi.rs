//! DPI scaling (with the `--debug-scale` screenshot override) and DWM / process helpers.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};

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

/// `dpi` times `num / den`, rounded; exact when the factor is 1.
fn rescale(dpi: u32, num: u32, den: u32) -> u32 {
    if num == den {
        dpi
    } else {
        (dpi as f64 * num as f64 / den as f64).round() as u32
    }
}

/// A real DPI with the UI scale override applied.
fn scaled(d: u32) -> u32 {
    rescale(d, UI_SCALE_MILLI.load(Ordering::Relaxed), 1000)
}

/// The DPI the system actually uses for window frames and title bars, given a (possibly scaled) UI DPI.
pub fn frame_dpi(dpi: u32) -> u32 {
    rescale(dpi, 1000, UI_SCALE_MILLI.load(Ordering::Relaxed))
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

/// Dark title bar for the theme and rounded corners (Windows 11); ignored where unsupported.
pub fn apply_window_theme(hwnd: HWND, dark: bool) {
    let dark = dark as i32;
    // SAFETY: attribute pointers reference 4-byte values that live through each call.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &dark as *const i32 as *const c_void,
            4,
        );
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &DWMWCP_ROUND as *const i32 as *const c_void,
            4,
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling() {
        assert_eq!(sc(100, 96), 100);
        assert_eq!(sc(100, 144), 150);
        assert_eq!(sc(3, 120), 4);
        assert_eq!(scf(1.5, 192), 3.0);
    }

    #[test]
    fn rescale_rounds_and_is_exact_at_one() {
        assert_eq!(rescale(144, 1000, 1000), 144);
        assert_eq!(rescale(96, 1500, 1000), 144);
        assert_eq!(rescale(144, 1000, 1500), 96);
        assert_eq!(rescale(97, 1000, 3000), 32);
    }
}
