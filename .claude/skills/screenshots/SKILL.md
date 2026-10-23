---
name: screenshots
description: Take or refresh Spectrum's app screenshots, the README demo animation, the README banners or the website screenshots. Use after any UI, theme or website visual change, or when a picture in docs/ or site/ is stale or missing.
---

# Screenshots

Every surface is rendered in light and dark, off-screen: no window opens, nothing takes focus or draws on the owner's screen, and a running Spectrum is left alone. Nothing here changes a color setting.

## 1. Build

```
cargo build --release
```

`scripts/render.ps1` reads `target\release\Spectrum.exe` (override with `-Exe`) and throws if it is missing.

## 2. Render the app

```
# from a PowerShell session at the repo root
.\scripts\render.ps1 [-Theme light|dark|both] [-Demo] [-Out docs\screenshots] [-SiteOut site\src\shots] [-RawOut screenshots-raw] [-Exe <path>]
```

- Defaults: `-Theme both`, no demo. A full run with `-Demo` takes about 30 s.
- Each picture is one `Spectrum.exe --render <surface> --theme <t> --scale <f> [--frame] --out <file>` call (`src/render.rs`). It builds the window state from `src/showcase.rs` (fixed data from the QD-OLED EDID fixture), runs the window's own layout and `draw` into an off-screen `Canvas` and writes a PNG. The output is the same on any machine and at any display scale. Extend `showcase.rs` when the UI gains state.
- Surfaces: `main` (Advanced open, HDR on), `gamut`, `profiles`, `hdr` (the HDR section of the main window), `traymenu`, `layout` (JSON), `demo` (a folder of frames).
- Files, per theme `<t>`:
  - `main-<t>.png`, `gamut-<t>.png`, `profiles-<t>.png`, `hdr-<t>.png`: 2x client area. The site draws a CSS title strip over them.
  - `main-<t>-1x.png`, `gamut-<t>-1x.png`, `profiles-<t>-1x.png`: 1x with `--frame`, a Windows 11 frame drawn by `src/ui/frame.rs` (border, rounded corners, icon, title, caption glyphs, measured colors). `hdr-<t>-1x.png` is the 1x crop.
  - `traymenu-<t>.png` (and a copy as `traymenu-<t>-1x.png`): the 1x tray menu. The real menu is a Windows popup menu, so `src/ui/traymenu.rs` draws a replica from the same `MenuMonitor` list (spacing, radio dot, check mark, separators, bold default item, measured on the real menu).
  - `site/src/data/layout.json`: main window size, callout targets and HDR crop rows, gamut plot rectangle and data window. `site/src/data/hotspots.ts` computes the site's fractions from it.
- Everything lands in `docs\screenshots`, then the script copies the same files to `site\src\shots`.
- Which file goes where: the README uses the `*-1x.png` files and `traymenu-<t>.png` (GitHub shows images at pixel size). The site uses only the plain names, never the `-1x` files (`site/scripts/images.mjs` enforces this).

## 2b. When the main window layout changes

Nothing to update by hand: the HDR crop (`Offscreen::marks` in `src/ui/main_window.rs`, from the divider above the HDR section to halfway between the restore button and the divider below) and the site callouts (Identify button, SDR dropdown, Peak brightness value) come from `geo()`. Re-run the script, then check the `hdr-*` crops and the site's window section. If a callout should point at a different control, change `marks()` and `layout_json()` in `src/render.rs`.

## 3. Checks worth keeping

- `--render` never creates a window. New screenshot code should draw into a `Canvas` the same way, so nothing from the desktop can end up in a picture.
- Hover, pressed and focus states are off in the renders (no mouse is involved), so a picture can't pick up a stray highlight.

## 4. Demo frames and banners

- `-Demo` renders the scripted README demo (idle window, tray icon hover, tray menu, the pointer walking down to Display P3 and picking it, peak slider 400 to 1000 nits; 7.8 s) at 10 fps into `screenshots-raw\demo-<t>\frame-NNNN.png`: 78 frames of 716x843 at 1x. Each frame is composed in `src/render.rs`: the stage color, DWM-like shadows, the main window with its drawn frame (inactive while the menu is open), the mock taskbar (`src/ui/mockbar.rs`), the tray menu replica and an arrow cursor. The timeline is the `T_*` constants there. `screenshots-raw` is gitignored.
- `cd site; npm run demo` joins the frames into the animated WebP the README embeds (`docs/demo-light.webp`, `docs/demo-dark.webp`, 2 MB or less each; runs of identical frames become one frame with a longer delay).
- `cd site; npm run banner` renders the README banners with headless Edge (`docs/banner-light.png`, `docs/banner-dark.png`, 1280×400).
- To look at a real window instead (layout work, hover states), `--debug-open main-hdr|gamut-showcase|profiles-showcase|traymenu-showcase` opens it on the same showcase data.

## 5. Site pages

After `cd site; npm run build`:

```
cd site; npm run shots -- --out <scratchpad folder>      # all sections, 1440x900 and 390x844 at DPR 2, light and dark
npm run shots -- --ids hero,tray --sizes m --themes dark
npm run shots -- --hover window --themes dark            # slab specular, animated page
npm run shots -- --reduced | --nojs | --widths 320,360 | --longtasks
```

- `site/scripts/shots.mjs` has no dependencies: it starts `vite preview` if needed, launches headless Edge with `--remote-debugging-port` and talks CDP over Node's WebSocket. It emulates the viewport (the plain `msedge --headless --screenshot --window-size=390,844` call is laid out at about 500 px and captures before the page is ready, so it's a poor fit), loads `?shot=<id>&theme=<t>`, waits for fonts and `window.__shotReady`, and writes `<id>-<width>-<theme>.png`. The default folder is `screenshots-raw/site` (gitignored).
- `<id>` is a `<section id>` from `site/index.html` (hero, window, hdr, gamut, tray) or `all`.
- Prefer the scratchpad or a temp folder over the repo for these, unless the task is the social preview image.
- `npm run shots` leaves the `vite preview` server it started running on port 4173. Stopping it afterwards (`Stop-Process` on the `node` that runs `vite.js preview`) avoids an orphan that outlives the session.
- After changing `site/src/shots/`, run `npm run images` (or `npm run build`) to regenerate AVIF and WebP.
- `traymenu-<theme>.png` is 1x (about 200 px wide; its height grows with the profile list), like the real menu at 100%. The site shows it at its natural width and never larger.

## 6. Inspect before committing

Open each new PNG with the Read tool and look at it, checking for correct theme, nothing cut off, text not clipped, the 1x and 2x pairs match. Re-run the script for any bad file. Commit inspected files, and mention the theme and scale in the commit message.
