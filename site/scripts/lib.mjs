// Shared helpers for the banner and og scripts: horseshoe raster, font URLs, headless Edge capture.
import sharp from "sharp";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { fillHorseshoe, LOCUS_POLY, locusNormal, tri } from "../src/data/color.ts";
import { SPECTRAL_LOCUS } from "../src/data/cie1931.ts";

export const siteRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
export const repoRoot = join(siteRoot, "..");

const font = (p) => pathToFileURL(join(siteRoot, "node_modules", p)).href;
export const FONT_CSS = `
@font-face{font-family:"Archivo Variable";font-weight:100 900;font-stretch:62% 125%;src:url("${font("@fontsource-variable/archivo/files/archivo-latin-wdth-normal.woff2")}") format("woff2-variations")}
@font-face{font-family:"IBM Plex Mono";font-weight:400;src:url("${font("@fontsource/ibm-plex-mono/files/ibm-plex-mono-latin-400-normal.woff2")}") format("woff2")}
@font-face{font-family:"IBM Plex Mono";font-weight:500;src:url("${font("@fontsource/ibm-plex-mono/files/ibm-plex-mono-latin-500-normal.woff2")}") format("woff2")}
`;

/** Horseshoe as a PNG data URL (transparent outside, `alpha` inside) for the box w x h at the given mapping. */
export async function horseshoePng(w, h, ox, oy, scale, alpha) {
  const px = fillHorseshoe(w, h, { x0: -ox / scale, y0: oy / scale, scale });
  for (let i = 3; i < px.length; i += 4) if (px[i]) px[i] = Math.round(255 * alpha);
  const buf = await sharp(Buffer.from(px.buffer), { raw: { width: w, height: h, channels: 4 } }).png().toBuffer();
  return "data:image/png;base64," + buf.toString("base64");
}

/** SVG path data for the locus outline and polygon helpers in pixel space. */
export function mapper(ox, oy, scale) {
  const X = (x) => +(ox + x * scale).toFixed(2);
  const Y = (y) => +(oy - y * scale).toFixed(2);
  return {
    X,
    Y,
    locusPath: LOCUS_POLY.map((q, i) => `${i ? "L" : "M"}${X(q.x)} ${Y(q.y)}`).join("") + "Z",
    triPath: (prims) => tri(prims).map((q, i) => `${i ? "L" : "M"}${X(q.x)} ${Y(q.y)}`).join("") + "Z",
    ticks: (len = 6, gap = 10) =>
      [460, 480, 500, 520, 540, 560, 580, 600, 620].map((nm) => {
        const i = SPECTRAL_LOCUS.findIndex((r) => r[0] === nm);
        const q = LOCUS_POLY[i];
        const n = locusNormal(i);
        return { nm, x1: X(q.x), y1: Y(q.y), x2: X(q.x) + n.x * len, y2: Y(q.y) - n.y * len, tx: X(q.x) + n.x * (len + gap), ty: Y(q.y) - n.y * (len + gap), nx: n.x };
      }),
  };
}

export function screenshot(html, out, width, height) {
  const dir = mkdtempSync(join(tmpdir(), "spectrum-render-"));
  const file = join(dir, "page.html");
  writeFileSync(file, html);
  const edge = "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe";
  const r = spawnSync(
    edge,
    ["--headless=new", "--disable-gpu", "--hide-scrollbars", "--allow-file-access-from-files", "--force-device-scale-factor=1", "--virtual-time-budget=4000", `--window-size=${width},${height}`, `--screenshot=${out}`, pathToFileURL(file).href],
    { stdio: "ignore", timeout: 60000 },
  );
  if (r.error) throw r.error;
}
