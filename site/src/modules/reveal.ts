// Headlines only: each character expands from a narrow width into place (a variable-width font can do that).
// Body text never animates.
import { gsap } from "gsap";
import { SplitText } from "gsap/SplitText";
import { isStatic } from "./env.ts";

export function initReveal() {
  const els = Array.from(document.querySelectorAll<HTMLElement>("[data-reveal]"));
  const done = () => document.documentElement.classList.add("ready");
  if (isStatic) {
    done();
    return;
  }
  els.forEach((el, n) => {
    const hero = el.tagName === "H1";
    const rest = hero ? 125 : 112;
    SplitText.create(el, {
      type: "lines,chars",
      linesClass: "split-line",
      charsClass: "split-char",
      autoSplit: true,
      onSplit(self) {
        return gsap.fromTo(
          self.chars,
          { y: "0.35em", opacity: 0, fontVariationSettings: '"wdth" 62' },
          {
            y: 0,
            opacity: 1,
            fontVariationSettings: `"wdth" ${rest}`,
            duration: 1.1,
            ease: "expo.out",
            stagger: 0.018,
            delay: hero ? 0.1 + n * 0.05 : 0,
            scrollTrigger: hero ? undefined : { trigger: el, start: "top 88%", once: true },
          },
        );
      },
    });
  });
  done();
}
