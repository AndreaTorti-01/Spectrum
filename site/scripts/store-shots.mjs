// Renders the Microsoft Store screenshots: docs/store/store-<n>-<name>-<theme>.png (1920x1080), light and dark.
// Each one is real app UI from `Spectrum.exe --render` (showcase data, off-screen, changes nothing), drawn 1:1
// at a Windows display scale and composed on a faint gradient with Windows 11 style shadows, captured with
// headless Edge. Captions match the "Screenshots" section of packaging/store-listing.md.
// Run: npm run store-shots [-- --exe <path to Spectrum.exe>]
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import sharp from "sharp";
import { prismSvg, SMALL_VIEWBOX } from "../src/data/prism.ts";
import { FONT_CSS, repoRoot, screenshot } from "./lib.mjs";

const W = 1920;
const H = 1080;

const arg = (name) => {
  const i = process.argv.indexOf(name);
  return i > 0 ? process.argv[i + 1] : undefined;
};
const srcExe = arg("--exe") ?? join(repoRoot, "target", "release", "Spectrum.exe");
if (!existsSync(srcExe)) throw new Error(`missing ${srcExe}: run cargo build --release first`);

// A private copy, so a rebuild of target\ during the run can't lock or swap the exe.
const work = mkdtempSync(join(tmpdir(), "spectrum-store-"));
const exe = join(work, "Spectrum.exe");
copyFileSync(srcExe, exe);

const outDir = join(repoRoot, "docs", "store");
mkdirSync(outDir, { recursive: true });

/**
 * The surface at `scale` (1 = 100%), as a file URL and its size in pixels, shown 1:1 in the page.
 * Below 200% it's rendered at 200% and downscaled with Lanczos, so text keeps the 200% shapes.
 */
async function render(surface, theme, scale, framed, padBottom = 0) {
  const at = Math.max(scale, 2);
  const raw = join(work, `${surface}-${theme}-${at}${framed ? "-f" : ""}.png`);
  const args = ["--render", surface, "--theme", theme, "--scale", String(at), ...(framed ? ["--frame"] : []), "--out", raw];
  const r = spawnSync(exe, args, { stdio: "ignore", timeout: 60000 });
  if (r.error) throw r.error;
  if (r.status !== 0 || !existsSync(raw)) throw new Error(`render ${surface} ${theme} failed (${r.status})`);
  let out = raw;
  if (at !== scale) {
    const m = await sharp(raw).metadata();
    out = raw.replace(/\.png$/, `-${scale}.png`);
    await sharp(raw).resize(Math.round((m.width * scale) / at), Math.round((m.height * scale) / at), { kernel: "lanczos3" }).png().toFile(out);
  }
  if (padBottom) {
    const { data, info } = await sharp(out).raw().toBuffer({ resolveWithObject: true });
    const i = ((info.height - 1) * info.width + 2) * info.channels;
    const padded = out.replace(/\.png$/, "-pad.png");
    await sharp(out).extend({ bottom: padBottom, background: { r: data[i], g: data[i + 1], b: data[i + 2], alpha: 1 } }).png().toFile(padded);
    out = padded;
  }
  const { width, height } = await sharp(out).metadata();
  return { src: pathToFileURL(out).href, w: width, h: height };
}

const themes = {
  light: {
    bg: "radial-gradient(1400px 900px at 50% 38%, #FBFAF7 0%, #F4F1EB 55%, #ECE7DE 100%)",
    ink: "#161412",
    // Windows 11 DWM-like: a tight contact shadow and a wide soft one.
    win: "0 0 0 1px rgba(0,0,0,.02), 0 2px 6px rgba(40,30,20,.10), 0 18px 48px rgba(40,30,20,.16), 0 40px 90px rgba(40,30,20,.10)",
    menu: "0 2px 4px rgba(40,30,20,.08), 0 10px 28px rgba(40,30,20,.16)",
    bar: "rgba(238,236,232,.96)",
    barLine: "#DCD7CE",
    barDim: "#5E5850",
    hover: "rgba(0,0,0,.06)",
  },
  dark: {
    bg: "radial-gradient(1400px 900px at 50% 38%, #22201D 0%, #191816 55%, #121110 100%)",
    ink: "#ECE7DF",
    win: "0 0 0 1px rgba(0,0,0,.35), 0 2px 8px rgba(0,0,0,.40), 0 18px 48px rgba(0,0,0,.50), 0 40px 90px rgba(0,0,0,.35)",
    menu: "0 2px 4px rgba(0,0,0,.30), 0 10px 28px rgba(0,0,0,.50)",
    bar: "rgba(28,27,25,.97)",
    barLine: "#2E2B27",
    barDim: "#A39D94",
    hover: "rgba(255,255,255,.08)",
  },
};

// The captions, identical to packaging/store-listing.md.
const shots = [
  { n: 1, name: "main", caption: "Color profile and HDR for each monitor" },
  { n: 2, name: "tray", caption: "Switch color profiles from the tray menu" },
  { n: 3, name: "gamut", caption: "Monitor gamut on the CIE 1931 diagram" },
  { n: 4, name: "profiles", caption: "Profiles and the monitors that use them" },
  { n: 5, name: "hdr", caption: "Tune the HDR profile Windows applies" },
];

const CAP_Y = 50; // caption top
const STAGE_T = 124; // top of the area under the caption

/** An image at its pixel size, clipped to `radius` (the drawn corners hold the render's stage color). */
const pane = (img, x, y, radius, shadow) =>
  `<div class="pane" style="left:${x}px;top:${y}px;width:${img.w}px;height:${img.h}px;border-radius:${radius}px;box-shadow:${shadow}"><img src="${img.src}" width="${img.w}" height="${img.h}"></div>`;

/** Centered in the area below the caption, nudged up a little so the bottom margin is the larger one. */
const centered = (img, bottom = H) => ({ x: Math.round((W - img.w) / 2), y: Math.round(STAGE_T + (bottom - STAGE_T - img.h) / 2 - 8) });

async function compose(shot, theme) {
  const t = themes[theme];
  let body = "";
  if (shot.name === "main" || shot.name === "gamut" || shot.name === "profiles") {
    const scale = { main: 1.25, gamut: 1.25, profiles: 1.5 }[shot.name];
    const img = await render(shot.name, theme, scale, true);
    const p = centered(img);
    body = pane(img, p.x, p.y, 8 * scale, t.win);
  } else if (shot.name === "hdr") {
    // The HDR section of the main window at 200%, as a card of the window surface.
    // The crop starts on the divider above the section; the top 4 px (the 2 px line and its gap) are left out.
    // The bottom gets 24 px more of the window surface, so the card has even padding.
    const img = await render("hdr", theme, 2, false, 24);
    const cut = 4;
    const card = { ...img, h: img.h - cut };
    const p = centered(card);
    body = pane(card, p.x, p.y, 16, t.win).replace("<img ", `<img style="margin-top:-${cut}px" `);
  } else if (shot.name === "tray") {
    // Close-up of the taskbar corner at 250%: the tray menu open above the Spectrum icon. The taskbar is a
    // mock (dim placeholders, no real icons or clock), like src/ui/mockbar.rs in the README demo.
    const s = 2.5;
    const menu = await render("traymenu", theme, s, false);
    const barH = Math.round(48 * s);
    const barT = H - barH;
    const iconCx = W / 2;
    const menuX = iconCx - Math.round(menu.w / 2);
    const menuY = barT - Math.round(12 * s) - menu.h;
    const sq = Math.round(26 * s);
    const gap = Math.round(14 * s);
    const apps = [0.18, 0.18, 0.18, 0.34]
      .map((a, i) => `<i style="left:${iconCx - Math.round(150 * s) - (4 - i) * (sq + gap) + gap}px;top:${(barH - sq) / 2}px;width:${sq}px;height:${sq}px;opacity:${a}"></i>`)
      .join("");
    const hov = Math.round(36 * s);
    const isz = Math.round(20 * s);
    const right = iconCx + Math.round(120 * s);
    const bh = Math.round(6 * s);
    body =
      pane(menu, menuX, menuY, 4 * s, t.menu) +
      `<div class="bar" style="top:${barT}px;height:${barH}px">${apps}` +
      `<b style="left:${iconCx - hov / 2}px;top:${(barH - hov) / 2}px;width:${hov}px;height:${hov}px"></b>` +
      `<span style="left:${iconCx - isz / 2}px;top:${(barH - isz) / 2}px">${prismSvg(isz, { viewBox: SMALL_VIEWBOX, id: "t" })}</span>` +
      `<u style="left:${right - Math.round(44 * s)}px;top:${barH / 2 - Math.round(9 * s)}px;width:${Math.round(44 * s)}px;height:${bh}px;opacity:.55"></u>` +
      `<u style="left:${right - Math.round(58 * s)}px;top:${barH / 2 + Math.round(3 * s)}px;width:${Math.round(58 * s)}px;height:${bh}px;opacity:.36"></u>` +
      `</div>`;
  }
  const html = `<!doctype html><meta charset="utf-8"><style>${FONT_CSS}
*{box-sizing:border-box;margin:0}
html,body{width:${W}px;height:${H}px;overflow:hidden}
body{position:relative;background:${t.bg};font-family:"Archivo Variable",sans-serif;color:${t.ink}}
h1{position:absolute;left:0;right:0;top:${CAP_Y}px;text-align:center;font-weight:650;font-variation-settings:"wdth" 112;font-size:34px;line-height:1.1;letter-spacing:-.01em}
.pane{position:absolute;overflow:hidden;isolation:isolate}
.pane img{display:block;image-rendering:pixelated}
.bar{position:absolute;left:0;right:0;background:${t.bar};border-top:1px solid ${t.barLine}}
.bar>*{position:absolute;display:block}
.bar i{border-radius:18px;background:${t.barDim}}
.bar b{border-radius:15px;background:${t.hover}}
.bar u{border-radius:8px;background:${t.barDim}}
.bar span svg{display:block}
</style>
<h1>${shot.caption}</h1>
${body}`;
  const out = join(outDir, `store-${shot.n}-${shot.name}-${theme}.png`);
  screenshot(html, out, W, H);
  const m = await sharp(out).metadata();
  if (m.width !== W || m.height !== H) throw new Error(`${out} is ${m.width}x${m.height}`);
  console.log("wrote", out);
}

try {
  for (const shot of shots) for (const theme of Object.keys(themes)) await compose(shot, theme);
} finally {
  rmSync(work, { recursive: true, force: true });
}
