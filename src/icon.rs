//! Procedural app icon: rendered in code at any size, as an HICON and as .ico file bytes.

use windows_sys::Win32::UI::WindowsAndMessaging::HICON;

/// Premultiplied BGRA, top-down, size x size.
pub fn render_bgra(_size: u32) -> Vec<u8> {
    todo!("icon::render_bgra")
}

pub fn create_hicon(_size: u32) -> HICON {
    todo!("icon::create_hicon")
}

/// 16, 24, 32, 48, 256 px 32-bit BMP DIB entries.
pub fn ico_bytes() -> Vec<u8> {
    todo!("icon::ico_bytes")
}
