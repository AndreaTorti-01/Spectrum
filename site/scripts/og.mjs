// Renders public/og.png (1200x630) from the hero: headline and the monitor's gamut over the horseshoe.
// Run: npm run og
import { join } from "node:path";
import { areaPercent, tri } from "../src/data/color.ts";
import { prismSvg } from "../src/data/prism.ts";
import { BT709 } from "../src/data/spaces.ts";
import monitor from "../src/data/monitor.json" with { type: "json" };
import { FONT_CSS, horseshoePng, mapper, screenshot, siteRoot } from "./lib.mjs";

const W = 1200;
const H = 630;
const t = { bg: "#121110", ink: "#ECE7DF", dim: "#A39D94", rule: "#2E2B27" };
const scale = 600;
const ox = 640;
const oy = 590;
const m = mapper(ox, oy, scale);
const mon = monitor.primaries;
const horseshoe = await horseshoePng(W, H, ox, oy, scale, 0.4);
const grid = [];
for (let i = 0; i <= 8; i++) grid.push(`<line x1="${m.X(i / 10)}" y1="${m.Y(0)}" x2="${m.X(i / 10)}" y2="${m.Y(0.9)}"/>`);
for (let i = 0; i <= 9; i++) grid.push(`<line x1="${m.X(0)}" y1="${m.Y(i / 10)}" x2="${m.X(0.8)}" y2="${m.Y(i / 10)}"/>`);
const ticks = m
  .ticks(7, 11)
  .map((k) => `<line x1="${k.x1}" y1="${k.y1}" x2="${k.x2}" y2="${k.y2}"/><text x="${k.tx}" y="${k.ty + 4}" text-anchor="${k.nx > 0.35 ? "start" : k.nx < -0.35 ? "end" : "middle"}">${k.nm}</text>`)
  .join("");
const dots = tri(mon).map((q) => `<circle cx="${m.X(q.x)}" cy="${m.Y(q.y)}" r="5" fill="${t.ink}"/>`).join("");
const html = `<!doctype html><meta charset="utf-8"><style>${FONT_CSS}
*{box-sizing:border-box;margin:0}
html,body{width:${W}px;height:${H}px;background:${t.bg};overflow:hidden}
body{position:relative;font-family:"Archivo Variable",sans-serif;color:${t.ink}}
.brand{position:absolute;left:64px;top:56px;display:flex;align-items:center;gap:14px;font-weight:800;font-variation-settings:"wdth" 125;font-size:26px;letter-spacing:.02em}
.kick{position:absolute;left:64px;top:176px;font:500 15px "IBM Plex Mono";letter-spacing:.1em;text-transform:uppercase;color:${t.dim}}
h1{position:absolute;left:60px;top:210px;width:590px;font-weight:800;font-variation-settings:"wdth" 125;font-size:64px;line-height:.92;letter-spacing:-.025em}
.facts{position:absolute;left:64px;bottom:56px;display:flex;gap:44px;font:500 28px "IBM Plex Mono"}
.facts small{display:block;margin-top:6px;font-size:13px;letter-spacing:.1em;text-transform:uppercase;color:${t.dim}}
body>svg{position:absolute;inset:0}
img{position:absolute;inset:0;width:${W}px;height:${H}px}
text{font:400 12px "IBM Plex Mono";fill:${t.dim}}
</style>
<div class="brand">${prismSvg(40, { id: "o" })}<span>SPECTRUM</span></div>
<div class="kick">Windows 11 · MIT</div>
<h1>Your monitor already knows its colors.</h1>
<div class="facts"><div>${areaPercent(tri(mon)).toFixed(1)}%<small>of CIE 1931 xy</small></div><div>${monitor.name.replace("MSI ", "")}<small>EDID primaries</small></div></div>
<svg width="${W}" height="${H}"><g stroke="${t.rule}" stroke-width="1">${grid.join("")}</g></svg>
<img src="${horseshoe}">
<svg width="${W}" height="${H}" fill="none">
  <path d="${m.locusPath}" stroke="${t.dim}" stroke-width="1"/>
  <g stroke="${t.dim}" stroke-width="1">${ticks}</g>
  <path d="${m.triPath(BT709)}" stroke="${t.dim}" stroke-width="1.5" stroke-dasharray="6 5"/>
  <path d="${m.triPath(mon)}" stroke="${t.ink}" stroke-width="3" stroke-linejoin="round"/>
  ${dots}
</svg>`;
const out = join(siteRoot, "public", "og.png");
screenshot(html, out, W, H);
console.log("wrote", out);
