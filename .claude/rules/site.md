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
- GSAP (ScrollTrigger, SplitText) and Lenis, synced through `lenis.on('scroll', ScrollTrigger.update)` and `gsap.ticker`. No three.js: slabs use CSS 3D transforms so screenshot text stays sharp. The hero diagram is Canvas 2D.
- Fonts: `@fontsource-variable/archivo` (has the `wdth` axis) and `@fontsource/ibm-plex-mono`, self-hosted. No Inter.
- `scripts/images.mjs` (sharp) turns `src/shots/*.png` into AVIF, WebP and PNG under `public/shots/`. `npm run build` runs it first.
- Budgets: JS at most 110 KB gzipped, CSS at most 20 KB gzipped, LCP image at most 180 KB.
- Pages deploy through `.github/workflows/pages.yml`, which is `workflow_dispatch` only. Do not add other triggers.

## Tokens (`src/styles/tokens.css`)

- Dark: `--bg #121110`, `--bg-raise #1A1917`, `--ink #ECE7DF`, `--ink-dim #A39D94`, `--rule #2E2B27`.
- Light: `--bg #F2EEE7`, `--bg-raise #E9E4DB`, `--ink #161412`, `--ink-dim #5E5850`, `--rule #D3CCC1`.
- Monitor accents, same order as the app overlay: `#FF5A3C #2EC4B6 #FFC93C #4D9DE0 #9BD94A #F28AB2`.
- Display type: Archivo wdth 125, weight 800. Labels and numbers: IBM Plex Mono 500, uppercase. Body line length at most 62ch.
- Corners: 2px on buttons and inputs, 10px only on window screenshots. Buttons are solid `--ink` with `--bg` text.

## Behaviors

- Glass slab (`glassSlab.ts`): pointer-driven 3D tilt (about 9° and 7°), a moving specular highlight, a conic spectral border and a blurred spectral caustic under the slab. Idle sway without a pointer. Reduced motion shows a flat image.
- Reveal (`reveal.ts`): only H1 and H2 animate, by characters, and the Archivo `wdth` axis expands into place. Body text never animates.
- Spectral bar: a canvas computed per pixel from the CIE 1931 table. It is the section divider and the nav progress line.
- Motion only where it explains something. `prefers-reduced-motion` turns off Lenis and pins and shows static slabs.

## Themes

- Default follows the system (`prefers-color-scheme`). The nav has a 3-state toggle (System, Light, Dark) stored in localStorage under `spectrum-theme`. The resolved theme is `data-theme` on `<html>`. `?theme=light|dark` overrides it for screenshots.
- Every screenshot exists in both themes and each slab holds both pictures. CSS shows the one matching the resolved theme.
- Screenshots are the 2× client-area captures (`<surface>-<theme>.png`). A CSS title strip draws the title bar, because the OS title bar does not scale. The `*-1x.png` files keep the OS title bar and are for the README only. Never mix them in.

## Anti-AI look

- No purple or indigo gradients, gradient blobs, decorative glows, glassmorphism blur panels, centered hero, pill badge, three-equal-cards row, emoji, icon-library icons, logo bar or testimonials.
- No invented numbers. Sizes, RAM and coverage come from measurement and `monitor.json`.
- Swap test: if the logo could be replaced by a competitor's and nobody would notice, it fails.
- Semantic landmarks, AA contrast in both themes, 2px `--ink` focus outline, alt text on every screenshot, `<noscript>` shows static images.

## Verify

```
cd site; npm run build; npm run preview      # http://127.0.0.1:4173
& "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe" --headless --disable-gpu --hide-scrollbars --virtual-time-budget=6000 --window-size=1440,900 --screenshot=$env:TEMP\hero.png "http://127.0.0.1:4173/?shot=hero&theme=dark"
```

- `?shot=<id>` freezes the page in a static state at one section (ids are the `<section id>` values in `index.html`: hero, edid, window, hdr, gamut, tray, drivers, download), or `all` for the full page. Check both themes and widths 1440×900 and 390×844.
- Open every PNG and look at it. A clean build is not enough. Also check the reduced-motion path, console errors and the budgets.
