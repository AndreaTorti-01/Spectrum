//! Windows Color System access: color folder files, per-user profile associations, reference profiles.

use crate::monitors::Monitor;
use crate::util::{app_data_dir, from_wide, wide};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use windows_sys::core::PWSTR;
use windows_sys::Win32::Foundation::{LocalFree, TRUE};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW};
use windows_sys::Win32::UI::ColorSystem::{
    ColorProfileAddDisplayAssociation, ColorProfileGetDisplayDefault, ColorProfileGetDisplayUserScope,
    ColorProfileRemoveDisplayAssociation, ColorProfileSetDisplayDefaultAssociation, GetColorDirectoryW,
    InstallColorProfileW, WcsEnumColorProfiles, WcsEnumColorProfilesSize, WcsSetUsePerUserProfiles,
    COLORPROFILESUBTYPE, CPST_EXTENDED_DISPLAY_COLOR_MODE, CPST_STANDARD_DISPLAY_COLOR_MODE, CPT_ICC,
    ENUMTYPEW, ENUM_TYPE_VERSION, ET_DEVICENAME, ET_EXTENDEDDISPLAYCOLOR, ET_STANDARDDISPLAYCOLOR,
    WCS_PROFILE_MANAGEMENT_SCOPE, WCS_PROFILE_MANAGEMENT_SCOPE_CURRENT_USER,
};

pub const SRGB_INBOX: &str = "sRGB Color Space Profile.icm";

const SCOPE: WCS_PROFILE_MANAGEMENT_SCOPE = WCS_PROFILE_MANAGEMENT_SCOPE_CURRENT_USER;
/// `CLASS_MONITOR` ('mntr') from icm.h.
const CLASS_MONITOR: u32 = 0x6d6e_7472;

/// The system color folder (`...\spool\drivers\color`).
pub fn color_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| unsafe {
        let mut buf = [0u16; 520];
        let mut size = (buf.len() * 2) as u32;
        if GetColorDirectoryW(std::ptr::null(), buf.as_mut_ptr(), &mut size) != 0 {
            PathBuf::from(from_wide(&buf))
        } else {
            let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
            PathBuf::from(root).join("System32\\spool\\drivers\\color")
        }
    })
    .clone()
}

/// `app_data_dir()\profiles`
pub fn backup_dir() -> PathBuf {
    app_data_dir().join("profiles")
}

/// File names only: no separators, no drive letters, no "..".
fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['\\', '/', ':', '\0'])
        && Path::new(name).file_name().map(|f| f == name).unwrap_or(false)
}

fn checked(name: &str) -> Result<(), String> {
    if valid_file_name(name) {
        Ok(())
    } else {
        Err(format!("invalid profile file name: {name:?}"))
    }
}

/// Writes straight into the color folder (users may create/overwrite their own files there) plus a
/// backup copy in `%APPDATA%\Spectrum\profiles`. Registering with `InstallColorProfileW` is best effort.
pub fn install_profile(file_name: &str, bytes: &[u8]) -> Result<(), String> {
    checked(file_name)?;
    let target = color_dir().join(file_name);
    std::fs::write(&target, bytes).map_err(|e| format!("cannot write {}: {e}", target.display()))?;
    let backup = backup_dir();
    if std::fs::create_dir_all(&backup).is_ok() {
        if let Err(e) = std::fs::write(backup.join(file_name), bytes) {
            crate::util::log(&format!("wcs: backup copy of {file_name} failed: {e}"));
        }
    }
    unsafe {
        let ok = InstallColorProfileW(std::ptr::null(), wide(&target.to_string_lossy()).as_ptr());
        if ok == 0 {
            crate::util::log(&format!(
                "wcs: InstallColorProfileW({file_name}) failed (ignored): {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(())
}

/// If missing in the color dir and present in the backup, copies it back.
pub fn restore_from_backup(file_name: &str) -> bool {
    if checked(file_name).is_err() {
        return false;
    }
    let target = color_dir().join(file_name);
    if target.exists() {
        return false;
    }
    match std::fs::read(backup_dir().join(file_name)) {
        Ok(bytes) => install_profile(file_name, &bytes).is_ok(),
        Err(_) => false,
    }
}

pub fn profile_exists(file_name: &str) -> bool {
    valid_file_name(file_name) && color_dir().join(file_name).is_file()
}

/// Largest profile file `read_profile` loads (real profiles are well under 1 MiB).
pub const MAX_PROFILE_BYTES: u64 = 8 << 20;

/// Size and modification time of a color folder file; changes when the file is rewritten.
pub fn profile_stamp(file_name: &str) -> Option<(u64, Option<std::time::SystemTime>)> {
    if !valid_file_name(file_name) {
        return None;
    }
    let md = std::fs::metadata(color_dir().join(file_name)).ok()?;
    Some((md.len(), md.modified().ok()))
}

/// Reads a color folder file; `None` when missing or larger than [`MAX_PROFILE_BYTES`] (checked before reading).
pub fn read_profile(file_name: &str) -> Option<Vec<u8>> {
    if !valid_file_name(file_name) {
        return None;
    }
    let path = color_dir().join(file_name);
    if std::fs::metadata(&path).ok()?.len() > MAX_PROFILE_BYTES {
        return None;
    }
    std::fs::read(path).ok()
}

/// Removes from the color dir and the backup.
pub fn delete_profile(file_name: &str) -> Result<(), String> {
    checked(file_name)?;
    let _ = std::fs::remove_file(backup_dir().join(file_name));
    let target = color_dir().join(file_name);
    match std::fs::remove_file(&target) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("cannot delete {}: {e}", target.display())),
    }
}

/// Why a color folder profile can't be offered for deletion. `hint()` is the text shown beside the
/// disabled button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Undeletable {
    /// The inbox sRGB profile, which Windows relies on.
    Inbox,
    /// The current user has no DELETE access to the file.
    NoAccess,
    /// Not a plain file name.
    Invalid,
}

impl Undeletable {
    pub fn hint(self) -> &'static str {
        match self {
            Undeletable::Inbox => "Installed by Windows. Needs admin to delete",
            Undeletable::NoAccess => "Needs admin to delete",
            Undeletable::Invalid => "Not a profile file",
        }
    }
}

/// The deletion rule, separated from the file system: `has_delete_access` is the result of the access
/// check on the file.
pub fn delete_decision(file_name: &str, has_delete_access: bool) -> Result<(), Undeletable> {
    if !valid_file_name(file_name) {
        Err(Undeletable::Invalid)
    } else if file_name.eq_ignore_ascii_case(SRGB_INBOX) {
        Err(Undeletable::Inbox)
    } else if !has_delete_access {
        Err(Undeletable::NoAccess)
    } else {
        Ok(())
    }
}

/// True when the current user may delete the file: opens it with DELETE access (nothing is deleted) and
/// closes it again. A missing file counts as not deletable.
fn has_delete_access(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    const DELETE: u32 = 0x0001_0000;
    const SHARE_ALL: u32 = 0x1 | 0x2 | 0x4;
    std::fs::OpenOptions::new()
        .access_mode(DELETE)
        .share_mode(SHARE_ALL)
        .open(path)
        .is_ok()
}

/// Whether Spectrum may offer to delete this color folder file.
pub fn can_delete(file_name: &str) -> Result<(), Undeletable> {
    let access = valid_file_name(file_name) && has_delete_access(&color_dir().join(file_name));
    delete_decision(file_name, access)
}

/// `*.icc` / `*.icm` file names in the color dir.
pub fn list_installed() -> Vec<String> {
    let mut v: Vec<String> = match std::fs::read_dir(color_dir()) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| {
                let l = n.to_ascii_lowercase();
                l.ends_with(".icc") || l.ends_with(".icm")
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    v.sort_by_key(|n| n.to_ascii_lowercase());
    v
}

fn hr(what: &str, code: i32) -> String {
    format!("{what} failed: HRESULT {:#010x}", code as u32)
}

fn user_scope(m: &Monitor) -> Option<WCS_PROFILE_MANAGEMENT_SCOPE> {
    let mut scope: WCS_PROFILE_MANAGEMENT_SCOPE = 0;
    let rc = unsafe { ColorProfileGetDisplayUserScope(m.adapter.to_win(), m.source_id, &mut scope) };
    (rc >= 0).then_some(scope)
}

/// True when per-user profile associations are active for this monitor.
pub fn is_user_scope(m: &Monitor) -> Option<bool> {
    user_scope(m).map(|s| s == SCOPE)
}

/// Makes sure associations made in the CURRENT_USER scope take effect: if the user scope is not
/// the active one, turns on "use my settings for this device" via `WcsSetUsePerUserProfiles`.
pub fn ensure_user_scope(m: &Monitor) -> Result<(), String> {
    if user_scope(m) == Some(SCOPE) {
        return Ok(());
    }
    let device_id =
        monitor_device_id(m).ok_or_else(|| format!("cannot resolve monitor device id for {}", m.gdi_name))?;
    if unsafe { WcsSetUsePerUserProfiles(wide(&device_id).as_ptr(), CLASS_MONITOR, TRUE) } == 0 {
        return Err(format!(
            "WcsSetUsePerUserProfiles({device_id}) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// `MONITOR\SPC0001\{4d36e96e-...}\0003`: the device name the legacy WCS calls want.
fn monitor_device_id(m: &Monitor) -> Option<String> {
    unsafe {
        let mut dd: DISPLAY_DEVICEW = std::mem::zeroed();
        dd.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        if EnumDisplayDevicesW(wide(&m.gdi_name).as_ptr(), 0, &mut dd, 0) == 0 {
            return None;
        }
        Some(from_wide(&dd.DeviceID))
    }
}

fn subtype(hdr: bool) -> COLORPROFILESUBTYPE {
    if hdr {
        CPST_EXTENDED_DISPLAY_COLOR_MODE
    } else {
        CPST_STANDARD_DISPLAY_COLOR_MODE
    }
}

/// Adds the association; with `set_default` also asks Windows to make it the default. Windows silently
/// ignores that flag when the profile is already associated, so use [`set_default`] to change defaults.
pub fn associate(m: &Monitor, file_name: &str, set_default: bool, hdr: bool) -> Result<(), String> {
    checked(file_name)?;
    let rc = unsafe {
        ColorProfileAddDisplayAssociation(
            SCOPE,
            wide(file_name).as_ptr(),
            m.adapter.to_win(),
            m.source_id,
            set_default as i32,
            hdr as i32,
        )
    };
    if rc >= 0 {
        Ok(())
    } else {
        Err(hr(&format!("associating {file_name}"), rc))
    }
}

/// Makes an already associated profile the default.
fn set_default_association(m: &Monitor, file_name: &str, hdr: bool) -> Result<(), String> {
    let rc = unsafe {
        ColorProfileSetDisplayDefaultAssociation(
            SCOPE,
            wide(file_name).as_ptr(),
            CPT_ICC,
            subtype(hdr),
            m.adapter.to_win(),
            m.source_id,
        )
    };
    if rc >= 0 {
        Ok(())
    } else {
        Err(hr(&format!("making {file_name} the default"), rc))
    }
}

/// Start of the error [`set_default`] returns when Windows keeps another default; safe to show to users.
pub const KEPT_PREFIX: &str = "Windows kept ";

/// `Ok` when `current` (the default Windows reports) is `wanted`.
fn check_default(current: Option<&str>, wanted: &str, monitor_num: u32) -> Result<(), String> {
    match current {
        Some(c) if c.eq_ignore_ascii_case(wanted) => Ok(()),
        Some(c) => Err(format!(
            "{KEPT_PREFIX}{c} as the default for monitor {monitor_num}."
        )),
        None => Err(format!(
            "Windows reports no default profile for monitor {monitor_num} after setting {wanted}."
        )),
    }
}

/// Makes `file_name` the default profile of the monitor (SDR, or advanced color when `hdr`) and verifies
/// it by reading the default back. `ColorProfileAddDisplayAssociation(setAsDefault)` is ignored by
/// Windows when the profile is already associated, so that case goes through
/// `ColorProfileSetDisplayDefaultAssociation`.
pub fn set_default(m: &Monitor, file_name: &str, hdr: bool) -> Result<(), String> {
    checked(file_name)?;
    let already = associated(m, hdr)
        .map(|list| list.iter().any(|f| f.eq_ignore_ascii_case(file_name)))
        .unwrap_or(false);
    if already {
        set_default_association(m, file_name, hdr)?;
    } else {
        associate(m, file_name, true, hdr)?;
    }
    let mut verdict = check_default(default_profile(m, hdr).as_deref(), file_name, m.num);
    if verdict.is_err() && !already {
        // The association list could not be read and the profile was there after all: set it explicitly.
        if set_default_association(m, file_name, hdr).is_ok() {
            verdict = check_default(default_profile(m, hdr).as_deref(), file_name, m.num);
        }
    }
    verdict
}

pub fn disassociate(m: &Monitor, file_name: &str, hdr: bool) -> Result<(), String> {
    checked(file_name)?;
    let rc = unsafe {
        ColorProfileRemoveDisplayAssociation(
            SCOPE,
            wide(file_name).as_ptr(),
            m.adapter.to_win(),
            m.source_id,
            hdr as i32,
        )
    };
    if rc >= 0 {
        Ok(())
    } else {
        Err(hr(&format!("removing association of {file_name}"), rc))
    }
}

/// Reads a NUL-terminated wide string. Returns "" for null.
unsafe fn read_pwstr(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *p.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
}

/// File name (no directory) of the default profile; `None` when none is set.
pub fn default_profile(m: &Monitor, hdr: bool) -> Option<String> {
    let subtype = subtype(hdr);
    unsafe {
        let mut name: PWSTR = std::ptr::null_mut();
        let rc = ColorProfileGetDisplayDefault(
            SCOPE,
            m.adapter.to_win(),
            m.source_id,
            CPT_ICC,
            subtype,
            &mut name,
        );
        if rc < 0 || name.is_null() {
            return None;
        }
        let s = read_pwstr(name);
        LocalFree(name as *mut _);
        let file = s.rsplit(['\\', '/']).next().unwrap_or("").to_string();
        (!file.is_empty()).then_some(file)
    }
}

/// Profiles associated with the monitor, as file names: standard (SDR) ones, or advanced-color (HDR)
/// ones when `hdr`. `Err` when the query itself fails (an empty `Ok` is a real, empty list), so callers
/// never mistake a failed query for "no associations". Uses `WcsEnumColorProfiles` with the monitor's device id plus
/// `ET_STANDARDDISPLAYCOLOR` / `ET_EXTENDEDDISPLAYCOLOR`: `ColorProfileGetDisplayList` returned an
/// empty list for every query on Windows 11 26300, even though the associations exist.
pub fn associated(m: &Monitor, hdr: bool) -> Result<Vec<String>, String> {
    let Some(device_id) = monitor_device_id(m) else {
        return Err(format!("cannot resolve monitor device id for {}", m.gdi_name));
    };
    let device = wide(&device_id);
    unsafe {
        let mut e: ENUMTYPEW = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<ENUMTYPEW>() as u32;
        e.dwVersion = ENUM_TYPE_VERSION;
        e.dwFields = ET_DEVICENAME
            | if hdr {
                ET_EXTENDEDDISPLAYCOLOR
            } else {
                ET_STANDARDDISPLAYCOLOR
            };
        e.pDeviceName = device.as_ptr();
        let mut size = 0u32;
        if WcsEnumColorProfilesSize(SCOPE, &e, &mut size) == 0 {
            return Err(format!(
                "WcsEnumColorProfilesSize({device_id}) failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        if size < 4 {
            return Ok(Vec::new());
        }
        let mut buf = vec![0u8; size as usize];
        let mut count = 0u32;
        if WcsEnumColorProfiles(SCOPE, &e, buf.as_mut_ptr(), size, &mut count) == 0 {
            return Err(format!(
                "WcsEnumColorProfiles({device_id}) failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(parse_profile_list(&buf))
    }
}

/// Splits the double-NUL-terminated UTF-16 list `WcsEnumColorProfiles` fills in into file names.
fn parse_profile_list(buf: &[u8]) -> Vec<String> {
    let w: Vec<u16> = buf
        .chunks(2)
        .map(|c| u16::from_le_bytes([c[0], c.get(1).copied().unwrap_or(0)]))
        .collect();
    w.split(|&c| c == 0)
        .filter(|s| !s.is_empty())
        .map(String::from_utf16_lossy)
        .map(|s| s.rsplit(['\\', '/']).next().unwrap_or("").to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Reference spaces that become monitor profiles: (space id, picker label, file name). sRGB is the inbox
/// profile, the rest are generated into the color folder on first use.
pub const REFERENCE_PROFILES: [(&str, &str, &str); 6] = [
    ("srgb", "sRGB", SRGB_INBOX),
    ("display-p3", "Display P3", "Spectrum Display P3.icm"),
    ("adobe-rgb", "Adobe RGB", "Spectrum Adobe RGB compatible.icm"),
    ("prophoto", "ProPhoto RGB", "Spectrum ProPhoto RGB.icm"),
    ("rec2020", "Rec. 2020", "Spectrum Rec. 2020.icm"),
    ("dci-p3", "DCI-P3", "Spectrum DCI-P3.icm"),
];

/// File name and ICC description of a generated reference profile ("Spectrum Display P3.icm" and
/// "Spectrum Display P3"); `None` for sRGB (the inbox profile) and unknown ids.
pub fn reference_profile_spec(space_id: &str) -> Option<(&'static str, String)> {
    let (_, _, file) = REFERENCE_PROFILES
        .iter()
        .find(|(id, _, f)| *id == space_id && !f.eq_ignore_ascii_case(SRGB_INBOX))?;
    let desc = file.strip_suffix(".icm").unwrap_or(file).to_string();
    Some((file, desc))
}

/// "srgb" gives SRGB_INBOX; the other five give "Spectrum <name>.icm" (generated via icc_write if missing).
pub fn ensure_reference_profile(space_id: &str) -> Result<String, String> {
    if space_id == "srgb" {
        return Ok(SRGB_INBOX.to_string());
    }
    let (file, desc) =
        reference_profile_spec(space_id).ok_or_else(|| format!("no reference profile for {space_id:?}"))?;
    if profile_exists(file) || restore_from_backup(file) {
        return Ok(file.to_string());
    }
    let sp = crate::color::space(space_id).ok_or_else(|| format!("unknown color space {space_id:?}"))?;
    let bytes = crate::icc_write::build_sdr_profile(&desc, &sp.prims, sp.trc);
    install_profile(file, &bytes)?;
    Ok(file.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_validation() {
        assert!(valid_file_name("Spectrum Test.icm"));
        assert!(valid_file_name("Spectrum HDR SPC-0001 A.icc"));
        assert!(!valid_file_name(""));
        assert!(!valid_file_name(".."));
        assert!(!valid_file_name("..\\evil.icm"));
        assert!(!valid_file_name("C:\\x.icm"));
        assert!(!valid_file_name("a/b.icm"));
    }

    fn utf16_list(items: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for s in items {
            for c in s.encode_utf16().chain(std::iter::once(0)) {
                out.extend_from_slice(&c.to_le_bytes());
            }
        }
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn profile_list_parsing() {
        let buf = utf16_list(&["C:\\Windows\\a.icm", "b.icc"]);
        assert_eq!(parse_profile_list(&buf), vec!["a.icm", "b.icc"]);
        assert!(parse_profile_list(&utf16_list(&[])).is_empty());
        assert!(parse_profile_list(&[]).is_empty());
    }

    #[test]
    fn reference_profile_names() {
        for (id, label, file) in REFERENCE_PROFILES {
            assert!(valid_file_name(file), "{file}");
            assert!(crate::color::space(id).is_some(), "{id}");
            assert!(!label.is_empty());
            match reference_profile_spec(id) {
                Some((f, desc)) => {
                    assert_eq!(f, file);
                    assert!(f.starts_with("Spectrum ") && f.ends_with(".icm"));
                    assert_eq!(format!("{desc}.icm"), f);
                }
                None => assert_eq!(id, "srgb"),
            }
        }
        assert_eq!(REFERENCE_PROFILES.len(), crate::color::STANDARD_SPACES.len());
    }

    #[test]
    fn default_check() {
        assert!(check_default(Some("a.icm"), "A.ICM", 1).is_ok());
        assert_eq!(
            check_default(Some("b.icm"), "a.icm", 2).unwrap_err(),
            "Windows kept b.icm as the default for monitor 2."
        );
        assert!(check_default(None, "a.icm", 1).is_err());
    }

    #[test]
    fn delete_rules() {
        assert_eq!(delete_decision("Spectrum Test.icm", true), Ok(()));
        assert_eq!(
            delete_decision("Spectrum Test.icm", false),
            Err(Undeletable::NoAccess)
        );
        // The inbox profile is never offered, even with access.
        assert_eq!(delete_decision(SRGB_INBOX, true), Err(Undeletable::Inbox));
        assert_eq!(
            delete_decision("srgb color space profile.ICM", true),
            Err(Undeletable::Inbox)
        );
        assert_eq!(delete_decision("..\\x.icm", true), Err(Undeletable::Invalid));
        for u in [Undeletable::Inbox, Undeletable::NoAccess, Undeletable::Invalid] {
            assert!(u.hint().split_whitespace().count() <= 8);
        }
    }

    #[test]
    fn inbox_profile_is_not_deletable_here() {
        assert_eq!(can_delete(SRGB_INBOX), Err(Undeletable::Inbox));
        assert!(can_delete("no such file.icm").is_err());
    }

    #[test]
    fn read_profile_guards() {
        assert!(read_profile("..\\x.icm").is_none());
        assert!(read_profile(SRGB_INBOX).is_some());
        assert!(profile_stamp(SRGB_INBOX).is_some());
        assert!(profile_stamp("no such file.icm").is_none());
    }

    #[test]
    fn color_dir_exists() {
        let d = color_dir();
        assert!(d.is_dir(), "{}", d.display());
        assert!(list_installed().iter().any(|n| n == SRGB_INBOX));
        assert!(profile_exists(SRGB_INBOX));
    }
}
