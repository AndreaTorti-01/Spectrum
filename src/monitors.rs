//! Active monitor enumeration (QueryDisplayConfig), EDID lookup from the registry, stable keys and HDR toggling.

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

/// Sorted by DISPLAYn, num = 1..N, accent = ACCENTS[(num-1)%6]; key from EDID (fallback
/// "UNKNOWN-<target_id>"), duplicate keys get "-T<target_id>".
pub fn enumerate() -> Vec<Monitor> {
    todo!("monitors::enumerate")
}

pub fn set_hdr(_m: &Monitor, _on: bool) -> Result<(), String> {
    todo!("monitors::set_hdr")
}
