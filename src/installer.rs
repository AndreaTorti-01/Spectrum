//! Per-user self-install and uninstall: exe copy, icon, Start Menu shortcut, Run key, Uninstall entry.

use std::path::PathBuf;

/// `%LOCALAPPDATA%\Programs\Spectrum`
pub fn install_dir() -> PathBuf {
    todo!("installer::install_dir")
}

pub fn installed_exe() -> PathBuf {
    todo!("installer::installed_exe")
}

pub fn is_running_installed() -> bool {
    todo!("installer::is_running_installed")
}

/// Copies the exe, writes spectrum.ico, creates the Start Menu shortcut and Uninstall key, optionally autostart.
pub fn install(_autostart: bool) -> Result<PathBuf, String> {
    todo!("installer::install")
}

/// Removes Run key, shortcut and Uninstall key, then self-deletes via a delayed `cmd /c`.
pub fn uninstall() -> Result<(), String> {
    todo!("installer::uninstall")
}

pub fn autostart_enabled() -> bool {
    todo!("installer::autostart_enabled")
}

pub fn set_autostart(_on: bool) -> Result<(), String> {
    todo!("installer::set_autostart")
}
