// The spectral bar: every pixel is the colour of one wavelength (380 to 700 nm) via the CIE 1931 CMF table.
import { gsap } from "gsap";
import { wavelengthToSrgb8 } from "../data/color.ts";
import { isStatic } from "./env.ts";

const cache = new Map<number, ImageData>();

function strip(width: number): ImageData {
  let img = cache.get(width);
  if (!img) {
    img = new ImageData(width, 1);
    const px = [0, 0, 0];
    for (let i = 0; i < width; i++) {
      wavelengthToSrgb8(380 + (320 * i) / Math.max(1, width - 1), px);
      img.data.set([px[0]!, px[1]!, px[2]!, 255], i * 4);
    }
    cache.set(width, img);
  }
  return img;
}

function paint(c: HTMLCanvasElement) {
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const w = Math.max(1, Math.round(c.clientWidth * dpr));
  const h = Math.max(1, Math.round(2 * dpr));
  c.width = w;
  c.height = h;
  const ctx = c.getContext("2d")!;
  const row = document.createElement("canvas");
  row.width = w;
  row.height = 1;
  row.getContext("2d")!.putImageData(strip(w), 0, 0);
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(row, 0, 0, w, h);
}

export function initSpectralBars() {
  const bars = Array.from(document.querySelectorAll<HTMLCanvasElement>(".spectral-canvas"));
  const paintAll = () => bars.forEach(paint);
  paintAll();
  let last = innerWidth;
  addEventListener("resize", () => {
    if (innerWidth !== last) {
      last = innerWidth;
      paintAll();
    }
  });

  // Nav: the bar is revealed by scroll progress.
  const navBar = document.querySelector<HTMLCanvasElement>(".nav-bar canvas");
  if (navBar) {
    const update = () => {
      const max = document.documentElement.scrollHeight - innerHeight;
      const p = max > 0 ? Math.min(1, Math.max(0, scrollY / max)) : 0;
      navBar.style.clipPath = `inset(0 ${((1 - p) * 100).toFixed(2)}% 0 0)`;
    };
    addEventListener("scroll", update, { passive: true });
    addEventListener("resize", update);
    update();
  }

  // Dividers: a scan line sweeps across when the section is entered.
  if (isStatic) return;
  document.querySelectorAll<HTMLElement>(".spectral").forEach((el) => {
    const scan = el.querySelector<HTMLElement>(".spectral-scan");
    const canvas = el.querySelector<HTMLCanvasElement>(".spectral-canvas");
    if (!scan || !canvas) return;
    const io = new IntersectionObserver(
      (es) => {
        if (!es.some((e) => e.isIntersecting)) return;
        io.disconnect();
        gsap.fromTo(scan, { x: 0, opacity: 1 }, { x: canvas.clientWidth, duration: 1.1, ease: "power2.inOut", onComplete: () => void gsap.set(scan, { opacity: 0 }) });
      },
      { rootMargin: "0px 0px -12% 0px" },
    );
    io.observe(el);
  });
}
