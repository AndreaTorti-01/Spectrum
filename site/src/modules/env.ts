// Shared runtime flags and the theme-change signal.
const params = new URLSearchParams(location.search);

/** Section id from ?shot=<id>, "all" for a static full page, or null. */
export const shot: string | null = params.get("shot");
export const reduceMotion = matchMedia("(prefers-reduced-motion: reduce)").matches;
/** True when nothing may animate: reduced motion, or the screenshot mode (?shot=). */
export const isStatic = reduceMotion || shot !== null;
export const finePointer = matchMedia("(hover: hover) and (pointer: fine)").matches;
export const isDesktop = () => matchMedia("(min-width: 900px)").matches;

/** Work that must finish before the page counts as rendered (read by the ?shot code path). */
export const pending: Promise<unknown>[] = [];

/** Runs fn when the main thread is idle (setTimeout where requestIdleCallback is missing). */
export function whenIdle(fn: () => void) {
  if ("requestIdleCallback" in window) window.requestIdleCallback(fn, { timeout: 300 });
  else setTimeout(fn, 16);
}

export const THEME_EVENT = "spectrum:theme";
export const onTheme = (fn: () => void) => window.addEventListener(THEME_EVENT, fn);

export interface Palette {
  bg: string;
  ink: string;
  dim: string;
  rule: string;
  horseshoeAlpha: number;
}

let cached: Palette | null = null;
export function palette(): Palette {
  if (!cached) {
    const cs = getComputedStyle(document.documentElement);
    const v = (n: string) => cs.getPropertyValue(n).trim();
    cached = { bg: v("--bg"), ink: v("--ink"), dim: v("--ink-dim"), rule: v("--rule"), horseshoeAlpha: parseFloat(v("--horseshoe-alpha")) || 0.4 };
  }
  return cached;
}
onTheme(() => (cached = null));

/** Backing-store size for a canvas, capped so huge screens do not allocate silly buffers. */
export function fitCanvas(c: HTMLCanvasElement): { w: number; h: number; dpr: number } {
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const w = c.clientWidth;
  const h = c.clientHeight;
  c.width = Math.max(1, Math.round(w * dpr));
  c.height = Math.max(1, Math.round(h * dpr));
  return { w, h, dpr };
}

export function debounce<T extends (...a: never[]) => void>(fn: T, ms: number): T {
  let t = 0;
  return ((...a: never[]) => {
    clearTimeout(t);
    t = window.setTimeout(() => fn(...a), ms);
  }) as T;
}

export const MONO = '500 11px "IBM Plex Mono", ui-monospace, Consolas, monospace';
