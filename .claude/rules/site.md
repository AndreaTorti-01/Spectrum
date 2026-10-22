---
paths:
  - "site/**"
---

# Website rules

Pictures pull the visitor in and text captions them. Copy follows `.claude/rules/copy.md`; the final strings live in `site/copy.md` and are used verbatim.

## Concept

"The page is a color instrument": a darkroom with one light source. Every color is physically meaningful (spectral wavelengths, gamut triangles, the app's monitor accents). The window screenshots are the heroes and behave like glass slabs in a beam of light.

## Stack

- Vite 6 + TypeScript, no UI framework. `base: './'` so it works under `/<repo>/` on GitHub Pages. Build output is `site/dist`.
- GSAP (ScrollTrigger, SplitText) and Lenis, synced through `lenis.on('scroll', ScrollTrigger.update)` and `gsap.ticker`. No three.js: slabs use CSS 3D transforms so screenshot text stays sharp. The hero diagram is Canvas 2D. Horseshoe fills are rendered at half resolution into an offscreen canvas in `requestIdleCallback` chunks (8 ms each); the EDID and HDR diagrams fill only when near the viewport. Init steps in `main.ts` run in separate tasks. Keep main-thread tasks short: check with `npm run shots -- --longtasks`.
- Fonts: `@fontsource-variable/archivo` (has the `wdth` axis) and `@fontsource/ibm-plex-mono`, self-hosted. No Inter.
- `scripts/images.mjs` (sharp) turns `src/shots/*.png` into AVIF, WebP and PNG under `public/shots/`. `npm run build` runs it first.
- Budgets: JS at most 110 KB gzipped, CSS at most 20 KB gzipped, LCP image at most 180 KB.
- Every URL (repo, releases, license, canonical, og:url, og:image, twitter:image) comes from `src/data/site.ts`; `build/html.ts` fills `%REPO%`, `%RELEASES%`, `%LICENSE%` and `%SITE_URL%` in `index.html`. Change URLs only in `site.ts`.
- Pages deploy through `.github/workflows/pages.yml`, which is `workflow_dispatch` only. Do not add other triggers.

## Tokens (`src/styles/tokens.css`)

- Dark: `--bg #121110`, `--bg-raise #1A1917`, `--ink #ECE7DF`, `--ink-dim #A39D94`, `--rule #2E2B27`.
- Light: `--bg #F2EEE7`, `--bg-raise #E9E4DB`, `--ink #161412`, `--ink-dim #5E5850`, `--rule #D3CCC1`.
- Light horseshoe alpha is 0.40 (`--horseshoe-alpha`), dark 0.36: lit from behind, not neon.
- Monitor accents, same order as the app overlay: `#FF5A3C #2EC4B6 #FFC93C #4D9DE0 #9BD94A #F28AB2`.
- Display type: Archivo wdth 125, weight 800. Labels and numbers: IBM Plex Mono 500, uppercase. Body line length at most 62ch.
- Corners: 2px on buttons and inputs, 10px only on window screenshots. Buttons are solid `--ink` with `--bg` text.

## Behaviors

- Glass slab (`glassSlab.ts`): pointer-driven 3D tilt (about 9° and 7°), a moving specular highlight (`screen` blend at 14% in dark, `overlay` in light; `--spec-*` tokens), a conic spectral border and a blurred spectral caustic under the slab. Idle sway without a pointer. Reduced motion shows a flat image.
- Reveal (`reveal.ts`): only H1 and H2 animate. Characters rise and fade in (transform and opacity only); the Archivo `wdth` axis expands per word, because re-shaping variable-font text every frame is what makes long tasks. Only the H1 is split at load, each H2 in an idle callback shortly before it scrolls in. Body text never animates.
- Spectral bar: a canvas computed per pixel from the CIE 1931 table. It is the section divider and the nav progress line.
- Motion only where it explains something. `prefers-reduced-motion` turns off Lenis and pins and shows static slabs.

## Themes

- Default follows the system (`prefers-color-scheme`). The nav has a 3-state toggle (System, Light, Dark) stored in localStorage under `spectrum-theme`. The resolved theme is `data-theme` on `<html>`. `?theme=light|dark` overrides it for screenshots.
- Every screenshot exists in both themes and each slab holds both pictures. CSS shows the one matching the resolved theme.
- The tray menu is drawn by Windows and `--debug-scale` does not change it, so `traymenu-<theme>.png` is a 1x capture. The site shows it at its natural 220 px width (`--menu-w`) and never larger; do not upscale it.
- Screenshots are the 2× client-area captures (`<surface>-<theme>.png`). A CSS title strip draws the title bar, because the OS title bar does not scale. The `*-1x.png` files keep the OS title bar and are for the README only. Never mix them in.

## Anti-AI look

- No purple or indigo gradients, gradient blobs, decorative glows, glassmorphism blur panels, centered hero, pill badge, three-equal-cards row, emoji, icon-library icons, logo bar or testimonials.
- No invented numbers. Sizes, RAM and coverage come from measurement and `monitor.json`.
- Swap test: if the logo could be replaced by a competitor's and nobody would notice, it fails.
- Semantic landmarks, AA contrast in both themes, 2px `--ink` focus outline, alt text on every screenshot, `<noscript>` shows static images.

## Verify

```
cd site; npm run build
npm run shots -- --out <dir>       # all sections, 1440x900 and 390x844 (DPR 2), light and dark
```

- `npm run shots` (`scripts/shots.mjs`, no dependencies) starts `vite preview` if nothing listens on 4173, launches headless Edge with `--remote-debugging-port` and drives it over CDP with Node's global WebSocket. It emulates the device metrics (a plain `msedge --headless --screenshot --window-size=390,...` is laid out at about 500 px and captures before the page is ready, so do not use it), loads `?shot=<id>&theme=<t>`, waits for fonts and `window.__shotReady`, and writes `<id>-<width>-<theme>.png` (default `../screenshots-raw/site`, gitignored). Use the scratchpad for checks.
- Options: `--ids hero,drivers` (ids are the `<section id>` values in `index.html`, or `all`), `--sizes d,m`, `--themes dark`, `--reduced` (prefers-reduced-motion, files get `-rm`), `--nojs` (`-nojs`), `--hover window` (animated page, pointer over that slab, `-hover`), `--widths 320,360` (reports the nav edge), `--longtasks` (4x CPU throttle, prints the longest main-thread tasks from a DevTools trace and the long-task observer).
- `?shot=<id>` freezes the page in its final static state at one section. Open every PNG and look at it: both themes, both sizes, reduced motion, no JS, one hover capture. A clean build is not enough. Also check console errors and the budgets.
