//! Application state and glue: UI snapshot/dispatch, apply flows, reconcile logic and the hidden
//! message-window loop.
//!
//! Everything runs on the UI thread. Shared state is tiny (the last error and a monitor cache);
//! the config is re-read from disk for every operation so a debug command run from a second process
//! never fights the tray instance's memory.
//!
//! - `snapshot`: the UI contract types and the state the windows read.
//! - `apply`: the actions the windows send ([`execute`], [`dispatch`]).
//! - `profiles`: the profiles window's rows and delete.
//! - `reconcile`: driver-update survival, first run, wipe and uninstall.
//! - `files`: profile file names, labels and restoring missing files.
//! - `commands`: tray menu, debug/IPC commands and exit.
//! - `window`: the hidden message window and its loop.

mod apply;
mod commands;
mod files;
mod profiles;
mod reconcile;
mod snapshot;
mod window;

pub use apply::{dispatch, dispatch_result, execute, UiAction};
pub use commands::{park_cursor_for_menu, post_command, raise, run_command, run_tray_cmd, show_tray_menu};
pub use profiles::{delete_profile, list_profiles, used_by_text, ProfileKind, ProfileRow};
pub use reconcile::{reconcile, remove_all_profiles, wipe_monitor};
pub use snapshot::{edid_hdr_defaults, hdr_defaults_from, snapshot, HdrProfile, UiMonitor, UiState};
pub use window::{run, run_tray, RunOpts};

use crate::color::{self, Primaries, BT709};
use crate::config::{Config, MonitorCfg};
use crate::monitors::{self, Monitor};
use std::cell::{Cell, RefCell};
use windows_sys::Win32::Foundation::HWND;

/// Window class of the hidden top-level window (message-only windows miss broadcast messages).
pub const WINDOW_CLASS: &str = "SpectrumMsgWnd";
/// Registered message that makes a running instance open its main window.
pub const SHOW_MESSAGE_NAME: &str = "Spectrum.Show";
/// `dwData` of the `WM_COPYDATA` messages a second instance sends for debug commands.
pub const COPYDATA_MAGIC: usize = 0x5350_4543;

thread_local! {
    /// Monitors from the most recent enumeration; used for cheap lookups (identify, the gamut window).
    static CACHE: RefCell<Vec<Monitor>> = const { RefCell::new(Vec::new()) };
    static HWND_CELL: Cell<isize> = const { Cell::new(0) };
    /// True when the tray icon is up; false for the short-lived debug windows (they quit with the UI).
    static TRAY_MODE: Cell<bool> = const { Cell::new(false) };
}

fn enumerate_cached() -> Vec<Monitor> {
    let mons = monitors::enumerate();
    CACHE.with(|c| *c.borrow_mut() = mons.clone());
    mons
}

fn cached_monitors() -> Vec<Monitor> {
    let cached = CACHE.with(|c| c.borrow().clone());
    if cached.is_empty() {
        enumerate_cached()
    } else {
        cached
    }
}

/// Fresh monitor list (also refreshes the cache).
pub fn current_monitors() -> Vec<Monitor> {
    enumerate_cached()
}

/// The hidden message window (null before `run` creates it and after it is destroyed).
pub fn hwnd() -> HWND {
    HWND_CELL.with(|c| c.get()) as HWND
}

fn set_hwnd(h: HWND) {
    HWND_CELL.with(|c| c.set(h as isize));
}

fn tray_mode() -> bool {
    TRAY_MODE.with(|t| t.get())
}

/// File names compare case-insensitively, as on NTFS.
fn eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn contains_ci(list: &[String], file: &str) -> bool {
    list.iter().any(|x| eq(x, file))
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

fn monitor_at(idx: usize) -> Result<Monitor, String> {
    enumerate_cached()
        .into_iter()
        .nth(idx)
        .ok_or_else(|| "Couldn't find that monitor. Reopen the window and try again.".to_string())
}

/// The config entry of `m`, or an empty one.
fn monitor_cfg(cfg: &Config, m: &Monitor) -> MonitorCfg {
    cfg.monitors.get(&m.key).cloned().unwrap_or_default()
}

/// Loads the config, updates the entry of `m` (and its display name) and saves it.
fn save_cfg(m: &Monitor, update: impl FnOnce(&mut MonitorCfg)) {
    let mut cfg = Config::load();
    let entry = cfg.monitor_mut(&m.key);
    entry.name = monitor_name(m);
    update(entry);
    cfg.save();
}

/// EDID primaries for the HDR profile; BT.709 when the EDID is missing or its chromaticities are bogus.
fn hdr_prims(m: &Monitor) -> Primaries {
    m.edid
        .as_ref()
        .map(|e| e.primaries)
        .filter(color::primaries_plausible)
        .unwrap_or(BT709)
}

/// Peak luminance of the EDID HDR static metadata block, if any.
fn edid_peak_nits(m: &Monitor) -> Option<f64> {
    m.edid
        .as_ref()
        .and_then(|e| e.hdr.as_ref())
        .and_then(|h| h.max_nits)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A monitor that exists only in memory (no EDID, no HDR).
    pub(crate) fn test_monitor(num: u32) -> Monitor {
        Monitor {
            num,
            gdi_name: format!("\\\\.\\DISPLAY{num}"),
            rect: monitors::Rect::default(),
            adapter: monitors::Luid { low: 0, high: 0 },
            source_id: 0,
            target_id: 0,
            friendly_name: String::new(),
            device_path: String::new(),
            edid: None,
            key: format!("TST-0000-{num:08X}"),
            hdr_supported: false,
            hdr_enabled: false,
            accent: 0,
        }
    }

    #[test]
    fn file_names_compare_case_insensitively() {
        assert!(eq("Spectrum HDR A.icc", "spectrum hdr a.ICC"));
        assert!(!eq("a.icc", "b.icc"));
        let list = vec!["sRGB Color Space Profile.icm".to_string()];
        assert!(contains_ci(&list, "SRGB COLOR SPACE PROFILE.ICM"));
        assert!(!contains_ci(&list, "Spectrum Display P3.icm"));
    }

    #[test]
    fn monitor_names_fall_back_to_the_friendly_name_then_the_number() {
        let mut m = test_monitor(2);
        m.friendly_name = "  ".into();
        assert_eq!(monitor_name(&m), "Monitor 2");
        m.friendly_name = "DELL U2720Q".into();
        assert_eq!(monitor_name(&m), "DELL U2720Q");
        m.edid = Some(crate::showcase::monitor_edid());
        assert_eq!(monitor_name(&m), "QD-OLED 27");
        assert_eq!(edid_peak_nits(&m).map(f64::round), Some(456.0));
    }

    #[test]
    fn hdr_primaries_fall_back_to_bt709() {
        let mut m = test_monitor(1);
        assert_eq!(hdr_prims(&m), BT709);
        let e = crate::showcase::monitor_edid();
        m.edid = Some(e.clone());
        assert_eq!(hdr_prims(&m), e.primaries);
    }
}
