//! The main window procedure. Message handlers only change `Main` and queue [`Pending`] work; the queue
//! runs after each message with the state unborrowed, because dispatching calls back into Win32.

use super::super::draw::{
    apply_window_theme, invalidate_theme, sc, theme, trim_working_set, Fonts, R, WM_MOUSELEAVE,
};
use super::super::widgets::{self, key_to_t, Kind, DROPDOWN_DISMISSED};
use super::super::window;
use super::{with, Id, Main, Pending, CLASS, HWND_CELL, MAIN, SLIDERS};
use crate::app::UiAction;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{BeginPaint, ClientToScreen, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_SHIFT,
    VK_SPACE, VK_TAB,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, KillTimer, SetTimer, CS_HREDRAW, CS_VREDRAW, WM_ACTIVATE, WM_APP, WM_CAPTURECHANGED,
    WM_CLOSE, WM_CREATE, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN,
    WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT,
    WM_SETCURSOR, WM_SETFOCUS, WM_SETTINGCHANGE, WM_TIMER,
};

/// Posted by the dropdown popup when it closes (`wParam` = the pick or `DROPDOWN_DISMISSED`).
const WM_DROPDOWN_DONE: u32 = WM_APP + 10;
/// Applies slider edits while they happen, at most once per interval, like the Windows HDR settings do.
pub(super) const TIMER_APPLY: usize = 1;
pub(super) const APPLY_INTERVAL_MS: u32 = 100;
/// Re-reads what Windows reports after a change; it takes a moment to apply a profile or a mode.
const TIMER_REFRESH: usize = 2;
const REFRESH_DELAY_MS: u32 = 600;
/// A click on the dropdown field this soon after the popup closed only dismissed the popup.
const DROPDOWN_REOPEN_DELAY: Duration = Duration::from_millis(300);
/// Slider travel per wheel notch, as a fraction of the track.
const WHEEL_STEP: f64 = 0.03;

pub(super) fn register_class() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    window::register_class(&ONCE, CLASS, CS_HREDRAW | CS_VREDRAW, Some(wnd_proc));
}

/// Runs the queued work until none is left (an action can queue more, e.g. a refresh that resizes).
fn flush_pending() {
    loop {
        let items = with(|main| std::mem::take(&mut main.pending)).unwrap_or_default();
        if items.is_empty() {
            break;
        }
        for p in items {
            match p {
                Pending::Dispatch(action) => {
                    let hdr_change = matches!(
                        action,
                        UiAction::ApplyHdr { .. }
                            | UiAction::ResetHdr { .. }
                            | UiAction::SetHdrEnabled { .. }
                            | UiAction::SetSdrAbovePeak { .. }
                    );
                    super::super::dispatch(action);
                    if hdr_change {
                        arm_refresh();
                    }
                }
                Pending::OpenDropdown => open_dropdown_now(),
                Pending::Close => close_requested(),
                Pending::Resize => {
                    if let Some((h, dpi)) = with(|main| (main.hwnd, main.dpi)) {
                        on_dpi_changed(h, dpi, None);
                    }
                }
            }
        }
    }
}

/// Refreshes the window shortly after Windows had time to apply a change.
fn arm_refresh() {
    with(|main| {
        // SAFETY: valid window handle; re-arming replaces the pending one-shot.
        unsafe { SetTimer(main.hwnd, TIMER_REFRESH, REFRESH_DELAY_MS, None) };
    });
}

/// `rect` in client coordinates of `hwnd`, as screen coordinates.
fn to_screen(hwnd: HWND, rect: R) -> R {
    let mut tl = POINT { x: rect.l, y: rect.t };
    let mut br = POINT { x: rect.r, y: rect.b };
    // SAFETY: valid window handle and points.
    unsafe {
        ClientToScreen(hwnd, &mut tl);
        ClientToScreen(hwnd, &mut br);
    }
    R::new(tl.x, tl.y, br.x, br.y)
}

fn open_dropdown_now() {
    let request = with(|main| {
        let recently_closed = main
            .dropdown_closed_at
            .is_some_and(|t| t.elapsed() < DROPDOWN_REOPEN_DELAY);
        if recently_closed {
            return None;
        }
        let mon = main.mon()?;
        let field = main.ui.get(Id::Sdr)?;
        let items: Vec<String> = mon.sdr_options.iter().map(|o| o.0.clone()).collect();
        Some((
            main.hwnd,
            to_screen(main.hwnd, field.rect),
            items,
            mon.sdr_current,
            main.theme(),
        ))
    })
    .flatten();
    if let Some((h, anchor, items, current, th)) = request {
        widgets::open_dropdown(h, anchor, items, current, th, WM_DROPDOWN_DONE);
        with(|main| main.invalidate());
    }
}

/// Esc / close box: commit pending edits, tell the app, destroy.
fn close_requested() {
    with(|main| main.commit());
    let actions = with(|main| std::mem::take(&mut main.pending)).unwrap_or_default();
    for p in actions {
        if let Pending::Dispatch(action) = p {
            super::super::dispatch(action);
        }
    }
    super::super::dispatch(UiAction::CloseMain);
    super::close();
}

/// New DPI (or a new shape at the same DPI): new fonts, layout and icons, and the window resized to fit.
pub(super) fn on_dpi_changed(hwnd: HWND, dpi: u32, suggested: Option<RECT>) {
    let size = with(|main| {
        main.dpi = dpi;
        main.fonts = Fonts::new(dpi);
        main.canvas = None;
        main.layout();
        main.window_client_size()
    });
    refresh_icons(hwnd, dpi);
    // Nested messages during the resize are handled with MAIN unborrowed.
    let (w, h) = size.unwrap_or_else(|| {
        (
            sc(super::CLIENT_W, dpi),
            sc(super::layout::client_h(Default::default()), dpi),
        )
    });
    window::resize_client(hwnd, w, h, dpi, suggested);
}

/// Rebuilds the title bar icons for a new DPI and frees the old ones.
fn refresh_icons(hwnd: HWND, dpi: u32) {
    if with(|_| ()).is_none() {
        return;
    }
    let icons = window::set_title_icons(hwnd, dpi);
    if let Some(old) = with(|main| std::mem::replace(&mut main.icons, icons)) {
        window::destroy_icons(old);
    }
}

fn shift_down() -> bool {
    // SAFETY: plain query.
    unsafe { GetKeyState(VK_SHIFT as i32) < 0 }
}

fn has_capture(hwnd: HWND) -> bool {
    // SAFETY: plain query.
    unsafe { GetCapture() == hwnd }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let r = handle(hwnd, msg, wp, lp);
    if msg != WM_DESTROY && msg != WM_NCDESTROY {
        flush_pending();
    }
    r
}

fn on_create(hwnd: HWND) {
    HWND_CELL.with(|c| c.set(hwnd as isize));
    apply_window_theme(hwnd, theme().dark);
    let icons = window::set_initial_icons(hwnd);
    with(|main| {
        main.hwnd = hwnd;
        main.icons = icons;
    });
}

impl Main {
    fn on_mouse_move(&mut self, hwnd: HWND, wp: WPARAM, x: i32, y: i32) {
        if !self.tracking {
            window::track_mouse_leave(hwnd);
            self.tracking = true;
        }
        let Some(id) = self.drag else {
            let hover = self.ui.hit(x, y);
            self.set_hover(hover);
            return;
        };
        if wp & 1 == 0 {
            // MK_LBUTTON is gone: the button-up was missed, so end the drag here.
            self.drag = None;
            self.ui.pressed = None;
            self.commit();
        } else {
            self.slider_at_x(id, x);
            self.schedule_apply();
        }
    }

    /// Press on an enabled control: press state, focus, and a slider starts dragging. True when the mouse
    /// should be captured.
    fn on_button_down(&mut self, x: i32, y: i32) -> bool {
        let hit = self.ui.hit(x, y);
        self.ui.focus_visible = false;
        let mut capture = false;
        match hit.and_then(|id| self.ui.get(id).copied()) {
            Some(w) if w.enabled => {
                self.ui.pressed = Some(w.id);
                if w.focusable {
                    self.ui.focus = Some(w.id);
                }
                if w.kind == Kind::Slider {
                    self.drag = Some(w.id);
                    self.slider_at_x(w.id, x);
                }
                capture = true;
            }
            Some(_) => {}
            None => self.ui.focus = None,
        }
        self.invalidate();
        capture
    }

    /// Release: ends a drag (applying it) or activates the pressed control when still over it.
    fn on_button_up(&mut self, x: i32, y: i32) {
        let pressed = self.ui.pressed.take();
        if let Some(id) = self.drag.take() {
            self.slider_at_x(id, x);
            self.commit();
        } else if let Some(p) = pressed {
            if self.ui.hit(x, y) == Some(p) && self.ui.get(p).map(|w| w.enabled).unwrap_or(false) {
                self.activate(p);
            }
        }
        let hover = self.ui.hit(x, y);
        self.set_hover(hover);
        self.invalidate();
    }

    /// Moves the slider under the pointer by `delta` wheel units.
    fn on_wheel(&mut self, x: i32, y: i32, delta: i32) {
        let Some(id) = self.ui.hit(x, y) else {
            return;
        };
        if SLIDERS.contains(&id) && self.ui.get(id).map(|w| w.enabled).unwrap_or(false) {
            let t = (self.slider_t(id) + WHEEL_STEP * (delta as f64 / 120.0)).clamp(0.0, 1.0);
            self.set_slider_t(id, t);
            self.schedule_apply();
            self.invalidate();
        }
    }

    /// Esc closes, Tab moves focus, Space / Enter activate, arrows and paging keys move a focused slider,
    /// Down opens a focused dropdown.
    fn on_key(&mut self, vk: u16, shift: bool) {
        match vk {
            VK_ESCAPE => self.pending.push(Pending::Close),
            VK_TAB => {
                self.ui.focus_next(shift);
                self.invalidate();
            }
            VK_SPACE | VK_RETURN => {
                self.ui.focus_visible = true;
                if let Some(f) = self.ui.focus {
                    self.activate(f);
                }
                self.invalidate();
            }
            _ => {
                self.ui.focus_visible = true;
                match self.ui.focus {
                    Some(f) if SLIDERS.contains(&f) => {
                        if let Some(t) = key_to_t(self.slider_t(f), vk) {
                            self.set_slider_t(f, t);
                            self.schedule_apply();
                        }
                    }
                    Some(Id::Sdr) if vk == VK_DOWN => self.pending.push(Pending::OpenDropdown),
                    _ => {}
                }
                self.invalidate();
            }
        }
    }

    /// The dropdown popup closed with `pick` (an index, or `DROPDOWN_DISMISSED`).
    fn on_dropdown_done(&mut self, pick: usize) {
        self.dropdown_closed_at = Some(Instant::now());
        if pick != DROPDOWN_DISMISSED {
            let mon = self.selected;
            self.pending
                .push(Pending::Dispatch(UiAction::SetSdr { mon, idx: pick }));
        }
        self.invalidate();
    }

    /// The hand cursor over enabled controls other than sliders.
    fn wants_hand(&self, x: i32, y: i32) -> bool {
        self.ui
            .hit(x, y)
            .and_then(|id| self.ui.get(id).copied())
            .is_some_and(|w| w.enabled && w.kind != Kind::Slider)
    }
}

fn on_destroy(hwnd: HWND) {
    // SAFETY: valid window; killing a timer that is not set is harmless.
    unsafe {
        KillTimer(hwnd, TIMER_APPLY);
        KillTimer(hwnd, TIMER_REFRESH);
    }
    HWND_CELL.with(|c| c.set(0));
    let main = MAIN.with(|m| m.try_borrow_mut().ok().and_then(|mut b| b.take()));
    if let Some(m) = &main {
        window::destroy_icons(m.icons);
    }
    drop(main); // fonts, canvas (DIB + DC)
    trim_working_set();
}

unsafe fn handle(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            on_create(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            with(|main| main.paint(hdc));
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
            with(|main| main.on_mouse_move(hwnd, wp, x, y));
            0
        }
        WM_MOUSELEAVE => {
            with(|main| {
                main.tracking = false;
                if main.drag.is_none() {
                    main.set_hover(None);
                }
            });
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = window::mouse_pos(lp);
            SetFocus(hwnd);
            if with(|main| main.on_button_down(x, y)) == Some(true) {
                SetCapture(hwnd);
            }
            0
        }
        WM_LBUTTONUP => {
            let (x, y) = window::mouse_pos(lp);
            with(|main| main.on_button_up(x, y));
            // After the state is taken: ReleaseCapture sends WM_CAPTURECHANGED.
            if has_capture(hwnd) {
                ReleaseCapture();
            }
            0
        }
        WM_CAPTURECHANGED => {
            // Capture stolen mid-press (Alt+Tab, a menu, a notification): end the press and drag.
            with(|main| {
                main.ui.pressed = None;
                if main.drag.take().is_some() {
                    main.commit();
                }
                main.invalidate();
            });
            0
        }
        WM_MOUSEWHEEL => {
            let (sx, sy) = window::mouse_pos(lp);
            let (x, y) = window::screen_to_client(hwnd, sx, sy);
            let delta = window::wheel_delta(wp);
            with(|main| main.on_wheel(x, y, delta));
            0
        }
        WM_KEYDOWN => {
            let shift = shift_down();
            with(|main| main.on_key(wp as u16, shift));
            0
        }
        WM_TIMER => {
            if wp == TIMER_APPLY {
                KillTimer(hwnd, TIMER_APPLY);
                with(|main| {
                    main.apply_armed = false;
                    main.commit();
                });
            } else if wp == TIMER_REFRESH {
                KillTimer(hwnd, TIMER_REFRESH);
                super::refresh();
            }
            0
        }
        WM_DISPLAYCHANGE => {
            arm_refresh();
            0
        }
        WM_DROPDOWN_DONE => {
            with(|main| main.on_dropdown_done(wp));
            0
        }
        WM_ACTIVATE => {
            // Brightness changed in Settings while this window was in the background.
            if wp & 0xFFFF != 0 {
                super::refresh();
            }
            0
        }
        WM_SETFOCUS | WM_KILLFOCUS => {
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_SETCURSOR => {
            if window::is_client_hit(lp) {
                let (x, y) = window::cursor_pos(hwnd);
                let hand = with(|main| main.wants_hand(x, y)).unwrap_or(false);
                window::set_cursor(hand);
                return 1;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_CLOSE => {
            close_requested();
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

    fn main() -> Main {
        let mut st = crate::showcase::state();
        st.advanced = true;
        Main::new(st, 96)
    }

    #[test]
    fn a_click_activates_on_release_over_the_same_control() {
        let mut m = main();
        let r = m.ui.get(Id::Read).unwrap().rect;
        assert!(m.on_button_down(r.l + 2, r.t + 2));
        assert_eq!(m.ui.pressed, Some(Id::Read));
        m.on_button_up(r.l + 3, r.t + 3);
        assert_eq!(m.ui.pressed, None);
        assert_eq!(m.pending.len(), 1);
        // Released elsewhere: nothing happens.
        m.pending.clear();
        m.on_button_down(r.l + 2, r.t + 2);
        m.on_button_up(0, 0);
        assert!(m.pending.is_empty());
    }

    #[test]
    fn a_press_on_a_slider_drags_it() {
        let mut m = main();
        let r = m.ui.get(Id::Peak).unwrap().rect;
        assert!(m.on_button_down(r.r - 1, r.t + 2));
        assert_eq!(m.drag, Some(Id::Peak));
        assert_eq!(m.peak, widgets::PEAK_MAX_NITS);
        m.on_button_up(r.l, r.t + 2);
        assert_eq!(m.drag, None);
        assert_eq!(m.peak, widgets::PEAK_MIN_NITS);
        assert_eq!(m.pending.len(), 1, "the drag is applied on release");
    }

    #[test]
    fn a_press_on_empty_space_drops_focus() {
        let mut m = main();
        m.ui.focus = Some(Id::Read);
        assert!(!m.on_button_down(-10, -10));
        assert_eq!(m.ui.focus, None);
    }

    #[test]
    fn wheel_and_keys_move_sliders() {
        let mut m = main();
        let r = m.ui.get(Id::Sat).unwrap().rect;
        m.on_wheel(r.l + 2, r.t + 2, 120 * 10);
        assert!((m.sat - 30.0).abs() < 1e-9, "{}", m.sat);
        m.ui.focus = Some(Id::Sat);
        m.on_key(windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_HOME, false);
        assert_eq!(m.sat, 0.0);
        m.ui.focus = Some(Id::Sdr);
        m.on_key(VK_DOWN, false);
        assert!(matches!(m.pending.last(), Some(Pending::OpenDropdown)));
        m.on_key(VK_ESCAPE, false);
        assert!(matches!(m.pending.last(), Some(Pending::Close)));
    }

    #[test]
    fn dropdown_picks_become_sdr_actions() {
        let mut m = main();
        m.on_dropdown_done(DROPDOWN_DISMISSED);
        assert!(m.pending.is_empty() && m.dropdown_closed_at.is_some());
        m.on_dropdown_done(3);
        assert!(matches!(
            m.pending.last(),
            Some(Pending::Dispatch(UiAction::SetSdr { mon: 0, idx: 3 }))
        ));
    }

    #[test]
    fn the_hand_cursor_is_for_buttons_not_sliders() {
        let m = main();
        let button = m.ui.get(Id::Compare).unwrap().rect;
        let slider = m.ui.get(Id::Peak).unwrap().rect;
        assert!(m.wants_hand(button.l + 1, button.t + 1));
        assert!(!m.wants_hand(slider.l + 1, slider.t + 1));
        assert!(!m.wants_hand(-5, -5));
    }
}
