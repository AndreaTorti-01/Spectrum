// Glass slabs: screenshots that tilt toward the cursor, catch a moving specular highlight and split a faint
// spectrum along their edge. CSS 3D transforms keep the screenshot text pixel-sharp (no WebGL resampling).
import { gsap } from "gsap";
import { finePointer, isStatic } from "./env.ts";

export function initSlabs() {
  if (isStatic) return;
  document.querySelectorAll<HTMLElement>("[data-slab]").forEach(setup);
}

function setup(slab: HTMLElement) {
  const sway = slab.querySelector<HTMLElement>(".slab-sway")!;
  const tilt = slab.querySelector<HTMLElement>(".slab-tilt")!;
  const edge = slab.querySelector<HTMLElement>(".slab-edge")!;
  const caustic = slab.querySelector<HTMLElement>(".slab-caustic");
  const maxTilt = Number(slab.dataset.maxTilt) || 9;

  // Entry: the slab rises into place, scrubbed by scroll.
  if (slab.dataset.entry !== "none") {
    gsap.fromTo(
      slab,
      { rotationX: 24, y: 80, opacity: 0, transformPerspective: 1400, transformOrigin: "50% 100%" },
      { rotationX: 0, y: 0, opacity: 1, ease: "none", scrollTrigger: { trigger: slab, start: "top 85%", end: "top 35%", scrub: true } },
    );
  }

  // Idle: slow sway, only while on screen.
  const idle = gsap.fromTo(sway, { rotationY: -4 }, { rotationY: 4, duration: 6, ease: "sine.inOut", yoyo: true, repeat: -1, paused: true });
  new IntersectionObserver((es) => es.forEach((e) => (e.isIntersecting ? idle.play() : idle.pause()))).observe(slab);

  if (!finePointer) return;

  const section = slab.closest("section") ?? slab;
  const ry = gsap.quickTo(tilt, "rotationY", { duration: 0.8, ease: "power3" });
  const rx = gsap.quickTo(tilt, "rotationX", { duration: 0.8, ease: "power3" });
  const rz = gsap.quickTo(tilt, "z", { duration: 0.8, ease: "power3" });
  const cx = caustic ? gsap.quickTo(caustic, "x", { duration: 0.8, ease: "power3" }) : null;
  const spec = { mx: 0, my: 0 };
  const apply = () => {
    tilt.style.setProperty("--mx", spec.mx.toFixed(1) + "px");
    tilt.style.setProperty("--my", spec.my.toFixed(1) + "px");
  };
  const qx = gsap.quickTo(spec, "mx", { duration: 0.6, ease: "power3", onUpdate: apply });
  const qy = gsap.quickTo(spec, "my", { duration: 0.6, ease: "power3", onUpdate: apply });
  const edgeOpacity = gsap.quickTo(edge, "opacity", { duration: 0.5, ease: "power2" });

  section.addEventListener("pointermove", (e) => {
    const pe = e as PointerEvent;
    if (pe.pointerType === "touch") return;
    const nx = Math.max(-1, Math.min(1, (pe.clientX / innerWidth) * 2 - 1));
    const ny = Math.max(-1, Math.min(1, (pe.clientY / innerHeight) * 2 - 1));
    const r = slab.getBoundingClientRect();
    ry(nx * maxTilt);
    rx(-ny * maxTilt * (7 / 9));
    rz(30);
    qx(pe.clientX - r.left);
    qy(pe.clientY - r.top);
    cx?.(-nx * 24);
    edge.style.setProperty("--angle", ((Math.atan2(ny, nx) * 180) / Math.PI + 90).toFixed(1) + "deg");
    edgeOpacity(0.55);
    tilt.style.setProperty("--shy", (40 + Math.abs(ny) * 24).toFixed(0) + "px");
  });
  section.addEventListener("pointerleave", () => {
    ry(0);
    rx(0);
    rz(0);
    cx?.(0);
    edgeOpacity(0.2);
    tilt.style.setProperty("--shy", "40px");
  });
}
