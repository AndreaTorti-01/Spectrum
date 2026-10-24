//! Tray menu, the debug/IPC command queue and exit.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use super::apply::{dispatch, open_gamut, UiAction};
use super::snapshot::sdr_options;
use super::{enumerate_cached, hwnd, monitor_cfg, monitor_name, Config};
use crate::tray::{self, MenuMonitor, TrayCmd};
use crate::ui::{self, draw};
use crate::util::{log, wide};
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, FindWindowW, GetWindowRect, PostMessageW, PostQuitMessage, SetCursorPos, SetWindowPos,
    SystemParametersInfoW, HWND_NOTOPMOST, HWND_TOPMOST, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, WM_APP,
};

/// Posted to the message window when [`post_command`] or `WM_COPYDATA` queued a command.
pub(super) const WM_RUN_CMD: u32 = WM_APP + 3;
/// Ignore repeated tray notifications (v4 sends several per click) for this long.
const TRAY_DEBOUNCE: Duration = Duration::from_millis(300);

thread_local! {
    static LAST_TOGGLE: Cell<Option<Instant>> = const { Cell::new(None) };
    static LAST_MENU: Cell<Option<Instant>> = const { Cell::new(None) };
    /// True while the (modal) tray menu is up; the second v4 notification must not open another.
    static IN_MENU: Cell<bool> = const { Cell::new(false) };
    static COMMANDS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// True when `last` is within the tray debounce window.
fn too_soon(last: Option<Instant>) -> bool {
    last.is_some_and(|t| t.elapsed() < TRAY_DEBOUNCE)
}

fn menu_monitors() -> Vec<MenuMonitor> {
    if ui::is_mock() {
        // Screenshots: the menu shows the mock state, like the windows do.
        return ui::snapshot().monitors.iter().map(MenuMonitor::from_ui).collect();
    }
    let cfg = Config::load();
    enumerate_cached()
        .iter()
        .map(|m| {
            let (profiles, current) = sdr_options(m, &monitor_cfg(&cfg, m));
            MenuMonitor {
                label: format!("{} \u{b7} {}", m.num, monitor_name(m)),
                profiles,
                current,
                hdr_supported: m.hdr_supported,
                hdr_on: m.hdr_enabled,
            }
        })
        .collect()
}

/// Shows the result of a change made outside the main window.
pub(super) fn refresh_main_window() {
    if ui::main_window::is_open() {
        ui::main_window::refresh();
    }
}

/// Runs a tray menu command (also used by `--debug-tray-cmd`).
pub fn run_tray_cmd(cmd: TrayCmd) {
    match cmd {
        TrayCmd::Open => ui::main_window::open(),
        TrayCmd::Exit => {
            exit_app();
            return;
        }
        TrayCmd::SetSdr { mon, idx } => {
            dispatch(UiAction::SetSdr { mon, idx });
            refresh_main_window();
        }
        TrayCmd::ToggleHdr { mon } => {
            let on = enumerate_cached()
                .get(mon)
                .map(|m| m.hdr_enabled)
                .unwrap_or(false);
            dispatch(UiAction::SetHdrEnabled { mon, on: !on });
            refresh_main_window();
        }
    }
    draw::trim_working_set();
}

/// Shows the tray menu at the cursor (modal) and runs the chosen command.
pub fn show_tray_menu() {
    if IN_MENU.with(|c| c.get()) || too_soon(LAST_MENU.with(|l| l.get())) {
        return;
    }
    IN_MENU.with(|c| c.set(true));
    let items = menu_monitors();
    let cmd = tray::show_menu(hwnd(), &items);
    LAST_MENU.with(|l| l.set(Some(Instant::now())));
    IN_MENU.with(|c| c.set(false));
    // A menu built from a mock state must never run anything.
    match cmd.filter(|_| !ui::is_mock()) {
        Some(c) => run_tray_cmd(c),
        None => draw::trim_working_set(),
    }
}

/// Left click on the tray icon: opens the main window, or closes it when it is open.
pub(super) fn toggle_main() {
    if too_soon(LAST_TOGGLE.with(|l| l.get())) {
        return;
    }
    LAST_TOGGLE.with(|l| l.set(Some(Instant::now())));
    if ui::main_window::is_open() {
        ui::main_window::request_close();
    } else {
        ui::main_window::open();
    }
}

/// Moves the cursor to a fixed point near the bottom-right of the primary work area (screenshots).
pub fn park_cursor_for_menu() {
    let mut work_area = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    // SAFETY: valid RECT pointers; the taskbar handle is only queried.
    unsafe {
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work_area as *mut _ as *mut _, 0);
        // A bottom taskbar may be auto-hidden (work area = full screen, bar sliding in and out), so keep a
        // fixed distance from the bottom edge: the menu stays clear of the bar and of its hover zone.
        let bar = FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null());
        let mut bar_rect = work_area;
        let bar_at_bottom = !bar.is_null()
            && GetWindowRect(bar, &mut bar_rect) != 0
            && bar_rect.top > (work_area.top + work_area.bottom) / 2;
        let bottom = if bar_at_bottom {
            work_area.bottom - 60
        } else {
            work_area.bottom
        };
        SetCursorPos(work_area.right - 260, bottom - 12);
    }
}

/// Puts a window at the top of the z-order without stealing focus (foreground rules may refuse
/// `SetForegroundWindow`, which would leave a freshly opened window behind others).
pub fn raise(h: HWND) {
    if h.is_null() {
        return;
    }
    // SAFETY: valid window handle on this thread.
    unsafe {
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, flags);
        SetWindowPos(h, HWND_NOTOPMOST, 0, 0, 0, 0, flags);
    }
}

/// Adds a command to the queue [`take_command`] drains.
pub(super) fn queue_command(cmd: String) {
    COMMANDS.with(|c| c.borrow_mut().push(cmd));
}

/// The oldest queued command, if any.
pub(super) fn take_command() -> Option<String> {
    COMMANDS.with(|c| {
        let mut queue = c.borrow_mut();
        (!queue.is_empty()).then(|| queue.remove(0))
    })
}

/// Queues a command and posts it to the message window, so it runs from the loop (modal menus included).
pub fn post_command(cmd: &str) {
    queue_command(cmd.to_string());
    // SAFETY: plain post to our own window (a null handle is rejected by the OS).
    unsafe { PostMessageW(hwnd(), WM_RUN_CMD, 0, 0) };
}

/// Debug and IPC commands: `open main`, `open main-hdr`, `open gamut`, `open traymenu`, `tray-cmd <id>`.
pub fn run_command(cmd: &str) {
    let mut words = cmd.split_whitespace();
    match (words.next(), words.next()) {
        (Some("open"), Some(what)) => {
            if !open_window(what) {
                log(&format!("unknown command {cmd:?}"));
            }
        }
        (Some("tray-cmd"), Some(id)) => {
            if let Some(c) = id.parse::<u32>().ok().and_then(tray::decode_cmd) {
                run_tray_cmd(c);
            }
        }
        _ => log(&format!("unknown command {cmd:?}")),
    }
}

/// `open <what>`: opens and raises a window or shows the tray menu. False for an unknown name.
fn open_window(what: &str) -> bool {
    match what {
        "main" => {
            ui::main_window::open();
            raise(ui::main_window::hwnd());
        }
        "main-hdr" => {
            // Screenshots: the main window on a mock state with HDR on (nothing is applied).
            ui::set_mock(Some(crate::debug::showcase_state()));
            ui::main_window::open();
            raise(ui::main_window::hwnd());
        }
        "traymenu-showcase" => {
            ui::set_mock(Some(crate::debug::showcase_state()));
            park_cursor_for_menu();
            show_tray_menu();
        }
        "gamut-showcase" => {
            crate::debug::open_gamut_showcase();
            raise(ui::gamut_window::hwnd());
        }
        "profiles" => {
            ui::profiles_window::open();
            raise(ui::profiles_window::hwnd());
        }
        "profiles-showcase" => {
            ui::profiles_window::open_showcase(crate::debug::showcase_profiles());
            raise(ui::profiles_window::hwnd());
        }
        "gamut" => {
            open_gamut();
            raise(ui::gamut_window::hwnd());
        }
        "traymenu" => {
            park_cursor_for_menu();
            show_tray_menu();
        }
        _ => return false,
    }
    true
}

/// Removes the tray icon, closes every window and ends the message loop.
pub(super) fn exit_app() {
    let h = hwnd();
    tray::remove(h);
    ui::overlay::hide_all();
    ui::gamut_window::close();
    ui::profiles_window::close();
    ui::main_window::request_close();
    if !h.is_null() {
        // SAFETY: window created by this thread; WM_DESTROY posts the quit message.
        unsafe { DestroyWindow(h) };
    } else {
        // SAFETY: plain call.
        unsafe { PostQuitMessage(0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_queue_is_first_in_first_out() {
        assert_eq!(take_command(), None);
        queue_command("open main".into());
        queue_command("tray-cmd 1".into());
        assert_eq!(take_command().as_deref(), Some("open main"));
        assert_eq!(take_command().as_deref(), Some("tray-cmd 1"));
        assert_eq!(take_command(), None);
    }

    #[test]
    fn debounce_window() {
        assert!(!too_soon(None));
        assert!(too_soon(Some(Instant::now())));
        let old = Instant::now().checked_sub(Duration::from_secs(1));
        assert!(old.is_none_or(|t| !too_soon(Some(t))));
    }
}
