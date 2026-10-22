// Hero diagram: the gamut triangle morphs sRGB -> Display P3 -> Adobe RGB (1998) -> the author's monitor,
// scrubbed by scroll. Vertices come from the real EDID in monitor.json.
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
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
  const st = { p: isStatic ? 1 : 0 };

  const render = () => {
    // hold the first and last state for a moment, morph in between
    const t = Math.min(1, Math.max(0, (st.p - 0.06) / 0.88)) * 3;
    const i = Math.min(2, Math.floor(t));
    const e = ease(t - i);
    const eased = i + e;
    diagram.tris = [{ prims: lerpPrims(states[i]!.prims, states[i + 1]!.prims, e), color: "ink", width: 1.5, white: true }];
    diagram.labels = states.map((s, j) => ({ title: s.name, sub: sub[j]!, alpha: Math.max(0, 1 - Math.abs(eased - j)) }));
    diagram.requestDraw();
  };
  render();
  if (isStatic) return;

  const mm = gsap.matchMedia();
  mm.add("(min-width: 900px)", () => {
    gsap.to(st, {
      p: 1,
      ease: "none",
      onUpdate: render,
      scrollTrigger: { trigger: ".hero-stage", start: "top top+=64", end: "+=120%", pin: true, scrub: 0.6, anticipatePin: 1 },
    });
  });
  mm.add("(max-width: 899px)", () => {
    gsap.to(st, {
      p: 1,
      ease: "none",
      onUpdate: render,
      scrollTrigger: { trigger: canvas, start: "top 80%", end: "bottom 40%", scrub: 0.6 },
    });
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
  ScrollTrigger.refresh();
}
