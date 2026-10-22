# Spectrum

Rust Windows tray app that builds ICC and HDR (MHC2) profiles from a monitor's EDID and applies them per monitor. Library in `src/lib.rs`, thin CLI shell in `src/main.rs`.

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

- The only dependency is `windows-sys`. `build.rs` is dependency-free too (it writes the `.rsrc` COFF object itself). No windres, no .rc files.
- No comctl controls. All UI is custom-drawn GDI in `src/ui/`.
- Never require admin. Everything is per-user.
- The app theme follows the Windows setting. `--theme light|dark` exists only for screenshots.
- Install and autostart belong to the Inno installer and a Startup-folder shortcut (`Spectrum.lnk --tray`), never to the exe. Defender ML quarantined a self-installing exe (`Program:Win32/Contebrew.A!ml`). Do not reintroduce self-copy, Run keys, an Uninstall key or cmd self-delete. The exe only has `--cleanup` and `--quit` for the uninstaller.
- Release profile is `panic = "abort"`. A panic or a `RefCell` double borrow kills the tray app. Release borrows before calling back into Win32.

## Testing

IMPORTANT: functional tests never drive the UI (no clicking, typing or dragging through scripts). Use unit tests (`tests/core.rs`, in-module tests) and the headless `--debug-*` switches, which go through the same dispatch path the windows use. Windows are opened only to look at them.

The exe is GUI-subsystem, so a plain call prints nothing. Capture output with:

```
Start-Process -Wait -WindowStyle Hidden target\release\Spectrum.exe -ArgumentList '--debug-dump','--out',"$env:TEMP\dump.txt"
```

Switches (each takes `--out <file>`; monitor numbers are the 1-based numbers in the UI):

- `--debug-dump [--json]`: monitors, EDID keys, defaults, associations. Read-only. `site/src/data/monitor.json` comes from `--json`.
- `--debug-edid <n>`: "Read profile from monitor" flow. Writes a profile into the color folder.
- `--debug-sdr <n> <option-index>`, `--debug-hdr <n> <min>,<peak>,<full>,<sat>`, `--debug-hdr-reset <n>`, `--debug-hdr-toggle <n> on|off`, `--debug-autostart on|off`: the matching UI actions, headless. They change real settings.
- `--debug-wipe <n>` removes Spectrum's associations (simulates a driver update); `--debug-reconcile` runs reconcile once.
- `--debug-tray-cmd <id>`: runs a tray menu command in the running instance.
- `--debug-open main|main-hdr|gamut|traymenu|gamut-showcase|traymenu-showcase`, `--debug-demo`, `--debug-stage`, `--debug-scale`: screenshots only. See the `screenshots` skill.

Never change the user's real color settings (default profile, associations, HDR state) without a backup and their consent. Dump first, restore after. HDR toggles flash the screen. `examples/sysprobe.rs` is a read-only probe.

## Gotchas

- `ColorProfileGetDisplayList` returns nothing. List associations with `WcsEnumColorProfiles` (`src/wcs.rs`).
- `ColorProfileAddDisplayAssociation(setAsDefault)` is ignored for an already associated profile. Use `ColorProfileSetDisplayDefaultAssociation`, then read the default back to verify (`wcs::set_default`).
- HDR profiles must mirror the byte layout of the Windows HDR Calibration app. Reference: `tests/fixtures/hdr_calibration_app.icc`, reproduced byte for byte by a test. `lumi` is full-frame nits, MHC2 holds min/peak, the MHC2 matrix is XYZ to XYZ, and colorants and white point are raw D65, not D50-adapted.
- Monitors are keyed by EDID identity (manufacturer, product, serial), not device path, so profiles survive driver updates.
- Reconcile restores Spectrum's profile when its association was wiped. It must never fight a default the user picked in Windows: if another profile is the default, update the config to match.

## Process

- Parallel agents work in git worktrees under `.claude/worktrees/` (gitignored).
- End commit messages with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` after a blank line.
- All human-readable text (UI strings, docs, commit messages, PR text, release notes) follows `.claude/rules/copy.md`. Code and comments are exempt.
- UI changes need light and dark screenshots. Use the `screenshots` skill.
- Website rules: `.claude/rules/site.md`.
- `SETUP-NOTES.txt` is the owner's private, gitignored file. Never commit it or quote it in public text.
