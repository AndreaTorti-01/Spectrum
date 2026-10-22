// Turns site/src/shots/*.png into AVIF, WebP and PNG under public/shots/, and writes
// src/generated/shots.json (logical size per shot) for the HTML template. Safe to re-run any time.
//
// Naming: {main,gamut,traymenu,hdr}-{light,dark}.png. If "<name>-1x.png" exists, "<name>.png" is the
// 2x capture (client area only, the site draws the title strip in CSS) and is the ONLY file used on the
// site (the 1x file keeps the OS title bar, so it must not be mixed in). If it does not exist, or the
// main file is not clearly larger than the 1x file (not re-captured yet), "<name>.png" is used as a 1x image.
// traymenu is always 1x: Windows draws that menu and --debug-scale does not change it, so the site shows it at its
// natural 220 px width and never above it (--menu-w in sections.css).
import sharp from "sharp";
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { prismSvg, SMALL_VIEWBOX } from "../src/data/prism.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = join(root, "src", "shots");
const out = join(root, "public", "shots");
const gen = join(root, "src", "generated");

const NAMES = ["main", "gamut", "traymenu", "hdr"];
const THEMES = ["light", "dark"];

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
mkdirSync(gen, { recursive: true });

async function emit(file, base) {
  const img = sharp(file);
  await img.clone().avif({ quality: 62, effort: 7, chromaSubsampling: "4:4:4" }).toFile(base + ".avif");
  await img.clone().webp({ quality: 88, effort: 6, smartSubsample: true }).toFile(base + ".webp");
  await img.clone().png({ compressionLevel: 9, palette: false }).toFile(base + ".png");
}

const manifest = {};
for (const n of NAMES) {
  for (const t of THEMES) {
    const key = `${n}-${t}`;
    const main = join(src, `${key}.png`);
    const one = join(src, `${key}-1x.png`);
    if (!existsSync(main)) {
      console.warn(`missing ${key}.png`);
      continue;
    }
    const m = await sharp(main).metadata();
    let scale = 1;
    if (existsSync(one)) {
      const m1 = await sharp(one).metadata();
      if (m.width >= m1.width * 1.5) scale = 2;
    }
    await emit(main, join(out, key));
    manifest[key] = { pw: m.width, ph: m.height, scale, w: Math.round(m.width / scale), h: Math.round(m.height / scale) };
    console.log(key, `${m.width}x${m.height}`, scale === 2 ? "2x client area" : n === "traymenu" ? "1x (system-drawn menu)" : "1x (not re-captured yet)");
  }
}
writeFileSync(join(gen, "shots.json"), JSON.stringify(manifest, null, 2) + "\n");

// Favicon: the prism in the zoomed small-size composition.
mkdirSync(join(root, "public"), { recursive: true });
writeFileSync(join(root, "public", "favicon.svg"), prismSvg(64, { viewBox: SMALL_VIEWBOX, id: "f" }).replace(' aria-hidden="true" focusable="false"', "") + "\n");
