//! "Start with Windows" through the package's `windows.startupTask` (`SpectrumStartup` in
//! `packaging/AppxManifest.xml`, which runs `Spectrum.exe --tray`). Windows owns the setting: the user can
//! also change it in Settings › Apps › Startup or Task Manager, and a choice made there wins.
//!
//! The WinRT `Windows.ApplicationModel.StartupTask` class is called through hand-written vtables, and
//! `RoGetActivationFactory` and the HSTRING helpers are loaded from combase.dll at runtime, so no
//! dependency beyond windows-sys is needed. Without package identity (a `cargo run` build) there is no
//! startup task and the state is [`Startup::Unavailable`].

use crate::util::{is_packaged, wide};
use std::ffi::c_void;
use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Threading::Sleep;

/// Argument the startup task passes: start in the tray without opening the window.
pub const TRAY_ARG: &str = "--tray";
/// `desktop:StartupTask/@TaskId` in the manifest.
pub const TASK_ID: &str = "SpectrumStartup";

/// What Windows reports for the startup task (`StartupTaskState`), plus "no package".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Startup {
    /// Not running from the MSIX package, so there is no startup task.
    Unavailable,
    Disabled,
    Enabled,
    /// Turned off in Settings › Apps › Startup or Task Manager: only the user can turn it back on there.
    DisabledByUser,
    DisabledByPolicy,
    EnabledByPolicy,
}

impl Startup {
    fn from_raw(v: i32) -> Startup {
        match v {
            1 => Startup::DisabledByUser,
            2 => Startup::Enabled,
            3 => Startup::DisabledByPolicy,
            4 => Startup::EnabledByPolicy,
            _ => Startup::Disabled,
        }
    }

    /// The plain state after the app turned the task on or off.
    pub fn toggled(on: bool) -> Startup {
        if on {
            Startup::Enabled
        } else {
            Startup::Disabled
        }
    }

    /// Whether Spectrum starts with Windows.
    pub fn is_on(self) -> bool {
        matches!(self, Startup::Enabled | Startup::EnabledByPolicy)
    }

    /// Whether the app's checkbox can change it.
    pub fn can_toggle(self) -> bool {
        matches!(self, Startup::Disabled | Startup::Enabled)
    }

    /// Short hint shown next to a checkbox the app can't change.
    pub fn hint(self) -> Option<&'static str> {
        match self {
            Startup::Unavailable => Some("Needs the installed app"),
            Startup::DisabledByUser => Some("Turn on in Settings › Apps › Startup"),
            Startup::DisabledByPolicy | Startup::EnabledByPolicy => Some("Set by your organization"),
            Startup::Disabled | Startup::Enabled => None,
        }
    }

    /// Name used by the debug dump.
    pub fn name(self) -> &'static str {
        match self {
            Startup::Unavailable => "unavailable (not packaged)",
            Startup::Disabled => "off",
            Startup::Enabled => "on",
            Startup::DisabledByUser => "off (turned off by the user in Windows)",
            Startup::DisabledByPolicy => "off (policy)",
            Startup::EnabledByPolicy => "on (policy)",
        }
    }
}

// ---- WinRT plumbing ----

type Hstring = *mut c_void;
type RoGetActivationFactory = unsafe extern "system" fn(Hstring, *const GUID, *mut *mut c_void) -> i32;
type WindowsCreateString = unsafe extern "system" fn(*const u16, u32, *mut Hstring) -> i32;
type WindowsDeleteString = unsafe extern "system" fn(Hstring) -> i32;
/// What `GetProcAddress` returns.
type Proc = unsafe extern "system" fn() -> isize;

#[repr(C)]
struct IInspectableVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_iids: usize,
    get_runtime_class_name: usize,
    get_trust_level: usize,
}

#[repr(C)]
struct IStartupTaskStaticsVtbl {
    base: IInspectableVtbl,
    get_for_current_package_async: usize,
    get_async: unsafe extern "system" fn(*mut c_void, Hstring, *mut *mut c_void) -> i32,
}

#[repr(C)]
struct IStartupTaskVtbl {
    base: IInspectableVtbl,
    request_enable_async: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
    disable: unsafe extern "system" fn(*mut c_void) -> i32,
    get_state: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32,
    get_task_id: usize,
}

/// `IAsyncOperation<T>` where T is an interface pointer or a 32-bit enum: both fit the out pointer.
#[repr(C)]
struct IAsyncOperationVtbl {
    base: IInspectableVtbl,
    put_completed: usize,
    get_completed: usize,
    get_results: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
}

#[repr(C)]
struct IAsyncInfoVtbl {
    base: IInspectableVtbl,
    get_id: usize,
    get_status: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32,
    get_error_code: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32,
    cancel: usize,
    close: usize,
}

const IID_ISTARTUP_TASK_STATICS: GUID = GUID::from_u128(0xee5b60bd_a148_41a7_b26e_e8b88a1e62f8);
const IID_IASYNC_INFO: GUID = GUID::from_u128(0x00000036_0000_0000_c000_000000000046);
const ASYNC_STARTED: i32 = 0;
const ASYNC_COMPLETED: i32 = 1;
/// Upper bound for one async call; the StartupTask calls finish in milliseconds.
const ASYNC_TIMEOUT_MS: u32 = 5000;

/// The vtable of a COM object.
unsafe fn vtbl<V>(obj: *mut c_void) -> &'static V {
    &**(obj as *const *const V)
}

unsafe fn release(obj: *mut c_void) {
    if !obj.is_null() {
        (vtbl::<IInspectableVtbl>(obj).release)(obj);
    }
}

fn hr_err(what: &str, hr: i32) -> String {
    format!("{what} failed: {:#010x}", hr as u32)
}

/// combase.dll and the three functions taken from it.
struct Combase {
    lib: HMODULE,
    activation_factory: RoGetActivationFactory,
    create_string: WindowsCreateString,
    delete_string: WindowsDeleteString,
}

impl Combase {
    fn load() -> Result<Combase, String> {
        // SAFETY: the module stays loaded until Drop; each pointer is cast to its documented signature.
        unsafe {
            let lib = LoadLibraryW(wide("combase.dll").as_ptr());
            if lib.is_null() {
                return Err("Couldn't load combase.dll".into());
            }
            let get = |name: &std::ffi::CStr| GetProcAddress(lib, name.as_ptr() as *const u8);
            let (Some(a), Some(c), Some(d)) = (
                get(c"RoGetActivationFactory"),
                get(c"WindowsCreateString"),
                get(c"WindowsDeleteString"),
            ) else {
                FreeLibrary(lib);
                return Err("combase.dll lacks the WinRT functions".into());
            };
            Ok(Combase {
                lib,
                activation_factory: std::mem::transmute::<Proc, RoGetActivationFactory>(a),
                create_string: std::mem::transmute::<Proc, WindowsCreateString>(c),
                delete_string: std::mem::transmute::<Proc, WindowsDeleteString>(d),
            })
        }
    }

    /// Runs `f` with an HSTRING holding `s`, deleted afterwards.
    unsafe fn with_hstring<R>(&self, s: &str, f: impl FnOnce(Hstring) -> R) -> Result<R, String> {
        let w: Vec<u16> = s.encode_utf16().collect();
        let mut h: Hstring = std::ptr::null_mut();
        let hr = (self.create_string)(w.as_ptr(), w.len() as u32, &mut h);
        if hr < 0 {
            return Err(hr_err("WindowsCreateString", hr));
        }
        let r = f(h);
        (self.delete_string)(h);
        Ok(r)
    }
}

impl Drop for Combase {
    fn drop(&mut self) {
        // SAFETY: `lib` came from LoadLibraryW and no pointer from it outlives `self`.
        unsafe {
            FreeLibrary(self.lib);
        }
    }
}

/// Waits for `op` (an `IAsyncOperation<T>`) and writes its result to `out`. Polls `IAsyncInfo::Status`:
/// the StartupTask operations complete on a thread-pool thread, so no message pump is needed.
unsafe fn wait_results(op: *mut c_void, out: *mut c_void) -> Result<(), String> {
    let mut info: *mut c_void = std::ptr::null_mut();
    let hr = (vtbl::<IInspectableVtbl>(op).query_interface)(op, &IID_IASYNC_INFO, &mut info);
    if hr < 0 || info.is_null() {
        return Err(hr_err("QueryInterface(IAsyncInfo)", hr));
    }
    let ivt = vtbl::<IAsyncInfoVtbl>(info);
    let mut status = ASYNC_STARTED;
    let mut waited = 0;
    while waited < ASYNC_TIMEOUT_MS {
        let hr = (ivt.get_status)(info, &mut status);
        if hr < 0 {
            release(info);
            return Err(hr_err("IAsyncInfo::Status", hr));
        }
        if status != ASYNC_STARTED {
            break;
        }
        Sleep(2);
        waited += 2;
    }
    let result = if status == ASYNC_COMPLETED {
        let hr = (vtbl::<IAsyncOperationVtbl>(op).get_results)(op, out);
        if hr < 0 {
            Err(hr_err("GetResults", hr))
        } else {
            Ok(())
        }
    } else if status == ASYNC_STARTED {
        Err("The startup task didn't answer in time".into())
    } else {
        let mut code = 0;
        (ivt.get_error_code)(info, &mut code);
        Err(hr_err("StartupTask", code))
    };
    release(info);
    result
}

/// Runs `f` with this package's `SpectrumStartup` task object, inside a COM apartment.
fn with_task<R>(f: impl FnOnce(*mut c_void) -> Result<R, String>) -> Result<R, String> {
    if !is_packaged() {
        return Err("Start with Windows needs the installed app".into());
    }
    let combase = Combase::load()?;
    // SAFETY: COM is initialized for the duration (and uninitialized only when this call did it); every
    // interface pointer obtained here is released before returning, and `f` gets a live task pointer.
    unsafe {
        let init = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        let r = (|| {
            let mut statics: *mut c_void = std::ptr::null_mut();
            let hr = combase.with_hstring("Windows.ApplicationModel.StartupTask", |cls| {
                (combase.activation_factory)(cls, &IID_ISTARTUP_TASK_STATICS, &mut statics)
            })?;
            if hr < 0 || statics.is_null() {
                return Err(hr_err("RoGetActivationFactory(StartupTask)", hr));
            }
            let mut op: *mut c_void = std::ptr::null_mut();
            let hr = combase.with_hstring(TASK_ID, |id| {
                (vtbl::<IStartupTaskStaticsVtbl>(statics).get_async)(statics, id, &mut op)
            })?;
            release(statics);
            if hr < 0 || op.is_null() {
                return Err(hr_err("StartupTask.GetAsync", hr));
            }
            let mut task: *mut c_void = std::ptr::null_mut();
            let waited = wait_results(op, &mut task as *mut _ as *mut c_void);
            release(op);
            waited?;
            if task.is_null() {
                return Err(format!("No startup task named {TASK_ID} in the package"));
            }
            let r = f(task);
            release(task);
            r
        })();
        if init >= 0 {
            CoUninitialize();
        }
        r
    }
}

unsafe fn task_state(task: *mut c_void) -> Result<Startup, String> {
    let mut raw = 0i32;
    let hr = (vtbl::<IStartupTaskVtbl>(task).get_state)(task, &mut raw);
    if hr < 0 {
        return Err(hr_err("StartupTask.State", hr));
    }
    Ok(Startup::from_raw(raw))
}

// ---- Public API ----

/// The startup task's state; `Unavailable` without a package, and `Disabled` when Windows won't say.
pub fn startup_state() -> Startup {
    if !is_packaged() {
        return Startup::Unavailable;
    }
    // SAFETY: `with_task` hands over a live task pointer.
    with_task(|t| unsafe { task_state(t) }).unwrap_or(Startup::Disabled)
}

/// Turns the startup task on or off. A state the user or policy set in Windows can't be changed here.
pub fn set_autostart(on: bool) -> Result<(), String> {
    with_task(|task| {
        // SAFETY: `task` is live for this closure; the async operation is released after use.
        unsafe {
            let state = task_state(task)?;
            if state.is_on() == on {
                return Ok(());
            }
            if !state.can_toggle() {
                return Err(state.hint().unwrap_or("Can't change it here").to_string());
            }
            let tvt = vtbl::<IStartupTaskVtbl>(task);
            if on {
                let mut op: *mut c_void = std::ptr::null_mut();
                let hr = (tvt.request_enable_async)(task, &mut op);
                if hr < 0 || op.is_null() {
                    return Err(hr_err("StartupTask.RequestEnableAsync", hr));
                }
                let mut raw = 0i32;
                let waited = wait_results(op, &mut raw as *mut _ as *mut c_void);
                release(op);
                waited?;
                if !Startup::from_raw(raw).is_on() {
                    return Err(Startup::from_raw(raw)
                        .hint()
                        .unwrap_or("Windows kept it off")
                        .to_string());
                }
            } else {
                let hr = (tvt.disable)(task);
                if hr < 0 {
                    return Err(hr_err("StartupTask.Disable", hr));
                }
            }
            Ok(())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_map_from_startup_task_state() {
        assert_eq!(Startup::from_raw(0), Startup::Disabled);
        assert_eq!(Startup::from_raw(1), Startup::DisabledByUser);
        assert_eq!(Startup::from_raw(2), Startup::Enabled);
        assert_eq!(Startup::from_raw(3), Startup::DisabledByPolicy);
        assert_eq!(Startup::from_raw(4), Startup::EnabledByPolicy);
    }

    #[test]
    fn only_plain_states_are_toggleable_and_the_rest_explain_why() {
        for s in [
            Startup::Unavailable,
            Startup::Disabled,
            Startup::Enabled,
            Startup::DisabledByUser,
            Startup::DisabledByPolicy,
            Startup::EnabledByPolicy,
        ] {
            assert_eq!(s.can_toggle(), s.hint().is_none(), "{s:?}");
        }
        assert!(Startup::EnabledByPolicy.is_on() && !Startup::DisabledByUser.is_on());
    }

    #[test]
    fn unpackaged_test_runs_report_unavailable() {
        // cargo test never runs with package identity.
        assert_eq!(startup_state(), Startup::Unavailable);
        assert!(set_autostart(true).is_err());
    }
}
