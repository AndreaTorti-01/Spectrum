//! Active monitor enumeration (QueryDisplayConfig), EDID lookup from the registry, stable keys and HDR toggling.

use crate::util::{from_wide, wide};
use windows_sys::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, DisplayConfigSetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
    DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE, DISPLAYCONFIG_DEVICE_INFO_TYPE,
    DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};
use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, LUID};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Luid {
    pub low: u32,
    pub high: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Clone, Debug)]
pub struct Monitor {
    pub num: u32,
    pub gdi_name: String,
    pub rect: Rect,
    pub adapter: Luid,
    pub source_id: u32,
    pub target_id: u32,
    pub friendly_name: String,
    pub device_path: String,
    pub edid: Option<crate::edid::Edid>,
    pub key: String,
    pub hdr_supported: bool,
    pub hdr_enabled: bool,
    /// 0xRRGGBB
    pub accent: u32,
}

pub const ACCENTS: [u32; 6] = [0xFF5A3C, 0x2EC4B6, 0xFFC93C, 0x4D9DE0, 0x9BD94A, 0xF28AB2];

impl Luid {
    pub(crate) fn to_win(self) -> LUID {
        LUID {
            LowPart: self.low,
            HighPart: self.high,
        }
    }
}

// ---- DisplayConfig structs missing from windows-sys 0.59 (Windows SDK 10.0.26100 wingdi.h) ----

/// DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2 (24H2).
const GET_ADVANCED_COLOR_INFO_2: DISPLAYCONFIG_DEVICE_INFO_TYPE = 15;
/// DISPLAYCONFIG_DEVICE_INFO_SET_HDR_STATE (24H2).
const SET_HDR_STATE: DISPLAYCONFIG_DEVICE_INFO_TYPE = 16;

/// `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2`. `value` bits: 0 advancedColorSupported,
/// 1 advancedColorActive, 2 reserved, 3 advancedColorLimitedByPolicy, 4 highDynamicRangeSupported,
/// 5 highDynamicRangeUserEnabled, 6 wideColorSupported, 7 wideColorUserEnabled.
#[repr(C)]
#[derive(Clone, Copy)]
struct AdvancedColorInfo2 {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    value: u32,
    color_encoding: i32,
    bits_per_color_channel: u32,
    /// 0 = SDR, 1 = WCG, 2 = HDR
    active_color_mode: i32,
}

/// `DISPLAYCONFIG_SET_HDR_STATE`: bit 0 = enableHdr.
#[repr(C)]
#[derive(Clone, Copy)]
struct SetHdrState {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    value: u32,
}

/// What the OS reports about a target's color capabilities.
#[derive(Clone, Copy, Debug, Default)]
struct ColorInfo {
    hdr_supported: bool,
    hdr_enabled: bool,
}

fn header(
    ty: DISPLAYCONFIG_DEVICE_INFO_TYPE,
    size: usize,
    adapter: LUID,
    id: u32,
) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: ty,
        size: size as u32,
        adapterId: adapter,
        id,
    }
}

fn query_color_info_2(adapter: LUID, target_id: u32) -> Option<AdvancedColorInfo2> {
    unsafe {
        let mut info: AdvancedColorInfo2 = std::mem::zeroed();
        info.header = header(
            GET_ADVANCED_COLOR_INFO_2,
            std::mem::size_of::<AdvancedColorInfo2>(),
            adapter,
            target_id,
        );
        if DisplayConfigGetDeviceInfo(&mut info.header) == 0 {
            Some(info)
        } else {
            None
        }
    }
}

fn query_color_info_1(adapter: LUID, target_id: u32) -> Option<u32> {
    unsafe {
        let mut info: DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO = std::mem::zeroed();
        info.header = header(
            DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
            std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>(),
            adapter,
            target_id,
        );
        if DisplayConfigGetDeviceInfo(&mut info.header) == 0 {
            Some(info.Anonymous.value)
        } else {
            None
        }
    }
}

fn color_info(adapter: LUID, target_id: u32) -> ColorInfo {
    if let Some(i) = query_color_info_2(adapter, target_id) {
        return ColorInfo {
            hdr_supported: i.value & (1 << 4) != 0,
            hdr_enabled: i.value & (1 << 5) != 0 || i.active_color_mode == 2,
        };
    }
    if let Some(v) = query_color_info_1(adapter, target_id) {
        // bit0 advancedColorSupported, bit1 advancedColorEnabled, bit2 wideColorEnforced, bit3 forceDisabled
        return ColorInfo {
            hdr_supported: v & 1 != 0,
            hdr_enabled: v & 2 != 0,
        };
    }
    ColorInfo::default()
}

/// Human-readable dump of both advanced-color APIs for one target (used by the dev probe).
#[doc(hidden)]
pub fn color_info_debug(m: &Monitor) -> String {
    let adapter = m.adapter.to_win();
    let v2 = match query_color_info_2(adapter, m.target_id) {
        Some(i) => format!(
            "v2 ok value={:#x} (hdrSupported={} hdrUserEnabled={} acSupported={} acActive={} wcgSupported={} wcgUser={} limitedByPolicy={}) encoding={} bpc={} activeMode={}",
            i.value,
            i.value >> 4 & 1,
            i.value >> 5 & 1,
            i.value & 1,
            i.value >> 1 & 1,
            i.value >> 6 & 1,
            i.value >> 7 & 1,
            i.value >> 3 & 1,
            i.color_encoding,
            i.bits_per_color_channel,
            i.active_color_mode
        ),
        None => "v2 unavailable".to_string(),
    };
    let v1 = match query_color_info_1(adapter, m.target_id) {
        Some(v) => format!(
            "v1 ok value={:#x} (supported={} enabled={} wideEnforced={} forceDisabled={})",
            v,
            v & 1,
            v >> 1 & 1,
            v >> 2 & 1,
            v >> 3 & 1
        ),
        None => "v1 unavailable".to_string(),
    };
    format!("{v2}; {v1}")
}

// ---- Enumeration ----

fn display_number(gdi_name: &str) -> u32 {
    let digits: String = gdi_name
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().unwrap_or(u32::MAX)
}

/// `\\?\DISPLAY#SPC0001#5&2ea131cc&1&UID155907#{guid}` to
/// `SYSTEM\CurrentControlSet\Enum\DISPLAY\SPC0001\5&2ea131cc&1&UID155907\Device Parameters`.
fn edid_registry_path(device_path: &str) -> Option<String> {
    let trimmed = device_path.strip_prefix("\\\\?\\").unwrap_or(device_path);
    let mut parts = trimmed.split('#');
    let class = parts.next()?;
    let hwid = parts.next()?;
    let instance = parts.next()?;
    if class.is_empty() || hwid.is_empty() || instance.is_empty() {
        return None;
    }
    Some(format!(
        "SYSTEM\\CurrentControlSet\\Enum\\{class}\\{hwid}\\{instance}\\Device Parameters"
    ))
}

/// Raw EDID bytes for a monitor device path from `HKLM\...\Enum\DISPLAY\...\Device Parameters\EDID`.
pub fn read_edid_registry(device_path: &str) -> Option<Vec<u8>> {
    let sub = edid_registry_path(device_path)?;
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, wide(&sub).as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return None;
        }
        let name = wide("EDID");
        let mut size: u32 = 0;
        let mut ty: u32 = 0;
        let mut out = None;
        if RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut ty,
            std::ptr::null_mut(),
            &mut size,
        ) == 0
            && size > 0
        {
            let mut buf = vec![0u8; size as usize];
            if RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null(),
                &mut ty,
                buf.as_mut_ptr(),
                &mut size,
            ) == 0
            {
                buf.truncate(size as usize);
                out = Some(buf);
            }
        }
        RegCloseKey(key);
        out
    }
}

fn query_paths() -> Vec<DISPLAYCONFIG_PATH_INFO> {
    unsafe {
        for _ in 0..5 {
            let (mut np, mut nm) = (0u32, 0u32);
            if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) != 0 {
                return Vec::new();
            }
            let mut paths: Vec<DISPLAYCONFIG_PATH_INFO> = vec![std::mem::zeroed(); np as usize];
            let mut modes: Vec<DISPLAYCONFIG_MODE_INFO> = vec![std::mem::zeroed(); nm as usize];
            let rc = QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut np,
                paths.as_mut_ptr(),
                &mut nm,
                modes.as_mut_ptr(),
                std::ptr::null_mut(),
            );
            if rc == ERROR_INSUFFICIENT_BUFFER {
                continue;
            }
            if rc != 0 {
                return Vec::new();
            }
            paths.truncate(np as usize);
            return paths;
        }
        Vec::new()
    }
}

fn source_gdi_name(adapter: LUID, source_id: u32) -> Option<String> {
    unsafe {
        let mut s: DISPLAYCONFIG_SOURCE_DEVICE_NAME = std::mem::zeroed();
        s.header = header(
            DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
            std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>(),
            adapter,
            source_id,
        );
        if DisplayConfigGetDeviceInfo(&mut s.header) != 0 {
            return None;
        }
        Some(from_wide(&s.viewGdiDeviceName))
    }
}

fn target_names(adapter: LUID, target_id: u32) -> (String, String) {
    unsafe {
        let mut t: DISPLAYCONFIG_TARGET_DEVICE_NAME = std::mem::zeroed();
        t.header = header(
            DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
            std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>(),
            adapter,
            target_id,
        );
        if DisplayConfigGetDeviceInfo(&mut t.header) != 0 {
            return (String::new(), String::new());
        }
        (
            from_wide(&t.monitorFriendlyDeviceName),
            from_wide(&t.monitorDevicePath),
        )
    }
}

/// Desktop rectangle in physical pixels for a GDI device name (`\\.\DISPLAY1`).
fn gdi_rect(gdi_name: &str) -> Rect {
    unsafe {
        let mut dm: DEVMODEW = std::mem::zeroed();
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        if EnumDisplaySettingsW(wide(gdi_name).as_ptr(), ENUM_CURRENT_SETTINGS, &mut dm) == 0 {
            return Rect::default();
        }
        let pos = dm.Anonymous1.Anonymous2.dmPosition;
        Rect {
            left: pos.x,
            top: pos.y,
            right: pos.x + dm.dmPelsWidth as i32,
            bottom: pos.y + dm.dmPelsHeight as i32,
        }
    }
}

/// Sorted by DISPLAYn, num = 1..N, accent = ACCENTS[(num-1)%6]; key from EDID (fallback
/// "UNKNOWN-<target_id>"), duplicate keys get "-T<target_id>".
pub fn enumerate() -> Vec<Monitor> {
    enumerate_with(true)
}

/// Same as [`enumerate`], optionally skipping EDID parsing (`edid` stays `None`). For dev probes.
#[doc(hidden)]
pub fn enumerate_with(parse_edid: bool) -> Vec<Monitor> {
    let mut out: Vec<Monitor> = Vec::new();
    for p in query_paths() {
        let src_adapter = p.sourceInfo.adapterId;
        let source_id = p.sourceInfo.id;
        let tgt_adapter = p.targetInfo.adapterId;
        let target_id = p.targetInfo.id;
        let Some(gdi_name) = source_gdi_name(src_adapter, source_id) else {
            continue;
        };
        // Clone mode yields several paths per source; keep the first.
        if out.iter().any(|m| m.gdi_name == gdi_name) {
            continue;
        }
        let (friendly_name, device_path) = target_names(tgt_adapter, target_id);
        let edid = if parse_edid {
            read_edid_registry(&device_path).and_then(|raw| crate::edid::parse(&raw).ok())
        } else {
            None
        };
        let ci = color_info(tgt_adapter, target_id);
        let key = match &edid {
            Some(e) => e.key(),
            None => format!("UNKNOWN-{target_id}"),
        };
        out.push(Monitor {
            num: 0,
            rect: gdi_rect(&gdi_name),
            gdi_name,
            adapter: Luid {
                low: tgt_adapter.LowPart,
                high: tgt_adapter.HighPart,
            },
            source_id,
            target_id,
            friendly_name,
            device_path,
            edid,
            key,
            hdr_supported: ci.hdr_supported,
            hdr_enabled: ci.hdr_enabled,
            accent: 0,
        });
    }
    out.sort_by_key(|m| (display_number(&m.gdi_name), m.gdi_name.clone()));
    for (i, m) in out.iter_mut().enumerate() {
        m.num = i as u32 + 1;
        m.accent = ACCENTS[i % ACCENTS.len()];
    }
    dedupe_keys(&mut out);
    out
}

/// Monitors sharing a key (identical panels with serial 0) get `-T<target_id>` appended.
fn dedupe_keys(mons: &mut [Monitor]) {
    let keys: Vec<String> = mons.iter().map(|m| m.key.clone()).collect();
    for m in mons.iter_mut() {
        if keys.iter().filter(|k| **k == m.key).count() > 1 {
            m.key = format!("{}-T{}", m.key, m.target_id);
        }
    }
}

/// Turns Windows HDR on or off for one monitor (the "Use HDR" switch in Settings).
pub fn set_hdr(m: &Monitor, on: bool) -> Result<(), String> {
    let adapter = m.adapter.to_win();
    // 24H2+: DISPLAYCONFIG_SET_HDR_STATE.
    let rc = unsafe {
        let s = SetHdrState {
            header: header(
                SET_HDR_STATE,
                std::mem::size_of::<SetHdrState>(),
                adapter,
                m.target_id,
            ),
            value: on as u32,
        };
        DisplayConfigSetDeviceInfo(&s.header)
    };
    if rc == 0 {
        return Ok(());
    }
    // Older builds: DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE (bit 0 = enableAdvancedColor).
    let rc2 = unsafe {
        let mut s: windows_sys::Win32::Devices::Display::DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE =
            std::mem::zeroed();
        s.header = header(
            DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE,
            std::mem::size_of::<windows_sys::Win32::Devices::Display::DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE>(
            ),
            adapter,
            m.target_id,
        );
        s.Anonymous.value = on as u32;
        DisplayConfigSetDeviceInfo(&s.header)
    };
    if rc2 == 0 {
        Ok(())
    } else {
        Err(format!(
            "could not switch HDR {} (SET_HDR_STATE error {rc}, SET_ADVANCED_COLOR_STATE error {rc2})",
            if on { "on" } else { "off" }
        ))
    }
}

// ---- SDR content brightness ("SDR content brightness" slider in Settings > Display > HDR) ----

/// `DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL`.
const GET_SDR_WHITE_LEVEL: DISPLAYCONFIG_DEVICE_INFO_TYPE = 11;
/// The undocumented set request (`0xFFFFFFEE`) Twinkle Tray and other tools use.
const SET_SDR_WHITE_LEVEL: DISPLAYCONFIG_DEVICE_INFO_TYPE = -18;

/// `DISPLAYCONFIG_SDR_WHITE_LEVEL`: `level` is in units where 1000 = 80 nits.
#[repr(C)]
#[derive(Clone, Copy)]
struct SdrWhiteLevel {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    level: u32,
}

/// The community definition of the set request: header, level, then a flag byte (1 = final value, not a
/// preview). The compiler pads the struct to 28 bytes, which is what the header's `size` must say.
#[repr(C)]
#[derive(Clone, Copy)]
struct SetSdrWhiteLevel {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    level: u32,
    final_value: u8,
}

/// Verified on Windows 11 26300 (QD-OLED 27, HDR on): the Settings slider s (0..100) is
/// nits = 80 + 4 * s, and the raw value is nits * 1000 / 80 (s = 0, 25, 50, 77, 100 gave 1000, 2250,
/// 3500, 4850, 6000; the registry `MonitorDataStore\<monitor>\SDRWhiteLevel` holds the same raw value).
/// Off-grid values are stored as given and Settings shows the floor of s (82 nits = 0, 246 nits = 41).
/// With HDR off, GET returns 1000 (80 nits) whatever the stored setting is.
/// Raw driver units per nit: 1000 units = 80 nits.
const SDR_UNITS_PER_80_NITS: f64 = 1000.0;
/// Range and step of the "SDR content" slider, in nits.
pub const SDR_WHITE_MIN_NITS: f64 = 80.0;
pub const SDR_WHITE_MAX_NITS: f64 = 480.0;
pub const SDR_WHITE_STEP_NITS: f64 = 4.0;

/// Raw SDR white level to nits.
pub fn sdr_white_raw_to_nits(raw: u32) -> f64 {
    raw as f64 * 80.0 / SDR_UNITS_PER_80_NITS
}

/// Nits to the raw unit Windows stores (rounded to the nearest unit).
pub fn sdr_white_nits_to_raw(nits: f64) -> u32 {
    (nits * SDR_UNITS_PER_80_NITS / 80.0).round().max(0.0) as u32
}

/// Snaps a brightness to the slider grid (4 nits from 80), clamped to 80..=480.
pub fn round_sdr_white_nits(nits: f64) -> f64 {
    let steps = ((nits - SDR_WHITE_MIN_NITS) / SDR_WHITE_STEP_NITS).round();
    (SDR_WHITE_MIN_NITS + steps * SDR_WHITE_STEP_NITS).clamp(SDR_WHITE_MIN_NITS, SDR_WHITE_MAX_NITS)
}

/// The highest SDR content value the slider offers while "Allow clipping above the monitor peak brightness" is
/// off: the EDID HDR max luminance rounded down to the 4-nit grid, clamped to 80..=480. `None` without an
/// EDID max, which means no cap.
pub fn sdr_white_cap(edid_max: Option<f64>) -> Option<u32> {
    let max = edid_max.filter(|v| v.is_finite() && *v > 0.0)?;
    let steps = ((max - SDR_WHITE_MIN_NITS) / SDR_WHITE_STEP_NITS).floor();
    let nits = SDR_WHITE_MIN_NITS + steps * SDR_WHITE_STEP_NITS;
    Some(nits.clamp(SDR_WHITE_MIN_NITS, SDR_WHITE_MAX_NITS) as u32)
}

/// The value SDR content must drop to when the cap applies: `cap` when `current` is above it.
pub fn snap_back(current: u32, cap: u32) -> Option<u32> {
    (current > cap).then_some(cap)
}

/// Raw SDR white level of a monitor (1000 = 80 nits), `None` when Windows does not answer.
pub fn sdr_white_raw(m: &Monitor) -> Option<u32> {
    unsafe {
        let mut s: SdrWhiteLevel = std::mem::zeroed();
        s.header = header(
            GET_SDR_WHITE_LEVEL,
            std::mem::size_of::<SdrWhiteLevel>(),
            m.adapter.to_win(),
            m.target_id,
        );
        if DisplayConfigGetDeviceInfo(&mut s.header) == 0 {
            Some(s.level)
        } else {
            None
        }
    }
}

/// Current SDR content brightness in nits.
pub fn sdr_white_nits(m: &Monitor) -> Option<f64> {
    sdr_white_raw(m).map(sdr_white_raw_to_nits)
}

/// Sets the raw SDR white level (the unit of [`sdr_white_raw`]).
pub fn set_sdr_white_raw(m: &Monitor, raw: u32) -> Result<(), String> {
    let s = SetSdrWhiteLevel {
        header: header(
            SET_SDR_WHITE_LEVEL,
            std::mem::size_of::<SetSdrWhiteLevel>(),
            m.adapter.to_win(),
            m.target_id,
        ),
        level: raw,
        final_value: 1,
    };
    // SAFETY: `s` is a live, correctly sized request; the call only reads it.
    let rc = unsafe { DisplayConfigSetDeviceInfo(&s.header) };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("could not set the SDR white level (error {rc})"))
    }
}

/// Sets the SDR content brightness in nits (snapped to the slider grid).
pub fn set_sdr_white_nits(m: &Monitor, nits: f64) -> Result<(), String> {
    set_sdr_white_raw(m, sdr_white_nits_to_raw(round_sdr_white_nits(nits)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_path_from_device_path() {
        let p = "\\\\?\\DISPLAY#SPC0001#5&2ea131cc&1&UID155907#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}";
        assert_eq!(
            edid_registry_path(p).unwrap(),
            "SYSTEM\\CurrentControlSet\\Enum\\DISPLAY\\SPC0001\\5&2ea131cc&1&UID155907\\Device Parameters"
        );
        assert!(edid_registry_path("garbage").is_none());
        assert!(edid_registry_path("").is_none());
    }

    #[test]
    fn display_numbers_sort_numerically() {
        assert_eq!(display_number("\\\\.\\DISPLAY2"), 2);
        assert!(display_number("\\\\.\\DISPLAY2") < display_number("\\\\.\\DISPLAY10"));
        assert_eq!(display_number("nonsense"), u32::MAX);
    }

    fn fake(key: &str, target: u32) -> Monitor {
        Monitor {
            num: 1,
            gdi_name: String::new(),
            rect: Rect::default(),
            adapter: Luid { low: 0, high: 0 },
            source_id: 0,
            target_id: target,
            friendly_name: String::new(),
            device_path: String::new(),
            edid: None,
            key: key.to_string(),
            hdr_supported: false,
            hdr_enabled: false,
            accent: 0,
        }
    }

    #[test]
    fn duplicate_keys_get_target_suffix() {
        let mut v = vec![fake("A-1-0", 10), fake("A-1-0", 11), fake("B-2-5", 12)];
        dedupe_keys(&mut v);
        assert_eq!(v[0].key, "A-1-0-T10");
        assert_eq!(v[1].key, "A-1-0-T11");
        assert_eq!(v[2].key, "B-2-5");
    }

    #[test]
    fn sdr_white_mapping() {
        assert_eq!(sdr_white_nits_to_raw(80.0), 1000);
        assert_eq!(sdr_white_nits_to_raw(240.0), 3000);
        assert_eq!(sdr_white_nits_to_raw(480.0), 6000);
        assert_eq!(sdr_white_raw_to_nits(1000), 80.0);
        assert_eq!(sdr_white_raw_to_nits(3000), 240.0);
        for n in (80..=480).step_by(4) {
            let n = n as f64;
            assert_eq!(sdr_white_raw_to_nits(sdr_white_nits_to_raw(n)), n);
        }
    }

    #[test]
    fn sdr_white_rounding() {
        assert_eq!(round_sdr_white_nits(79.0), 80.0);
        assert_eq!(round_sdr_white_nits(81.9), 80.0);
        assert_eq!(round_sdr_white_nits(82.1), 84.0);
        assert_eq!(round_sdr_white_nits(241.0), 240.0);
        assert_eq!(round_sdr_white_nits(243.0), 244.0);
        assert_eq!(round_sdr_white_nits(900.0), 480.0);
        assert_eq!(round_sdr_white_nits(0.0), 80.0);
    }

    #[test]
    fn sdr_white_cap_rounds_down_to_the_grid() {
        assert_eq!(sdr_white_cap(Some(455.5)), Some(452));
        assert_eq!(sdr_white_cap(Some(480.0)), Some(480));
        assert_eq!(sdr_white_cap(Some(600.0)), Some(480));
        assert_eq!(sdr_white_cap(Some(79.0)), Some(80));
        assert_eq!(sdr_white_cap(Some(0.0)), None);
        assert_eq!(sdr_white_cap(None), None);
    }

    #[test]
    fn snap_back_only_lowers_values_above_the_cap() {
        assert_eq!(snap_back(480, 452), Some(452));
        assert_eq!(snap_back(452, 452), None);
        assert_eq!(snap_back(240, 452), None);
    }

    #[test]
    fn struct_sizes_match_sdk() {
        assert_eq!(std::mem::size_of::<AdvancedColorInfo2>(), 36);
        assert_eq!(std::mem::size_of::<SetHdrState>(), 24);
        assert_eq!(std::mem::size_of::<SdrWhiteLevel>(), 24);
        assert_eq!(std::mem::size_of::<SetSdrWhiteLevel>(), 28);
    }
}
