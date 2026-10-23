// CIE 1931 xy diagram on Canvas 2D. The horseshoe is filled once per size into an offscreen canvas at half the
// device resolution (every pixel: xy -> XYZ with Y = 1 -> sRGB, normalised to its brightest channel), a few
// rows per idle callback so no main-thread task runs long, and drawn scaled up and dimmed so it reads as lit
// from behind. Triangles, ticks and labels are drawn live on top at full resolution.
import { SPECTRAL_LOCUS } from "../data/cie1931.ts";
import { fillHorseshoeRows, locusNormal, LOCUS_POLY, tri } from "../data/color.ts";
import type { Prims } from "../data/spaces.ts";
import { fitCanvas, palette, pending, whenIdle, isStatic, MONO, onTheme, debounce } from "./env.ts";

export interface TriSpec {
  prims: Prims;
  color: "ink" | "dim";
  width?: number;
  dash?: number[];
  /** 0..1, how much of the outline is drawn (starting at red). */
  progress?: number;
  alpha?: number;
  dots?: boolean;
  white?: boolean;
}

export interface CieLabel {
  title: string;
  sub: string;
  alpha: number;
}

export interface CieOptions {
  /** wavelength ticks and labels */
  wavelengths?: boolean;
  /** gridlines and numeric axis labels */
  axes?: boolean;
  /** limit the drawn width to what is on screen when the canvas bleeds past the viewport */
  clipToViewport?: boolean;
  fontSize?: number;
  /** wait until the canvas is near the viewport before filling the horseshoe (not in static mode) */
  lazy?: boolean;
}

/** Longest stretch of fill work in one idle callback, in ms. Idle callbacks last at most 50 ms. */
const CHUNK_MS = 8;

const XMAX = 0.8;
const YMAX = 0.9;

export class CieDiagram {
  tris: TriSpec[] = [];
  labels: CieLabel[] = [];

  private ctx: CanvasRenderingContext2D;
  private off: HTMLCanvasElement | null = null;
  private offKey = "";
  private job = 0;
  private near = false;
  /** Resolves when the horseshoe of the current size is filled and drawn. */
  ready: Promise<void> = Promise.resolve();
  private w = 0;
  private h = 0;
  private dpr = 1;
  private ox = 0;
  private oy = 0;
  private scale = 1;
  private visibleW = 0;
  private raf = 0;

  constructor(
    readonly canvas: HTMLCanvasElement,
    private opts: CieOptions = {},
  ) {
    this.ctx = canvas.getContext("2d")!;
    this.near = !(opts.lazy && !isStatic);
    this.resize();
    const rs = debounce(() => this.resize(), 120);
    new ResizeObserver(rs).observe(canvas);
    if (opts.lazy && !isStatic) {
      const io = new IntersectionObserver(
        (es) => {
          if (!es.some((e) => e.isIntersecting)) return;
          io.disconnect();
          this.near = true;
          this.makeOffscreen();
        },
        { rootMargin: "600px 0px" },
      );
      io.observe(canvas);
    }
    onTheme(() => this.requestDraw());
  }

  resize() {
    const { w, h, dpr } = fitCanvas(this.canvas);
    if (w === 0 || h === 0) return;
    this.w = w;
    this.h = h;
    this.dpr = dpr;
    const rect = this.canvas.getBoundingClientRect();
    const vw = document.documentElement.clientWidth;
    this.visibleW = this.opts.clipToViewport ? Math.max(120, Math.min(w, vw - rect.left)) : w;
    const small = !this.opts.axes;
    const padL = small ? 6 : 34;
    const padB = small ? 6 : 26;
    const padT = small ? 6 : 16;
    const padR = small ? 6 : 10;
    const sx = (this.visibleW - padL - padR) / XMAX;
    const sy = (h - padT - padB) / YMAX;
    this.scale = Math.min(sx, sy);
    this.ox = padL;
    this.oy = h - padB;
    this.makeOffscreen();
    this.requestDraw();
  }

  private makeOffscreen() {
    if (!this.near) return;
    const key = `${this.w}x${this.h}x${this.dpr}`;
    if (key === this.offKey && this.off) return;
    this.offKey = key;
    const job = ++this.job;
    // Half the device resolution (the fill is a smooth gradient), drawn back up with smoothing.
    const ow = Math.max(1, Math.round((this.w * this.dpr) / 2));
    const oh = Math.max(1, Math.round((this.h * this.dpr) / 2));
    const view = { x0: -this.ox / this.scale, y0: this.oy / this.scale, scale: (this.scale * ow) / this.w };
    const c = document.createElement("canvas");
    c.width = ow;
    c.height = oh;
    const octx = c.getContext("2d")!;
    const data = new Uint8ClampedArray(ow * oh * 4);
    const image = new ImageData(data as unknown as Uint8ClampedArray<ArrayBuffer>, ow, oh);
    let row = 0;
    let lastDraw = 0;
    this.ready = new Promise<void>((done) => {
      const step = () => {
        if (job !== this.job) return done();
        const t0 = performance.now();
        const from = row;
        while (row < oh && performance.now() - t0 < CHUNK_MS) {
          const to = Math.min(oh, row + 8);
          fillHorseshoeRows(data, ow, view, row, to);
          row = to;
        }
        octx.putImageData(image, 0, 0, 0, from, ow, row - from);
        this.off = c;
        // show progress now and then, not after every chunk (a full redraw costs more than a chunk)
        if (row >= oh || performance.now() - lastDraw > 150) {
          lastDraw = performance.now();
          this.requestDraw();
        }
        if (row < oh) whenIdle(step);
        else {
          // resolve after the frame that shows the finished fill
          requestAnimationFrame(() => requestAnimationFrame(() => done()));
        }
      };
      whenIdle(step);
    });
    pending.push(this.ready);
  }

  X(x: number) {
    return this.ox + x * this.scale;
  }
  Y(y: number) {
    return this.oy - y * this.scale;
  }

  requestDraw() {
    if (this.raf) return;
    this.raf = requestAnimationFrame(() => {
      this.raf = 0;
      this.draw();
    });
  }

  draw() {
    const { ctx, dpr } = this;
    if (!this.w) return;
    const p = palette();
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, this.w, this.h);
    ctx.save();
    ctx.beginPath();
    ctx.rect(0, 0, this.visibleW + 2, this.h);
    ctx.clip();

    if (this.opts.axes) this.drawAxes(p);

    // horseshoe fill
    if (this.off) {
      ctx.globalAlpha = p.horseshoeAlpha;
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(this.off, 0, 0, this.w, this.h);
      ctx.globalAlpha = 1;
    }
    // locus outline (closed by the purple line)
    ctx.beginPath();
    LOCUS_POLY.forEach((q, i) => (i ? ctx.lineTo(this.X(q.x), this.Y(q.y)) : ctx.moveTo(this.X(q.x), this.Y(q.y))));
    ctx.closePath();
    ctx.strokeStyle = p.dim;
    ctx.lineWidth = 1;
    ctx.setLineDash([]);
    ctx.stroke();

    for (const t of this.tris) this.drawTri(t, p);

    if (this.opts.wavelengths) this.drawWavelengths(p);
    ctx.restore();
    this.drawLabels(p);
  }

  private drawAxes(p: ReturnType<typeof palette>) {
    const { ctx } = this;
    ctx.save();
    ctx.strokeStyle = p.rule;
    ctx.lineWidth = 1;
    ctx.fillStyle = p.dim;
    ctx.font = this.opts.fontSize ? MONO.replace("11px", this.opts.fontSize + "px") : MONO;
    ctx.beginPath();
    for (let i = 0; i <= 8; i++) {
      const x = Math.round(this.X(i / 10)) + 0.5;
      ctx.moveTo(x, this.Y(0));
      ctx.lineTo(x, this.Y(YMAX));
    }
    for (let i = 0; i <= 9; i++) {
      const y = Math.round(this.Y(i / 10)) + 0.5;
      ctx.moveTo(this.X(0), y);
      ctx.lineTo(this.X(XMAX), y);
    }
    ctx.stroke();
    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    for (let i = 0; i <= 8; i += 2) if (this.X(i / 10) < this.visibleW - 14) ctx.fillText((i / 10).toFixed(1), this.X(i / 10), this.Y(0) + 7);
    ctx.textAlign = "right";
    ctx.textBaseline = "middle";
    for (let i = 0; i <= 9; i += 2) ctx.fillText((i / 10).toFixed(1), this.X(0) - 7, this.Y(i / 10));
    ctx.textAlign = "left";
    ctx.textBaseline = "alphabetic";
    ctx.fillText("x", this.X(XMAX) - 8, this.Y(0) - 8);
    // beside the axis, where the spectral tick labels (520 nm sits near the top) cannot reach it
    ctx.textAlign = "right";
    ctx.textBaseline = "middle";
    ctx.fillText("y", this.X(0) - 7, this.Y(YMAX));
    ctx.restore();
  }

  private drawTri(t: TriSpec, p: ReturnType<typeof palette>) {
    const { ctx } = this;
    const pts = tri(t.prims).map((q) => [this.X(q.x), this.Y(q.y)] as const);
    const alpha = t.alpha ?? 1;
    ctx.save();
    ctx.globalAlpha = alpha;
    ctx.strokeStyle = t.color === "ink" ? p.ink : p.dim;
    ctx.lineWidth = t.width ?? 1.5;
    ctx.lineJoin = "round";
    ctx.setLineDash(t.dash ?? []);
    const prog = t.progress ?? 1;
    if (prog >= 1) {
      ctx.beginPath();
      pts.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
      ctx.closePath();
      ctx.stroke();
    } else if (prog > 0) {
      const loop = [...pts, pts[0]!];
      const lens = loop.slice(1).map((q, i) => Math.hypot(q[0] - loop[i]![0], q[1] - loop[i]![1]));
      let left = prog * lens.reduce((a, b) => a + b, 0);
      ctx.beginPath();
      ctx.moveTo(loop[0]![0], loop[0]![1]);
      for (let i = 0; i < lens.length && left > 0; i++) {
        const f = Math.min(1, left / lens[i]!);
        const a = loop[i]!;
        const b = loop[i + 1]!;
        ctx.lineTo(a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f);
        left -= lens[i]!;
      }
      ctx.stroke();
    }
    ctx.setLineDash([]);
    if (t.dots !== false && prog >= 1) {
      ctx.fillStyle = ctx.strokeStyle;
      for (const [x, y] of pts) {
        ctx.beginPath();
        ctx.arc(x, y, 2.6, 0, Math.PI * 2);
        ctx.fill();
      }
    }
    if (t.white) {
      const wx = this.X(t.prims.w.x);
      const wy = this.Y(t.prims.w.y);
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(wx - 6, wy);
      ctx.lineTo(wx + 6, wy);
      ctx.moveTo(wx, wy - 6);
      ctx.lineTo(wx, wy + 6);
      ctx.stroke();
    }
    ctx.restore();
  }

  private drawWavelengths(p: ReturnType<typeof palette>) {
    const { ctx } = this;
    ctx.save();
    ctx.strokeStyle = p.dim;
    ctx.fillStyle = p.dim;
    ctx.lineWidth = 1;
    ctx.font = MONO.replace("500", "400").replace("11px", "10px");
    ctx.textBaseline = "middle";
    for (let nm = 460; nm <= 620; nm += 20) {
      const i = SPECTRAL_LOCUS.findIndex((r) => r[0] === nm);
      const q = LOCUS_POLY[i]!;
      const n = locusNormal(i);
      const x = this.X(q.x);
      const y = this.Y(q.y);
      ctx.beginPath();
      ctx.moveTo(x, y);
      ctx.lineTo(x + n.x * 6, y - n.y * 6);
      ctx.stroke();
      ctx.textAlign = n.x > 0.35 ? "left" : n.x < -0.35 ? "right" : "center";
      ctx.fillText(String(nm), x + n.x * 10 + (n.x > 0.35 ? 2 : n.x < -0.35 ? -2 : 0), y - n.y * 11);
    }
    ctx.restore();
  }

  private drawLabels(p: ReturnType<typeof palette>) {
    const { ctx } = this;
    ctx.save();
    ctx.textAlign = "right";
    const narrow = this.visibleW < 560;
    // Narrow screens: keep the label inside the plot's empty top-right corner, on a plate so gridlines don't
    // run through the text. Wide screens: below the locus, past the plot's right edge.
    const x = narrow ? Math.min(this.visibleW - 10, this.X(XMAX) - 8) : this.visibleW - 10;
    const y = narrow ? this.Y(0.86) : this.Y(0.115);
    const titleFont = `500 ${narrow ? 13 : 15}px "IBM Plex Mono", ui-monospace, Consolas, monospace`;
    const subFont = `400 ${narrow ? 11 : 12}px "IBM Plex Mono", ui-monospace, Consolas, monospace`;
    const gap = narrow ? 17 : 20;
    for (const l of this.labels) {
      if (l.alpha <= 0.01) continue;
      if (narrow) {
        ctx.font = titleFont;
        let w = ctx.measureText(l.title).width;
        ctx.font = subFont;
        w = Math.max(w, ctx.measureText(l.sub).width);
        ctx.globalAlpha = 1;
        ctx.fillStyle = p.bg;
        ctx.fillRect(x - w - 6, y - 15, w + 12, gap + 21);
      }
      ctx.globalAlpha = l.alpha;
      ctx.fillStyle = p.ink;
      ctx.font = titleFont;
      ctx.fillText(l.title, x, y);
      ctx.fillStyle = p.dim;
      ctx.font = subFont;
      ctx.fillText(l.sub, x, y + gap);
    }
    ctx.restore();
  }
}
