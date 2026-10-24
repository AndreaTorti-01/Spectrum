//! The dropdown's popup list: a themed tool window under (or above) the field. The owner learns the
//! choice from a posted message.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use super::super::draw::{apply_window_theme, sc, scf, window_dpi, Canvas, Fonts, Theme, R, TA_LEFT};
use super::super::window::{mouse_pos, register_class};
use crate::util::wide;
use std::cell::RefCell;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromWindow, HDC, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SetFocus, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_RETURN, VK_SPACE, VK_TAB, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, PostMessageW, SetWindowPos, ShowWindow,
    CS_DROPSHADOW, SWP_NOACTIVATE, SW_SHOW, WM_ACTIVATE, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT, WS_EX_TOOLWINDOW, WS_POPUP,
};

struct Popup {
    owner: HWND,
    msg: u32,
    items: Vec<String>,
    /// The current choice (marked with an accent bar).
    selected: Option<usize>,
    /// The row under the mouse or the keyboard cursor.
    hover: Option<usize>,
    th: Theme,
    dpi: u32,
    fonts: Fonts,
    canvas: Option<Canvas>,
    row_h: i32,
    pad: i32,
    /// The owner was told; later close paths do nothing.
    done: bool,
}

thread_local! {
    static POPUP: RefCell<Option<Popup>> = const { RefCell::new(None) };
}

const POPUP_CLASS: &str = "SpectrumDropdownList";

/// Value of `wParam` posted to the owner when the popup was dismissed without a choice.
pub const DROPDOWN_DISMISSED: usize = usize::MAX;

/// Runs `f` on the popup state unless it is gone or already borrowed.
fn with_popup<T>(f: impl FnOnce(&mut Popup) -> T) -> Option<T> {
    POPUP.with(|p| p.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

pub fn dropdown_is_open() -> bool {
    POPUP.with(|p| p.try_borrow().map(|b| b.is_some()).unwrap_or(true))
}

/// Opens the list popup under (or above) `anchor` (screen coordinates). On close the owner receives `msg`
/// with `wParam` = chosen index or `DROPDOWN_DISMISSED`.
pub fn open_dropdown(
    owner: HWND,
    anchor: R,
    items: Vec<String>,
    selected: Option<usize>,
    th: Theme,
    msg: u32,
) {
    if items.is_empty() || dropdown_is_open() {
        return;
    }
    static ONCE: std::sync::Once = std::sync::Once::new();
    register_class(&ONCE, POPUP_CLASS, CS_DROPSHADOW, Some(popup_proc));
    let dpi = window_dpi(owner);
    let row_h = sc(34, dpi);
    let margin = sc(4, dpi);
    let h = row_h * items.len() as i32 + margin * 2;
    let w = anchor.w();
    // SAFETY: standard window creation; state is installed before the window can receive messages
    // that read it (WM_PAINT arrives only after ShowWindow below).
    unsafe {
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST), &mut mi);
        // Below the field, or above it when it would leave the work area.
        let mut y = anchor.b + sc(2, dpi);
        if y + h > mi.rcWork.bottom && anchor.t - h - sc(2, dpi) >= mi.rcWork.top {
            y = anchor.t - h - sc(2, dpi);
        }
        POPUP.with(|p| {
            *p.borrow_mut() = Some(Popup {
                owner,
                msg,
                items,
                selected,
                hover: selected,
                th,
                dpi,
                fonts: Fonts::new(dpi),
                canvas: None,
                row_h,
                pad: margin,
                done: false,
            })
        });
        let cls = wide(POPUP_CLASS);
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            cls.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            anchor.l,
            y,
            w,
            h,
            owner,
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        if hwnd.is_null() {
            POPUP.with(|p| *p.borrow_mut() = None);
            return;
        }
        apply_window_theme(hwnd, th.dark);
        SetWindowPos(hwnd, std::ptr::null_mut(), anchor.l, y, w, h, SWP_NOACTIVATE);
        ShowWindow(hwnd, SW_SHOW);
        SetFocus(hwnd);
    }
}

/// Closes the popup once and tells the owner what was picked.
fn finish(hwnd: HWND, pick: Option<usize>) {
    let owner = with_popup(|p| {
        if p.done {
            return None;
        }
        p.done = true;
        Some((p.owner, p.msg))
    })
    .flatten();
    if let Some((owner, msg)) = owner {
        // SAFETY: valid window handles; PostMessage never blocks.
        unsafe {
            DestroyWindow(hwnd);
            PostMessageW(owner, msg, pick.unwrap_or(DROPDOWN_DISMISSED), 0);
        }
    }
}

/// Row index at client `y` (rows start below the top padding).
fn item_at(row_h: i32, pad: i32, count: usize, y: i32) -> Option<usize> {
    let i = (y - pad).div_euclid(row_h);
    (y >= pad && i >= 0 && (i as usize) < count).then_some(i as usize)
}

impl Popup {
    fn item_at(&self, y: i32) -> Option<usize> {
        item_at(self.row_h, self.pad, self.items.len(), y)
    }

    fn paint(&mut self, hwnd: HWND, hdc: HDC) {
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: valid window handle and out pointer.
        unsafe { GetClientRect(hwnd, &mut rc) };
        let (w, h) = (rc.right, rc.bottom);
        if self.canvas.as_ref().map(|c| (c.width(), c.height())) != Some((w, h)) {
            self.canvas = Canvas::new(w, h);
        }
        let (th, dpi, row_h, pad) = (self.th, self.dpi, self.row_h, self.pad);
        let Some(canvas) = self.canvas.as_mut() else {
            return;
        };
        canvas.clear(th.surface);
        for (i, item) in self.items.iter().enumerate() {
            let r = R::xywh(pad, pad + i as i32 * row_h, w - pad * 2, row_h);
            if self.hover == Some(i) {
                canvas.fill_rrect(r, scf(4.0, dpi), th.hover_fill(), 1.0);
                canvas.fill_rrect(r, scf(4.0, dpi), th.text, 0.04);
            }
            if self.selected == Some(i) {
                let bar = R::xywh(
                    r.l + sc(2, dpi),
                    r.t + sc(9, dpi),
                    sc(3, dpi),
                    r.h() - sc(18, dpi),
                );
                canvas.fill_rrect(bar, scf(1.5, dpi), th.accent, 1.0);
            }
            let text = R::new(r.l + sc(14, dpi), r.t, r.r - sc(8, dpi), r.b);
            canvas.text(text, item, self.fonts.body, th.text, TA_LEFT);
        }
        canvas.stroke_rrect(R::new(0, 0, w, h), scf(8.0, dpi), 1.0, th.line, 1.0);
        canvas.present(hdc, 0, 0);
    }

    /// A key press: `Err(pick)` closes the popup with that pick, `Ok(true)` moved the highlight.
    fn key(&mut self, vk: u16) -> Result<bool, Option<usize>> {
        let n = self.items.len();
        let hover = match vk {
            VK_ESCAPE | VK_TAB => return Err(None),
            VK_RETURN | VK_SPACE => return Err(self.hover),
            VK_DOWN => self.hover.map(|h| (h + 1) % n).unwrap_or(0),
            VK_UP => self.hover.map(|h| (h + n - 1) % n).unwrap_or(n - 1),
            VK_HOME => 0,
            VK_END => n - 1,
            _ => return Ok(false),
        };
        self.hover = Some(hover);
        Ok(true)
    }
}

unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            with_popup(|p| p.paint(hwnd, hdc));
            EndPaint(hwnd, &ps);
            0
        }
        WM_MOUSEMOVE => {
            let (_, y) = mouse_pos(lp);
            let changed = with_popup(|p| {
                let hover = p.item_at(y);
                let changed = hover != p.hover;
                p.hover = hover;
                changed
            });
            if changed == Some(true) {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_LBUTTONUP => {
            let (_, y) = mouse_pos(lp);
            let pick = POPUP.with(|p| {
                p.try_borrow()
                    .ok()
                    .and_then(|b| b.as_ref().and_then(|s| s.item_at(y)))
            });
            if pick.is_some() {
                finish(hwnd, pick);
            }
            0
        }
        WM_KEYDOWN => {
            match with_popup(|p| p.key(wp as u16)) {
                Some(Err(pick)) => finish(hwnd, pick),
                Some(Ok(true)) => {
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                _ => {}
            }
            0
        }
        WM_ACTIVATE => {
            if (wp & 0xFFFF) == 0 {
                finish(hwnd, None);
            }
            0
        }
        WM_NCDESTROY => {
            POPUP.with(|p| {
                if let Ok(mut b) = p.try_borrow_mut() {
                    *b = None;
                }
            });
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_start_below_the_padding() {
        assert_eq!(item_at(34, 4, 3, 3), None);
        assert_eq!(item_at(34, 4, 3, 4), Some(0));
        assert_eq!(item_at(34, 4, 3, 37), Some(0));
        assert_eq!(item_at(34, 4, 3, 38), Some(1));
        assert_eq!(item_at(34, 4, 3, 4 + 3 * 34), None);
        assert_eq!(item_at(34, 4, 3, -50), None);
    }

    fn popup(items: usize, hover: Option<usize>) -> Popup {
        Popup {
            owner: std::ptr::null_mut(),
            msg: 0,
            items: (0..items).map(|i| i.to_string()).collect(),
            selected: hover,
            hover,
            th: crate::ui::draw::theme(),
            dpi: 96,
            fonts: Fonts::new(96),
            canvas: None,
            row_h: 34,
            pad: 4,
            done: false,
        }
    }

    #[test]
    fn keys_move_the_highlight_and_wrap() {
        let mut p = popup(3, None);
        assert_eq!(p.key(VK_UP), Ok(true));
        assert_eq!(p.hover, Some(2), "up from nothing starts at the end");
        assert_eq!(p.key(VK_DOWN), Ok(true));
        assert_eq!(p.hover, Some(0), "wraps");
        assert_eq!(p.key(VK_END), Ok(true));
        assert_eq!(p.hover, Some(2));
        assert_eq!(p.key(VK_HOME), Ok(true));
        assert_eq!(p.key(VK_RETURN), Err(Some(0)));
        assert_eq!(p.key(VK_ESCAPE), Err(None));
        assert_eq!(p.key(VK_TAB), Err(None));
        assert_eq!(p.key(0x41), Ok(false));
    }
}
