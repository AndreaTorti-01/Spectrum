//! Developer switches: state dumps, headless flows, the scripted README demo. Output goes to `--out <file>`
//! when given, otherwise to the parent console (the exe is a GUI-subsystem program).

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use crate::app::{self, UiAction, UiMonitor, UiState};
use crate::autostart;
use crate::color::Primaries;
use crate::config::Config;
use crate::icc_write::HdrParams;
use crate::monitors::Monitor;
use crate::ui;
use crate::util::wide;
use crate::wcs;
use std::cell::Cell;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::time::Instant;
use windows_sys::Win32::Foundation::{HWND, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Console::{
    AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EndMenu, FindWindowW, PostMessageW, SendMessageW, SetTimer, WM_CLOSE, WM_COPYDATA,
};

// ---------------------------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------------------------

/// Writes `text` to the `--out` file, or to stdout (attaching to the parent console when the process
/// has no usable standard output).
pub fn emit(out: Option<&Path>, text: &str) {
    if let Some(p) = out {
        if let Err(e) = std::fs::write(p, text) {
            crate::util::log(&format!("debug: cannot write {}: {e}", p.display()));
        }
        return;
    }
    // SAFETY: plain handle query and console attach.
    let have_stdout = unsafe {
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        !h.is_null() && h != INVALID_HANDLE_VALUE
    };
    if have_stdout {
        let mut so = std::io::stdout();
        let _ = so.write_all(text.as_bytes());
        let _ = so.flush();
        return;
    }
    // SAFETY: attaching is harmless when there is no parent console.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
        let _ = f.write_all(text.as_bytes());
    }
}

// ---------------------------------------------------------------------------------------------
// Dump
// ---------------------------------------------------------------------------------------------

fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn json_num(v: f64) -> String {
    if v.is_finite() {
        format!("{v}")
    } else {
        "null".into()
    }
}

fn json_opt(s: &Option<String>) -> String {
    s.as_deref().map(json_str).unwrap_or_else(|| "null".into())
}

fn json_list(v: &[String]) -> String {
    format!(
        "[{}]",
        v.iter().map(|s| json_str(s)).collect::<Vec<_>>().join(",")
    )
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn prims_json(p: &Primaries) -> String {
    let xy =
        |n: &str, v: crate::color::Xy| format!("\"{n}\":{{\"x\":{},\"y\":{}}}", json_num(v.x), json_num(v.y));
    format!(
        "{{{},{},{},{}}}",
        xy("r", p.r),
        xy("g", p.g),
        xy("b", p.b),
        xy("w", p.w)
    )
}

struct Info {
    sdr_default: Option<String>,
    hdr_default: Option<String>,
    sdr_assoc: Result<Vec<String>, String>,
    hdr_assoc: Result<Vec<String>, String>,
    user_scope: Option<bool>,
}

fn info(m: &Monitor) -> Info {
    Info {
        sdr_default: wcs::default_profile(m, false),
        hdr_default: wcs::default_profile(m, true),
        sdr_assoc: wcs::associated(m, false),
        hdr_assoc: wcs::associated(m, true),
        user_scope: wcs::is_user_scope(m),
    }
}

fn name_of(m: &Monitor) -> String {
    m.edid
        .as_ref()
        .map(|e| e.display_name())
        .unwrap_or_else(|| m.friendly_name.clone())
}

fn opt(s: &Option<String>) -> &str {
    s.as_deref().unwrap_or("(none)")
}

fn hdr_text(h: &Option<HdrParams>) -> String {
    match h {
        Some(h) => format!(
            "min {} peak {} full {} sat {}",
            h.min_nits, h.peak_nits, h.full_frame_nits, h.saturation
        ),
        None => "(none)".into(),
    }
}

/// Autostart state: whether the Startup shortcut exists, and where it is.
fn install_text(autostart: bool) -> String {
    let lnk = autostart::shortcut_path();
    let mut s = String::new();
    let _ = writeln!(s, "autostart:       {}", if autostart { "on" } else { "off" });
    let _ = writeln!(s, "startup shortcut: {} (exists={})", lnk.display(), lnk.exists());
    s
}

fn sdr_option_lines(m: &UiMonitor, indent: &str) -> String {
    let mut s = String::new();
    for (i, (label, file)) in m.sdr_options.iter().enumerate() {
        let mark = if m.sdr_current == Some(i) {
            "  <- current"
        } else {
            ""
        };
        let _ = writeln!(s, "{indent}[{i}] {label} ({file}){mark}");
    }
    s
}

fn dump_text(mons: &[Monitor], cfg: &Config, ui: &UiState) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "Spectrum {} debug dump", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(s, "color folder: {}", wcs::color_dir().display());
    let cpath = crate::config::config_path();
    let _ = writeln!(
        s,
        "\nconfig file:     {} (exists={})",
        cpath.display(),
        cpath.exists()
    );
    let _ = writeln!(s, "config load failed: {}", cfg.load_failed);
    let _ = writeln!(s, "config monitors: {}", cfg.monitors.len());
    for (key, c) in &cfg.monitors {
        let _ = writeln!(
            s,
            "  [monitor:{key}] name={} edid_profile={} sdr={} hdr={} hdr_file={}",
            c.name,
            opt(&c.edid_profile),
            opt(&c.sdr),
            hdr_text(&c.hdr),
            opt(&c.hdr_file)
        );
    }
    let _ = writeln!(s, "\n{}", install_text(ui.autostart).trim_end());
    let _ = writeln!(s, "\nmonitors: {}", mons.len());
    for m in mons {
        let i = info(m);
        let _ = writeln!(s, "\nmonitor {}: {}", m.num, name_of(m));
        let _ = writeln!(s, "  key:            {}", m.key);
        let _ = writeln!(s, "  gdi:            {}", m.gdi_name);
        let _ = writeln!(
            s,
            "  rect:           {},{} to {},{} ({} x {})",
            m.rect.left,
            m.rect.top,
            m.rect.right,
            m.rect.bottom,
            m.rect.right - m.rect.left,
            m.rect.bottom - m.rect.top
        );
        let _ = writeln!(s, "  target/source:  {} / {}", m.target_id, m.source_id);
        let _ = writeln!(
            s,
            "  hdr:            supported={} enabled={}",
            m.hdr_supported, m.hdr_enabled
        );
        let _ = writeln!(s, "  user scope:     {:?}", i.user_scope);
        let _ = writeln!(
            s,
            "  sdr default:    {}",
            i.sdr_default.as_deref().unwrap_or("(none)")
        );
        let _ = writeln!(s, "  sdr associated: {:?}", i.sdr_assoc);
        let _ = writeln!(
            s,
            "  hdr default:    {}",
            i.hdr_default.as_deref().unwrap_or("(none)")
        );
        let _ = writeln!(s, "  hdr associated: {:?}", i.hdr_assoc);
        match ui.monitors.iter().find(|u| u.num == m.num) {
            Some(u) => {
                let _ = writeln!(s, "  sdr options:");
                s.push_str(&sdr_option_lines(u, "    "));
                let _ = writeln!(
                    s,
                    "  hdr sliders:    {}  (custom active={})",
                    hdr_text(&Some(u.hdr)),
                    u.hdr_custom_active
                );
            }
            None => {
                let _ = writeln!(s, "  sdr options:    (none)");
            }
        }
        match &m.edid {
            Some(e) => {
                let p = &e.primaries;
                let _ = writeln!(
                    s,
                    "  edid:           {} {:04X} serial {:08X} year {} gamma {:?}",
                    e.manufacturer, e.product_code, e.serial, e.year, e.gamma
                );
                let _ = writeln!(
                    s,
                    "  primaries:      R {:.4},{:.4}  G {:.4},{:.4}  B {:.4},{:.4}  W {:.4},{:.4}",
                    p.r.x, p.r.y, p.g.x, p.g.y, p.b.x, p.b.y, p.w.x, p.w.y
                );
                if let Some(h) = &e.hdr {
                    let _ = writeln!(s, "  edid hdr:       {h:?}");
                }
            }
            None => {
                let _ = writeln!(s, "  edid:           (none)");
            }
        }
        match cfg.monitors.get(&m.key) {
            Some(c) => {
                let _ = writeln!(s, "  config:         {c:?}");
            }
            None => {
                let _ = writeln!(s, "  config:         (none)");
            }
        }
    }
    s
}

fn dump_json(mons: &[Monitor], cfg: &Config, ui: &UiState) -> String {
    let mut items = Vec::new();
    for m in mons {
        let i = info(m);
        let mut o = String::new();
        let _ = write!(
            o,
            "{{\"num\":{},\"name\":{},\"key\":{},\"gdi\":{},\"rect\":{{\"left\":{},\"top\":{},\"right\":{},\"bottom\":{}}},",
            m.num,
            json_str(&name_of(m)),
            json_str(&m.key),
            json_str(&m.gdi_name),
            m.rect.left,
            m.rect.top,
            m.rect.right,
            m.rect.bottom
        );
        let _ = write!(
            o,
            "\"hdr_supported\":{},\"hdr_enabled\":{},\"user_scope\":{},",
            m.hdr_supported,
            m.hdr_enabled,
            i.user_scope
                .map(|b| b.to_string())
                .unwrap_or_else(|| "null".into())
        );
        let _ = write!(
            o,
            "\"sdr_default\":{},\"hdr_default\":{},\"sdr_associated\":{},\"hdr_associated\":{},",
            json_opt(&i.sdr_default),
            json_opt(&i.hdr_default),
            json_list(i.sdr_assoc.as_deref().unwrap_or_default()),
            json_list(i.hdr_assoc.as_deref().unwrap_or_default())
        );
        let opts: Vec<String> = ui
            .monitors
            .iter()
            .find(|u| u.num == m.num)
            .map(|u| {
                u.sdr_options
                    .iter()
                    .enumerate()
                    .map(|(i, (label, file))| {
                        format!(
                            "{{\"index\":{i},\"label\":{},\"file\":{},\"current\":{}}}",
                            json_str(label),
                            json_str(file),
                            u.sdr_current == Some(i)
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let _ = write!(o, "\"sdr_options\":[{}],", opts.join(","));
        match &m.edid {
            Some(e) => {
                let hdr = match &e.hdr {
                    Some(h) => format!(
                        "{{\"max_nits\":{},\"max_frame_avg_nits\":{},\"min_nits\":{},\"pq\":{}}}",
                        h.max_nits.map(json_num).unwrap_or_else(|| "null".into()),
                        h.max_frame_avg_nits
                            .map(json_num)
                            .unwrap_or_else(|| "null".into()),
                        h.min_nits.map(json_num).unwrap_or_else(|| "null".into()),
                        h.pq
                    ),
                    None => "null".into(),
                };
                let _ = write!(
                    o,
                    "\"edid\":{{\"hex\":{},\"manufacturer\":{},\"product_code\":{},\"serial\":{},\"year\":{},\"gamma\":{},\"srgb_default\":{},\"primaries\":{},\"hdr\":{}}},",
                    json_str(&hex(&e.raw)),
                    json_str(&e.manufacturer),
                    e.product_code,
                    e.serial,
                    e.year,
                    e.gamma.map(json_num).unwrap_or_else(|| "null".into()),
                    e.srgb_default,
                    prims_json(&e.primaries),
                    hdr
                );
            }
            None => o.push_str("\"edid\":null,"),
        }
        match cfg.monitors.get(&m.key) {
            Some(c) => {
                let hdr = match c.hdr {
                    Some(h) => format!(
                        "{{\"min_nits\":{},\"peak_nits\":{},\"full_frame_nits\":{},\"saturation\":{}}}",
                        json_num(h.min_nits),
                        json_num(h.peak_nits),
                        json_num(h.full_frame_nits),
                        json_num(h.saturation)
                    ),
                    None => "null".into(),
                };
                let _ = write!(
                    o,
                    "\"config\":{{\"name\":{},\"edid_profile\":{},\"sdr\":{},\"hdr\":{},\"hdr_file\":{}}}",
                    json_str(&c.name),
                    json_opt(&c.edid_profile),
                    json_opt(&c.sdr),
                    hdr,
                    json_opt(&c.hdr_file)
                );
            }
            None => o.push_str("\"config\":null"),
        }
        o.push('}');
        items.push(o);
    }
    let lnk = autostart::shortcut_path();
    let install = format!(
        "{{\"startup_shortcut\":{},\"startup_shortcut_exists\":{}}}",
        json_str(&lnk.display().to_string()),
        lnk.exists()
    );
    let cpath = crate::config::config_path();
    format!(
        "{{\"autostart\":{},\"install\":{},\"config_file\":{},\"config_exists\":{},\"monitors\":[{}]}}\n",
        ui.autostart,
        install,
        json_str(&cpath.display().to_string()),
        cpath.exists(),
        items.join(",\n")
    )
}

/// `--debug-dump [--json]`
pub fn dump(json: bool, out: Option<&Path>) {
    let mons = crate::monitors::enumerate();
    let cfg = Config::load();
    let ui = app::snapshot();
    let text = if json {
        dump_json(&mons, &cfg, &ui)
    } else {
        dump_text(&mons, &cfg, &ui)
    };
    emit(out, &text);
}

/// `--debug-edid <num>`: the "Read profile from monitor" flow, headless.
pub fn read_edid(num: u32, out: Option<&Path>) {
    let r = app::execute(UiAction::ReadEdid {
        mon: num.saturating_sub(1) as usize,
    });
    let text = match r {
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

// ---------------------------------------------------------------------------------------------
// Headless UI actions (same dispatch path as the windows)
// ---------------------------------------------------------------------------------------------

fn parse_num(s: &str) -> Result<u32, String> {
    match s.trim().parse::<u32>() {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(format!("invalid monitor number {s:?}")),
    }
}

fn parse_on_off(s: &str) -> Result<bool, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("expected on or off, got {s:?}")),
    }
}

/// Parses `min,peak,full,sat` (sat 0..1).
fn parse_hdr_params(s: &str) -> Result<HdrParams, String> {
    let bad = || format!("expected min,peak,full,sat as numbers, got {s:?}");
    let nums: Vec<f64> = s
        .split(',')
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| bad())?;
    if nums.len() != 4 || nums.iter().any(|n| !n.is_finite()) {
        return Err(bad());
    }
    if !(0.0..=1.0).contains(&nums[3]) {
        return Err(format!("saturation must be between 0 and 1, got {}", nums[3]));
    }
    Ok(HdrParams {
        min_nits: nums[0],
        peak_nits: nums[1],
        full_frame_nits: nums[2],
        saturation: nums[3],
    })
}

/// Position of monitor `num` in the UI list (what the `mon` field of a `UiAction` indexes).
fn ui_index(num: u32) -> Result<usize, String> {
    app::snapshot()
        .monitors
        .iter()
        .position(|m| m.num == num)
        .ok_or_else(|| format!("no monitor {num}"))
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
        let _ = writeln!(s, "  sdr default:    {}", opt(&wcs::default_profile(m, false)));
        let _ = writeln!(s, "  hdr default:    {}", opt(&wcs::default_profile(m, true)));
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
                    opt(&c.sdr),
                    hdr_text(&c.hdr),
                    opt(&c.hdr_file)
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
    let code = match app::dispatch_result(action) {
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
    };
    text.push_str(&monitor_state(num));
    emit(out, &text);
    code
}

fn fail(out: Option<&Path>, msg: &str) -> i32 {
    emit(out, &format!("error: {msg}\n"));
    1
}

/// Parses the monitor number and finds its UI index.
fn target(num: &str) -> Result<(u32, usize), String> {
    let n = parse_num(num)?;
    Ok((n, ui_index(n)?))
}

/// `--debug-sdr <num> <option-index>`
pub fn set_sdr(num: &str, idx: &str, out: Option<&Path>) -> i32 {
    let Ok(idx) = idx.trim().parse::<usize>() else {
        return fail(out, &format!("invalid option index {idx:?}"));
    };
    let (num, mon) = match target(num) {
        Ok(t) => t,
        Err(e) => return fail(out, &e),
    };
    let label = app::snapshot()
        .monitors
        .get(mon)
        .and_then(|m| m.sdr_options.get(idx))
        .map(|(l, f)| format!("{l} ({f})"))
        .unwrap_or_else(|| "no such option".into());
    run_action(
        &format!("set sdr on monitor {num}, option {idx}: {label}"),
        num,
        UiAction::SetSdr { mon, idx },
        out,
    )
}

/// `--debug-hdr <num> <min>,<peak>,<full>,<sat>`
pub fn apply_hdr(num: &str, params: &str, out: Option<&Path>) -> i32 {
    let p = match parse_hdr_params(params) {
        Ok(p) => p,
        Err(e) => return fail(out, &e),
    };
    let (num, mon) = match target(num) {
        Ok(t) => t,
        Err(e) => return fail(out, &e),
    };
    run_action(
        &format!("apply hdr on monitor {num}: {}", hdr_text(&Some(p))),
        num,
        UiAction::ApplyHdr { mon, params: p },
        out,
    )
}

/// `--debug-hdr-reset <num>`
pub fn reset_hdr(num: &str, out: Option<&Path>) -> i32 {
    let (num, mon) = match target(num) {
        Ok(t) => t,
        Err(e) => return fail(out, &e),
    };
    run_action(
        &format!("reset hdr on monitor {num}"),
        num,
        UiAction::ResetHdr { mon },
        out,
    )
}

/// `--debug-hdr-toggle <num> on|off`
pub fn toggle_hdr(num: &str, state: &str, out: Option<&Path>) -> i32 {
    let on = match parse_on_off(state) {
        Ok(o) => o,
        Err(e) => return fail(out, &e),
    };
    let (num, mon) = match target(num) {
        Ok(t) => t,
        Err(e) => return fail(out, &e),
    };
    run_action(
        &format!("turn hdr {} on monitor {num}", if on { "on" } else { "off" }),
        num,
        UiAction::SetHdrEnabled { mon, on },
        out,
    )
}

/// Result line plus autostart state, for the switch that touches the Startup shortcut.
fn finish_install(mut text: String, r: Result<String, String>, out: Option<&Path>) -> i32 {
    let code = match r {
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
    };
    text.push_str(&install_text(autostart::autostart_enabled()));
    emit(out, &text);
    code
}

/// `--debug-autostart on|off`
pub fn autostart(state: &str, out: Option<&Path>) -> i32 {
    let on = match parse_on_off(state) {
        Ok(o) => o,
        Err(e) => return fail(out, &e),
    };
    let text = format!("action: set autostart {}\n", if on { "on" } else { "off" });
    finish_install(text, app::dispatch_result(UiAction::SetAutostart(on)), out)
}

// ---------------------------------------------------------------------------------------------
// Talking to a running instance
// ---------------------------------------------------------------------------------------------

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

/// Asks a running instance to exit (`--quit`, `--cleanup`) and waits up to `ms` for its window to go.
pub fn stop_running(ms: u64) {
    if let Some(h) = find_running() {
        // SAFETY: plain post.
        unsafe { PostMessageW(h, WM_CLOSE, 0, 0) };
        let start = Instant::now();
        while find_running().is_some() && start.elapsed().as_millis() < ms as u128 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Scripted demo (README frames, about 8 s)
// ---------------------------------------------------------------------------------------------

const DEMO_TIMER: usize = app::TIMER_DEBUG_BASE;
const TICK_MS: u32 = 50;

const T_CLOSE_MENU: u128 = 2200;
const T_SWITCH: u128 = 2600;
const T_OPEN_MAIN: u128 = 3300;
const T_SLIDE_FROM: u128 = 4000;
const T_SLIDE_TO: u128 = 6000;
const T_END: u128 = 7600;

thread_local! {
    static START: Cell<Option<Instant>> = const { Cell::new(None) };
    static DONE: Cell<u32> = const { Cell::new(0) };
}

/// Starts the demo: tray menu, profile switch, main window, peak slider animation, exit.
pub fn start_demo(hwnd: HWND) {
    START.with(|s| s.set(Some(Instant::now())));
    DONE.with(|d| d.set(0));
    // SAFETY: valid window handle; the timer runs on this thread.
    unsafe { SetTimer(hwnd, DEMO_TIMER, TICK_MS, None) };
    app::post_command("open traymenu");
}

fn once(bit: u32) -> bool {
    DONE.with(|d| {
        let v = d.get();
        if v & bit != 0 {
            false
        } else {
            d.set(v | bit);
            true
        }
    })
}

fn switch_profile() {
    let st = ui::snapshot();
    let Some(m) = st.monitors.first() else {
        return;
    };
    let current = m
        .sdr_current
        .and_then(|i| m.sdr_options.get(i))
        .map(|o| o.0.as_str());
    let want = if current == Some("sRGB") {
        "Display P3"
    } else {
        "sRGB"
    };
    if let Some(idx) = m.sdr_options.iter().position(|o| o.0 == want) {
        app::dispatch(UiAction::SetSdr { mon: 0, idx });
    }
}

/// Timer callback for ids at or above `app::TIMER_DEBUG_BASE`.
pub fn on_timer(hwnd: HWND, id: usize) {
    if id != DEMO_TIMER {
        return;
    }
    let Some(start) = START.with(|s| s.get()) else {
        return;
    };
    let t = start.elapsed().as_millis();
    if t >= T_CLOSE_MENU && once(1) {
        // SAFETY: ends the modal menu loop if one is running on this thread.
        unsafe { EndMenu() };
    }
    if t >= T_SWITCH && once(2) {
        switch_profile();
    }
    if t >= T_OPEN_MAIN && once(4) {
        ui::main_window::open();
        app::raise(ui::main_window::hwnd());
        ui::main_window::demo_set_peak(400.0);
    }
    if t >= T_SLIDE_FROM && t <= T_SLIDE_TO + TICK_MS as u128 && ui::main_window::is_open() {
        let f = ((t - T_SLIDE_FROM) as f64 / (T_SLIDE_TO - T_SLIDE_FROM) as f64).clamp(0.0, 1.0);
        let ease = f * f * (3.0 - 2.0 * f);
        ui::main_window::demo_set_peak(400.0 + 600.0 * ease);
    }
    if t >= T_END && once(8) {
        // SAFETY: valid window; WM_CLOSE exits the app.
        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
    }
}
