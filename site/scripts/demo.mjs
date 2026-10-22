// Joins screenshots-raw/demo-{light,dark}/frame-*.png (recorded at 10 fps by scripts/capture.ps1 -Demo) into
// docs/demo-light.webp and docs/demo-dark.webp: animated, looping forever, 100 ms per frame, with runs of
// identical frames (the holds) merged into one frame with a longer delay. At most 2 MB each. Run: npm run demo
import sharp from "sharp";
import { mkdirSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { repoRoot } from "./lib.mjs";

const MAX = 2 * 1024 * 1024;
const FRAME_MS = 100;
const outDir = join(repoRoot, "docs");
mkdirSync(outDir, { recursive: true });

/** Pixels that differ by more than this (any channel) count as a change between two frames. */
const PIXEL_TOLERANCE = 6;
/** Frames with at most this many changed pixels are the same picture. */
const SAME_PIXELS = 12;

function changed(a, b) {
  let n = 0;
  for (let i = 0; i < a.length; i += 3) {
    if (
      Math.abs(a[i] - b[i]) > PIXEL_TOLERANCE ||
      Math.abs(a[i + 1] - b[i + 1]) > PIXEL_TOLERANCE ||
      Math.abs(a[i + 2] - b[i + 2]) > PIXEL_TOLERANCE
    ) {
      if (++n > SAME_PIXELS) break;
    }
  }
  return n;
}

async function build(theme) {
  const dir = join(repoRoot, "screenshots-raw", `demo-${theme}`);
  const files = readdirSync(dir)
    .filter((f) => /^frame-\d+\.png$/.test(f))
    .sort()
    .map((f) => join(dir, f));
  if (!files.length) throw new Error(`no frames in ${dir}`);
  const out = join(outDir, `demo-${theme}.webp`);

  // Merge each run of identical frames into the first one of the run, with a longer delay.
  const kept = [];
  const delays = [];
  let prev = null;
  for (const f of files) {
    const raw = await sharp(f).removeAlpha().raw().toBuffer();
    if (prev && changed(prev, raw) <= SAME_PIXELS) {
      delays[delays.length - 1] += FRAME_MS;
    } else {
      kept.push(f);
      delays.push(FRAME_MS);
      prev = raw;
    }
  }
  const meta = await sharp(files[0]).metadata();
  console.log(theme, `${files.length} frames recorded, ${kept.length} distinct, ${meta.width}x${meta.height}`, `holds: ${delays.filter((d) => d > FRAME_MS).join(", ")} ms`);

  // Attempts, in order: lossless at native size (the UI is flat color and text, and the lossy encoder
  // leaves ghosts between frames), then lossy at decreasing quality, then smaller.
  const attempts = [
    { width: meta.width, lossless: true },
    { width: meta.width, quality: 92 },
    { width: meta.width, quality: 80 },
    { width: Math.round(meta.width * 0.85), quality: 75 },
  ];
  for (const a of attempts) {
    const frames = await Promise.all(kept.map((f) => sharp(f).resize({ width: a.width }).png().toBuffer()));
    await sharp(frames, { join: { animated: true } })
      .webp({ lossless: a.lossless ?? false, quality: a.quality ?? 100, effort: 5, delay: delays, loop: 0 })
      .toFile(out);
    const size = statSync(out).size;
    console.log(theme, `${a.width}px`, a.lossless ? "lossless" : `q${a.quality}`, `${(size / 1024).toFixed(0)} KB`);
    if (size <= MAX) return;
  }
  throw new Error(`${theme} demo is still over 2 MB`);
}

await build("light");
await build("dark");
