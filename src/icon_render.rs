// Pure pixel renderer of the app icon (no windows-sys): `build.rs` includes this file to embed the icon
// resources, and `icon.rs` uses it at runtime. Plain `//` comments because `include!` rejects `//!`.
//
// A rounded glass prism, a thin white beam entering from the left and a five-band rainbow fanning out
// to the right, with a faint dark halo so it reads on both light and dark taskbars. Rendered with 4x4
// supersampling and a box downsample, premultiplied BGRA.

type V = (f32, f32);

const SS: u32 = 4;
/// Sizes stored in the .ico file and in the exe's icon resource.
pub const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];
/// Largest size that gets the zoomed-in tray composition, and its zoom factor.
const SMALL_MAX: u32 = 24;
const SMALL_ZOOM: f32 = 1.35;

// ---- geometry in unit coordinates (v grows downward) -----------------------------------------

const PRISM_A: V = (0.47, 0.19);
const PRISM_B: V = (0.15, 0.744);
const PRISM_C: V = (0.79, 0.744);
const CORNER: f32 = 0.05;
const ENTRY: V = (0.306, 0.474);
const EXIT: V = (0.646, 0.494);
const BEAM_FROM: V = (-0.05, 0.594);
const HALO: f32 = 0.028;
const FAN_DEG: [f32; 6] = [6.0, 13.2, 20.4, 27.6, 34.8, 42.0];
const BAND_COLORS: [[f32; 3]; 5] = [
    [1.00, 0.24, 0.22], // red
    [1.00, 0.72, 0.16], // orange / yellow
    [0.25, 0.80, 0.38], // green
    [0.14, 0.72, 0.96], // cyan / blue
    [0.50, 0.38, 1.00], // violet
];

struct Scene {
    prism: [V; 3],
    bands: [[V; 3]; 5],
}

impl Scene {
    fn new() -> Scene {
        let g = (
            (PRISM_A.0 + PRISM_B.0 + PRISM_C.0) / 3.0,
            (PRISM_A.1 + PRISM_B.1 + PRISM_C.1) / 3.0,
        );
        let side = PRISM_C.0 - PRISM_B.0;
        let inr = side / (2.0 * 3f32.sqrt());
        let k = (inr - CORNER) / inr;
        let shrink = |p: V| (g.0 + (p.0 - g.0) * k, g.1 + (p.1 - g.1) * k);
        let far = 1.5;
        let ray = |deg: f32| {
            let a = deg.to_radians();
            (EXIT.0 + far * a.cos(), EXIT.1 + far * a.sin())
        };
        let mut bands = [[(0.0, 0.0); 3]; 5];
        for (i, b) in bands.iter_mut().enumerate() {
            *b = [EXIT, ray(FAN_DEG[i]), ray(FAN_DEG[i + 1])];
        }
        Scene {
            prism: [shrink(PRISM_A), shrink(PRISM_B), shrink(PRISM_C)],
            bands,
        }
    }

    /// Signed distance to the rounded prism (negative inside).
    fn sd_prism(&self, p: V) -> f32 {
        sd_triangle(p, self.prism[0], self.prism[1], self.prism[2]) - CORNER
    }
}

fn sd_segment(p: V, a: V, b: V) -> f32 {
    let (pa, ba) = ((p.0 - a.0, p.1 - a.1), (b.0 - a.0, b.1 - a.1));
    let h = ((pa.0 * ba.0 + pa.1 * ba.1) / (ba.0 * ba.0 + ba.1 * ba.1)).clamp(0.0, 1.0);
    ((pa.0 - ba.0 * h).powi(2) + (pa.1 - ba.1 * h).powi(2)).sqrt()
}

/// Signed distance to a triangle (negative inside).
fn sd_triangle(p: V, a: V, b: V, c: V) -> f32 {
    let e = [
        (b.0 - a.0, b.1 - a.1),
        (c.0 - b.0, c.1 - b.1),
        (a.0 - c.0, a.1 - c.1),
    ];
    let v = [
        (p.0 - a.0, p.1 - a.1),
        (p.0 - b.0, p.1 - b.1),
        (p.0 - c.0, p.1 - c.1),
    ];
    let s = if e[0].0 * e[2].1 - e[0].1 * e[2].0 >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let mut d = (f32::MAX, f32::MAX);
    for i in 0..3 {
        let t = ((v[i].0 * e[i].0 + v[i].1 * e[i].1) / (e[i].0 * e[i].0 + e[i].1 * e[i].1)).clamp(0.0, 1.0);
        let pq = (v[i].0 - e[i].0 * t, v[i].1 - e[i].1 * t);
        let dd = pq.0 * pq.0 + pq.1 * pq.1;
        let cross = s * (v[i].0 * e[i].1 - v[i].1 * e[i].0);
        d = (d.0.min(dd), d.1.min(cross));
    }
    -d.0.sqrt() * d.1.signum()
}

/// Premultiplied RGBA accumulator.
#[derive(Clone, Copy, Default)]
struct Px {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl Px {
    /// `self` over nothing, then `src` (straight color, alpha) painted on top.
    fn over(&mut self, c: [f32; 3], a: f32) {
        if a <= 0.0 {
            return;
        }
        let k = 1.0 - a;
        self.r = c[0] * a + self.r * k;
        self.g = c[1] * a + self.g * k;
        self.b = c[2] * a + self.b * k;
        self.a = a + self.a * k;
    }
}

/// Anti-aliasing helper: 1 inside, 0 outside, with a one-sample soft edge of width `soft`.
fn edge(d: f32, soft: f32) -> f32 {
    (0.5 - d / soft).clamp(0.0, 1.0)
}

fn shade(sc: &Scene, p: V, soft: f32) -> Px {
    let mut px = Px::default();
    let d_prism = sc.sd_prism(p);
    let beam_w = 0.028;
    let d_beam_out = sd_segment(p, BEAM_FROM, ENTRY) - beam_w;
    let in_fan_box = p.0 > EXIT.0 - HALO * 2.0 && p.1 > EXIT.1 - HALO * 2.0;
    let mut d_fan = f32::MAX;
    if in_fan_box {
        for b in &sc.bands {
            d_fan = d_fan.min(sd_triangle(p, b[0], b[1], b[2]));
        }
    }
    // dark halo around everything
    let d_all = d_prism.min(d_fan).min(d_beam_out - HALO * 0.4);
    px.over([0.04, 0.05, 0.09], 0.55 * edge(d_all - HALO, soft));
    // rainbow
    if in_fan_box {
        for (i, b) in sc.bands.iter().enumerate() {
            let d = sd_triangle(p, b[0], b[1], b[2]);
            px.over(BAND_COLORS[i], edge(d, soft));
        }
    }
    // incoming beam
    px.over([1.0, 1.0, 1.0], edge(d_beam_out, soft) * 0.98);
    // glass
    if d_prism < soft {
        let t = ((p.1 - PRISM_A.1) / (PRISM_B.1 - PRISM_A.1)).clamp(0.0, 1.0);
        let top = [0.97, 0.98, 1.00];
        let bot = [0.70, 0.76, 0.87];
        let glass = [
            top[0] + (bot[0] - top[0]) * t,
            top[1] + (bot[1] - top[1]) * t,
            top[2] + (bot[2] - top[2]) * t,
        ];
        let cover = edge(d_prism, soft);
        // pad the glass over what is below with the dark outline ring first
        px.over([0.27, 0.33, 0.45], cover);
        let inner = edge(d_prism + 0.032, soft);
        px.over(glass, inner);
        // soft gloss near the left face
        let gloss = edge((d_prism + 0.075).abs() - 0.018, soft) * if p.0 < 0.52 { 0.45 } else { 0.0 };
        px.over([1.0, 1.0, 1.0], gloss * inner);
        // beam passing through the glass
        let d_in = sd_segment(p, ENTRY, EXIT) - beam_w * 0.85;
        px.over([1.0, 1.0, 1.0], edge(d_in, soft) * inner);
    }
    px
}

/// Premultiplied BGRA, top-down, size x size.
pub fn render_bgra(size: u32) -> Vec<u8> {
    let size = size.max(1);
    let sc = Scene::new();
    let mut out = vec![0u8; (size * size * 4) as usize];
    let inv = 1.0 / (size * SS) as f32;
    // Tray sizes (16 to 24 px) zoom the whole scene about the prism's center so the prism fills about 85 %
    // of the frame width; beam and rainbow scale with it and the rainbow runs off the right edge.
    let (zoom, cx, cy) = if size <= SMALL_MAX {
        (SMALL_ZOOM, 0.49, 0.467)
    } else {
        (1.0, 0.5, 0.5)
    };
    let soft = 1.0 * inv * 1.2 / zoom;
    let n = (SS * SS) as f32;
    for y in 0..size {
        for x in 0..size {
            let mut acc = Px::default();
            for sy in 0..SS {
                for sx in 0..SS {
                    let p = (
                        (((x * SS + sx) as f32 + 0.5) * inv - 0.5) / zoom + cx,
                        (((y * SS + sy) as f32 + 0.5) * inv - 0.5) / zoom + cy,
                    );
                    let s = shade(&sc, p, soft);
                    acc.r += s.r;
                    acc.g += s.g;
                    acc.b += s.b;
                    acc.a += s.a;
                }
            }
            let o = ((y * size + x) * 4) as usize;
            let a = (acc.a / n).clamp(0.0, 1.0);
            let q = |v: f32| ((v / n).clamp(0.0, 1.0).min(a) * 255.0 + 0.5) as u8;
            out[o] = q(acc.b);
            out[o + 1] = q(acc.g);
            out[o + 2] = q(acc.r);
            out[o + 3] = (a * 255.0 + 0.5) as u8;
        }
    }
    out
}

/// All [`SIZES`], each a 32-bit BMP DIB entry (straight alpha, as the ICO format wants).
pub fn ico_bytes() -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = SIZES.iter().map(|&s| (s, dib_entry(s))).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in &images {
        out.push(if *size >= 256 { 0 } else { *size as u8 });
        out.push(if *size >= 256 { 0 } else { *size as u8 });
        out.push(0); // palette colors
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bit count
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in images {
        out.extend_from_slice(&data);
    }
    out
}

/// One ICO image / RT_ICON resource: BITMAPINFOHEADER + bottom-up BGRA pixels + 1-bit AND mask.
pub fn dib_entry(size: u32) -> Vec<u8> {
    let px = render_bgra(size);
    let mask_row = (size.div_ceil(32) * 4) as usize;
    let mask_len = mask_row * size as usize;
    let image_len = (size * size * 4) as usize + mask_len;
    let mut d = Vec::with_capacity(40 + image_len);
    d.extend_from_slice(&40u32.to_le_bytes());
    d.extend_from_slice(&(size as i32).to_le_bytes());
    d.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // XOR + AND heights
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&32u16.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    d.extend_from_slice(&(image_len as u32).to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    for y in (0..size).rev() {
        for x in 0..size {
            let o = ((y * size + x) * 4) as usize;
            let a = px[o + 3] as u32;
            let un = |c: u8| -> u8 { ((c as u32 * 255 + a / 2).checked_div(a).unwrap_or(0)).min(255) as u8 };
            d.extend_from_slice(&[un(px[o]), un(px[o + 1]), un(px[o + 2]), px[o + 3]]);
        }
    }
    d.resize(d.len() + mask_len, 0);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16le(b: &[u8], o: usize) -> u16 {
        u16::from_le_bytes([b[o], b[o + 1]])
    }
    fn u32le(b: &[u8], o: usize) -> u32 {
        u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
    }

    #[test]
    fn ico_container_layout() {
        let b = ico_bytes();
        assert_eq!(u16le(&b, 0), 0);
        assert_eq!(u16le(&b, 2), 1);
        assert_eq!(u16le(&b, 4), SIZES.len() as u16);
        let want = SIZES;
        let mut end = 6 + 16 * SIZES.len();
        for (i, &s) in want.iter().enumerate() {
            let e = 6 + i * 16;
            let (w, h) = (b[e], b[e + 1]);
            if s == 256 {
                assert_eq!((w, h), (0, 0));
            } else {
                assert_eq!((w as u32, h as u32), (s, s));
            }
            assert_eq!(u16le(&b, e + 4), 1);
            assert_eq!(u16le(&b, e + 6), 32);
            let len = u32le(&b, e + 8) as usize;
            let off = u32le(&b, e + 12) as usize;
            assert_eq!(off, end, "entry {i} offset");
            end = off + len;
            // BITMAPINFOHEADER
            assert_eq!(u32le(&b, off), 40);
            assert_eq!(u32le(&b, off + 4), s);
            assert_eq!(u32le(&b, off + 8), s * 2);
            assert_eq!(u16le(&b, off + 14), 32);
            let mask = (s.div_ceil(32) * 4 * s) as usize;
            assert_eq!(len, 40 + (s * s * 4) as usize + mask);
        }
        assert_eq!(end, b.len());
    }

    #[test]
    fn bgra_is_premultiplied_and_shaped() {
        for s in [16u32, 32] {
            let p = render_bgra(s);
            assert_eq!(p.len(), (s * s * 4) as usize);
            for px in p.chunks(4) {
                assert!(px[0] <= px[3] && px[1] <= px[3] && px[2] <= px[3]);
            }
            // corners are transparent, the prism body is opaque
            assert_eq!(p[3], 0);
            let c = ((s / 2 * s + s / 2) * 4 + 3) as usize;
            assert!(p[c] > 200, "center alpha {}", p[c]);
        }
    }

    #[test]
    fn rainbow_has_color_on_the_right() {
        let s = 64u32;
        let p = render_bgra(s);
        // somewhere in the right third there are strongly saturated pixels
        let mut sat = 0;
        for y in 0..s {
            for x in (s * 3 / 4)..s {
                let o = ((y * s + x) * 4) as usize;
                let (b, g, r, a) = (p[o] as i32, p[o + 1] as i32, p[o + 2] as i32, p[o + 3] as i32);
                if a > 200 && (r.max(g).max(b) - r.min(g).min(b)) > 120 {
                    sat += 1;
                }
            }
        }
        assert!(sat > 40, "saturated pixels: {sat}");
    }
}
