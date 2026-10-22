//! EDID parser: base block identity, descriptors, 10-bit chromaticities, plus CTA-861 HDR static metadata.

use crate::color::{Primaries, Trc};

#[derive(Clone, Debug, PartialEq)]
pub struct HdrStatic {
    pub max_nits: Option<f64>,
    pub max_frame_avg_nits: Option<f64>,
    pub min_nits: Option<f64>,
    pub pq: bool,
}

#[derive(Clone, Debug)]
pub struct Edid {
    pub raw: Vec<u8>,
    /// 3 letters
    pub manufacturer: String,
    pub product_code: u16,
    pub serial: u32,
    pub serial_text: Option<String>,
    pub name: Option<String>,
    pub year: u16,
    pub gamma: Option<f64>,
    pub srgb_default: bool,
    pub primaries: Primaries,
    pub hdr: Option<HdrStatic>,
}

/// Validates header + base checksum; extensions are optional.
pub fn parse(_raw: &[u8]) -> Result<Edid, &'static str> {
    todo!("edid::parse")
}

impl Edid {
    /// e.g. "MSI-3CB0-0000012A" (+ "-" + sanitized serial_text if present); filename-safe.
    pub fn key(&self) -> String {
        todo!("edid::Edid::key")
    }

    /// Name descriptor or "MFG PROD".
    pub fn display_name(&self) -> String {
        todo!("edid::Edid::display_name")
    }

    /// `srgb_default` gives Srgb, otherwise Gamma(gamma or 2.2).
    pub fn trc(&self) -> Trc {
        todo!("edid::Edid::trc")
    }
}
