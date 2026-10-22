//! Developer switches: state dumps, headless flows, the scripted README demo. Output goes to `--out <file>`
//! when given, otherwise to the parent console (the exe is a GUI-subsystem program).

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use crate::app::{self, UiAction, UiMonitor, UiState};
use crate::autostart;
use crate::color::Primaries;
use crate::config::Config;
use crate::icc_write::HdrParams;
use crate::monitors::Monitor;
use crate::tray;
use crate::ui;
use crate::util::wide;
use crate::wcs;
use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::time::Instant;
use windows_sys::Win32::Foundation::{HWND, INVALID_HANDLE_VALUE, RECT};
use windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, InvalidateRect, UpdateWindow};
use windows_sys::Win32::System::Console::{
    AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::keybd_event;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CheckMenuRadioItem, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EndMenu,
    FindWindowW, GetMenuItemCount, GetMenuItemRect, GetMessageW, GetWindowRect, PostMessageW,
    PostQuitMessage, RegisterClassExW, SendMessageW, SetCursorPos, SetForegroundWindow, SetTimer,
    SetWindowPos, SystemParametersInfoW, TranslateMessage, HMENU, HWND_TOPMOST, MF_BYPOSITION, MSG,
    SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_SHOWWINDOW, WM_CLOSE, WM_COPYDATA, WM_DESTROY, WM_PAINT,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
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
        let _ = writeln!(
            s,
            "  sdr white:      {}",
            crate::monitors::sdr_white_nits(m).map_or("(unavailable)".to_string(), |n| format!("{n} nits"))
        );
        let _ = writeln!(
            s,
            "  windows reports: {}",
            crate::dxgi::reported(&m.gdi_name).map_or("(unavailable)".to_string(), |r| dxgi_text(&r))
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

fn dxgi_text(r: &crate::dxgi::Reported) -> String {
    format!(
        "min {} max {} maxfullframe {} nits; color space {}; {} bpc; R {:.4},{:.4} G {:.4},{:.4} B {:.4},{:.4} W {:.4},{:.4}",
        r.min_nits,
        r.max_nits,
        r.max_full_frame_nits,
        r.color_space_name(),
        r.bits_per_color,
        r.red[0],
        r.red[1],
        r.green[0],
        r.green[1],
        r.blue[0],
        r.blue[1],
        r.white[0],
        r.white[1]
    )
}

/// `--debug-dxgi <num>`: what Windows reports for the monitor, read through DXGI.
pub fn dxgi(num: &str, out: Option<&Path>) -> i32 {
    let n = match parse_num(num) {
        Ok(n) => n,
        Err(e) => return fail(out, &e),
    };
    let mons = crate::monitors::enumerate();
    let Some(m) = mons.iter().find(|m| m.num == n) else {
        return fail(out, &format!("no monitor {n}"));
    };
    match crate::dxgi::reported(&m.gdi_name) {
        Some(r) => {
            emit(
                out,
                &format!(
                    "monitor {n} ({}): {}
",
                    m.gdi_name,
                    dxgi_text(&r)
                ),
            );
            0
        }
        None => fail(
            out,
            &format!("DXGI has no output for monitor {n} ({})", m.gdi_name),
        ),
    }
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
        let _ = match crate::monitors::sdr_white_raw(m) {
            Some(raw) => writeln!(
                text,
                "{label}: raw {raw} = {} nits",
                crate::monitors::sdr_white_raw_to_nits(raw)
            ),
            None => writeln!(text, "{label}: unavailable"),
        };
    };
    show("before", &mut text);
    let mut code = 0;
    if let Some(v) = nits {
        match v.trim().parse::<f64>() {
            Ok(v) if v.is_finite() && v > 0.0 => {
                let raw = crate::monitors::sdr_white_nits_to_raw(v);
                let _ = writeln!(text, "set: {v} nits -> raw {raw}");
                match crate::monitors::set_sdr_white_raw(m, raw) {
                    Ok(()) => text.push_str("result: ok\n"),
                    Err(e) => {
                        let _ = writeln!(text, "result: error: {e}");
                        code = 1;
                    }
                }
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
    let code = match app::dispatch_result(UiAction::DeleteProfile {
        file: file.to_string(),
    }) {
        Ok(m) => {
            let _ = writeln!(text, "result: ok: {m}");
            0
        }
        Err(e) => {
            let _ = writeln!(text, "result: error: {e}");
            1
        }
    };
    text.push_str("after:\n");
    text.push_str(&profile_presence(file));
    emit(out, &text);
    code
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
// Screenshot backdrop
// ---------------------------------------------------------------------------------------------

/// `WM_COPYDATA` marker of the demo's message to the stage: six `i32`: mock taskbar x, y, w, h (screen
/// pixels), tray icon center x, hover (0 or 1).
const STAGE_MAGIC: usize = 0x5350_4254;

/// Mock taskbar the stage draws, in screen pixels.
#[derive(Clone, Copy)]
struct StageBar {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    icon_cx: i32,
    hover: bool,
}

thread_local! {
    static STAGE_BAR: Cell<Option<StageBar>> = const { Cell::new(None) };
    /// Stage window origin (screen) and background color (RGB).
    static STAGE_GEO: Cell<(i32, i32, u32)> = const { Cell::new((0, 0, 0)) };
}

fn paint_stage(hwnd: HWND) {
    use windows_sys::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
    // SAFETY: standard paint cycle on this window.
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let dc = BeginPaint(hwnd, &mut ps);
        if let Some(b) = STAGE_BAR.with(|c| c.get()) {
            let (ox, oy, bg) = STAGE_GEO.with(|g| g.get());
            let th = ui::draw::theme();
            if let Some(mut cv) = ui::draw::Canvas::new(b.w, b.h) {
                cv.clear(bg);
                ui::mockbar::paint(&mut cv, &th, ui::draw::system_dpi(), b.icon_cx - b.x, b.hover);
                cv.present(dc, b.x - ox, b.y - oy);
            }
        }
        EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn stage_proc(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
    match msg {
        WM_PAINT => {
            paint_stage(hwnd);
            0
        }
        WM_COPYDATA => {
            let cds = &*(lp as *const COPYDATASTRUCT);
            if cds.dwData == STAGE_MAGIC && cds.cbData as usize == 6 * 4 && !cds.lpData.is_null() {
                let v = std::slice::from_raw_parts(cds.lpData as *const i32, 6);
                let bar = StageBar {
                    x: v[0],
                    y: v[1],
                    w: v[2],
                    h: v[3],
                    icon_cx: v[4],
                    hover: v[5] != 0,
                };
                STAGE_BAR.with(|c| c.set(Some(bar)));
                let (ox, oy, _) = STAGE_GEO.with(|g| g.get());
                let r = RECT {
                    left: bar.x - ox,
                    top: bar.y - oy,
                    right: bar.x - ox + bar.w,
                    bottom: bar.y - oy + bar.h,
                };
                InvalidateRect(hwnd, &r, 0);
                UpdateWindow(hwnd);
            }
            1
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// `--debug-stage light|dark`: a borderless solid-color window over the primary work area, so screenshots
/// of Spectrum's windows never pick up the desktop or other windows around them. Runs until closed.
/// The demo may ask it to draw a mock taskbar segment (see `ui::mockbar`).
///
/// It is topmost (so no other window, such as the terminal that launched it, can come in front and end up in
/// a capture) and does not take focus. While it exists, `app::raise` keeps the surfaces topmost too, and as
/// they are raised after it, they sit above it.
pub fn stage(theme: &str) -> i32 {
    // COLORREF is 0x00BBGGRR: these are 0xE9E6E1 and 0x202020 as RGB.
    let dark = theme.eq_ignore_ascii_case("dark");
    ui::draw::set_forced_theme(Some(dark));
    let color: u32 = if dark { 0x0020_2020 } else { 0x00E1_E6E9 };
    let mut wa = windows_sys::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    let cls = wide("SpectrumStage");
    // SAFETY: class data and the brush outlive the window; the window belongs to this thread.
    unsafe {
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut _ as *mut _, 0);
        let rgb = if dark { 0x202020 } else { 0xE9E6E1 };
        STAGE_GEO.with(|g| g.set((wa.left, wa.top, rgb)));
        let hinst = GetModuleHandleW(std::ptr::null());
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(stage_proc);
        wc.hInstance = hinst;
        wc.hbrBackground = CreateSolidBrush(color);
        wc.lpszClassName = cls.as_ptr();
        RegisterClassExW(&wc);
        let h = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            cls.as_ptr(),
            wide("Spectrum stage").as_ptr(),
            WS_POPUP,
            wa.left,
            wa.top,
            wa.right - wa.left,
            wa.bottom - wa.top,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );
        if h.is_null() {
            return 1;
        }
        SetWindowPos(
            h,
            HWND_TOPMOST,
            wa.left,
            wa.top,
            wa.right - wa.left,
            wa.bottom - wa.top,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    0
}

// ---------------------------------------------------------------------------------------------
// Scripted demo (README frames, about 8 s)
// ---------------------------------------------------------------------------------------------
//
// The demo is the director. It puts the main window at a fixed spot, opens the tray menu once to measure it
// (nothing is recorded yet), then tells the stage to draw a mock taskbar under the menu and writes
// `--out <file>` with the capture region, the rectangles and a cursor path. `scripts/capture.ps1 -Demo`
// records the region from the start time in that file and draws a cursor sprite on the frames; the real
// cursor stays parked far away.

const DEMO_TIMER: usize = app::TIMER_DEBUG_BASE;
const TICK_MS: u32 = 20;

// Times in ms from the start of the recording.
const T_MOVE_TRAY: (i64, i64) = (800, 1300);
const T_HOVER_ON: i64 = 1300;
const T_OPEN_MENU: i64 = 1600;
const T_LEAVE_TRAY: (i64, i64) = (1950, 2650);
const T_FIRST_HIGHLIGHT: i64 = 2650;
const T_HIGHLIGHT_STEP: i64 = 250;
const T_TO_ITEM: (i64, i64) = (2650, 3150);
const T_RADIO: i64 = 3500;
const T_CLOSE_MENU: i64 = 3900;
const T_TO_SLIDER: (i64, i64) = (3950, 4450);
const T_SLIDE: (i64, i64) = (4600, 6600);
const T_END: i64 = 7800;
/// Margin of stage color around the window, the menu and the taskbar (at 100% scale).
const STAGE_MARGIN: i32 = 32;
const PEAK_FROM: f64 = 400.0;
const PEAK_TO: f64 = 1000.0;

type Pt = (f32, f32);

struct Layout {
    main: RECT,
    menu: RECT,
    bar: RECT,
    icon_cx: i32,
    /// Cursor waypoints (screen pixels).
    start: Pt,
    tray: Pt,
    outside: Pt,
    first_item: Pt,
    target_item: Pt,
    /// Menu position of "Display P3" and the number of profile items before the first separator.
    target_idx: usize,
    profiles: usize,
    /// Where the pointer rests on each profile item.
    item_pts: Vec<Pt>,
}

/// What the calibration pass learned about the tray menu.
struct MenuProbe {
    rect: RECT,
    items: Vec<RECT>,
}

thread_local! {
    /// Start of the demo, and (once calibrated) the start of the recording.
    static START: Cell<Option<Instant>> = const { Cell::new(None) };
    static T0: Cell<Option<Instant>> = const { Cell::new(None) };
    static DONE: Cell<u32> = const { Cell::new(0) };
    static CAL: RefCell<Option<MenuProbe>> = const { RefCell::new(None) };
    static LAYOUT: RefCell<Option<Layout>> = const { RefCell::new(None) };
    static OUT: RefCell<Option<std::path::PathBuf>> = const { RefCell::new(None) };
}

/// `--out` target of the next demo (the layout file the capture script reads).
pub fn set_demo_out(p: Option<std::path::PathBuf>) {
    OUT.with(|o| *o.borrow_mut() = p);
}

/// The current state with the first monitor shown as HDR-on with tuned values. Used for screenshots only:
/// the windows read it through `ui::set_mock`, so nothing is applied and no setting changes.
pub fn showcase_state() -> UiState {
    let mut st = app::snapshot();
    if let Some(m) = st.monitors.first_mut() {
        if !m.sdr_options.is_empty() {
            m.sdr_current = Some(0); // the EDID-derived profile
        }
        m.hdr_supported = true;
        m.hdr_on = true;
        m.sdr_white_nits = Some(240.0);
        m.hdr_custom_active = true;
        // Realistic values for the info lines: what the EDID says and what Windows reports for the profile.
        m.hdr_profile = app::HdrProfile::Spectrum;
        m.edid_hdr = Some(crate::edid::HdrStatic {
            max_nits: Some(455.0),
            max_frame_avg_nits: Some(254.0),
            min_nits: Some(0.0),
            pq: true,
        });
        m.reported = Some(crate::dxgi::Reported {
            min_nits: 0.0,
            max_nits: 450.0,
            max_full_frame_nits: 250.0,
            color_space: 12,
            bits_per_color: 10,
            red: [0.6855, 0.3037],
            green: [0.2402, 0.7119],
            blue: [0.1436, 0.0576],
            white: [0.3135, 0.3291],
        });
        m.hdr = HdrParams {
            min_nits: 0.0,
            peak_nits: 450.0,
            full_frame_nits: 250.0,
            saturation: 0.0,
        };
    }
    st
}

/// Profile rows for the profiles window screenshots: this machine's real color folder and associations, run
/// through the same type, usage and deletable logic as the window. Read-only; the showcase window is a mock
/// (`profiles_window::open_showcase`), so Delete only edits the list and never touches a file.
pub fn showcase_profiles() -> Vec<app::ProfileRow> {
    app::list_profiles()
}

/// `open gamut-showcase`: the gamut window with the monitor's EDID-derived profile, sRGB and Display P3
/// ticked. When that profile is not installed, an entry is built in memory from the EDID primaries.
pub fn open_gamut_showcase() {
    let st = showcase_state();
    let Some((label, file)) = st.monitors.first().and_then(|m| m.sdr_options.first().cloned()) else {
        ui::gamut_window::open(None, 0);
        return;
    };
    let prims = app::current_monitors()
        .first()
        .and_then(|m| m.edid.as_ref().map(|e| e.primaries));
    ui::gamut_window::open_showcase(file, label, prims, 0);
}

fn frame_rect(h: HWND) -> RECT {
    // SAFETY: valid RECT out-pointer of the size passed.
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        if DwmGetWindowAttribute(
            h,
            9,
            &mut r as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        ) != 0
        {
            GetWindowRect(h, &mut r);
        }
        r
    }
}

fn menu_window() -> HWND {
    // SAFETY: plain lookup.
    unsafe { FindWindowW(wide("#32768").as_ptr(), std::ptr::null()) }
}

/// The HMENU behind a menu window (`MN_GETHMENU`).
fn menu_handle(win: HWND) -> HMENU {
    // SAFETY: plain message to a menu window of this thread.
    unsafe { SendMessageW(win, 0x1E1, 0, 0) as HMENU }
}

fn tell_stage(bar: &RECT, icon_cx: i32, hover: bool) {
    // SAFETY: the payload outlives the synchronous SendMessage.
    unsafe {
        let h = FindWindowW(wide("SpectrumStage").as_ptr(), std::ptr::null());
        if h.is_null() {
            return;
        }
        let v: [i32; 6] = [
            bar.left,
            bar.top,
            bar.right - bar.left,
            bar.bottom - bar.top,
            icon_cx,
            hover as i32,
        ];
        let cds = COPYDATASTRUCT {
            dwData: STAGE_MAGIC,
            cbData: std::mem::size_of_val(&v) as u32,
            lpData: v.as_ptr() as *mut _,
        };
        SendMessageW(h, WM_COPYDATA, 0, &cds as *const COPYDATASTRUCT as isize);
    }
}

/// Starts the demo: the main window at a fixed spot, then a calibration pass that opens and closes the
/// tray menu once. The windows run against a mock copy of the current state, so nothing is applied.
pub fn start_demo(hwnd: HWND) {
    let mut st = showcase_state();
    if let Some(m) = st.monitors.first_mut() {
        m.hdr.peak_nits = PEAK_FROM;
    }
    ui::set_mock(Some(st));
    START.with(|s| s.set(Some(Instant::now())));
    T0.with(|t| t.set(None));
    DONE.with(|d| d.set(0));
    app::park_cursor_for_menu();
    let mut wa = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    // SAFETY: valid RECT pointer.
    unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut _ as *mut _, 0) };
    let dpi = ui::draw::system_dpi();
    ui::main_window::open_at(wa.left + ui::draw::sc(120, dpi), wa.top + ui::draw::sc(90, dpi));
    app::raise(ui::main_window::hwnd());
    let main = frame_rect(ui::main_window::hwnd());
    // The menu rises from the taskbar tray to the right of the window, its bottom edge at the window's.
    tray::set_menu_anchor(Some((main.right + ui::draw::sc(10, dpi), main.bottom)));
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
    if let Some(idx) = m.sdr_options.iter().position(|o| o.0 == "Display P3") {
        ui::dispatch(UiAction::SetSdr { mon: 0, idx });
    }
}

/// Gives the main window the foreground (an active title bar). A synthetic key press first, because
/// Windows only lets a process that just received input take the foreground.
fn activate_main() {
    // SAFETY: plain input and window calls; F24 is not on any keyboard.
    unsafe {
        keybd_event(0x87, 0, 0, 0);
        keybd_event(0x87, 0, 2, 0);
        SetForegroundWindow(ui::main_window::hwnd());
    }
}

fn smooth(f: f64) -> f64 {
    let f = f.clamp(0.0, 1.0);
    f * f * (3.0 - 2.0 * f)
}

fn lerp(a: Pt, b: Pt, f: f64) -> Pt {
    (a.0 + (b.0 - a.0) * f as f32, a.1 + (b.1 - a.1) * f as f32)
}

/// Position at fraction `f` of the arc length of a polyline.
fn along(pts: &[Pt], f: f64) -> Pt {
    let len = |a: Pt, b: Pt| ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt() as f64;
    let total: f64 = pts.windows(2).map(|w| len(w[0], w[1])).sum();
    let mut left = f.clamp(0.0, 1.0) * total;
    for w in pts.windows(2) {
        let l = len(w[0], w[1]);
        if left <= l || l == 0.0 {
            return lerp(w[0], w[1], if l == 0.0 { 1.0 } else { left / l });
        }
        left -= l;
    }
    pts[pts.len() - 1]
}

fn within(t: i64, span: (i64, i64)) -> f64 {
    smooth((t - span.0) as f64 / (span.1 - span.0) as f64)
}

fn peak_at(t: i64) -> f64 {
    PEAK_FROM + (PEAK_TO - PEAK_FROM) * within(t, T_SLIDE)
}

fn thumb(nits: f64) -> Pt {
    ui::main_window::demo_peak_thumb(nits)
        .map(|(x, y)| (x as f32, y as f32))
        .unwrap_or((0.0, 0.0))
}

/// Scripted cursor position (screen pixels) at `t` ms into the recording.
fn cursor_at(l: &Layout, t: i64) -> Pt {
    if t >= T_SLIDE.0 {
        return thumb(peak_at(t));
    }
    if t >= T_TO_SLIDER.0 {
        return along(&[l.target_item, thumb(PEAK_FROM)], within(t, T_TO_SLIDER));
    }
    if t >= T_TO_ITEM.0 {
        return along(&[l.first_item, l.target_item], within(t, T_TO_ITEM));
    }
    if t >= T_LEAVE_TRAY.0 {
        let r = (l.outside.0, l.tray.1);
        return along(&[l.tray, r, l.outside, l.first_item], within(t, T_LEAVE_TRAY));
    }
    along(&[l.start, l.tray], within(t, T_MOVE_TRAY))
}

/// Calibration pass: waits for the tray menu, measures it, closes it, then plans the recording.
fn calibrate(hwnd: HWND, el: i64) {
    if CAL.with(|c| c.borrow().is_none()) {
        if el > 5000 {
            // SAFETY: valid window; WM_CLOSE exits the app.
            unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
            return;
        }
        let mw = menu_window();
        if mw.is_null() || el < 400 {
            return;
        }
        let hm = menu_handle(mw);
        let mut items = Vec::new();
        // SAFETY: the menu belongs to this thread and stays alive until EndMenu.
        unsafe {
            for i in 0..GetMenuItemCount(hm).max(0) as u32 {
                let mut r: RECT = std::mem::zeroed();
                GetMenuItemRect(std::ptr::null_mut(), hm, i, &mut r);
                items.push(r);
            }
            CAL.with(|c| {
                *c.borrow_mut() = Some(MenuProbe {
                    rect: frame_rect(mw),
                    items,
                })
            });
            EndMenu();
        }
        // Count the settle time from the moment the menu was closed.
        START.with(|s| s.set(Some(Instant::now())));
        return;
    }
    if el < 500 || !menu_window().is_null() {
        return;
    }
    plan();
}

fn plan() {
    let Some(probe) = CAL.with(|c| c.borrow_mut().take()) else {
        return;
    };
    let dpi = ui::draw::system_dpi();
    let sc = |v: i32| ui::draw::sc(v, dpi);
    let st = ui::snapshot();
    let Some(m) = st.monitors.first() else {
        return;
    };
    let target_idx = m
        .sdr_options
        .iter()
        .position(|o| o.0 == "Display P3")
        .unwrap_or(0);
    let profiles = m.sdr_options.len();
    let main = frame_rect(ui::main_window::hwnd());
    let menu = probe.rect;
    let top = main.bottom.max(menu.bottom) + sc(24);
    let bar = RECT {
        left: main.left,
        top,
        right: menu.right.max(main.right),
        bottom: top + sc(ui::mockbar::BAR_H),
    };
    let icon_cx = (menu.left + menu.right) / 2;
    let item = |i: usize| -> Pt {
        let r = probe.items.get(i).copied().unwrap_or(menu);
        ((r.right - sc(40)) as f32, ((r.top + r.bottom) / 2) as f32)
    };
    let bar_cy = ((bar.top + bar.bottom) / 2) as f32;
    let layout = Layout {
        main,
        menu,
        bar,
        icon_cx,
        start: (
            (menu.left + (menu.right - menu.left) * 3 / 5) as f32,
            (main.top + sc(130)) as f32,
        ),
        tray: (icon_cx as f32, bar_cy),
        outside: ((menu.right + sc(18)) as f32, bar_cy),
        first_item: item(0),
        target_item: item(target_idx),
        target_idx,
        profiles,
        item_pts: (0..profiles).map(item).collect(),
    };
    tell_stage(&bar, icon_cx, false);
    // Active title bar again; the clock starts 300 ms from now.
    activate_main();
    let lead = 300;
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
        + lead;
    write_layout(&layout, unix, dpi);
    LAYOUT.with(|l| *l.borrow_mut() = Some(layout));
    T0.with(|t| {
        t.set(Some(
            Instant::now() + std::time::Duration::from_millis(lead as u64),
        ))
    });
}

/// The layout file: scale, start time (Unix ms), length, the capture region, the rectangles and a cursor
/// path every 20 ms. Written to a temporary file and renamed, so the reader never sees half of it.
fn write_layout(l: &Layout, unix_ms: i64, dpi: u32) {
    let Some(path) = OUT.with(|o| o.borrow().clone()) else {
        return;
    };
    let wh = |r: &RECT| format!("{} {} {} {}", r.left, r.top, r.right - r.left, r.bottom - r.top);
    let m = STAGE_MARGIN * dpi as i32 / 96;
    let region = RECT {
        left: l.main.left.min(l.menu.left).min(l.bar.left) - m,
        top: l.main.top.min(l.menu.top).min(l.bar.top) - m,
        right: l.main.right.max(l.menu.right).max(l.bar.right) + m,
        bottom: l.main.bottom.max(l.menu.bottom).max(l.bar.bottom) + m,
    };
    let mut o = String::new();
    let _ = writeln!(o, "scale {:.4}", dpi as f64 / 96.0);
    let _ = writeln!(o, "t0 {unix_ms}");
    let _ = writeln!(o, "end {T_END}");
    let _ = writeln!(o, "region {}", wh(&region));
    let _ = writeln!(o, "main {}", wh(&l.main));
    let _ = writeln!(o, "menu {}", wh(&l.menu));
    let _ = writeln!(o, "bar {}", wh(&l.bar));
    let mut t = 0;
    while t <= T_END {
        let p = cursor_at(l, t);
        let _ = writeln!(o, "key {t} {:.1} {:.1}", p.0, p.1);
        t += 20;
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, o).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Highlights the next menu item the way the cursor would (the real cursor is parked far away).
fn menu_down(p: Pt) {
    // SAFETY: plain call.
    unsafe { SetCursorPos(p.0 as i32, p.1 as i32) };
}

/// Moves the radio dot of the open tray menu to `idx` (mock only: nothing is applied).
fn menu_radio(idx: usize, count: usize) {
    let mw = menu_window();
    if mw.is_null() {
        return;
    }
    // SAFETY: the menu belongs to this thread and is alive while its window is.
    unsafe {
        CheckMenuRadioItem(
            menu_handle(mw),
            0,
            count.saturating_sub(1) as u32,
            idx as u32,
            MF_BYPOSITION,
        );
        InvalidateRect(mw, std::ptr::null(), 1);
        UpdateWindow(mw);
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
    let Some(t0) = T0.with(|t| t.get()) else {
        calibrate(hwnd, start.elapsed().as_millis() as i64);
        return;
    };
    let now = Instant::now();
    if now < t0 {
        return;
    }
    let t = (now - t0).as_millis() as i64;
    LAYOUT.with(|l| {
        let l = l.borrow();
        let Some(l) = l.as_ref() else {
            return;
        };
        if t >= T_HOVER_ON && once(1) {
            tell_stage(&l.bar, l.icon_cx, true);
        }
        if t >= T_OPEN_MENU && once(2) {
            app::post_command("open traymenu");
        }
        for i in 0..=l.target_idx {
            if t >= T_FIRST_HIGHLIGHT + T_HIGHLIGHT_STEP * i as i64 && once(0x10 << i) {
                menu_down(l.item_pts[i]);
            }
        }
        if t >= T_RADIO && once(4) {
            menu_radio(l.target_idx, l.profiles);
        }
        if t >= T_CLOSE_MENU && once(8) {
            // SAFETY: ends the modal menu loop if one is running on this thread.
            unsafe { EndMenu() };
            tell_stage(&l.bar, l.icon_cx, false);
            switch_profile();
            activate_main();
        }
    });
    if t >= T_SLIDE.0 && t <= T_SLIDE.1 + TICK_MS as i64 && ui::main_window::is_open() {
        ui::main_window::demo_set_peak(peak_at(t));
    }
    if t >= T_END + 300 && once(0x8000_0000) {
        // SAFETY: valid window; WM_CLOSE exits the app.
        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
    }
}
