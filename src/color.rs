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
        description: "The web and Windows default. If a monitor is 'sRGB mode', this is what it emulates.",
        color: 0x8FA3B8,
    },
    StandardSpace {
        id: "display-p3",
        name: "Display P3",
        prims: Primaries { r: xy(0.680, 0.320), g: xy(0.265, 0.690), b: xy(0.150, 0.060), w: D65 },
        trc: Trc::Srgb,
        description: "Apple's wide gamut: DCI-P3 primaries with a D65 white and the sRGB curve. Most modern wide-gamut and OLED monitors target it.",
        color: 0x2EC4B6,
    },
    StandardSpace {
        id: "adobe-rgb",
        name: "Adobe RGB (1998)",
        prims: Primaries { r: xy(0.64, 0.33), g: xy(0.21, 0.71), b: xy(0.15, 0.06), w: D65 },
        trc: Trc::Gamma(563.0 / 256.0),
        description: "Wider greens and cyans for print and photography workflows. Shares sRGB's red and blue.",
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
        description: "A huge editing space that even includes imaginary colors. For RAW photo editing, not for displays.",
        color: 0xF28AB2,
    },
    StandardSpace {
        id: "rec2020",
        name: "Rec. 2020",
        prims: BT2020,
        trc: Trc::Gamma(2.4),
        description: "The UHD/HDR video container. No consumer display covers it fully yet.",
        color: 0x9BD94A,
    },
    StandardSpace {
        id: "dci-p3",
        name: "DCI-P3",
        prims: Primaries { r: xy(0.680, 0.320), g: xy(0.265, 0.690), b: xy(0.150, 0.060), w: xy(0.314, 0.351) },
        trc: Trc::Gamma(2.6),
        description: "The digital cinema standard: same primaries as Display P3, greenish projector white and gamma 2.6.",
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
    (685, 0.73376, 0.26624),
    (690, 0.73403, 0.26597),
    (695, 0.73418, 0.26582),
    (700, 0.73469, 0.26531),
];

pub fn space(id: &str) -> Option<&'static StandardSpace> {
    STANDARD_SPACES.iter().find(|s| s.id == id)
}

pub fn mat_mul(_a: &Mat3, _b: &Mat3) -> Mat3 {
    todo!("color::mat_mul")
}

pub fn mat_inv(_m: &Mat3) -> Option<Mat3> {
    todo!("color::mat_inv")
}

pub fn mat_vec(_m: &Mat3, _v: Xyz) -> Xyz {
    todo!("color::mat_vec")
}

pub fn xy_to_xyz(_xy: Xy, _y_lum: f64) -> Xyz {
    todo!("color::xy_to_xyz")
}

pub fn xyz_to_xy(_v: Xyz) -> Xy {
    todo!("color::xyz_to_xy")
}

/// Columns = XYZ of R, G, B; white maps to Y = 1.
pub fn rgb_to_xyz_matrix(_p: &Primaries) -> Mat3 {
    todo!("color::rgb_to_xyz_matrix")
}

pub fn bradford(_src_white: Xyz, _dst_white: Xyz) -> Mat3 {
    todo!("color::bradford")
}

pub fn to_s15f16(_v: f64) -> i32 {
    todo!("color::to_s15f16")
}

pub fn from_s15f16(_v: i32) -> f64 {
    todo!("color::from_s15f16")
}

/// Absolute polygon area.
pub fn polygon_area(_p: &[Xy]) -> f64 {
    todo!("color::polygon_area")
}

/// Sutherland-Hodgman; both polygons convex.
pub fn clip_convex(_subject: &[Xy], _clip: &[Xy]) -> Vec<Xy> {
    todo!("color::clip_convex")
}

pub fn triangle(_p: &Primaries) -> [Xy; 3] {
    todo!("color::triangle")
}

/// area(a ∩ reference) / area(reference), 0..1.
pub fn coverage(_a: &Primaries, _reference: &Primaries) -> f64 {
    todo!("color::coverage")
}

pub fn iou(_a: &Primaries, _b: &Primaries) -> f64 {
    todo!("color::iou")
}

/// Area of the spectral locus polygon (for "% of CIE 1931").
pub fn locus_area() -> f64 {
    todo!("color::locus_area")
}

/// Linear sRGB, normalized to the max channel, sRGB-encoded; used to paint the horseshoe.
pub fn xyz_to_srgb8(_v: Xyz) -> [u8; 3] {
    todo!("color::xyz_to_srgb8")
}

/// `(1-s)·I + s·(M2020 · M709⁻¹)`, s in 0..1.
pub fn saturation_matrix(_s: f64) -> Mat3 {
    todo!("color::saturation_matrix")
}
