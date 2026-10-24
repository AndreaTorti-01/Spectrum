//! Slider math: value scales, the HDR and SDR content ranges, rounding to the steps Windows applies, and
//! keyboard and mouse positions on the track.

use super::super::draw::{scf, R};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    VK_DOWN, VK_END, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_UP,
};

/// Mapping between a normalized slider position `t` in 0..=1 and a value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scale {
    /// `value = max * t^2` (fine control near zero).
    Quadratic {
        max: f64,
    },
    /// `value = min * (max/min)^t`.
    Log {
        min: f64,
        max: f64,
    },
    Linear {
        min: f64,
        max: f64,
    },
}

impl Scale {
    pub fn to_value(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match *self {
            Scale::Quadratic { max } => max * t * t,
            Scale::Log { min, max } => {
                if max <= min {
                    min
                } else {
                    min * (max / min).powf(t)
                }
            }
            Scale::Linear { min, max } => min + (max - min) * t,
        }
    }

    pub fn to_t(&self, v: f64) -> f64 {
        match *self {
            Scale::Quadratic { max } => (v / max).clamp(0.0, 1.0).sqrt(),
            Scale::Log { min, max } => {
                if max <= min {
                    0.0
                } else {
                    ((v.clamp(min, max) / min).ln() / (max / min).ln()).clamp(0.0, 1.0)
                }
            }
            Scale::Linear { min, max } => {
                if max <= min {
                    0.0
                } else {
                    ((v - min) / (max - min)).clamp(0.0, 1.0)
                }
            }
        }
    }
}

pub const BLACK_MAX_NITS: f64 = 0.5;
pub const PEAK_MIN_NITS: f64 = 100.0;
pub const PEAK_MAX_NITS: f64 = 4000.0;

pub fn black_scale() -> Scale {
    Scale::Quadratic { max: BLACK_MAX_NITS }
}
pub fn peak_scale() -> Scale {
    Scale::Log {
        min: PEAK_MIN_NITS,
        max: PEAK_MAX_NITS,
    }
}
/// Full-frame uses the same fixed 100 to 4000 range as Peak, so moving Peak never rescales it.
pub fn full_scale() -> Scale {
    peak_scale()
}

/// New (peak, full-frame) when Peak moves to `peak`: a lower peak pushes Full-frame down with it.
pub fn move_peak(peak: f64, full: f64) -> (f64, f64) {
    (peak, full.min(peak))
}

/// New full-frame when it is dragged to `full`: it stops at the current peak.
pub fn move_full(full: f64, peak: f64) -> f64 {
    full.min(peak)
}

/// The SDR content value the peak tick sits on: the highest 4-nit step not above the peak. `None` when the
/// peak is outside the slider's range.
pub fn sdr_peak_target(peak: f64) -> Option<f64> {
    use crate::monitors::{SDR_WHITE_MAX_NITS as MAX, SDR_WHITE_MIN_NITS as MIN};
    if !(MIN..MAX).contains(&peak.round()) {
        return None;
    }
    Some(MIN + ((peak.round() - MIN) / 4.0).floor() * 4.0)
}
/// Windows "SDR content brightness": 80 nits up to `max` (the EDID cap, or 480).
pub fn white_scale(max: f64) -> Scale {
    Scale::Linear {
        min: crate::monitors::SDR_WHITE_MIN_NITS,
        max,
    }
}

/// Slider value snapped to the 4-nit grid Windows uses.
pub fn round_sdr_white_nits(v: f64) -> f64 {
    crate::monitors::round_sdr_white_nits(v)
}

pub fn sat_scale() -> Scale {
    Scale::Linear { min: 0.0, max: 100.0 }
}

/// Black level shown/applied in 0.01-nit steps.
pub fn round_black(v: f64) -> f64 {
    ((v * 100.0).round() / 100.0).clamp(0.0, BLACK_MAX_NITS)
}

/// Luminance shown/applied in 5-nit steps (10 above 1000), clamped to `lo..=hi`.
pub fn round_nits(v: f64, lo: f64, hi: f64) -> f64 {
    let step = if v >= 1000.0 { 10.0 } else { 5.0 };
    ((v / step).round() * step).clamp(lo, hi.max(lo))
}

/// New `t` for a slider key press, or None when the key is not a slider key.
pub fn key_to_t(t: f64, vk: u16) -> Option<f64> {
    const STEP: f64 = 0.02;
    const PAGE: f64 = 0.1;
    let n = match vk {
        VK_LEFT | VK_DOWN => t - STEP,
        VK_RIGHT | VK_UP => t + STEP,
        VK_PRIOR => t + PAGE,
        VK_NEXT => t - PAGE,
        VK_HOME => 0.0,
        VK_END => 1.0,
        _ => return None,
    };
    Some(n.clamp(0.0, 1.0))
}

/// Horizontal travel of the thumb center for a slider occupying `r`.
pub fn slider_span(r: R, dpi: u32) -> (f32, f32) {
    let pad = scf(THUMB_R, dpi) + 1.0;
    (r.l as f32 + pad, r.r as f32 - pad)
}

pub fn slider_t_at(r: R, x: i32, dpi: u32) -> f64 {
    let (a, b) = slider_span(r, dpi);
    (((x as f32 - a) / (b - a).max(1.0)) as f64).clamp(0.0, 1.0)
}

/// Radius of the slider thumb at 96 dpi.
pub(super) const THUMB_R: f32 = 9.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn black_is_quadratic() {
        let s = black_scale();
        assert!(close(s.to_value(0.0), 0.0));
        assert!(close(s.to_value(1.0), 0.5));
        assert!(close(s.to_value(0.5), 0.125));
        assert!(close(s.to_t(0.125), 0.5));
    }

    #[test]
    fn peak_is_log() {
        let s = peak_scale();
        assert!(close(s.to_value(0.0), 100.0));
        assert!((s.to_value(1.0) - 4000.0).abs() < 1e-6);
        assert!((s.to_value(0.5) - (100.0f64 * 4000.0).sqrt()).abs() < 1e-6);
        assert!((s.to_t(1000.0) - (10.0f64).ln() / (40.0f64).ln()).abs() < 1e-9);
    }

    #[test]
    fn full_frame_keeps_its_range_and_stops_at_peak() {
        assert_eq!(full_scale(), peak_scale());
        // dragging full-frame past the peak stops at the peak's position
        assert!(close(move_full(800.0, 600.0), 600.0));
        assert!(close(move_full(300.0, 600.0), 300.0));
        assert!(close(
            full_scale().to_t(move_full(800.0, 600.0)),
            peak_scale().to_t(600.0)
        ));
        // lowering the peak below full-frame pushes it down; raising it leaves full-frame alone
        assert_eq!(move_peak(250.0, 400.0), (250.0, 250.0));
        assert_eq!(move_peak(1000.0, 400.0), (1000.0, 400.0));
    }

    #[test]
    fn sdr_peak_tick_sits_on_the_grid() {
        assert_eq!(sdr_peak_target(455.0), Some(452.0));
        assert_eq!(sdr_peak_target(452.0), Some(452.0));
        assert_eq!(sdr_peak_target(1000.0), None);
        assert_eq!(sdr_peak_target(60.0), None);
    }

    #[test]
    fn saturation_is_linear() {
        let s = sat_scale();
        assert!(close(s.to_value(0.25), 25.0));
        assert!(close(s.to_t(75.0), 0.75));
    }

    #[test]
    fn roundtrip_all_scales() {
        let scales = [black_scale(), peak_scale(), full_scale(), sat_scale()];
        for s in scales {
            for i in 0..=20 {
                let t = i as f64 / 20.0;
                assert!((s.to_t(s.to_value(t)) - t).abs() < 1e-9, "{s:?} t={t}");
            }
        }
    }

    #[test]
    fn rounding() {
        assert!(close(round_black(0.123), 0.12));
        assert!(close(round_black(0.9), 0.5));
        assert!(close(round_nits(632.5, 100.0, 4000.0), 635.0));
        assert!(close(round_nits(1234.0, 100.0, 4000.0), 1230.0));
        assert!(close(round_nits(5000.0, 100.0, 4000.0), 4000.0));
        assert!(close(round_nits(3.0, 100.0, 4000.0), 100.0));
    }

    #[test]
    fn keys_move_slider() {
        assert!(close(key_to_t(0.5, VK_RIGHT).unwrap(), 0.52));
        assert!(close(key_to_t(0.5, VK_NEXT).unwrap(), 0.4));
        assert!(close(key_to_t(0.5, VK_HOME).unwrap(), 0.0));
        assert!(close(key_to_t(0.99, VK_PRIOR).unwrap(), 1.0));
        assert!(key_to_t(0.5, 0x41).is_none());
    }

    #[test]
    fn slider_hit_math() {
        let r = R::xywh(100, 0, 200, 24);
        assert!(close(slider_t_at(r, 0, 96), 0.0));
        assert!(close(slider_t_at(r, 1000, 96), 1.0));
        let mid = slider_t_at(r, 200, 96);
        assert!((mid - 0.5).abs() < 0.02);
    }

    #[test]
    fn degenerate_scales_stay_at_the_start() {
        let log = Scale::Log {
            min: 100.0,
            max: 100.0,
        };
        assert_eq!((log.to_value(0.7), log.to_t(500.0)), (100.0, 0.0));
        let lin = Scale::Linear { min: 5.0, max: 5.0 };
        assert_eq!((lin.to_value(0.7), lin.to_t(500.0)), (5.0, 0.0));
        // Positions outside 0..=1 clamp.
        assert!(close(sat_scale().to_value(1.5), 100.0));
        assert!(close(sat_scale().to_t(-10.0), 0.0));
    }

    #[test]
    fn sdr_content_runs_from_80_nits_to_the_cap() {
        let s = white_scale(452.0);
        assert!(close(s.to_value(0.0), 80.0));
        assert!(close(s.to_value(1.0), 452.0));
        assert!(close(round_sdr_white_nits(241.0), 240.0));
        assert_eq!(sdr_peak_target(80.0), Some(80.0));
        assert_eq!(sdr_peak_target(479.4), Some(476.0));
        assert_eq!(sdr_peak_target(480.0), None);
    }

    #[test]
    fn rounding_keeps_the_upper_bound_above_the_lower() {
        assert!(close(round_nits(700.0, 100.0, 50.0), 100.0));
        assert!(close(round_black(-0.2), 0.0));
    }

    #[test]
    fn thumb_travel_is_inset_by_the_thumb() {
        let r = R::xywh(0, 0, 200, 24);
        assert_eq!(slider_span(r, 96), (10.0, 190.0));
        assert_eq!(slider_span(r, 192), (19.0, 181.0));
        assert!(close(slider_t_at(r, 100, 96), 0.5));
    }
}
