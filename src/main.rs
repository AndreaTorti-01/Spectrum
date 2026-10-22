#![windows_subsystem = "windows"]
//! Thin CLI shell: parses switches, handles single instance and the first-run install flow, and hands off
//! to the library.

use spectrum::app::{self, RunOpts};
use spectrum::ui::draw;
use spectrum::ui::install_window::{self, InstallChoice};
use spectrum::util::{log, wide};
use spectrum::{debug, installer};
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, GetWindowThreadProcessId, MessageBoxW, PostMessageW, RegisterWindowMessageW,
    MB_ICONERROR, MB_ICONINFORMATION, MB_OK,
};

const MUTEX_NAME: &str = "Local\\Spectrum.SingleInstance";

#[derive(Debug, PartialEq)]
enum Command {
    /// No switches: install prompt / launch installed copy / run.
    Default,
    Tray,
    Uninstall,
    DebugDump {
        json: bool,
    },
    DebugEdid(u32),
    DebugWipe(u32),
    DebugReconcile,
    DebugOpen(String),
    DebugTrayCmd(u32),
    DebugDemo,
}

#[derive(Debug)]
struct Args {
    command: Command,
    /// `--theme dark|light`: forces the theme (screenshots only).
    dark: Option<bool>,
    /// `--out <file>`: debug output target (console otherwise).
    out: Option<PathBuf>,
}

fn parse_args<I: Iterator<Item = String>>(mut it: I) -> Args {
    let mut args = Args {
        command: Command::Default,
        dark: None,
        out: None,
    };
    let mut json = false;
    let mut dump = false;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--tray" => args.command = Command::Tray,
            "--uninstall" => args.command = Command::Uninstall,
            "--theme" => args.dark = it.next().map(|v| v.eq_ignore_ascii_case("dark")),
            "--out" => args.out = it.next().map(PathBuf::from),
            "--json" => json = true,
            "--debug-dump" => dump = true,
            "--debug-edid" => args.command = Command::DebugEdid(num(it.next())),
            "--debug-wipe" => args.command = Command::DebugWipe(num(it.next())),
            "--debug-reconcile" => args.command = Command::DebugReconcile,
            "--debug-open" => args.command = Command::DebugOpen(it.next().unwrap_or_default()),
            "--debug-tray-cmd" => args.command = Command::DebugTrayCmd(num(it.next())),
            "--debug-demo" => args.command = Command::DebugDemo,
            _ => {}
        }
    }
    if dump {
        args.command = Command::DebugDump { json };
    }
    args
}

fn num(s: Option<String>) -> u32 {
    s.and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn message_box(text: &str, error: bool) {
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(text).as_ptr(),
            wide("Spectrum").as_ptr(),
            MB_OK | if error { MB_ICONERROR } else { MB_ICONINFORMATION },
        );
    }
}

/// Takes the single-instance mutex. `None` when another instance already holds it.
fn acquire_instance() -> Option<HANDLE> {
    // SAFETY: plain mutex creation; the handle is kept for the life of the process.
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 0, wide(MUTEX_NAME).as_ptr());
        if h.is_null() {
            return None;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(h);
            return None;
        }
        Some(h)
    }
}

/// Asks the running instance to open its main window.
fn show_running() {
    let Some(h) = debug::find_running() else {
        return;
    };
    // SAFETY: plain calls; the registered message is the one the running instance listens for.
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        AllowSetForegroundWindow(pid);
        let msg = RegisterWindowMessageW(wide(app::SHOW_MESSAGE_NAME).as_ptr());
        PostMessageW(h, msg, 0, 0);
    }
}

/// Starts `exe` with `args` as a detached process.
fn spawn(exe: &Path, args: &str) -> bool {
    let mut cmd = wide(&format!("\"{}\" {args}", exe.display()));
    // SAFETY: zeroed STARTUPINFOW/PROCESS_INFORMATION are valid inputs; handles are closed right away.
    unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            std::ptr::null(),
            cmd.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            &si,
            &mut pi,
        );
        if ok == 0 {
            return false;
        }
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
    }
    true
}

/// True when the running exe differs from the installed one (size or newer modification time).
fn differs_from_installed() -> bool {
    let (Ok(cur), Ok(inst)) = (
        std::env::current_exe().and_then(std::fs::metadata),
        std::fs::metadata(installer::installed_exe()),
    ) else {
        return false;
    };
    if cur.len() != inst.len() {
        return true;
    }
    match (cur.modified(), inst.modified()) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}

fn run_interactive() -> i32 {
    app::run(RunOpts {
        tray: true,
        show_main: true,
        command: None,
    })
}

/// No switches: first-run install box, launch of the installed copy, or just run.
fn run_default() -> i32 {
    let Some(mutex) = acquire_instance() else {
        show_running();
        return 0;
    };
    if installer::is_running_installed() {
        return run_interactive();
    }
    let installed = installer::installed_exe().exists();
    if installed {
        // Another copy (Downloads, a build folder) was started: update the installed one if this one differs,
        // then hand over to it.
        if differs_from_installed() {
            if let Err(e) = installer::install(installer::autostart_enabled()) {
                log(&format!("update of the installed copy failed: {e}"));
            }
        }
        // SAFETY: releasing our own handle so the launched copy can take the mutex.
        unsafe { CloseHandle(mutex) };
        if spawn(&installer::installed_exe(), "") {
            return 0;
        }
        message_box("Couldn't start the installed copy of Spectrum.", true);
        return 1;
    }
    match install_window::run() {
        InstallChoice::Install { autostart } => match installer::install(autostart) {
            Ok(exe) => {
                // SAFETY: releasing our own handle so the installed copy can take the mutex.
                unsafe { CloseHandle(mutex) };
                if spawn(&exe, "") {
                    0
                } else {
                    message_box(
                        "Installed, but couldn't start Spectrum. Start it from the Start menu.",
                        true,
                    );
                    1
                }
            }
            Err(e) => {
                log(&format!("install failed: {e}"));
                message_box(&format!("Couldn't install Spectrum. {e}"), true);
                1
            }
        },
        InstallChoice::JustRun => run_interactive(),
        InstallChoice::Cancel => 0,
    }
}

fn run_tray() -> i32 {
    if acquire_instance().is_none() {
        return 0;
    }
    app::run_tray()
}

fn uninstall() -> i32 {
    debug::stop_running(3000);
    app::remove_all_profiles();
    message_box("Spectrum is uninstalled.", false);
    if let Err(e) = installer::uninstall() {
        message_box(&format!("Couldn't remove all of Spectrum. {e}"), true);
        return 1;
    }
    0
}

fn debug_open(cmd: String) -> i32 {
    if debug::send_to_running(&cmd) {
        return 0;
    }
    app::run(RunOpts {
        tray: false,
        show_main: false,
        command: Some(cmd),
    })
}

fn main() {
    // SAFETY: process-wide DPI mode, set before any window exists.
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let args = parse_args(std::env::args().skip(1));
    draw::set_forced_theme(args.dark);
    let out = args.out.as_deref();
    let code = match args.command {
        Command::Default => run_default(),
        Command::Tray => run_tray(),
        Command::Uninstall => uninstall(),
        Command::DebugDump { json } => {
            debug::dump(json, out);
            0
        }
        Command::DebugEdid(n) => {
            debug::read_edid(n, out);
            0
        }
        Command::DebugWipe(n) => {
            debug::wipe(n, out);
            0
        }
        Command::DebugReconcile => {
            debug::reconcile(out);
            0
        }
        Command::DebugOpen(what) => debug_open(format!("open {what}")),
        Command::DebugTrayCmd(id) => debug_open(format!("tray-cmd {id}")),
        Command::DebugDemo => app::run(RunOpts {
            tray: false,
            show_main: false,
            command: Some("demo".into()),
        }),
    };
    std::process::exit(code);
}
