//! ICC profile writers: v4.3 matrix/TRC SDR profiles and Windows-HDR-Calibration-style MHC2 profiles.

use crate::color::{Primaries, Trc};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HdrParams {
    pub min_nits: f64,
    pub peak_nits: f64,
    pub full_frame_nits: f64,
    /// 0..1
    pub saturation: f64,
}

/// ICC v4.3 mntr RGB/XYZ, D50 PCS, Bradford chad, para TRC, mluc desc/cprt, MD5 profile ID.
pub fn build_sdr_profile(_desc: &str, _prims: &Primaries, _trc: Trc) -> Vec<u8> {
    todo!("icc_write::build_sdr_profile")
}

/// Mirrors the Windows HDR Calibration app byte layout.
pub fn build_hdr_profile(_desc: &str, _prims: &Primaries, _p: &HdrParams) -> Vec<u8> {
    todo!("icc_write::build_hdr_profile")
}
