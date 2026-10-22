// Hero diagram: the gamut triangle morphs sRGB -> Display P3 -> Adobe RGB (1998) -> the author's monitor.
// Autoplays on its own while the diagram is on screen (no scroll pin or scrub). Static (reduced motion, ?shot):
// the monitor's triangle. Vertices come from the real EDID in monitor.json.
import { gsap } from "gsap";
import { areaPercent, lerpPrims, tri } from "../data/color.ts";
import { SPACE_BY_ID, type Prims } from "../data/spaces.ts";
import monitor from "../data/monitor.json";
import { CieDiagram } from "./cieCanvas.ts";
import { finePointer, isStatic } from "./env.ts";

export const MONITOR_PRIMS = monitor.primaries as Prims;

export function initHero() {
  const canvas = document.querySelector<HTMLCanvasElement>("#hero-cie");
  if (!canvas) return;
  const diagram = new CieDiagram(canvas, { wavelengths: true, axes: true, clipToViewport: true });

  const states: { name: string; prims: Prims }[] = [
    { name: SPACE_BY_ID.srgb!.name, prims: SPACE_BY_ID.srgb!.prims },
    { name: SPACE_BY_ID["display-p3"]!.name, prims: SPACE_BY_ID["display-p3"]!.prims },
    { name: SPACE_BY_ID["adobe-rgb"]!.name, prims: SPACE_BY_ID["adobe-rgb"]!.prims },
    { name: monitor.name, prims: MONITOR_PRIMS },
  ];
  const sub = states.map((s) => `${areaPercent(tri(s.prims)).toFixed(1)}% of CIE 1931 xy`);
  const ease = gsap.parseEase("power2.inOut");
  const n = states.length;

  /** t = 0..n: integer values are the gamuts themselves, in between is the morph (t = n wraps to the first). */
  const render = (t: number) => {
    const i = Math.min(n - 1, Math.floor(t));
    const e = ease(t - i);
    const pos = i + e;
    diagram.tris = [{ prims: lerpPrims(states[i]!.prims, states[(i + 1) % n]!.prims, e), color: "ink", width: 1.5, white: true }];
    diagram.labels = states.map((s, j) => {
      const d = Math.abs(pos - j);
      return { title: s.name, sub: sub[j]!, alpha: Math.max(0, 1 - Math.min(d, n - d)) };
    });
    diagram.requestDraw();
  };
  // Static (reduced motion, ?shot=): the monitor's triangle.
  if (isStatic) {
    render(n - 1);
    return;
  }
  render(0);

  // Autoplay, same on every screen size: hold each gamut 2.4 s, morph 0.9 s; the last morph returns to the first
  // gamut. Plays only while the diagram is in view and the tab is visible. No scroll pin, no scrub.
  const HOLD = 2.4;
  const MORPH = 0.9;
  const c = { t: 0 };
  const tl = gsap.timeline({ repeat: -1, paused: true, onUpdate: () => render(c.t) });
  states.forEach((_s, i) => tl.to(c, { t: i + 1, duration: MORPH, ease: "none" }, i * (HOLD + MORPH) + HOLD));
  tl.to({}, { duration: 0 }, n * (HOLD + MORPH));
  let inView = false;
  const sync = () => (inView && !document.hidden ? tl.play() : tl.pause());
  new IntersectionObserver((es) => {
    inView = es[es.length - 1]!.isIntersecting;
    sync();
  }).observe(canvas);
  document.addEventListener("visibilitychange", sync);

  if (finePointer) {
    const hero = document.querySelector<HTMLElement>("#hero")!;
    const qx = gsap.quickTo(diagram.parallax, "x", { duration: 0.8, ease: "power3", onUpdate: () => diagram.requestDraw() });
    const qy = gsap.quickTo(diagram.parallax, "y", { duration: 0.8, ease: "power3", onUpdate: () => diagram.requestDraw() });
    hero.addEventListener("pointermove", (e) => {
      qx(((e.clientX / innerWidth) * 2 - 1) * 10);
      qy(((e.clientY / innerHeight) * 2 - 1) * 10);
    });
  }
}
