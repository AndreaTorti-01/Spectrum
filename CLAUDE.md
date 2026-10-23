# Spectrum

Rust Windows tray app that builds ICC and HDR (MHC2) profiles from a monitor's EDID and applies them per monitor. Library in `src/lib.rs`, thin CLI shell in `src/main.rs`. The showcase website is in `site/` (Vite + TypeScript).

Owner: Andrea Torti (MIT). Repo `https://github.com/AndreaTorti-01/Spectrum`, site `https://andreatorti-01.github.io/Spectrum/`. Shipping a version or redeploying the site follows the `release` skill.

## Commands

Run in PowerShell. `%USERPROFILE%\.cargo\bin` may not be on PATH in a fresh shell, so prepend it.

```
rustup default stable-x86_64-pc-windows-gnu    # once; GNU target, no Visual Studio
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release                          # also writes target\spectrum.ico
& "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" installer\spectrum.iss   # needs the release exe; output target\installer\SpectrumSetup.exe
cd site; npm ci; npm run build                 # also: dev, preview (127.0.0.1:4173), images, demo, banner
```

CI (`.github/workflows/ci.yml`) runs the same four cargo commands, then builds the installer.

## Hard rules

- The only dependency is `windows-sys`, and `build.rs` is dependency-free too (it writes the `.rsrc` COFF object itself, so there is no windres or .rc file). Keep it that way unless there's a strong reason: it keeps builds small and the GNU toolchain enough.
- UI is custom-drawn GDI in `src/ui/`. Prefer extending that over comctl controls, which would not match the theme. The design language (spacing grid, control height, type ramp, button variants, when to hide or disable) is written down and tokenized in `src/ui/design.rs`; build layouts from its tokens and draw with the `widgets.rs` components, so the windows stay consistent.
- Everything is per-user. Avoid anything that needs admin; it would break the zero-prompt install.
- The app theme follows the Windows setting. `--theme light|dark` is for screenshots.
- Install and autostart belong to the Inno installer and a Startup-folder shortcut (`Spectrum.lnk --tray`), not the exe. That is the reason to keep self-copy, Run keys, an Uninstall key and cmd self-delete out: Defender ML quarantined a self-installing exe (`Program:Win32/Contebrew.A!ml`). The exe only has `--cleanup` and `--quit` for the uninstaller.
- Release profile is `panic = "abort"`. A panic or a `RefCell` double borrow kills the tray app. Release borrows before calling back into Win32.

## Testing

Prefer unit tests (`tests/core.rs`, in-module tests) and the headless `--debug-*` switches over driving the UI for functional checks: the switches go through the same dispatch path the windows use, and they are repeatable. Scripted clicking, typing or dragging is flaky and takes over the owner's screen, but it's fine when you're exploring. Opening windows to look at them is normal.

The exe is GUI-subsystem, so a plain call prints nothing. Capture output with:

```
Start-Process -Wait -WindowStyle Hidden target\release\Spectrum.exe -ArgumentList '--debug-dump','--out',"$env:TEMP\dump.txt"
```

Switches (each takes `--out <file>`; monitor numbers are the 1-based numbers in the UI):

- `--debug-dump [--json]`: monitors, EDID keys, defaults, associations. Read-only. `site/src/data/monitor.json` comes from `--json`.
- `--debug-edid <n>`: "Read from monitor" flow. Writes a profile into the color folder.
- `--debug-sdr <n> <option-index>`, `--debug-hdr <n> <min>,<peak>,<full>,<sat>`, `--debug-hdr-reset <n>`, `--debug-hdr-toggle <n> on|off`, `--debug-autostart on|off`, `--debug-sdr-white <n> [nits]`, `--debug-delete-profile <file name>`: the matching UI actions, headless. They change real settings (delete only files you created for the test).
- `--debug-dxgi <n>`: min, max and full-frame luminance, primaries and color space that Windows reports for the monitor (also in `--debug-dump`). Read-only.
- `--debug-wipe <n>` removes Spectrum's associations (simulates a driver update); `--debug-reconcile` runs reconcile once.
- `--debug-tray-cmd <id>`: runs a tray menu command in the running instance.
- `--render main|gamut|profiles|hdr|traymenu|layout|demo --theme light|dark --scale <f> [--frame] --out <file>`: off-screen screenshots (`src/render.rs`). No window opens. See the `screenshots` skill.
- `--debug-open main|main-hdr|gamut|profiles|traymenu|gamut-showcase|profiles-showcase|traymenu-showcase`, `--debug-scale`: open the real windows to look at them.
- `--render`, `main-hdr` and `*-showcase` use `src/showcase.rs`: fixed data from the QD-OLED EDID fixture, so they look the same on any machine. Extend that module when the UI gains state.

Before changing the owner's real color settings (default profile, associations, HDR state, SDR content brightness), take a `--debug-dump` and plan to restore it afterwards; ask first if the change isn't part of the task. The recipe that worked every time: dump, run the switch with `--out`, dump again and compare, put back whatever the test changed with the matching switch (for example `--debug-sdr-white 1 <original nits>`), and confirm a final dump matches the first. HDR toggles flash the screen. `examples/sysprobe.rs` is a read-only probe.

## Gotchas

- `ColorProfileGetDisplayList` returns nothing. List associations with `WcsEnumColorProfiles` (`src/wcs.rs`).
- `ColorProfileAddDisplayAssociation(setAsDefault)` is ignored for an already associated profile. Use `ColorProfileSetDisplayDefaultAssociation`, then read the default back to verify (`wcs::set_default`).
- Windows applies a new HDR profile's min, peak and full-frame values within about a second. Verified with `--debug-hdr` and `--debug-dxgi`: 0.05/700/300 and 0/1000/600 were reported back, and a reset returned the EDID values 455.5/253.8/0. Read them through `IDXGIOutput6::GetDesc1` on a fresh factory (`src/dxgi.rs`), since DXGI caches per factory.
- HDR profiles need to mirror the byte layout of the Windows HDR Calibration app. Reference: `tests/fixtures/hdr_calibration_app.icc`, reproduced byte for byte by a test. `lumi` is full-frame nits, MHC2 holds min/peak, the MHC2 matrix is XYZ to XYZ, and colorants and white point are raw D65, not D50-adapted.
- Monitors are keyed by EDID identity (manufacturer, product, serial), not device path, so profiles survive driver updates.
- Windows' "SDR content brightness" is `nits = 80 + 4 * slider` (raw `nits * 1000 / 80`, set with the undocumented `DISPLAYCONFIG_DEVICE_INFO` type `0xFFFFFFEE`). Prefer reading it only while HDR is on: with HDR off the API reports 80 nits whatever the stored setting is (`monitors.rs`, verified on 26300).
- Reconcile restores Spectrum's profile when its association was wiped. It should not fight a default the user picked in Windows: if another profile is the default, update the config to match.

## Process

- Parallel agents work in git worktrees under `.claude/worktrees/` (gitignored).
- Commit messages usually end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` after a blank line.
- Human-readable text (UI strings, docs, commit messages, PR text, release notes) is easiest to keep consistent by following `.claude/rules/copy.md`. Code and comments are exempt.
- UI changes are best reviewed with light and dark screenshots. The `screenshots` skill covers it, including what to update when the main window layout moves.
- Website guidance: `.claude/rules/site.md`. Every visible site string is recorded in `site/copy.md`; update it with the page.
- Not yet verified on real hardware: the Inno installer's install and `--cleanup` uninstall round trip. Everything else was exercised on the owner's QD-OLED HDR monitor (Windows 11 build 26300).
- `SETUP-NOTES.txt` is the owner's private, gitignored file. Keep it out of commits and public text.
- Prefer PowerShell or Node over `python3` for quick checks: on this PC `python3` is the Microsoft Store alias, and a heredoc run through it waited without exiting until it was stopped by hand. Give any command that can wait (interpreters, UI Automation reads, servers) a `timeout`, and stop helpers you start (`npm run shots` leaves `vite preview` on port 4173) before reporting.
