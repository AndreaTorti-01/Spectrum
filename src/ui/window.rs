//! Win32 plumbing the custom-drawn windows share: class registration, a centered captioned window sized
//! by its client area, DPI resizing, title bar icons, the back buffer, and message parameter decoding.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use super::draw::{frame_dpi, window_dpi, Canvas};
use crate::util::wide;
use std::sync::Once;
use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{InvalidateRect, ScreenToClient};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyIcon, GetClientRect, GetCursorPos, GetWindowRect, IsIconic, LoadCursorW,
    RegisterClassExW, SendMessageW, SetCursor, SetForegroundWindow, SetWindowPos, ShowWindow,
    SystemParametersInfoW, CW_USEDEFAULT, HICON, ICON_BIG, ICON_SMALL, IDC_ARROW, IDC_HAND, SM_CXICON,
    SM_CXSMICON, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOZORDER, SW_RESTORE, SW_SHOW, WINDOW_STYLE,
    WM_SETICON, WNDCLASSEXW, WNDCLASS_STYLES, WNDPROC, WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
};

/// Style of the app's windows: caption, close box and minimize box, no resizing.
pub const STYLE: WINDOW_STYLE = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;

/// Registers a window class with the arrow cursor, once per `once`.
pub fn register_class(once: &'static Once, class: &str, style: WNDCLASS_STYLES, proc: WNDPROC) {
    once.call_once(|| {
        let cls = wide(class);
        // SAFETY: class data outlives the call; `proc` has the window procedure signature.
        unsafe {
            let mut wc: WNDCLASSEXW = std::mem::zeroed();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.style = style;
            wc.lpfnWndProc = proc;
            wc.hInstance = GetModuleHandleW(std::ptr::null());
            wc.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
            wc.lpszClassName = cls.as_ptr();
            RegisterClassExW(&wc);
        }
    });
}

/// Window size for a client area of `client_w` x `client_h` physical pixels at `dpi`.
fn outer_size(client_w: i32, client_h: i32, dpi: u32) -> (i32, i32) {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: client_w,
        bottom: client_h,
    };
    // SAFETY: valid RECT pointer.
    unsafe { AdjustWindowRectExForDpi(&mut r, STYLE, 0, 0, frame_dpi(dpi)) };
    (r.right - r.left, r.bottom - r.top)
}

/// Creates (hidden) a [`STYLE`] window with the given client size, centered on the primary work area.
/// Null on failure. The window procedure runs during the call, so the caller must hold no borrow of the
/// window's state.
pub fn create_centered(class: &str, title: &str, client_w: i32, client_h: i32, dpi: u32) -> HWND {
    let (w, h) = outer_size(client_w, client_h, dpi);
    let mut work_area = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    let (cls, title) = (wide(class), wide(title));
    // SAFETY: valid pointers; the strings outlive the calls.
    unsafe {
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work_area as *mut RECT as *mut _, 0);
        let x = work_area.left + ((work_area.right - work_area.left) - w) / 2;
        let y = work_area.top + ((work_area.bottom - work_area.top) - h) / 2;
        CreateWindowExW(
            0,
            cls.as_ptr(),
            title.as_ptr(),
            STYLE,
            x.max(work_area.left),
            y.max(work_area.top),
            if w <= 0 { CW_USEDEFAULT } else { w },
            h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        )
    }
}

/// Shows a new window and brings it to the front.
pub fn show_new(hwnd: HWND) {
    // SAFETY: valid window handle.
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
}

/// Brings an existing window back: restores it when minimized, then to the front.
pub fn restore_and_focus(hwnd: HWND) {
    // SAFETY: valid window handle.
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
}

/// Moves and sizes the window to `suggested` (from `WM_DPICHANGED`), or keeps its position and fits it to
/// a client area of `client_w` x `client_h` at `dpi`. Repaints it. Nested messages run during the call.
pub fn resize_client(hwnd: HWND, client_w: i32, client_h: i32, dpi: u32, suggested: Option<RECT>) {
    let r = suggested.unwrap_or_else(|| {
        let (w, h) = outer_size(client_w, client_h, dpi);
        let mut cur = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: valid window handle and out pointer.
        unsafe { GetWindowRect(hwnd, &mut cur) };
        RECT {
            left: cur.left,
            top: cur.top,
            right: cur.left + w,
            bottom: cur.top + h,
        }
    });
    // SAFETY: valid window handle.
    unsafe {
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

/// Repaints the whole window (null-safe).
pub fn invalidate(hwnd: HWND) {
    if !hwnd.is_null() {
        // SAFETY: valid window handle.
        unsafe {
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
    }
}

fn icon_at(dpi: u32, metric: i32) -> HICON {
    // SAFETY: plain query.
    let px = unsafe { GetSystemMetricsForDpi(metric, dpi) };
    crate::icon::create_hicon(px.max(16) as u32)
}

/// Creates the big and small title bar icons for `dpi` and hands them to the window. The caller owns them
/// and frees them with [`destroy_icons`] once the window no longer uses them.
pub fn set_title_icons(hwnd: HWND, dpi: u32) -> [HICON; 2] {
    let icons = [icon_at(dpi, SM_CXICON), icon_at(dpi, SM_CXSMICON)];
    // SAFETY: valid window handle; the window only keeps the handles.
    unsafe {
        SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, icons[0] as LPARAM);
        SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, icons[1] as LPARAM);
    }
    icons
}

/// Title bar icons for a window that is being created, at its own DPI.
pub fn set_initial_icons(hwnd: HWND) -> [HICON; 2] {
    set_title_icons(hwnd, window_dpi(hwnd))
}

pub fn destroy_icons(icons: [HICON; 2]) {
    for icon in icons {
        if !icon.is_null() {
            // SAFETY: icon created by `set_title_icons` and no longer used by a window.
            unsafe { DestroyIcon(icon) };
        }
    }
}

/// The back buffer for the window's client area: `slot`'s canvas when it still has the right size, else a
/// new one. `None` for an empty client area (`slot` untouched) or when no canvas could be made.
pub fn client_canvas(hwnd: HWND, slot: &mut Option<Canvas>) -> Option<Canvas> {
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: valid window handle and out pointer.
    unsafe {
        GetClientRect(hwnd, &mut rc);
    }
    let (w, h) = (rc.right, rc.bottom);
    if w <= 0 || h <= 0 {
        return None;
    }
    match slot.take() {
        Some(c) if c.width() == w && c.height() == h => Some(c),
        _ => Canvas::new(w, h),
    }
}

/// Asks for `WM_MOUSELEAVE` when the pointer leaves the window.
pub fn track_mouse_leave(hwnd: HWND) {
    let mut t = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    // SAFETY: valid struct pointer.
    unsafe { TrackMouseEvent(&mut t) };
}

/// Client position of a mouse message's `lParam` (signed: it can be outside the window while captured).
pub fn mouse_pos(lp: LPARAM) -> (i32, i32) {
    ((lp & 0xFFFF) as i16 as i32, ((lp >> 16) & 0xFFFF) as i16 as i32)
}

/// Wheel rotation of a `WM_MOUSEWHEEL` `wParam`, in 1/120 notches.
pub fn wheel_delta(wp: WPARAM) -> i32 {
    ((wp >> 16) & 0xFFFF) as i16 as i32
}

/// Screen point to client coordinates.
pub fn screen_to_client(hwnd: HWND, x: i32, y: i32) -> (i32, i32) {
    let mut pt = POINT { x, y };
    // SAFETY: valid window handle and point.
    unsafe { ScreenToClient(hwnd, &mut pt) };
    (pt.x, pt.y)
}

/// The mouse pointer in client coordinates.
pub fn cursor_pos(hwnd: HWND) -> (i32, i32) {
    let mut pt = POINT { x: 0, y: 0 };
    // SAFETY: valid point.
    unsafe { GetCursorPos(&mut pt) };
    screen_to_client(hwnd, pt.x, pt.y)
}

/// `WM_SETCURSOR` over the client area (`HTCLIENT` in the low word of `lParam`).
pub fn is_client_hit(lp: LPARAM) -> bool {
    (lp & 0xFFFF) as u32 == 1
}

/// Hand over clickable controls, arrow elsewhere.
pub fn set_cursor(hand: bool) {
    // SAFETY: system cursors need no instance.
    unsafe {
        SetCursor(LoadCursorW(
            std::ptr::null_mut(),
            if hand { IDC_HAND } else { IDC_ARROW },
        ));
    }
}

/// `WM_SETTINGCHANGE` for "ImmersiveColorSet": the app theme (light/dark) may have changed.
///
/// # Safety
/// `wp` and `lp` are the parameters of a `WM_SETTINGCHANGE` message; a string `lParam` only comes with
/// `wParam` 0, anything else may not be a pointer.
pub unsafe fn is_theme_change(wp: WPARAM, lp: LPARAM) -> bool {
    if wp != 0 || lp == 0 {
        return false;
    }
    let p = lp as *const u16;
    let mut n = 0;
    while n < 64 && *p.add(n) != 0 {
        n += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, n)) == "ImmersiveColorSet"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_positions_are_signed() {
        assert_eq!(mouse_pos(0x0014_000A), (10, 20));
        assert_eq!(mouse_pos(0xFFFE_FFFF), (-1, -2));
        assert_eq!(wheel_delta(0x0078_0000), 120);
        assert_eq!(wheel_delta(0xFF88_0000), -120);
    }

    #[test]
    fn client_hit_test() {
        assert!(is_client_hit(0x0201_0001));
        assert!(!is_client_hit(0x0201_0002));
    }

    #[test]
    fn theme_change_needs_the_color_set_string() {
        let name: Vec<u16> = "ImmersiveColorSet\0".encode_utf16().collect();
        let other: Vec<u16> = "Policy\0".encode_utf16().collect();
        // SAFETY: NUL-terminated strings that outlive the calls.
        unsafe {
            assert!(is_theme_change(0, name.as_ptr() as LPARAM));
            assert!(!is_theme_change(0, other.as_ptr() as LPARAM));
            assert!(!is_theme_change(1, name.as_ptr() as LPARAM));
            assert!(!is_theme_change(0, 0));
        }
    }
}
