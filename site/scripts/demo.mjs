// Joins screenshots-raw/demo-{light,dark}/frame-*.png into docs/demo-light.webp and docs/demo-dark.webp
// (animated WebP, 100 ms per frame, loop forever, at most 2 MB each). Run: npm run demo
import sharp from "sharp";
import { mkdirSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { repoRoot } from "./lib.mjs";

const MAX = 2 * 1024 * 1024;
const outDir = join(repoRoot, "docs");
mkdirSync(outDir, { recursive: true });

async function build(theme) {
  const dir = join(repoRoot, "screenshots-raw", `demo-${theme}`);
  const files = readdirSync(dir)
    .filter((f) => /^frame-\d+\.png$/.test(f))
    .sort()
    .map((f) => join(dir, f));
  if (!files.length) throw new Error(`no frames in ${dir}`);
  const out = join(outDir, `demo-${theme}.webp`);

  // The capture region is larger than the app: crop every frame to the union of what changes from the backdrop.
  const meta = await sharp(files[0]).metadata();
  let box = { l: meta.width, t: meta.height, r: 0, b: 0 };
  for (const f of files) {
    const { info } = await sharp(f).trim({ threshold: 12 }).toBuffer({ resolveWithObject: true });
    const l = -(info.trimOffsetLeft ?? 0);
    const t = -(info.trimOffsetTop ?? 0);
    box = { l: Math.min(box.l, l), t: Math.min(box.t, t), r: Math.max(box.r, l + info.width), b: Math.max(box.b, t + info.height) };
  }
  const pad = 24;
  const crop = {
    left: Math.max(0, box.l - pad),
    top: Math.max(0, box.t - pad),
  };
  crop.width = Math.min(meta.width, box.r + pad) - crop.left;
  crop.height = Math.min(meta.height, box.b + pad) - crop.top;
  console.log(theme, "crop", JSON.stringify(crop));

  // Attempts, in order: full rate at 1000 px, then smaller, then every other frame (200 ms per frame keeps the pace).
  const attempts = [
    { width: 1000, step: 1, quality: 80 },
    { width: 900, step: 1, quality: 78 },
    { width: 800, step: 1, quality: 76 },
    { width: 800, step: 2, quality: 78 },
    { width: 700, step: 2, quality: 74 },
  ];
  for (const a of attempts) {
    const picked = files.filter((_, i) => i % a.step === 0);
    const frames = await Promise.all(picked.map((f) => sharp(f).extract(crop).resize({ width: Math.min(a.width, crop.width) }).png().toBuffer()));
    await sharp(frames, { join: { animated: true } })
      .webp({ quality: a.quality, effort: 5, delay: 100 * a.step, loop: 0, smartSubsample: true })
      .toFile(out);
    const size = statSync(out).size;
    console.log(theme, `${picked.length} frames`, `${a.width}px`, `${(size / 1024).toFixed(0)} KB`);
    if (size <= MAX) return;
  }
  throw new Error(`${theme} demo is still over 2 MB`);
}

await build("light");
await build("dark");
