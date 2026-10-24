//! Fixed data for the screenshot paths: `--render` and `--debug-open main-hdr | gamut-showcase |
//! profiles-showcase | traymenu-showcase`. The builders here read nothing from the machine, so the pictures
//! look the same everywhere. The monitor is a QD-OLED 27: its EDID is `tests/fixtures/edid_qdoled.bin`, and the
//! profile rows are made up to match it. Extend this module when the UI gains state.

use crate::app::{HdrProfile, ProfileKind, ProfileRow, UiMonitor, UiState};
use crate::color::{Primaries, STANDARD_SPACES};
use crate::dxgi::Reported;
use crate::edid::{parse as parse_edid, Edid};
use crate::icc_write::HdrParams;
use crate::monitors::{Rect, ACCENTS};
use crate::wcs::{Undeletable, REFERENCE_PROFILES};

const EDID_BIN: &[u8] = include_bytes!("../tests/fixtures/edid_qdoled.bin");

/// The monitor's EDID (QD-OLED 27, SPC-0001-00000000). The fixture is covered by a unit test.
pub fn monitor_edid() -> Edid {
    parse_edid(EDID_BIN).expect("the showcase EDID fixture parses")
}

/// SDR profile file the EDID profile is written as, and HDR profile file, for the showcase monitor.
fn edid_file(e: &Edid) -> String {
    format!("Spectrum {} {} EDID.icm", e.display_name(), e.key())
}

fn hdr_file(e: &Edid) -> String {
    format!("Spectrum HDR {} A.icc", e.key())
}

/// The one monitor, HDR on, with the Advanced rows expanded.
pub fn state() -> UiState {
    let e = monitor_edid();
    let mut sdr_options = vec![(format!("{} (EDID)", e.display_name()), edid_file(&e))];
    sdr_options.extend(
        REFERENCE_PROFILES
            .iter()
            .map(|(_, label, file)| (label.to_string(), file.to_string())),
    );
    let m = UiMonitor {
        num: 1,
        rect: Rect {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1440,
        },
        accent: ACCENTS[0],
        name: e.display_name(),
        width: 2560,
        height: 1440,
        hdr_supported: true,
        hdr_on: true,
        sdr_options,
        sdr_current: Some(0),
        has_edid: true,
        hdr: HdrParams {
            min_nits: 0.0,
            peak_nits: 450.0,
            full_frame_nits: 250.0,
            saturation: 0.0,
        },
        hdr_custom_active: true,
        sdr_white_nits: Some(240.0),
        sdr_above_peak: false,
        hdr_profile: HdrProfile::Spectrum,
        reported: Some(Reported {
            min_nits: 0.0,
            max_nits: 450.0,
            max_full_frame_nits: 250.0,
            color_space: 12,
            bits_per_color: 10,
            red: [0.6855, 0.3037],
            green: [0.2402, 0.7119],
            blue: [0.1436, 0.0576],
            white: [0.3135, 0.3291],
        }),
        edid_hdr: e.hdr.clone(),
    };
    UiState {
        monitors: vec![m],
        autostart: true,
        advanced: true,
        error: None,
    }
}

/// The profiles window rows.
pub fn profiles() -> Vec<ProfileRow> {
    let e = monitor_edid();
    let used = "Used by monitor 1 (default)".to_string();
    vec![
        ProfileRow {
            file: edid_file(&e),
            name: format!("{} (EDID)", e.display_name()),
            kind: ProfileKind::Sdr,
            used: used.clone(),
            deletable: Ok(()),
        },
        ProfileRow {
            file: hdr_file(&e),
            name: format!("{} HDR", e.display_name()),
            kind: ProfileKind::Hdr,
            used,
            deletable: Ok(()),
        },
        ProfileRow {
            file: "Display P3 calibrated.icm".into(),
            name: "Display P3 calibrated".into(),
            kind: ProfileKind::Sdr,
            used: "Not in use".into(),
            deletable: Ok(()),
        },
        ProfileRow {
            file: "sRGB Color Space Profile.icm".into(),
            name: "sRGB Color Space Profile".into(),
            kind: ProfileKind::Sdr,
            used: "Not in use".into(),
            deletable: Err(Undeletable::Inbox),
        },
    ]
}

/// The installed-profile rows of the gamut window: (file, name, primaries, tag), as the window reads them.
pub fn gamut_profiles() -> Vec<(String, String, Option<Primaries>, String)> {
    let e = monitor_edid();
    let edid_prims = Some(e.primaries);
    vec![
        (
            edid_file(&e),
            format!("{} (EDID)", e.display_name()),
            edid_prims,
            "Spectrum".to_string(),
        ),
        (
            hdr_file(&e),
            format!("{} HDR", e.display_name()),
            edid_prims,
            "HDR".to_string(),
        ),
        (
            "Display P3 calibrated.icm".to_string(),
            "Display P3 calibrated".to_string(),
            Some(STANDARD_SPACES[1].prims),
            "SDR".to_string(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_is_the_neutral_qd_oled() {
        let e = monitor_edid();
        assert_eq!(e.manufacturer, "SPC");
        assert_eq!(e.product_code, 0x0001);
        assert_eq!(e.display_name(), "QD-OLED 27");
        assert_eq!(e.key(), "SPC-0001-00000000");
        let h = e.hdr.clone().expect("HDR static metadata");
        assert!(h.max_nits.is_some_and(|v| (v - 455.5).abs() < 1.0));
    }

    #[test]
    fn builders_return_the_same_fixed_data() {
        assert_eq!(format!("{:?}", state()), format!("{:?}", state()));
        assert_eq!(format!("{:?}", profiles()), format!("{:?}", profiles()));
        assert_eq!(
            format!("{:?}", gamut_profiles()),
            format!("{:?}", gamut_profiles())
        );
        let st = state();
        let m = &st.monitors[0];
        assert_eq!(
            m.sdr_options[m.sdr_current.unwrap()].1,
            "Spectrum QD-OLED 27 SPC-0001-00000000 EDID.icm"
        );
        assert_eq!(m.sdr_cap(), Some(452));
        assert_eq!(profiles()[1].file, "Spectrum HDR SPC-0001-00000000 A.icc");
    }

    #[test]
    fn builders_read_nothing_from_the_machine() {
        // Only the code above the test module counts: no Win32, registry, color folder, DXGI or config access.
        let src = include_str!("showcase.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        for banned in [
            "windows_sys",
            "app::snapshot",
            "current_monitors",
            "enumerate",
            "list_profiles",
            "Config::load",
            "dxgi::reported",
            "wcs::associated",
            "wcs::read_profile",
            "profile_label",
        ] {
            assert!(!code.contains(banned), "showcase reads {banned}");
        }
    }
}
