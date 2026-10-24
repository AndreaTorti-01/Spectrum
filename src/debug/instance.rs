//! Talking to a running instance: find its hidden window, send it a command, or ask it to exit.

use crate::app;
use crate::util::wide;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowThreadProcessId, PostMessageW, SendMessageW, WM_CLOSE, WM_COPYDATA,
};

/// The hidden window of a running Spectrum, if any.
pub fn find_running() -> Option<HWND> {
    // SAFETY: plain lookup.
    let h = unsafe { FindWindowW(wide(app::WINDOW_CLASS).as_ptr(), std::ptr::null()) };
    (!h.is_null()).then_some(h)
}

/// Sends a debug command (see `app::run_command`) to the running instance. False when none runs.
pub fn send_to_running(cmd: &str) -> bool {
    let Some(h) = find_running() else {
        return false;
    };
    let bytes = cmd.as_bytes();
    let cds = COPYDATASTRUCT {
        dwData: app::COPYDATA_MAGIC,
        cbData: bytes.len() as u32,
        lpData: bytes.as_ptr() as *mut _,
    };
    // SAFETY: the data outlives the synchronous SendMessage.
    unsafe {
        SendMessageW(h, WM_COPYDATA, 0, &cds as *const COPYDATASTRUCT as isize);
    }
    true
}

/// Asks a running instance to exit (`--quit`, `--cleanup`) and waits up to `ms` for its process to end, so
/// the installer can overwrite the exe right after.
pub fn stop_running(ms: u64) {
    let Some(h) = find_running() else {
        return;
    };
    // SAFETY: plain calls; the process handle is closed below.
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        PostMessageW(h, WM_CLOSE, 0, 0);
        if !process.is_null() {
            WaitForSingleObject(process, ms as u32);
            CloseHandle(process);
            return;
        }
    }
    // No process handle: wait for the window to go instead.
    let start = Instant::now();
    while find_running().is_some() && start.elapsed().as_millis() < ms as u128 {
        std::thread::sleep(Duration::from_millis(50));
    }
}
