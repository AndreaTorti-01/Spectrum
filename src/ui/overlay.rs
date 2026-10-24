//! Click-through topmost per-monitor overlay for Identify: a colored border plus a big number on each screen.

use super::draw::{colorref, display_face, make_font_px, window_dpi, Canvas, R, TA_CENTER};
use crate::monitors::Rect;
use crate::util::wide;
use std::cell::RefCell;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW,
    SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, ShowWindow, GWLP_USERDATA, LWA_COLORKEY,
    SW_SHOWNOACTIVATE, WM_ERASEBKGND, WM_NCDESTROY, WM_NCHITTEST, WM_PAINT, WM_TIMER, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const CLASS: &str = "SpectrumOverlay";
const KEY: u32 = 0xFF00FF; // magenta color key (0xRRGGBB == COLORREF here)
const IDENTIFY_MS: u32 = 2500;
const NONANTIALIASED_QUALITY: u8 = 3;

struct Data {
    /// The big number; `None` draws the border only.
    num: Option<u32>,
    accent: u32,
}

thread_local! {
    static WINDOWS: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) };
}

/// Destroys every overlay window.
pub fn hide_all() {
    let wins: Vec<HWND> = WINDOWS.with(|w| std::mem::take(&mut *w.borrow_mut()));
    for h in wins {
        // SAFETY: handles created by `create` on this thread.
        unsafe {
            DestroyWindow(h);
        }
    }
}

/// Border and big number on every monitor: `(rect, num, accent)` each; auto-hides after 2.5 s.
pub fn identify_all(mons: &[(Rect, u32, u32)]) {
    hide_all();
    let mut first = None;
    for &(rect, num, accent) in mons {
        if let Some(h) = create(rect, Some(num), accent) {
            WINDOWS.with(|w| w.borrow_mut().push(h));
            first.get_or_insert(h);
        }
    }
    if let Some(h) = first {
        // SAFETY: valid window handle; the timer dies with the window.
        unsafe {
            SetTimer(h, 1, IDENTIFY_MS, None);
        }
    }
}

fn register_class() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    super::window::register_class(&ONCE, CLASS, 0, Some(wnd_proc));
}

fn create(rect: Rect, num: Option<u32>, accent: u32) -> Option<HWND> {
    register_class();
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    if w <= 0 || h <= 0 {
        return None;
    }
    let cls = wide(CLASS);
    let data = Box::into_raw(Box::new(Data { num, accent }));
    // SAFETY: standard window creation; ownership of `data` passes to the window (freed in WM_NCDESTROY).
    unsafe {
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            cls.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            rect.left,
            rect.top,
            w,
            h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            data as *const _,
        );
        if hwnd.is_null() {
            drop(Box::from_raw(data));
            return None;
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, data as isize);
        SetLayeredWindowAttributes(hwnd, colorref(KEY), 0, LWA_COLORKEY);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        Some(hwnd)
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_NCHITTEST => -1, // HTTRANSPARENT
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Data;
            if !p.is_null() {
                paint(hwnd, hdc, &*p);
            }
            EndPaint(hwnd, &ps);
            0
        }
        WM_TIMER => {
            hide_all();
            0
        }
        WM_NCDESTROY => {
            let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Data;
            if !p.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(p));
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// The key color everywhere, the accent border, then the number with a dark outline.
///
/// # Safety
/// `hwnd` is an overlay window and `hdc` its paint DC from `BeginPaint`.
unsafe fn paint(hwnd: HWND, hdc: windows_sys::Win32::Graphics::Gdi::HDC, d: &Data) {
    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rc);
    let (w, h) = (rc.right, rc.bottom);
    let dpi = window_dpi(hwnd);
    // transparent key fill, then the accent border
    let key = CreateSolidBrush(colorref(KEY));
    FillRect(hdc, &rc, key);
    DeleteObject(key as _);
    let border = ((8.0 * dpi as f32 / 96.0).round() as i32).max(4);
    let accent = CreateSolidBrush(colorref(d.accent));
    for r in [
        RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: border,
        },
        RECT {
            left: 0,
            top: h - border,
            right: w,
            bottom: h,
        },
        RECT {
            left: 0,
            top: border,
            right: border,
            bottom: h - border,
        },
        RECT {
            left: w - border,
            top: border,
            right: w,
            bottom: h - border,
        },
    ] {
        FillRect(hdc, &r, accent);
    }
    DeleteObject(accent as _);
    let Some(num) = d.num else { return };

    // the number, in an off-screen canvas (no flicker), non-antialiased so no pink fringes on the key color
    let em = (h as f32 * 0.34) as i32;
    let font = make_font_px(display_face(), -em, 700, NONANTIALIASED_QUALITY);
    let text = num.to_string();
    let Some(mut probe) = Canvas::new(8, 8) else {
        DeleteObject(font as _);
        return;
    };
    let tw = probe.text_width(&text, font);
    drop(probe);
    let pad = em / 4;
    let (cw, ch) = (tw + pad * 2, em * 3 / 2 + pad);
    if let Some(mut canvas) = Canvas::new(cw, ch) {
        canvas.clear(KEY);
        let r = R::new(0, 0, cw, ch);
        // A dark outline: the number drawn 16 times around a circle of `outline` px.
        let outline = ((2.0 * dpi as f32 / 96.0).round() as i32).max(2);
        for k in 0..16 {
            let a = k as f32 * std::f32::consts::TAU / 16.0;
            let (dx, dy) = (
                (a.cos() * outline as f32).round() as i32,
                (a.sin() * outline as f32).round() as i32,
            );
            canvas.text(r.offset(dx, dy), &text, font, 0x14110F, TA_CENTER);
        }
        canvas.text(r, &text, font, d.accent, TA_CENTER);
        canvas.present(hdc, (w - cw) / 2, (h - ch) / 2);
    }
    DeleteObject(font as _);
}
