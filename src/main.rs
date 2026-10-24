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
    DebugSdr(String, String),
    DebugHdr(String, String),
    DebugHdrReset(String),
    DebugHdrToggle(String, String),
    DebugAutostart(String),
    DebugDxgi(String),
    DebugDeleteProfile(String),
    DebugSdrWhite(String, Option<String>),
    DebugSdrCap(String, String),
    /// `--render <surface>`: off-screen screenshot (`spectrum::render`).
    Render(String),
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
    /// `--scale <f>` of `--render`: 1 = 96 dpi.
    render_scale: f64,
    /// `--frame` of `--render`: draw a window frame around the client area.
    frame: bool,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            command: Command::Default,
            dark: None,
            scale: None,
            out: None,
            render_scale: 1.0,
            frame: false,
        }
    }
}

type ArgIter<I> = std::iter::Peekable<I>;

/// The next argument, or an empty string when the command line ends.
fn value<I: Iterator<Item = String>>(it: &mut ArgIter<I>) -> String {
    it.next().unwrap_or_default()
}

/// The next argument as a number, 0 when missing or not a number.
fn num<I: Iterator<Item = String>>(it: &mut ArgIter<I>) -> u32 {
    it.next().and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// Applies an option switch (`--theme`, `--out`, ...) to `args`. False when `arg` is not one.
fn parse_option<I: Iterator<Item = String>>(arg: &str, it: &mut ArgIter<I>, args: &mut Args) -> bool {
    match arg {
        "--theme" => args.dark = it.next().map(|v| v.eq_ignore_ascii_case("dark")),
        "--out" => args.out = it.next().map(PathBuf::from),
        "--debug-scale" => args.scale = it.next().and_then(|v| v.parse().ok()),
        "--scale" => args.render_scale = it.next().and_then(|v| v.parse().ok()).unwrap_or(1.0),
        "--frame" => args.frame = true,
        _ => return false,
    }
    true
}

/// The command a switch selects, reading its arguments. `None` for anything else (unknown switches are
/// ignored, so a removed one falls back to the default run).
fn parse_command<I: Iterator<Item = String>>(arg: &str, it: &mut ArgIter<I>) -> Option<Command> {
    Some(match arg {
        "--tray" => Command::Tray,
        "--cleanup" => Command::Cleanup,
        "--quit" => Command::Quit,
        "--render" => Command::Render(value(it)),
        "--debug-edid" => Command::DebugEdid(num(it)),
        "--debug-wipe" => Command::DebugWipe(num(it)),
        "--debug-reconcile" => Command::DebugReconcile,
        "--debug-open" => Command::DebugOpen(value(it)),
        "--debug-tray-cmd" => Command::DebugTrayCmd(num(it)),
        "--debug-sdr" => Command::DebugSdr(value(it), value(it)),
        "--debug-hdr" => Command::DebugHdr(value(it), value(it)),
        "--debug-hdr-reset" => Command::DebugHdrReset(value(it)),
        "--debug-hdr-toggle" => Command::DebugHdrToggle(value(it), value(it)),
        "--debug-dxgi" => Command::DebugDxgi(value(it)),
        "--debug-autostart" => Command::DebugAutostart(value(it)),
        "--debug-sdr-white" => {
            let n = value(it);
            // The nits value is optional: without it the switch only reads.
            Command::DebugSdrWhite(n, it.next_if(|v| !v.starts_with("--")))
        }
        "--debug-sdr-cap" => Command::DebugSdrCap(value(it), value(it)),
        "--debug-delete-profile" => Command::DebugDeleteProfile(value(it)),
        _ => return None,
    })
}

/// The last command switch wins, except that `--debug-dump` (with `--json`) wins over all of them.
fn parse_args<I: Iterator<Item = String>>(it: I) -> Args {
    let mut it = it.peekable();
    let mut args = Args::default();
    let (mut json, mut dump) = (false, false);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--debug-dump" => dump = true,
            other => {
                if !parse_option(other, &mut it, &mut args) {
                    if let Some(c) = parse_command(other, &mut it) {
                        args.command = c;
                    }
                }
            }
        }
    }
    if dump {
        args.command = Command::DebugDump { json };
    }
    args
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
        Command::Render(s) => spectrum::render::run(
            &s,
            args.dark.unwrap_or_else(|| draw::theme().dark),
            args.render_scale,
            args.frame,
            out,
        ),
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
    fn options_parse_with_defaults() {
        let a = parse(&[]);
        assert_eq!(
            (a.dark, a.scale, a.out.clone(), a.render_scale, a.frame),
            (None, None, None, 1.0, false)
        );
        let a = parse(&[
            "--render",
            "main",
            "--theme",
            "Dark",
            "--scale",
            "1.5",
            "--frame",
            "--out",
            "x.png",
            "--debug-scale",
            "2",
        ]);
        assert_eq!(a.command, Command::Render("main".into()));
        assert_eq!(a.dark, Some(true));
        assert_eq!((a.render_scale, a.frame, a.scale), (1.5, true, Some(2.0)));
        assert_eq!(a.out, Some(PathBuf::from("x.png")));
        let a = parse(&["--theme", "light", "--scale", "big", "--debug-scale", "x"]);
        assert_eq!((a.dark, a.render_scale, a.scale), (Some(false), 1.0, None));
    }

    #[test]
    fn debug_switches_take_their_arguments() {
        let s = |v: &str| v.to_string();
        let cases: Vec<(Vec<&str>, Command)> = vec![
            (vec!["--debug-edid", "2"], Command::DebugEdid(2)),
            (vec!["--debug-wipe", "x"], Command::DebugWipe(0)),
            (vec!["--debug-reconcile"], Command::DebugReconcile),
            (vec!["--debug-open", "gamut"], Command::DebugOpen(s("gamut"))),
            (vec!["--debug-tray-cmd", "1003"], Command::DebugTrayCmd(1003)),
            (vec!["--debug-sdr", "1", "3"], Command::DebugSdr(s("1"), s("3"))),
            (
                vec!["--debug-hdr", "1", "0,1000,400,0"],
                Command::DebugHdr(s("1"), s("0,1000,400,0")),
            ),
            (vec!["--debug-hdr-reset", "1"], Command::DebugHdrReset(s("1"))),
            (
                vec!["--debug-hdr-toggle", "1", "on"],
                Command::DebugHdrToggle(s("1"), s("on")),
            ),
            (vec!["--debug-dxgi", "1"], Command::DebugDxgi(s("1"))),
            (
                vec!["--debug-autostart", "off"],
                Command::DebugAutostart(s("off")),
            ),
            (
                vec!["--debug-sdr-cap", "1", "on"],
                Command::DebugSdrCap(s("1"), s("on")),
            ),
            (
                vec!["--debug-delete-profile", "a b.icm"],
                Command::DebugDeleteProfile(s("a b.icm")),
            ),
            (
                vec!["--debug-sdr"],
                Command::DebugSdr(String::new(), String::new()),
            ),
        ];
        for (argv, want) in cases {
            assert_eq!(parse(&argv).command, want, "{argv:?}");
        }
    }

    #[test]
    fn sdr_white_value_is_optional() {
        assert_eq!(
            parse(&["--debug-sdr-white", "1"]).command,
            Command::DebugSdrWhite("1".into(), None)
        );
        let a = parse(&["--debug-sdr-white", "1", "--out", "f.txt"]);
        assert_eq!(a.command, Command::DebugSdrWhite("1".into(), None));
        assert_eq!(a.out, Some(PathBuf::from("f.txt")));
        assert_eq!(
            parse(&["--debug-sdr-white", "1", "240"]).command,
            Command::DebugSdrWhite("1".into(), Some("240".into()))
        );
    }

    #[test]
    fn the_last_command_wins() {
        assert_eq!(parse(&["--tray", "--quit"]).command, Command::Quit);
        assert_eq!(
            parse(&["--json", "--debug-dump"]).command,
            Command::DebugDump { json: true }
        );
        assert_eq!(
            parse(&["--debug-dump"]).command,
            Command::DebugDump { json: false }
        );
    }

    #[test]
    fn debug_dump_wins_and_keeps_json() {
        assert_eq!(
            parse(&["--cleanup", "--debug-dump", "--json"]).command,
            Command::DebugDump { json: true }
        );
    }
}
