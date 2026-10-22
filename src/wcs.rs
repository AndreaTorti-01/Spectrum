//! Windows Color System access: color folder files, per-user profile associations, reference profiles.

use crate::monitors::Monitor;
use std::path::PathBuf;

pub const SRGB_INBOX: &str = "sRGB Color Space Profile.icm";

pub fn color_dir() -> PathBuf {
    todo!("wcs::color_dir")
}

/// `app_data_dir()\profiles`
pub fn backup_dir() -> PathBuf {
    todo!("wcs::backup_dir")
}

/// Writes to the color dir plus a backup copy.
pub fn install_profile(_file_name: &str, _bytes: &[u8]) -> Result<(), String> {
    todo!("wcs::install_profile")
}

/// If missing in the color dir and present in the backup, copies it back.
pub fn restore_from_backup(_file_name: &str) -> bool {
    todo!("wcs::restore_from_backup")
}

pub fn profile_exists(_file_name: &str) -> bool {
    todo!("wcs::profile_exists")
}

pub fn read_profile(_file_name: &str) -> Option<Vec<u8>> {
    todo!("wcs::read_profile")
}

/// Removes from the color dir and the backup.
pub fn delete_profile(_file_name: &str) -> Result<(), String> {
    todo!("wcs::delete_profile")
}

/// `*.icc` / `*.icm` file names in the color dir.
pub fn list_installed() -> Vec<String> {
    todo!("wcs::list_installed")
}

pub fn ensure_user_scope(_m: &Monitor) -> Result<(), String> {
    todo!("wcs::ensure_user_scope")
}

pub fn associate(_m: &Monitor, _file_name: &str, _set_default: bool, _hdr: bool) -> Result<(), String> {
    todo!("wcs::associate")
}

pub fn disassociate(_m: &Monitor, _file_name: &str, _hdr: bool) -> Result<(), String> {
    todo!("wcs::disassociate")
}

pub fn default_profile(_m: &Monitor, _hdr: bool) -> Option<String> {
    todo!("wcs::default_profile")
}

pub fn associated(_m: &Monitor, _hdr: bool) -> Vec<String> {
    todo!("wcs::associated")
}

/// "srgb" gives SRGB_INBOX; "display-p3" gives "Spectrum Display P3.icm"; "adobe-rgb" gives
/// "Spectrum Adobe RGB compatible.icm" (generated via icc_write if missing).
pub fn ensure_reference_profile(_space_id: &str) -> Result<String, String> {
    todo!("wcs::ensure_reference_profile")
}
