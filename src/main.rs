#![windows_subsystem = "windows"]
//! Thin CLI shell: parses switches and hands off to the library.

use spectrum::{app, installer, monitors, ui};
use std::path::PathBuf;

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
    DebugOpen(String),
    DebugTrayCmd(u32),
}

#[derive(Debug)]
struct Args {
    command: Command,
    /// `--theme dark|light`
    dark: Option<bool>,
    /// `--out <file>`: debug output target (console otherwise).
    #[allow(dead_code)]
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
            "--debug-open" => args.command = Command::DebugOpen(it.next().unwrap_or_default()),
            "--debug-tray-cmd" => args.command = Command::DebugTrayCmd(num(it.next())),
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

fn main() {
    let args = parse_args(std::env::args().skip(1));
    ui::draw::set_forced_theme(args.dark);
    let code = match args.command {
        Command::Default | Command::Tray => app::run_tray(),
        Command::Uninstall => match installer::uninstall() {
            Ok(()) => 0,
            Err(_) => 1,
        },
        Command::DebugDump { .. } => {
            let _ = monitors::enumerate();
            0
        }
        Command::DebugEdid(_) | Command::DebugWipe(_) | Command::DebugTrayCmd(_) => 0,
        Command::DebugOpen(what) => {
            match what.as_str() {
                "main" => ui::main_window::open(),
                "gamut" => ui::gamut_window::open(None),
                _ => {}
            }
            0
        }
    };
    std::process::exit(code);
}
