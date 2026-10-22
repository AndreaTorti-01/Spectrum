// Renders docs/banner-light.png and docs/banner-dark.png (1280x400): prism, wordmark and the real monitor
// gamut triangle over the CIE 1931 horseshoe. HTML page captured with headless Edge.
// Run: npm run banner
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { areaPercent, tri } from "../src/data/color.ts";
import { prismSvg } from "../src/data/prism.ts";
import { BT709 } from "../src/data/spaces.ts";
import monitor from "../src/data/monitor.json" with { type: "json" };
import { FONT_CSS, horseshoePng, mapper, repoRoot, screenshot } from "./lib.mjs";

const W = 1280;
const H = 400;
const themes = {
  light: { bg: "#FBFAF7", ink: "#161412", dim: "#6A645B", rule: "#E3DED3", grid: "#E8E3D8", alpha: 0.92, plot: "#FFFFFF" },
  dark: { bg: "#121110", ink: "#ECE7DF", dim: "#A39D94", rule: "#2E2B27", grid: "#262320", alpha: 0.42, plot: "#121110" },
};

// plot box: x 0..0.8, y 0..0.9 at 400 px per unit
const scale = 380;
const ox = 872;
const oy = 372;

const outDir = join(repoRoot, "docs");
mkdirSync(outDir, { recursive: true });

for (const [name, t] of Object.entries(themes)) {
  const m = mapper(ox, oy, scale);
  const horseshoe = await horseshoePng(W, H, ox, oy, scale, t.alpha);
  const grid = [];
  for (let i = 0; i <= 8; i++) grid.push(`<line x1="${m.X(i / 10)}" y1="${m.Y(0)}" x2="${m.X(i / 10)}" y2="${m.Y(0.9)}"/>`);
  for (let i = 0; i <= 9; i++) grid.push(`<line x1="${m.X(0)}" y1="${m.Y(i / 10)}" x2="${m.X(0.8)}" y2="${m.Y(i / 10)}"/>`);
  const ticks = m
    .ticks(5, 9)
    .map((k) => `<line x1="${k.x1}" y1="${k.y1}" x2="${k.x2}" y2="${k.y2}"/><text x="${k.tx}" y="${k.ty + 3}" text-anchor="${k.nx > 0.35 ? "start" : k.nx < -0.35 ? "end" : "middle"}">${k.nm}</text>`)
    .join("");
  const mon = monitor.primaries;
  const dots = tri(mon).map((q) => `<circle cx="${m.X(q.x)}" cy="${m.Y(q.y)}" r="4.5" fill="${t.ink}"/>`).join("");
  const wx = m.X(mon.w.x);
  const wy = m.Y(mon.w.y);
  const html = `<!doctype html><meta charset="utf-8"><style>${FONT_CSS}
*{box-sizing:border-box;margin:0}
html,body{width:${W}px;height:${H}px;background:${t.bg};overflow:hidden}
body{position:relative;font-family:"Archivo Variable",sans-serif;color:${t.ink}}
.frame{position:absolute;inset:14px;border:1px solid ${t.rule}}
.kick{position:absolute;left:64px;top:62px;font:500 13px "IBM Plex Mono";letter-spacing:.1em;text-transform:uppercase;color:${t.dim}}
.brand{position:absolute;left:60px;top:112px;display:flex;align-items:center;gap:20px}
.word{font-weight:800;font-stretch:125%;font-variation-settings:"wdth" 125;font-size:76px;line-height:.9;letter-spacing:-.02em}
.tag{position:absolute;left:64px;top:262px;font:500 18px/1.4 "IBM Plex Mono";letter-spacing:.01em;color:${t.ink}}
.spec{position:absolute;left:64px;bottom:44px;display:flex;gap:34px;font:500 12px "IBM Plex Mono";letter-spacing:.1em;text-transform:uppercase;color:${t.dim}}
.spec b{display:block;font-size:22px;letter-spacing:0;color:${t.ink};margin-bottom:3px}
body>svg{position:absolute;inset:0}
img{position:absolute;inset:0;width:${W}px;height:${H}px}
text{font:400 10px "IBM Plex Mono";fill:${t.dim}}
.lab{font:500 13px "IBM Plex Mono";fill:${t.ink}}
</style>
<div class="frame"></div>
<div class="kick">Windows 11 · MIT</div>
<div class="brand">${prismSvg(112, { id: "b" })}<div class="word">SPECTRUM</div></div>
<div class="tag">Your monitor already knows its colors.</div>
<div class="spec"><div><b>${areaPercent(tri(mon)).toFixed(1)}%</b>of CIE 1931 xy</div><div><b>${monitor.name.replace("MSI ", "")}</b>EDID primaries</div></div>
<svg width="${W}" height="${H}"><g stroke="${t.grid}" stroke-width="1">${grid.join("")}</g></svg>
<img src="${horseshoe}">
<svg width="${W}" height="${H}" fill="none">
  <path d="${m.locusPath}" stroke="${t.dim}" stroke-width="1"/>
  <g stroke="${t.dim}" stroke-width="1">${ticks}</g>
  <path d="${m.triPath(BT709)}" stroke="${t.dim}" stroke-width="1.5" stroke-dasharray="5 4"/>
  <path d="${m.triPath(mon)}" stroke="${t.ink}" stroke-width="3" stroke-linejoin="round"/>
  ${dots}
  <g stroke="${t.ink}" stroke-width="1.5"><line x1="${wx - 8}" y1="${wy}" x2="${wx + 8}" y2="${wy}"/><line x1="${wx}" y1="${wy - 8}" x2="${wx}" y2="${wy + 8}"/></g>
  <text class="lab" x="${m.X(0.8)}" y="${m.Y(0.085)}" text-anchor="end" style="fill:${t.ink}">${monitor.name}</text>
  <text x="${m.X(0.8)}" y="${m.Y(0.085) + 17}" text-anchor="end">sRGB dashed</text>
</svg>`;
  const out = join(outDir, `banner-${name}.png`);
  screenshot(html, out, W, H);
  console.log("wrote", out);
}
