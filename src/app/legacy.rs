//! The pre-Store install (Inno Setup, `%LOCALAPPDATA%\Programs\Spectrum`). It shares the profile file
//! names and `%APPDATA%\Spectrum` with the Store build, and its uninstaller deletes both. So on startup
//! the Store build saves its config once (the package then keeps its own copy, out of the uninstaller's
//! reach) and asks the user to uninstall the old copy. After that uninstall, reconcile rebuilds the
//! deleted profile files from the config and the backups.

use super::snapshot::set_error;
use super::Config;
use crate::util::{is_packaged, known_folder, log};
use std::path::PathBuf;
use windows_sys::Win32::UI::Shell::FOLDERID_LocalAppData;

/// Shown in the main window's footer while the old copy is installed.
pub const NOTICE: &str = "An older Spectrum is installed. Uninstall it in Settings › Apps.";

/// Where the Inno installer put the exe.
fn legacy_exe() -> Option<PathBuf> {
    known_folder(&FOLDERID_LocalAppData).map(|d| d.join(r"Programs\Spectrum\Spectrum.exe"))
}

/// On startup of the Store build: keep a package copy of the config and show [`NOTICE`] when the old
/// copy is still installed.
pub(super) fn check() {
    if !is_packaged() || !legacy_exe().is_some_and(|p| p.exists()) {
        return;
    }
    let cfg = Config::load();
    if !cfg.load_failed {
        // Writing through the package's AppData redirection gives the package its own copy.
        cfg.save();
    }
    log("older install found at %LOCALAPPDATA%\\Programs\\Spectrum");
    set_error(Some(NOTICE.to_string()));
}
