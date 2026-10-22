// Headlines only: each character expands from a narrow width into place (a variable-width font can do that).
// Body text never animates. Splitting a headline costs a layout, so only the H1 is split at load; every H2 is
// split in an idle callback shortly before it scrolls into view and plays when it enters.
import { gsap } from "gsap";
import { SplitText } from "gsap/SplitText";
import { isStatic, whenIdle } from "./env.ts";

function split(el: HTMLElement, hero: boolean, delay: number): gsap.core.Timeline {
  const rest = hero ? 125 : 112;
  // px, not em: an em offset makes GSAP measure every character separately
  const rise = parseFloat(getComputedStyle(el).fontSize) * 0.35;
  const s = SplitText.create(el, { type: "words,chars", wordsClass: "split-word", charsClass: "split-char" });
  // Characters rise and fade in one after another (transform and opacity only). The width axis expands per word:
  // a variable-font change re-shapes text and lays it out again every frame, so it runs on a few words at a time
  // instead of on every character.
  const tween = gsap.timeline({ paused: !hero, delay });
  tween.fromTo(s.chars, { y: rise, opacity: 0 }, { y: 0, opacity: 1, duration: 0.7, ease: "expo.out", stagger: 0.02 }, 0);
  tween.fromTo(s.words, { fontVariationSettings: '"wdth" 62' }, { fontVariationSettings: `"wdth" ${rest}`, duration: 0.6, ease: "expo.out", stagger: 0.12 }, 0);
  el.classList.remove("reveal-wait");
  return tween;
}

export function initReveal() {
  const els = Array.from(document.querySelectorAll<HTMLElement>("[data-reveal]"));
  const done = () => document.documentElement.classList.add("ready");
  if (isStatic) {
    done();
    return;
  }
  els.forEach((el) => el.classList.add("reveal-wait"));
  done();
  els.forEach((el, n) => {
    if (el.tagName === "H1") {
      split(el, true, 0.1 + n * 0.05);
      return;
    }
    const near = new IntersectionObserver(
      (es) => {
        if (!es.some((e) => e.isIntersecting)) return;
        near.disconnect();
        whenIdle(() => {
          const tween = split(el, false, 0);
          const enter = new IntersectionObserver(
            (es2) => {
              if (!es2.some((e) => e.isIntersecting)) return;
              enter.disconnect();
              tween.play();
            },
            { rootMargin: "0px 0px -12% 0px" },
          );
          enter.observe(el);
        });
      },
      { rootMargin: "0px 0px 150% 0px" },
    );
    near.observe(el);
  });
}
