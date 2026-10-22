// A from-scratch PNG encoder for the 256 px icon image (no crates): RGBA 8-bit, Sub filter, zlib stream
// made of one fixed-Huffman deflate block with a small hash-chain LZ77 matcher. `build.rs` includes this
// file, and `lib.rs` compiles it so the unit tests cover it. Plain `//` comments because `include!` rejects
// `//!`.
//
// Windows Vista and later read PNG-compressed entries in .ico files and in RT_ICON resources.

const WINDOW: usize = 32768;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const MAX_CHAIN: usize = 64;
const HASH_BITS: u32 = 15;

/// Length codes 257..=285: (base length, extra bits).
const LEN_TABLE: [(u16, u8); 29] = [
    (3, 0),
    (4, 0),
    (5, 0),
    (6, 0),
    (7, 0),
    (8, 0),
    (9, 0),
    (10, 0),
    (11, 1),
    (13, 1),
    (15, 1),
    (17, 1),
    (19, 2),
    (23, 2),
    (27, 2),
    (31, 2),
    (35, 3),
    (43, 3),
    (51, 3),
    (59, 3),
    (67, 4),
    (83, 4),
    (99, 4),
    (115, 4),
    (131, 5),
    (163, 5),
    (195, 5),
    (227, 5),
    (258, 0),
];

/// Distance codes 0..=29: (base distance, extra bits).
const DIST_TABLE: [(u16, u8); 30] = [
    (1, 0),
    (2, 0),
    (3, 0),
    (4, 0),
    (5, 1),
    (7, 1),
    (9, 2),
    (13, 2),
    (17, 3),
    (25, 3),
    (33, 4),
    (49, 4),
    (65, 5),
    (97, 5),
    (129, 6),
    (193, 6),
    (257, 7),
    (385, 7),
    (513, 8),
    (769, 8),
    (1025, 9),
    (1537, 9),
    (2049, 10),
    (3073, 10),
    (4097, 11),
    (6145, 11),
    (8193, 12),
    (12289, 12),
    (16385, 13),
    (24577, 13),
];

/// LSB-first bit writer, as deflate wants.
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn new() -> Bits {
        Bits {
            out: Vec::new(),
            acc: 0,
            n: 0,
        }
    }

    /// Writes the low `count` bits of `value`, least significant first (for header fields and extra bits).
    fn put(&mut self, value: u32, count: u32) {
        self.acc |= (value as u64) << self.n;
        self.n += count;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// Writes a Huffman code: codes are packed most significant bit first.
    fn put_code(&mut self, code: u32, len: u32) {
        let mut rev = 0u32;
        for i in 0..len {
            rev |= ((code >> i) & 1) << (len - 1 - i);
        }
        self.put(rev, len);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// The fixed literal/length code of RFC 1951 3.2.6.
fn put_lit_len(b: &mut Bits, sym: u32) {
    match sym {
        0..=143 => b.put_code(0x30 + sym, 8),
        144..=255 => b.put_code(0x190 + (sym - 144), 9),
        256..=279 => b.put_code(sym - 256, 7),
        _ => b.put_code(0xC0 + (sym - 280), 8),
    }
}

fn put_match(b: &mut Bits, len: usize, dist: usize) {
    let li = LEN_TABLE
        .iter()
        .rposition(|&(base, _)| base as usize <= len)
        .unwrap_or(0);
    let (lbase, lextra) = LEN_TABLE[li];
    put_lit_len(b, 257 + li as u32);
    b.put((len - lbase as usize) as u32, lextra as u32);
    let di = DIST_TABLE
        .iter()
        .rposition(|&(base, _)| base as usize <= dist)
        .unwrap_or(0);
    let (dbase, dextra) = DIST_TABLE[di];
    b.put_code(di as u32, 5);
    b.put((dist - dbase as usize) as u32, dextra as u32);
}

fn hash3(d: &[u8], i: usize) -> usize {
    let v = (d[i] as u32) | ((d[i + 1] as u32) << 8) | ((d[i + 2] as u32) << 16);
    (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

/// Raw deflate stream: a single final block with the fixed Huffman codes.
pub fn deflate_fixed(data: &[u8]) -> Vec<u8> {
    let mut b = Bits::new();
    b.put(1, 1); // BFINAL
    b.put(1, 2); // BTYPE = 01, fixed Huffman
    let n = data.len();
    let mut head = vec![usize::MAX; 1 << HASH_BITS];
    let mut prev = vec![usize::MAX; n];
    let mut i = 0usize;
    // Inserts position `p` into the hash chains (needs 3 bytes of lookahead).
    let insert = |head: &mut Vec<usize>, prev: &mut Vec<usize>, p: usize| {
        if p + MIN_MATCH <= n {
            let h = hash3(data, p);
            prev[p] = head[h];
            head[h] = p;
        }
    };
    while i < n {
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        if i + MIN_MATCH <= n {
            let mut cand = head[hash3(data, i)];
            let mut chain = 0;
            let max_len = MAX_MATCH.min(n - i);
            while cand != usize::MAX && i - cand <= WINDOW && chain < MAX_CHAIN {
                let mut l = 0;
                while l < max_len && data[cand + l] == data[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = i - cand;
                    if l == max_len {
                        break;
                    }
                }
                cand = prev[cand];
                chain += 1;
            }
        }
        if best_len >= MIN_MATCH {
            put_match(&mut b, best_len, best_dist);
            for p in i..i + best_len {
                insert(&mut head, &mut prev, p);
            }
            i += best_len;
        } else {
            put_lit_len(&mut b, data[i] as u32);
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    put_lit_len(&mut b, 256); // end of block
    b.finish()
}

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (n, t) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for &x in data {
        c = table[((c ^ x as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// zlib stream: header, fixed-Huffman deflate, Adler-32 (big endian).
pub fn zlib_fixed(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    out.extend_from_slice(&deflate_fixed(data));
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

/// PNG file for a `size` x `size` image of straight-alpha RGBA bytes (top-down).
pub fn encode_rgba(size: u32, rgba: &[u8]) -> Vec<u8> {
    let w = size as usize;
    assert_eq!(rgba.len(), w * w * 4);
    // Sub filter: each byte minus the byte 4 to its left. Long runs of equal pixels become runs of zeros.
    let mut raw = Vec::with_capacity((w * 4 + 1) * w);
    for row in rgba.chunks(w * 4) {
        raw.push(1);
        for (x, &v) in row.iter().enumerate() {
            let left = if x >= 4 { row[x - 4] } else { 0 };
            raw.push(v.wrapping_sub(left));
        }
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 bit, RGBA, deflate, adaptive filtering, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_fixed(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal inflater for fixed-Huffman blocks (the only kind the encoder writes).
    fn inflate_fixed(data: &[u8]) -> Vec<u8> {
        let mut pos = 0usize; // bit position
        let bit = |pos: &mut usize| -> u32 {
            let v = (data[*pos / 8] >> (*pos % 8)) & 1;
            *pos += 1;
            v as u32
        };
        let bits = |pos: &mut usize, n: u32| -> u32 {
            let mut v = 0;
            for i in 0..n {
                v |= bit(pos) << i;
            }
            v
        };
        assert_eq!(bits(&mut pos, 1), 1, "final block");
        assert_eq!(bits(&mut pos, 2), 1, "fixed Huffman");
        let mut out: Vec<u8> = Vec::new();
        loop {
            // Decode a literal/length symbol by growing the code one bit at a time.
            let mut code = 0u32;
            let mut len = 0u32;
            let sym = loop {
                code = (code << 1) | bit(&mut pos);
                len += 1;
                match len {
                    7 if code <= 0x17 => break 256 + code,
                    8 if (0x30..=0xBF).contains(&code) => break code - 0x30,
                    8 if (0xC0..=0xC7).contains(&code) => break 280 + (code - 0xC0),
                    9 if (0x190..=0x1FF).contains(&code) => break 144 + (code - 0x190),
                    _ => {}
                }
                assert!(len < 9, "bad code");
            };
            match sym {
                0..=255 => out.push(sym as u8),
                256 => return out,
                _ => {
                    let (base, extra) = LEN_TABLE[(sym - 257) as usize];
                    let l = base as usize + bits(&mut pos, extra as u32) as usize;
                    let mut dc = 0u32;
                    for _ in 0..5 {
                        dc = (dc << 1) | bit(&mut pos);
                    }
                    let (dbase, dextra) = DIST_TABLE[dc as usize];
                    let d = dbase as usize + bits(&mut pos, dextra as u32) as usize;
                    for _ in 0..l {
                        out.push(out[out.len() - d]);
                    }
                }
            }
        }
    }

    fn sample(n: usize) -> Vec<u8> {
        // Runs, repeats at assorted distances, and pseudo-random bytes.
        let mut v = vec![0u8; 1000];
        let mut s = 12345u32;
        for _ in 0..n {
            s = s.wrapping_mul(1103515245).wrapping_add(12345);
            v.push((s >> 16) as u8);
        }
        let copy = v[500..1500].to_vec();
        v.extend_from_slice(&copy);
        v.extend(std::iter::repeat_n(7u8, 600));
        v
    }

    #[test]
    fn deflate_roundtrips() {
        for data in [
            Vec::new(),
            vec![42],
            vec![1, 2, 3],
            vec![0; 100_000],
            sample(3000),
            sample(70_000),
        ] {
            assert_eq!(inflate_fixed(&deflate_fixed(&data)), data, "len {}", data.len());
        }
    }

    #[test]
    fn checksums_match_known_values() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    #[test]
    fn png_structure() {
        let size = 16u32;
        let rgba: Vec<u8> = (0..size * size * 4).map(|i| (i % 251) as u8).collect();
        let png = encode_rgba(size, &rgba);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), size);
        // Walk the chunks, checking each CRC, and inflate IDAT back to the filtered rows.
        let mut pos = 8;
        let mut idat = Vec::new();
        let mut kinds = Vec::new();
        while pos < png.len() {
            let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
            let body = &png[pos + 4..pos + 8 + len];
            let crc = u32::from_be_bytes(png[pos + 8 + len..pos + 12 + len].try_into().unwrap());
            assert_eq!(crc32(body), crc);
            kinds.push(body[..4].to_vec());
            if &body[..4] == b"IDAT" {
                idat.extend_from_slice(&body[4..]);
            }
            pos += 12 + len;
        }
        assert_eq!(kinds, [b"IHDR".to_vec(), b"IDAT".to_vec(), b"IEND".to_vec()]);
        assert_eq!(&idat[..2], &[0x78, 0x01]);
        let raw = inflate_fixed(&idat[2..idat.len() - 4]);
        assert_eq!(
            u32::from_be_bytes(idat[idat.len() - 4..].try_into().unwrap()),
            adler32(&raw)
        );
        // Undo the Sub filter and compare with the input.
        let mut back = Vec::new();
        for row in raw.chunks(size as usize * 4 + 1) {
            assert_eq!(row[0], 1);
            let mut line: Vec<u8> = Vec::new();
            for (x, &v) in row[1..].iter().enumerate() {
                let left = if x >= 4 { line[x - 4] } else { 0 };
                line.push(v.wrapping_add(left));
            }
            back.extend_from_slice(&line);
        }
        assert_eq!(back, rgba);
    }
}
