//! EDID parser: base block identity, descriptors, 10-bit chromaticities, plus CTA-861 HDR static metadata.

use crate::color::{Primaries, Trc, Xy};

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

const HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

fn checksum_ok(block: &[u8]) -> bool {
    block.len() == 128 && block.iter().fold(0u8, |a, &b| a.wrapping_add(b)) == 0
}

/// Validates header + base checksum; extensions are optional and skipped when their checksum is bad.
pub fn parse(raw: &[u8]) -> Result<Edid, &'static str> {
    if raw.len() < 128 {
        return Err("EDID shorter than 128 bytes");
    }
    let base = &raw[..128];
    if base[..8] != HEADER {
        return Err("bad EDID header");
    }
    if !checksum_ok(base) {
        return Err("bad EDID base block checksum");
    }

    let m = u16::from_be_bytes([base[8], base[9]]);
    let letter = |shift: u32| {
        let v = (m >> shift) & 0x1F;
        if (1..=26).contains(&v) {
            (b'A' + (v as u8) - 1) as char
        } else {
            '_'
        }
    };
    let manufacturer: String = [letter(10), letter(5), letter(0)].iter().collect();
    let product_code = u16::from_le_bytes([base[10], base[11]]);
    let serial = u32::from_le_bytes([base[12], base[13], base[14], base[15]]);
    let year = 1990 + base[17] as u16;
    let gamma = if base[23] == 0xFF {
        None
    } else {
        Some((base[23] as f64 + 100.0) / 100.0)
    };
    let srgb_default = base[24] & 0x04 != 0;

    // Chromaticity: 10-bit values; low 2 bits of each live in bytes 25 and 26, high 8 bits in 27..=34.
    let low = |byte: u8, shift: u32| (byte >> shift) & 0x03;
    let val = |hi: u8, lo: u8| (((hi as u16) << 2) | lo as u16) as f64 / 1024.0;
    let pt = |hi_x: u8, lo_x: u8, hi_y: u8, lo_y: u8| Xy {
        x: val(hi_x, lo_x),
        y: val(hi_y, lo_y),
    };
    let primaries = Primaries {
        r: pt(base[27], low(base[25], 6), base[28], low(base[25], 4)),
        g: pt(base[29], low(base[25], 2), base[30], low(base[25], 0)),
        b: pt(base[31], low(base[26], 6), base[32], low(base[26], 4)),
        w: pt(base[33], low(base[26], 2), base[34], low(base[26], 0)),
    };

    let mut name = None;
    let mut serial_text = None;
    for off in [54usize, 72, 90, 108] {
        let d = &base[off..off + 18];
        if d[0] != 0 || d[1] != 0 {
            continue; // detailed timing descriptor
        }
        match d[3] {
            0xFC if name.is_none() => name = descriptor_text(&d[5..18]),
            0xFF if serial_text.is_none() => serial_text = descriptor_text(&d[5..18]),
            _ => {}
        }
    }

    let mut hdr = None;
    let ext_count = base[126] as usize;
    for i in 1..=ext_count {
        let start = i * 128;
        let Some(block) = raw.get(start..start + 128) else {
            break;
        };
        if !checksum_ok(block) {
            continue;
        }
        if block[0] == 0x02 && hdr.is_none() {
            hdr = parse_cta_hdr(block);
        }
        // DisplayID (0x70) extensions carry nothing we need.
    }

    Ok(Edid {
        raw: raw.to_vec(),
        manufacturer,
        product_code,
        serial,
        serial_text,
        name,
        year,
        gamma,
        srgb_default,
        primaries,
        hdr,
    })
}

/// 13-byte descriptor text: ends at 0x0A, padded with spaces; non-printable bytes dropped.
fn descriptor_text(bytes: &[u8]) -> Option<String> {
    let end = bytes.iter().position(|&b| b == 0x0A).unwrap_or(bytes.len());
    let s: String = bytes[..end]
        .iter()
        .map(|&b| if (0x20..0x7F).contains(&b) { b as char } else { ' ' })
        .collect();
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// Walks the CTA-861 data block collection looking for the HDR static metadata block
/// (extended tag 0x06).
fn parse_cta_hdr(block: &[u8]) -> Option<HdrStatic> {
    let dtd_offset = block[2] as usize;
    if dtd_offset < 5 {
        return None; // no data block collection
    }
    let end = dtd_offset.min(127);
    let mut i = 4;
    while i < end {
        let header = block[i];
        let tag = header >> 5;
        let len = (header & 0x1F) as usize;
        let payload = block.get(i + 1..i + 1 + len)?;
        if tag == 7 && len >= 3 && payload[0] == 0x06 {
            // payload: [ext tag, EOTF flags, static metadata descriptors, max cv, avg cv, min cv]
            let eotf = payload[1];
            let code = |idx: usize| payload.get(idx).copied();
            let max_nits = code(3)
                .filter(|&c| c != 0)
                .map(|c| 50.0 * (c as f64 / 32.0).exp2());
            let avg_nits = code(4)
                .filter(|&c| c != 0)
                .map(|c| 50.0 * (c as f64 / 32.0).exp2());
            let min_nits = match (code(5), max_nits) {
                (Some(c), Some(max)) => {
                    let f = c as f64 / 255.0;
                    Some(max * f * f / 100.0)
                }
                _ => None,
            };
            return Some(HdrStatic {
                max_nits,
                max_frame_avg_nits: avg_nits,
                min_nits,
                pq: eotf & 0x04 != 0,
            });
        }
        i += 1 + len;
    }
    None
}

impl Edid {
    /// e.g. "MSI-3CB0-0000012A" (+ "-" + sanitized serial_text if present); filename-safe.
    pub fn key(&self) -> String {
        let mut k = format!(
            "{}-{:04X}-{:08X}",
            self.manufacturer, self.product_code, self.serial
        );
        if let Some(t) = &self.serial_text {
            let clean: String = t
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .map(|c| c.to_ascii_uppercase())
                .collect();
            if !clean.is_empty() {
                k.push('-');
                k.push_str(&clean);
            }
        }
        k
    }

    /// Name descriptor or "MFG PROD".
    pub fn display_name(&self) -> String {
        match &self.name {
            Some(n) => n.clone(),
            None => format!("{} {:04X}", self.manufacturer, self.product_code),
        }
    }

    /// `srgb_default` gives Srgb, otherwise Gamma(gamma or 2.2).
    pub fn trc(&self) -> Trc {
        if self.srgb_default {
            Trc::Srgb
        } else {
            Trc::Gamma(self.gamma.unwrap_or(2.2))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(block: &mut [u8]) {
        block[127] = 0;
        let sum = block[..127].iter().fold(0u8, |a, &b| a.wrapping_add(b));
        block[127] = 0u8.wrapping_sub(sum);
    }

    fn synthetic() -> Vec<u8> {
        let mut b = vec![0u8; 256];
        b[..8].copy_from_slice(&HEADER);
        // "DEL": D=4, E=5, L=12
        let m: u16 = (4 << 10) | (5 << 5) | 12;
        b[8..10].copy_from_slice(&m.to_be_bytes());
        b[10..12].copy_from_slice(&0x1234u16.to_le_bytes());
        b[12..16].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        b[17] = 30; // 2020
        b[23] = 120; // gamma 2.2
        b[24] = 0x04; // sRGB default
        b[54 + 3] = 0xFC;
        b[54 + 5..54 + 5 + 7].copy_from_slice(b"Test 1\n");
        b[54 + 12..54 + 18].fill(0x20);
        b[72 + 3] = 0xFF;
        b[72 + 5..72 + 5 + 6].copy_from_slice(b"AB-12\n");
        b[72 + 11..72 + 18].fill(0x20);
        b[126] = 1;
        fix(&mut b[..128]);
        b[128] = 0x02;
        b[129] = 3;
        b[130] = 4 + 7; // data blocks end
                        // HDR static metadata block: tag 7, len 6: ext 6, ET=0x05 (gamma + PQ), SM=1, max, avg, min
        b[132] = (7 << 5) | 6;
        b[133] = 0x06;
        b[134] = 0x05;
        b[135] = 0x01;
        b[136] = 0x66;
        b[137] = 0x4B;
        b[138] = 0x80;
        fix(&mut b[128..]);
        b
    }

    #[test]
    fn synthetic_edid() {
        let b = synthetic();
        let e = parse(&b).unwrap();
        assert_eq!(e.manufacturer, "DEL");
        assert_eq!(e.product_code, 0x1234);
        assert_eq!(e.serial, 0xDEADBEEF);
        assert_eq!(e.year, 2020);
        assert_eq!(e.gamma, Some(2.2));
        assert!(e.srgb_default);
        assert_eq!(e.trc(), Trc::Srgb);
        assert_eq!(e.name.as_deref(), Some("Test 1"));
        assert_eq!(e.serial_text.as_deref(), Some("AB-12"));
        assert_eq!(e.key(), "DEL-1234-DEADBEEF-AB12");
        let h = e.hdr.unwrap();
        assert!(h.pq);
        assert!((h.max_nits.unwrap() - 50.0 * 2f64.powf(102.0 / 32.0)).abs() < 1e-9);
        assert!((h.max_nits.unwrap() - 455.5).abs() < 0.5);
        assert!(h.min_nits.unwrap() > 0.4);
    }

    #[test]
    fn rejects_garbage_and_bad_checksums() {
        assert!(parse(&[]).is_err());
        assert!(parse(&[0u8; 128]).is_err());
        let mut b = synthetic();
        b[20] ^= 1;
        assert!(parse(&b).is_err());
        // A bad extension checksum only drops the extension.
        let mut b = synthetic();
        b[200] ^= 1;
        let e = parse(&b).unwrap();
        assert!(e.hdr.is_none());
        // Truncated extension is tolerated.
        let b = synthetic();
        assert!(parse(&b[..200]).unwrap().hdr.is_none());
    }

    #[test]
    fn gamma_undefined() {
        let mut b = synthetic();
        b[23] = 0xFF;
        b[24] = 0;
        fix(&mut b[..128]);
        let e = parse(&b).unwrap();
        assert_eq!(e.gamma, None);
        assert_eq!(e.trc(), Trc::Gamma(2.2));
    }
}
