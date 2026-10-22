//! Tiny INI persistence for per-monitor choices in `%APPDATA%\Spectrum\spectrum.ini`.

use crate::icc_write::HdrParams;
use crate::util::{app_data_dir, wide};
use std::collections::BTreeMap;
use std::path::PathBuf;
use windows_sys::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonitorCfg {
    pub name: String,
    pub edid_profile: Option<String>,
    pub sdr: Option<String>,
    pub hdr: Option<HdrParams>,
    pub hdr_file: Option<String>,
    /// "Allow SDR content above the monitor peak"; `None` means off.
    pub sdr_above_peak: Option<bool>,
}

#[derive(Clone, Debug, Default)]
pub struct Config {
    pub monitors: BTreeMap<String, MonitorCfg>,
    /// The HDR section's "Advanced" row is expanded.
    pub advanced: bool,
    /// The file exists but could not be read (antivirus lock, permissions). `save` refuses to overwrite
    /// it, so a transient read error cannot wipe the other monitors' settings.
    pub load_failed: bool,
}

/// `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`: other Spectrum processes may read, write or
/// replace the file while we hold it open.
const SHARE_ALL: u32 = 0x1 | 0x2 | 0x4;
const RETRIES: u32 = 5;
const BACKOFF: std::time::Duration = std::time::Duration::from_millis(50);

fn read_shared(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    use std::os::windows::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_ALL)
        .open(path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

fn write_shared(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::windows::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .share_mode(SHARE_ALL)
        .open(path)?;
    f.write_all(bytes)
}

/// Another process replacing or scanning the file shows up as access denied or a sharing/lock violation.
fn is_transient(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(5 | 32 | 33))
}

/// Runs `op`, retrying up to [`RETRIES`] times with a short backoff while it fails transiently.
fn retry_io<T>(mut op: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut attempt = 0;
    loop {
        match op() {
            Err(e) if is_transient(&e) && attempt < RETRIES => {
                attempt += 1;
                std::thread::sleep(BACKOFF);
            }
            r => return r,
        }
    }
}

const SECTION_PREFIX: &str = "monitor:";
const UI_SECTION: &str = "ui";

pub fn config_path() -> PathBuf {
    app_data_dir().join("spectrum.ini")
}

fn parse_hdr(v: &str) -> Option<HdrParams> {
    let nums: Vec<f64> = v
        .split(',')
        .map(|s| s.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    if nums.len() != 4 || nums.iter().any(|n| !n.is_finite()) {
        return None;
    }
    Some(HdrParams {
        min_nits: nums[0],
        peak_nits: nums[1],
        full_frame_nits: nums[2],
        saturation: nums[3],
    })
}

/// Single-line, trimmed value (INI values cannot hold line breaks).
fn clean(v: &str) -> String {
    v.chars()
        .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

impl Config {
    pub fn load() -> Config {
        Config::from_read(retry_io(|| read_shared(&config_path())))
    }

    /// A missing file is an empty config; any other read error marks the config as `load_failed`.
    fn from_read(r: std::io::Result<Vec<u8>>) -> Config {
        match r {
            Ok(bytes) => Config::parse(&String::from_utf8_lossy(&bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => {
                crate::util::log(&format!("config: read failed, keeping the file untouched: {e}"));
                Config {
                    load_failed: true,
                    ..Config::default()
                }
            }
        }
    }

    /// Atomic: writes `spectrum.ini.tmp`, then replaces the real file. Failures are logged, not fatal.
    pub fn save(&self) {
        if self.load_failed {
            crate::util::log("config: not saving, the file could not be read earlier");
            return;
        }
        let path = config_path();
        let tmp = path.with_extension(format!("ini.{}.tmp", std::process::id()));
        if let Err(e) = retry_io(|| write_shared(&tmp, self.serialize().as_bytes())) {
            crate::util::log(&format!("config: write {} failed: {e}", tmp.display()));
            return;
        }
        let replaced = retry_io(|| {
            let ok = unsafe {
                MoveFileExW(
                    wide(&tmp.to_string_lossy()).as_ptr(),
                    wide(&path.to_string_lossy()).as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            };
            if ok == 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
        if let Err(e) = replaced {
            crate::util::log(&format!("config: replacing {} failed: {e}", path.display()));
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// A deliberate deletion: clears every field that names `file` so reconcile does not bring it back.
    /// Returns true when something changed.
    pub fn forget_profile(&mut self, file: &str) -> bool {
        let is = |v: &Option<String>| {
            v.as_deref()
                .map(|x| x.eq_ignore_ascii_case(file))
                .unwrap_or(false)
        };
        let mut changed = false;
        for m in self.monitors.values_mut() {
            if is(&m.edid_profile) {
                m.edid_profile = None;
                changed = true;
            }
            if is(&m.sdr) {
                m.sdr = None;
                changed = true;
            }
            if is(&m.hdr_file) {
                m.hdr_file = None;
                m.hdr = None;
                changed = true;
            }
        }
        changed
    }

    pub fn monitor_mut(&mut self, key: &str) -> &mut MonitorCfg {
        self.monitors.entry(key.to_string()).or_default()
    }

    /// Lenient: unknown sections/keys, comments, junk lines and bad values are skipped.
    pub fn parse(text: &str) -> Config {
        let mut cfg = Config::default();
        let mut current: Option<String> = None;
        let mut in_ui = false;
        for raw in text.lines() {
            let line = raw.trim().trim_start_matches('\u{feff}');
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some(inner) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                in_ui = inner.trim().eq_ignore_ascii_case(UI_SECTION);
                current = inner
                    .trim()
                    .strip_prefix(SECTION_PREFIX)
                    .map(|k| k.trim().to_string())
                    .filter(|k| !k.is_empty());
                if let Some(k) = &current {
                    cfg.monitor_mut(k);
                }
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            if in_ui {
                if name.trim().eq_ignore_ascii_case("advanced") {
                    cfg.advanced = value.trim() == "1";
                }
                continue;
            }
            let Some(key) = &current else { continue };
            let value = value.trim();
            let m = cfg.monitor_mut(key);
            let opt = || (!value.is_empty()).then(|| value.to_string());
            match name.trim().to_ascii_lowercase().as_str() {
                "name" => m.name = value.to_string(),
                "edid_profile" => m.edid_profile = opt(),
                "sdr" => m.sdr = opt(),
                "hdr" => m.hdr = parse_hdr(value),
                "hdr_file" => m.hdr_file = opt(),
                "sdr_above_peak" => m.sdr_above_peak = Some(value == "1"),
                _ => {}
            }
        }
        cfg
    }

    pub fn serialize(&self) -> String {
        let mut s = String::new();
        if self.advanced {
            s.push_str(&format!(
                "[{UI_SECTION}]
advanced=1

"
            ));
        }
        for (key, m) in &self.monitors {
            s.push_str(&format!("[{SECTION_PREFIX}{}]\n", clean(key)));
            s.push_str(&format!("name={}\n", clean(&m.name)));
            if let Some(v) = &m.edid_profile {
                s.push_str(&format!("edid_profile={}\n", clean(v)));
            }
            if let Some(v) = &m.sdr {
                s.push_str(&format!("sdr={}\n", clean(v)));
            }
            if let Some(h) = &m.hdr {
                s.push_str(&format!(
                    "hdr={},{},{},{}\n",
                    h.min_nits, h.peak_nits, h.full_frame_nits, h.saturation
                ));
            }
            if let Some(v) = &m.hdr_file {
                s.push_str(&format!("hdr_file={}\n", clean(v)));
            }
            if let Some(on) = m.sdr_above_peak {
                s.push_str(&format!("sdr_above_peak={}\n", u8::from(on)));
            }
            s.push('\n');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Config {
        let mut c = Config::default();
        {
            let m = c.monitor_mut("MSI-3CB0-0000012A");
            m.name = "MAG 271QPX E2".into();
            m.edid_profile = Some("MAG 271QPX (EDID).icm".into());
            m.sdr = Some("sRGB Color Space Profile.icm".into());
            m.hdr = Some(HdrParams {
                min_nits: 0.0,
                peak_nits: 450.0,
                full_frame_nits: 400.5,
                saturation: 0.25,
            });
            m.hdr_file = Some("Spectrum HDR MSI A.icc".into());
            m.sdr_above_peak = Some(true);
        }
        c.monitor_mut("DEL-1234-0").name = "Dell = weird [name]".into();
        c
    }

    #[test]
    fn roundtrip() {
        let c = sample();
        let text = c.serialize();
        let back = Config::parse(&text);
        assert_eq!(back.monitors, c.monitors);
        assert_eq!(back.serialize(), text);
    }

    #[test]
    fn junk_is_ignored() {
        let text = "\u{feff}garbage line\n=novalue\n[unknown]\nfoo=bar\n; comment\n\
                    [monitor:A-1]\nname = Foo \nhdr=1,2,x,4\nsdr=\nnonsense\nedid_profile=e.icm\n\
                    [monitor:]\nname=lost\n[monitor:B-2\nname=lost2\n[monitor:C-3]\nHDR=0,1000,400,0.5\n";
        let c = Config::parse(text);
        assert_eq!(c.monitors.len(), 2);
        let a = &c.monitors["A-1"];
        assert_eq!(a.name, "Foo");
        assert_eq!(a.hdr, None);
        assert_eq!(a.sdr, None);
        assert_eq!(a.edid_profile.as_deref(), Some("e.icm"));
        assert_eq!(c.monitors["C-3"].hdr.unwrap().peak_nits, 1000.0);
    }

    #[test]
    fn read_error_is_not_not_found() {
        use std::io::{Error, ErrorKind};
        let missing = Config::from_read(Err(Error::from(ErrorKind::NotFound)));
        assert!(!missing.load_failed);
        assert!(missing.monitors.is_empty());
        let locked = Config::from_read(Err(Error::from(ErrorKind::PermissionDenied)));
        assert!(locked.load_failed);
        assert!(locked.monitors.is_empty());
        let ok = Config::from_read(Ok(b"[monitor:A-1]
name=x
"
        .to_vec()));
        assert!(!ok.load_failed);
        assert_eq!(ok.monitors.len(), 1);
    }

    #[test]
    fn retry_gives_up_after_five_retries() {
        use std::io::{Error, ErrorKind};
        let mut calls = 0;
        let r: std::io::Result<()> = retry_io(|| {
            calls += 1;
            Err(Error::from(ErrorKind::PermissionDenied))
        });
        assert!(r.is_err());
        assert_eq!(calls, 6);
        let mut calls = 0;
        let r = retry_io(|| {
            calls += 1;
            if calls < 3 {
                Err(Error::from_raw_os_error(32))
            } else {
                Ok(7)
            }
        });
        assert_eq!(r.unwrap(), 7);
        let mut calls = 0;
        let _: std::io::Result<()> = retry_io(|| {
            calls += 1;
            Err(Error::from(ErrorKind::NotFound))
        });
        assert_eq!(calls, 1);
    }

    #[test]
    fn forgetting_a_profile_clears_matching_fields_only() {
        let mut c = sample();
        c.monitor_mut("DEL-1234-0").sdr = Some("Other.icm".into());
        assert!(c.forget_profile("spectrum hdr msi a.ICC"));
        let m = &c.monitors["MSI-3CB0-0000012A"];
        assert_eq!(m.hdr_file, None);
        assert_eq!(m.hdr, None);
        assert!(m.edid_profile.is_some() && m.sdr.is_some());
        assert!(c.forget_profile("MAG 271QPX (EDID).icm"));
        assert_eq!(c.monitors["MSI-3CB0-0000012A"].edid_profile, None);
        assert!(c.forget_profile("sRGB Color Space Profile.icm"));
        assert_eq!(c.monitors["MSI-3CB0-0000012A"].sdr, None);
        assert_eq!(c.monitors["DEL-1234-0"].sdr.as_deref(), Some("Other.icm"));
        assert!(!c.forget_profile("nothing.icm"));
    }

    #[test]
    fn empty_and_nonsense() {
        assert!(Config::parse("").monitors.is_empty());
        assert!(Config::parse("\0\0\0\n[[[\n]]]\n").monitors.is_empty());
    }

    #[test]
    fn newlines_in_values_cannot_inject_keys() {
        let mut c = Config::default();
        c.monitor_mut("K").name = "a\nsdr=evil".into();
        let back = Config::parse(&c.serialize());
        assert_eq!(back.monitors["K"].sdr, None);
    }
}
