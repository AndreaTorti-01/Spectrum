---
name: screenshots
description: Take or refresh Spectrum's app screenshots, the README demo animation, the README banners or the website screenshots. Use after any UI, theme or website visual change, or when a picture in docs/ or site/ is stale or missing.
---

# Screenshots

Every surface is shot in light and dark. Windows are opened only to look at them; nothing here changes a color setting (the showcase variants run on a mock UI state).

## 1. Build

```
cargo build --release
```

`scripts/capture.ps1` reads `target\release\Spectrum.exe` (override with `-Exe`) and throws if it is missing.

## 2. Capture the app

```
# from a PowerShell session at the repo root (so -Scale 1,2 binds as an array)
.\scripts\capture.ps1 [-Theme light|dark|both] [-Scale 1,2] [-Demo] [-Out docs\screenshots] [-SiteOut site\src\shots] [-RawOut screenshots-raw] [-Exe <path>]
```

- Defaults: `-Theme both`, `-Scale 1,2`, no demo. A full run takes about a minute and owns the screen. Tell the user first.
- It stops any running Spectrum (including the user's tray copy) at the start and end of each pass. The user restarts it afterwards.
- Per theme and scale it starts `--debug-stage <theme>`, then `--tray --theme <theme>` (plus `--debug-scale <f>` for scales other than 1), then opens `gamut`, `traymenu` (1x pass only) and `main` through `--debug-open gamut-showcase|traymenu-showcase|main-hdr`.
- Files, per theme `<t>`:
  - Scale 1: `main-<t>-1x.png`, `gamut-<t>-1x.png`, `hdr-<t>-1x.png` (full window with the OS title bar), plus `traymenu-<t>.png` and `traymenu-<t>-1x.png`.
  - Other scales (the 2x pass): `main-<t>.png`, `gamut-<t>.png`, `hdr-<t>.png`, client area only, because the OS title bar does not scale. The site draws a CSS title strip over them.
  - `hdr-<t>*.png` is the HDR section cropped out of the main window.
- Everything lands in `docs\screenshots`, then the script copies the same files to `site\src\shots`.
- Which file goes where: the README uses the `*-1x.png` files and `traymenu-<t>.png` (GitHub shows images at pixel size). The site uses only the plain names, never the `-1x` files (`site/scripts/images.mjs` enforces this).

## 3. Privacy guards (keep them)

- `--debug-stage` covers the primary work area with a solid topmost backdrop, so the desktop, wallpaper and other windows cannot appear.
- The script refuses to capture a surface that is not topmost over the backdrop (the tray menu is exempt: Windows draws it).
- Captures use the exact window rectangle (DWM frame bounds, no shadow); the 1 px border ring is repainted gray so no wallpaper shows through.
- Demo frames are clipped above the taskbar, and every frame is scanned: any pixel outside the window and menu rectangles that is not the backdrop color aborts the run and writes `screenshots-raw\failed-frame.png`.
- Never screenshot the whole desktop. Any new capture code must go through the stage and keep these checks.

## 4. Demo frames and banners

- `-Demo` records the scripted `--debug-demo` (idle window, tray icon hover, tray menu, Display P3 picked, peak slider 400 to 1000 nits; mock state, about 8 s) at 10 fps into `screenshots-raw\demo-<t>\frame-NNNN.png`. The demo draws a mock taskbar on the stage, writes a layout file (`screenshots-raw\demo-<t>.layout.txt`: capture region, rectangles, start time, cursor path); the script crops to that region (about 736x783: window, menu and taskbar plus a 32 px margin) and draws the arrow cursor itself. It aborts on a blank frame or any stray pixel. `screenshots-raw` is gitignored. It only records during the 1x pass, so keep `1` in `-Scale`. To leave the committed screenshots alone, pass `-Out` and `-SiteOut` folders outside the repo.
- `cd site; npm run demo` joins the frames into the animated WebP the README embeds (`docs/demo-light.webp`, `docs/demo-dark.webp`, 2 MB or less each; runs of identical frames become one frame with a longer delay).
- `cd site; npm run banner` renders the README banners with headless Edge (`docs/banner-light.png`, `docs/banner-dark.png`, 1280×400).
- If `site/scripts/demo.mjs` or `banner.mjs` is missing, they have not been written yet; do not invent a different pipeline without asking.

## 5. Site pages

After `cd site; npm run build`:

```
cd site; npm run shots -- --out <scratchpad folder>      # all sections, 1440x900 and 390x844 at DPR 2, light and dark
npm run shots -- --ids hero,tray --sizes m --themes dark
npm run shots -- --hover window --themes dark            # slab specular, animated page
npm run shots -- --reduced | --nojs | --widths 320,360 | --longtasks
```

- `site/scripts/shots.mjs` has no dependencies: it starts `vite preview` if needed, launches headless Edge with `--remote-debugging-port` and talks CDP over Node's WebSocket. It emulates the viewport (the plain `msedge --headless --screenshot --window-size=390,844` call is laid out at about 500 px and captures before the page is ready, so it is not used), loads `?shot=<id>&theme=<t>`, waits for fonts and `window.__shotReady`, and writes `<id>-<width>-<theme>.png`. The default folder is `screenshots-raw/site` (gitignored).
- `<id>` is a `<section id>` from `site/index.html` (hero, edid, window, hdr, gamut, tray, drivers, download) or `all`.
- Write these to the scratchpad or a temp folder, not into the repo, unless the task is the social preview image.
- After changing `site/src/shots/`, run `npm run images` (or `npm run build`) to regenerate AVIF and WebP.
- The tray menu is drawn by Windows: `--debug-scale` does not change it, so `traymenu-<theme>.png` stays a 1x capture (220x200). The site shows it at 220 CSS px and never larger.

## 6. Inspect before committing

Open every new PNG with the Read tool and look at it: correct theme, nothing cut off, no desktop or taskbar content, no personal data, text not clipped, the 1x and 2x pairs match. Re-run the capture for any bad file. Commit only inspected files, and mention the theme and scale in the commit message.
