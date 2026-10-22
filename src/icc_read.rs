//! ICC profile reader: header, description, native colorants (chad-unadapted), lumi and the MHC2 tag.
//!
//! Every access is bounds-checked; garbage input yields `Err` or partial info, never a panic.

use crate::color::{
    bradford, from_s15f16, mat_inv, mat_vec, xy_to_xyz, xyz_to_xy, Mat3, Primaries, Xyz, D50, D65, IDENTITY,
};

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
    /// Native xy: colorants un-adapted via chad⁻¹ if chad present; white = xy of the (un-adapted) `wtpt`,
    /// falling back to the xy of R+G+B. Profiles with no `chad` whose colorants sum to D50 (typical ICC v2
    /// display profiles) are assumed to have been adapted from a D65 native white.
    pub primaries: Option<Primaries>,
    pub is_matrix_trc: bool,
    pub lumi: Option<f64>,
    pub mhc2: Option<Mhc2>,
}

fn slice(data: &[u8], off: usize, len: usize) -> Option<&[u8]> {
    data.get(off..off.checked_add(len)?)
}

fn be32(data: &[u8], off: usize) -> Option<u32> {
    let b = slice(data, off, 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn s15(data: &[u8], off: usize) -> Option<f64> {
    be32(data, off).map(|v| from_s15f16(v as i32))
}

/// A tag's bytes within the file.
struct Tags<'a> {
    data: &'a [u8],
    entries: Vec<([u8; 4], usize, usize)>,
}

impl<'a> Tags<'a> {
    fn get(&self, sig: &[u8; 4]) -> Option<&'a [u8]> {
        let &(_, off, size) = self.entries.iter().find(|e| &e.0 == sig)?;
        slice(self.data, off, size)
    }

    fn has(&self, sig: &[u8; 4]) -> bool {
        self.get(sig).is_some()
    }

    fn xyz(&self, sig: &[u8; 4]) -> Option<Xyz> {
        let t = self.get(sig)?;
        if t.get(..4)? != b"XYZ " {
            return None;
        }
        Some(Xyz {
            x: s15(t, 8)?,
            y: s15(t, 12)?,
            z: s15(t, 16)?,
        })
    }

    fn chad(&self) -> Option<Mat3> {
        let t = self.get(b"chad")?;
        if t.get(..4)? != b"sf32" {
            return None;
        }
        let mut m = [[0.0; 3]; 3];
        for (i, cell) in m.iter_mut().flatten().enumerate() {
            *cell = s15(t, 8 + 4 * i)?;
        }
        Some(m)
    }
}

/// Handles v2 'desc' and v4 'mluc'; must never panic on garbage.
pub fn parse(data: &[u8]) -> Result<IccInfo, &'static str> {
    if data.len() < 132 {
        return Err("ICC profile too short");
    }
    if &data[36..40] != b"acsp" {
        return Err("missing ICC 'acsp' signature");
    }
    let version = be32(data, 8).ok_or("truncated header")?;
    let class = [data[12], data[13], data[14], data[15]];
    let color_space = [data[16], data[17], data[18], data[19]];

    let count = be32(data, 128).ok_or("truncated tag table")? as usize;
    let max_count = (data.len() - 132) / 12;
    if count > max_count {
        return Err("tag table exceeds file size");
    }
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let base = 132 + 12 * i;
        let (Some(sig), Some(off), Some(size)) =
            (slice(data, base, 4), be32(data, base + 4), be32(data, base + 8))
        else {
            break;
        };
        entries.push(([sig[0], sig[1], sig[2], sig[3]], off as usize, size as usize));
    }
    let tags = Tags { data, entries };

    let desc = tags.get(b"desc").and_then(parse_text).unwrap_or_default();

    let have_cols = tags.has(b"rXYZ") && tags.has(b"gXYZ") && tags.has(b"bXYZ");
    let have_trc = tags.has(b"rTRC") && tags.has(b"gTRC") && tags.has(b"bTRC");
    let is_matrix_trc = have_cols && have_trc;

    let primaries = native_primaries(&tags);
    let lumi = tags.xyz(b"lumi").map(|v| v.y);
    let mhc2 = tags.get(b"MHC2").and_then(parse_mhc2);

    Ok(IccInfo {
        version,
        class,
        color_space,
        desc,
        primaries,
        is_matrix_trc,
        lumi,
        mhc2,
    })
}

fn native_primaries(tags: &Tags) -> Option<Primaries> {
    let (r, g, b) = (tags.xyz(b"rXYZ")?, tags.xyz(b"gXYZ")?, tags.xyz(b"bXYZ")?);
    let wtpt = tags.xyz(b"wtpt");
    let sum = Xyz {
        x: r.x + g.x + b.x,
        y: r.y + g.y + b.y,
        z: r.z + g.z + b.z,
    };

    // `unadapt` maps profile-connection-space values back to the native illuminant. With a `chad` that is its
    // inverse (and `wtpt` is D50, so it un-adapts too). Without one (ICC v2 style), `wtpt` is the native media
    // white and the colorants were adapted from it to D50.
    let mut wtpt_is_native = false;
    let unadapt: Mat3 = match tags.chad() {
        Some(chad) => mat_inv(&chad)?,
        None => {
            let near = |a: Xyz, b: Xyz| {
                (a.x - b.x).abs() < 0.005 && (a.y - b.y).abs() < 0.005 && (a.z - b.z).abs() < 0.005
            };
            if near(sum, D50) {
                let native_white = match wtpt {
                    Some(w) if w.y > 0.0 && !near(w, D50) => {
                        wtpt_is_native = true;
                        w
                    }
                    _ => xy_to_xyz(D65, 1.0), // assume a D65 native white
                };
                mat_inv(&bradford(native_white, D50)).unwrap_or(IDENTITY)
            } else {
                wtpt_is_native = true;
                IDENTITY
            }
        }
    };
    let native = |v: Xyz| mat_vec(&unadapt, v);
    let (r, g, b) = (native(r), native(g), native(b));
    if [r, g, b].iter().any(|v| v.x + v.y + v.z <= 1e-9) {
        return None;
    }
    let white_xyz = match wtpt {
        Some(w) if w.x + w.y + w.z > 1e-9 => {
            if wtpt_is_native {
                w
            } else {
                native(w)
            }
        }
        _ => Xyz {
            x: r.x + g.x + b.x,
            y: r.y + g.y + b.y,
            z: r.z + g.z + b.z,
        },
    };
    let p = Primaries {
        r: xyz_to_xy(r),
        g: xyz_to_xy(g),
        b: xyz_to_xy(b),
        w: xyz_to_xy(white_xyz),
    };
    let finite = [p.r, p.g, p.b, p.w]
        .iter()
        .all(|v| v.x.is_finite() && v.y.is_finite());
    finite.then_some(p)
}

/// Text from a v2 'desc' / 'text' tag or a v4 'mluc' tag (prefers enUS, else the first record).
fn parse_text(t: &[u8]) -> Option<String> {
    match t.get(..4)? {
        b"desc" => {
            let n = be32(t, 8)? as usize;
            let s = slice(t, 12, n.min(t.len().saturating_sub(12)))?;
            Some(ascii_until_nul(s))
        }
        b"text" => Some(ascii_until_nul(t.get(8..)?)),
        b"mluc" => {
            let count = be32(t, 8)? as usize;
            let rec_size = be32(t, 12)? as usize;
            if rec_size < 12 || count == 0 || count > 256 {
                return None;
            }
            let mut chosen: Option<(usize, usize)> = None;
            for i in 0..count {
                let rec = 16 + i * rec_size;
                let lang = slice(t, rec, 4)?;
                let len = be32(t, rec + 4)? as usize;
                let off = be32(t, rec + 8)? as usize;
                if chosen.is_none() {
                    chosen = Some((off, len));
                }
                if lang == b"enUS" {
                    chosen = Some((off, len));
                    break;
                }
            }
            let (off, len) = chosen?;
            let bytes = slice(t, off, len)?;
            let units: Vec<u16> = bytes
                .windows(2)
                .step_by(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
            Some(String::from_utf16_lossy(&units[..end]))
        }
        _ => None,
    }
}

fn ascii_until_nul(s: &[u8]) -> String {
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    s[..end]
        .iter()
        .map(|&b| if b.is_ascii() { b as char } else { '?' })
        .collect()
}

/// MHC2 layout (offsets relative to the tag start): sig, reserved, LUT entry count, min and peak nits (s15.16),
/// then offsets to the 3x4 matrix and the red, green and blue LUTs.
fn parse_mhc2(t: &[u8]) -> Option<Mhc2> {
    if t.get(..4)? != b"MHC2" {
        return None;
    }
    let lut_count = be32(t, 8)?;
    let min_nits = s15(t, 12)?;
    let peak_nits = s15(t, 16)?;
    let matrix_off = be32(t, 20)? as usize;
    let matrix = if matrix_off == 0 {
        None
    } else {
        let mut m = [[0.0; 4]; 3];
        let mut ok = true;
        for (i, cell) in m.iter_mut().flatten().enumerate() {
            match matrix_off.checked_add(4 * i).and_then(|o| s15(t, o)) {
                Some(v) => *cell = v,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        ok.then_some(m)
    };
    Some(Mhc2 {
        min_nits,
        peak_nits,
        matrix,
        lut_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_and_garbage() {
        assert!(parse(&[]).is_err());
        assert!(parse(&[0u8; 131]).is_err());
        assert!(parse(&[0u8; 400]).is_err());
    }

    #[test]
    fn never_panics_on_mutated_profile() {
        let fixture = include_bytes!("../tests/fixtures/hdr_calibration_app.icc");
        // Truncations at every length.
        for n in 0..fixture.len() {
            let _ = parse(&fixture[..n]);
        }
        // Deterministic pseudo-random corruption (xorshift).
        let mut s = 0x1234_5678_9ABC_DEF0u64;
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for _ in 0..4000 {
            let mut d = fixture.to_vec();
            for _ in 0..(1 + next() % 8) {
                let i = (next() % d.len() as u64) as usize;
                d[i] = next() as u8;
            }
            let _ = parse(&d);
        }
        // Pure noise with a valid signature.
        for _ in 0..500 {
            let len = 132 + (next() % 600) as usize;
            let mut d: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            d[36..40].copy_from_slice(b"acsp");
            let _ = parse(&d);
        }
    }
}
