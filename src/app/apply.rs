//! The actions the windows send: apply flows, [`execute`] and [`dispatch`].

use super::files::{
    backup_has, edid_file_name, edid_option_file, ensure_file, hdr_file_name, hdr_profile_bytes, safe_name,
    user_error,
};
use super::snapshot::{edid_hdr_defaults, sdr_options, set_error};
use super::{
    cached_monitors, contains_ci, edid_peak_nits, enumerate_cached, eq, monitor_at, monitor_cfg, save_cfg,
    tray_mode, Config, Monitor,
};
use crate::color;
use crate::icc_write::{self, HdrParams};
use crate::monitors::{self, Rect};
use crate::ui::{self, draw};
use crate::util::{app_data_dir, log};
use crate::wcs;
use std::time::Duration;
use windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage;

#[derive(Clone, Debug)]
pub enum UiAction {
    SetSdr { mon: usize, idx: usize },
    ReadEdid { mon: usize },
    SetHdrEnabled { mon: usize, on: bool },
    ApplyHdr { mon: usize, params: HdrParams },
    ResetHdr { mon: usize },
    SetSdrWhite { mon: usize, nits: f64 },
    SetSdrAbovePeak { mon: usize, on: bool },
    SetAutostart(bool),
    SetAdvanced(bool),
    OpenGamut,
    OpenProfiles,
    DeleteProfile { file: String },
    Identify,
    CloseMain,
}

impl UiAction {
    /// Opens or closes a window without changing any setting: no error update, no monitor re-read.
    fn is_quiet(&self) -> bool {
        matches!(
            self,
            UiAction::CloseMain | UiAction::OpenGamut | UiAction::OpenProfiles | UiAction::Identify
        )
    }
}

/// SDR content brightness that "Restore recommended HDR settings" sets (capped at the monitor's peak).
const RECOMMENDED_SDR_WHITE_NITS: f64 = 240.0;

fn set_sdr(mon_idx: usize, option_idx: usize) -> Result<String, String> {
    let m = monitor_at(mon_idx)?;
    let entry = monitor_cfg(&Config::load(), &m);
    let (options, _) = sdr_options(&m, &entry);
    let (label, file) = options
        .get(option_idx)
        .cloned()
        .ok_or_else(|| "Couldn't find that profile. Reopen the window and try again.".to_string())?;
    let fail = |e: String| {
        log(&format!("set sdr: {file} on monitor {}: {e}", m.num));
        user_error(e, || {
            format!("Couldn't apply {label} to monitor {}. Try again.", m.num)
        })
    };
    if m.edid.is_some()
        && eq(&file, &edid_option_file(&m, &entry))
        && !wcs::profile_exists(&file)
        && !backup_has(&file)
    {
        // The EDID profile is not built yet: same flow as "Read from monitor".
        return read_edid(mon_idx);
    }
    ensure_file(&file).map_err(fail)?;
    wcs::ensure_user_scope(&m).map_err(fail)?;
    wcs::set_default(&m, &file, false).map_err(fail)?;
    save_cfg(&m, |e| e.sdr = Some(file.clone()));
    log(&format!("applied {file} to monitor {} ({})", m.num, m.key));
    Ok(format!("Applied {label} to monitor {}", m.num))
}

fn read_edid(mon_idx: usize) -> Result<String, String> {
    install_edid(mon_idx, true)
}

/// The monitor's EDID: the one read at enumeration, else the registry copy.
fn load_edid(m: &Monitor) -> Option<crate::edid::Edid> {
    match &m.edid {
        Some(e) => Some(e.clone()),
        None => monitors::read_edid_registry(&m.device_path).and_then(|raw| crate::edid::parse(&raw).ok()),
    }
}

/// Keeps a copy of the raw EDID in `%APPDATA%\Spectrum\edid` (for bug reports). Failures are only logged.
fn save_raw_edid(m: &Monitor, raw: &[u8]) {
    let dir = app_data_dir().join("edid");
    if std::fs::create_dir_all(&dir).is_ok() {
        if let Err(e) = std::fs::write(dir.join(format!("{}.bin", safe_name(&m.key))), raw) {
            log(&format!("read edid: saving raw EDID failed: {e}"));
        }
    }
}

/// Builds and installs the EDID profile; `set_default` false only associates it (first run).
pub(super) fn install_edid(mon_idx: usize, set_default: bool) -> Result<String, String> {
    let m = monitor_at(mon_idx)?;
    let fail = |e: &str| {
        log(&format!("read edid: monitor {}: {e}", m.num));
        user_error(e.to_string(), || {
            format!(
                "Couldn't read EDID from monitor {}. Reconnect the cable and try again.",
                m.num
            )
        })
    };
    let edid = load_edid(&m).ok_or_else(|| fail("no EDID in the registry"))?;
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
    let current = Config::load()
        .monitors
        .get(&m.key)
        .and_then(|e| e.edid_profile.clone());
    let file = edid_file_name(&name, &m.key, current.as_deref());
    let bytes = icc_write::build_sdr_profile(&format!("{name} (EDID)"), &edid.primaries, edid.trc());

    save_raw_edid(&m, &edid.raw);
    wcs::install_profile(&file, &bytes).map_err(|e| fail(&e))?;
    wcs::ensure_user_scope(&m).map_err(|e| fail(&e))?;
    if !set_default {
        wcs::associate(&m, &file, false, false).map_err(|e| fail(&e))?;
        save_cfg(&m, |e| e.edid_profile = Some(file.clone()));
        log(&format!(
            "first run: EDID profile {file} installed for monitor {}",
            m.num
        ));
        return Ok(format!("Installed EDID profile for monitor {}", m.num));
    }
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

/// Waits up to a second for monitor `mon_idx` to report the HDR state `on`.
fn wait_for_hdr(mon_idx: usize, on: bool) {
    for _ in 0..10 {
        let now = enumerate_cached().get(mon_idx).map(|m| m.hdr_enabled);
        if now == Some(on) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn require_hdr(m: &Monitor) -> Result<(), String> {
    if m.hdr_supported {
        Ok(())
    } else {
        Err(format!("Monitor {} doesn't support HDR.", m.num))
    }
}

fn set_hdr_enabled(mon_idx: usize, on: bool) -> Result<String, String> {
    let m = monitor_at(mon_idx)?;
    require_hdr(&m)?;
    if let Err(e) = monitors::set_hdr(&m, on) {
        log(&format!("set hdr {on} on monitor {}: {e}", m.num));
        return Err(format!(
            "Couldn't change HDR on monitor {}. Check Settings \u{203a} Display \u{203a} HDR.",
            m.num
        ));
    }
    wait_for_hdr(mon_idx, on);
    Ok(format!(
        "Turned HDR {} for monitor {}",
        if on { "on" } else { "off" },
        m.num
    ))
}

/// Writes the HDR profile under the name the previous one did not use (A/B), makes it the HDR default and
/// then removes the previous one, so Windows always sees a changed file.
fn apply_hdr(mon_idx: usize, params: HdrParams) -> Result<String, String> {
    let m = monitor_at(mon_idx)?;
    require_hdr(&m)?;
    let previous = Config::load()
        .monitors
        .get(&m.key)
        .and_then(|e| e.hdr_file.clone());
    let (a, b) = (hdr_file_name(&m, "A"), hdr_file_name(&m, "B"));
    let file = match &previous {
        Some(p) if eq(p, &a) => b,
        _ => a,
    };
    let fail = |e: String| {
        log(&format!("apply hdr on monitor {}: {e}", m.num));
        user_error(e, || {
            format!("Couldn't apply HDR profile to monitor {}. Try again.", m.num)
        })
    };
    wcs::install_profile(&file, &hdr_profile_bytes(&m, &params)).map_err(fail)?;
    if let Err(e) = wcs::ensure_user_scope(&m).and_then(|_| wcs::set_default(&m, &file, true)) {
        let _ = retire_hdr_profile(&m, &file);
        return Err(fail(e));
    }
    if let Some(p) = previous.filter(|p| !eq(p, &file)) {
        if let Err(e) = retire_hdr_profile(&m, &p) {
            log(&format!("apply hdr: deleting old {p}: {e}"));
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

/// Drops the HDR association of `file` and, once it is gone, deletes the file. `Err` only for a failed
/// delete; a file that is still associated stays in place.
pub(super) fn retire_hdr_profile(m: &Monitor, file: &str) -> Result<(), String> {
    if drop_hdr_profile(m, file) {
        wcs::delete_profile(file)
    } else {
        Ok(())
    }
}

/// Windows' own "SDR content brightness" (not part of the MHC2 profile).
fn set_sdr_white(mon_idx: usize, nits: f64) -> Result<String, String> {
    let m = monitor_at(mon_idx)?;
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

/// The "Allow clipping above the monitor peak brightness" toggle (off by default). Turning it off snaps SDR
/// content down to the cap through the same path as the slider.
fn set_sdr_above_peak(mon_idx: usize, on: bool) -> Result<String, String> {
    let m = monitor_at(mon_idx)?;
    let cap = monitors::sdr_white_cap(edid_peak_nits(&m))
        .ok_or_else(|| format!("Monitor {} reports no peak brightness to cap at.", m.num))?;
    save_cfg(&m, |e| e.sdr_above_peak = Some(on));
    log(&format!(
        "allow sdr content above the peak on monitor {}: {on}",
        m.num
    ));
    if on {
        return Ok(String::new());
    }
    let current = monitors::sdr_white_nits(&m).map(|n| n.round() as u32);
    match current.and_then(|c| monitors::snap_back(c, cap)) {
        Some(to) => set_sdr_white(mon_idx, to as f64),
        None => Ok(String::new()),
    }
}

/// "Restore recommended HDR settings": writes the Spectrum HDR profile again with the EDID values and keeps it as
/// the HDR default (same A/B alternation as `apply_hdr`), turns "Allow clipping above the monitor peak
/// brightness" off and sets SDR content to 240 nits (or the monitor's peak, if that is lower).
fn reset_hdr(mon_idx: usize) -> Result<String, String> {
    let m = monitor_at(mon_idx)?;
    apply_hdr(mon_idx, edid_hdr_defaults(&m))?;
    save_cfg(&m, |e| e.sdr_above_peak = Some(false));
    let cap = monitors::sdr_white_cap(edid_peak_nits(&m)).map_or(f64::INFINITY, |c| c as f64);
    set_sdr_white(mon_idx, RECOMMENDED_SDR_WHITE_NITS.min(cap))?;
    log(&format!("HDR defaults restored on monitor {}", m.num));
    Ok(format!("Restored HDR defaults on monitor {}", m.num))
}

fn set_autostart(on: bool) -> Result<String, String> {
    crate::autostart::set_autostart(on)
        .map(|_| String::new())
        .map_err(|e| {
            log(&format!("autostart {on}: {e}"));
            "Couldn't change the startup setting. Try again.".to_string()
        })
}

fn set_advanced(on: bool) {
    let mut cfg = Config::load();
    cfg.advanced = on;
    cfg.save();
}

/// Opens the gamut window on the SDR default of the monitor selected in the main window.
pub(super) fn open_gamut() {
    let mons = cached_monitors();
    let mon_idx = if ui::main_window::is_open() {
        ui::main_window::selected()
    } else {
        0
    };
    let file = mons.get(mon_idx).and_then(|m| wcs::default_profile(m, false));
    ui::gamut_window::open(file, mon_idx);
}

fn identify() {
    let rects: Vec<(Rect, u32, u32)> = enumerate_cached()
        .iter()
        .map(|m| (m.rect, m.num, m.accent))
        .collect();
    ui::overlay::identify_all(&rects);
}

/// The main window closed: hide the overlays, free memory, and end a debug run once no window is left.
fn close_main() {
    ui::overlay::hide_all();
    draw::trim_working_set();
    if !tray_mode() && !ui::gamut_window::is_open() && !ui::profiles_window::is_open() {
        // SAFETY: plain call on the UI thread.
        unsafe { PostQuitMessage(0) };
    }
}

/// Runs one UI action. `Ok` carries a past-tense report for headless callers (may be empty).
pub fn execute(action: UiAction) -> Result<String, String> {
    let done = |run: fn()| -> Result<String, String> {
        run();
        Ok(String::new())
    };
    match action {
        UiAction::SetSdr { mon, idx } => set_sdr(mon, idx),
        UiAction::ReadEdid { mon } => read_edid(mon),
        UiAction::SetHdrEnabled { mon, on } => set_hdr_enabled(mon, on),
        UiAction::ApplyHdr { mon, params } => apply_hdr(mon, params),
        UiAction::ResetHdr { mon } => reset_hdr(mon),
        UiAction::SetSdrWhite { mon, nits } => set_sdr_white(mon, nits),
        UiAction::SetSdrAbovePeak { mon, on } => set_sdr_above_peak(mon, on),
        UiAction::SetAutostart(on) => set_autostart(on),
        UiAction::SetAdvanced(on) => {
            set_advanced(on);
            Ok(String::new())
        }
        UiAction::OpenGamut => done(open_gamut),
        UiAction::OpenProfiles => done(ui::profiles_window::open),
        UiAction::DeleteProfile { file } => super::profiles::delete_profile(&file),
        UiAction::Identify => done(identify),
        UiAction::CloseMain => done(close_main),
    }
}

/// Synchronous; the UI calls `snapshot()` again afterwards.
pub fn dispatch(action: UiAction) {
    let _ = dispatch_result(action);
}

/// Same as [`dispatch`] (last error and monitor cache included), but also returns the outcome so
/// headless callers can report it.
pub fn dispatch_result(action: UiAction) -> Result<String, String> {
    let quiet = action.is_quiet();
    let result = execute(action);
    match &result {
        Ok(_) if !quiet => set_error(None),
        Ok(_) => {}
        Err(msg) => set_error(Some(msg.clone())),
    }
    if !quiet {
        // A changed default or HDR state may move other monitors' options too.
        enumerate_cached();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_actions_are_quiet() {
        for a in [
            UiAction::CloseMain,
            UiAction::OpenGamut,
            UiAction::OpenProfiles,
            UiAction::Identify,
        ] {
            assert!(a.is_quiet(), "{a:?}");
        }
        for a in [
            UiAction::SetSdr { mon: 0, idx: 0 },
            UiAction::SetAdvanced(true),
            UiAction::DeleteProfile { file: "x.icm".into() },
            UiAction::ResetHdr { mon: 0 },
        ] {
            assert!(!a.is_quiet(), "{a:?}");
        }
    }

    #[test]
    fn hdr_actions_need_an_hdr_display() {
        let mut m = super::super::tests::test_monitor(2);
        assert_eq!(require_hdr(&m), Err("Monitor 2 doesn't support HDR.".to_string()));
        m.hdr_supported = true;
        assert_eq!(require_hdr(&m), Ok(()));
    }
}
