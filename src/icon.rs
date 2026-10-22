//! Procedural app icon at runtime: the pixels come from [`crate::icon_render`]; this adds the HICON.

use crate::icon_render::render_bgra;
use std::ffi::c_void;
use windows_sys::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

pub use crate::icon_render::ico_bytes;

/// Creates a 32-bit HICON of `size` x `size` px. The caller owns it (`DestroyIcon`).
pub fn create_hicon(size: u32) -> HICON {
    let size = size.clamp(8, 512);
    let bgra = render_bgra(size);
    // SAFETY: DIB-section bits are valid for size*size*4 bytes; bitmaps are deleted after the icon copies them.
    unsafe {
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
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
        let color = CreateDIBSection(
            std::ptr::null_mut(),
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if color.is_null() || bits.is_null() {
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len());
        let row = size.div_ceil(16) * 2;
        let zeros = vec![0u8; (row * size) as usize];
        let mask = CreateBitmap(size as i32, size as i32, 1, 1, zeros.as_ptr() as *const c_void);
        let info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info);
        DeleteObject(color as _);
        DeleteObject(mask as _);
        icon
    }
}
