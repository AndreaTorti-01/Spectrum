//! Per-user self-install and uninstall: exe copy, icon, Start Menu shortcut, Run key, Uninstall entry.

use crate::util::{from_wide, wide};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, GetCurrentProcessId, OpenProcess, WaitForSingleObject, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows_sys::Win32::UI::Shell::{FOLDERID_LocalAppData, FOLDERID_Programs, SHGetKnownFolderPath};

const EXE_NAME: &str = "Spectrum.exe";
const ICO_NAME: &str = "spectrum.ico";
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const UNINSTALL_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Spectrum";
const RUN_VALUE: &str = "Spectrum";

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

/// `%LOCALAPPDATA%\Programs\Spectrum`
pub fn install_dir() -> PathBuf {
    let base = known_folder(&FOLDERID_LocalAppData)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);
    base.join("Programs").join("Spectrum")
}

pub fn installed_exe() -> PathBuf {
    install_dir().join(EXE_NAME)
}

/// `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Spectrum.lnk`
fn shortcut_path() -> PathBuf {
    let base = known_folder(&FOLDERID_Programs).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default())
            .join("Microsoft\\Windows\\Start Menu\\Programs")
    });
    base.join("Spectrum.lnk")
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        p.canonicalize()
            .unwrap_or_else(|_| p.to_path_buf())
            .to_string_lossy()
            .trim_start_matches("\\\\?\\")
            .to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

/// True when the running exe is the installed copy.
pub fn is_running_installed() -> bool {
    match std::env::current_exe() {
        Ok(exe) => same_path(&exe, &installed_exe()),
        Err(_) => false,
    }
}

// ---- Pure string builders (unit tested) ----

fn q(p: &Path) -> String {
    format!("\"{}\"", p.display())
}

/// Value of the Run key: `"<exe>" --tray`.
fn run_command(exe: &Path) -> String {
    format!("{} --tray", q(exe))
}

/// `UninstallString`: `"<exe>" --uninstall`.
fn uninstall_string(exe: &Path) -> String {
    format!("{} --uninstall", q(exe))
}

enum RegValue {
    Sz(String),
    Dword(u32),
}

/// Everything written under `HKCU\...\Uninstall\Spectrum`.
fn uninstall_values(dir: &Path, exe: &Path, ico: &Path, size_kb: u32) -> Vec<(&'static str, RegValue)> {
    use RegValue::{Dword, Sz};
    vec![
        ("DisplayName", Sz("Spectrum".into())),
        ("DisplayIcon", Sz(ico.display().to_string())),
        ("DisplayVersion", Sz(env!("CARGO_PKG_VERSION").into())),
        ("Publisher", Sz("Spectrum contributors".into())),
        ("UninstallString", Sz(uninstall_string(exe))),
        ("InstallLocation", Sz(dir.display().to_string())),
        ("NoModify", Dword(1)),
        ("NoRepair", Dword(1)),
        ("EstimatedSize", Dword(size_kb)),
    ]
}

/// Command line of the cleanup helper: `"<copy>" --finish-uninstall "<dir>" <pid>`.
fn finish_uninstall_command(copy: &Path, dir: &Path, pid: u32) -> String {
    format!("{} --finish-uninstall {} {pid}", q(copy), q(dir))
}

/// Where the cleanup helper copy of the exe goes: `%TEMP%\Spectrum-uninstall-<pid>.exe`.
fn uninstall_copy_path(pid: u32) -> PathBuf {
    std::env::temp_dir().join(format!("Spectrum-uninstall-{pid}.exe"))
}

// ---- Registry helpers ----

fn open_key(sub: &str, access: u32, create: bool) -> Option<HKEY> {
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        let rc = if create {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(sub).as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                access,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        } else {
            RegOpenKeyExW(HKEY_CURRENT_USER, wide(sub).as_ptr(), 0, access, &mut key)
        };
        (rc == 0).then_some(key)
    }
}

fn set_value(key: HKEY, name: &str, v: &RegValue) -> Result<(), String> {
    let n = wide(name);
    let rc = unsafe {
        match v {
            RegValue::Sz(s) => {
                let w = wide(s);
                RegSetValueExW(
                    key,
                    n.as_ptr(),
                    0,
                    REG_SZ,
                    w.as_ptr() as *const u8,
                    (w.len() * 2) as u32,
                )
            }
            RegValue::Dword(d) => {
                RegSetValueExW(key, n.as_ptr(), 0, REG_DWORD, d as *const u32 as *const u8, 4)
            }
        }
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!(
            "Couldn't write {name} to the registry (error {rc}). Run the installer again."
        ))
    }
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

fn copy_exe(src: &Path, dst: &Path) -> Result<(), String> {
    if same_path(src, dst) {
        return Ok(());
    }
    match std::fs::copy(src, dst) {
        Ok(_) => Ok(()),
        Err(e) => match e.raw_os_error() {
            // ERROR_ACCESS_DENIED / ERROR_SHARING_VIOLATION / ERROR_LOCK_VIOLATION
            Some(5) | Some(32) | Some(33) => Err(format!(
                "{} is in use. Exit Spectrum from its tray menu, then install again.",
                dst.display()
            )),
            _ => Err(format!("Couldn't copy Spectrum to {}: {e}", dst.display())),
        },
    }
}

/// Copies the exe, writes spectrum.ico, creates the Start Menu shortcut and Uninstall key, optionally autostart.
pub fn install(autostart: bool) -> Result<PathBuf, String> {
    let dir = install_dir();
    let exe = installed_exe();
    let ico = dir.join(ICO_NAME);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;

    if !is_running_installed() {
        let src = std::env::current_exe().map_err(|e| format!("Couldn't locate Spectrum.exe: {e}"))?;
        copy_exe(&src, &exe)?;
    }
    std::fs::write(&ico, crate::icon::ico_bytes())
        .map_err(|e| format!("Couldn't write {}: {e}", ico.display()))?;

    let lnk = shortcut_path();
    if let Some(parent) = lnk.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    create_shortcut(&ShortcutSpec {
        lnk: &lnk,
        target: &exe,
        args: "",
        icon: &ico,
        description: "Spectrum: color profiles for your monitors",
        work_dir: &dir,
    })
    .map_err(|e| format!("Couldn't create the Start menu shortcut: {e}"))?;

    let size_kb = ((std::fs::metadata(&exe).map(|m| m.len()).unwrap_or(0)
        + std::fs::metadata(&ico).map(|m| m.len()).unwrap_or(0))
        / 1024) as u32;
    let key = open_key(UNINSTALL_KEY, KEY_WRITE, true).ok_or_else(|| {
        "Couldn't create the Uninstall registry entry. Run the installer again.".to_string()
    })?;
    let mut res = Ok(());
    for (name, v) in uninstall_values(&dir, &exe, &ico, size_kb) {
        if let Err(e) = set_value(key, name, &v) {
            res = Err(e);
            break;
        }
    }
    unsafe { RegCloseKey(key) };
    res?;

    set_autostart(autostart)?;
    Ok(exe)
}

/// Removes the Run key value, the Start menu shortcut and the Uninstall key.
pub fn remove_entries() {
    let _ = set_autostart(false);
    let _ = std::fs::remove_file(shortcut_path());
    unsafe {
        RegDeleteTreeW(HKEY_CURRENT_USER, wide(UNINSTALL_KEY).as_ptr());
    }
}

/// Starts the cleanup helper that deletes the install folder once this process has exited: a copy of
/// this exe in `%TEMP%` run with `--finish-uninstall`. Call it as the very last step before exiting.
pub fn schedule_dir_removal() -> Result<(), String> {
    let dir = install_dir();
    if is_install_dir_name(&dir) && dir.exists() {
        spawn_finish_uninstall(&dir)?;
    }
    Ok(())
}

fn is_install_dir_name(dir: &Path) -> bool {
    dir.file_name().map(|n| n == "Spectrum").unwrap_or(false)
}

fn spawn_finish_uninstall(dir: &Path) -> Result<(), String> {
    let manual = "Delete the Spectrum folder in %LOCALAPPDATA%\\Programs by hand.";
    let pid = unsafe { GetCurrentProcessId() };
    let copy = uninstall_copy_path(pid);
    let me = std::env::current_exe().map_err(|e| format!("Couldn't locate Spectrum: {e}. {manual}"))?;
    std::fs::copy(&me, &copy).map_err(|e| format!("Couldn't prepare the cleanup: {e}. {manual}"))?;
    let mut cmd = wide(&finish_uninstall_command(&copy, dir, pid));
    unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            wide(&copy.to_string_lossy()).as_ptr(),
            cmd.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            &si,
            &mut pi,
        );
        if ok == 0 {
            return Err(format!(
                "Couldn't start the cleanup process: {}. {manual}",
                std::io::Error::last_os_error()
            ));
        }
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
    }
    Ok(())
}

/// The `--finish-uninstall <dir> <pid>` side: waits (at most 10 s) for process `pid` to exit, then deletes
/// `dir`, but only when it is exactly Spectrum's install folder. Returns the exit code.
pub fn finish_uninstall(dir: &Path, pid: u32) -> i32 {
    if !same_path(dir, &install_dir()) || !is_install_dir_name(dir) {
        crate::util::log(&format!(
            "finish-uninstall: refusing to delete {}, it is not the install folder",
            dir.display()
        ));
        return 2;
    }
    unsafe {
        let h = OpenProcess(SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            WaitForSingleObject(h, 10_000);
            CloseHandle(h);
        }
    }
    // Antivirus or the shell may hold a file briefly: retry a few times.
    for _ in 0..10 {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => return 0,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return 0,
            Err(e) => {
                crate::util::log(&format!("finish-uninstall: {e}"));
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
    1
}

/// Where the install stands, for `--debug-dump` and the headless install check.
#[derive(Clone, Debug)]
pub struct InstallState {
    pub exe: PathBuf,
    pub exe_exists: bool,
    pub running_installed: bool,
    /// Data of the Run key value, if set.
    pub run_value: Option<String>,
    pub shortcut: PathBuf,
    pub shortcut_exists: bool,
    pub uninstall_key_exists: bool,
}

fn read_run_value() -> Option<String> {
    let key = open_key(RUN_KEY, KEY_READ, false)?;
    let name = wide(RUN_VALUE);
    let mut size = 0u32;
    // SAFETY: size query, then a read into a buffer of exactly that many bytes.
    let out = unsafe {
        let rc = RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        );
        if rc != 0 || size < 2 {
            None
        } else {
            let mut buf = vec![0u16; (size as usize).div_ceil(2)];
            let rc = RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut u8,
                &mut size,
            );
            (rc == 0).then(|| from_wide(&buf))
        }
    };
    // SAFETY: key was opened above.
    unsafe { RegCloseKey(key) };
    out
}

pub fn state() -> InstallState {
    let exe = installed_exe();
    let shortcut = shortcut_path();
    let uninstall_key_exists = match open_key(UNINSTALL_KEY, KEY_READ, false) {
        Some(k) => {
            // SAFETY: key was opened above.
            unsafe { RegCloseKey(k) };
            true
        }
        None => false,
    };
    InstallState {
        exe_exists: exe.exists(),
        exe,
        running_installed: is_running_installed(),
        run_value: read_run_value(),
        shortcut_exists: shortcut.exists(),
        shortcut,
        uninstall_key_exists,
    }
}

pub fn autostart_enabled() -> bool {
    let Some(key) = open_key(RUN_KEY, KEY_READ, false) else {
        return false;
    };
    let mut size = 0u32;
    let rc = unsafe {
        RegQueryValueExW(
            key,
            wide(RUN_VALUE).as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    unsafe { RegCloseKey(key) };
    rc == 0 && size > 2
}

pub fn set_autostart(on: bool) -> Result<(), String> {
    let key = open_key(RUN_KEY, KEY_WRITE, true)
        .ok_or_else(|| "Couldn't open the Run registry key. Run the installer again.".to_string())?;
    let r = if on {
        let mut exe = installed_exe();
        if !exe.exists() {
            if let Ok(cur) = std::env::current_exe() {
                exe = cur;
            }
        }
        set_value(key, RUN_VALUE, &RegValue::Sz(run_command(&exe)))
    } else {
        let rc = unsafe { RegDeleteValueW(key, wide(RUN_VALUE).as_ptr()) };
        // 2 = ERROR_FILE_NOT_FOUND: already off.
        if rc == 0 || rc == 2 {
            Ok(())
        } else {
            Err(format!("Couldn't remove the Run entry (error {rc})"))
        }
    };
    unsafe { RegCloseKey(key) };
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirs_are_per_user() {
        let d = install_dir();
        assert!(d.ends_with("Programs\\Spectrum"), "{}", d.display());
        assert!(installed_exe().ends_with("Programs\\Spectrum\\Spectrum.exe"));
        assert!(shortcut_path()
            .to_string_lossy()
            .ends_with("Programs\\Spectrum.lnk"));
    }

    #[test]
    fn registry_strings() {
        let exe = Path::new("C:\\Users\\a b\\AppData\\Local\\Programs\\Spectrum\\Spectrum.exe");
        assert_eq!(
            run_command(exe),
            "\"C:\\Users\\a b\\AppData\\Local\\Programs\\Spectrum\\Spectrum.exe\" --tray"
        );
        assert!(uninstall_string(exe).ends_with("Spectrum.exe\" --uninstall"));
        let vals = uninstall_values(
            exe.parent().unwrap(),
            exe,
            &exe.with_file_name("spectrum.ico"),
            123,
        );
        let get = |n: &str| vals.iter().find(|(k, _)| *k == n).map(|(_, v)| v);
        assert!(matches!(get("DisplayName"), Some(RegValue::Sz(s)) if s == "Spectrum"));
        assert!(matches!(get("Publisher"), Some(RegValue::Sz(s)) if s == "Spectrum contributors"));
        assert!(matches!(get("NoModify"), Some(RegValue::Dword(1))));
        assert!(matches!(get("NoRepair"), Some(RegValue::Dword(1))));
        assert!(matches!(get("EstimatedSize"), Some(RegValue::Dword(123))));
        assert!(matches!(get("DisplayVersion"), Some(RegValue::Sz(s)) if s == env!("CARGO_PKG_VERSION")));
        assert!(matches!(get("DisplayIcon"), Some(RegValue::Sz(s)) if s.ends_with("spectrum.ico")));
    }

    #[test]
    fn finish_uninstall_line() {
        let c = finish_uninstall_command(
            Path::new(r"C:\t\Spectrum-uninstall-7.exe"),
            Path::new(r"C:\x\Spectrum"),
            7,
        );
        assert_eq!(
            c,
            r#""C:\t\Spectrum-uninstall-7.exe" --finish-uninstall "C:\x\Spectrum" 7"#
        );
    }

    #[test]
    fn finish_uninstall_refuses_other_dirs() {
        let other = std::env::temp_dir().join("Spectrum");
        assert_eq!(finish_uninstall(&other, 0), 2);
        assert_eq!(finish_uninstall(Path::new(r"C:\Windows"), 0), 2);
    }

    #[test]
    fn path_comparison_ignores_case() {
        assert!(same_path(
            Path::new("C:\\Foo\\Bar.exe"),
            Path::new("c:\\foo\\bar.EXE")
        ));
        assert!(!same_path(
            Path::new("C:\\Foo\\Bar.exe"),
            Path::new("C:\\Foo\\Baz.exe")
        ));
    }

    #[test]
    fn shortcut_roundtrip_in_temp() {
        let dir = std::env::temp_dir().join("spectrum-lnk-test");
        let _ = std::fs::create_dir_all(&dir);
        let lnk = dir.join("t.lnk");
        let _ = std::fs::remove_file(&lnk);
        let target = std::env::current_exe().unwrap();
        create_shortcut(&ShortcutSpec {
            lnk: &lnk,
            target: &target,
            args: "--tray",
            icon: &target,
            description: "test",
            work_dir: &dir,
        })
        .unwrap();
        assert!(lnk.metadata().unwrap().len() > 100);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
