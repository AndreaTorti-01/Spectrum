//! The gamut window procedure: hover and toggling rows, scrolling (wheel, scrollbar, keyboard cursor),
//! DPI and theme changes.

use super::super::draw::{
    apply_window_theme, invalidate_theme, sc, theme, trim_working_set, Fonts, WM_MOUSELEAVE,
};
use super::super::widgets::Press;
use super::super::window;
use super::{with, Gamut, CLASS, CLIENT_H, CLIENT_W, GAMUT, HWND_CELL};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, SetFocus, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_SPACE, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DestroyWindow, CS_HREDRAW, CS_VREDRAW, WM_CAPTURECHANGED, WM_CLOSE, WM_CREATE,
    WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_PAINT, WM_SETTINGCHANGE,
};

pub(super) fn register_class() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    window::register_class(&ONCE, CLASS, CS_HREDRAW | CS_VREDRAW, Some(wnd_proc));
}

/// New DPI: fonts, back buffers and icons for it, and the window resized to fit.
pub(super) fn on_dpi_changed(hwnd: HWND, dpi: u32, suggested: Option<RECT>) {
    with(|gamut| {
        gamut.dpi = dpi;
        gamut.fonts = Fonts::new(dpi);
        gamut.canvas = None;
        gamut.horseshoe = None;
        gamut.clamp_scroll();
    });
    refresh_icons(hwnd, dpi);
    // GAMUT is not borrowed here: the resize sends messages.
    window::resize_client(hwnd, sc(CLIENT_W, dpi), sc(CLIENT_H, dpi), dpi, suggested);
}

/// Rebuilds the title bar icons for a new DPI and frees the old ones.
fn refresh_icons(hwnd: HWND, dpi: u32) {
    if with(|_| ()).is_none() {
        return;
    }
    let icons = window::set_title_icons(hwnd, dpi);
    if let Some(old) = with(|gamut| std::mem::replace(&mut gamut.icons, icons)) {
        window::destroy_icons(old);
    }
}

impl Gamut {
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
        let hover = self.row_at(x, y);
        if hover != self.hover {
            self.hover = hover;
            self.invalidate();
        }
    }

    /// A press on the scrollbar (drag or page) or a row (toggle). True when the mouse should be captured.
    fn on_button_down(&mut self, x: i32, y: i32) -> bool {
        self.keyboard = false;
        let mut capture = false;
        match self.scroll_list().press(self.scroll, x, y) {
            Some(Press::Thumb(offset)) => {
                self.drag = Some(offset);
                capture = true;
            }
            Some(Press::Page(scroll)) => self.scroll = scroll,
            None => match self.row_at(x, y) {
                Some(i) => self.toggle(i),
                None => return false,
            },
        }
        self.invalidate();
        capture
    }

    /// One row per wheel notch.
    fn on_wheel(&mut self, delta: i32) {
        let steps = (delta.abs() / 120).max(1) * delta.signum();
        self.scroll -= steps * self.row_h();
        self.clamp_scroll();
        self.invalidate();
    }

    fn on_key(&mut self, vk: u16) {
        match vk {
            VK_DOWN => self.move_selection(1),
            VK_UP => self.move_selection(-1),
            VK_SPACE | VK_RETURN => {
                if let Some(i) = self.selected {
                    self.toggle(i);
                }
            }
            _ => {}
        }
        self.invalidate();
    }
}

fn on_create(hwnd: HWND) {
    HWND_CELL.with(|c| c.set(hwnd as isize));
    apply_window_theme(hwnd, theme().dark);
    let icons = window::set_initial_icons(hwnd);
    with(|gamut| {
        gamut.hwnd = hwnd;
        gamut.icons = icons;
    });
}

fn on_destroy() {
    HWND_CELL.with(|c| c.set(0));
    let gamut = GAMUT.with(|m| m.try_borrow_mut().ok().and_then(|mut b| b.take()));
    if let Some(g) = &gamut {
        window::destroy_icons(g.icons);
    }
    drop(gamut); // fonts, canvas, horseshoe DIB
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
            with(|gamut| gamut.paint(hdc));
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
            with(|gamut| gamut.on_mouse_move(hwnd, x, y));
            0
        }
        WM_MOUSELEAVE => {
            with(|gamut| {
                gamut.tracking = false;
                if gamut.hover.take().is_some() {
                    gamut.invalidate();
                }
            });
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = window::mouse_pos(lp);
            SetFocus(hwnd);
            // Released on button up or capture loss.
            if with(|gamut| gamut.on_button_down(x, y)) == Some(true) {
                SetCapture(hwnd);
            }
            0
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED => {
            with(|gamut| {
                if gamut.drag.take().is_some() {
                    gamut.invalidate();
                }
            });
            if msg == WM_LBUTTONUP {
                ReleaseCapture();
            }
            0
        }
        WM_MOUSEWHEEL => {
            let delta = window::wheel_delta(wp);
            with(|gamut| gamut.on_wheel(delta));
            0
        }
        WM_KEYDOWN => {
            let vk = wp as u16;
            if vk == VK_ESCAPE {
                DestroyWindow(hwnd);
                return 0;
            }
            with(|gamut| gamut.on_key(vk));
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            on_destroy();
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[cfg(test)]
mod tests {
    use super::super::entries::{Showcase, SHOWCASE};
    use super::*;

    fn window() -> Gamut {
        let st = crate::showcase::state();
        let (label, file) = st.monitors[0].sdr_options[0].clone();
        SHOWCASE.with(|s| {
            *s.borrow_mut() = Some(Showcase {
                file: file.clone(),
                label,
                prims: None,
            })
        });
        let g = Gamut::new(Some(&file), 0, 96);
        SHOWCASE.with(|s| *s.borrow_mut() = None);
        g
    }

    #[test]
    fn keyboard_cursor_walks_entries_and_toggles() {
        let mut g = window();
        g.selected = None;
        g.on_key(VK_DOWN);
        assert_eq!(g.selected, Some(0));
        assert!(g.keyboard);
        g.on_key(VK_UP);
        assert_eq!(g.selected, Some(0), "stops at the top");
        g.on_key(VK_DOWN);
        g.on_key(VK_SPACE);
        assert!(g.entries[1].checked != window().entries[1].checked);
        for _ in 0..20 {
            g.on_key(VK_DOWN);
        }
        assert_eq!(g.selected, g.entry_order().last().copied());
    }

    #[test]
    fn clicking_a_row_toggles_it() {
        let mut g = window();
        let list = g.geo().list;
        let row = g.row_h();
        // Row 2 is the second reference space (Display P3, ticked by the showcase).
        let (x, y) = (list.l + 40, list.t + 2 * row + row / 2);
        assert_eq!(g.row_at(x, y), Some(1));
        assert!(g.entries[1].checked);
        assert!(!g.on_button_down(x, y));
        assert!(!g.entries[1].checked);
        assert_eq!(g.row_at(x, list.t + row / 2), None, "header");
    }

    #[test]
    fn the_wheel_scrolls_by_rows_within_bounds() {
        let mut g = window();
        g.on_wheel(-120);
        let max = (g.content_h() - g.geo().list.h()).max(0);
        assert_eq!(g.scroll, g.row_h().min(max));
        g.on_wheel(120 * 50);
        assert_eq!(g.scroll, 0);
    }
}
