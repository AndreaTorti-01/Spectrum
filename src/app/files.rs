//! Profile files: Spectrum's file names, the ICC descriptions shown as labels, and restoring a missing file.

use super::{eq, monitor_name, Monitor, MonitorCfg};
use crate::icc_read;
use crate::wcs;
use std::cell::RefCell;
use std::collections::HashMap;

type ProfileStamp = (u64, Option<std::time::SystemTime>);

thread_local! {
    /// Profile description cache: file -> (stamp, label), so snapshots don't re-read profile files.
    static LABELS: RefCell<HashMap<String, (ProfileStamp, String)>> = RefCell::new(HashMap::new());
}

/// Spectrum's own profiles all start with "Spectrum ".
pub(super) fn is_ours(file: &str) -> bool {
    file.to_ascii_lowercase().starts_with("spectrum ")
}

/// Characters that are safe in a file name.
pub(super) fn safe_name(s: &str) -> String {
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

/// "Spectrum <name> <key> EDID.icm": the key keeps two identical monitors apart. A config that still
/// holds the older "Spectrum <name> EDID.icm" keeps using it.
pub(super) fn edid_file_name(name: &str, key: &str, current: Option<&str>) -> String {
    let old = format!("Spectrum {} EDID.icm", safe_name(name));
    match current {
        Some(c) if eq(c, &old) => old,
        _ => format!("Spectrum {} {} EDID.icm", safe_name(name), safe_name(key)),
    }
}

/// File name of the monitor's EDID profile, built or not (the configured one, else the default name).
pub(super) fn edid_option_file(m: &Monitor, cfg: &MonitorCfg) -> String {
    edid_file_name(&monitor_name(m), &m.key, cfg.edid_profile.as_deref())
}

/// "Spectrum HDR <key> A.icc" or "... B.icc": applying alternates between the two names.
pub(super) fn hdr_file_name(m: &Monitor, suffix: &str) -> String {
    format!("Spectrum HDR {} {suffix}.icc", safe_name(&m.key))
}

/// The HDR (MHC2) profile Spectrum writes for `m` with `params`.
pub(super) fn hdr_profile_bytes(m: &Monitor, params: &crate::icc_write::HdrParams) -> Vec<u8> {
    let desc = format!("Spectrum HDR {}", monitor_name(m));
    crate::icc_write::build_hdr_profile(&desc, &super::hdr_prims(m), params)
}

fn reference_id(file: &str) -> Option<&'static str> {
    wcs::REFERENCE_PROFILES
        .iter()
        .find(|(_, _, f)| eq(f, file))
        .map(|(id, _, _)| *id)
}

pub(super) fn backup_has(file: &str) -> bool {
    wcs::backup_dir().join(file).is_file()
}

/// Makes sure a profile file exists in the color folder: restores it from the backup or regenerates a
/// reference space.
pub(super) fn ensure_file(file: &str) -> Result<(), String> {
    if wcs::profile_exists(file) || wcs::restore_from_backup(file) {
        return Ok(());
    }
    if let Some(id) = reference_id(file) {
        wcs::ensure_reference_profile(id)?;
        return Ok(());
    }
    Err(format!("Profile {file} is missing and has no backup"))
}

/// Reads the ICC description of an installed profile, falling back to the file stem.
pub(super) fn profile_label(file: &str) -> String {
    let stamp = wcs::profile_stamp(file);
    if let Some(stamp) = stamp {
        let hit = LABELS.with(|l| {
            l.borrow()
                .get(file)
                .filter(|(s, _)| *s == stamp)
                .map(|(_, v)| v.clone())
        });
        if let Some(label) = hit {
            return label;
        }
    }
    let label = read_label(file);
    if let Some(stamp) = stamp {
        LABELS.with(|l| l.borrow_mut().insert(file.to_string(), (stamp, label.clone())));
    }
    label
}

/// Drops the cached label of a deleted profile.
pub(super) fn forget_label(file: &str) {
    LABELS.with(|l| l.borrow_mut().remove(file));
}

fn read_label(file: &str) -> String {
    let desc = wcs::read_profile(file)
        .and_then(|bytes| icc_read::parse(&bytes).ok())
        .map(|info| info.desc.trim().to_string())
        .filter(|d| !d.is_empty());
    desc.unwrap_or_else(|| file_stem(file))
}

/// `file` without its extension.
fn file_stem(file: &str) -> String {
    file.rsplit_once('.')
        .map(|(stem, _)| stem.to_string())
        .unwrap_or_else(|| file.to_string())
}

/// The user-facing text for a failed profile operation: a "kept" notice from `wcs` passes through, any
/// other error becomes `fallback()` (the details only go to the log).
pub(super) fn user_error(e: String, fallback: impl FnOnce() -> String) -> String {
    if e.starts_with(wcs::KEPT_PREFIX) {
        e
    } else {
        fallback()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edid_file_name_includes_the_monitor_key() {
        let a = edid_file_name("QD-OLED 27", "SPC-0001-0000012A", None);
        let b = edid_file_name("QD-OLED 27", "SPC-0001-0000012B", None);
        assert_eq!(a, "Spectrum QD-OLED 27 SPC-0001-0000012A EDID.icm");
        assert_ne!(a, b);
        // Config from an older version keeps its file name.
        let old = "Spectrum QD-OLED 27 EDID.icm";
        assert_eq!(edid_file_name("QD-OLED 27", "K-1", Some(old)), old);
        assert_eq!(
            edid_file_name("QD-OLED 27", "K-1", Some("other.icm")),
            "Spectrum QD-OLED 27 K-1 EDID.icm"
        );
    }

    #[test]
    fn safe_names_keep_only_file_name_characters() {
        assert_eq!(safe_name("QD-OLED 27 (rev. 2)"), "QD-OLED 27 (rev. 2)");
        assert_eq!(safe_name("AT&T: \"pro\"/max"), "AT_T_ _pro__max");
        assert_eq!(safe_name("  ..name..  "), "name");
        assert_eq!(safe_name("\u{e9}cran"), "_cran");
    }

    #[test]
    fn spectrum_files_are_recognized_by_prefix() {
        assert!(is_ours("Spectrum Display P3.icm"));
        assert!(is_ours("spectrum hdr x a.icc"));
        assert!(!is_ours("SpectrumX.icm"));
        assert!(!is_ours("sRGB Color Space Profile.icm"));
    }

    #[test]
    fn hdr_files_alternate_between_two_names_per_monitor() {
        let m = super::super::tests::test_monitor(3);
        assert_eq!(hdr_file_name(&m, "A"), "Spectrum HDR TST-0000-00000003 A.icc");
        assert_eq!(hdr_file_name(&m, "B"), "Spectrum HDR TST-0000-00000003 B.icc");
        assert!(is_ours(&hdr_file_name(&m, "A")));
    }

    #[test]
    fn reference_files_map_to_their_ids() {
        for (id, _, file) in wcs::REFERENCE_PROFILES {
            assert_eq!(reference_id(&file.to_uppercase()), Some(id));
        }
        assert_eq!(reference_id("other.icm"), None);
    }

    #[test]
    fn stems_drop_the_last_extension() {
        assert_eq!(file_stem("Vendor sRGB.mode.icm"), "Vendor sRGB.mode");
        assert_eq!(file_stem("noext"), "noext");
    }

    #[test]
    fn kept_notices_pass_through_user_errors() {
        let kept = format!("{} something", wcs::KEPT_PREFIX);
        assert_eq!(user_error(kept.clone(), || "fallback".into()), kept);
        assert_eq!(user_error("os error 5".into(), || "fallback".into()), "fallback");
    }
}
