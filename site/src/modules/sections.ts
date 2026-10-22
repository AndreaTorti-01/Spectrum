// Section-level interactions: window callouts, gamut hover overlay, tray menu spring, driver-update diagram.
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { tri } from "../data/color.ts";
import { GAMUT_PLOT } from "../data/hotspots.ts";
import { SPACES } from "../data/spaces.ts";
import { isStatic } from "./env.ts";

/** 4. Main window: leader lines draw in. */
export function initCallouts() {
  if (isStatic) return;
  const stage = document.querySelector<HTMLElement>(".win-stage");
  if (!stage) return;
  const dots = stage.querySelectorAll(".callout-dot");
  const lines = stage.querySelectorAll(".callout-line");
  const labels = stage.querySelectorAll(".callout-label");
  gsap.set(dots, { scale: 0 });
  gsap.set(lines, { scaleX: 0 });
  gsap.set(labels, { opacity: 0 });
  ScrollTrigger.create({
    trigger: stage,
    start: "top 55%",
    once: true,
    onEnter: () => {
      const tl = gsap.timeline();
      dots.forEach((d, i) => {
        const at = i * 0.35;
        tl.to(d, { scale: 1, duration: 0.35, ease: "back.out(2)" }, at)
          .to(lines[i]!, { scaleX: 1, duration: 0.6, ease: "power2.out" }, at + 0.15)
          .to(labels[i]!, { opacity: 1, duration: 0.4 }, at + 0.55);
      });
    },
  });
}

/** 6. Gamut: hovering or focusing a space draws its triangle over the screenshot. */
export function initGamut() {
  const svg = document.querySelector<SVGSVGElement>(".gamut-overlay");
  const rows = Array.from(document.querySelectorAll<HTMLButtonElement>(".spec-row"));
  if (!svg || !rows.length) return;
  const { x0, sx, y0, sy } = GAMUT_PLOT;
  svg.setAttribute("viewBox", "0 0 1 1");
  const polys = new Map<string, SVGPolygonElement>();
  for (const s of SPACES) {
    const p = document.createElementNS("http://www.w3.org/2000/svg", "polygon");
    p.setAttribute("points", tri(s.prims).map((q) => `${(x0 + q.x * sx).toFixed(4)},${(y0 - q.y * sy).toFixed(4)}`).join(" "));
    p.setAttribute("fill", s.color);
    p.setAttribute("fill-opacity", "0.16");
    p.setAttribute("stroke", s.color);
    p.style.opacity = "0";
    svg.appendChild(p);
    polys.set(s.id, p);
  }
  const show = (id: string | null) => {
    polys.forEach((p, k) => (p.style.opacity = k === id ? "1" : "0"));
  };
  let pinned: string | null = null;
  rows.forEach((row) => {
    const id = row.dataset.space!;
    row.addEventListener("pointerenter", () => show(id));
    row.addEventListener("pointerleave", () => show(pinned));
    row.addEventListener("focus", () => show(id));
    row.addEventListener("blur", () => show(pinned));
    row.addEventListener("click", () => {
      pinned = pinned === id ? null : id;
      rows.forEach((r) => r.classList.toggle("active", r.dataset.space === pinned));
      show(pinned);
    });
  });
}

/** 7. Tray: the menu springs out of the tray icon. */
export function initTray() {
  const bar = document.querySelector<HTMLElement>("[data-tray]");
  const menu = bar?.querySelector<HTMLElement>(".slab-tray");
  const icon = bar?.querySelector<HTMLButtonElement>(".tray-icon");
  if (!bar || !menu || !icon) return;
  let open = true;
  const set = (next: boolean) => {
    open = next;
    icon.setAttribute("aria-expanded", String(open));
    if (open) {
      gsap.fromTo(menu, { scale: 0.92, opacity: 0, y: 14, display: "block", transformOrigin: "100% 100%" }, { scale: 1, opacity: 1, y: 0, duration: 0.7, ease: "back.out(1.6)" });
    } else {
      gsap.to(menu, { scale: 0.94, opacity: 0, y: 10, duration: 0.2, ease: "power2.in", onComplete: () => void gsap.set(menu, { display: "none" }) });
    }
  };
  icon.addEventListener("click", () => set(!open));
  if (isStatic) return;
  gsap.set(menu, { opacity: 0, scale: 0.92, transformOrigin: "100% 100%" });
  ScrollTrigger.create({ trigger: bar, start: "top 92%", once: true, onEnter: () => set(true) });
}

/** 8. Driver updates: the line breaks with a short glitch, then re-forms with a spectral sweep. */
export function initDrivers() {
  const section = document.querySelector<HTMLElement>("#drivers");
  if (!section) return;
  const diagrams = Array.from(section.querySelectorAll<SVGSVGElement>(".drv-diagram"));
  const q = (svg: SVGElement, c: string) => svg.querySelector<SVGLineElement>(c)!;

  // Static / reduced motion end state: restored line, no break.
  const restored = (svg: SVGSVGElement) => {
    gsap.set([q(svg, ".drv-seg-2a"), q(svg, ".drv-seg-2b"), q(svg, ".drv-seg-sweep")], { opacity: 0 });
  };
  diagrams.forEach((svg) => {
    q(svg, ".drv-seg-final").setAttribute("pathLength", "1");
    q(svg, ".drv-seg-sweep").setAttribute("pathLength", "1");
    q(svg, ".drv-seg-1").setAttribute("pathLength", "1");
    restored(svg);
  });
  if (isStatic) return;

  const play = (svg: SVGSVGElement) => {
    const vertical = svg.classList.contains("drv-v");
    const ax = vertical ? "y" : "x";
    const seg1 = q(svg, ".drv-seg-1");
    const a = q(svg, ".drv-seg-2a");
    const b = q(svg, ".drv-seg-2b");
    const fin = q(svg, ".drv-seg-final");
    const sweep = q(svg, ".drv-seg-sweep");
    const node2 = q(svg, ".drv-node-2");
    const labels = svg.querySelectorAll(".drv-labels text");
    const nodes = svg.querySelectorAll(".drv-node");
    const mid = Number(a.getAttribute(`${ax}2`));
    const jitter = (el: Element, d: number) => gsap.set(el, { [ax]: d });

    gsap.set([seg1, fin], { opacity: 1, strokeDasharray: 1, strokeDashoffset: 1 });
    gsap.set([a, b], { opacity: 1, strokeDasharray: 1, strokeDashoffset: 1 });
    gsap.set(nodes, { opacity: 0 });
    gsap.set(labels, { opacity: 0 });
    gsap.set(sweep, { opacity: 0, strokeDasharray: "0.22 1.5", strokeDashoffset: 0.22 });

    const tl = gsap.timeline();
    tl.to(nodes[0]!, { opacity: 1, duration: 0.3 }, 0)
      .to(labels[0]!, { opacity: 1, duration: 0.3 }, 0.1)
      .to(seg1, { strokeDashoffset: 0, duration: 0.8, ease: "power2.inOut" }, 0.3)
      .to(nodes[1]!, { opacity: 1, duration: 0.3 }, 1.0)
      .to([a, b], { strokeDashoffset: 0, duration: 0.7, ease: "power2.out", stagger: 0.25 }, 1.0)
      .to(labels[1]!, { opacity: 1, duration: 0.3 }, 1.4)
      // the glitch: three frames of jitter, then the gap opens
      .call(() => void jitter(a, 7), [], 2.3)
      .call(() => void jitter(a, -5), [], 2.38)
      .call(() => void jitter(a, 4), [], 2.46)
      .call(() => void jitter(a, 0), [], 2.54)
      .to(node2, { stroke: "var(--c-r)", duration: 0.1 }, 2.3)
      .to(a, { attr: { [`${ax}2`]: mid - (vertical ? 36 : 44) }, duration: 0.12 }, 2.54)
      .to(b, { attr: { [`${ax}1`]: mid + (vertical ? 14 : 12) }, strokeDasharray: "0.04 0.03", duration: 0.12 }, 2.54)
      // restore: a spectral sweep re-forms the line
      .to(nodes[2]!, { opacity: 1, duration: 0.3 }, 3.5)
      .to(labels[2]!, { opacity: 1, duration: 0.3 }, 3.6)
      .set(sweep, { opacity: 1 }, 3.7)
      .to(fin, { strokeDashoffset: 0, duration: 1.1, ease: "power2.inOut" }, 3.7)
      .to(sweep, { strokeDashoffset: -1.3, duration: 1.3, ease: "power2.inOut" }, 3.7)
      .to([a, b], { opacity: 0, duration: 0.4 }, 4.2)
      .to(node2, { stroke: "var(--ink)", duration: 0.3 }, 4.4)
      .to(sweep, { opacity: 0, duration: 0.3 }, 4.9);
  };

  ScrollTrigger.create({
    trigger: section,
    start: "top 55%",
    once: true,
    onEnter: () => diagrams.forEach(play),
  });
  // hide the end state until the section is entered
  diagrams.forEach((svg) => {
    gsap.set(svg.querySelectorAll(".drv-node, .drv-labels text"), { opacity: 0 });
    gsap.set([q(svg, ".drv-seg-1"), q(svg, ".drv-seg-final"), q(svg, ".drv-seg-2a"), q(svg, ".drv-seg-2b")], { opacity: 0 });
  });
}
