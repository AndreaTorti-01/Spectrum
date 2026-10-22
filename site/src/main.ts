/*
 * Spectrum site entry point.
 *
 * Screenshot mode (used for visual checks and by the screenshot skill):
 *   ?shot=<section-id>&theme=light|dark
 *   <section-id> is one of hero, edid, window, hdr, gamut, tray, drivers, download, footer, or "all"
 *   (no scroll jump, whole page). The page jumps to that section, forces the theme (not persisted), turns
 *   off Lenis, pins and every animation, and renders each animation in its final state.
 *   The same static path is used for prefers-reduced-motion.
 */
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { SplitText } from "gsap/SplitText";
import Lenis from "lenis";

import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/layout.css";
import "./styles/sections.css";

import { isStatic, shot } from "./modules/env.ts";
import { initNav } from "./modules/nav.ts";
import { initSpectralBars } from "./modules/spectralBar.ts";
import { initReveal } from "./modules/reveal.ts";
import { initHero } from "./modules/hero.ts";
import { initEdid } from "./modules/edidDecoder.ts";
import { initSlabs } from "./modules/glassSlab.ts";
import { initHdrLab } from "./modules/hdrLab.ts";
import { initCallouts, initDrivers, initGamut, initTray } from "./modules/sections.ts";

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

initSpectralBars();
initReveal();
initHero();
initEdid();
initHdrLab();
initSlabs();
initCallouts();
initGamut();
initTray();
initDrivers();

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
  });
});
