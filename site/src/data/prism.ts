// The Spectrum prism as SVG, ported from the geometry in src/icon_render.rs (unit square scaled to 100).
// Glass prism, white beam entering from the left, five-band rainbow leaving on the right.

type V = [number, number];

const A: V = [47, 19];
const B: V = [15, 74.4];
const C: V = [79, 74.4];
const CORNER = 5;
const RING = 3.2;
const ENTRY: V = [30.6, 47.4];
const EXIT: V = [64.6, 49.4];
const BEAM_FROM: V = [-5, 59.4];
const FAN_DEG = [6, 13.2, 20.4, 27.6, 34.8, 42];
const BANDS = ["#FF3D38", "#FFB829", "#40CC61", "#24B8F5", "#8061FF"];

function shrunk(extra: number): V[] {
  const g: V = [(A[0] + B[0] + C[0]) / 3, (A[1] + B[1] + C[1]) / 3];
  const side = C[0] - B[0];
  const inr = side / (2 * Math.sqrt(3));
  const k = (inr - CORNER - extra) / inr;
  return [A, B, C].map((p) => [g[0] + (p[0] - g[0]) * k, g[1] + (p[1] - g[1]) * k] as V);
}

const pts = (p: V[]) => p.map((q) => q.map((n) => +n.toFixed(2)).join(",")).join(" ");

function fan(): string[] {
  const far = 150;
  return BANDS.map((_, i) => {
    const a = (FAN_DEG[i]! * Math.PI) / 180;
    const b = (FAN_DEG[i + 1]! * Math.PI) / 180;
    return pts([EXIT, [EXIT[0] + far * Math.cos(a), EXIT[1] + far * Math.sin(a)], [EXIT[0] + far * Math.cos(b), EXIT[1] + far * Math.sin(b)]]);
  });
}

export interface PrismOptions {
  /** viewBox override; default is the whole unit square. */
  viewBox?: string;
  halo?: boolean;
  id?: string;
}

/** Inner markup (no <svg> wrapper) so callers can size it. Gradient id is prefixed with `id`. */
export function prismInner({ halo = true, id = "p" }: PrismOptions = {}): string {
  const outer = pts(shrunk(0));
  const bands = fan();
  const haloShapes = halo
    ? `<g fill="#0a0d17" stroke="#0a0d17" stroke-linejoin="round" stroke-width="5" opacity=".5">` +
      bands.map((b) => `<polygon points="${b}"/>`).join("") +
      `<polygon points="${outer}" stroke-width="${CORNER * 2 + 2.8}"/>` +
      `<line x1="${BEAM_FROM[0]}" y1="${BEAM_FROM[1]}" x2="${ENTRY[0]}" y2="${ENTRY[1]}" stroke-width="8.4" stroke-linecap="round"/></g>`
    : "";
  return (
    `<defs><linearGradient id="${id}g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#F7FAFF"/><stop offset="1" stop-color="#B3C2DE"/></linearGradient></defs>` +
    haloShapes +
    bands.map((b, i) => `<polygon points="${b}" fill="${BANDS[i]}"/>`).join("") +
    `<line x1="${BEAM_FROM[0]}" y1="${BEAM_FROM[1]}" x2="${ENTRY[0]}" y2="${ENTRY[1]}" stroke="#fff" stroke-width="5.6" stroke-linecap="round"/>` +
    `<polygon points="${outer}" fill="#455473" stroke="#455473" stroke-width="${CORNER * 2}" stroke-linejoin="round"/>` +
    `<polygon points="${outer}" fill="url(#${id}g)" stroke="url(#${id}g)" stroke-width="${(CORNER - RING) * 2}" stroke-linejoin="round"/>` +
    `<line x1="${ENTRY[0]}" y1="${ENTRY[1]}" x2="${EXIT[0]}" y2="${EXIT[1]}" stroke="#fff" stroke-width="4.8" stroke-linecap="round"/>`
  );
}

/** Zoomed composition for tray and favicon sizes (see SMALL_ZOOM in icon_render.rs). */
export const SMALL_VIEWBOX = "12 9.7 74.1 74.1";

export function prismSvg(size: number, opts: PrismOptions = {}): string {
  const vb = opts.viewBox ?? "0 0 100 100";
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="${vb}" aria-hidden="true" focusable="false">${prismInner(opts)}</svg>`;
}
