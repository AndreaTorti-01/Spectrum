//! The checklist: the six reference spaces and the installed (or showcase / mock) profiles, with the
//! texts of the info block.

use super::{Gamut, Row};
use crate::color::{self, Primaries, Xy, STANDARD_SPACES};
use std::cell::RefCell;

/// Triangle colors of installed profiles, in list order.
const PROFILE_COLORS: [u32; 8] = [
    0xFFFFFF, 0xB57BFF, 0x4D9DE0, 0xFF9F43, 0x00E5FF, 0xFF4D8D, 0xC6F135, 0xA0522D,
];

/// One checklist row's data.
pub(super) struct Entry {
    pub name: String,
    /// Right-aligned dim tag in the list.
    pub tag: String,
    pub detail: String,
    pub prims: Option<Primaries>,
    pub color: u32,
    pub checked: bool,
    /// One of the six standard spaces (always listed first).
    pub reference: bool,
}

/// A profile as the window reads it: (file, name, primaries when matrix/TRC, tag).
type ProfileInfo = (String, String, Option<Primaries>, String);

/// Screenshot request: what to tick, and an in-memory entry for the monitor's EDID primaries when the
/// profile file is not installed (nothing is written anywhere).
pub(super) struct Showcase {
    pub file: String,
    pub label: String,
    pub prims: Option<Primaries>,
}

thread_local! {
    pub(super) static SHOWCASE: RefCell<Option<Showcase>> = const { RefCell::new(None) };
}

/// "Adobe RGB (1998)" -> "Adobe RGB".
pub(super) fn short_name(n: &str) -> &str {
    n.split(" (").next().unwrap_or(n)
}

/// Index of the standard space whose reference profile is `file` (`wcs::REFERENCE_PROFILES` and
/// `STANDARD_SPACES` share the order).
fn reference_file_id(file: &str) -> Option<usize> {
    crate::wcs::REFERENCE_PROFILES
        .iter()
        .position(|(_, _, f)| f.eq_ignore_ascii_case(file))
}

/// Spectrum's EDID profiles are named "Spectrum ... EDID.icm".
fn is_edid_profile(file: &str) -> bool {
    file.starts_with("Spectrum") && file.ends_with("EDID.icm")
}

/// List order: Spectrum's EDID profile, then SDR-associated, then other plottable, then HDR, then not
/// plottable.
fn sort_rank(p: &ProfileInfo) -> u8 {
    if p.2.is_none() {
        return 4;
    }
    match p.3.as_str() {
        "Spectrum" => 0,
        "SDR" => 1,
        "HDR" => 3,
        _ => 2,
    }
}

/// The entry for an installed profile, drawn in `color`.
fn profile_entry(file: &str, name: String, prims: Option<Primaries>, tag: String, color: u32) -> Entry {
    let plottable = prims.is_some();
    Entry {
        name,
        tag: if plottable {
            tag
        } else {
            "not plottable".to_string()
        },
        detail: if plottable && is_edid_profile(file) {
            "Built by Spectrum from the monitor's EDID".to_string()
        } else if plottable {
            format!("{file} \u{b7} matrix/TRC profile")
        } else {
            format!("{file} \u{b7} LUT-based profile, no triangle to plot")
        },
        prims,
        color,
        checked: false,
        reference: false,
    }
}

/// The entries, and the index of the one for `selected_file` (the monitor's current profile) if listed.
pub(super) fn build_entries(selected_file: Option<&str>, mon: usize) -> (Vec<Entry>, Option<usize>) {
    let mut entries: Vec<Entry> = STANDARD_SPACES
        .iter()
        .map(|s| Entry {
            name: s.name.to_string(),
            tag: String::new(),
            detail: s.description.to_string(),
            prims: Some(s.prims),
            color: s.color,
            checked: false,
            reference: true,
        })
        .collect();
    // sRGB is always ticked by default.
    entries[0].checked = true;
    let showcase = SHOWCASE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|s| (s.file.clone(), s.label.clone(), s.prims))
    });
    let mut profiles = if showcase.is_some() {
        crate::showcase::gamut_profiles()
    } else if super::super::is_mock() {
        mock_profiles()
    } else {
        installed_profiles(mon)
    };
    profiles.sort_by_key(sort_rank);
    if let Some((file, label, prims)) = &showcase {
        // The monitor's own profile goes first; build it from the EDID when it is not installed.
        match profiles.iter().position(|p| p.0.eq_ignore_ascii_case(file)) {
            Some(i) => {
                let p = profiles.remove(i);
                profiles.insert(0, p);
            }
            None => profiles.insert(0, (file.clone(), label.clone(), *prims, "Spectrum".to_string())),
        }
        entries[1].checked = true; // Display P3
    }
    let mut monitor_entry = None;
    let mut color_idx = 0usize;
    for (file, name, prims, tag) in profiles {
        let is_selected = selected_file == Some(file.as_str());
        if let Some(i) = reference_file_id(&file) {
            if is_selected {
                entries[i].checked = true;
            }
            continue;
        }
        let color = PROFILE_COLORS[color_idx % PROFILE_COLORS.len()];
        color_idx += 1;
        let mut entry = profile_entry(&file, name, prims, tag, color);
        entry.checked = is_selected && entry.prims.is_some();
        if is_selected {
            monitor_entry = Some(entries.len());
        }
        entries.push(entry);
    }
    (entries, monitor_entry)
}

/// Every display profile in the color folder. Tags follow the associations of monitor `mon_idx` (0-based).
fn installed_profiles(mon_idx: usize) -> Vec<ProfileInfo> {
    let mut out = Vec::new();
    let mon = crate::app::current_monitors().into_iter().nth(mon_idx);
    let assoc = |hdr: bool| {
        mon.as_ref()
            .and_then(|m| crate::wcs::associated(m, hdr).ok())
            .unwrap_or_default()
    };
    let (sdr_assoc, hdr_assoc) = (assoc(false), assoc(true));
    for file in crate::wcs::list_installed() {
        let Some(bytes) = crate::wcs::read_profile(&file) else {
            continue;
        };
        let Ok(info) = crate::icc_read::parse(&bytes) else {
            continue;
        };
        if !info.is_display_rgb() {
            continue;
        }
        let name = if info.desc.trim().is_empty() {
            file.clone()
        } else {
            info.desc.clone()
        };
        let prims = if info.is_matrix_trc { info.primaries } else { None };
        // Tag by association with the current monitor, not by what is inside the file.
        let listed = |l: &[String]| l.iter().any(|f| f.eq_ignore_ascii_case(&file));
        let tag = if is_edid_profile(&file) {
            "Spectrum"
        } else if listed(&hdr_assoc) {
            "HDR"
        } else if listed(&sdr_assoc) {
            "SDR"
        } else {
            ""
        }
        .to_string();
        out.push((file, name, prims, tag));
    }
    out
}

/// Profiles for the `uipreview` mock state.
fn mock_profiles() -> Vec<ProfileInfo> {
    let xy = |x, y| Xy { x, y };
    let w = xy(0.3127, 0.3290);
    let qd_oled = Primaries {
        r: xy(0.6855, 0.3047),
        g: xy(0.2373, 0.7041),
        b: xy(0.1396, 0.0508),
        w,
    };
    vec![
        (
            "Spectrum QD-OLED 27 EDID.icm".to_string(),
            "QD-OLED 27 (EDID)".to_string(),
            Some(qd_oled),
            "Spectrum".to_string(),
        ),
        (
            "Spectrum HDR 1.icc".to_string(),
            "QD-OLED 27 HDR 450".to_string(),
            Some(qd_oled),
            "HDR".to_string(),
        ),
        (
            "VENDOR_sRGB_mode.icm".to_string(),
            "Vendor sRGB mode".to_string(),
            Some(Primaries {
                r: xy(0.6406, 0.3301),
                g: xy(0.2998, 0.5996),
                b: xy(0.1504, 0.0596),
                w,
            }),
            String::new(),
        ),
        (
            "Office_CMYK_proof.icc".to_string(),
            "Office CMYK proof".to_string(),
            None,
            String::new(),
        ),
    ]
}

/// Index of the reference space closest to `p`: highest overlap (IoU); spaces within 0.005 of the best
/// (Display P3 and DCI-P3 share primaries) are separated by the white-point distance.
pub(super) fn closest_space(p: &Primaries) -> usize {
    let ious: Vec<f64> = STANDARD_SPACES.iter().map(|s| color::iou(p, &s.prims)).collect();
    let top = ious.iter().cloned().fold(f64::MIN, f64::max);
    let dist =
        |s: &color::StandardSpace| ((p.w.x - s.prims.w.x).powi(2) + (p.w.y - s.prims.w.y).powi(2)).sqrt();
    (0..STANDARD_SPACES.len())
        .filter(|&i| ious[i] >= top - 0.005)
        .min_by(|&a, &b| {
            dist(&STANDARD_SPACES[a])
                .partial_cmp(&dist(&STANDARD_SPACES[b]))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0)
}

impl Gamut {
    /// Name and description of row `i`. An installed profile also lists its primaries.
    pub(super) fn row_info(&self, i: usize) -> (String, String) {
        let entry = &self.entries[i];
        let mut body = entry.detail.clone();
        if let (false, Some(p)) = (entry.reference, entry.prims) {
            body.push_str(&format!(
                "\nR {:.3}, {:.3}  G {:.3}, {:.3}  B {:.3}, {:.3}",
                p.r.x, p.r.y, p.g.x, p.g.y, p.b.x, p.b.y
            ));
        }
        (entry.name.clone(), body)
    }

    /// Coverage of sRGB, Display P3 and Adobe RGB by installed profile `i`, and its closest reference space.
    pub(super) fn coverage_info(&self, i: usize) -> Option<(String, String)> {
        let entry = &self.entries[i];
        if entry.reference {
            return None;
        }
        let p = entry.prims?;
        let percent = |idx: usize| (color::coverage(&p, &STANDARD_SPACES[idx].prims) * 100.0).round() as i32;
        // No-break spaces inside each "Name NN%" unit: a line may only break between units.
        let unit = |idx: usize| {
            format!("{} {}%", short_name(STANDARD_SPACES[idx].name), percent(idx)).replace(' ', "\u{a0}")
        };
        Some((
            entry.name.clone(),
            format!(
                "{} \u{b7} {} \u{b7} {}\nClosest match: {}",
                unit(0),
                unit(1),
                unit(2),
                short_name(STANDARD_SPACES[closest_space(&p)].name),
            ),
        ))
    }

    /// Info block for the current state: the hovered row, else the first ticked installed profile.
    /// None when neither applies.
    pub(super) fn info(&self) -> Option<(String, String)> {
        match self.hover {
            Some(i) => Some(self.row_info(i)),
            None => {
                let i = self
                    .entries
                    .iter()
                    .position(|e| !e.reference && e.checked && e.prims.is_some())?;
                self.coverage_info(i)
            }
        }
    }

    /// Entry indices in list order.
    pub(super) fn entry_order(&self) -> Vec<usize> {
        self.rows
            .iter()
            .filter_map(|r| match r {
                Row::Entry(i) => Some(*i),
                Row::Header(_) => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_names() {
        assert_eq!(short_name("Adobe RGB (1998)"), "Adobe RGB");
        assert_eq!(short_name("sRGB"), "sRGB");
    }

    #[test]
    fn closest_match_breaks_primary_ties_by_white_point() {
        // The QD-OLED 27 EDID: Display P3 primaries with a near-D65 white. DCI-P3 shares the primaries
        // (a tie on overlap), but its white is far greener, so Display P3 must win.
        let xy = |x, y| Xy { x, y };
        let mag = Primaries {
            r: xy(0.6855, 0.3037),
            g: xy(0.2402, 0.7119),
            b: xy(0.1436, 0.0576),
            w: xy(0.3135, 0.3291),
        };
        assert_eq!(STANDARD_SPACES[closest_space(&mag)].name, "Display P3");
        let p3 = STANDARD_SPACES[1].prims;
        assert_eq!(STANDARD_SPACES[closest_space(&p3)].name, "Display P3");
        let dci = STANDARD_SPACES[5].prims;
        assert_eq!(STANDARD_SPACES[closest_space(&dci)].name, "DCI-P3");
    }

    #[test]
    fn reference_files_map() {
        assert_eq!(reference_file_id("sRGB Color Space Profile.icm"), Some(0));
        assert_eq!(reference_file_id("Spectrum Display P3.icm"), Some(1));
        assert_eq!(reference_file_id("spectrum dci-p3.ICM"), Some(5));
        assert_eq!(reference_file_id("foo.icc"), None);
        // The two lists must stay in the same order.
        for (i, (id, _, _)) in crate::wcs::REFERENCE_PROFILES.iter().enumerate() {
            assert_eq!(STANDARD_SPACES[i].id, *id);
        }
    }

    #[test]
    fn profiles_sort_edid_first_and_unplottable_last() {
        let p = |tag: &str, plottable: bool| {
            let prims = plottable.then_some(STANDARD_SPACES[0].prims);
            (String::new(), String::new(), prims, tag.to_string())
        };
        let ranks: Vec<u8> = [
            p("Spectrum", true),
            p("SDR", true),
            p("", true),
            p("HDR", true),
            p("Spectrum", false),
        ]
        .iter()
        .map(sort_rank)
        .collect();
        assert_eq!(ranks, [0, 1, 2, 3, 4]);
    }

    #[test]
    fn entry_texts_describe_the_profile() {
        let prims = Some(STANDARD_SPACES[1].prims);
        let edid = profile_entry("Spectrum X EDID.icm", "X".into(), prims, "Spectrum".into(), 1);
        assert_eq!(edid.detail, "Built by Spectrum from the monitor's EDID");
        let other = profile_entry("vendor.icm", "V".into(), prims, String::new(), 1);
        assert_eq!(other.detail, "vendor.icm \u{b7} matrix/TRC profile");
        let lut = profile_entry("lut.icc", "L".into(), None, "SDR".into(), 1);
        assert_eq!(lut.tag, "not plottable");
        assert!(lut.detail.ends_with("no triangle to plot"));
        assert!(!edid.reference && !edid.checked);
    }
}
