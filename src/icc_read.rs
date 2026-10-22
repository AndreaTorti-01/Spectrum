//! ICC profile reader: header, description, native colorants (chad-unadapted), lumi and the MHC2 tag.

use crate::color::Primaries;

#[derive(Clone, Debug, PartialEq)]
pub struct Mhc2 {
    pub min_nits: f64,
    pub peak_nits: f64,
    pub matrix: Option<[[f64; 4]; 3]>,
    pub lut_count: u32,
}

#[derive(Clone, Debug)]
pub struct IccInfo {
    pub version: u32,
    pub class: [u8; 4],
    pub color_space: [u8; 4],
    pub desc: String,
    /// Native xy: colorants un-adapted via chad⁻¹ if chad present; white = xy of (R+G+B) native sum.
    pub primaries: Option<Primaries>,
    pub is_matrix_trc: bool,
    pub lumi: Option<f64>,
    pub mhc2: Option<Mhc2>,
}

/// Handles v2 'desc' and v4 'mluc'; must never panic on garbage.
pub fn parse(_data: &[u8]) -> Result<IccInfo, &'static str> {
    todo!("icc_read::parse")
}
