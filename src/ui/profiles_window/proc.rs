//! The profiles window procedure and the two-click delete.

use super::super::draw::{
    apply_window_theme, invalidate_theme, sc, theme, trim_working_set, Fonts, WM_MOUSELEAVE,
};
use super::super::widgets::Press;
use super::super::window;
use super::{with, Profiles, CLASS, CLIENT_H, CLIENT_W, HWND_CELL, PROFILES};
use crate::app::UiAction;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DestroyWindow, KillTimer, SetTimer, CS_HREDRAW, CS_VREDRAW, WM_CAPTURECHANGED, WM_CLOSE,
    WM_CREATE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SETCURSOR, WM_SETTINGCHANGE, WM_TIMER,
};

const TIMER_CONFIRM: usize = 1;
/// How long the button stays on "Click to confirm".
const CONFIRM_MS: u32 = 3000;

pub(super) fn register_class() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    window::register_class(&ONCE, CLASS, CS_HREDRAW | CS_VREDRAW, Some(wnd_proc));
}

/// New DPI: fonts, back buffer and icons for it, and the window resized to fit.
pub(super) fn on_dpi_changed(hwnd: HWND, dpi: u32, suggested: Option<RECT>) {
    with(|p| {
        p.dpi = dpi;
        p.fonts = Fonts::new(dpi);
        p.canvas = None;
        p.clamp_scroll();
    });
    refresh_icons(hwnd, dpi);
    // PROFILES is not borrowed here: the resize sends messages.
    window::resize_client(hwnd, sc(CLIENT_W, dpi), sc(CLIENT_H, dpi), dpi, suggested);
}

/// Rebuilds the title bar icons for a new DPI and frees the old ones.
fn refresh_icons(hwnd: HWND, dpi: u32) {
    if with(|_| ()).is_none() {
        return;
    }
    let icons = window::set_title_icons(hwnd, dpi);
    if let Some(old) = with(|p| std::mem::replace(&mut p.icons, icons)) {
        window::destroy_icons(old);
    }
}

/// What releasing the mouse button does.
#[derive(Debug, PartialEq)]
enum Release {
    Nothing,
    /// First click on a Delete button: it now asks for confirmation.
    Confirm,
    /// Second click: delete this file.
    Delete(String),
}

impl Profiles {
    fn on_mouse_move(&mut self, hwnd: HWND, x: i32, y: i32) {
        if let Some(offset) = self.drag {
            if let Some(scroll) = self.scroll_list().drag_to(self.scroll, y - offset) {
                self.scroll = scroll;
            }
            self.invalidate();
            return;
        }
        if !self.tracking {
            window::track_mouse_leave(hwnd);
            self.tracking = true;
        }
        self.mouse = (x, y);
        self.hover = self.row_at(x, y);
        self.invalidate();
    }

    /// A press on the scrollbar (drag or page) or an enabled Delete button. True when the mouse should be
    /// captured.
    fn on_button_down(&mut self, x: i32, y: i32) -> bool {
        match self.scroll_list().press(self.scroll, x, y) {
            Some(Press::Thumb(offset)) => {
                self.drag = Some(offset);
                self.invalidate();
                true
            }
            Some(Press::Page(scroll)) => {
                self.scroll = scroll;
                self.invalidate();
                false
            }
            None => match self.button_at(x, y) {
                Some(i) => {
                    self.pressed = Some(i);
                    self.invalidate();
                    true
                }
                None => false,
            },
        }
    }

    /// Release (or lost capture) at (x, y): a click completes only on the button it started on.
    fn on_release(&mut self, x: i32, y: i32, capture_lost: bool) -> Release {
        let was_drag = self.drag.take().is_some();
        let pressed = self.pressed.take();
        self.invalidate();
        if was_drag || capture_lost {
            return Release::Nothing;
        }
        let Some(i) = pressed.filter(|&i| self.button_at(x, y) == Some(i)) else {
            return Release::Nothing;
        };
        if self.confirm == Some(i) {
            return Release::Delete(self.rows[i].file.clone());
        }
        self.confirm = Some(i);
        Release::Confirm
    }

    /// One row per wheel notch; the hover follows the row now under the pointer.
    fn on_wheel(&mut self, x: i32, y: i32, delta: i32) {
        let steps = (delta.abs() / 120).max(1) * delta.signum();
        self.scroll -= steps * self.row_h();
        self.clamp_scroll();
        self.hover = self.row_at(x, y);
        self.invalidate();
    }
}

/// Second click: deletes through the app (same path as `--debug-delete-profile`), then reloads the rows.
/// Runs with PROFILES unborrowed: the Win32 calls below can send messages back to this window.
fn delete_now(hwnd: HWND, file: String) {
    // SAFETY: valid window; killing the timer twice is harmless.
    unsafe { KillTimer(hwnd, TIMER_CONFIRM) };
    let mock = with(|p| p.mock).unwrap_or(false);
    let result = if mock {
        let name = with(|p| p.rows.iter().find(|r| r.file == file).map(|r| r.name.clone()))
            .flatten()
            .unwrap_or_else(|| file.clone());
        Ok(format!("Deleted {name}"))
    } else {
        crate::app::dispatch_result(UiAction::DeleteProfile { file: file.clone() })
    };
    let rows = (!mock).then(crate::app::list_profiles);
    with(|p| {
        p.confirm = None;
        p.status = Some(result.unwrap_or_else(|e| e));
        match rows {
            Some(r) => p.rows = r,
            None => p.rows.retain(|r| r.file != file),
        }
        p.clamp_scroll();
        p.invalidate();
    });
    if !mock && super::super::main_window::is_open() {
        super::super::main_window::refresh();
    }
}

fn on_create(hwnd: HWND) {
    HWND_CELL.with(|c| c.set(hwnd as isize));
    apply_window_theme(hwnd, theme().dark);
    let icons = window::set_initial_icons(hwnd);
    with(|p| {
        p.hwnd = hwnd;
        p.icons = icons;
    });
}

fn on_destroy(hwnd: HWND) {
    // SAFETY: valid window; killing a timer that is not set is harmless.
    unsafe { KillTimer(hwnd, TIMER_CONFIRM) };
    HWND_CELL.with(|c| c.set(0));
    let profiles = PROFILES.with(|m| m.try_borrow_mut().ok().and_then(|mut b| b.take()));
    if let Some(p) = &profiles {
        window::destroy_icons(p.icons);
    }
    drop(profiles); // fonts and canvas
    trim_working_set();
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            on_create(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            with(|p| p.paint(hdc));
            EndPaint(hwnd, &ps);
            0
        }
        WM_DPICHANGED => {
            // SAFETY: lParam of WM_DPICHANGED points at the suggested window RECT.
            let suggested = *(lp as *const RECT);
            on_dpi_changed(hwnd, (wp & 0xFFFF) as u32, Some(suggested));
            0
        }
        WM_SETTINGCHANGE => {
            if window::is_theme_change(wp, lp) {
                invalidate_theme();
                apply_window_theme(hwnd, theme().dark);
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = window::mouse_pos(lp);
            with(|p| p.on_mouse_move(hwnd, x, y));
            0
        }
        WM_MOUSELEAVE => {
            with(|p| {
                p.tracking = false;
                p.hover = None;
                p.mouse = (-1, -1);
                p.invalidate();
            });
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = window::mouse_pos(lp);
            SetFocus(hwnd);
            // Released on button up or capture loss.
            if with(|p| p.on_button_down(x, y)) == Some(true) {
                SetCapture(hwnd);
            }
            0
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED => {
            let (x, y) = window::mouse_pos(lp);
            // Decide inside the borrow, act after it: the delete calls into Win32.
            let release = with(|p| p.on_release(x, y, msg == WM_CAPTURECHANGED));
            if msg == WM_LBUTTONUP {
                ReleaseCapture();
            }
            match release {
                Some(Release::Confirm) => {
                    SetTimer(hwnd, TIMER_CONFIRM, CONFIRM_MS, None);
                }
                Some(Release::Delete(file)) => delete_now(hwnd, file),
                _ => {}
            }
            0
        }
        WM_TIMER => {
            if wp == TIMER_CONFIRM {
                KillTimer(hwnd, TIMER_CONFIRM);
                with(|p| {
                    p.confirm = None;
                    p.invalidate();
                });
            }
            0
        }
        WM_MOUSEWHEEL => {
            let (sx, sy) = window::mouse_pos(lp);
            let (x, y) = window::screen_to_client(hwnd, sx, sy);
            let delta = window::wheel_delta(wp);
            with(|p| p.on_wheel(x, y, delta));
            0
        }
        WM_SETCURSOR => {
            if window::is_client_hit(lp) {
                let (x, y) = window::cursor_pos(hwnd);
                let hand = with(|p| p.button_at(x, y).is_some()).unwrap_or(false);
                window::set_cursor(hand);
                return 1;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_KEYDOWN => {
            if wp as u16 == VK_ESCAPE {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            on_destroy(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_takes_two_clicks_on_the_same_button() {
        let mut p = Profiles::new(crate::showcase::profiles(), true, 96);
        let b = p.button_rect(2);
        let (x, y) = (b.l + 2, b.t + 2);
        assert!(p.on_button_down(x, y));
        assert_eq!(p.on_release(x, y, false), Release::Confirm);
        assert_eq!(p.confirm, Some(2));
        assert!(p.on_button_down(x, y));
        assert_eq!(
            p.on_release(x, y, false),
            Release::Delete("Display P3 calibrated.icm".into())
        );
    }

    #[test]
    fn a_click_that_leaves_the_button_or_loses_capture_does_nothing() {
        let mut p = Profiles::new(crate::showcase::profiles(), true, 96);
        let b = p.button_rect(0);
        p.on_button_down(b.l + 2, b.t + 2);
        assert_eq!(p.on_release(b.l - 40, b.t + 2, false), Release::Nothing);
        p.on_button_down(b.l + 2, b.t + 2);
        assert_eq!(p.on_release(b.l + 2, b.t + 2, true), Release::Nothing);
        assert_eq!(p.confirm, None);
        // The locked inbox profile has no live button.
        let locked = p.button_rect(3);
        assert!(!p.on_button_down(locked.l + 2, locked.t + 2));
    }
}
