//! Click-through topmost per-monitor overlay (colored border + big number) used to identify screens.

use super::draw::{colorref, display_face, make_font_px, window_dpi, Canvas};
use crate::monitors::Rect;
use crate::util::wide;
use std::cell::RefCell;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW, LoadCursorW,
    RegisterClassExW, SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, ShowWindow, GWLP_USERDATA,
    IDC_ARROW, LWA_COLORKEY, SW_SHOWNOACTIVATE, WM_ERASEBKGND, WM_NCDESTROY, WM_NCHITTEST, WM_PAINT,
    WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

const CLASS: &str = "SpectrumOverlay";
const KEY: u32 = 0xFF00FF; // magenta color key (0xRRGGBB == COLORREF here)
const IDENTIFY_MS: u32 = 2500;
const NONANTIALIASED_QUALITY: u8 = 3;

struct Data {
    num: u32,
    accent: u32,
}

thread_local! {
    static WINDOWS: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) };
    static SHOWN: RefCell<Option<(Rect, u32, u32)>> = const { RefCell::new(None) };
}

/// Hover highlight: shows exactly one overlay (replaces any other).
pub fn show(rect: Rect, num: u32, accent: u32) {
    let same =
        SHOWN.with(|s| *s.borrow() == Some((rect, num, accent))) && WINDOWS.with(|w| w.borrow().len() == 1);
    if same {
        return;
    }
    hide_all();
    if let Some(h) = create(rect, num, accent) {
        WINDOWS.with(|w| w.borrow_mut().push(h));
        SHOWN.with(|s| *s.borrow_mut() = Some((rect, num, accent)));
    }
}

/// Destroys every overlay window.
pub fn hide_all() {
    let wins: Vec<HWND> = WINDOWS.with(|w| std::mem::take(&mut *w.borrow_mut()));
    SHOWN.with(|s| *s.borrow_mut() = None);
    for h in wins {
        // SAFETY: handles created by `create` on this thread.
        unsafe {
            DestroyWindow(h);
        }
    }
}

/// `(rect, num, accent)` per monitor; auto-hides after 2.5 s.
pub fn identify_all(mons: &[(Rect, u32, u32)]) {
    hide_all();
    let mut first = None;
    for &(rect, num, accent) in mons {
        if let Some(h) = create(rect, num, accent) {
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
    ONCE.call_once(|| {
        let cls = wide(CLASS);
        // SAFETY: class data outlives the call.
        unsafe {
            let mut wc: WNDCLASSEXW = std::mem::zeroed();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(wnd_proc);
            wc.hInstance = GetModuleHandleW(std::ptr::null());
            wc.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
            wc.lpszClassName = cls.as_ptr();
            RegisterClassExW(&wc);
        }
    });
}

fn create(rect: Rect, num: u32, accent: u32) -> Option<HWND> {
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

unsafe fn paint(hwnd: HWND, hdc: windows_sys::Win32::Graphics::Gdi::HDC, d: &Data) {
    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rc);
    let (w, h) = (rc.right, rc.bottom);
    let dpi = window_dpi(hwnd);
    // transparent key fill, then the accent border
    let key = CreateSolidBrush(colorref(KEY));
    FillRect(hdc, &rc, key);
    DeleteObject(key as _);
    let bw = ((8.0 * dpi as f32 / 96.0).round() as i32).max(4);
    let acc = CreateSolidBrush(colorref(d.accent));
    for r in [
        RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: bw,
        },
        RECT {
            left: 0,
            top: h - bw,
            right: w,
            bottom: h,
        },
        RECT {
            left: 0,
            top: bw,
            right: bw,
            bottom: h - bw,
        },
        RECT {
            left: w - bw,
            top: bw,
            right: w,
            bottom: h - bw,
        },
    ] {
        FillRect(hdc, &r, acc);
    }
    DeleteObject(acc as _);

    // the number, in an off-screen canvas (no flicker), non-antialiased so no pink fringes on the key color
    let em = (h as f32 * 0.34) as i32;
    let font = make_font_px(display_face(), -em, 700, NONANTIALIASED_QUALITY);
    let text = d.num.to_string();
    let Some(mut probe) = Canvas::new(8, 8) else {
        DeleteObject(font as _);
        return;
    };
    let tw = probe.text_width(&text, font);
    drop(probe);
    let pad = em / 4;
    let (cw, ch) = (tw + pad * 2, em * 3 / 2 + pad);
    if let Some(mut cv) = Canvas::new(cw, ch) {
        cv.clear(KEY);
        let r = super::draw::R::new(0, 0, cw, ch);
        let o = ((2.0 * dpi as f32 / 96.0).round() as i32).max(2);
        for k in 0..16 {
            let a = k as f32 * std::f32::consts::TAU / 16.0;
            let (dx, dy) = (
                (a.cos() * o as f32).round() as i32,
                (a.sin() * o as f32).round() as i32,
            );
            cv.text(r.offset(dx, dy), &text, font, 0x14110F, super::draw::TA_CENTER);
        }
        cv.text(r, &text, font, d.accent, super::draw::TA_CENTER);
        cv.present(hdc, (w - cw) / 2, (h - ch) / 2);
    }
    DeleteObject(font as _);
}
