// Vite plugin: fills the placeholders in index.html (constants, screenshots, EDID dump, spec list, dividers).
// Everything here is static markup so the page still reads with JavaScript disabled.
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import type { Plugin } from "vite";
import { REPO, SITE_URL, RELEASES, LICENSE_URL, EXE_KB, IDLE_MB } from "../src/data/site.ts";
import { prismSvg, SMALL_VIEWBOX } from "../src/data/prism.ts";
import { SPACES } from "../src/data/spaces.ts";
import { coverage, tri } from "../src/data/color.ts";
import { MAIN_CALLOUTS } from "../src/data/hotspots.ts";
import monitor from "../src/data/monitor.json" with { type: "json" };

interface ShotInfo {
  pw: number;
  ph: number;
  scale: number;
  w: number;
  h: number;
}

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");

function loadShots(root: string): Record<string, ShotInfo> {
  const f = join(root, "src", "generated", "shots.json");
  return existsSync(f) ? JSON.parse(readFileSync(f, "utf8")) : {};
}

/** Windows 11 style caption strip drawn in CSS for the client-area-only 2x captures. */
function titlebar(title: string, id: string): string {
  const glyph = (inner: string, cls = "") => `<span class="tb-btn ${cls}"><svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1">${inner}</svg></span>`;
  return (
    `<div class="titlebar" aria-hidden="true"><span class="tb-title">${prismSvg(16, { id, viewBox: SMALL_VIEWBOX })}<span>${esc(title)}</span></span>` +
    `<span class="tb-btns">` +
    glyph('<path d="M0 5.5h10"/>') +
    glyph('<rect x=".5" y=".5" width="9" height="9"/>', "tb-off") +
    glyph('<path d="M0 0l10 10M10 0L0 10"/>') +
    `</span></div>`
  );
}

function picture(key: string, info: ShotInfo, alt: string, sizes: string, theme: "light" | "dark", eager: boolean, strip: string): string {
  const set = (ext: string) => `./shots/${key}.${ext} ${info.pw}w`;
  return (
    `<div class="shot th-${theme}">` +
    (strip && info.scale === 2 ? titlebar(strip, "tb" + key) : "") +
    `<picture>` +
    `<source type="image/avif" srcset="${set("avif")}" sizes="${sizes}">` +
    `<source type="image/webp" srcset="${set("webp")}" sizes="${sizes}">` +
    `<img src="./shots/${key}.png" srcset="${set("png")}" sizes="${sizes}" width="${info.w}" height="${info.h}" alt="${esc(alt)}" decoding="async"${eager ? "" : ' loading="lazy"'}>` +
    `</picture></div>`
  );
}

function ticks(): string {
  const labels = [400, 450, 500, 550, 600, 650, 700];
  return labels
    .map((nm, i) => `<li style="--p:${(((nm - 380) / 320) * 100).toFixed(2)}%">${i === 0 ? "λ " : ""}${nm}</li>`)
    .join("");
}

/** Which readouts each highlighted byte feeds. */
const BYTE_KEYS: Record<number, string> = { 0x17: "gamma", 0x19: "r g", 0x1a: "b w", 0x1b: "r", 0x1c: "r", 0x1d: "g", 0x1e: "g", 0x1f: "b", 0x20: "b", 0x21: "w", 0x22: "w" };

function edidHex(): string {
  const hex = monitor.edidHex;
  const bytes: string[] = [];
  for (let i = 0; i < 128; i++) bytes.push(hex.slice(i * 2, i * 2 + 2));
  const rows: string[] = [];
  for (let r = 0; r < 8; r++) {
    const cells = bytes
      .slice(r * 16, r * 16 + 16)
      .map((b, c) => {
        const i = r * 16 + c;
        const hot = (i >= 0x19 && i <= 0x22) || i === 0x17;
        const k = BYTE_KEYS[i];
        return `<span class="byte${hot ? " hot" : ""}" data-i="${i}"${k ? ` data-k="${k}"` : ""}>${b.toUpperCase()}</span>`;
      })
      .join("");
    rows.push(`<div class="edid-row" aria-hidden="true"><span class="edid-off">0x${(r * 16).toString(16).padStart(2, "0").toUpperCase()}</span>${cells}</div>`);
  }
  return rows.join("\n");
}

function edidReadout(): string {
  const p = monitor.primaries;
  const f = (n: number) => n.toFixed(4);
  const row = (k: string, name: string, q: { x: number; y: number }) =>
    `<li data-k="${k}"><span class="ro-name">${name}</span> <span class="ro-v">x ${f(q.x)}</span> <span class="ro-v">y ${f(q.y)}</span></li>`;
  return [
    row("r", "Red", p.r),
    row("g", "Green", p.g),
    row("b", "Blue", p.b),
    row("w", "White", p.w),
    `<li data-k="gamma"><span class="ro-name">Gamma</span> <span class="ro-v">${monitor.gamma}</span></li>`,
  ].join("\n");
}

function specRows(): string {
  const mon = tri(monitor.primaries as never);
  return SPACES.map((s) => {
    const cov = coverage(mon, tri(s.prims));
    const pct = (cov >= 99.95 ? 100 : Math.round(cov * 10) / 10).toString();
    return (
      `<li><button class="spec-row" type="button" data-space="${s.id}" aria-describedby="d-${s.id}" style="--c:${s.color}">` +
      `<span class="spec-line"><i class="spec-sw"></i><span class="spec-name">${s.name}</span><span class="spec-pct">${pct}%</span></span>` +
      `<span class="spec-desc" id="d-${s.id}">${esc(s.description)}</span></button></li>`
    );
  }).join("\n");
}

function callouts(): string {
  const texts = [
    "Hover a monitor and its real screen gets a border",
    "The SDR profile applies on selection",
    "HDR sliders write an MHC2 profile on release",
  ];
  return (
    `<ul class="callouts">` +
    MAIN_CALLOUTS.map(
      (c, i) =>
        `<li class="callout" style="--x:${(c.x * 100).toFixed(2)}%;--y:${(c.y * 100).toFixed(2)}%"><i class="callout-dot"><b>${i + 1}</b></i><span class="callout-line"></span><span class="callout-label" aria-hidden="true">${texts[i]}</span></li>`,
    ).join("") +
    `</ul>`
  );
}

export function spectrumHtml(): Plugin {
  let root = process.cwd();
  return {
    name: "spectrum-html",
    configResolved(c) {
      root = c.root;
    },
    transformIndexHtml: {
      order: "pre",
      handler(html) {
        const shots = loadShots(root);
        const get = (attrs: string, name: string) => new RegExp(`${name}="([^"]*)"`).exec(attrs)?.[1] ?? "";
        const unesc = (s: string) => s.replace(/&quot;/g, '"');
        return html
          .replace(/<shot\s+([^>]*)><\/shot>/g, (_m, attrs: string) => {
            const name = get(attrs, "name");
            const sizes = get(attrs, "sizes") || "100vw";
            const eager = /\beager\b/.test(attrs);
            const strip = get(attrs, "strip");
            const out: string[] = [];
            for (const theme of ["light", "dark"] as const) {
              const key = `${name}-${theme}`;
              const info = shots[key];
              if (!info) throw new Error(`No screenshot ${key}; run npm run images`);
              out.push(picture(key, info, unesc(get(attrs, `alt-${theme}`)), sizes, theme, eager, strip));
            }
            return out.join("");
          })
          .replace(/<spectral><\/spectral>/g, `<div class="spectral" aria-hidden="true"><canvas class="spectral-canvas"></canvas><i class="spectral-scan"></i><ol class="spectral-ticks">${ticks()}</ol></div>`)
          .replace(/<!--prism:(\d+)-->/g, (_m, n: string) => prismSvg(Number(n), { id: "p" + n }))
          .replace("<!--edid-hex-->", edidHex())
          .replace("<!--edid-readout-->", edidReadout())
          .replace("<!--spec-rows-->", specRows())
          .replace("<!--callouts-->", callouts())
          .replace(/%REPO%/g, REPO)
          .replace(/%SITE_URL%/g, SITE_URL)
          .replace(/%RELEASES%/g, RELEASES)
          .replace(/%LICENSE%/g, LICENSE_URL)
          .replace(/%EXE_KB%/g, String(EXE_KB))
          .replace(/%IDLE_MB%/g, String(IDLE_MB));
      },
    },
  };
}
