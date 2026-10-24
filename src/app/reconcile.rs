//! Driver-update survival ([`reconcile`]), the first run, and the debug wipe and uninstall helpers.

use super::apply::{install_edid, retire_hdr_profile};
use super::files::{ensure_file, hdr_file_name, hdr_profile_bytes, is_ours};
use super::{contains_ci, enumerate_cached, eq, monitor_at, monitor_name, Config, Monitor, MonitorCfg};
use crate::color;
use crate::monitors;
use crate::util::{app_data_dir, log};
use crate::wcs;

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
            entry.name = name;
            changed = true;
        }
        restore_sdr_files(entry, &tag, &mut out);
        // A driver update can switch "use my settings for this device" off while the list stays intact.
        if entry.sdr.is_some() || entry.hdr_file.is_some() {
            if let Err(e) = wcs::ensure_user_scope(m) {
                out.push(format!("{tag}: could not enable per-user profiles: {e}"));
            }
        }
        changed |= reconcile_sdr(m, entry, &tag, &mut out);
        if m.hdr_supported {
            changed |= reconcile_hdr(m, entry, &tag, &mut out);
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

/// Puts the EDID profile and the SDR choice back into the color folder when they went missing.
fn restore_sdr_files(entry: &MonitorCfg, tag: &str, out: &mut Vec<String>) {
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
}

/// Re-associates the SDR choice when its association was wiped; follows a default the user picked in
/// Windows. True when the config changed.
fn reconcile_sdr(m: &Monitor, entry: &mut MonitorCfg, tag: &str, out: &mut Vec<String>) -> bool {
    let Some(file) = entry.sdr.clone().filter(|f| wcs::profile_exists(f)) else {
        return false;
    };
    match wcs::associated(m, false) {
        // A failed query is not an empty list: leave the monitor alone.
        Err(e) => out.push(format!("{tag}: skipped SDR check, could not list profiles: {e}")),
        Ok(list) if !contains_ci(&list, &file) => {
            match wcs::ensure_user_scope(m).and_then(|_| wcs::set_default(m, &file, false)) {
                Ok(()) => out.push(format!("{tag}: re-associated {file} as the SDR default")),
                Err(e) => out.push(format!("{tag}: could not re-associate {file}: {e}")),
            }
        }
        Ok(_) => {
            if let Some(default) = wcs::default_profile(m, false).filter(|d| !eq(d, &file)) {
                out.push(format!(
                    "{tag}: default changed in Windows to {default}; config now follows it"
                ));
                entry.sdr = Some(default);
                return true;
            }
        }
    }
    false
}

/// Restores Spectrum's HDR profile file (from the backup, else rebuilt from the config values) and its
/// association. A different HDR default picked in Windows wins: Spectrum's profile is dropped. True when
/// the config changed.
fn reconcile_hdr(m: &Monitor, entry: &mut MonitorCfg, tag: &str, out: &mut Vec<String>) -> bool {
    let Some(file) = entry.hdr_file.clone() else {
        return false;
    };
    if !wcs::profile_exists(&file) && !wcs::restore_from_backup(&file) {
        if let Some(params) = entry.hdr {
            match wcs::install_profile(&file, &hdr_profile_bytes(m, &params)) {
                Ok(()) => out.push(format!("{tag}: rebuilt {file}")),
                Err(e) => out.push(format!("{tag}: could not rebuild {file}: {e}")),
            }
        }
    } else if !wcs::profile_exists(&file) {
        out.push(format!("{tag}: could not restore {file}"));
    }
    if !wcs::profile_exists(&file) {
        return false;
    }
    match wcs::associated(m, true) {
        Err(e) => out.push(format!("{tag}: skipped HDR check, could not list profiles: {e}")),
        Ok(list) if !contains_ci(&list, &file) => {
            match wcs::ensure_user_scope(m).and_then(|_| wcs::set_default(m, &file, true)) {
                Ok(()) => out.push(format!("{tag}: re-associated {file} as the HDR default")),
                Err(e) => out.push(format!("{tag}: could not re-associate {file}: {e}")),
            }
        }
        Ok(_) => {
            if let Some(default) = wcs::default_profile(m, true).filter(|d| !eq(d, &file)) {
                // The user picked another HDR profile: stop managing ours instead of fighting.
                out.push(format!(
                    "{tag}: HDR default changed in Windows to {default}; dropped {file}"
                ));
                let _ = retire_hdr_profile(m, &file);
                entry.hdr_file = None;
                entry.hdr = None;
                return true;
            }
        }
    }
    false
}

/// First-ever start (no config file): installs and associates the EDID profile of every monitor with a
/// valid EDID. The default profile stays as it is.
pub(super) fn first_run() {
    let count = enumerate_cached().len();
    for i in 0..count {
        let plausible = monitor_at(i)
            .map(|m| {
                m.edid
                    .as_ref()
                    .is_some_and(|e| color::primaries_plausible(&e.primaries))
            })
            .unwrap_or(false);
        if !plausible {
            continue;
        }
        match install_edid(i, false) {
            Ok(s) => log(&format!("first run: {s}")),
            Err(e) => log(&format!("first run: {e}")),
        }
    }
    // Marks the first run as done even when no monitor had an EDID.
    Config::load().save();
}

fn kind_name(hdr: bool) -> &'static str {
    if hdr {
        "HDR"
    } else {
        "SDR"
    }
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
        let kind = kind_name(hdr);
        let list = match wcs::associated(m, hdr) {
            Ok(l) => l,
            Err(e) => {
                out.push(format!("could not list {kind} profiles: {e}"));
                continue;
            }
        };
        for f in list.into_iter().filter(|f| is_ours(f)) {
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
    let mut files: Vec<String> = Vec::new();
    if hdr {
        files.push(hdr_file_name(m, "A"));
        files.push(hdr_file_name(m, "B"));
    } else {
        files.extend(
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
            if !contains_ci(&files, &f) {
                files.push(f);
            }
        }
    }
    files
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
    for f in wcs::list_installed().into_iter().filter(|f| is_ours(f)) {
        match wcs::delete_profile(&f) {
            Ok(()) => out.push(format!("deleted {f}")),
            Err(e) => out.push(format!("could not delete {f}: {e}")),
        }
    }
    let _ = std::fs::remove_dir_all(app_data_dir());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_files_cover_the_fixed_and_configured_names() {
        let m = super::super::tests::test_monitor(1);
        let mut cfg = Config::default();
        let sdr = known_files(&m, &cfg, false);
        assert_eq!(sdr.len(), wcs::REFERENCE_PROFILES.len() - 1);
        assert!(
            !contains_ci(&sdr, wcs::SRGB_INBOX),
            "the inbox sRGB is Windows' own"
        );
        assert_eq!(
            known_files(&m, &cfg, true),
            [hdr_file_name(&m, "A"), hdr_file_name(&m, "B")]
        );
        let entry = cfg.monitor_mut(&m.key);
        entry.edid_profile = Some("Spectrum Old EDID.icm".into());
        entry.sdr = Some("Vendor.icm".into());
        entry.hdr_file = Some(hdr_file_name(&m, "a").to_uppercase());
        let sdr = known_files(&m, &cfg, false);
        assert!(contains_ci(&sdr, "Spectrum Old EDID.icm"));
        assert!(!contains_ci(&sdr, "Vendor.icm"), "only Spectrum's own files");
        // The configured HDR name is already in the list (case-insensitively).
        assert_eq!(known_files(&m, &cfg, true).len(), 2);
    }

    #[test]
    fn kinds_are_named() {
        assert_eq!((kind_name(false), kind_name(true)), ("SDR", "HDR"));
    }
}
