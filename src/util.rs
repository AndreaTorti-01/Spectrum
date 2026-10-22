//! Shared helpers: UTF-16 conversion, the `%APPDATA%\Spectrum` directory and a tiny size-capped log file.

use std::io::Write;
use std::path::PathBuf;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{FOLDERID_RoamingAppData, SHGetKnownFolderPath};

const LOG_CAP: u64 = 256 * 1024;

/// UTF-8 to NUL-terminated UTF-16.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// UTF-16 to String, stopping at the first NUL.
pub fn from_wide(w: &[u16]) -> String {
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}

fn roaming_app_data() -> Option<PathBuf> {
    unsafe {
        let mut p: *mut u16 = std::ptr::null_mut();
        let hr = SHGetKnownFolderPath(&FOLDERID_RoamingAppData, 0, std::ptr::null_mut(), &mut p);
        let out = if hr >= 0 && !p.is_null() {
            let mut len = 0usize;
            while *p.add(len) != 0 {
                len += 1;
            }
            Some(PathBuf::from(from_wide(std::slice::from_raw_parts(p, len))))
        } else {
            None
        };
        if !p.is_null() {
            CoTaskMemFree(p as *const _);
        }
        out
    }
}

/// `%APPDATA%\Spectrum`, created on demand.
pub fn app_data_dir() -> PathBuf {
    let base = roaming_app_data()
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("Spectrum");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Appends a timestamped line to `spectrum.log`; the file is truncated when it exceeds 256 KB.
pub fn log(msg: &str) {
    let path = app_data_dir().join("spectrum.log");
    let too_big = std::fs::metadata(&path)
        .map(|m| m.len() > LOG_CAP)
        .unwrap_or(false);
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true);
    if too_big {
        opts.write(true).truncate(true);
    } else {
        opts.append(true);
    }
    if let Ok(mut f) = opts.open(&path) {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{secs}] {msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_roundtrip() {
        let w = wide("héllo");
        assert_eq!(*w.last().unwrap(), 0);
        assert_eq!(from_wide(&w), "héllo");
        assert_eq!(from_wide(&[104, 105, 0, 120]), "hi");
    }
}
