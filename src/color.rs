//! Pure color math: matrices, xy/XYZ, Bradford adaptation, s15Fixed16, polygon clipping, gamut coverage,
//! the CIE 1931 spectral locus and the standard reference color spaces.

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Xy {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Xyz {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub type Mat3 = [[f64; 3]; 3];

pub const IDENTITY: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Primaries {
    pub r: Xy,
    pub g: Xy,
    pub b: Xy,
    pub w: Xy,
}

/// ICC PCS illuminant.
pub const D50: Xyz = Xyz {
    x: 0.9642,
    y: 1.0,
    z: 0.8249,
};
pub const D65: Xy = Xy { x: 0.3127, y: 0.3290 };

const fn xy(x: f64, y: f64) -> Xy {
    Xy { x, y }
}

pub const BT709: Primaries = Primaries {
    r: xy(0.64, 0.33),
    g: xy(0.30, 0.60),
    b: xy(0.15, 0.06),
    w: D65,
};
pub const BT2020: Primaries = Primaries {
    r: xy(0.708, 0.292),
    g: xy(0.170, 0.797),
    b: xy(0.131, 0.046),
    w: D65,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Trc {
    Gamma(f64),
    Srgb,
}

pub struct StandardSpace {
    pub id: &'static str,
    pub name: &'static str,
    pub prims: Primaries,
    pub trc: Trc,
    pub description: &'static str,
    /// 0xRRGGBB plot color
    pub color: u32,
}

/// Order: srgb, display-p3, adobe-rgb, prophoto, rec2020, dci-p3.
pub static STANDARD_SPACES: [StandardSpace; 6] = [
    StandardSpace {
        id: "srgb",
        name: "sRGB",
        prims: BT709,
        trc: Trc::Srgb,
        description: "The default for the web and Windows. A monitor's sRGB mode emulates it.",
        color: 0x8FA3B8,
    },
    StandardSpace {
        id: "display-p3",
        name: "Display P3",
        prims: Primaries {
            r: xy(0.680, 0.320),
            g: xy(0.265, 0.690),
            b: xy(0.150, 0.060),
            w: D65,
        },
        trc: Trc::Srgb,
        description:
            "DCI-P3 primaries with a D65 white and sRGB curve, used by Apple displays and many OLED monitors.",
        color: 0x2EC4B6,
    },
    StandardSpace {
        id: "adobe-rgb",
        name: "Adobe RGB (1998)",
        prims: Primaries {
            r: xy(0.64, 0.33),
            g: xy(0.21, 0.71),
            b: xy(0.15, 0.06),
            w: D65,
        },
        trc: Trc::Gamma(563.0 / 256.0),
        description:
            "Print and photography space with wider greens and cyans than sRGB and the same red and blue.",
        color: 0xFFC93C,
    },
    StandardSpace {
        id: "prophoto",
        name: "ProPhoto RGB",
        prims: Primaries {
            r: xy(0.7347, 0.2653),
            g: xy(0.1596, 0.8404),
            b: xy(0.0366, 0.0001),
            w: xy(0.3457, 0.3585),
        },
        trc: Trc::Gamma(1.8),
        description:
            "Editing space for RAW photos whose green and blue primaries fall outside visible color.",
        color: 0xF28AB2,
    },
    StandardSpace {
        id: "rec2020",
        name: "Rec. 2020",
        prims: BT2020,
        trc: Trc::Gamma(2.4),
        description: "Gamut of UHD and HDR video, with primaries at 630, 532 and 467 nm.",
        color: 0x9BD94A,
    },
    StandardSpace {
        id: "dci-p3",
        name: "DCI-P3",
        prims: Primaries {
            r: xy(0.680, 0.320),
            g: xy(0.265, 0.690),
            b: xy(0.150, 0.060),
            w: xy(0.314, 0.351),
        },
        trc: Trc::Gamma(2.6),
        description:
            "Digital cinema standard with Display P3 primaries, a greenish projector white and gamma 2.6.",
        color: 0xFF5A3C,
    },
];

/// CIE 1931 2 degree spectral locus: (nm, x, y), 380..=700 step 5.
pub static SPECTRAL_LOCUS: &[(u16, f64, f64)] = &[
    (380, 0.17411, 0.00496),
    (385, 0.17401, 0.00492),
    (390, 0.17380, 0.00492),
    (395, 0.17356, 0.00492),
    (400, 0.17334, 0.00480),
    (405, 0.17302, 0.00478),
    (410, 0.17258, 0.00480),
    (415, 0.17209, 0.00483),
    (420, 0.17141, 0.00510),
    (425, 0.17030, 0.00579),
    (430, 0.16888, 0.00690),
    (435, 0.16690, 0.00856),
    (440, 0.16441, 0.01086),
    (445, 0.16110, 0.01379),
    (450, 0.15664, 0.01770),
    (455, 0.15099, 0.02274),
    (460, 0.14396, 0.02970),
    (465, 0.13550, 0.03988),
    (470, 0.12412, 0.05780),
    (475, 0.10959, 0.08684),
    (480, 0.09129, 0.13270),
    (485, 0.06871, 0.20072),
    (490, 0.04539, 0.29498),
    (495, 0.02346, 0.41270),
    (500, 0.00817, 0.53842),
    (505, 0.00386, 0.65482),
    (510, 0.01387, 0.75019),
    (515, 0.03885, 0.81202),
    (520, 0.07430, 0.83380),
    (525, 0.11416, 0.82621),
    (530, 0.15472, 0.80586),
    (535, 0.19288, 0.78163),
    (540, 0.22962, 0.75433),
    (545, 0.26578, 0.72432),
    (550, 0.30160, 0.69231),
    (555, 0.33736, 0.65885),
    (560, 0.37310, 0.62445),
    (565, 0.40874, 0.58961),
    (570, 0.44406, 0.55486),
    (575, 0.47877, 0.52020),
    (580, 0.51249, 0.48659),
    (585, 0.54479, 0.45443),
    (590, 0.57515, 0.42423),
    (595, 0.60293, 0.39650),
    (600, 0.62704, 0.37249),
    (605, 0.64823, 0.35140),
    (610, 0.66575, 0.33401),
    (615, 0.68008, 0.31975),
    (620, 0.69150, 0.30834),
    (625, 0.70061, 0.29925),
    (630, 0.70792, 0.29203),
    (635, 0.71403, 0.28593),
    (640, 0.71903, 0.28093),
    (645, 0.72303, 0.27693),
    (650, 0.72599, 0.27401),
    (655, 0.72827, 0.27173),
    (660, 0.72997, 0.27003),
    (665, 0.73108, 0.26892),
    (670, 0.73199, 0.26801),
    (675, 0.73272, 0.26728),
    (680, 0.73344, 0.26656),
    (685, 0.73401, 0.26599),
    (690, 0.73437, 0.26563),
    (695, 0.73456, 0.26544),
    (700, 0.73469, 0.26531),
];

pub fn space(id: &str) -> Option<&'static StandardSpace> {
    STANDARD_SPACES.iter().find(|s| s.id == id)
}

pub fn mat_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

/// Inverse via the adjugate; `None` when the matrix is (numerically) singular or not finite.
pub fn mat_inv(m: &Mat3) -> Option<Mat3> {
    let c00 = m[1][1] * m[2][2] - m[1][2] * m[2][1];
    let c01 = m[1][2] * m[2][0] - m[1][0] * m[2][2];
    let c02 = m[1][0] * m[2][1] - m[1][1] * m[2][0];
    let det = m[0][0] * c00 + m[0][1] * c01 + m[0][2] * c02;
    if !det.is_finite() || det.abs() < 1e-14 {
        return None;
    }
    let inv_det = 1.0 / det;
    let out = [
        [
            c00 * inv_det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
        ],
        [
            c01 * inv_det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
        ],
        [
            c02 * inv_det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
        ],
    ];
    if out.iter().flatten().all(|v| v.is_finite()) {
        Some(out)
    } else {
        None
    }
}

pub fn mat_vec(m: &Mat3, v: Xyz) -> Xyz {
    Xyz {
        x: m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
        y: m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
        z: m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
    }
}

/// Chromaticity plus luminance to XYZ. A degenerate `y == 0` gives black.
pub fn xy_to_xyz(xy: Xy, y_lum: f64) -> Xyz {
    if xy.y.abs() < 1e-12 {
        return Xyz::default();
    }
    Xyz {
        x: xy.x * y_lum / xy.y,
        y: y_lum,
        z: (1.0 - xy.x - xy.y) * y_lum / xy.y,
    }
}

/// XYZ to chromaticity. A zero sum gives (0, 0).
pub fn xyz_to_xy(v: Xyz) -> Xy {
    let s = v.x + v.y + v.z;
    if s.abs() < 1e-15 {
        return Xy::default();
    }
    Xy {
        x: v.x / s,
        y: v.y / s,
    }
}

/// Columns = XYZ of R, G, B; white maps to Y = 1. Falls back to identity for degenerate primaries.
pub fn rgb_to_xyz_matrix(p: &Primaries) -> Mat3 {
    let r = xy_to_xyz(p.r, 1.0);
    let g = xy_to_xyz(p.g, 1.0);
    let b = xy_to_xyz(p.b, 1.0);
    let prim = [[r.x, g.x, b.x], [r.y, g.y, b.y], [r.z, g.z, b.z]];
    let Some(inv) = mat_inv(&prim) else {
        return IDENTITY;
    };
    let s = mat_vec(&inv, xy_to_xyz(p.w, 1.0));
    [
        [prim[0][0] * s.x, prim[0][1] * s.y, prim[0][2] * s.z],
        [prim[1][0] * s.x, prim[1][1] * s.y, prim[1][2] * s.z],
        [prim[2][0] * s.x, prim[2][1] * s.y, prim[2][2] * s.z],
    ]
}

const BRADFORD: Mat3 = [
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
];

/// Bradford chromatic adaptation matrix taking colors under `src_white` to `dst_white`.
pub fn bradford(src_white: Xyz, dst_white: Xyz) -> Mat3 {
    let Some(inv) = mat_inv(&BRADFORD) else {
        return IDENTITY;
    };
    let s = mat_vec(&BRADFORD, src_white);
    let d = mat_vec(&BRADFORD, dst_white);
    if s.x == 0.0 || s.y == 0.0 || s.z == 0.0 {
        return IDENTITY;
    }
    let scale = [
        [d.x / s.x, 0.0, 0.0],
        [0.0, d.y / s.y, 0.0],
        [0.0, 0.0, d.z / s.z],
    ];
    mat_mul(&inv, &mat_mul(&scale, &BRADFORD))
}

pub fn to_s15f16(v: f64) -> i32 {
    // `as` saturates for out-of-range values and maps NaN to 0.
    (v * 65536.0).round() as i32
}

pub fn from_s15f16(v: i32) -> f64 {
    v as f64 / 65536.0
}

/// Absolute polygon area (shoelace).
pub fn polygon_area(p: &[Xy]) -> f64 {
    signed_area(p).abs()
}

fn signed_area(p: &[Xy]) -> f64 {
    if p.len() < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    for i in 0..p.len() {
        let q = p[(i + 1) % p.len()];
        a += p[i].x * q.y - q.x * p[i].y;
    }
    a / 2.0
}

/// Sutherland-Hodgman; both polygons convex (either winding).
pub fn clip_convex(subject: &[Xy], clip: &[Xy]) -> Vec<Xy> {
    let mut out: Vec<Xy> = subject.to_vec();
    if clip.len() < 3 || out.is_empty() {
        return Vec::new();
    }
    // Normalize the clip polygon to counter-clockwise so "inside" is the left side of each edge.
    let ccw = signed_area(clip) >= 0.0;
    for i in 0..clip.len() {
        let (a, b) = if ccw {
            (clip[i], clip[(i + 1) % clip.len()])
        } else {
            (clip[(i + 1) % clip.len()], clip[i])
        };
        let inside = |p: Xy| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x) >= -1e-15;
        let input = std::mem::take(&mut out);
        if input.is_empty() {
            break;
        }
        let mut prev = input[input.len() - 1];
        for &cur in &input {
            match (inside(cur), inside(prev)) {
                (true, true) => out.push(cur),
                (true, false) => {
                    out.push(intersect(prev, cur, a, b));
                    out.push(cur);
                }
                (false, true) => out.push(intersect(prev, cur, a, b)),
                (false, false) => {}
            }
            prev = cur;
        }
    }
    out
}

/// Intersection of segment p-q with the infinite line a-b.
fn intersect(p: Xy, q: Xy, a: Xy, b: Xy) -> Xy {
    let (dx, dy) = (q.x - p.x, q.y - p.y);
    let (ex, ey) = (b.x - a.x, b.y - a.y);
    let denom = dx * ey - dy * ex;
    if denom.abs() < 1e-18 {
        return p;
    }
    let t = ((a.x - p.x) * ey - (a.y - p.y) * ex) / denom;
    Xy {
        x: p.x + t * dx,
        y: p.y + t * dy,
    }
}

pub fn triangle(p: &Primaries) -> [Xy; 3] {
    [p.r, p.g, p.b]
}

/// area(a ∩ reference) / area(reference), 0..1.
pub fn coverage(a: &Primaries, reference: &Primaries) -> f64 {
    let ref_area = polygon_area(&triangle(reference));
    if ref_area <= 0.0 {
        return 0.0;
    }
    let inter = clip_convex(&triangle(a), &triangle(reference));
    (polygon_area(&inter) / ref_area).clamp(0.0, 1.0)
}

/// Intersection over union of the two gamut triangles.
pub fn iou(a: &Primaries, b: &Primaries) -> f64 {
    let (area_a, area_b) = (polygon_area(&triangle(a)), polygon_area(&triangle(b)));
    let inter = polygon_area(&clip_convex(&triangle(a), &triangle(b)));
    let union = area_a + area_b - inter;
    if union <= 0.0 {
        0.0
    } else {
        (inter / union).clamp(0.0, 1.0)
    }
}

/// Area of the spectral locus polygon, closed by the line of purples (for "% of CIE 1931").
pub fn locus_area() -> f64 {
    let pts: Vec<Xy> = SPECTRAL_LOCUS.iter().map(|&(_, x, y)| Xy { x, y }).collect();
    polygon_area(&pts)
}

/// Linear sRGB, negatives clamped, normalized to the max channel, sRGB-encoded; used to paint the horseshoe.
pub fn xyz_to_srgb8(v: Xyz) -> [u8; 3] {
    const XYZ_TO_LINEAR_SRGB: Mat3 = [
        [3.2404542, -1.5371385, -0.4985314],
        [-0.9692660, 1.8760108, 0.0415560],
        [0.0556434, -0.2040259, 1.0572252],
    ];
    let lin = mat_vec(&XYZ_TO_LINEAR_SRGB, v);
    let c = [lin.x.max(0.0), lin.y.max(0.0), lin.z.max(0.0)];
    let max = c[0].max(c[1]).max(c[2]);
    if !max.is_finite() || max <= 0.0 {
        return [0, 0, 0];
    }
    let enc = |l: f64| {
        let l = (l / max).clamp(0.0, 1.0);
        let e = if l <= 0.0031308 {
            12.92 * l
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        };
        (e * 255.0).round().clamp(0.0, 255.0) as u8
    };
    [enc(c[0]), enc(c[1]), enc(c[2])]
}

/// `(1-s)·I + s·(M2020 · M709⁻¹)`, s in 0..1 (clamped).
pub fn saturation_matrix(s: f64) -> Mat3 {
    let s = if s.is_finite() { s.clamp(0.0, 1.0) } else { 0.0 };
    let m709 = rgb_to_xyz_matrix(&BT709);
    let m2020 = rgb_to_xyz_matrix(&BT2020);
    let wide = match mat_inv(&m709) {
        Some(inv) => mat_mul(&m2020, &inv),
        None => IDENTITY,
    };
    let mut out = IDENTITY;
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (1.0 - s) * IDENTITY[i][j] + s * wide[i][j];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_roundtrip() {
        let m = rgb_to_xyz_matrix(&BT709);
        let i = mat_mul(&m, &mat_inv(&m).unwrap());
        for (r, row) in i.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                assert!((v - IDENTITY[r][c]).abs() < 1e-12);
            }
        }
        assert!(mat_inv(&[[1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [0.0, 0.0, 1.0]]).is_none());
    }

    #[test]
    fn srgb_matrix_matches_reference() {
        // Standard sRGB D65 RGB->XYZ matrix diagonal.
        let m = rgb_to_xyz_matrix(&BT709);
        assert!((m[0][0] - 0.4124).abs() < 5e-4);
        assert!((m[1][1] - 0.7152).abs() < 5e-4);
        assert!((m[2][2] - 0.9505).abs() < 5e-4);
    }

    #[test]
    fn bradford_maps_white_to_white() {
        let src = xy_to_xyz(D65, 1.0);
        let m = bradford(src, D50);
        let w = mat_vec(&m, src);
        assert!((w.x - D50.x).abs() < 1e-9 && (w.y - D50.y).abs() < 1e-9 && (w.z - D50.z).abs() < 1e-9);
    }

    #[test]
    fn s15f16_roundtrip() {
        assert_eq!(to_s15f16(1.0), 0x10000);
        assert_eq!(to_s15f16(-1.0), -0x10000);
        assert!((from_s15f16(to_s15f16(0.9642)) - 0.9642).abs() < 1e-5);
        assert_eq!(to_s15f16(f64::NAN), 0);
    }

    #[test]
    fn clip_square() {
        let sq = |x0: f64, y0: f64| {
            [
                Xy { x: x0, y: y0 },
                Xy { x: x0 + 1.0, y: y0 },
                Xy {
                    x: x0 + 1.0,
                    y: y0 + 1.0,
                },
                Xy { x: x0, y: y0 + 1.0 },
            ]
        };
        let out = clip_convex(&sq(0.0, 0.0), &sq(0.5, 0.5));
        assert!((polygon_area(&out) - 0.25).abs() < 1e-12);
        // A clockwise clip polygon works too.
        let mut cw = sq(0.5, 0.5);
        cw.reverse();
        assert!((polygon_area(&clip_convex(&sq(0.0, 0.0), &cw)) - 0.25).abs() < 1e-12);
        assert!(clip_convex(&sq(0.0, 0.0), &sq(5.0, 5.0)).is_empty());
    }

    #[test]
    fn saturation_matrix_properties() {
        assert_eq!(saturation_matrix(0.0), IDENTITY);
        // M2020 · M709⁻¹ maps D65 white to D65 white, so any blend does too.
        let w = xy_to_xyz(D65, 1.0);
        let out = mat_vec(&saturation_matrix(0.7), w);
        assert!((out.x - w.x).abs() < 1e-9 && (out.y - w.y).abs() < 1e-9 && (out.z - w.z).abs() < 1e-9);
    }

    #[test]
    fn horseshoe_colors() {
        assert_eq!(xyz_to_srgb8(Xyz::default()), [0, 0, 0]);
        let white = xyz_to_srgb8(xy_to_xyz(D65, 1.0));
        assert!(white.iter().all(|&c| c >= 253));
    }
}
