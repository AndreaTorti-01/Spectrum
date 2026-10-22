/*
 * Spectrum site entry point.
 *
 * Screenshot mode (used for visual checks and by the screenshot skill):
 *   ?shot=<section-id>&theme=light|dark
 *   <section-id> is one of hero, window, hdr, gamut, tray, footer, or "all"
 *   (no scroll jump, whole page). The page jumps to that section, forces the theme (not persisted), turns
 *   off Lenis, pins and every animation, and renders each animation in its final state.
 *   The same static path is used for prefers-reduced-motion. window.__shotReady becomes true once the final
 *   state has rendered (fonts, images, canvas fills); scripts/shots.mjs waits for it.
 */
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { SplitText } from "gsap/SplitText";
import Lenis from "lenis";

import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/layout.css";
import "./styles/sections.css";

import { isStatic, pending, shot } from "./modules/env.ts";
import { initNav } from "./modules/nav.ts";
import { initSpectralBars } from "./modules/spectralBar.ts";
import { initReveal } from "./modules/reveal.ts";
import { initHero } from "./modules/hero.ts";
import { initSlabs } from "./modules/glassSlab.ts";
import { initHdrLab } from "./modules/hdrLab.ts";
import { initCallouts, initGamut, initTray } from "./modules/sections.ts";

declare global {
  interface Window {
    __shotReady?: boolean;
  }
}

gsap.registerPlugin(ScrollTrigger, SplitText);

const root = document.documentElement;
if (isStatic) root.classList.add("static");
if (shot !== null) document.querySelectorAll<HTMLImageElement>('img[loading="lazy"]').forEach((i) => (i.loading = "eager"));

initNav();

let lenis: Lenis | null = null;
if (!isStatic) {
  lenis = new Lenis({ lerp: 0.1, wheelMultiplier: 1 });
  lenis.on("scroll", ScrollTrigger.update);
  gsap.ticker.add((t) => lenis!.raf(t * 1000));
  gsap.ticker.lagSmoothing(0);
}

// In-page anchors: smooth under Lenis, native otherwise.
document.querySelectorAll<HTMLAnchorElement>('a[href^="#"]').forEach((a) => {
  a.addEventListener("click", (e) => {
    const id = a.getAttribute("href")!.slice(1);
    const target = id === "hero" ? document.body : document.getElementById(id);
    if (!target) return;
    e.preventDefault();
    if (lenis) lenis.scrollTo(id === "hero" ? 0 : target, { offset: -64 });
    else window.scrollTo({ top: id === "hero" ? 0 : target.getBoundingClientRect().top + scrollY - 64 });
    history.replaceState(null, "", "#" + id);
  });
});

/** Lets the browser paint and handle input between two init steps, so no single task runs long. */
const nextTask = () => new Promise<void>((r) => setTimeout(r, 0));
for (const step of [initSpectralBars, initReveal, initHero, initHdrLab, initSlabs, initCallouts, initGamut, initTray]) {
  step();
  if (!isStatic) await nextTask();
}

function jumpToShot() {
  if (shot === null || shot === "all") return;
  const el = document.getElementById(shot);
  if (!el) return;
  const top = shot === "hero" ? 0 : el.getBoundingClientRect().top + scrollY - 64;
  window.scrollTo(0, Math.max(0, top));
}

Promise.all([document.fonts.ready, new Promise<void>((r) => (document.readyState === "complete" ? r() : addEventListener("load", () => r())))]).then(() => {
  ScrollTrigger.refresh();
  jumpToShot();
  requestAnimationFrame(() => {
    jumpToShot();
    root.classList.add("shot-ready");
    // ?shot= path: tell the capture script (scripts/shots.mjs) once the canvases have their final fill.
    if (shot !== null) void Promise.all(pending).then(() => requestAnimationFrame(() => (window.__shotReady = true)));
  });
});
