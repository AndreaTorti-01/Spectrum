//! The UI contract types and [`snapshot`], the state every window reads.

use super::files::{edid_option_file, is_ours, profile_label};
use super::{contains_ci, enumerate_cached, eq, monitor_cfg, monitor_name, Config, Monitor, MonitorCfg};
use crate::icc_read;
use crate::icc_write::HdrParams;
use crate::monitors::{self, Rect};
use crate::wcs;
use std::cell::RefCell;

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
    /// "Allow clipping above the monitor peak brightness" is on (the slider may go past the EDID cap).
    pub sdr_above_peak: bool,
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

    /// The EDID cap on the SDR content slider (see [`monitors::sdr_white_cap`]); `None` without an EDID max.
    pub fn sdr_cap(&self) -> Option<u32> {
        monitors::sdr_white_cap(self.edid_hdr.as_ref().and_then(|h| h.max_nits))
    }

    /// Top of the SDR content slider: the cap unless "Allow clipping above the monitor peak brightness" is on.
    pub fn sdr_max(&self) -> f64 {
        match self.sdr_cap() {
            Some(cap) if !self.sdr_above_peak => cap as f64,
            _ => monitors::SDR_WHITE_MAX_NITS,
        }
    }

    /// SDR content `white` is above the EDID cap (drawn in amber).
    pub fn sdr_over_cap(&self, white: f64) -> bool {
        self.sdr_cap().is_some_and(|cap| white > cap as f64)
    }

    /// Windows already shows SDR content above the EDID cap (set elsewhere, e.g. in Settings).
    fn sdr_white_above_cap(&self) -> bool {
        self.sdr_cap()
            .zip(self.sdr_white_nits)
            .is_some_and(|(cap, white)| monitors::snap_back(white.round() as u32, cap).is_some())
    }
}

#[derive(Clone, Debug)]
pub struct UiState {
    pub monitors: Vec<UiMonitor>,
    pub autostart: bool,
    /// HDR "Advanced" row expanded.
    pub advanced: bool,
    /// The last action's error, cleared by the next action that succeeds.
    pub error: Option<String>,
}

thread_local! {
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub(super) fn set_error(error: Option<String>) {
    LAST_ERROR.with(|c| *c.borrow_mut() = error);
}

/// SDR picker entries for one monitor: EDID profile (always, when the monitor has an EDID; picking it builds
/// the profile first), the six reference spaces, then any other profile associated with the monitor.
/// Second value: index of the current default.
pub(super) fn sdr_options(m: &Monitor, cfg: &MonitorCfg) -> (Vec<(String, String)>, Option<usize>) {
    // Spectrum's HDR profile is listed in the SDR associations too; it is not an SDR choice.
    let is_our_hdr = |f: &str| cfg.hdr_file.as_deref().is_some_and(|h| eq(h, f));
    let mut list: Vec<(String, String)> = Vec::new();
    if m.edid.is_some() {
        list.push((format!("{} (EDID)", monitor_name(m)), edid_option_file(m, cfg)));
    }
    for (_, label, file) in wcs::REFERENCE_PROFILES {
        list.push((label.into(), file.into()));
    }
    for f in wcs::associated(m, false).unwrap_or_default() {
        if is_our_hdr(&f) || list.iter().any(|(_, x)| eq(x, &f)) {
            continue;
        }
        list.push((profile_label(&f), f));
    }
    let mut current = None;
    if let Some(default) = wcs::default_profile(m, false) {
        match list.iter().position(|(_, x)| eq(x, &default)) {
            Some(i) => current = Some(i),
            None if !is_our_hdr(&default) => {
                list.push((profile_label(&default), default));
                current = Some(list.len() - 1);
            }
            None => {}
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
        if let Some(params) = hdr_from_default_profile(m) {
            return params;
        }
    }
    edid_hdr_defaults(m)
}

/// Min, peak and full-frame of the monitor's HDR default profile, when it has an MHC2 tag.
fn hdr_from_default_profile(m: &Monitor) -> Option<HdrParams> {
    let file = wcs::default_profile(m, true)?;
    let info = icc_read::parse(&wcs::read_profile(&file)?).ok()?;
    let mhc2 = info.mhc2?;
    let peak = if mhc2.peak_nits > 0.0 {
        mhc2.peak_nits
    } else {
        1000.0
    };
    Some(HdrParams {
        min_nits: mhc2.min_nits.max(0.0),
        peak_nits: peak,
        full_frame_nits: info.lumi.filter(|v| *v > 0.0).unwrap_or(peak * 0.4),
        saturation: 0.0,
    })
}

/// Default HDR values: the EDID HDR block (min / peak / full-frame), else 0 / 1000 / 400; saturation 0.
pub fn edid_hdr_defaults(m: &Monitor) -> HdrParams {
    hdr_defaults_from(m.edid.as_ref().and_then(|e| e.hdr.as_ref()))
}

pub fn hdr_defaults_from(h: Option<&crate::edid::HdrStatic>) -> HdrParams {
    let peak = h
        .and_then(|h| h.max_nits)
        .filter(|v| *v > 0.0)
        .unwrap_or(1000.0)
        .clamp(100.0, 4000.0);
    HdrParams {
        min_nits: h.and_then(|h| h.min_nits).unwrap_or(0.0).clamp(0.0, 0.5),
        peak_nits: peak,
        full_frame_nits: h
            .and_then(|h| h.max_frame_avg_nits)
            .filter(|v| *v > 0.0)
            .unwrap_or(400.0)
            .clamp(100.0, peak),
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
    let entry = monitor_cfg(cfg, m);
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
        // A display without HDR can still report the flag (a stale mode after a cable swap); never show it on.
        hdr_on: m.hdr_supported && m.hdr_enabled,
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
        sdr_above_peak: entry.sdr_above_peak.unwrap_or(false),
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

/// SDR content already above the EDID cap in Windows: turn "Allow clipping" on rather than lower the value.
/// Returns true when the config changed.
fn adopt_sdr_above_cap(mons: &[Monitor], ui: &mut [UiMonitor], cfg: &mut Config) -> bool {
    let mut changed = false;
    for (m, u) in mons.iter().zip(ui.iter_mut()) {
        if u.sdr_white_above_cap() && !u.sdr_above_peak {
            u.sdr_above_peak = true;
            let entry = cfg.monitor_mut(&m.key);
            entry.name = monitor_name(m);
            entry.sdr_above_peak = Some(true);
            changed = true;
        }
    }
    changed
}

pub fn snapshot() -> UiState {
    let mut cfg = Config::load();
    let mons = enumerate_cached();
    let mut monitors: Vec<UiMonitor> = mons.iter().map(|m| build_ui_monitor(m, &cfg)).collect();
    if adopt_sdr_above_cap(&mons, &mut monitors, &mut cfg) {
        cfg.save();
    }
    UiState {
        monitors,
        autostart: crate::autostart::autostart_enabled(),
        advanced: cfg.advanced,
        error: LAST_ERROR.with(|s| s.borrow().clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            sdr_above_peak: false,
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
    fn hdr_defaults_come_from_the_edid() {
        let m = ui_mon();
        let d = hdr_defaults_from(m.edid_hdr.as_ref());
        assert_eq!(
            (d.min_nits, d.peak_nits, d.full_frame_nits, d.saturation),
            (0.0, 455.0, 254.0, 0.0)
        );
        let d = hdr_defaults_from(None);
        assert_eq!(
            (d.min_nits, d.peak_nits, d.full_frame_nits, d.saturation),
            (0.0, 1000.0, 400.0, 0.0)
        );
    }

    #[test]
    fn hdr_defaults_clamp_odd_edid_values() {
        let h = crate::edid::HdrStatic {
            max_nits: Some(9000.0),
            max_frame_avg_nits: Some(50.0),
            min_nits: Some(2.0),
            pq: true,
        };
        let d = hdr_defaults_from(Some(&h));
        assert_eq!((d.min_nits, d.peak_nits, d.full_frame_nits), (0.5, 4000.0, 100.0));
        let zero = crate::edid::HdrStatic {
            max_nits: Some(0.0),
            max_frame_avg_nits: Some(0.0),
            min_nits: None,
            pq: true,
        };
        let d = hdr_defaults_from(Some(&zero));
        assert_eq!((d.min_nits, d.peak_nits, d.full_frame_nits), (0.0, 1000.0, 400.0));
    }

    #[test]
    fn sdr_cap_follows_the_edid_max() {
        let mut m = ui_mon();
        // EDID max 455 caps SDR content at 452; the reported max does not move the cap.
        assert_eq!(m.sdr_cap(), Some(452));
        assert_eq!(m.sdr_max(), 452.0);
        assert!(m.sdr_over_cap(460.0));
        assert!(!m.sdr_over_cap(452.0));
        m.sdr_above_peak = true;
        assert_eq!(m.sdr_max(), 480.0);
        m.edid_hdr = None;
        assert_eq!(m.sdr_cap(), None);
        assert_eq!(m.sdr_max(), 480.0);
        assert!(!m.sdr_over_cap(480.0));
    }

    #[test]
    fn effective_peak_prefers_what_windows_reports() {
        let mut m = ui_mon();
        assert_eq!(m.effective_peak(), Some(455.0));
        let mut reported = crate::showcase::state().monitors[0].reported.unwrap();
        reported.max_nits = 1000.0;
        m.reported = Some(reported);
        assert_eq!(m.effective_peak(), Some(1000.0));
        reported.max_nits = 0.0;
        m.reported = Some(reported);
        assert_eq!(m.effective_peak(), Some(455.0));
        m.edid_hdr = None;
        assert_eq!(m.effective_peak(), None);
    }

    #[test]
    fn sdr_white_above_the_cap_turns_clipping_on() {
        let mons = [super::super::tests::test_monitor(1)];
        let mut cfg = Config::default();
        let mut ui = [ui_mon()];
        assert!(!adopt_sdr_above_cap(&mons, &mut ui, &mut cfg));
        assert!(cfg.monitors.is_empty());
        ui[0].sdr_white_nits = Some(480.0);
        assert!(adopt_sdr_above_cap(&mons, &mut ui, &mut cfg));
        assert!(ui[0].sdr_above_peak);
        let entry = &cfg.monitors[&mons[0].key];
        assert_eq!(
            (entry.sdr_above_peak, entry.name.as_str()),
            (Some(true), "Monitor 1")
        );
        // Already on: nothing more to save.
        assert!(!adopt_sdr_above_cap(&mons, &mut ui, &mut cfg));
    }
}
