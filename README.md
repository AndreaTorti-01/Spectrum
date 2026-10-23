<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/banner-dark.png">
    <img src="docs/banner-light.png" alt="Spectrum: prism icon, wordmark and a gamut triangle over the CIE 1931 diagram" width="100%">
  </picture>
</p>

<p align="center">
  Your monitor can already report to Windows which colors it can show. Spectrum turns that into a color profile and keeps it applied. If you use your monitor's color space emulation modes, Spectrum allows you to change your color profile with just one click, from your system tray. Easily enable, disable and manage HDR, too! All this, open source and for free, today.
</p>

<p align="center">
  <a href="https://github.com/AndreaTorti-01/Spectrum/actions/workflows/ci.yml"><img src="https://github.com/AndreaTorti-01/Spectrum/actions/workflows/ci.yml/badge.svg" alt="CI status"></a>
  <a href="https://github.com/AndreaTorti-01/Spectrum/releases/latest"><img src="https://img.shields.io/github/v/release/AndreaTorti-01/Spectrum" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/AndreaTorti-01/Spectrum" alt="MIT license"></a>
</p>

<p align="center">
  <b><a href="https://github.com/AndreaTorti-01/Spectrum/releases/latest/download/SpectrumSetup.exe">Download SpectrumSetup.exe</a></b>
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/demo-dark.webp">
    <img src="docs/demo-light.webp" alt="Animated demo: right-click the tray icon, switch a profile, open the main window, drag an HDR slider" width="100%">
  </picture>
</p>

<table>
  <tr>
    <td width="50%" align="center" valign="middle">
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/main-dark-1x.png">
        <img src="docs/screenshots/main-light-1x.png" alt="Spectrum main window with a monitor map, SDR profile dropdown and HDR sliders" width="300">
      </picture>
    </td>
    <td width="50%" align="center" valign="middle">
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/gamut-dark-1x.png">
        <img src="docs/screenshots/gamut-light-1x.png" alt="Spectrum gamut window with gamut triangles on a CIE 1931 xy diagram and a coverage checklist" width="420">
      </picture>
    </td>
  </tr>
  <tr>
    <td width="50%" align="center" valign="middle">
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/traymenu-dark-1x.png">
        <img src="docs/screenshots/traymenu-light-1x.png" alt="Spectrum tray menu listing SDR profiles per monitor and an HDR on/off item" width="220">
      </picture>
    </td>
    <td width="50%" align="center" valign="middle">
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/hdr-dark-1x.png">
        <img src="docs/screenshots/hdr-light-1x.png" alt="Spectrum HDR section with Black level, Peak brightness, Full-frame, Saturation and SDR content sliders" width="420">
      </picture>
    </td>
  </tr>
</table>

## How it works

### EDID to ICC

Monitors can send [EDID](https://en.wikipedia.org/wiki/Extended_Display_Identification_Data) data over the display cable, containing information about their capabilities, following a request from the computer. This software can trigger such request.

### HDR profiles

The included profile copies the layout of the Windows HDR Calibration app: minimum and peak luminance, a saturation matrix that pushes BT.709 toward BT.2020, and identity LUTs.

### Driver updates

Spectrum keys each monitor by its EDID and keeps a copy of every profile it generates in `%APPDATA%\Spectrum\profiles`. At startup, on display changes and after sleep, it restores any profile a driver update dropped.

### No admin needed

Standard users can create files in the Windows color folder, and the profile association APIs have a per-user scope. Spectrum uses both. `SpectrumSetup.exe` installs to `%LOCALAPPDATA%\Programs\Spectrum` and offers a Start with Windows option, which adds a Startup-folder shortcut.

## FAQ

<details>
<summary>Does it use the network?</summary>

No, Spectrum sends no telemetry.

</details>

<details>
<summary>How do I uninstall?</summary>

Use Settings › Apps. Uninstalling removes every profile and association Spectrum created, plus its data folder. For the portable exe, run `Spectrum.exe --cleanup`, then delete the file.

</details>

## Build from source

Spectrum is Rust against the Win32 API (`windows-sys` only), with a GDI-drawn UI. The GNU toolchain brings its own linker, so Visual Studio is not needed.

```
rustup default stable-x86_64-pc-windows-gnu
cargo build --release
```

The installer needs [Inno Setup 6](https://jrsoftware.org/isinfo.php). It writes `target\installer\SpectrumSetup.exe`:

```
iscc /DExePath=..\target\release\Spectrum.exe installer\spectrum.iss
```

The website lives in `site/` and needs Node 22 or newer:

```
cd site
npm ci
npm run dev
```

The site is published by hand: Actions tab → Pages workflow → Run workflow. Set Settings → Pages → Source to GitHub Actions once, before the first run. `scripts/render.ps1` renders the screenshots off-screen.

## License

[MIT](LICENSE)
