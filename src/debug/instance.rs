//! Talking to a running instance: find its hidden window, send it a command, or ask it to exit.

use crate::app;
use crate::util::{from_wide, wide};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};
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

/// Full path of the exe that owns window `h`.
fn exe_of_window(h: HWND) -> Option<PathBuf> {
    // SAFETY: plain calls into a buffer of the stated size; the process handle is closed below.
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(process);
        (ok != 0).then(|| PathBuf::from(from_wide(&buf[..len as usize])))
    }
}

/// A running Spectrum started from another exe than this one: the pre-Store install. It holds the
/// shared single-instance mutex, so the Store build asks it to exit first. True when one was stopped.
pub fn stop_legacy_copy() -> bool {
    if !crate::util::is_packaged() {
        return false;
    }
    let Some(other) = find_running().and_then(exe_of_window) else {
        return false;
    };
    let me = std::env::current_exe().unwrap_or_default();
    let same = other.to_string_lossy().to_lowercase() == me.to_string_lossy().to_lowercase();
    if same || other.to_string_lossy().to_lowercase().contains(r"\windowsapps\") {
        return false;
    }
    crate::util::log(&format!("stopping the older copy at {}", other.display()));
    stop_running(5000);
    true
}

/// Asks the running instance to exit and waits up to `ms` for its process to end.
fn stop_running(ms: u64) {
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
