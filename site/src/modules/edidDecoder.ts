// EDID section: bytes 0x19-0x22 light up in sequence, connector lines draw to the decoded readouts,
// then a mini CIE plot strokes the triangle. The markup (hex dump, readouts) is static HTML.
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import type { Prims } from "../data/spaces.ts";
import monitor from "../data/monitor.json";
import { CieDiagram } from "./cieCanvas.ts";
import { debounce, isDesktop, isStatic } from "./env.ts";

const GROUP_ORDER = ["r", "g", "b", "w", "gamma"];
const GROUP_COLOR: Record<string, string> = { r: "var(--c-r)", g: "var(--c-g)", b: "var(--c-b)", w: "var(--ink)", gamma: "var(--ink-dim)" };

export function initEdid() {
  const section = document.querySelector<HTMLElement>("#edid");
  const grid = section?.querySelector<HTMLElement>(".edid-grid");
  const svg = section?.querySelector<SVGSVGElement>(".edid-lines");
  const plot = section?.querySelector<HTMLCanvasElement>("#edid-cie");
  if (!section || !grid || !svg || !plot) return;

  const bytes = Array.from(section.querySelectorAll<HTMLElement>(".byte.hot")).sort((a, b) => Number(a.dataset.i) - Number(b.dataset.i));
  const readouts = Array.from(section.querySelectorAll<HTMLElement>(".edid-readout li"));
  const diagram = new CieDiagram(plot, { lazy: true });
  const prims = monitor.primaries as Prims;
  const state = { drawn: isStatic, progress: isStatic ? 1 : 0 };
  const setProgress = () => {
    diagram.tris = [{ prims, color: "ink", width: 1.5, progress: state.progress, white: state.progress >= 1 }];
    diagram.requestDraw();
  };
  setProgress();

  let paths: { el: SVGPathElement; byte: HTMLElement; key: string }[] = [];

  const build = () => {
    svg.replaceChildren();
    paths = [];
    if (!isDesktop()) return;
    const g = grid.getBoundingClientRect();
    const dump = section.querySelector<HTMLElement>(".edid-dump")!.getBoundingClientRect();
    // One horizontal lane per connector. Bytes above a row route over it, bytes of the 0x20 row under it; within
    // a side the leftmost byte takes the outermost lane, so a lane never runs through a neighbour's stub.
    // Each readout gets its own vertical bus, the topmost readout the farthest one out.
    type Link = { byte: HTMLElement; key: string; i: number; below: boolean };
    const links: Link[] = [];
    for (const byte of bytes) {
      const i = Number(byte.dataset.i);
      for (const key of (byte.dataset.k ?? "").split(" ")) if (key) links.push({ byte, key, i, below: i >= 0x20 });
    }
    const lane = new Map<Link, number>();
    for (const side of [false, true]) {
      const group = links.filter((l) => l.below === side).sort((p, q) => p.i - q.i);
      group.forEach((l, k) => lane.set(l, group.length - 1 - k));
    }
    for (const l of links) {
      const li = section.querySelector<HTMLElement>(`.edid-readout li[data-k="${l.key}"]`);
      if (!li) continue;
      const gi = GROUP_ORDER.indexOf(l.key);
      const r = l.byte.getBoundingClientRect();
      const lr = li.getBoundingClientRect();
      const x0 = r.left + r.width / 2 - g.left;
      const y0 = (l.below ? r.bottom : r.top) - g.top;
      const rail = y0 + (l.below ? 1 : -1) * (8 + lane.get(l)! * 3.5);
      const bus = dump.right - g.left + 14 + (GROUP_ORDER.length - 1 - gi) * 9;
      const yEnd = lr.top - g.top + 8;
      const xEnd = lr.left - g.left - 8;
      const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
      path.setAttribute("d", `M${x0} ${y0} V${rail} H${bus} V${yEnd} H${xEnd}`);
      path.setAttribute("pathLength", "1");
      path.style.stroke = GROUP_COLOR[l.key]!;
      path.style.strokeDasharray = "1";
      path.style.strokeDashoffset = state.drawn ? "0" : "1";
      path.style.opacity = l.i === 0x19 || l.i === 0x1a ? "0.45" : "0.9";
      svg.appendChild(path);
      paths.push({ el: path, byte: l.byte, key: l.key });
    }
  };

  const finish = () => {
    state.drawn = true;
    bytes.forEach((b) => b.classList.add("lit"));
    readouts.forEach((r) => r.classList.add("on"));
    state.progress = 1;
    setProgress();
    paths.forEach((p) => (p.el.style.strokeDashoffset = "0"));
  };

  document.fonts.ready.then(() => {
    build();
    if (isStatic) finish();
  });
  addEventListener("resize", debounce(build, 150));
  ScrollTrigger.addEventListener("refresh", build);

  if (isStatic) {
    finish();
    return;
  }

  ScrollTrigger.create({
    trigger: section,
    start: "top 62%",
    once: true,
    onEnter: () => {
      build();
      const tl = gsap.timeline();
      bytes.forEach((b, n) => {
        const at = n * 0.13;
        tl.call(() => void b.classList.add("lit"), [], at);
        paths
          .filter((p) => p.byte === b)
          .forEach((p) => {
            tl.to(p.el, { strokeDashoffset: 0, duration: 0.7, ease: "power2.out" }, at + 0.05);
            tl.call(() => void section.querySelector(`.edid-readout li[data-k="${p.key}"]`)?.classList.add("on"), [], at + 0.55);
          });
      });
      if (!isDesktop()) readouts.forEach((r, n) => tl.call(() => void r.classList.add("on"), [], 0.2 + n * 0.12));
      const end = tl.duration();
      tl.to(state, { progress: 1, duration: 1.2, ease: "power2.inOut", onUpdate: setProgress }, end - 0.2);
    },
  });
}
