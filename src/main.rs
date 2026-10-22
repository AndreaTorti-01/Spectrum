#![windows_subsystem = "windows"]
//! Thin CLI shell: parses switches, handles single instance, and hands off to the library. The app never
//! installs itself: `installer/spectrum.iss` does that, and `--cleanup` / `--quit` serve its uninstaller.

use spectrum::app::{self, RunOpts};
use spectrum::ui::draw;
use spectrum::util::wide;
use spectrum::{autostart, debug};
use std::path::PathBuf;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, GetWindowThreadProcessId, PostMessageW, RegisterWindowMessageW,
};

const MUTEX_NAME: &str = "Local\\Spectrum.SingleInstance";

#[derive(Debug, PartialEq)]
enum Command {
    /// No switches: run with the main window open.
    Default,
    /// `--tray`: run in the tray only.
    Tray,
    /// `--cleanup`: remove Spectrum's associations, profiles, Startup shortcut and data (the uninstaller).
    Cleanup,
    /// `--quit`: ask a running instance to exit.
    Quit,
    DebugDump {
        json: bool,
    },
    DebugEdid(u32),
    DebugWipe(u32),
    DebugReconcile,
    DebugOpen(String),
    DebugTrayCmd(u32),
    DebugDemo,
    DebugStage(String),
    DebugSdr(String, String),
    DebugHdr(String, String),
    DebugHdrReset(String),
    DebugHdrToggle(String, String),
    DebugAutostart(String),
    DebugDxgi(String),
    DebugDeleteProfile(String),
    DebugSdrWhite(String, Option<String>),
    DebugSdrCap(String, String),
}

#[derive(Debug)]
struct Args {
    command: Command,
    /// `--theme dark|light`: forces the theme (screenshots only).
    dark: Option<bool>,
    /// `--debug-scale <f>`: UI scale = f x the real DPI scale (screenshots only).
    scale: Option<f64>,
    /// `--out <file>`: debug output target (console otherwise).
    out: Option<PathBuf>,
}

fn parse_args<I: Iterator<Item = String>>(it: I) -> Args {
    let mut it = it.peekable();
    let mut args = Args {
        command: Command::Default,
        dark: None,
        scale: None,
        out: None,
    };
    let mut json = false;
    let mut dump = false;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--tray" => args.command = Command::Tray,
            "--cleanup" => args.command = Command::Cleanup,
            "--quit" => args.command = Command::Quit,
            "--theme" => args.dark = it.next().map(|v| v.eq_ignore_ascii_case("dark")),
            "--out" => args.out = it.next().map(PathBuf::from),
            "--debug-scale" => args.scale = it.next().and_then(|v| v.parse().ok()),
            "--json" => json = true,
            "--debug-dump" => dump = true,
            "--debug-edid" => args.command = Command::DebugEdid(num(it.next())),
            "--debug-wipe" => args.command = Command::DebugWipe(num(it.next())),
            "--debug-reconcile" => args.command = Command::DebugReconcile,
            "--debug-open" => args.command = Command::DebugOpen(it.next().unwrap_or_default()),
            "--debug-tray-cmd" => args.command = Command::DebugTrayCmd(num(it.next())),
            "--debug-demo" => args.command = Command::DebugDemo,
            "--debug-stage" => args.command = Command::DebugStage(it.next().unwrap_or_default()),
            "--debug-sdr" => {
                args.command = Command::DebugSdr(it.next().unwrap_or_default(), it.next().unwrap_or_default())
            }
            "--debug-hdr" => {
                args.command = Command::DebugHdr(it.next().unwrap_or_default(), it.next().unwrap_or_default())
            }
            "--debug-hdr-reset" => args.command = Command::DebugHdrReset(it.next().unwrap_or_default()),
            "--debug-hdr-toggle" => {
                args.command =
                    Command::DebugHdrToggle(it.next().unwrap_or_default(), it.next().unwrap_or_default())
            }
            "--debug-dxgi" => args.command = Command::DebugDxgi(it.next().unwrap_or_default()),
            "--debug-autostart" => args.command = Command::DebugAutostart(it.next().unwrap_or_default()),
            "--debug-sdr-white" => {
                let n = it.next().unwrap_or_default();
                let v = it.next_if(|v| !v.starts_with("--"));
                args.command = Command::DebugSdrWhite(n, v)
            }
            "--debug-sdr-cap" => {
                args.command =
                    Command::DebugSdrCap(it.next().unwrap_or_default(), it.next().unwrap_or_default())
            }
            "--debug-delete-profile" => {
                args.command = Command::DebugDeleteProfile(it.next().unwrap_or_default())
            }
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

fn run_interactive() -> i32 {
    app::run(RunOpts {
        tray: true,
        show_main: true,
        command: None,
    })
}

/// No switches: open the main window, or the one of the running instance.
fn run_default() -> i32 {
    if acquire_instance().is_none() {
        show_running();
        return 0;
    }
    run_interactive()
}

fn run_tray() -> i32 {
    if acquire_instance().is_none() {
        return 0;
    }
    app::run_tray()
}

/// `--cleanup`: the uninstaller's step. Stops a running copy first so it can't re-associate anything, then
/// removes associations and profiles (SDR and HDR), the Startup shortcut and `%APPDATA%\Spectrum`.
fn cleanup() -> i32 {
    debug::stop_running(5000);
    app::remove_all_profiles();
    let _ = autostart::set_autostart(false);
    0
}

/// `--quit`: asks a running instance to exit and waits up to 5 s.
fn quit() -> i32 {
    debug::stop_running(5000);
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
    if let Some(f) = args.scale {
        draw::set_ui_scale(f);
    }
    let out = args.out.as_deref();
    let code = match args.command {
        Command::Default => run_default(),
        Command::Tray => run_tray(),
        Command::Cleanup => cleanup(),
        Command::Quit => quit(),
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
        Command::DebugSdr(n, idx) => debug::set_sdr(&n, &idx, out),
        Command::DebugHdr(n, v) => debug::apply_hdr(&n, &v, out),
        Command::DebugHdrReset(n) => debug::reset_hdr(&n, out),
        Command::DebugHdrToggle(n, v) => debug::toggle_hdr(&n, &v, out),
        Command::DebugDxgi(n) => debug::dxgi(&n, out),
        Command::DebugAutostart(v) => debug::autostart(&v, out),
        Command::DebugDeleteProfile(f) => debug::delete_profile(&f, out),
        Command::DebugSdrWhite(n, v) => debug::sdr_white(&n, v.as_deref(), out),
        Command::DebugSdrCap(n, v) => debug::sdr_cap(&n, &v, out),
        Command::DebugOpen(what) => debug_open(format!("open {what}")),
        Command::DebugTrayCmd(id) => debug_open(format!("tray-cmd {id}")),
        Command::DebugStage(t) => debug::stage(&t),
        Command::DebugDemo => {
            debug::set_demo_out(out.map(|p| p.to_path_buf()));
            app::run(RunOpts {
                tray: false,
                show_main: false,
                command: Some("demo".into()),
            })
        }
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(a: &[&str]) -> Args {
        parse_args(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn plain_launch_runs_the_app() {
        assert_eq!(parse(&[]).command, Command::Default);
        assert_eq!(parse(&["--tray"]).command, Command::Tray);
    }

    #[test]
    fn cleanup_and_quit_switches() {
        assert_eq!(parse(&["--cleanup"]).command, Command::Cleanup);
        assert_eq!(parse(&["--quit"]).command, Command::Quit);
    }

    #[test]
    fn removed_install_switches_are_ignored() {
        for sw in [
            "--uninstall",
            "--finish-uninstall",
            "--debug-install",
            "--install",
        ] {
            assert_eq!(parse(&[sw]).command, Command::Default, "{sw}");
        }
    }

    #[test]
    fn debug_dump_wins_and_keeps_json() {
        assert_eq!(
            parse(&["--cleanup", "--debug-dump", "--json"]).command,
            Command::DebugDump { json: true }
        );
    }
}
