// HDR lab: four real range inputs drive a PQ (ST 2084) response chart and a BT.709 -> BT.2020 gamut chart.
// Ranges match the app: black 0-0.5 nits, peak 100-4000 (log), full-frame 100-4000 (log, independent of peak), saturation 0-100%.
import { gsap } from "gsap";
import { pqToNits, saturatedPrims } from "../data/color.ts";
import { BT709, BT2020 } from "../data/spaces.ts";
import { CieDiagram } from "./cieCanvas.ts";
import { fitCanvas, palette, MONO, onTheme, debounce } from "./env.ts";

const YMIN = 0.01;
const YMAX = 10000;

const peakOf = (v: number) => 100 * Math.pow(40, v / 1000);
const blackOf = (v: number) => 0.5 * Math.pow(v / 1000, 2);

const fmtNits = (n: number) => (n >= 100 ? Math.round(n).toLocaleString("en-US") : n.toFixed(n >= 10 ? 1 : 2));

export function initHdrLab() {
  const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T | null;
  const rBlack = $<HTMLInputElement>("r-black");
  const rPeak = $<HTMLInputElement>("r-peak");
  const rFull = $<HTMLInputElement>("r-full");
  const rSat = $<HTMLInputElement>("r-sat");
  const pq = $<HTMLCanvasElement>("hdr-pq");
  const gamut = $<HTMLCanvasElement>("hdr-gamut");
  if (!rBlack || !rPeak || !rFull || !rSat || !pq || !gamut) return;

  let size = { w: 0, h: 0, dpr: 1 };
  const m = { l: 50, r: 12, t: 10, b: 32 };
  const diagram = new CieDiagram(gamut, { fontSize: 10, lazy: true });
  const live = { black: 0, peak: 450, full: 450, sat: 0 };
  const target = { ...live };
  const tweens = {
    black: gsap.quickTo(live, "black", { duration: 0.25, ease: "power2", onUpdate: draw }),
    peak: gsap.quickTo(live, "peak", { duration: 0.25, ease: "power2", onUpdate: draw }),
    full: gsap.quickTo(live, "full", { duration: 0.25, ease: "power2", onUpdate: draw }),
    sat: gsap.quickTo(live, "sat", { duration: 0.25, ease: "power2", onUpdate: draw }),
  };

  const fill = (el: HTMLInputElement) => {
    const min = Number(el.min);
    const max = Number(el.max);
    el.style.setProperty("--p", `${((Number(el.value) - min) / (max - min)) * 100}%`);
  };

  function read(reduce: boolean) {
    target.black = blackOf(Number(rBlack!.value));
    target.peak = peakOf(Number(rPeak!.value));
    target.full = peakOf(Number(rFull!.value));
    target.sat = Number(rSat!.value) / 100;
    $("o-black")!.textContent = `${target.black.toFixed(2)} nits`;
    $("o-peak")!.textContent = `${Math.round(target.peak)} nits`;
    $("o-full")!.textContent = `${Math.round(target.full)} nits`;
    $("o-sat")!.textContent = `${Math.round(target.sat * 100)}%`;
    rPeak!.setAttribute("aria-valuetext", `${Math.round(target.peak)} nits`);
    rBlack!.setAttribute("aria-valuetext", `${target.black.toFixed(2)} nits`);
    rFull!.setAttribute("aria-valuetext", `${Math.round(target.full)} nits`);
    rSat!.setAttribute("aria-valuetext", `${Math.round(target.sat * 100)}%`);
    [rBlack!, rPeak!, rFull!, rSat!].forEach(fill);
    if (reduce) {
      Object.assign(live, target);
      draw();
    } else {
      tweens.black(target.black);
      tweens.peak(target.peak);
      tweens.full(target.full);
      tweens.sat(target.sat);
    }
  }

  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  [rBlack, rPeak, rFull, rSat].forEach((el) => el.addEventListener("input", () => read(reduce)));
  // initial values: 0 / 450 / 450 / 0 (the author's calibration)
  rFull.value = rPeak.value;
  read(true);

  // ---- PQ chart ----
  function rolloff(L: number, peak: number) {
    const knee = peak * 0.6;
    if (L <= knee) return L;
    const r = peak - knee;
    return knee + r * (1 - Math.exp(-(L - knee) / r));
  }

  function drawPq() {
    const ctx = pq!.getContext("2d")!;
    const { w, h, dpr } = size;
    if (!w) return;
    const p = palette();
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    const X = (e: number) => m.l + e * (w - m.l - m.r);
    const lg = (n: number) => Math.log10(Math.min(Math.max(n, YMIN), YMAX));
    const Y = (n: number) => h - m.b - ((lg(n) - lg(YMIN)) / (lg(YMAX) - lg(YMIN))) * (h - m.t - m.b);

    ctx.font = MONO.replace("11px", "10px");
    ctx.fillStyle = p.dim;
    ctx.strokeStyle = p.rule;
    ctx.lineWidth = 1;
    ctx.textBaseline = "middle";
    ctx.textAlign = "right";
    ctx.beginPath();
    for (let d = -2; d <= 4; d++) {
      const v = Math.pow(10, d);
      const y = Math.round(Y(v)) + 0.5;
      ctx.moveTo(m.l, y);
      ctx.lineTo(w - m.r, y);
    }
    for (let i = 0; i <= 4; i++) {
      const x = Math.round(X(i / 4)) + 0.5;
      ctx.moveTo(x, m.t);
      ctx.lineTo(x, h - m.b);
    }
    ctx.stroke();
    for (let d = -2; d <= 4; d++) ctx.fillText(Math.pow(10, d).toLocaleString("en-US", { maximumFractionDigits: 2 }), m.l - 8, Y(Math.pow(10, d)));
    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    for (let i = 0; i <= 4; i += 1) ctx.fillText((i / 4).toFixed(2).replace(/0$/, "").replace(/\.$/, ""), X(i / 4), h - m.b + 8);
    ctx.textAlign = "right";
    ctx.fillText("Input signal", w - m.r, h - 12);
    ctx.textAlign = "left";
    ctx.textBaseline = "alphabetic";
    ctx.fillText("Nits", m.l + 6, m.t + 12);

    // full-frame band
    const yf = Y(live.full);
    ctx.fillStyle = p.ink;
    ctx.globalAlpha = 0.16;
    ctx.fillRect(m.l, yf - 3, w - m.l - m.r, 6);
    ctx.globalAlpha = 1;
    ctx.fillStyle = p.dim;
    ctx.textAlign = "left";
    ctx.fillText(`Full-frame ${Math.round(live.full)} nits`, m.l + 8, yf + 17);

    const N = 240;
    const curve = (f: (L: number) => number) => {
      ctx.beginPath();
      let started = false;
      for (let i = 0; i <= N; i++) {
        const e = i / N;
        const L = pqToNits(e);
        if (L < YMIN * 0.5 && !started) continue;
        const y = Y(f(L));
        if (!started) {
          ctx.moveTo(X(e), y);
          started = true;
        } else ctx.lineTo(X(e), y);
      }
    };
    ctx.setLineDash([5, 4]);
    ctx.strokeStyle = p.dim;
    ctx.lineWidth = 1.25;
    curve((L) => L);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.strokeStyle = p.ink;
    ctx.lineWidth = 2;
    ctx.lineJoin = "round";
    curve((L) => Math.max(rolloff(L, live.peak), live.black));
    ctx.stroke();
    // peak marker
    ctx.fillStyle = p.ink;
    ctx.textAlign = "right";
    ctx.textBaseline = "alphabetic";
    ctx.fillText(`Peak ${fmtNits(live.peak)} nits`, w - m.r - 4, Y(live.peak) - 8);
  }

  function drawGamut() {
    const prims = saturatedPrims(live.sat, BT709, BT2020);
    diagram.tris = [
      { prims: { ...BT709 }, color: "dim", width: 1, dots: false },
      { prims: { ...BT2020 }, color: "dim", width: 1, dash: [4, 4], dots: false },
      { prims, color: "ink", width: 2, white: true },
    ];
    diagram.requestDraw();
  }

  function draw() {
    drawPq();
    drawGamut();
  }

  const layout = () => {
    size = fitCanvas(pq!);
    draw();
  };
  layout();
  new ResizeObserver(debounce(layout, 120)).observe(pq);
  onTheme(draw);
  document.fonts.ready.then(draw);
}
