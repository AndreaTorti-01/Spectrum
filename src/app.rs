//! Application state and glue: UI snapshot/dispatch, apply flows, reconcile logic and the hidden
//! message-window loop.
//!
//! Everything runs on the UI thread. Shared state is tiny (the last status line and a monitor cache);
//! the config is re-read from disk for every operation so a debug command run from a second process
//! never fights the tray instance's memory.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use crate::color::{self, Primaries, BT709};
use crate::config::{Config, MonitorCfg};
use crate::icc_read;
use crate::icc_write::{self, HdrParams};
use crate::monitors::{self, Monitor, Rect};
use crate::tray::{self, MenuMonitor, TrayCmd, WM_TRAY};
use crate::ui::{self, draw};
use crate::util::{app_data_dir, log, wide};
use crate::wcs;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::NIN_SELECT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, KillTimer, PostMessageW,
    PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SetCursorPos, SetTimer, SetWindowPos,
    SystemParametersInfoW, TranslateMessage, DBT_DEVNODES_CHANGED, HWND_NOTOPMOST, HWND_TOPMOST, MSG,
    PBT_APMRESUMEAUTOMATIC, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WM_APP, WM_CLOSE,
    WM_CONTEXTMENU, WM_COPYDATA, WM_DESTROY, WM_DEVICECHANGE, WM_DISPLAYCHANGE, WM_LBUTTONUP,
    WM_POWERBROADCAST, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSEXW, WS_POPUP,
};

/// Window class of the hidden top-level window (message-only windows miss broadcast messages).
pub const WINDOW_CLASS: &str = "SpectrumMsgWnd";
/// Registered message that makes a running instance open its main window.
pub const SHOW_MESSAGE_NAME: &str = "Spectrum.Show";
/// `dwData` of the `WM_COPYDATA` messages a second instance sends for debug commands.
pub const COPYDATA_MAGIC: usize = 0x5350_4543;

const TIMER_RECONCILE: usize = 1;
const RECONCILE_DELAY_MS: u32 = 2000;
/// Retries adding the tray icon while the shell is not ready (Run-key start at logon).
const TIMER_TRAY_RETRY: usize = 2;
const TRAY_RETRY_MS: u32 = 2000;
const TRAY_RETRY_MAX: u32 = 15;
/// Timer ids at or above this value belong to `debug` (the demo).
pub const TIMER_DEBUG_BASE: usize = 100;
/// NIN_KEYSELECT (NIN_SELECT | 1).
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
const WM_STARTUP: u32 = WM_APP + 2;
const WM_RUN_CMD: u32 = WM_APP + 3;
/// Ignore repeated tray notifications (v4 sends several per click) for this long.
const TRAY_DEBOUNCE: Duration = Duration::from_millis(300);

// ---------------------------------------------------------------------------------------------
// UI contract types
// ---------------------------------------------------------------------------------------------

/// Which HDR profile Windows uses for a monitor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HdrProfile {
    /// No HDR profile is associated: Windows uses the monitor's EDID values.
    None,
    /// One of Spectrum's own HDR profiles.
    Spectrum,
    /// Someone else's profile (its description).
    Other(String),
}

#[derive(Clone, Debug)]
pub struct UiMonitor {
    pub num: u32,
    pub rect: Rect,
    pub accent: u32,
    pub name: String,
    pub width: i32,
    pub height: i32,
    pub hdr_supported: bool,
    pub hdr_on: bool,
    /// (label, file)
    pub sdr_options: Vec<(String, String)>,
    pub sdr_current: Option<usize>,
    pub has_edid: bool,
    pub hdr: HdrParams,
    pub hdr_custom_active: bool,
    /// Windows "SDR content brightness" in nits; `None` when the driver does not report it.
    pub sdr_white_nits: Option<f64>,
    pub hdr_profile: HdrProfile,
    /// Luminance Windows reports for the output (read through DXGI); `None` with HDR off or when DXGI fails.
    pub reported: Option<crate::dxgi::Reported>,
    /// The HDR static metadata block of the monitor's EDID.
    pub edid_hdr: Option<crate::edid::HdrStatic>,
}

impl UiMonitor {
    /// Highest brightness the monitor can show: what Windows reports, else the EDID peak.
    pub fn effective_peak(&self) -> Option<f64> {
        self.reported
            .as_ref()
            .map(|r| r.max_nits)
            .filter(|v| *v > 0.0)
            .or_else(|| {
                self.edid_hdr
                    .as_ref()
                    .and_then(|h| h.max_nits)
                    .filter(|v| *v > 0.0)
            })
    }

    /// "HDR profile: Spectrum"; `None` for a monitor without HDR.
    pub fn hdr_profile_line(&self) -> Option<String> {
        if !self.hdr_supported {
            return None;
        }
        Some(match &self.hdr_profile {
            HdrProfile::None => "HDR profile: none, using the monitor's EDID values".to_string(),
            HdrProfile::Spectrum => "HDR profile: Spectrum".to_string(),
            HdrProfile::Other(d) => format!("HDR profile: {d}"),
        })
    }

    /// "Windows reports: peak 450 \u{b7} full-frame 250 \u{b7} black 0.00 nits".
    pub fn reported_line(&self) -> Option<String> {
        if !self.hdr_supported {
            return None;
        }
        Some(match &self.reported {
            Some(r) => format!("Windows reports: {}", crate::dxgi::summary(r)),
            None if !self.hdr_on => "Windows reports: unavailable while HDR is off".to_string(),
            None => "Windows reports: unavailable".to_string(),
        })
    }

    /// "Monitor EDID: peak 455 \u{b7} full-frame 254 \u{b7} black 0 nits".
    pub fn edid_hdr_line(&self) -> Option<String> {
        if !self.hdr_supported {
            return None;
        }
        const NONE: &str = "No HDR values in this monitor's EDID";
        let Some(h) = &self.edid_hdr else {
            return Some(NONE.to_string());
        };
        let nits = |v: f64| format!("{}", v.round() as i64);
        let black = |v: f64| {
            if v < 0.005 {
                "0".to_string()
            } else {
                format!("{v:.2}")
            }
        };
        let mut parts = Vec::new();
        if let Some(v) = h.max_nits {
            parts.push(format!("peak {}", nits(v)));
        }
        if let Some(v) = h.max_frame_avg_nits {
            parts.push(format!("full-frame {}", nits(v)));
        }
        if let Some(v) = h.min_nits {
            parts.push(format!("black {}", black(v)));
        }
        if parts.is_empty() {
            return Some(NONE.to_string());
        }
        Some(format!("Monitor EDID: {} nits", parts.join(" \u{b7} ")))
    }

    /// "Above this monitor's 455-nit peak" when the SDR content brightness `white` exceeds the peak.
    pub fn white_warning(&self, white: f64) -> Option<String> {
        let peak = self.effective_peak()?;
        (white > peak.round()).then(|| format!("Above this monitor's {}-nit peak", peak.round() as i64))
    }
}

#[derive(Clone, Debug)]
pub struct UiState {
    pub monitors: Vec<UiMonitor>,
    pub autostart: bool,
    pub status: Option<String>,
}

#[derive(Clone, Debug)]
pub enum UiAction {
    SetSdr { mon: usize, idx: usize },
    ReadEdid { mon: usize },
    SetHdrEnabled { mon: usize, on: bool },
    ApplyHdr { mon: usize, params: HdrParams },
    ResetHdr { mon: usize },
    SetSdrWhite { mon: usize, nits: f64 },
    SetAutostart(bool),
    OpenGamut,
    OpenProfiles,
    DeleteProfile { file: String },
    Identify,
    CloseMain,
}

// ---------------------------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------------------------

thread_local! {
    static STATUS: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Monitors from the most recent enumeration; used for cheap lookups (identify, the gamut window).
    static CACHE: RefCell<Vec<Monitor>> = const { RefCell::new(Vec::new()) };
    static HWND_CELL: Cell<isize> = const { Cell::new(0) };
    /// True when the tray icon is up; false for the short-lived debug windows (they quit with the UI).
    static TRAY_MODE: Cell<bool> = const { Cell::new(false) };
    static LAST_TOGGLE: Cell<Option<Instant>> = const { Cell::new(None) };
    static LAST_MENU: Cell<Option<Instant>> = const { Cell::new(None) };
    /// True while the (modal) tray menu is up; the second v4 notification must not open another.
    static IN_MENU: Cell<bool> = const { Cell::new(false) };
    static TRAY_TRIES: Cell<u32> = const { Cell::new(0) };
    /// Profile description cache: file -> (stamp, label), so snapshots don't re-read profile files.
    static LABELS: RefCell<HashMap<String, (ProfileStamp, String)>> = RefCell::new(HashMap::new());
    static COMMANDS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static SHOW_MSG: Cell<u32> = const { Cell::new(0) };
    static TASKBAR_MSG: Cell<u32> = const { Cell::new(0) };
    static OPTS: RefCell<Option<RunOpts>> = const { RefCell::new(None) };
}

type ProfileStamp = (u64, Option<std::time::SystemTime>);

fn set_status(s: Option<String>) {
    STATUS.with(|c| *c.borrow_mut() = s);
}

fn enumerate_cached() -> Vec<Monitor> {
    let m = monitors::enumerate();
    CACHE.with(|c| *c.borrow_mut() = m.clone());
    m
}

fn cached_monitors() -> Vec<Monitor> {
    let c = CACHE.with(|c| c.borrow().clone());
    if c.is_empty() {
        enumerate_cached()
    } else {
        c
    }
}

/// Fresh monitor list (also refreshes the cache).
pub fn current_monitors() -> Vec<Monitor> {
    enumerate_cached()
}

pub fn hwnd() -> HWND {
    HWND_CELL.with(|c| c.get()) as HWND
}

fn eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn contains_ci(list: &[String], f: &str) -> bool {
    list.iter().any(|x| eq(x, f))
}

fn is_ours(file: &str) -> bool {
    file.to_ascii_lowercase().starts_with("spectrum ")
}

fn monitor_name(m: &Monitor) -> String {
    if let Some(e) = &m.edid {
        return e.display_name();
    }
    if !m.friendly_name.trim().is_empty() {
        return m.friendly_name.clone();
    }
    format!("Monitor {}", m.num)
}

/// Characters that are safe in a file name.
fn safe_name(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')') {
                c
            } else {
                '_'
            }
        })
        .collect();
    t.trim().trim_matches('.').to_string()
}

fn reference_id(file: &str) -> Option<&'static str> {
    wcs::REFERENCE_PROFILES
        .iter()
        .find(|(_, _, f)| eq(f, file))
        .map(|(id, _, _)| *id)
}

/// EDID primaries for the HDR profile; BT.709 when the EDID is missing or its chromaticities are bogus.
fn hdr_prims(m: &Monitor) -> Primaries {
    m.edid
        .as_ref()
        .map(|e| e.primaries)
        .filter(color::primaries_plausible)
        .unwrap_or(BT709)
}

fn backup_has(file: &str) -> bool {
    wcs::backup_dir().join(file).is_file()
}

/// Reads the ICC description of an installed profile, falling back to the file stem.
fn profile_label(file: &str) -> String {
    let stamp = wcs::profile_stamp(file);
    if let Some(st) = stamp {
        let hit = LABELS.with(|l| {
            l.borrow()
                .get(file)
                .filter(|(s, _)| *s == st)
                .map(|(_, v)| v.clone())
        });
        if let Some(v) = hit {
            return v;
        }
    }
    let label = read_label(file);
    if let Some(st) = stamp {
        LABELS.with(|l| l.borrow_mut().insert(file.to_string(), (st, label.clone())));
    }
    label
}

fn read_label(file: &str) -> String {
    if let Some(bytes) = wcs::read_profile(file) {
        if let Ok(info) = icc_read::parse(&bytes) {
            let d = info.desc.trim();
            if !d.is_empty() {
                return d.to_string();
            }
        }
    }
    file.rsplit_once('.')
        .map(|(a, _)| a.to_string())
        .unwrap_or_else(|| file.to_string())
}

// ---------------------------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------------------------

/// File name of the monitor's EDID profile, built or not (the configured one, else the default name).
fn edid_option_file(m: &Monitor, cfg: &MonitorCfg) -> String {
    edid_file_name(&monitor_name(m), &m.key, cfg.edid_profile.as_deref())
}

/// SDR picker entries for one monitor: EDID profile (always, when the monitor has an EDID; picking it builds
/// the profile first), the six reference spaces, then any other profile associated with the monitor.
/// Second value: index of the current default.
fn sdr_options(m: &Monitor, cfg: &MonitorCfg) -> (Vec<(String, String)>, Option<usize>) {
    let mut list: Vec<(String, String)> = Vec::new();
    if m.edid.is_some() {
        list.push((format!("{} (EDID)", monitor_name(m)), edid_option_file(m, cfg)));
    }
    for (_, label, file) in wcs::REFERENCE_PROFILES {
        list.push((label.into(), file.into()));
    }
    for f in wcs::associated(m, false).unwrap_or_default() {
        let hdr_ours = cfg.hdr_file.as_deref().map(|h| eq(h, &f)).unwrap_or(false);
        if hdr_ours || list.iter().any(|(_, x)| eq(x, &f)) {
            continue;
        }
        list.push((profile_label(&f), f));
    }
    let mut current = None;
    if let Some(d) = wcs::default_profile(m, false) {
        match list.iter().position(|(_, x)| eq(x, &d)) {
            Some(i) => current = Some(i),
            None => {
                let hdr_ours = cfg.hdr_file.as_deref().map(|h| eq(h, &d)).unwrap_or(false);
                if !hdr_ours {
                    list.push((profile_label(&d), d));
                    current = Some(list.len() - 1);
                }
            }
        }
    }
    (list, current)
}

/// Slider start values: config, else the current HDR default profile (MHC2), else the EDID HDR block,
/// else 0 / 1000 / 400 / 0.
fn initial_hdr(m: &Monitor, cfg: &MonitorCfg) -> HdrParams {
    if let Some(h) = cfg.hdr {
        return h;
    }
    if m.hdr_supported {
        if let Some(f) = wcs::default_profile(m, true) {
            if let Some(bytes) = wcs::read_profile(&f) {
                if let Ok(info) = icc_read::parse(&bytes) {
                    if let Some(mh) = info.mhc2 {
                        let peak = if mh.peak_nits > 0.0 { mh.peak_nits } else { 1000.0 };
                        return HdrParams {
                            min_nits: mh.min_nits.max(0.0),
                            peak_nits: peak,
                            full_frame_nits: info.lumi.filter(|v| *v > 0.0).unwrap_or(peak * 0.4),
                            saturation: 0.0,
                        };
                    }
                }
            }
        }
    }
    if let Some(h) = m.edid.as_ref().and_then(|e| e.hdr.as_ref()) {
        let peak = h.max_nits.unwrap_or(1000.0);
        return HdrParams {
            min_nits: h.min_nits.unwrap_or(0.0),
            peak_nits: peak,
            full_frame_nits: h.max_frame_avg_nits.unwrap_or(400.0).min(peak),
            saturation: 0.0,
        };
    }
    HdrParams {
        min_nits: 0.0,
        peak_nits: 1000.0,
        full_frame_nits: 400.0,
        saturation: 0.0,
    }
}

fn hdr_profile_of(m: &Monitor) -> HdrProfile {
    match wcs::default_profile(m, true) {
        None => HdrProfile::None,
        Some(f) if is_ours(&f) => HdrProfile::Spectrum,
        Some(f) => HdrProfile::Other(profile_label(&f)),
    }
}

fn build_ui_monitor(m: &Monitor, cfg: &Config) -> UiMonitor {
    let entry = cfg.monitors.get(&m.key).cloned().unwrap_or_default();
    let (sdr_options, sdr_current) = sdr_options(m, &entry);
    let hdr_custom_active = match (&entry.hdr_file, &entry.hdr) {
        (Some(f), Some(_)) => wcs::associated(m, true)
            .map(|l| contains_ci(&l, f))
            .unwrap_or(false),
        _ => false,
    };
    UiMonitor {
        num: m.num,
        rect: m.rect,
        accent: m.accent,
        name: monitor_name(m),
        width: m.rect.right - m.rect.left,
        height: m.rect.bottom - m.rect.top,
        hdr_supported: m.hdr_supported,
        hdr_on: m.hdr_enabled,
        sdr_options,
        sdr_current,
        has_edid: m.edid.is_some(),
        hdr: initial_hdr(m, &entry),
        hdr_custom_active,
        sdr_white_nits: if m.hdr_supported {
            monitors::sdr_white_nits(m)
        } else {
            None
        },
        hdr_profile: if m.hdr_supported {
            hdr_profile_of(m)
        } else {
            HdrProfile::None
        },
        reported: if m.hdr_supported && m.hdr_enabled {
            crate::dxgi::reported(&m.gdi_name)
        } else {
            None
        },
        edid_hdr: m.edid.as_ref().and_then(|e| e.hdr.clone()),
    }
}

pub fn snapshot() -> UiState {
    let cfg = Config::load();
    let mons = enumerate_cached();
    UiState {
        monitors: mons.iter().map(|m| build_ui_monitor(m, &cfg)).collect(),
        autostart: crate::autostart::autostart_enabled(),
        status: STATUS.with(|s| s.borrow().clone()),
    }
}

// ---------------------------------------------------------------------------------------------
// Apply flows
// ---------------------------------------------------------------------------------------------

fn monitor_at(idx: usize) -> Result<Monitor, String> {
    enumerate_cached()
        .into_iter()
        .nth(idx)
        .ok_or_else(|| "Couldn't find that monitor. Reopen the window and try again.".to_string())
}

fn save_cfg(m: &Monitor, f: impl FnOnce(&mut MonitorCfg)) {
    let mut cfg = Config::load();
    let e = cfg.monitor_mut(&m.key);
    e.name = monitor_name(m);
    f(e);
    cfg.save();
}

/// Makes sure a profile file exists in the color folder: restores it from the backup or regenerates a
/// reference space.
fn ensure_file(file: &str) -> Result<(), String> {
    if wcs::profile_exists(file) || wcs::restore_from_backup(file) {
        return Ok(());
    }
    if let Some(id) = reference_id(file) {
        wcs::ensure_reference_profile(id)?;
        return Ok(());
    }
    Err(format!("Profile {file} is missing and has no backup"))
}

fn set_sdr(idx_mon: usize, idx: usize) -> Result<String, String> {
    let m = monitor_at(idx_mon)?;
    let cfg = Config::load();
    let entry = cfg.monitors.get(&m.key).cloned().unwrap_or_default();
    let (opts, _) = sdr_options(&m, &entry);
    let (label, file) = opts
        .get(idx)
        .cloned()
        .ok_or_else(|| "Couldn't find that profile. Reopen the window and try again.".to_string())?;
    let fail = |e: String| {
        log(&format!("set sdr: {file} on monitor {}: {e}", m.num));
        if e.starts_with(wcs::KEPT_PREFIX) {
            return e;
        }
        format!("Couldn't apply {label} to monitor {}. Try again.", m.num)
    };
    if m.edid.is_some()
        && eq(&file, &edid_option_file(&m, &entry))
        && !wcs::profile_exists(&file)
        && !backup_has(&file)
    {
        // The EDID profile is not built yet: same flow as "Read profile from monitor".
        return read_edid(idx_mon);
    }
    ensure_file(&file).map_err(fail)?;
    wcs::ensure_user_scope(&m).map_err(fail)?;
    wcs::set_default(&m, &file, false).map_err(fail)?;
    save_cfg(&m, |e| e.sdr = Some(file.clone()));
    log(&format!("applied {file} to monitor {} ({})", m.num, m.key));
    Ok(format!("Applied {label} to monitor {}", m.num))
}

/// "Spectrum <name> <key> EDID.icm": the key keeps two identical monitors apart. A config that still
/// holds the older "Spectrum <name> EDID.icm" keeps using it.
fn edid_file_name(name: &str, key: &str, current: Option<&str>) -> String {
    let old = format!("Spectrum {} EDID.icm", safe_name(name));
    match current {
        Some(c) if eq(c, &old) => old,
        _ => format!("Spectrum {} {} EDID.icm", safe_name(name), safe_name(key)),
    }
}

fn read_edid(idx_mon: usize) -> Result<String, String> {
    let m = monitor_at(idx_mon)?;
    let fail = |e: &str| {
        log(&format!("read edid: monitor {}: {e}", m.num));
        if e.starts_with(wcs::KEPT_PREFIX) {
            return e.to_string();
        }
        format!(
            "Couldn't read EDID from monitor {}. Reconnect the cable and try again.",
            m.num
        )
    };
    let edid = match &m.edid {
        Some(e) => e.clone(),
        None => monitors::read_edid_registry(&m.device_path)
            .and_then(|raw| crate::edid::parse(&raw).ok())
            .ok_or_else(|| fail("no EDID in the registry"))?,
    };
    if !color::primaries_plausible(&edid.primaries) {
        log(&format!(
            "read edid: monitor {}: implausible chromaticities {:?}",
            m.num, edid.primaries
        ));
        return Err(format!(
            "Monitor {} reports invalid color data. Spectrum can't build a profile from it.",
            m.num
        ));
    }
    let name = edid.display_name();
    let cfg = Config::load();
    let current = cfg.monitors.get(&m.key).and_then(|e| e.edid_profile.clone());
    let file = edid_file_name(&name, &m.key, current.as_deref());
    let desc = format!("{name} (EDID)");
    let bytes = icc_write::build_sdr_profile(&desc, &edid.primaries, edid.trc());

    let dir = app_data_dir().join("edid");
    if std::fs::create_dir_all(&dir).is_ok() {
        if let Err(e) = std::fs::write(dir.join(format!("{}.bin", safe_name(&m.key))), &edid.raw) {
            log(&format!("read edid: saving raw EDID failed: {e}"));
        }
    }
    wcs::install_profile(&file, &bytes).map_err(|e| fail(&e))?;
    wcs::ensure_user_scope(&m).map_err(|e| fail(&e))?;
    wcs::set_default(&m, &file, false).map_err(|e| fail(&e))?;
    save_cfg(&m, |e| {
        e.edid_profile = Some(file.clone());
        e.sdr = Some(file.clone());
    });
    log(&format!(
        "EDID profile {file} applied to monitor {} ({})",
        m.num, m.key
    ));
    Ok(format!("Applied EDID profile to monitor {}", m.num))
}

fn wait_for_hdr(idx: usize, on: bool) {
    for _ in 0..10 {
        let now = enumerate_cached().get(idx).map(|m| m.hdr_enabled);
        if now == Some(on) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn set_hdr_enabled(idx_mon: usize, on: bool) -> Result<String, String> {
    let m = monitor_at(idx_mon)?;
    if let Err(e) = monitors::set_hdr(&m, on) {
        log(&format!("set hdr {on} on monitor {}: {e}", m.num));
        return Err(format!(
            "Couldn't change HDR on monitor {}. Check Settings \u{203a} Display \u{203a} HDR.",
            m.num
        ));
    }
    wait_for_hdr(idx_mon, on);
    Ok(format!(
        "Turned HDR {} for monitor {}",
        if on { "on" } else { "off" },
        m.num
    ))
}

fn hdr_file_name(m: &Monitor, suffix: &str) -> String {
    format!("Spectrum HDR {} {suffix}.icc", safe_name(&m.key))
}

fn apply_hdr(idx_mon: usize, params: HdrParams) -> Result<String, String> {
    let m = monitor_at(idx_mon)?;
    if !m.hdr_supported {
        return Err(format!("Monitor {} doesn't support HDR.", m.num));
    }
    let cfg = Config::load();
    let prev = cfg.monitors.get(&m.key).and_then(|e| e.hdr_file.clone());
    let a = hdr_file_name(&m, "A");
    let b = hdr_file_name(&m, "B");
    let file = match &prev {
        Some(p) if eq(p, &a) => b,
        _ => a,
    };
    let fail = |e: String| {
        log(&format!("apply hdr on monitor {}: {e}", m.num));
        if e.starts_with(wcs::KEPT_PREFIX) {
            return e;
        }
        format!("Couldn't apply HDR profile to monitor {}. Try again.", m.num)
    };
    let desc = format!("Spectrum HDR {}", monitor_name(&m));
    let bytes = icc_write::build_hdr_profile(&desc, &hdr_prims(&m), &params);
    wcs::install_profile(&file, &bytes).map_err(fail)?;
    if let Err(e) = wcs::ensure_user_scope(&m).and_then(|_| wcs::set_default(&m, &file, true)) {
        if drop_hdr_profile(&m, &file) {
            let _ = wcs::delete_profile(&file);
        }
        return Err(fail(e));
    }
    if let Some(p) = prev.filter(|p| !eq(p, &file)) {
        if drop_hdr_profile(&m, &p) {
            if let Err(e) = wcs::delete_profile(&p) {
                log(&format!("apply hdr: deleting old {p}: {e}"));
            }
        }
    }
    save_cfg(&m, |e| {
        e.hdr = Some(params);
        e.hdr_file = Some(file.clone());
    });
    log(&format!(
        "HDR profile {file} applied to monitor {} (min {} peak {} full {} sat {})",
        m.num, params.min_nits, params.peak_nits, params.full_frame_nits, params.saturation
    ));
    Ok(format!("Applied HDR profile to monitor {}", m.num))
}

/// Removes the HDR association of `file`. True when it is gone (removed, or not listed any more), so the
/// caller may delete the file; false when the association may still exist.
fn drop_hdr_profile(m: &Monitor, file: &str) -> bool {
    let Err(e) = wcs::disassociate(m, file, true) else {
        return true;
    };
    match wcs::associated(m, true) {
        Ok(list) if !contains_ci(&list, file) => true,
        _ => {
            log(&format!(
                "monitor {}: could not remove HDR association of {file}: {e}",
                m.num
            ));
            false
        }
    }
}

/// Windows' own "SDR content brightness" (not part of the MHC2 profile).
fn set_sdr_white(idx_mon: usize, nits: f64) -> Result<String, String> {
    let m = monitor_at(idx_mon)?;
    let nits = monitors::round_sdr_white_nits(nits);
    if let Err(e) = monitors::set_sdr_white_nits(&m, nits) {
        log(&format!("set sdr white on monitor {}: {e}", m.num));
        return Err(format!(
            "Couldn't set SDR content on monitor {}. Check Settings \u{203a} Display \u{203a} HDR.",
            m.num
        ));
    }
    log(&format!(
        "SDR content brightness {nits} nits on monitor {}",
        m.num
    ));
    Ok(format!("Set SDR content to {nits} nits"))
}

fn reset_hdr(idx_mon: usize) -> Result<String, String> {
    let m = monitor_at(idx_mon)?;
    let cfg = Config::load();
    let entry = cfg.monitors.get(&m.key).cloned().unwrap_or_default();
    let Some(file) = entry.hdr_file else {
        return Ok(format!("Nothing to reset on monitor {}", m.num));
    };
    if !drop_hdr_profile(&m, &file) {
        // Keep the file and the config entry so the next reconcile or reset can still find it.
        return Err(format!("Couldn't reset HDR on monitor {}. Try again.", m.num));
    }
    if let Err(e) = wcs::delete_profile(&file) {
        log(&format!("reset hdr: deleting {file}: {e}"));
    }
    save_cfg(&m, |e| {
        e.hdr = None;
        e.hdr_file = None;
    });
    log(&format!("HDR profile {file} removed from monitor {}", m.num));
    Ok(format!("Reset HDR on monitor {}", m.num))
}

// ---------------------------------------------------------------------------------------------
// Manage profiles
// ---------------------------------------------------------------------------------------------

/// What a color folder profile is, for the tag in the profiles window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileKind {
    Sdr,
    Hdr,
    Lut,
    Matrix,
}

impl ProfileKind {
    pub fn label(self) -> &'static str {
        match self {
            ProfileKind::Sdr => "SDR",
            ProfileKind::Hdr => "HDR",
            ProfileKind::Lut => "LUT",
            ProfileKind::Matrix => "Matrix",
        }
    }
}

/// One row of the profiles window.
#[derive(Clone, Debug)]
pub struct ProfileRow {
    pub file: String,
    pub name: String,
    pub kind: ProfileKind,
    /// "Used by monitor 1 (default)" or "Not in use".
    pub used: String,
    pub deletable: Result<(), wcs::Undeletable>,
}

/// Tag rule: MHC2 in an advanced-color list is HDR, a profile with LUTs is LUT, a matrix profile in an
/// SDR list is SDR, any other matrix profile is Matrix.
fn profile_kind(has_mhc2: bool, matrix: bool, in_hdr_list: bool, in_sdr_list: bool) -> ProfileKind {
    if has_mhc2 && in_hdr_list {
        ProfileKind::Hdr
    } else if !matrix {
        ProfileKind::Lut
    } else if in_sdr_list {
        ProfileKind::Sdr
    } else {
        ProfileKind::Matrix
    }
}

/// `users` are (monitor number, is default). "Used by monitor 1 (default), 2" or "Not in use".
pub fn used_by_text(users: &[(u32, bool)]) -> String {
    if users.is_empty() {
        return "Not in use".to_string();
    }
    let list: Vec<String> = users
        .iter()
        .map(|(n, d)| {
            if *d {
                format!("{n} (default)")
            } else {
                n.to_string()
            }
        })
        .collect();
    format!(
        "Used by monitor{} {}",
        if users.len() > 1 { "s" } else { "" },
        list.join(", ")
    )
}

/// Every .icc/.icm in the color folder with its type, the monitors using it and whether it can be deleted.
pub fn list_profiles() -> Vec<ProfileRow> {
    struct Use {
        num: u32,
        sdr: bool,
        hdr: bool,
        default: bool,
    }
    let mons = enumerate_cached();
    let mut uses: HashMap<String, Vec<Use>> = HashMap::new();
    for m in &mons {
        for hdr in [false, true] {
            let default = wcs::default_profile(m, hdr);
            for f in wcs::associated(m, hdr).unwrap_or_default() {
                let key = f.to_ascii_lowercase();
                let is_default = default.as_deref().map(|d| eq(d, &f)).unwrap_or(false);
                let list = uses.entry(key).or_default();
                match list.iter_mut().find(|u| u.num == m.num) {
                    Some(u) => {
                        u.sdr |= !hdr;
                        u.hdr |= hdr;
                        u.default |= is_default;
                    }
                    None => list.push(Use {
                        num: m.num,
                        sdr: !hdr,
                        hdr,
                        default: is_default,
                    }),
                }
            }
        }
    }
    wcs::list_installed()
        .into_iter()
        .filter_map(|file| {
            let u = uses.get(&file.to_ascii_lowercase());
            let in_hdr = u.map(|l| l.iter().any(|x| x.hdr)).unwrap_or(false);
            let in_sdr = u.map(|l| l.iter().any(|x| x.sdr)).unwrap_or(false);
            let info = wcs::read_profile(&file).and_then(|b| icc_read::parse(&b).ok());
            // Print, scanner and abstract profiles are not monitor profiles; an unreadable file stays listed.
            if info.as_ref().is_some_and(|i| !i.is_display_rgb()) {
                return None;
            }
            let kind = match &info {
                Some(i) => profile_kind(i.mhc2.is_some(), i.is_matrix_trc, in_hdr, in_sdr),
                None => profile_kind(false, true, in_hdr, in_sdr),
            };
            let users: Vec<(u32, bool)> = u
                .map(|l| l.iter().map(|x| (x.num, x.default)).collect())
                .unwrap_or_default();
            Some(ProfileRow {
                name: profile_label(&file),
                kind,
                used: used_by_text(&users),
                deletable: wcs::can_delete(&file),
                file,
            })
        })
        .collect()
}

/// Removes a profile from the system: disassociates it from every monitor (SDR and HDR), deletes the file
/// and Spectrum's backup copy, and clears the config fields that name it so reconcile leaves it gone.
pub fn delete_profile(file: &str) -> Result<String, String> {
    if let Err(why) = wcs::can_delete(file) {
        return Err(format!("Can't delete {file}. {}", why.hint()));
    }
    let label = profile_label(file);
    for m in &enumerate_cached() {
        for hdr in [false, true] {
            let listed = match wcs::associated(m, hdr) {
                Ok(l) => contains_ci(&l, file),
                Err(_) => true,
            };
            if !listed {
                continue;
            }
            if let Err(e) = wcs::disassociate(m, file, hdr) {
                let still = wcs::associated(m, hdr)
                    .map(|l| contains_ci(&l, file))
                    .unwrap_or(true);
                if still {
                    log(&format!("delete profile: {file} on monitor {}: {e}", m.num));
                    return Err(format!(
                        "Couldn't remove {label} from monitor {}. Try again.",
                        m.num
                    ));
                }
            }
        }
    }
    if let Err(e) = wcs::delete_profile(file) {
        log(&format!("delete profile: {file}: {e}"));
        return Err(format!(
            "Couldn't delete {label}. Close apps using it and try again."
        ));
    }
    let mut cfg = Config::load();
    if cfg.forget_profile(file) {
        cfg.save();
    }
    LABELS.with(|l| l.borrow_mut().remove(file));
    log(&format!("deleted profile {file}"));
    Ok(format!("Deleted {label}"))
}

fn current_sdr_file(m: &Monitor) -> Option<String> {
    wcs::default_profile(m, false)
}

fn open_gamut() {
    let mons = cached_monitors();
    let idx = if ui::main_window::is_open() {
        ui::main_window::selected()
    } else {
        0
    };
    let file = mons.get(idx).and_then(current_sdr_file);
    ui::gamut_window::open(file, idx);
}

fn identify() {
    let rects: Vec<(Rect, u32, u32)> = enumerate_cached()
        .iter()
        .map(|m| (m.rect, m.num, m.accent))
        .collect();
    ui::overlay::identify_all(&rects);
}

/// Runs one UI action. `Ok` carries the status line (may be empty).
pub fn execute(a: UiAction) -> Result<String, String> {
    match a {
        UiAction::SetSdr { mon, idx } => set_sdr(mon, idx),
        UiAction::ReadEdid { mon } => read_edid(mon),
        UiAction::SetHdrEnabled { mon, on } => set_hdr_enabled(mon, on),
        UiAction::ApplyHdr { mon, params } => apply_hdr(mon, params),
        UiAction::ResetHdr { mon } => reset_hdr(mon),
        UiAction::SetSdrWhite { mon, nits } => set_sdr_white(mon, nits),
        UiAction::SetAutostart(on) => crate::autostart::set_autostart(on)
            .map(|_| String::new())
            .map_err(|e| {
                log(&format!("autostart {on}: {e}"));
                "Couldn't change the startup setting. Try again.".to_string()
            }),
        UiAction::OpenGamut => {
            open_gamut();
            Ok(String::new())
        }
        UiAction::OpenProfiles => {
            ui::profiles_window::open();
            Ok(String::new())
        }
        UiAction::DeleteProfile { file } => delete_profile(&file),
        UiAction::Identify => {
            identify();
            Ok(String::new())
        }
        UiAction::CloseMain => {
            ui::overlay::hide_all();
            draw::trim_working_set();
            if !TRAY_MODE.with(|t| t.get()) && !ui::gamut_window::is_open() && !ui::profiles_window::is_open()
            {
                // SAFETY: plain call on the UI thread.
                unsafe { PostQuitMessage(0) };
            }
            Ok(String::new())
        }
    }
}

/// Synchronous; the UI calls `snapshot()` again afterwards.
pub fn dispatch(a: UiAction) {
    let _ = dispatch_result(a);
}

/// Same as [`dispatch`] (status line and monitor cache included), but also returns the outcome so
/// headless callers can report it.
pub fn dispatch_result(a: UiAction) -> Result<String, String> {
    let quiet = matches!(
        a,
        UiAction::CloseMain | UiAction::OpenGamut | UiAction::OpenProfiles | UiAction::Identify
    );
    let r = execute(a);
    match &r {
        Ok(msg) if !msg.is_empty() => set_status(Some(msg.clone())),
        Ok(_) => {}
        Err(msg) => set_status(Some(msg.clone())),
    }
    if !quiet {
        // A changed default or HDR state may move other monitors' options too.
        enumerate_cached();
    }
    r
}

// ---------------------------------------------------------------------------------------------
// Reconcile (driver-update survival)
// ---------------------------------------------------------------------------------------------

/// Brings every configured monitor back to the user's choice. Returns one line per action taken.
pub fn reconcile() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mons = enumerate_cached();
    let mut cfg = Config::load();
    let mut changed = false;
    for m in &mons {
        let Some(entry) = cfg.monitors.get_mut(&m.key) else {
            continue;
        };
        let tag = format!("monitor {} ({})", m.num, m.key);
        let name = monitor_name(m);
        if entry.name != name {
            entry.name = name.clone();
            changed = true;
        }

        // Profile files in the color folder.
        for f in [entry.edid_profile.clone(), entry.sdr.clone()]
            .into_iter()
            .flatten()
        {
            if wcs::profile_exists(&f) {
                continue;
            }
            match ensure_file(&f) {
                Ok(()) => out.push(format!("{tag}: restored {f}")),
                Err(e) => out.push(format!("{tag}: {e}")),
            }
        }

        // A driver update can switch "use my settings for this device" off while the list stays intact.
        if entry.sdr.is_some() || entry.hdr_file.is_some() {
            if let Err(e) = wcs::ensure_user_scope(m) {
                out.push(format!("{tag}: could not enable per-user profiles: {e}"));
            }
        }

        // SDR choice.
        if let Some(file) = entry.sdr.clone() {
            if wcs::profile_exists(&file) {
                match wcs::associated(m, false) {
                    // A failed query is not an empty list: leave the monitor alone.
                    Err(e) => out.push(format!("{tag}: skipped SDR check, could not list profiles: {e}")),
                    Ok(list) if !contains_ci(&list, &file) => {
                        let r = wcs::ensure_user_scope(m).and_then(|_| wcs::set_default(m, &file, false));
                        match r {
                            Ok(()) => out.push(format!("{tag}: re-associated {file} as the SDR default")),
                            Err(e) => out.push(format!("{tag}: could not re-associate {file}: {e}")),
                        }
                    }
                    Ok(_) => {
                        if let Some(def) = wcs::default_profile(m, false) {
                            if !eq(&def, &file) {
                                out.push(format!(
                                    "{tag}: default changed in Windows to {def}; config now follows it"
                                ));
                                entry.sdr = Some(def);
                                changed = true;
                            }
                        }
                    }
                }
            }
        }

        // HDR profile.
        if m.hdr_supported {
            if let Some(file) = entry.hdr_file.clone() {
                if !wcs::profile_exists(&file) && !wcs::restore_from_backup(&file) {
                    if let Some(p) = entry.hdr {
                        let desc = format!("Spectrum HDR {name}");
                        let bytes = icc_write::build_hdr_profile(&desc, &hdr_prims(m), &p);
                        match wcs::install_profile(&file, &bytes) {
                            Ok(()) => out.push(format!("{tag}: rebuilt {file}")),
                            Err(e) => out.push(format!("{tag}: could not rebuild {file}: {e}")),
                        }
                    }
                } else if !wcs::profile_exists(&file) {
                    out.push(format!("{tag}: could not restore {file}"));
                }
                if wcs::profile_exists(&file) {
                    match wcs::associated(m, true) {
                        Err(e) => out.push(format!("{tag}: skipped HDR check, could not list profiles: {e}")),
                        Ok(list) if !contains_ci(&list, &file) => {
                            let r = wcs::ensure_user_scope(m).and_then(|_| wcs::set_default(m, &file, true));
                            match r {
                                Ok(()) => out.push(format!("{tag}: re-associated {file} as the HDR default")),
                                Err(e) => out.push(format!("{tag}: could not re-associate {file}: {e}")),
                            }
                        }
                        Ok(_) => {
                            if let Some(def) = wcs::default_profile(m, true) {
                                if !eq(&def, &file) {
                                    // The user picked another HDR profile: stop managing ours instead of fighting.
                                    out.push(format!(
                                        "{tag}: HDR default changed in Windows to {def}; dropped {file}"
                                    ));
                                    if drop_hdr_profile(m, &file) {
                                        let _ = wcs::delete_profile(&file);
                                    }
                                    entry.hdr_file = None;
                                    entry.hdr = None;
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if changed {
        cfg.save();
    }
    for line in &out {
        log(&format!("reconcile: {line}"));
    }
    out
}

/// Debug: removes Spectrum's own associations (SDR and HDR) from one monitor, as a driver update would.
/// The files stay in place. Returns one line per removal.
pub fn wipe_monitor(num: u32) -> Result<Vec<String>, String> {
    let mons = enumerate_cached();
    let m = mons
        .iter()
        .find(|m| m.num == num)
        .ok_or_else(|| format!("no monitor {num}"))?;
    let mut out = Vec::new();
    for hdr in [false, true] {
        let kind = if hdr { "HDR" } else { "SDR" };
        let list = match wcs::associated(m, hdr) {
            Ok(l) => l,
            Err(e) => {
                out.push(format!("could not list {kind} profiles: {e}"));
                continue;
            }
        };
        for f in list {
            if !is_ours(&f) {
                continue;
            }
            match wcs::disassociate(m, &f, hdr) {
                Ok(()) => out.push(format!("removed {kind} association of {f}")),
                Err(e) => out.push(format!("could not remove {f}: {e}")),
            }
        }
    }
    log(&format!("wipe monitor {num}: {out:?}"));
    Ok(out)
}

/// Every profile file name Spectrum may have associated with this monitor, from the config and the
/// fixed names.
fn known_files(m: &Monitor, cfg: &Config, hdr: bool) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    if hdr {
        v.push(hdr_file_name(m, "A"));
        v.push(hdr_file_name(m, "B"));
    } else {
        v.extend(
            wcs::REFERENCE_PROFILES
                .iter()
                .map(|(_, _, f)| f.to_string())
                .filter(|f| !eq(f, wcs::SRGB_INBOX)),
        );
    }
    if let Some(e) = cfg.monitors.get(&m.key) {
        let cfg_files = if hdr {
            [e.hdr_file.clone(), None]
        } else {
            [e.edid_profile.clone(), e.sdr.clone()]
        };
        for f in cfg_files.into_iter().flatten().filter(|f| is_ours(f)) {
            if !contains_ci(&v, &f) {
                v.push(f);
            }
        }
    }
    v
}

/// Uninstall helper: removes every Spectrum profile association and file, plus Spectrum's data folder.
pub fn remove_all_profiles() -> Vec<String> {
    let mut out = Vec::new();
    let cfg = Config::load();
    for m in &monitors::enumerate() {
        for hdr in [false, true] {
            let names = match wcs::associated(m, hdr) {
                Ok(list) => list.into_iter().filter(|f| is_ours(f)).collect(),
                Err(e) => {
                    // Can't list: try every file name Spectrum could have associated.
                    out.push(format!("monitor {}: could not list profiles: {e}", m.num));
                    known_files(m, &cfg, hdr)
                }
            };
            for f in names {
                match wcs::disassociate(m, &f, hdr) {
                    Ok(()) => out.push(format!("monitor {}: removed {f}", m.num)),
                    Err(e) => out.push(format!("monitor {}: could not remove {f}: {e}", m.num)),
                }
            }
        }
    }
    for f in wcs::list_installed() {
        if is_ours(&f) {
            match wcs::delete_profile(&f) {
                Ok(()) => out.push(format!("deleted {f}")),
                Err(e) => out.push(format!("could not delete {f}: {e}")),
            }
        }
    }
    let _ = std::fs::remove_dir_all(app_data_dir());
    out
}

// ---------------------------------------------------------------------------------------------
// Tray menu and commands
// ---------------------------------------------------------------------------------------------

fn menu_monitors() -> Vec<MenuMonitor> {
    if ui::is_mock() {
        // Screenshots: the menu shows the mock state, like the windows do.
        return ui::snapshot()
            .monitors
            .iter()
            .map(|u| MenuMonitor {
                label: format!("{} \u{b7} {}", u.num, u.name),
                profiles: u.sdr_options.clone(),
                current: u.sdr_current,
                hdr_supported: u.hdr_supported,
                hdr_on: u.hdr_on,
            })
            .collect();
    }
    let cfg = Config::load();
    enumerate_cached()
        .iter()
        .map(|m| {
            let entry = cfg.monitors.get(&m.key).cloned().unwrap_or_default();
            let (opts, current) = sdr_options(m, &entry);
            MenuMonitor {
                label: format!("{} \u{b7} {}", m.num, monitor_name(m)),
                profiles: opts,
                current,
                hdr_supported: m.hdr_supported,
                hdr_on: m.hdr_enabled,
            }
        })
        .collect()
}

fn finish() {
    if ui::main_window::is_open() {
        ui::main_window::refresh();
    }
}

/// Runs a tray menu command (also used by `--debug-tray-cmd`).
pub fn run_tray_cmd(cmd: TrayCmd) {
    let exiting = cmd == TrayCmd::Exit;
    match cmd {
        TrayCmd::Open => ui::main_window::open(),
        TrayCmd::Exit => exit_app(),
        TrayCmd::SetSdr { mon, idx } => {
            dispatch(UiAction::SetSdr { mon, idx });
            finish();
        }
        TrayCmd::ToggleHdr { mon } => {
            let on = enumerate_cached()
                .get(mon)
                .map(|m| m.hdr_enabled)
                .unwrap_or(false);
            dispatch(UiAction::SetHdrEnabled { mon, on: !on });
            finish();
        }
    }
    if !exiting {
        draw::trim_working_set();
    }
}

/// Shows the tray menu at the cursor (modal) and runs the chosen command.
pub fn show_tray_menu() {
    if IN_MENU.with(|c| c.get()) {
        return;
    }
    let last = LAST_MENU.with(|l| l.get());
    if last.map(|t| t.elapsed() < TRAY_DEBOUNCE).unwrap_or(false) {
        return;
    }
    IN_MENU.with(|c| c.set(true));
    let items = menu_monitors();
    let h = hwnd();
    let cmd = tray::show_menu(h, &items);
    LAST_MENU.with(|l| l.set(Some(Instant::now())));
    IN_MENU.with(|c| c.set(false));
    // A menu built from a mock state must never run anything.
    let cmd = cmd.filter(|_| !ui::is_mock());
    match cmd {
        Some(c) => run_tray_cmd(c),
        None => draw::trim_working_set(),
    }
}

fn toggle_main() {
    let last = LAST_TOGGLE.with(|l| l.get());
    if last.map(|t| t.elapsed() < TRAY_DEBOUNCE).unwrap_or(false) {
        return;
    }
    LAST_TOGGLE.with(|l| l.set(Some(Instant::now())));
    if ui::main_window::is_open() {
        ui::main_window::request_close();
    } else {
        ui::main_window::open();
    }
}

/// Moves the cursor to a fixed point near the bottom-right of the primary work area (screenshots).
pub fn park_cursor_for_menu() {
    let mut wa = windows_sys::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    // SAFETY: valid RECT pointer.
    unsafe {
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut _ as *mut _, 0);
        // A bottom taskbar may be auto-hidden (work area = full screen, bar sliding in and out), so keep a
        // fixed distance from the bottom edge: the menu stays clear of the bar and of its hover zone.
        let bar = windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW(
            wide("Shell_TrayWnd").as_ptr(),
            std::ptr::null(),
        );
        let mut br = wa;
        let bar_at_bottom = !bar.is_null()
            && windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect(bar, &mut br) != 0
            && br.top > (wa.top + wa.bottom) / 2;
        let bottom = if bar_at_bottom { wa.bottom - 60 } else { wa.bottom };
        SetCursorPos(wa.right - 260, bottom - 12);
    }
}

/// Puts a window at the top of the z-order without stealing focus (foreground rules may refuse
/// `SetForegroundWindow`, which would leave a freshly opened window behind others).
pub fn raise(h: HWND) {
    if h.is_null() {
        return;
    }
    // SAFETY: valid window handle on this thread.
    unsafe {
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, flags);
        // With a screenshot stage up (itself topmost), stay topmost so nothing can slip in between.
        if windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW(
            wide("SpectrumStage").as_ptr(),
            std::ptr::null(),
        )
        .is_null()
        {
            SetWindowPos(h, HWND_NOTOPMOST, 0, 0, 0, 0, flags);
        }
    }
}

/// Queues a command and posts it to the message window, so it runs from the loop (modal menus included).
pub fn post_command(cmd: &str) {
    COMMANDS.with(|c| c.borrow_mut().push(cmd.to_string()));
    // SAFETY: plain post to our own window (a null handle is rejected by the OS).
    unsafe { PostMessageW(hwnd(), WM_RUN_CMD, 0, 0) };
}

/// Debug and IPC commands: `open main`, `open main-hdr`, `open gamut`, `open traymenu`, `tray-cmd <id>`, `demo`.
pub fn run_command(cmd: &str) {
    let mut parts = cmd.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some("open"), Some("main")) => {
            ui::main_window::open();
            raise(ui::main_window::hwnd());
        }
        (Some("open"), Some("main-hdr")) => {
            // Screenshots: the main window on a mock state with HDR on (nothing is applied).
            ui::set_mock(Some(crate::debug::showcase_state()));
            ui::main_window::open();
            raise(ui::main_window::hwnd());
        }
        (Some("open"), Some("traymenu-showcase")) => {
            ui::set_mock(Some(crate::debug::showcase_state()));
            park_cursor_for_menu();
            show_tray_menu();
        }
        (Some("open"), Some("gamut-showcase")) => {
            crate::debug::open_gamut_showcase();
            raise(ui::gamut_window::hwnd());
        }
        (Some("open"), Some("profiles")) => {
            ui::profiles_window::open();
            raise(ui::profiles_window::hwnd());
        }
        (Some("open"), Some("profiles-showcase")) => {
            ui::profiles_window::open_showcase(crate::debug::showcase_profiles());
            raise(ui::profiles_window::hwnd());
        }
        (Some("open"), Some("gamut")) => {
            open_gamut();
            raise(ui::gamut_window::hwnd());
        }
        (Some("open"), Some("traymenu")) => {
            park_cursor_for_menu();
            show_tray_menu();
        }
        (Some("tray-cmd"), Some(id)) => {
            if let Some(c) = id.parse::<u32>().ok().and_then(tray::decode_cmd) {
                run_tray_cmd(c);
            }
        }
        (Some("demo"), _) => crate::debug::start_demo(hwnd()),
        _ => log(&format!("unknown command {cmd:?}")),
    }
}

fn exit_app() {
    let h = hwnd();
    tray::remove(h);
    ui::overlay::hide_all();
    ui::gamut_window::close();
    ui::profiles_window::close();
    ui::main_window::request_close();
    if !h.is_null() {
        // SAFETY: window created by this thread; WM_DESTROY posts the quit message.
        unsafe { DestroyWindow(h) };
    } else {
        // SAFETY: plain call.
        unsafe { PostQuitMessage(0) };
    }
}

// ---------------------------------------------------------------------------------------------
// Window and message loop
// ---------------------------------------------------------------------------------------------

/// How the message loop starts.
#[derive(Clone, Debug, Default)]
pub struct RunOpts {
    /// Put the icon in the notification area (otherwise this is a short-lived debug window).
    pub tray: bool,
    /// Open the main window right after startup.
    pub show_main: bool,
    /// A command (see [`run_command`]) to run once the loop is up.
    pub command: Option<String>,
}

fn lo(v: isize) -> u32 {
    (v as usize & 0xFFFF) as u32
}

/// Adds the tray icon; on failure (shell not ready yet) retries every 2 s, at most 15 times.
fn add_tray_with_retry(hwnd: HWND) {
    if tray::add(hwnd) {
        TRAY_TRIES.with(|t| t.set(0));
        // SAFETY: valid window; killing an unset timer is harmless.
        unsafe { KillTimer(hwnd, TIMER_TRAY_RETRY) };
        return;
    }
    let n = TRAY_TRIES.with(|t| t.get());
    if n >= TRAY_RETRY_MAX {
        // SAFETY: as above.
        unsafe { KillTimer(hwnd, TIMER_TRAY_RETRY) };
        log("could not add the tray icon; giving up");
        return;
    }
    TRAY_TRIES.with(|t| t.set(n + 1));
    log(&format!(
        "could not add the tray icon; retry {} of {TRAY_RETRY_MAX}",
        n + 1
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

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let show_msg = SHOW_MSG.with(|c| c.get());
    let taskbar_msg = TASKBAR_MSG.with(|c| c.get());
    if msg != 0 && msg == show_msg {
        ui::main_window::open();
        raise(ui::main_window::hwnd());
        return 0;
    }
    if msg != 0 && msg == taskbar_msg {
        if TRAY_MODE.with(|t| t.get()) {
            TRAY_TRIES.with(|t| t.set(0));
            add_tray_with_retry(hwnd);
        }
        return 0;
    }
    match msg {
        WM_TRAY => {
            match lo(lp) {
                WM_LBUTTONUP | NIN_SELECT | NIN_KEYSELECT => toggle_main(),
                // v4 sends WM_CONTEXTMENU for a right click and for Shift+F10 / the menu key.
                WM_CONTEXTMENU => show_tray_menu(),
                _ => {}
            }
            0
        }
        WM_STARTUP => {
            let opts = OPTS.with(|o| o.borrow_mut().take()).unwrap_or_default();
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
            0
        }
        WM_RUN_CMD => {
            let cmd = COMMANDS.with(|c| {
                let mut c = c.borrow_mut();
                if c.is_empty() {
                    None
                } else {
                    Some(c.remove(0))
                }
            });
            if let Some(c) = cmd {
                run_command(&c);
            }
            0
        }
        WM_COPYDATA => {
            let cds = lp as *const COPYDATASTRUCT;
            if !cds.is_null() && (*cds).dwData == COPYDATA_MAGIC && !(*cds).lpData.is_null() {
                let len = ((*cds).cbData as usize).min(256);
                let bytes = std::slice::from_raw_parts((*cds).lpData as *const u8, len);
                let text = String::from_utf8_lossy(bytes).trim_end_matches('\0').to_string();
                COMMANDS.with(|c| c.borrow_mut().push(text));
                // Handled from a posted message so the sender is not blocked by a modal menu.
                PostMessageW(hwnd, WM_RUN_CMD, 0, 0);
                return 1;
            }
            0
        }
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
            // A string lParam only comes with wParam 0; anything else may not be a pointer.
            if wp == 0 && lp != 0 {
                let p = lp as *const u16;
                let mut n = 0;
                while n < 64 && *p.add(n) != 0 {
                    n += 1;
                }
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
                if s == "ImmersiveColorSet" {
                    draw::invalidate_theme();
                }
            }
            0
        }
        WM_TIMER => {
            if wp == TIMER_RECONCILE {
                KillTimer(hwnd, TIMER_RECONCILE);
                let lines = reconcile();
                if !lines.is_empty() {
                    log(&format!("display change: {} reconcile action(s)", lines.len()));
                }
                finish();
                draw::trim_working_set();
            } else if wp == TIMER_TRAY_RETRY {
                add_tray_with_retry(hwnd);
            } else if wp >= TIMER_DEBUG_BASE {
                crate::debug::on_timer(hwnd, wp);
            }
            0
        }
        WM_CLOSE => {
            exit_app();
            0
        }
        WM_DESTROY => {
            HWND_CELL.with(|c| c.set(0));
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
    HWND_CELL.with(|c| c.set(h as isize));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn used_by_text_lists_monitors() {
        assert_eq!(used_by_text(&[]), "Not in use");
        assert_eq!(used_by_text(&[(1, true)]), "Used by monitor 1 (default)");
        assert_eq!(
            used_by_text(&[(1, true), (2, false)]),
            "Used by monitors 1 (default), 2"
        );
    }

    fn ui_mon() -> UiMonitor {
        UiMonitor {
            num: 1,
            rect: Rect::default(),
            accent: 0,
            name: "M".into(),
            width: 0,
            height: 0,
            hdr_supported: true,
            hdr_on: true,
            sdr_options: Vec::new(),
            sdr_current: None,
            has_edid: true,
            hdr: HdrParams {
                min_nits: 0.0,
                peak_nits: 1000.0,
                full_frame_nits: 400.0,
                saturation: 0.0,
            },
            hdr_custom_active: false,
            sdr_white_nits: Some(240.0),
            hdr_profile: HdrProfile::None,
            reported: None,
            edid_hdr: Some(crate::edid::HdrStatic {
                max_nits: Some(455.0),
                max_frame_avg_nits: Some(254.0),
                min_nits: Some(0.0),
                pq: true,
            }),
        }
    }

    #[test]
    fn hdr_info_lines() {
        let mut m = ui_mon();
        assert_eq!(
            m.hdr_profile_line().unwrap(),
            "HDR profile: none, using the monitor's EDID values"
        );
        assert_eq!(
            m.edid_hdr_line().unwrap(),
            "Monitor EDID: peak 455 \u{b7} full-frame 254 \u{b7} black 0 nits"
        );
        assert_eq!(m.reported_line().unwrap(), "Windows reports: unavailable");
        m.hdr_on = false;
        assert_eq!(
            m.reported_line().unwrap(),
            "Windows reports: unavailable while HDR is off"
        );
        m.hdr_profile = HdrProfile::Spectrum;
        assert_eq!(m.hdr_profile_line().unwrap(), "HDR profile: Spectrum");
        m.edid_hdr = None;
        assert_eq!(m.edid_hdr_line().unwrap(), "No HDR values in this monitor's EDID");
        m.hdr_supported = false;
        assert!(m.hdr_profile_line().is_none() && m.reported_line().is_none());
    }

    #[test]
    fn sdr_white_warning_uses_the_best_peak() {
        let mut m = ui_mon();
        assert_eq!(m.white_warning(372.0), None);
        assert_eq!(
            m.white_warning(460.0).unwrap(),
            "Above this monitor's 455-nit peak"
        );
        // The value Windows reports wins over the EDID.
        m.reported = Some(crate::dxgi::Reported {
            min_nits: 0.0,
            max_nits: 450.0,
            max_full_frame_nits: 250.0,
            color_space: 12,
            bits_per_color: 10,
            red: [0.0; 2],
            green: [0.0; 2],
            blue: [0.0; 2],
            white: [0.0; 2],
        });
        assert_eq!(
            m.white_warning(452.0).unwrap(),
            "Above this monitor's 450-nit peak"
        );
        m.reported = None;
        m.edid_hdr = None;
        assert_eq!(m.white_warning(480.0), None);
    }

    #[test]
    fn profile_tags() {
        assert_eq!(profile_kind(true, false, true, false), ProfileKind::Hdr);
        assert_eq!(profile_kind(true, true, false, true), ProfileKind::Sdr);
        assert_eq!(profile_kind(false, false, false, true), ProfileKind::Lut);
        assert_eq!(profile_kind(false, true, false, false), ProfileKind::Matrix);
    }

    #[test]
    fn edid_file_name_includes_the_monitor_key() {
        let a = edid_file_name("MAG 271QPX E2", "MSI-3CB0-0000012A", None);
        let b = edid_file_name("MAG 271QPX E2", "MSI-3CB0-0000012B", None);
        assert_eq!(a, "Spectrum MAG 271QPX E2 MSI-3CB0-0000012A EDID.icm");
        assert_ne!(a, b);
        // Config from an older version keeps its file name.
        let old = "Spectrum MAG 271QPX E2 EDID.icm";
        assert_eq!(edid_file_name("MAG 271QPX E2", "K-1", Some(old)), old);
        assert_eq!(
            edid_file_name("MAG 271QPX E2", "K-1", Some("other.icm")),
            "Spectrum MAG 271QPX E2 K-1 EDID.icm"
        );
    }
}
