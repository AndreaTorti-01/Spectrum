//! Developer switches: state dumps, headless flows, the scripted README demo. Output goes to `--out <file>`
//! when given, otherwise to the parent console (the exe is a GUI-subsystem program).
//!
//! - `dump`: `--debug-dump [--json]` and `--debug-dxgi`.
//! - `actions`: the headless UI actions (`--debug-sdr`, `--debug-hdr`, ...), through the windows' dispatch.
//! - `instance`: finding, commanding and stopping a running instance.

mod actions;
mod dump;
mod instance;

pub use actions::{
    apply_hdr, autostart, delete_profile, read_edid, reconcile, reset_hdr, sdr_cap, sdr_white, set_sdr,
    toggle_hdr, wipe,
};
pub use dump::{dump, dxgi};
pub use instance::{find_running, send_to_running, stop_running};

use crate::app::{ProfileRow, UiState};
use crate::icc_write::HdrParams;
use crate::ui;
use std::io::Write as _;
use std::path::Path;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Console::{
    AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
};

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
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.flush();
        return;
    }
    // SAFETY: attaching is harmless when there is no parent console.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
        let _ = f.write_all(text.as_bytes());
    }
}

/// Reports `msg` as an error. Exit code 1.
fn fail(out: Option<&Path>, msg: &str) -> i32 {
    emit(out, &format!("error: {msg}\n"));
    1
}

/// A 1-based monitor number as the UI shows it.
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

fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

/// Parses `min,peak,full,sat` (sat 0..1).
fn parse_hdr_params(s: &str) -> Result<HdrParams, String> {
    let params = crate::config::parse_hdr(s)
        .ok_or_else(|| format!("expected min,peak,full,sat as numbers, got {s:?}"))?;
    if !(0.0..=1.0).contains(&params.saturation) {
        return Err(format!(
            "saturation must be between 0 and 1, got {}",
            params.saturation
        ));
    }
    Ok(params)
}

/// "min 0 peak 1000 full 400 sat 0", or "(none)".
fn hdr_text(h: Option<HdrParams>) -> String {
    match h {
        Some(h) => format!(
            "min {} peak {} full {} sat {}",
            h.min_nits, h.peak_nits, h.full_frame_nits, h.saturation
        ),
        None => "(none)".into(),
    }
}

fn opt(s: Option<&str>) -> &str {
    s.unwrap_or("(none)")
}

// ---------------------------------------------------------------------------------------------
// Showcase (screenshots)
// ---------------------------------------------------------------------------------------------

/// The screenshot state: the fixed QD-OLED 27 of `crate::showcase`. The windows read it through
/// `ui::set_mock`, so nothing is applied and no setting changes.
pub fn showcase_state() -> UiState {
    crate::showcase::state()
}

/// Profile rows for the profiles window screenshots (`crate::showcase`).
pub fn showcase_profiles() -> Vec<ProfileRow> {
    crate::showcase::profiles()
}

/// `open gamut-showcase`: the gamut window with the monitor's EDID-derived profile, sRGB and Display P3
/// ticked. When that profile is not installed, an entry is built in memory from the EDID primaries.
pub fn open_gamut_showcase() {
    let st = showcase_state();
    let Some((label, file)) = st.monitors.first().and_then(|m| m.sdr_options.first().cloned()) else {
        ui::gamut_window::open(None, 0);
        return;
    };
    let prims = Some(crate::showcase::monitor_edid().primaries);
    ui::gamut_window::open_showcase(file, label, prims, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_numbers_start_at_one() {
        assert_eq!(parse_num(" 2 "), Ok(2));
        assert!(parse_num("0").is_err());
        assert!(parse_num("-1").is_err());
        assert!(parse_num("x").is_err());
    }

    #[test]
    fn on_off_parses_both_ways() {
        assert_eq!(parse_on_off("ON"), Ok(true));
        assert_eq!(parse_on_off(" off"), Ok(false));
        assert!(parse_on_off("yes").is_err());
        assert_eq!((on_off(true), on_off(false)), ("on", "off"));
    }

    #[test]
    fn hdr_params_parse_and_check_saturation() {
        let p = parse_hdr_params("0.05, 700, 300, 0.5").unwrap();
        assert_eq!(
            (p.min_nits, p.peak_nits, p.full_frame_nits, p.saturation),
            (0.05, 700.0, 300.0, 0.5)
        );
        assert_eq!(hdr_text(Some(p)), "min 0.05 peak 700 full 300 sat 0.5");
        assert_eq!(hdr_text(None), "(none)");
        assert!(parse_hdr_params("0,1000,400").is_err());
        assert!(parse_hdr_params("0,1000,400,x").is_err());
        assert!(parse_hdr_params("0,inf,400,0").is_err());
        assert_eq!(
            parse_hdr_params("0,1000,400,2").unwrap_err(),
            "saturation must be between 0 and 1, got 2"
        );
    }
}
