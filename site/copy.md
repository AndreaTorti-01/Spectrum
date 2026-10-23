# Spectrum website copy

Total prose words: 144 (budget 380; counts Headline, Hero text, Body, Caption, Callout, noscript and the footer line; excludes kickers, button and label lines, numbers, axis ticks, alt text and the gamut descriptions, which are the app's own strings)

Line types: Headline, Hero text, Body, Caption, Callout and Noscript count as prose and carry a word count. Kicker, Button, Label, Alt, Aria and Description are UI text and are not counted. Placeholders in braces ({rx}, {area}) are filled by the builder from the measurement report and monitor.json. Coverage percentages are computed from monitor.json (checked by hand: 100, 98.4, 94.9 and 78.1).

UI labels quoted here match the app verbatim: "Read from monitor", "Compare gamuts…", "Start with Windows", "Black level", "Peak brightness", "Full-frame", "Saturation", "SDR content", "Advanced", "Restore recommended HDR settings", "Open Spectrum". Gamut space names and descriptions are copied from `STANDARD_SPACES` in `src/color.rs`; if that file changes, change the strings here.

## Meta

- Meta title: "Spectrum: monitor color profiles for Windows" (44 chars)
- Meta description: "Windows tray utility that builds an ICC profile from your monitor's EDID, writes MHC2 profiles for HDR and re-applies them after driver updates." (146 chars)
- OG title: "Spectrum: your monitor already knows its colors"
- OG description: same as meta description
- Twitter card title: same as OG title
- Noscript: "Demos need JavaScript. Screenshots follow." (5 words)

## 1. Nav

- Skip link: "Skip to content"
- Wordmark: "SPECTRUM"
- Link: "How it works" (jumps to section 3, main window)
- Link: "GitHub"
- Button: "Download"
- Icons: monitor, sun, moon (theme toggle states, no visible text)
- Aria: "Theme: System" / "Theme: Light" / "Theme: Dark" (toggle), "Spectrum home" (wordmark link)

## 2. Hero

- Kicker: "Windows 11 · MIT"
- Headline: "Your monitor already knows its colors." (6 words)
- Hero text: "Your monitor can already report to Windows which colors it can show. Spectrum turns that into a color profile and keeps it applied. If you use your monitor's color space emulation modes, Spectrum allows you to change your color profile with just one click, from your system tray. Easily enable, disable and manage HDR, too! All this, open source and for free, today." (64 words; the owner's official description, also at the top of the README, used verbatim)
- Button: "Download for Windows"
- Button (ghost): "View on GitHub"
- Canvas labels (mono): "sRGB" · "Display P3" · "Adobe RGB (1998)" · "QD-OLED 27" · "{area}% of CIE 1931 xy" (area computed live)
- Canvas axis labels: wavelength ticks every 20 nm, "x" and "y"
- Aria: "CIE 1931 xy chromaticity diagram. The gamut triangle morphs from sRGB through Display P3 and Adobe RGB (1998) to the author's monitor, a QD-OLED 27."

## 3. Main window

- Headline: none (the screenshot and callouts carry the section)
- Body: none
- Callout 1: "Identify shows each monitor's number on its screen" (8 words)
- Callout 2: "The SDR profile applies on selection" (6 words)
- Callout 3: "HDR changes apply when you let go of a slider" (10 words)
- Alt (light): "Spectrum main window, light theme. A map of the connected monitors, an SDR profile dropdown, Read from monitor and Compare gamuts… buttons, an HDR toggle with four sliders, and a Start with Windows checkbox."
- Alt (dark): "Spectrum main window, dark theme. A map of the connected monitors, an SDR profile dropdown, Read from monitor and Compare gamuts… buttons, an HDR toggle with four sliders, and a Start with Windows checkbox."

## 4. HDR lab

- Kicker: none
- Headline: "HDR tuning, done right." (4 words)
- Body: none
- Caption: none. The gamut chart sits to the right of the PQ chart.
- Label: "Black level" (unit "nits", range 0–0.5)
- Label: "Peak brightness" (unit "nits", range 100–4000)
- Label: "Full-frame" (unit "nits", range 100–4000, stops at peak)
- Label: "Saturation" (unit "%", range 0–100)
- Label (chart 1 title): "PQ (ST 2084) response"
- Label (chart 1 legend): "Reference" · "Your display" · "Full-frame"
- Label (chart 1 axes): "Input signal" · "Nits"
- Label (chart 2 title): "Gamut"
- Label (chart 2 legend): "BT.709" · "BT.2020" · "Result"
- Aria (sliders group): "HDR sliders"
- Aria (chart 1): "Line chart of the PQ response from 0 to 10,000 nits, with the reference curve dashed and your display's curve clipped at peak brightness."
- Aria (chart 2): "CIE 1931 xy diagram with the BT.709 triangle growing toward BT.2020 as saturation rises."

## 5. Gamut

- Kicker: "Gamut"
- Headline: "Compare gamuts against reference spaces." (5 words)
- Body: none
- Caption: "Computed from a QD-OLED monitor's EDID." (6 words)
- Spec row 1 label: "sRGB" · "100%"
- Description (shown on hover or focus, as in the app): "The web and Windows default, with the same primaries as Rec. 709 (HD video)."
- Spec row 2 label: "Display P3" · "98.4%"
- Description: "DCI-P3 primaries with a D65 white and sRGB curve, used by Apple displays and many OLED monitors."
- Spec row 3 label: "Adobe RGB (1998)" · "94.9%"
- Description: "Print and photography space with wider greens and cyans than sRGB and the same red and blue."
- Spec row 4 label: "Rec. 2020" · "78.1%"
- Description: "Gamut of UHD and HDR video, with primaries at 630, 532 and 467 nm."
- Alt (light): "Spectrum gamut window, light theme. A CIE 1931 xy diagram with gamut triangles, and a checklist of color spaces with coverage figures."
- Alt (dark): "Spectrum gamut window, dark theme. A CIE 1931 xy diagram with gamut triangles, and a checklist of color spaces with coverage figures."

## 6. Tray

- Headline: "Monitor color profile switching, just one click away." (8 words; the owner's wording)
- Body: none
- Caption: "Spectrum lives in your system tray." (6 words)
- Label (taskbar mock): "Spectrum" (tray icon tooltip)
- Alt (light): "Spectrum tray menu, light theme. For each monitor, a radio list of SDR profiles (QD-OLED 27 (EDID), sRGB, Display P3, Adobe RGB, ProPhoto RGB, Rec. 2020, DCI-P3), an HDR check item, then Open Spectrum and Exit."
- Alt (dark): "Spectrum tray menu, dark theme. For each monitor, a radio list of SDR profiles (QD-OLED 27 (EDID), sRGB, Display P3, Adobe RGB, ProPhoto RGB, Rec. 2020, DCI-P3), an HDR check item, then Open Spectrum and Exit."

## 7. Footer

- Text: "© 2026 Andrea Torti"
- Link: "MIT license"
- Link: "GitHub"
- Footer line: "Built with Win32, GDI and a stubborn refusal to ship Electron." (11 words)
