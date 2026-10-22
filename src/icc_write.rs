//! ICC profile writers: v4.3 matrix/TRC SDR profiles and Windows-HDR-Calibration-style MHC2 profiles.

use crate::color::{
    bradford, mat_mul, rgb_to_xyz_matrix, saturation_matrix, to_s15f16, xy_to_xyz, Mat3, Primaries, Trc, Xyz,
    D50, IDENTITY,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HdrParams {
    pub min_nits: f64,
    pub peak_nits: f64,
    pub full_frame_nits: f64,
    /// 0..1
    pub saturation: f64,
}

/// `cprt` text of the Windows HDR Calibration app, reproduced verbatim so the profile bytes match its output.
const HDR_CPRT: &str = "Created by Spectrum (MIT license)";
const SDR_CPRT: &str = "Created by Spectrum (MIT license)";
/// The app's private-use `MSCA` text tag, which marks the colorants as raw (not D50-adapted) D65 values.
const HDR_MSCA: &str = "{'Appversion':'1.0.152.0','D65Adapted':True}";

// ---------------------------------------------------------------------------------------------------------
// Binary building blocks
// ---------------------------------------------------------------------------------------------------------

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn put_s15(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&to_s15f16(v).to_be_bytes());
}

/// `mluc` with a single enUS record.
fn mluc(text: &str) -> Vec<u8> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut t = Vec::with_capacity(28 + units.len() * 2);
    t.extend_from_slice(b"mluc");
    put32(&mut t, 0);
    put32(&mut t, 1); // record count
    put32(&mut t, 12); // record size
    t.extend_from_slice(b"enUS");
    put32(&mut t, (units.len() * 2) as u32);
    put32(&mut t, 28); // offset of the string from the tag start
    for u in units {
        t.extend_from_slice(&u.to_be_bytes());
    }
    t
}

/// `XYZ ` tag with one triple.
fn xyz_tag(v: Xyz) -> Vec<u8> {
    let mut t = Vec::with_capacity(20);
    t.extend_from_slice(b"XYZ ");
    put32(&mut t, 0);
    put_s15(&mut t, v.x);
    put_s15(&mut t, v.y);
    put_s15(&mut t, v.z);
    t
}

/// `sf32` tag holding a 3x3 matrix, row-major.
fn sf32_matrix(m: &Mat3) -> Vec<u8> {
    let mut t = Vec::with_capacity(44);
    t.extend_from_slice(b"sf32");
    put32(&mut t, 0);
    for v in m.iter().flatten() {
        put_s15(&mut t, *v);
    }
    t
}

/// `para` parametric curve: sRGB (function type 3) or pure gamma (function type 0).
fn para_tag(trc: Trc) -> Vec<u8> {
    let mut t = Vec::with_capacity(32);
    t.extend_from_slice(b"para");
    put32(&mut t, 0);
    match trc {
        Trc::Srgb => {
            t.extend_from_slice(&3u16.to_be_bytes());
            t.extend_from_slice(&0u16.to_be_bytes());
            for v in [2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045] {
                put_s15(&mut t, v);
            }
        }
        Trc::Gamma(g) => {
            t.extend_from_slice(&0u16.to_be_bytes());
            t.extend_from_slice(&0u16.to_be_bytes());
            put_s15(&mut t, g);
        }
    }
    t
}

struct Header {
    version: u32,
    cmm: [u8; 4],
    attributes: u64,
    intent: u32,
    creator: [u8; 4],
}

/// Lays out header, tag table and tag data. `tags` lists (signature, blob index) in table order; blobs are
/// written in order of first reference, 4-byte aligned, and a blob referenced twice is stored once (its
/// offset is shared). The profile ID is the MD5 of the profile with flags, intent and ID zeroed.
fn assemble(h: &Header, tags: &[([u8; 4], usize)], blobs: &[Vec<u8>]) -> Vec<u8> {
    let table_end = 128 + 4 + 12 * tags.len();
    let mut data: Vec<u8> = Vec::new();
    let mut placed: Vec<Option<usize>> = vec![None; blobs.len()];
    let mut table = Vec::with_capacity(tags.len());
    for &(sig, idx) in tags {
        let off = match placed[idx] {
            Some(o) => o,
            None => {
                let o = table_end + data.len();
                data.extend_from_slice(&blobs[idx]);
                while !data.len().is_multiple_of(4) {
                    data.push(0);
                }
                placed[idx] = Some(o);
                o
            }
        };
        table.push((sig, off as u32, blobs[idx].len() as u32));
    }

    let total = table_end + data.len();
    let mut out = Vec::with_capacity(total);
    put32(&mut out, total as u32);
    out.extend_from_slice(&h.cmm);
    put32(&mut out, h.version);
    out.extend_from_slice(b"mntr");
    out.extend_from_slice(b"RGB ");
    out.extend_from_slice(b"XYZ ");
    for v in now_utc() {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(b"acsp");
    out.extend_from_slice(b"MSFT"); // primary platform
    put32(&mut out, 0); // flags
    put32(&mut out, 0); // device manufacturer
    put32(&mut out, 0); // device model
    out.extend_from_slice(&h.attributes.to_be_bytes());
    put32(&mut out, h.intent);
    put_s15(&mut out, D50.x);
    put_s15(&mut out, D50.y);
    put_s15(&mut out, D50.z);
    out.extend_from_slice(&h.creator);
    out.resize(128, 0); // profile ID and reserved bytes
    put32(&mut out, tags.len() as u32);
    for (sig, off, size) in table {
        out.extend_from_slice(&sig);
        put32(&mut out, off);
        put32(&mut out, size);
    }
    out.extend_from_slice(&data);
    debug_assert_eq!(out.len(), total);

    let id = profile_id(&out);
    out[84..100].copy_from_slice(&id);
    out
}

/// ICC profile ID: MD5 of the whole profile with flags (44..48), intent (64..68) and ID (84..100) zeroed.
fn profile_id(profile: &[u8]) -> [u8; 16] {
    let mut copy = profile.to_vec();
    for range in [44..48, 64..68, 84..100] {
        if let Some(s) = copy.get_mut(range) {
            s.fill(0);
        }
    }
    md5(&copy)
}

/// Current UTC time as the six u16 fields of an ICC dateTimeNumber.
fn now_utc() -> [u16; 6] {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // Howard Hinnant's civil-from-days algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    [
        y as u16,
        m as u16,
        d as u16,
        (rem / 3600) as u16,
        (rem % 3600 / 60) as u16,
        (rem % 60) as u16,
    ]
}

// ---------------------------------------------------------------------------------------------------------
// SDR profile
// ---------------------------------------------------------------------------------------------------------

/// ICC v4.3 mntr RGB/XYZ, D50 PCS, Bradford chad, para TRC, mluc desc/cprt, MD5 profile ID.
pub fn build_sdr_profile(desc: &str, prims: &Primaries, trc: Trc) -> Vec<u8> {
    let native = rgb_to_xyz_matrix(prims);
    let chad = bradford(xy_to_xyz(prims.w, 1.0), D50);
    let adapted = mat_mul(&chad, &native);
    let col = |j: usize| Xyz {
        x: adapted[0][j],
        y: adapted[1][j],
        z: adapted[2][j],
    };

    let blobs = vec![
        mluc(desc),         // 0
        mluc(SDR_CPRT),     // 1
        xyz_tag(D50),       // 2
        sf32_matrix(&chad), // 3
        xyz_tag(col(0)),    // 4
        xyz_tag(col(1)),    // 5
        xyz_tag(col(2)),    // 6
        para_tag(trc),      // 7
    ];
    let tags = [
        (*b"desc", 0),
        (*b"cprt", 1),
        (*b"wtpt", 2),
        (*b"chad", 3),
        (*b"rXYZ", 4),
        (*b"gXYZ", 5),
        (*b"bXYZ", 6),
        (*b"rTRC", 7),
        (*b"gTRC", 7),
        (*b"bTRC", 7),
    ];
    let header = Header {
        version: 0x0430_0000,
        cmm: [0; 4],
        attributes: 0,
        intent: 0,
        creator: *b"SPCT",
    };
    assemble(&header, &tags, &blobs)
}

// ---------------------------------------------------------------------------------------------------------
// HDR (MHC2) profile
// ---------------------------------------------------------------------------------------------------------

/// MHC2 tag: header (36 bytes), a 3x4 matrix with no type header (48 bytes), then three 2-entry `sf32` LUTs.
fn mhc2_tag(p: &HdrParams) -> Vec<u8> {
    let sat = if p.saturation.is_finite() {
        p.saturation.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let m = if sat == 0.0 {
        IDENTITY
    } else {
        saturation_matrix(sat)
    };
    let mut t = Vec::with_capacity(132);
    t.extend_from_slice(b"MHC2");
    put32(&mut t, 0);
    put32(&mut t, 2); // LUT entry count
    put_s15(&mut t, p.min_nits.max(0.0));
    put_s15(&mut t, p.peak_nits.max(0.0));
    put32(&mut t, 0x24); // matrix offset
    put32(&mut t, 0x54); // red LUT offset
    put32(&mut t, 0x64); // green LUT offset
    put32(&mut t, 0x74); // blue LUT offset
    for row in &m {
        for v in row {
            put_s15(&mut t, *v);
        }
        put_s15(&mut t, 0.0); // 4th column
    }
    for _ in 0..3 {
        t.extend_from_slice(b"sf32");
        put32(&mut t, 0);
        put_s15(&mut t, 0.0);
        put_s15(&mut t, 1.0);
    }
    t
}

/// Mirrors the Windows HDR Calibration app byte layout.
///
/// Header, tag order, shared `curv` gamma 2.2, identity `chad`, raw (not D50-adapted) colorants and white,
/// the `MSCA` text tag, `lumi` = full-frame nits, `MHC2` and the trailing `desc` follow the app's own
/// profile (`tests/fixtures/hdr_calibration_app.icc`) tag for tag.
///
/// The one known difference: the app's colorant and white-point XYZ carry luminance scales that follow no
/// rule we could recover (its colorant Y sum is 1.42 and its white Y is 0.569). The chromaticities are
/// exact, and Windows uses only those, so we write the textbook scale: colorants as the columns of the
/// RGB-to-XYZ matrix (white Y = 1) and the white point at Y = 1.
pub fn build_hdr_profile(desc: &str, prims: &Primaries, p: &HdrParams) -> Vec<u8> {
    let m = rgb_to_xyz_matrix(prims);
    let col = |j: usize| Xyz {
        x: m[0][j],
        y: m[1][j],
        z: m[2][j],
    };
    let full = p.full_frame_nits.max(0.0);

    let mut curv = Vec::with_capacity(16);
    curv.extend_from_slice(b"curv");
    put32(&mut curv, 0);
    put32(&mut curv, 1);
    curv.extend_from_slice(&0x0233u16.to_be_bytes()); // u8Fixed8 gamma 2.2 (563/256)
    curv.extend_from_slice(&[0, 0]); // the app counts 2 padding bytes in the tag size

    let mut msca = Vec::with_capacity(8 + HDR_MSCA.len() + 1);
    msca.extend_from_slice(b"text");
    put32(&mut msca, 0);
    msca.extend_from_slice(HDR_MSCA.as_bytes());
    msca.push(0);

    let blobs = vec![
        mluc(HDR_CPRT),                   // 0
        curv,                             // 1
        sf32_matrix(&IDENTITY),           // 2
        xyz_tag(col(0)),                  // 3
        xyz_tag(col(1)),                  // 4
        xyz_tag(col(2)),                  // 5
        xyz_tag(xy_to_xyz(prims.w, 1.0)), // 6
        msca,                             // 7
        xyz_tag(Xyz {
            x: full,
            y: full,
            z: full,
        }), // 8
        mhc2_tag(p),                      // 9
        mluc(desc),                       // 10
    ];
    let tags = [
        (*b"cprt", 0),
        (*b"rTRC", 1),
        (*b"gTRC", 1),
        (*b"bTRC", 1),
        (*b"chad", 2),
        (*b"rXYZ", 3),
        (*b"gXYZ", 4),
        (*b"bXYZ", 5),
        (*b"wtpt", 6),
        (*b"MSCA", 7),
        (*b"lumi", 8),
        (*b"MHC2", 9),
        (*b"desc", 10),
    ];
    let header = Header {
        version: 0x0440_0000,
        cmm: *b"SICC",
        attributes: 1, // transparency
        intent: 1,     // relative colorimetric
        creator: *b"SPCT",
    };
    assemble(&header, &tags, &blobs)
}

// ---------------------------------------------------------------------------------------------------------
// MD5 (RFC 1321), private: only used for the ICC profile ID
// ---------------------------------------------------------------------------------------------------------

fn md5(data: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
        5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15,
        21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64)
        .map(|i| ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32)
        .collect();
    let (mut a0, mut b0, mut c0, mut d0) = (0x6745_2301u32, 0xefcd_ab89u32, 0x98ba_dcfeu32, 0x1032_5476u32);

    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_le_bytes());

    for chunk in msg.chunks(64) {
        let m: Vec<u32> = (0..16)
            .map(|i| u32::from_le_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]))
            .collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f2 = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f2.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, v) in [a0, b0, c0, d0].iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{space, BT709};

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn md5_rfc1321_vectors() {
        assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(&md5(b"a")), "0cc175b9c0f1b6a831c399e269772661");
        assert_eq!(hex(&md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hex(&md5(b"message digest")), "f96b697d7cb7938d525a2f31aaf161d0");
        assert_eq!(
            hex(&md5(b"abcdefghijklmnopqrstuvwxyz")),
            "c3fcd3d76192e4007dfb496cca67e13b"
        );
        assert_eq!(
            hex(&md5(
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
            )),
            "d174ab98d277d9f5a5611c2c9f419d9f"
        );
        assert_eq!(
            hex(&md5(
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            )),
            "57edf4a22be3c955ac49da2e2107b67a"
        );
    }

    #[test]
    fn profile_id_algorithm_matches_windows_app() {
        // The HDR Calibration app's own profile stores exactly this MD5 in its header.
        let f = include_bytes!("../tests/fixtures/hdr_calibration_app.icc");
        assert_eq!(profile_id(f), f[84..100]);
        let f = include_bytes!("../tests/fixtures/hdr_calibration_app_2.icc");
        assert_eq!(profile_id(f), f[84..100]);
    }

    #[test]
    fn generated_profiles_are_well_formed() {
        let hdr = HdrParams {
            min_nits: 0.0,
            peak_nits: 450.0,
            full_frame_nits: 450.0,
            saturation: 0.25,
        };
        let profiles = [
            build_sdr_profile("sRGB test", &BT709, Trc::Srgb),
            build_sdr_profile("Odd length", &space("adobe-rgb").unwrap().prims, Trc::Gamma(2.2)),
            build_hdr_profile("HDR odd", &BT709, &hdr),
        ];
        for p in &profiles {
            assert_eq!(p.len() % 4, 0);
            assert_eq!(u32::from_be_bytes([p[0], p[1], p[2], p[3]]) as usize, p.len());
            assert_eq!(&p[36..40], b"acsp");
            assert_eq!(profile_id(p), p[84..100]);
            assert_ne!(&p[84..100], &[0u8; 16]);
        }
        assert_eq!(&profiles[0][8..12], &[4, 0x30, 0, 0]);
        assert_eq!(&profiles[2][8..12], &[4, 0x40, 0, 0]);
    }
}
