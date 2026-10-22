// Hero diagram: the gamut triangle morphs sRGB -> Display P3 -> Adobe RGB (1998) -> the author's monitor.
// Desktop: scrubbed by a pinned scroll. 701-899 px: scrubbed by scroll without a pin. 700 px and below: a gentle
// auto-cycle through the four gamuts (3 s each) while the diagram is on screen. Static (reduced motion, ?shot):
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
  // scrub progress p 0..1: hold the first and last state for a moment, morph in between
  const st = { p: isStatic ? 1 : 0 };
  const scrub = () => render(Math.min(1, Math.max(0, (st.p - 0.06) / 0.88)) * (n - 1));
  scrub();
  if (isStatic) return;

  const mm = gsap.matchMedia();
  mm.add("(min-width: 900px)", () => {
    gsap.to(st, {
      p: 1,
      ease: "none",
      onUpdate: scrub,
      scrollTrigger: { trigger: ".hero-stage", start: "top top+=64", end: "+=120%", pin: true, scrub: 0.6, anticipatePin: 1 },
    });
  });
  mm.add("(min-width: 701px) and (max-width: 899px)", () => {
    gsap.to(st, {
      p: 1,
      ease: "none",
      onUpdate: scrub,
      scrollTrigger: { trigger: canvas, start: "top 80%", end: "bottom 40%", scrub: 0.6 },
    });
  });
  mm.add("(max-width: 700px)", () => {
    // 3 s per gamut: hold 2.2 s, morph 0.8 s; the last morph returns to the first gamut. Runs only in view.
    const c = { t: 0 };
    const tl = gsap.timeline({ repeat: -1, paused: true, onUpdate: () => render(c.t) });
    states.forEach((_s, i) => tl.to(c, { t: i + 1, duration: 0.8, ease: "none" }, i * 3 + 2.2));
    tl.to({}, { duration: 0 }, n * 3);
    const io = new IntersectionObserver((es) => es.forEach((e) => (e.isIntersecting ? tl.play() : tl.pause())));
    io.observe(canvas);
    return () => {
      io.disconnect();
      tl.kill();
    };
  });

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
