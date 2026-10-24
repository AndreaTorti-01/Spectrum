//! The hidden message window: tray notifications, display and power changes, IPC, and the message loop.

use super::commands::{
    exit_app, queue_command, refresh_main_window, run_command, show_tray_menu, take_command, toggle_main,
    WM_RUN_CMD,
};
use super::reconcile::{first_run, reconcile};
use super::{set_hwnd, tray_mode, COPYDATA_MAGIC, SHOW_MESSAGE_NAME, TRAY_MODE, WINDOW_CLASS};
use crate::tray::{self, WM_TRAY};
use crate::ui::{self, draw};
use crate::util::{log, wide};
use std::cell::{Cell, RefCell};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::NIN_SELECT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, PostMessageW, PostQuitMessage,
    RegisterClassExW, RegisterWindowMessageW, SetTimer, TranslateMessage, DBT_DEVNODES_CHANGED, MSG,
    PBT_APMRESUMEAUTOMATIC, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_COPYDATA, WM_DESTROY, WM_DEVICECHANGE,
    WM_DISPLAYCHANGE, WM_LBUTTONUP, WM_POWERBROADCAST, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSEXW, WS_POPUP,
};

const TIMER_RECONCILE: usize = 1;
const RECONCILE_DELAY_MS: u32 = 2000;
/// Retries adding the tray icon while the shell is not ready (Run-key start at logon).
const TIMER_TRAY_RETRY: usize = 2;
const TRAY_RETRY_MS: u32 = 2000;
const TRAY_RETRY_MAX: u32 = 15;
/// NIN_KEYSELECT (NIN_SELECT | 1).
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
const WM_STARTUP: u32 = WM_APP + 2;
/// Longest debug command accepted through `WM_COPYDATA`, in bytes.
const MAX_COPYDATA: usize = 256;

thread_local! {
    static TRAY_TRIES: Cell<u32> = const { Cell::new(0) };
    static SHOW_MSG: Cell<u32> = const { Cell::new(0) };
    static TASKBAR_MSG: Cell<u32> = const { Cell::new(0) };
    static OPTS: RefCell<Option<RunOpts>> = const { RefCell::new(None) };
}

/// How the message loop starts.
#[derive(Clone, Debug, Default)]
pub struct RunOpts {
    /// Put the icon in the notification area (otherwise this is a short-lived debug window).
    pub tray: bool,
    /// Open the main window right after startup.
    pub show_main: bool,
    /// A command (see [`run_command`](super::run_command)) to run once the loop is up.
    pub command: Option<String>,
}

/// Adds the tray icon; on failure (shell not ready yet) retries every 2 s, at most 15 times.
fn add_tray_with_retry(hwnd: HWND) {
    if tray::add(hwnd) {
        TRAY_TRIES.with(|t| t.set(0));
        // SAFETY: valid window; killing an unset timer is harmless.
        unsafe { KillTimer(hwnd, TIMER_TRAY_RETRY) };
        return;
    }
    let tries = TRAY_TRIES.with(|t| t.get());
    if tries >= TRAY_RETRY_MAX {
        // SAFETY: as above.
        unsafe { KillTimer(hwnd, TIMER_TRAY_RETRY) };
        log("could not add the tray icon; giving up");
        return;
    }
    TRAY_TRIES.with(|t| t.set(tries + 1));
    log(&format!(
        "could not add the tray icon; retry {} of {TRAY_RETRY_MAX}",
        tries + 1
    ));
    // SAFETY: valid window.
    unsafe { SetTimer(hwnd, TIMER_TRAY_RETRY, TRAY_RETRY_MS, None) };
}

fn schedule_reconcile(hwnd: HWND) {
    // SAFETY: valid window; re-arming the same timer id restarts the delay.
    unsafe {
        SetTimer(hwnd, TIMER_RECONCILE, RECONCILE_DELAY_MS, None);
    }
}

/// Tray icon notification (`lParam` low word is the event in NOTIFYICON_VERSION_4).
fn on_tray(lp: LPARAM) {
    match (lp as usize & 0xFFFF) as u32 {
        WM_LBUTTONUP | NIN_SELECT | NIN_KEYSELECT => toggle_main(),
        // v4 sends WM_CONTEXTMENU for a right click and for Shift+F10 / the menu key.
        WM_CONTEXTMENU => show_tray_menu(),
        _ => {}
    }
}

/// First message of the loop: first run, reconcile, then what `RunOpts` asked for.
fn on_startup() {
    let opts = OPTS.with(|o| o.borrow_mut().take()).unwrap_or_default();
    if !crate::config::config_path().exists() {
        first_run();
    }
    for line in reconcile() {
        log(&format!("startup: {line}"));
    }
    if opts.show_main {
        ui::main_window::open();
    }
    if let Some(c) = opts.command {
        run_command(&c);
    }
    draw::trim_working_set();
}

/// A debug command from a second instance (`debug::send_to_running`). True when it was ours.
///
/// # Safety
/// `lp` is the `COPYDATASTRUCT` pointer of a `WM_COPYDATA` message, valid for the duration of the call.
unsafe fn on_copydata(hwnd: HWND, lp: LPARAM) -> bool {
    let cds = lp as *const COPYDATASTRUCT;
    if cds.is_null() || (*cds).dwData != COPYDATA_MAGIC || (*cds).lpData.is_null() {
        return false;
    }
    let len = ((*cds).cbData as usize).min(MAX_COPYDATA);
    let bytes = std::slice::from_raw_parts((*cds).lpData as *const u8, len);
    queue_command(String::from_utf8_lossy(bytes).trim_end_matches('\0').to_string());
    // Handled from a posted message so the sender is not blocked by a modal menu.
    PostMessageW(hwnd, WM_RUN_CMD, 0, 0);
    true
}

fn on_timer(hwnd: HWND, id: usize) {
    if id == TIMER_RECONCILE {
        // SAFETY: valid window.
        unsafe { KillTimer(hwnd, TIMER_RECONCILE) };
        let lines = reconcile();
        if !lines.is_empty() {
            log(&format!("display change: {} reconcile action(s)", lines.len()));
        }
        refresh_main_window();
        draw::trim_working_set();
    } else if id == TIMER_TRAY_RETRY {
        add_tray_with_retry(hwnd);
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // Registered messages have no constant; 0 means registration failed.
    if msg != 0 && msg == SHOW_MSG.with(|c| c.get()) {
        ui::main_window::open();
        super::raise(ui::main_window::hwnd());
        return 0;
    }
    if msg != 0 && msg == TASKBAR_MSG.with(|c| c.get()) {
        // Explorer restarted: the tray icon is gone.
        if tray_mode() {
            TRAY_TRIES.with(|t| t.set(0));
            add_tray_with_retry(hwnd);
        }
        return 0;
    }
    match msg {
        WM_TRAY => {
            on_tray(lp);
            0
        }
        WM_STARTUP => {
            on_startup();
            0
        }
        WM_RUN_CMD => {
            if let Some(c) = take_command() {
                run_command(&c);
            }
            0
        }
        WM_COPYDATA => on_copydata(hwnd, lp) as LRESULT,
        WM_DISPLAYCHANGE => {
            schedule_reconcile(hwnd);
            0
        }
        WM_DEVICECHANGE => {
            if wp as u32 == DBT_DEVNODES_CHANGED {
                schedule_reconcile(hwnd);
            }
            1
        }
        WM_POWERBROADCAST => {
            if wp as u32 == PBT_APMRESUMEAUTOMATIC {
                schedule_reconcile(hwnd);
            }
            1
        }
        WM_SETTINGCHANGE => {
            if ui::window::is_theme_change(wp, lp) {
                draw::invalidate_theme();
            }
            0
        }
        WM_TIMER => {
            on_timer(hwnd, wp);
            0
        }
        WM_CLOSE => {
            exit_app();
            0
        }
        WM_DESTROY => {
            set_hwnd(std::ptr::null_mut());
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn create_window() -> HWND {
    let cls = wide(WINDOW_CLASS);
    // SAFETY: class data outlives the calls; the window is never shown.
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(wnd_proc);
        wc.hInstance = hinst;
        wc.lpszClassName = cls.as_ptr();
        RegisterClassExW(&wc);
        CreateWindowExW(
            0,
            cls.as_ptr(),
            wide("Spectrum").as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        )
    }
}

/// Creates the hidden window (and the tray icon), then runs the message loop until the app exits.
pub fn run(opts: RunOpts) -> i32 {
    TRAY_MODE.with(|t| t.set(opts.tray));
    let tray_on = opts.tray;
    // SAFETY: plain registration calls.
    unsafe {
        SHOW_MSG.with(|c| c.set(RegisterWindowMessageW(wide(SHOW_MESSAGE_NAME).as_ptr())));
        TASKBAR_MSG.with(|c| c.set(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr())));
    }
    OPTS.with(|o| *o.borrow_mut() = Some(opts));
    let h = create_window();
    if h.is_null() {
        log("could not create the message window");
        return 1;
    }
    set_hwnd(h);
    log(&format!("message window {h:?} up, tray={tray_on}"));
    if tray_on {
        add_tray_with_retry(h);
    }
    // SAFETY: valid window handle.
    unsafe {
        PostMessageW(h, WM_STARTUP, 0, 0);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    0
}

/// Silent tray start.
pub fn run_tray() -> i32 {
    run(RunOpts {
        tray: true,
        ..Default::default()
    })
}
