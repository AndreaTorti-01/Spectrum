//! The profiles window: every profile in the color folder with its type and users, and deleting one.

use super::files::{forget_label, profile_label};
use super::{contains_ci, enumerate_cached, eq, Config, Monitor};
use crate::icc_read;
use crate::util::log;
use crate::wcs;
use std::collections::HashMap;

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
        .map(|(num, is_default)| {
            if *is_default {
                format!("{num} (default)")
            } else {
                num.to_string()
            }
        })
        .collect();
    format!(
        "Used by monitor{} {}",
        if users.len() > 1 { "s" } else { "" },
        list.join(", ")
    )
}

/// How one monitor uses a profile.
#[derive(Debug, PartialEq)]
struct ProfileUse {
    num: u32,
    sdr: bool,
    hdr: bool,
    default: bool,
}

/// Records that monitor `num` lists a profile (in its HDR list when `hdr`), merging with an earlier entry.
fn add_use(list: &mut Vec<ProfileUse>, num: u32, hdr: bool, is_default: bool) {
    match list.iter_mut().find(|u| u.num == num) {
        Some(u) => {
            u.sdr |= !hdr;
            u.hdr |= hdr;
            u.default |= is_default;
        }
        None => list.push(ProfileUse {
            num,
            sdr: !hdr,
            hdr,
            default: is_default,
        }),
    }
}

/// Lowercased file name -> the monitors that list it, from every monitor's SDR and HDR associations.
fn collect_uses(mons: &[Monitor]) -> HashMap<String, Vec<ProfileUse>> {
    let mut uses: HashMap<String, Vec<ProfileUse>> = HashMap::new();
    for m in mons {
        for hdr in [false, true] {
            let default = wcs::default_profile(m, hdr);
            for f in wcs::associated(m, hdr).unwrap_or_default() {
                let is_default = default.as_deref().is_some_and(|d| eq(d, &f));
                add_use(
                    uses.entry(f.to_ascii_lowercase()).or_default(),
                    m.num,
                    hdr,
                    is_default,
                );
            }
        }
    }
    uses
}

/// The row for `file`; `None` for print, scanner and abstract profiles (an unreadable file stays listed).
fn profile_row(file: String, uses: Option<&Vec<ProfileUse>>) -> Option<ProfileRow> {
    let in_hdr = uses.is_some_and(|l| l.iter().any(|x| x.hdr));
    let in_sdr = uses.is_some_and(|l| l.iter().any(|x| x.sdr));
    let info = wcs::read_profile(&file).and_then(|b| icc_read::parse(&b).ok());
    if info.as_ref().is_some_and(|i| !i.is_display_rgb()) {
        return None;
    }
    let kind = match &info {
        Some(i) => profile_kind(i.mhc2.is_some(), i.is_matrix_trc, in_hdr, in_sdr),
        None => profile_kind(false, true, in_hdr, in_sdr),
    };
    let users: Vec<(u32, bool)> = uses
        .map(|l| l.iter().map(|x| (x.num, x.default)).collect())
        .unwrap_or_default();
    Some(ProfileRow {
        name: profile_label(&file),
        kind,
        used: used_by_text(&users),
        deletable: wcs::can_delete(&file),
        file,
    })
}

/// Every .icc/.icm in the color folder with its type, the monitors using it and whether it can be deleted.
pub fn list_profiles() -> Vec<ProfileRow> {
    let uses = collect_uses(&enumerate_cached());
    wcs::list_installed()
        .into_iter()
        .filter_map(|file| {
            let u = uses.get(&file.to_ascii_lowercase());
            profile_row(file, u)
        })
        .collect()
}

/// Disassociates `file` from every monitor that lists it (or may list it, when the list can't be read).
fn remove_from_monitors(file: &str, label: &str) -> Result<(), String> {
    let listed = |m: &Monitor, hdr: bool| {
        wcs::associated(m, hdr)
            .map(|l| contains_ci(&l, file))
            .unwrap_or(true)
    };
    for m in &enumerate_cached() {
        for hdr in [false, true] {
            if !listed(m, hdr) {
                continue;
            }
            // A failed call is fine when the association is gone anyway.
            if let Err(e) = wcs::disassociate(m, file, hdr) {
                if listed(m, hdr) {
                    log(&format!("delete profile: {file} on monitor {}: {e}", m.num));
                    return Err(format!(
                        "Couldn't remove {label} from monitor {}. Try again.",
                        m.num
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Removes a profile from the system: disassociates it from every monitor (SDR and HDR), deletes the file
/// and Spectrum's backup copy, and clears the config fields that name it so reconcile leaves it gone.
pub fn delete_profile(file: &str) -> Result<String, String> {
    if let Err(why) = wcs::can_delete(file) {
        return Err(format!("Can't delete {file}. {}", why.hint()));
    }
    let label = profile_label(file);
    remove_from_monitors(file, &label)?;
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
    forget_label(file);
    log(&format!("deleted profile {file}"));
    Ok(format!("Deleted {label}"))
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

    #[test]
    fn profile_tags() {
        assert_eq!(profile_kind(true, false, true, false), ProfileKind::Hdr);
        assert_eq!(profile_kind(true, true, false, true), ProfileKind::Sdr);
        assert_eq!(profile_kind(false, false, false, true), ProfileKind::Lut);
        assert_eq!(profile_kind(false, true, false, false), ProfileKind::Matrix);
        // An MHC2 profile that only sits in an SDR list is not tagged HDR.
        assert_eq!(profile_kind(true, true, false, false), ProfileKind::Matrix);
    }

    #[test]
    fn kind_labels() {
        let labels: Vec<&str> = [
            ProfileKind::Sdr,
            ProfileKind::Hdr,
            ProfileKind::Lut,
            ProfileKind::Matrix,
        ]
        .map(ProfileKind::label)
        .to_vec();
        assert_eq!(labels, ["SDR", "HDR", "LUT", "Matrix"]);
    }

    #[test]
    fn uses_merge_per_monitor() {
        let mut list = Vec::new();
        add_use(&mut list, 1, false, true);
        add_use(&mut list, 1, true, false);
        add_use(&mut list, 2, true, false);
        assert_eq!(
            list,
            [
                ProfileUse {
                    num: 1,
                    sdr: true,
                    hdr: true,
                    default: true
                },
                ProfileUse {
                    num: 2,
                    sdr: false,
                    hdr: true,
                    default: false
                },
            ]
        );
    }
}
