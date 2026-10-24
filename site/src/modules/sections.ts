// Section-level interactions: window callouts, gamut hover overlay, tray menu spring.
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

/** 4. Main window section, taskbar: the menu springs out of the tray icon. */
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
