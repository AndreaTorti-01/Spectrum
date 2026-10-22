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

/**
 * 8. Driver updates: three lines. Applied is a solid ink line, the driver update leaves a broken line (gap and
 * offset pieces), Spectrum's restore is a solid line in the spectral gradient. The markup and CSS are that end
 * state; the animation plays on every entry (scroll in, scroll back up into it) and its last frame is the same
 * picture, so static, reduced-motion and finished states match.
 */
export function initDrivers() {
  const section = document.querySelector<HTMLElement>("#drivers");
  if (!section || isStatic) return;
  const num = (el: Element, a: string) => Number(el.getAttribute(a));

  section.querySelectorAll<SVGSVGElement>(".drv-diagram").forEach((svg) => {
    const q = (c: string) => svg.querySelector<SVGLineElement>(c)!;
    const l1 = q(".drv-l1");
    const a = q(".drv-l2a");
    const b = q(".drv-l2b");
    const l3 = q(".drv-l3");
    const nodes = svg.querySelectorAll(".drv-node");
    const labels = svg.querySelectorAll(".drv-labels text");
    [l1, a, b, l3].forEach((l) => l.setAttribute("pathLength", "1"));

    // Broken end state (from the markup) and the intact line it starts from.
    const aEnd = num(a, "x2");
    const bStart = num(b, "x1");
    const mid = Math.round((aEnd + bStart) / 2);
    const y = num(a, "y1");
    const bY = num(b, "y1");
    const draw = { strokeDasharray: 1, strokeDashoffset: 1 };

    // Every tween ends on the values of the static markup (dash offset 0 on a pathLength 1 line is a solid line,
    // opacity 1, x 0), so nothing needs clearing: clearing props would stop a later restart from re-applying them.
    const finish = () => svg.classList.remove("is-anim", "is-broken");
    const tl = gsap.timeline({
      paused: true,
      onStart: () => {
        svg.classList.add("is-anim");
        svg.classList.remove("is-broken");
      },
      onComplete: finish,
    });
    svg.classList.add("is-anim");
    tl.fromTo(nodes[0]!, { opacity: 0 }, { opacity: 1, duration: 0.3 }, 0)
      .fromTo(labels[0]!, { opacity: 0 }, { opacity: 1, duration: 0.3 }, 0.1)
      .fromTo(l1, draw, { strokeDashoffset: 0, duration: 0.8, ease: "power2.inOut" }, 0.3)
      // step 2: the line is there, then the driver update glitches and breaks it
      .fromTo(nodes[1]!, { opacity: 0 }, { opacity: 1, duration: 0.3 }, 1.2)
      .fromTo(labels[1]!, { opacity: 0 }, { opacity: 1, duration: 0.3 }, 1.4)
      .fromTo(a, { ...draw, attr: { x2: mid } }, { strokeDashoffset: 0, duration: 0.6, ease: "power2.out" }, 1.2)
      .fromTo(b, { ...draw, attr: { x1: mid, y1: y, y2: y } }, { strokeDashoffset: 0, duration: 0.6, ease: "power2.out" }, 1.5)
      .to(b, { x: 7, duration: 0.07 }, 2.6)
      .to(b, { x: -5, duration: 0.07 })
      .to(b, { x: 4, duration: 0.07 })
      .to(b, { x: 0, duration: 0.07 })
      .call(() => void svg.classList.add("is-broken"), [], 2.9)
      .to(a, { attr: { x2: aEnd }, duration: 0.18, ease: "power2.out" }, 2.9)
      .to(b, { attr: { x1: bStart, y1: bY, y2: bY }, duration: 0.18, ease: "power2.out" }, 2.9)
      // step 3: a spectral line is drawn in
      .fromTo(nodes[2]!, { opacity: 0 }, { opacity: 1, duration: 0.3 }, 3.7)
      .fromTo(labels[2]!, { opacity: 0 }, { opacity: 1, duration: 0.3 }, 3.8)
      .fromTo(l3, draw, { strokeDashoffset: 0, duration: 1.2, ease: "power2.inOut" }, 3.9);

    ScrollTrigger.create({ trigger: section, start: "top 55%", end: "bottom top", animation: tl, toggleActions: "restart none restart none" });
  });
}
