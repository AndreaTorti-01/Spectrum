//! "Start with Windows": a `Spectrum.lnk` in the user's Startup folder that runs this exe with `--tray`.
//! The Inno Setup installer creates the same shortcut (see `installer/spectrum.iss`), so toggling the
//! checkbox in the app and the installer task agree on one file.

use crate::util::from_wide;
use crate::util::wide;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use windows_sys::core::GUID;
use windows_sys::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows_sys::Win32::UI::Shell::{FOLDERID_Startup, SHGetKnownFolderPath};

/// File name of the Startup shortcut.
pub const SHORTCUT_NAME: &str = "Spectrum.lnk";
/// Argument the shortcut passes: start in the tray without opening the window.
pub const TRAY_ARG: &str = "--tray";

fn known_folder(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let mut p: *mut u16 = std::ptr::null_mut();
        let hr = SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut p);
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

/// `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup`, the fallback when the shell won't say.
fn fallback_startup_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default())
        .join(r"Microsoft\Windows\Start Menu\Programs\Startup")
}

/// `<startup folder>\Spectrum.lnk`
pub fn shortcut_in(startup_dir: &Path) -> PathBuf {
    startup_dir.join(SHORTCUT_NAME)
}

/// The Startup shortcut of the current user.
pub fn shortcut_path() -> PathBuf {
    shortcut_in(&known_folder(&FOLDERID_Startup).unwrap_or_else(fallback_startup_dir))
}

// ---- Shortcut (hand-written COM vtables) ----

#[repr(C)]
struct IUnknownVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

type Pcwstr = *const u16;

#[repr(C)]
struct IShellLinkWVtbl {
    base: IUnknownVtbl,
    get_path: usize,
    get_id_list: usize,
    set_id_list: usize,
    get_description: usize,
    set_description: unsafe extern "system" fn(*mut c_void, Pcwstr) -> i32,
    get_working_directory: usize,
    set_working_directory: unsafe extern "system" fn(*mut c_void, Pcwstr) -> i32,
    get_arguments: usize,
    set_arguments: unsafe extern "system" fn(*mut c_void, Pcwstr) -> i32,
    get_hotkey: usize,
    set_hotkey: usize,
    get_show_cmd: usize,
    set_show_cmd: usize,
    get_icon_location: usize,
    set_icon_location: unsafe extern "system" fn(*mut c_void, Pcwstr, i32) -> i32,
    set_relative_path: usize,
    resolve: usize,
    set_path: unsafe extern "system" fn(*mut c_void, Pcwstr) -> i32,
}

#[repr(C)]
struct IPersistFileVtbl {
    base: IUnknownVtbl,
    get_class_id: usize,
    is_dirty: usize,
    load: usize,
    save: unsafe extern "system" fn(*mut c_void, Pcwstr, i32) -> i32,
    save_completed: usize,
    get_cur_file: usize,
}

#[repr(C)]
struct ComObj<V> {
    vtbl: *const V,
}

const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
const IID_ISHELL_LINK_W: GUID = GUID::from_u128(0x000214f9_0000_0000_c000_000000000046);
const IID_IPERSIST_FILE: GUID = GUID::from_u128(0x0000010b_0000_0000_c000_000000000046);

struct ShortcutSpec<'a> {
    lnk: &'a Path,
    target: &'a Path,
    args: &'a str,
    icon: &'a Path,
    description: &'a str,
    work_dir: &'a Path,
}

fn create_shortcut(s: &ShortcutSpec) -> Result<(), String> {
    unsafe {
        let init = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        // S_OK / S_FALSE need a matching CoUninitialize; RPC_E_CHANGED_MODE does not (COM still usable).
        let must_uninit = init >= 0;
        let r = create_shortcut_inner(s);
        if must_uninit {
            CoUninitialize();
        }
        r
    }
}

unsafe fn create_shortcut_inner(s: &ShortcutSpec) -> Result<(), String> {
    let mut link: *mut c_void = std::ptr::null_mut();
    let hr = CoCreateInstance(
        &CLSID_SHELL_LINK,
        std::ptr::null_mut(),
        CLSCTX_INPROC_SERVER,
        &IID_ISHELL_LINK_W,
        &mut link,
    );
    if hr < 0 || link.is_null() {
        return Err(format!("CoCreateInstance(ShellLink) failed: {:#010x}", hr as u32));
    }
    let result = (|| {
        let vt = &*(*(link as *const ComObj<IShellLinkWVtbl>)).vtbl;
        let check = |what: &str, hr: i32| {
            if hr < 0 {
                Err(format!("IShellLink::{what} failed: {:#010x}", hr as u32))
            } else {
                Ok(())
            }
        };
        check(
            "SetPath",
            (vt.set_path)(link, wide(&s.target.to_string_lossy()).as_ptr()),
        )?;
        check("SetArguments", (vt.set_arguments)(link, wide(s.args).as_ptr()))?;
        check(
            "SetWorkingDirectory",
            (vt.set_working_directory)(link, wide(&s.work_dir.to_string_lossy()).as_ptr()),
        )?;
        check(
            "SetDescription",
            (vt.set_description)(link, wide(s.description).as_ptr()),
        )?;
        check(
            "SetIconLocation",
            (vt.set_icon_location)(link, wide(&s.icon.to_string_lossy()).as_ptr(), 0),
        )?;
        let mut pf: *mut c_void = std::ptr::null_mut();
        let hr = (vt.base.query_interface)(link, &IID_IPERSIST_FILE, &mut pf);
        if hr < 0 || pf.is_null() {
            return Err(format!(
                "QueryInterface(IPersistFile) failed: {:#010x}",
                hr as u32
            ));
        }
        let pvt = &*(*(pf as *const ComObj<IPersistFileVtbl>)).vtbl;
        let hr = (pvt.save)(pf, wide(&s.lnk.to_string_lossy()).as_ptr(), 1);
        (pvt.base.release)(pf);
        check("Save", hr)
    })();
    let vt = &*(*(link as *const ComObj<IShellLinkWVtbl>)).vtbl;
    (vt.base.release)(link);
    result
}

// ---- Public API ----

pub fn autostart_enabled() -> bool {
    shortcut_path().exists()
}

/// Creates (or replaces) the Startup shortcut for `exe` at `lnk`.
fn write_shortcut(lnk: &Path, exe: &Path) -> Result<(), String> {
    let dir = exe.parent().unwrap_or(Path::new("."));
    create_shortcut(&ShortcutSpec {
        lnk,
        target: exe,
        args: TRAY_ARG,
        icon: exe,
        description: "Spectrum",
        work_dir: dir,
    })
}

pub fn set_autostart(on: bool) -> Result<(), String> {
    let lnk = shortcut_path();
    if on {
        let exe = std::env::current_exe().map_err(|e| format!("Couldn't locate Spectrum.exe: {e}"))?;
        if let Some(parent) = lnk.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        write_shortcut(&lnk, &exe).map_err(|e| format!("Couldn't create the Startup shortcut: {e}"))
    } else {
        match std::fs::remove_file(&lnk) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Couldn't remove the Startup shortcut: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_lives_in_the_startup_folder() {
        let p = shortcut_in(Path::new(
            r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup",
        ));
        assert!(p.ends_with(r"Startup\Spectrum.lnk"), "{}", p.display());
        let real = shortcut_path();
        assert_eq!(real.file_name().unwrap(), "Spectrum.lnk");
        assert!(real.parent().unwrap().ends_with("Startup"), "{}", real.display());
    }

    #[test]
    fn fallback_dir_is_under_appdata() {
        assert!(fallback_startup_dir().ends_with(r"Programs\Startup"));
    }

    #[test]
    fn shortcut_roundtrip_in_temp() {
        let dir = std::env::temp_dir().join("spectrum-lnk-test");
        let _ = std::fs::create_dir_all(&dir);
        let lnk = shortcut_in(&dir);
        let _ = std::fs::remove_file(&lnk);
        let target = std::env::current_exe().unwrap();
        write_shortcut(&lnk, &target).unwrap();
        assert!(lnk.metadata().unwrap().len() > 100);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
