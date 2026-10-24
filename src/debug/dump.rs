//! `--debug-dump [--json]` and `--debug-dxgi`: read-only state reports.

use super::{emit, fail, hdr_text, on_off, opt, parse_num};
use crate::app::{self, UiMonitor, UiState};
use crate::autostart;
use crate::color::{Primaries, Xy};
use crate::config::{Config, MonitorCfg};
use crate::dxgi::Reported;
use crate::edid::Edid;
use crate::monitors::{self, Monitor};
use crate::wcs;
use std::fmt::Write as _;
use std::path::Path;

// ---------------------------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------------------------

fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn json_num(v: f64) -> String {
    if v.is_finite() {
        format!("{v}")
    } else {
        "null".into()
    }
}

fn json_opt_num(v: Option<f64>) -> String {
    v.map(json_num).unwrap_or_else(|| "null".into())
}

fn json_opt(s: Option<&str>) -> String {
    s.map(json_str).unwrap_or_else(|| "null".into())
}

fn json_list(v: &[String]) -> String {
    format!(
        "[{}]",
        v.iter().map(|s| json_str(s)).collect::<Vec<_>>().join(",")
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|x| format!("{x:02x}")).collect()
}

fn prims_json(p: &Primaries) -> String {
    let xy = |n: &str, v: Xy| format!("\"{n}\":{{\"x\":{},\"y\":{}}}", json_num(v.x), json_num(v.y));
    format!(
        "{{{},{},{},{}}}",
        xy("r", p.r),
        xy("g", p.g),
        xy("b", p.b),
        xy("w", p.w)
    )
}

// ---------------------------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------------------------

/// What Windows has associated with one monitor.
struct Associations {
    sdr_default: Option<String>,
    hdr_default: Option<String>,
    sdr_assoc: Result<Vec<String>, String>,
    hdr_assoc: Result<Vec<String>, String>,
    user_scope: Option<bool>,
}

fn associations(m: &Monitor) -> Associations {
    Associations {
        sdr_default: wcs::default_profile(m, false),
        hdr_default: wcs::default_profile(m, true),
        sdr_assoc: wcs::associated(m, false),
        hdr_assoc: wcs::associated(m, true),
        user_scope: wcs::is_user_scope(m),
    }
}

/// EDID name, else the Windows friendly name (no "Monitor N" fallback: the dump shows what is there).
fn name_of(m: &Monitor) -> String {
    m.edid
        .as_ref()
        .map(|e| e.display_name())
        .unwrap_or_else(|| m.friendly_name.clone())
}

/// Autostart state: whether the Startup shortcut exists, and where it is.
pub(super) fn install_text(autostart: bool) -> String {
    let lnk = autostart::shortcut_path();
    let mut s = String::new();
    let _ = writeln!(s, "autostart:       {}", on_off(autostart));
    let _ = writeln!(s, "startup shortcut: {} (exists={})", lnk.display(), lnk.exists());
    s
}

pub(super) fn sdr_option_lines(m: &UiMonitor, indent: &str) -> String {
    let mut s = String::new();
    for (i, (label, file)) in m.sdr_options.iter().enumerate() {
        let mark = if m.sdr_current == Some(i) {
            "  <- current"
        } else {
            ""
        };
        let _ = writeln!(s, "{indent}[{i}] {label} ({file}){mark}");
    }
    s
}

fn dxgi_text(r: &Reported) -> String {
    format!(
        "min {} max {} maxfullframe {} nits; color space {}; {} bpc; R {:.4},{:.4} G {:.4},{:.4} B {:.4},{:.4} W {:.4},{:.4}",
        r.min_nits,
        r.max_nits,
        r.max_full_frame_nits,
        r.color_space_name(),
        r.bits_per_color,
        r.red[0],
        r.red[1],
        r.green[0],
        r.green[1],
        r.blue[0],
        r.blue[1],
        r.white[0],
        r.white[1]
    )
}

// ---------------------------------------------------------------------------------------------
// Text dump
// ---------------------------------------------------------------------------------------------

fn config_text(s: &mut String, cfg: &Config) {
    let cpath = crate::config::config_path();
    let _ = writeln!(
        s,
        "\nconfig file:     {} (exists={})",
        cpath.display(),
        cpath.exists()
    );
    let _ = writeln!(s, "config load failed: {}", cfg.load_failed);
    let _ = writeln!(s, "config monitors: {}", cfg.monitors.len());
    for (key, c) in &cfg.monitors {
        let _ = writeln!(
            s,
            "  [monitor:{key}] name={} edid_profile={} sdr={} hdr={} hdr_file={}",
            c.name,
            opt(c.edid_profile.as_deref()),
            opt(c.sdr.as_deref()),
            hdr_text(c.hdr),
            opt(c.hdr_file.as_deref())
        );
    }
}

/// Windows' view of the monitor: geometry, HDR, SDR content brightness, DXGI and associations.
fn windows_state_text(s: &mut String, m: &Monitor, cfg: &Config) {
    let a = associations(m);
    let _ = writeln!(s, "  key:            {}", m.key);
    let _ = writeln!(s, "  gdi:            {}", m.gdi_name);
    let _ = writeln!(
        s,
        "  rect:           {},{} to {},{} ({} x {})",
        m.rect.left,
        m.rect.top,
        m.rect.right,
        m.rect.bottom,
        m.rect.right - m.rect.left,
        m.rect.bottom - m.rect.top
    );
    let _ = writeln!(s, "  target/source:  {} / {}", m.target_id, m.source_id);
    let _ = writeln!(
        s,
        "  hdr:            supported={} enabled={}",
        m.hdr_supported, m.hdr_enabled
    );
    let _ = writeln!(
        s,
        "  sdr white:      {}",
        monitors::sdr_white_nits(m).map_or("(unavailable)".to_string(), |n| format!("{n} nits"))
    );
    let cap = monitors::sdr_white_cap(
        m.edid
            .as_ref()
            .and_then(|e| e.hdr.as_ref())
            .and_then(|h| h.max_nits),
    );
    let above = cfg
        .monitors
        .get(&m.key)
        .and_then(|c| c.sdr_above_peak)
        .unwrap_or(false);
    let _ = writeln!(
        s,
        "  sdr cap:        {} (allow above peak: {})",
        cap.map_or("(none)".to_string(), |c| format!("{c} nits")),
        on_off(above)
    );
    let _ = writeln!(
        s,
        "  windows reports: {}",
        crate::dxgi::reported(&m.gdi_name).map_or("(unavailable)".to_string(), |r| dxgi_text(&r))
    );
    let _ = writeln!(s, "  user scope:     {:?}", a.user_scope);
    let _ = writeln!(s, "  sdr default:    {}", opt(a.sdr_default.as_deref()));
    let _ = writeln!(s, "  sdr associated: {:?}", a.sdr_assoc);
    let _ = writeln!(s, "  hdr default:    {}", opt(a.hdr_default.as_deref()));
    let _ = writeln!(s, "  hdr associated: {:?}", a.hdr_assoc);
}

fn ui_state_text(s: &mut String, u: Option<&UiMonitor>) {
    match u {
        Some(u) => {
            let _ = writeln!(s, "  sdr options:");
            s.push_str(&sdr_option_lines(u, "    "));
            let _ = writeln!(
                s,
                "  hdr sliders:    {}  (custom active={})",
                hdr_text(Some(u.hdr)),
                u.hdr_custom_active
            );
        }
        None => {
            let _ = writeln!(s, "  sdr options:    (none)");
        }
    }
}

fn edid_text(s: &mut String, edid: Option<&Edid>) {
    let Some(e) = edid else {
        let _ = writeln!(s, "  edid:           (none)");
        return;
    };
    let p = &e.primaries;
    let _ = writeln!(
        s,
        "  edid:           {} {:04X} serial {:08X} year {} gamma {:?}",
        e.manufacturer, e.product_code, e.serial, e.year, e.gamma
    );
    let _ = writeln!(
        s,
        "  primaries:      R {:.4},{:.4}  G {:.4},{:.4}  B {:.4},{:.4}  W {:.4},{:.4}",
        p.r.x, p.r.y, p.g.x, p.g.y, p.b.x, p.b.y, p.w.x, p.w.y
    );
    if let Some(h) = &e.hdr {
        let _ = writeln!(s, "  edid hdr:       {h:?}");
    }
}

fn dump_text(mons: &[Monitor], cfg: &Config, ui: &UiState) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "Spectrum {} debug dump", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(s, "color folder: {}", wcs::color_dir().display());
    config_text(&mut s, cfg);
    let _ = writeln!(s, "\n{}", install_text(ui.autostart).trim_end());
    let _ = writeln!(s, "\nmonitors: {}", mons.len());
    for m in mons {
        let _ = writeln!(s, "\nmonitor {}: {}", m.num, name_of(m));
        windows_state_text(&mut s, m, cfg);
        ui_state_text(&mut s, ui.monitors.iter().find(|u| u.num == m.num));
        edid_text(&mut s, m.edid.as_ref());
        match cfg.monitors.get(&m.key) {
            Some(c) => {
                let _ = writeln!(s, "  config:         {c:?}");
            }
            None => {
                let _ = writeln!(s, "  config:         (none)");
            }
        }
    }
    s
}

// ---------------------------------------------------------------------------------------------
// JSON dump (`site/src/data/monitor.json`)
// ---------------------------------------------------------------------------------------------

fn sdr_options_json(u: Option<&UiMonitor>) -> String {
    let opts: Vec<String> = u
        .map(|u| {
            u.sdr_options
                .iter()
                .enumerate()
                .map(|(i, (label, file))| {
                    format!(
                        "{{\"index\":{i},\"label\":{},\"file\":{},\"current\":{}}}",
                        json_str(label),
                        json_str(file),
                        u.sdr_current == Some(i)
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    opts.join(",")
}

fn edid_json(edid: Option<&Edid>) -> String {
    let Some(e) = edid else {
        return "null".into();
    };
    let hdr = match &e.hdr {
        Some(h) => format!(
            "{{\"max_nits\":{},\"max_frame_avg_nits\":{},\"min_nits\":{},\"pq\":{}}}",
            json_opt_num(h.max_nits),
            json_opt_num(h.max_frame_avg_nits),
            json_opt_num(h.min_nits),
            h.pq
        ),
        None => "null".into(),
    };
    format!(
        "{{\"hex\":{},\"manufacturer\":{},\"product_code\":{},\"serial\":{},\"year\":{},\"gamma\":{},\"srgb_default\":{},\"primaries\":{},\"hdr\":{}}}",
        json_str(&hex(&e.raw)),
        json_str(&e.manufacturer),
        e.product_code,
        e.serial,
        e.year,
        json_opt_num(e.gamma),
        e.srgb_default,
        prims_json(&e.primaries),
        hdr
    )
}

fn config_json(c: Option<&MonitorCfg>) -> String {
    let Some(c) = c else {
        return "null".into();
    };
    let hdr = match c.hdr {
        Some(h) => format!(
            "{{\"min_nits\":{},\"peak_nits\":{},\"full_frame_nits\":{},\"saturation\":{}}}",
            json_num(h.min_nits),
            json_num(h.peak_nits),
            json_num(h.full_frame_nits),
            json_num(h.saturation)
        ),
        None => "null".into(),
    };
    format!(
        "{{\"name\":{},\"edid_profile\":{},\"sdr\":{},\"hdr\":{},\"hdr_file\":{}}}",
        json_str(&c.name),
        json_opt(c.edid_profile.as_deref()),
        json_opt(c.sdr.as_deref()),
        hdr,
        json_opt(c.hdr_file.as_deref())
    )
}

fn monitor_json(m: &Monitor, cfg: &Config, ui: &UiState) -> String {
    let a = associations(m);
    let mut o = String::new();
    let _ = write!(
        o,
        "{{\"num\":{},\"name\":{},\"key\":{},\"gdi\":{},\"rect\":{{\"left\":{},\"top\":{},\"right\":{},\"bottom\":{}}},",
        m.num,
        json_str(&name_of(m)),
        json_str(&m.key),
        json_str(&m.gdi_name),
        m.rect.left,
        m.rect.top,
        m.rect.right,
        m.rect.bottom
    );
    let _ = write!(
        o,
        "\"hdr_supported\":{},\"hdr_enabled\":{},\"user_scope\":{},",
        m.hdr_supported,
        m.hdr_enabled,
        a.user_scope
            .map(|b| b.to_string())
            .unwrap_or_else(|| "null".into())
    );
    let _ = write!(
        o,
        "\"sdr_default\":{},\"hdr_default\":{},\"sdr_associated\":{},\"hdr_associated\":{},",
        json_opt(a.sdr_default.as_deref()),
        json_opt(a.hdr_default.as_deref()),
        json_list(a.sdr_assoc.as_deref().unwrap_or_default()),
        json_list(a.hdr_assoc.as_deref().unwrap_or_default())
    );
    let ui_mon = ui.monitors.iter().find(|u| u.num == m.num);
    let _ = write!(o, "\"sdr_options\":[{}],", sdr_options_json(ui_mon));
    let _ = write!(o, "\"edid\":{},", edid_json(m.edid.as_ref()));
    let _ = write!(o, "\"config\":{}", config_json(cfg.monitors.get(&m.key)));
    o.push('}');
    o
}

fn dump_json(mons: &[Monitor], cfg: &Config, ui: &UiState) -> String {
    let items: Vec<String> = mons.iter().map(|m| monitor_json(m, cfg, ui)).collect();
    let lnk = autostart::shortcut_path();
    let install = format!(
        "{{\"startup_shortcut\":{},\"startup_shortcut_exists\":{}}}",
        json_str(&lnk.display().to_string()),
        lnk.exists()
    );
    let cpath = crate::config::config_path();
    format!(
        "{{\"autostart\":{},\"install\":{},\"config_file\":{},\"config_exists\":{},\"monitors\":[{}]}}\n",
        ui.autostart,
        install,
        json_str(&cpath.display().to_string()),
        cpath.exists(),
        items.join(",\n")
    )
}

// ---------------------------------------------------------------------------------------------
// Switches
// ---------------------------------------------------------------------------------------------

/// `--debug-dxgi <num>`: what Windows reports for the monitor, read through DXGI.
pub fn dxgi(num: &str, out: Option<&Path>) -> i32 {
    let n = match parse_num(num) {
        Ok(n) => n,
        Err(e) => return fail(out, &e),
    };
    let mons = monitors::enumerate();
    let Some(m) = mons.iter().find(|m| m.num == n) else {
        return fail(out, &format!("no monitor {n}"));
    };
    match crate::dxgi::reported(&m.gdi_name) {
        Some(r) => {
            emit(out, &format!("monitor {n} ({}): {}\n", m.gdi_name, dxgi_text(&r)));
            0
        }
        None => fail(
            out,
            &format!("DXGI has no output for monitor {n} ({})", m.gdi_name),
        ),
    }
}

/// `--debug-dump [--json]`
pub fn dump(json: bool, out: Option<&Path>) {
    let mons = monitors::enumerate();
    let cfg = Config::load();
    let ui = app::snapshot();
    let text = if json {
        dump_json(&mons, &cfg, &ui)
    } else {
        dump_text(&mons, &cfg, &ui)
    };
    emit(out, &text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(json_str("a\"b\\c"), r#""a\"b\\c""#);
        assert_eq!(json_str("l1\nl2\t\r"), r#""l1\nl2\t\r""#);
        assert_eq!(json_str("\u{1}"), r#""\u0001""#);
        assert_eq!(json_str("QD-OLED \u{b7} 27"), "\"QD-OLED \u{b7} 27\"");
    }

    #[test]
    fn json_numbers_and_options() {
        assert_eq!(json_num(455.5), "455.5");
        assert_eq!(json_num(f64::NAN), "null");
        assert_eq!(json_opt_num(None), "null");
        assert_eq!(json_opt(Some("x")), "\"x\"");
        assert_eq!(json_opt(None), "null");
        assert_eq!(json_list(&["a".into(), "b".into()]), r#"["a","b"]"#);
        assert_eq!(json_list(&[]), "[]");
        assert_eq!(hex(&[0x00, 0xAB, 0x10]), "00ab10");
    }

    #[test]
    fn edid_json_has_the_fixture_fields() {
        let e = crate::showcase::monitor_edid();
        let j = edid_json(Some(&e));
        assert!(j.starts_with("{\"hex\":\"00ffffffffffff00"), "{j}");
        assert!(j.contains("\"manufacturer\":\"SPC\""), "{j}");
        assert!(j.contains("\"pq\":true"), "{j}");
        assert_eq!(edid_json(None), "null");
        assert_eq!(config_json(None), "null");
    }

    #[test]
    fn showcase_ui_state_lines() {
        let st = crate::showcase::state();
        let lines = sdr_option_lines(&st.monitors[0], "  ");
        assert!(lines.starts_with(
            "  [0] QD-OLED 27 (EDID) (Spectrum QD-OLED 27 SPC-0001-00000000 EDID.icm)  <- current\n"
        ));
        assert_eq!(lines.lines().count(), 7);
        let r = st.monitors[0].reported.unwrap();
        assert!(dxgi_text(&r).starts_with("min 0 max 450 maxfullframe 250 nits; color space "));
    }
}
