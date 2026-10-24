//! Headless UI actions: the same dispatch path as the windows, with a before/after report.

use super::dump::{install_text, sdr_option_lines};
use super::{emit, fail, hdr_text, on_off, opt, parse_hdr_params, parse_num, parse_on_off};
use crate::app::{self, UiAction};
use crate::config::Config;
use crate::monitors;
use crate::wcs;
use std::fmt::Write as _;
use std::path::Path;

/// Appends "result: ok[: <report>]" or "result: error: <message>" and returns the exit code.
fn push_result(text: &mut String, result: Result<String, String>) -> i32 {
    match result {
        Ok(m) if m.is_empty() => {
            text.push_str("result: ok\n");
            0
        }
        Ok(m) => {
            let _ = writeln!(text, "result: ok: {m}");
            0
        }
        Err(e) => {
            let _ = writeln!(text, "result: error: {e}");
            1
        }
    }
}

/// `--debug-edid <num>`: the "Read from monitor" flow, headless.
pub fn read_edid(num: u32, out: Option<&Path>) {
    let result = app::execute(UiAction::ReadEdid {
        mon: num.saturating_sub(1) as usize,
    });
    let text = match result {
        Ok(m) => format!("ok: {m}\n"),
        Err(e) => format!("error: {e}\n"),
    };
    emit(out, &text);
}

/// `--debug-wipe <num>`
pub fn wipe(num: u32, out: Option<&Path>) {
    let text = match app::wipe_monitor(num) {
        Ok(lines) if lines.is_empty() => "nothing of Spectrum's was associated\n".to_string(),
        Ok(lines) => lines.join("\n") + "\n",
        Err(e) => format!("error: {e}\n"),
    };
    emit(out, &text);
}

/// `--debug-reconcile`
pub fn reconcile(out: Option<&Path>) {
    let lines = app::reconcile();
    let text = if lines.is_empty() {
        "reconcile: nothing to do\n".to_string()
    } else {
        lines.join("\n") + "\n"
    };
    emit(out, &text);
}

/// Parses the monitor number and finds its position in the UI list (what the `mon` field of a `UiAction`
/// indexes).
fn target(num: &str) -> Result<(u32, usize), String> {
    let n = parse_num(num)?;
    let idx = app::snapshot()
        .monitors
        .iter()
        .position(|m| m.num == n)
        .ok_or_else(|| format!("no monitor {n}"))?;
    Ok((n, idx))
}

/// State of one monitor after an action: SDR options, Windows defaults, HDR state and our config.
fn monitor_state(num: u32) -> String {
    let ui = app::snapshot();
    let Some(u) = ui.monitors.iter().find(|m| m.num == num) else {
        return format!("monitor {num}: not found\n");
    };
    let mut s = String::new();
    let _ = writeln!(s, "monitor {num}: {}", u.name);
    let _ = writeln!(s, "  sdr options:");
    s.push_str(&sdr_option_lines(u, "    "));
    let cfg = Config::load();
    if let Some(m) = app::current_monitors().iter().find(|m| m.num == num) {
        let _ = writeln!(
            s,
            "  sdr default:    {}",
            opt(wcs::default_profile(m, false).as_deref())
        );
        let _ = writeln!(
            s,
            "  hdr default:    {}",
            opt(wcs::default_profile(m, true).as_deref())
        );
        let _ = writeln!(s, "  hdr associated: {:?}", wcs::associated(m, true));
        let _ = writeln!(
            s,
            "  hdr:            supported={} enabled={}",
            m.hdr_supported, m.hdr_enabled
        );
        match cfg.monitors.get(&m.key) {
            Some(c) => {
                let _ = writeln!(
                    s,
                    "  config:         sdr={} hdr={} hdr_file={}",
                    opt(c.sdr.as_deref()),
                    hdr_text(c.hdr),
                    opt(c.hdr_file.as_deref())
                );
            }
            None => {
                let _ = writeln!(s, "  config:         (none)");
            }
        }
    }
    let _ = writeln!(s, "  hdr custom active: {}", u.hdr_custom_active);
    s
}

/// Runs one action through `app::dispatch_result` and reports it. Exit code 0 on success, 1 on failure.
fn run_action(what: &str, num: u32, action: UiAction, out: Option<&Path>) -> i32 {
    let mut text = format!("action: {what}\n");
    let code = push_result(&mut text, app::dispatch_result(action));
    text.push_str(&monitor_state(num));
    emit(out, &text);
    code
}

/// Runs the action `prepare` builds from the parsed arguments, or reports why they don't parse.
fn run_parsed(out: Option<&Path>, prepare: impl FnOnce() -> Result<(String, u32, UiAction), String>) -> i32 {
    match prepare() {
        Ok((what, num, action)) => run_action(&what, num, action, out),
        Err(e) => fail(out, &e),
    }
}

/// `--debug-sdr-cap <num> on|off`: the "Allow clipping above the monitor peak brightness" toggle, through the
/// same action the window sends (turning it off snaps SDR content down to the cap).
pub fn sdr_cap(num: &str, state: &str, out: Option<&Path>) -> i32 {
    run_parsed(out, || {
        let on = parse_on_off(state)?;
        let (num, mon) = target(num)?;
        Ok((
            format!("allow sdr content above the peak {} on monitor {num}", on_off(on)),
            num,
            UiAction::SetSdrAbovePeak { mon, on },
        ))
    })
}

/// `--debug-sdr <num> <option-index>`
pub fn set_sdr(num: &str, idx: &str, out: Option<&Path>) -> i32 {
    run_parsed(out, || {
        let idx = idx
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("invalid option index {idx:?}"))?;
        let (num, mon) = target(num)?;
        let label = app::snapshot()
            .monitors
            .get(mon)
            .and_then(|m| m.sdr_options.get(idx))
            .map(|(l, f)| format!("{l} ({f})"))
            .unwrap_or_else(|| "no such option".into());
        Ok((
            format!("set sdr on monitor {num}, option {idx}: {label}"),
            num,
            UiAction::SetSdr { mon, idx },
        ))
    })
}

/// `--debug-hdr <num> <min>,<peak>,<full>,<sat>`
pub fn apply_hdr(num: &str, params: &str, out: Option<&Path>) -> i32 {
    run_parsed(out, || {
        let params = parse_hdr_params(params)?;
        let (num, mon) = target(num)?;
        Ok((
            format!("apply hdr on monitor {num}: {}", hdr_text(Some(params))),
            num,
            UiAction::ApplyHdr { mon, params },
        ))
    })
}

/// `--debug-hdr-reset <num>`
pub fn reset_hdr(num: &str, out: Option<&Path>) -> i32 {
    run_parsed(out, || {
        let (num, mon) = target(num)?;
        Ok((
            format!("reset hdr on monitor {num}"),
            num,
            UiAction::ResetHdr { mon },
        ))
    })
}

/// `--debug-hdr-toggle <num> on|off`
pub fn toggle_hdr(num: &str, state: &str, out: Option<&Path>) -> i32 {
    run_parsed(out, || {
        let on = parse_on_off(state)?;
        let (num, mon) = target(num)?;
        Ok((
            format!("turn hdr {} on monitor {num}", on_off(on)),
            num,
            UiAction::SetHdrEnabled { mon, on },
        ))
    })
}

/// Where a profile file shows up: the color folder and every monitor's SDR and HDR association lists.
fn profile_presence(file: &str) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "  file in color folder: {}", wcs::profile_exists(file));
    let _ = writeln!(
        s,
        "  backup copy:          {}",
        wcs::backup_dir().join(file).is_file()
    );
    for m in &app::current_monitors() {
        for hdr in [false, true] {
            let listed = wcs::associated(m, hdr)
                .map(|l| l.iter().any(|f| f.eq_ignore_ascii_case(file)))
                .map_or("unknown".to_string(), |b| b.to_string());
            let _ = writeln!(
                s,
                "  monitor {} {} associated: {listed}",
                m.num,
                if hdr { "HDR" } else { "SDR" }
            );
        }
    }
    s
}

/// `--debug-sdr-white <num> [nits]`: reads Windows' "SDR content brightness" and, with `nits`, sets it
/// (exactly as given: no rounding, so off-grid values can be probed) and reads it back.
pub fn sdr_white(num: &str, nits: Option<&str>, out: Option<&Path>) -> i32 {
    let n = match parse_num(num) {
        Ok(n) => n,
        Err(e) => return fail(out, &e),
    };
    let mons = app::current_monitors();
    let Some(m) = mons.iter().find(|m| m.num == n) else {
        return fail(out, &format!("no monitor {n}"));
    };
    let mut text = format!("monitor {n}: hdr enabled={}\n", m.hdr_enabled);
    let show = |label: &str, text: &mut String| {
        let _ = match monitors::sdr_white_raw(m) {
            Some(raw) => writeln!(
                text,
                "{label}: raw {raw} = {} nits",
                monitors::sdr_white_raw_to_nits(raw)
            ),
            None => writeln!(text, "{label}: unavailable"),
        };
    };
    show("before", &mut text);
    let mut code = 0;
    if let Some(v) = nits {
        match v.trim().parse::<f64>() {
            Ok(v) if v.is_finite() && v > 0.0 => {
                let raw = monitors::sdr_white_nits_to_raw(v);
                let _ = writeln!(text, "set: {v} nits -> raw {raw}");
                code = push_result(
                    &mut text,
                    monitors::set_sdr_white_raw(m, raw).map(|_| String::new()),
                );
                show("after", &mut text);
            }
            _ => {
                let _ = writeln!(text, "error: invalid nits {v:?}");
                code = 1;
            }
        }
    }
    emit(out, &text);
    code
}

/// `--debug-delete-profile <file name>`: the profiles window's Delete (second click), headless.
pub fn delete_profile(file: &str, out: Option<&Path>) -> i32 {
    let mut text = format!("action: delete profile {file}\n");
    match wcs::can_delete(file) {
        Ok(()) => text.push_str("deletable: yes\n"),
        Err(why) => {
            let _ = writeln!(text, "deletable: no ({})", why.hint());
        }
    }
    text.push_str("before:\n");
    text.push_str(&profile_presence(file));
    let code = push_result(
        &mut text,
        app::dispatch_result(UiAction::DeleteProfile {
            file: file.to_string(),
        }),
    );
    text.push_str("after:\n");
    text.push_str(&profile_presence(file));
    emit(out, &text);
    code
}

/// `--debug-autostart on|off`: result line plus the Startup shortcut state.
pub fn autostart(state: &str, out: Option<&Path>) -> i32 {
    let on = match parse_on_off(state) {
        Ok(o) => o,
        Err(e) => return fail(out, &e),
    };
    let mut text = format!("action: set autostart {}\n", on_off(on));
    let code = push_result(&mut text, app::dispatch_result(UiAction::SetAutostart(on)));
    text.push_str(&install_text(crate::autostart::autostart_enabled()));
    emit(out, &text);
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_report_and_set_the_exit_code() {
        let mut text = String::new();
        assert_eq!(push_result(&mut text, Ok(String::new())), 0);
        assert_eq!(push_result(&mut text, Ok("Deleted X".into())), 0);
        assert_eq!(push_result(&mut text, Err("Can't".into())), 1);
        assert_eq!(text, "result: ok\nresult: ok: Deleted X\nresult: error: Can't\n");
    }

    #[test]
    fn bad_arguments_fail_before_any_action() {
        let dir = std::env::temp_dir().join(format!("spectrum-debug-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("out.txt");
        assert_eq!(toggle_hdr("1", "maybe", Some(&out)), 1);
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "error: expected on or off, got \"maybe\"\n"
        );
        assert_eq!(apply_hdr("1", "0,1000", Some(&out)), 1);
        assert!(std::fs::read_to_string(&out)
            .unwrap()
            .starts_with("error: expected min,peak,full,sat"));
        assert_eq!(set_sdr("0", "1", Some(&out)), 1);
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "error: invalid monitor number \"0\"\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
