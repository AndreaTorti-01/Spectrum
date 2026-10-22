//! What Windows reports for a monitor's HDR luminance, read through DXGI (`IDXGIOutput6::GetDesc1`).
//!
//! `dxgi.dll` is loaded on demand and freed again, so the idle tray never keeps it. The COM interfaces are
//! hand-written `#[repr(C)]` vtables with only the slots used here; every slot index below counts
//! IUnknown (QueryInterface 0, AddRef 1, Release 2) and matches the Windows SDK headers
//! (dxgi.h, dxgi1_2.h .. dxgi1_6.h).
//!
//! DXGI caches output descriptions per factory, so each read creates a fresh factory.

use crate::util::{from_wide, wide};
use std::ffi::c_void;
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE, RECT};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Guid(u32, u16, u16, [u8; 8]);

const IID_IDXGIFACTORY1: Guid = Guid(
    0x770a_ae78,
    0xf26f,
    0x4dba,
    [0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87],
);
const IID_IDXGIOUTPUT6: Guid = Guid(
    0x0683_46e8,
    0xaaec,
    0x4b84,
    [0xad, 0xd7, 0x13, 0x7f, 0x51, 0x3f, 0x77, 0xa1],
);

/// `DXGI_OUTPUT_DESC1`.
#[repr(C)]
struct OutputDesc1 {
    device_name: [u16; 32],
    desktop_coordinates: RECT,
    attached_to_desktop: i32,
    rotation: i32,
    monitor: *mut c_void,
    bits_per_color: u32,
    color_space: i32,
    red: [f32; 2],
    green: [f32; 2],
    blue: [f32; 2],
    white: [f32; 2],
    min_luminance: f32,
    max_luminance: f32,
    max_full_frame_luminance: f32,
}

/// IUnknown part shared by every interface.
#[repr(C)]
struct UnknownVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

/// IDXGIFactory1: slots 0..2 IUnknown, 3..6 IDXGIObject, 7..11 IDXGIFactory, 12 EnumAdapters1.
#[repr(C)]
struct Factory1Vtbl {
    unknown: UnknownVtbl,
    _object: [usize; 4],
    _factory: [usize; 5],
    enum_adapters1: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32,
}

/// IDXGIAdapter1 (only the part up to EnumOutputs): 3..6 IDXGIObject, 7 EnumOutputs.
#[repr(C)]
struct AdapterVtbl {
    unknown: UnknownVtbl,
    _object: [usize; 4],
    enum_outputs: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32,
}

/// IDXGIOutput6: 3..6 IDXGIObject, 7..18 IDXGIOutput, 19..22 Output1, 23 Output2, 24 Output3, 25 Output4,
/// 26 Output5, 27 GetDesc1.
#[repr(C)]
struct Output6Vtbl {
    unknown: UnknownVtbl,
    _object: [usize; 4],
    _output: [usize; 12],
    _output1: [usize; 4],
    _output2: usize,
    _output3: usize,
    _output4: usize,
    _output5: usize,
    get_desc1: unsafe extern "system" fn(*mut c_void, *mut OutputDesc1) -> i32,
}

/// Owns one COM reference and releases it on drop.
struct Com(*mut c_void);

impl Com {
    fn vtbl<T>(&self) -> &T {
        // SAFETY: a COM pointer starts with a pointer to its vtable, which outlives the object.
        unsafe { &**(self.0 as *const *const T) }
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let u: &UnknownVtbl = self.vtbl();
            // SAFETY: releases the one reference this wrapper owns.
            unsafe { (u.release)(self.0) };
        }
    }
}

/// What Windows reports for one output.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reported {
    pub min_nits: f64,
    pub max_nits: f64,
    pub max_full_frame_nits: f64,
    /// `DXGI_COLOR_SPACE_TYPE`; 12 is HDR10 (G2084, BT.2020).
    pub color_space: i32,
    pub bits_per_color: u32,
    pub red: [f32; 2],
    pub green: [f32; 2],
    pub blue: [f32; 2],
    pub white: [f32; 2],
}

impl Reported {
    /// Name of the common color spaces, else the number.
    pub fn color_space_name(&self) -> String {
        match self.color_space {
            0 => "RGB full G22 P709 (sRGB)".into(),
            12 => "RGB full G2084 P2020 (HDR10)".into(),
            n => n.to_string(),
        }
    }
}

type CreateFactory1 = unsafe extern "system" fn(*const Guid, *mut *mut c_void) -> i32;

/// Luminance Windows reports for the output named `gdi_name` (`\\.\DISPLAY1`), `None` when DXGI is missing,
/// the output is not found or the query fails.
pub fn reported(gdi_name: &str) -> Option<Reported> {
    // SAFETY: plain library load; freed below once every COM object is released.
    let lib: HMODULE = unsafe { LoadLibraryW(wide("dxgi.dll").as_ptr()) };
    if lib.is_null() {
        return None;
    }
    let r = read(lib, gdi_name);
    // SAFETY: every COM object created from the library was dropped inside `read`.
    unsafe { FreeLibrary(lib) };
    r
}

fn read(lib: HMODULE, gdi_name: &str) -> Option<Reported> {
    // SAFETY: the export has the documented CreateDXGIFactory1 signature.
    let create: CreateFactory1 =
        unsafe { std::mem::transmute(GetProcAddress(lib, c"CreateDXGIFactory1".as_ptr() as *const u8)?) };
    let mut raw: *mut c_void = std::ptr::null_mut();
    // SAFETY: valid IID and out pointer.
    if unsafe { create(&IID_IDXGIFACTORY1, &mut raw) } < 0 || raw.is_null() {
        return None;
    }
    let factory = Com(raw);
    let fv: &Factory1Vtbl = factory.vtbl();
    for ai in 0.. {
        let mut a: *mut c_void = std::ptr::null_mut();
        // SAFETY: valid factory; the call fails with DXGI_ERROR_NOT_FOUND after the last adapter.
        if unsafe { (fv.enum_adapters1)(factory.0, ai, &mut a) } < 0 || a.is_null() {
            break;
        }
        let adapter = Com(a);
        let av: &AdapterVtbl = adapter.vtbl();
        for oi in 0.. {
            let mut o: *mut c_void = std::ptr::null_mut();
            // SAFETY: valid adapter; fails after the last output.
            if unsafe { (av.enum_outputs)(adapter.0, oi, &mut o) } < 0 || o.is_null() {
                break;
            }
            let output = Com(o);
            let ov: &UnknownVtbl = output.vtbl();
            let mut o6: *mut c_void = std::ptr::null_mut();
            // SAFETY: valid output; IDXGIOutput6 exists from Windows 10 1803.
            if unsafe { (ov.query_interface)(output.0, &IID_IDXGIOUTPUT6, &mut o6) } < 0 || o6.is_null() {
                continue;
            }
            let output6 = Com(o6);
            let v6: &Output6Vtbl = output6.vtbl();
            // SAFETY: all-zero is valid for this plain-data struct.
            let mut d: OutputDesc1 = unsafe { std::mem::zeroed() };
            // SAFETY: valid output and a correctly sized out struct.
            if unsafe { (v6.get_desc1)(output6.0, &mut d) } < 0 {
                continue;
            }
            if !from_wide(&d.device_name).eq_ignore_ascii_case(gdi_name) {
                continue;
            }
            return Some(Reported {
                min_nits: d.min_luminance as f64,
                max_nits: d.max_luminance as f64,
                max_full_frame_nits: d.max_full_frame_luminance as f64,
                color_space: d.color_space,
                bits_per_color: d.bits_per_color,
                red: d.red,
                green: d.green,
                blue: d.blue,
                white: d.white,
            });
        }
    }
    None
}

/// "peak 450 · full-frame 250 · black 0.00 nits".
pub fn summary(r: &Reported) -> String {
    format!(
        "peak {} \u{b7} full-frame {} \u{b7} black {:.2} nits",
        r.max_nits.round() as i64,
        r.max_full_frame_nits.round() as i64,
        r.min_nits
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_sdk() {
        // DXGI_OUTPUT_DESC1 is 148 bytes with 8-byte alignment padding on x64 (152).
        assert_eq!(std::mem::offset_of!(OutputDesc1, monitor), 88);
        assert_eq!(std::mem::offset_of!(OutputDesc1, min_luminance), 136);
        assert_eq!(std::mem::size_of::<OutputDesc1>(), 152);
        let p = std::mem::size_of::<usize>();
        assert_eq!(std::mem::offset_of!(Factory1Vtbl, enum_adapters1), 12 * p);
        assert_eq!(std::mem::offset_of!(AdapterVtbl, enum_outputs), 7 * p);
        assert_eq!(std::mem::offset_of!(Output6Vtbl, get_desc1), 27 * p);
    }

    #[test]
    fn unknown_monitor_is_none() {
        assert!(reported("\\\\.\\NOPE").is_none());
    }
}
