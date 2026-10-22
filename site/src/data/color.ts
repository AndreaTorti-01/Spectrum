// Colour math shared by the site modules and the Node scripts (banner, og). No DOM access here.
import { SPECTRAL_LOCUS, CMF } from "./cie1931.ts";
import type { Pt, Prims } from "./spaces.ts";

export type Tri = [Pt, Pt, Pt];

export const tri = (p: Prims): Tri => [p.r, p.g, p.b];

export const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
export const lerpPt = (a: Pt, b: Pt, t: number): Pt => ({ x: lerp(a.x, b.x, t), y: lerp(a.y, b.y, t) });
export const lerpPrims = (a: Prims, b: Prims, t: number): Prims => ({
  r: lerpPt(a.r, b.r, t),
  g: lerpPt(a.g, b.g, t),
  b: lerpPt(a.b, b.b, t),
  w: lerpPt(a.w, b.w, t),
});

/** The spectral locus; the polygon closes along the line of purples. */
export const LOCUS_POLY: Pt[] = SPECTRAL_LOCUS.map(([, x, y]) => ({ x, y }));

function signedArea(poly: readonly Pt[]): number {
  let s = 0;
  for (let i = 0; i < poly.length; i++) {
    const a = poly[i]!;
    const b = poly[(i + 1) % poly.length]!;
    s += a.x * b.y - b.x * a.y;
  }
  return s / 2;
}

export const polyArea = (poly: readonly Pt[]) => Math.abs(signedArea(poly));

export const LOCUS_AREA = polyArea(LOCUS_POLY);

/** Percentage of the CIE 1931 xy horseshoe covered by the triangle. */
export const areaPercent = (t: readonly Pt[]) => (polyArea(t) / LOCUS_AREA) * 100;

/** Sutherland-Hodgman clip of `subject` against the convex polygon `clip` (any winding). */
export function clipPoly(subject: Pt[], clip: Pt[]): Pt[] {
  const orient = signedArea(clip) >= 0 ? 1 : -1;
  let out = subject;
  for (let i = 0; i < clip.length && out.length; i++) {
    const a = clip[i]!;
    const b = clip[(i + 1) % clip.length]!;
    const inside = (q: Pt) => orient * ((b.x - a.x) * (q.y - a.y) - (b.y - a.y) * (q.x - a.x)) >= 0;
    const input = out;
    out = [];
    for (let j = 0; j < input.length; j++) {
      const cur = input[j]!;
      const prev = input[(j + input.length - 1) % input.length]!;
      const ci = inside(cur);
      const pi = inside(prev);
      if (ci !== pi) {
        const d1x = cur.x - prev.x;
        const d1y = cur.y - prev.y;
        const d2x = b.x - a.x;
        const d2y = b.y - a.y;
        const den = d1x * d2y - d1y * d2x;
        const t = ((a.x - prev.x) * d2y - (a.y - prev.y) * d2x) / den;
        out.push({ x: prev.x + d1x * t, y: prev.y + d1y * t });
      }
      if (ci) out.push(cur);
    }
  }
  return out;
}

/** Share of `ref` that lies inside `device`, in percent. */
export function coverage(device: Tri, ref: Tri): number {
  const inter = clipPoly([...ref], [...device]);
  return inter.length < 3 ? 0 : (polyArea(inter) / polyArea(ref)) * 100;
}

// ---- xy / XYZ / sRGB -------------------------------------------------------------------------

const srgbEncode = (c: number) => (c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055);

type Out = number[] | Uint8ClampedArray;

/** XYZ to linear sRGB (D65), clip negatives, normalise to the brightest channel, encode. Writes 0..255. */
export function xyzToSrgb8(X: number, Y: number, Z: number, out: Out = [0, 0, 0], o = 0): Out {
  let r = 3.2404542 * X - 1.5371385 * Y - 0.4985314 * Z;
  let g = -0.969266 * X + 1.8760108 * Y + 0.041556 * Z;
  let b = 0.0556434 * X - 0.2040259 * Y + 1.0572252 * Z;
  r = Math.max(r, 0);
  g = Math.max(g, 0);
  b = Math.max(b, 0);
  const m = Math.max(r, g, b) || 1;
  out[o] = Math.round(255 * srgbEncode(r / m));
  out[o + 1] = Math.round(255 * srgbEncode(g / m));
  out[o + 2] = Math.round(255 * srgbEncode(b / m));
  return out;
}

export function xyToSrgb8(x: number, y: number, out: Out = [0, 0, 0], o = 0): Out {
  y = Math.max(y, 0.004);
  return xyzToSrgb8(x / y, 1, (1 - x - y) / y, out, o);
}

/** Colour of a monochromatic wavelength (380..700 nm), via the CMF table. */
export function wavelengthToSrgb8(nm: number, out: Out = [0, 0, 0], o = 0): Out {
  const f = Math.min(Math.max((nm - 380) / 5, 0), CMF.length - 1.0001);
  const i = Math.floor(f);
  const t = f - i;
  const a = CMF[i]!;
  const b = CMF[i + 1]!;
  return xyzToSrgb8(lerp(a[1], b[1], t), lerp(a[2], b[2], t), lerp(a[3], b[3], t), out, o);
}

export function inv3(m: number[][]): number[][] {
  const [a, b, c] = m[0]! as [number, number, number];
  const [d, e, f] = m[1]! as [number, number, number];
  const [g, h, i] = m[2]! as [number, number, number];
  const A = e * i - f * h;
  const B = -(d * i - f * g);
  const C = d * h - e * g;
  const det = a * A + b * B + c * C;
  return [
    [A / det, -(b * i - c * h) / det, (b * f - c * e) / det],
    [B / det, (a * i - c * g) / det, -(a * f - c * d) / det],
    [C / det, -(a * h - b * g) / det, (a * e - b * d) / det],
  ];
}

/** RGB to XYZ matrix (primaries scaled so that RGB white is the white point with Y = 1). */
export function rgbToXyz(p: Prims): number[][] {
  const xyz = (q: Pt) => [q.x / q.y, 1, (1 - q.x - q.y) / q.y] as const;
  const R = xyz(p.r);
  const G = xyz(p.g);
  const B = xyz(p.b);
  const W = xyz(p.w);
  const m = [
    [R[0], G[0], B[0]],
    [R[1], G[1], B[1]],
    [R[2], G[2], B[2]],
  ];
  const inv = inv3(m);
  const s = [0, 1, 2].map((i) => inv[i]![0]! * W[0] + inv[i]![1]! * W[1] + inv[i]![2]! * W[2]);
  return m.map((row) => row.map((v, j) => v * s[j]!));
}

// ---- horseshoe fill --------------------------------------------------------------------------

export interface View {
  /** Data x at pixel column 0, data y at pixel row 0 (y grows upward), pixels per data unit. */
  x0: number;
  y0: number;
  scale: number;
}

/**
 * RGBA pixels of the horseshoe: every pixel inside the spectral locus (closed by the purple line)
 * gets the xy chromaticity colour normalised to its brightest channel. Outside is transparent.
 */
export function fillHorseshoe(w: number, h: number, v: View): Uint8ClampedArray {
  const out = new Uint8ClampedArray(w * h * 4);
  fillHorseshoeRows(out, w, v, 0, h);
  return out;
}

/** Fills rows [from, to) of a w-wide RGBA buffer, so a caller can split the work into short tasks. */
export function fillHorseshoeRows(out: Uint8ClampedArray, w: number, v: View, from: number, to: number): void {
  const n = LOCUS_POLY.length;
  const xs: number[] = [];
  for (let py = from; py < to; py++) {
    const y = v.y0 - (py + 0.5) / v.scale;
    xs.length = 0;
    for (let i = 0; i < n; i++) {
      const a = LOCUS_POLY[i]!;
      const b = LOCUS_POLY[(i + 1) % n]!;
      if (a.y <= y !== b.y <= y) xs.push(a.x + ((y - a.y) / (b.y - a.y)) * (b.x - a.x));
    }
    xs.sort((p, q) => p - q);
    for (let k = 0; k + 1 < xs.length; k += 2) {
      const pxa = Math.max(0, Math.ceil((xs[k]! - v.x0) * v.scale - 0.5));
      const pxb = Math.min(w - 1, Math.floor((xs[k + 1]! - v.x0) * v.scale - 0.5));
      for (let px = pxa; px <= pxb; px++) {
        const x = v.x0 + (px + 0.5) / v.scale;
        const o = (py * w + px) * 4;
        xyToSrgb8(x, y, out, o);
        out[o + 3] = 255;
      }
    }
  }
}

/** Outward unit normal of the locus at index i (finite difference), for tick labels. */
export function locusNormal(i: number): Pt {
  const a = LOCUS_POLY[Math.max(i - 1, 0)]!;
  const b = LOCUS_POLY[Math.min(i + 1, LOCUS_POLY.length - 1)]!;
  let nx = b.y - a.y;
  let ny = -(b.x - a.x);
  const c = LOCUS_POLY[i]!;
  if (nx * (c.x - 0.33) + ny * (c.y - 0.33) < 0) {
    nx = -nx;
    ny = -ny;
  }
  const l = Math.hypot(nx, ny) || 1;
  return { x: nx / l, y: ny / l };
}

// ---- HDR helpers ------------------------------------------------------------------------------

const M1 = 2610 / 16384;
const M2 = (2523 / 4096) * 128;
const C1 = 3424 / 4096;
const C2 = (2413 / 4096) * 32;
const C3 = (2392 / 4096) * 32;

/** ST 2084 EOTF: signal 0..1 to nits. */
export function pqToNits(e: number): number {
  const p = Math.pow(Math.min(Math.max(e, 0), 1), 1 / M2);
  return 10000 * Math.pow(Math.max(p - C1, 0) / (C2 - C3 * p), 1 / M1);
}

/**
 * Primaries of BT.709 content after the app's saturation matrix M(s) = (1-s) I + s (M2020 M709^-1).
 * M(s) M709 = (1-s) M709 + s M2020, so the result is a blend of the two XYZ columns.
 */
export function saturatedPrims(s: number, a: Prims, b: Prims): Prims {
  const A = rgbToXyz(a);
  const B = rgbToXyz(b);
  const col = (j: number) => [0, 1, 2].map((i) => (1 - s) * A[i]![j]! + s * B[i]![j]!);
  const toXy = (c: number[]): Pt => {
    const sum = c[0]! + c[1]! + c[2]!;
    return { x: c[0]! / sum, y: c[1]! / sum };
  };
  return { r: toXy(col(0)), g: toXy(col(1)), b: toXy(col(2)), w: a.w };
}
