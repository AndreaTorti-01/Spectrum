---
name: release
description: Ship a new Spectrum version - bump the version, tag it, let the Release workflow publish the installer, and redeploy the GitHub Pages site. Use when the owner asks to ship, release, publish or tag a version, or to redeploy the website.
---

# Release

Releasing publishes to GitHub, so do it when the owner asks for it, and stop to ask if something looks off
(a failing check, an unexpected change in `git status`, a run that fails).

Repo: `https://github.com/AndreaTorti-01/Spectrum`. Site: `https://andreatorti-01.github.io/Spectrum/`.

## 1. Check the tree

```
git status -sb                      # on main, nothing unexpected
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
cd site; npm run build
```

If the UI or the site visuals changed since the last screenshots, refresh them first with the `screenshots`
skill (app shots, README demo, and banners if the monitor data changed).

## 2. Bump the version

Versions so far are patch bumps (`0.1.x`). Change both places, then build so `Cargo.lock` follows:

- `Cargo.toml`: `version = "X.Y.Z"`
- `installer/spectrum.iss`: `#define AppVersion "X.Y.Z"`

```
cargo build --release
```

## 3. Commit, push, tag

```
git add -A
git commit -m "vX.Y.Z: <what changed>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push origin main
git tag -a vX.Y.Z -m "Spectrum X.Y.Z"
git push origin vX.Y.Z
```

Pushing the `v*` tag starts `.github/workflows/release.yml`. It builds `Spectrum.exe`, signs it through
SignPath when the `SIGNPATH_*` repo variables are set (they aren't yet, so releases go out unsigned and the
release notes say so), builds `SpectrumSetup.exe` with Inno Setup from that exe, and publishes
`SpectrumSetup.exe`, the portable `Spectrum.exe` and `SHA256SUMS.txt` as the GitHub Release.

Moving or re-pushing an existing tag rewrites a public release. Ask the owner before doing that.

## 4. Redeploy the site

`pages.yml` only runs on `workflow_dispatch` (the owner doesn't want a deploy on every push). There is no
`gh` CLI on this PC; the git credential that pushes also dispatches. Keep the token out of the output:

```
tok=$(printf 'protocol=https\nhost=github.com\n\n' | git credential fill 2>/dev/null | sed -n 's/^password=//p'); curl -s -o /dev/null -w "%{http_code}\n" -X POST -H "Authorization: Bearer $tok" -H "Accept: application/vnd.github+json" https://api.github.com/repos/AndreaTorti-01/Spectrum/actions/workflows/pages.yml/dispatches -d '{"ref":"main"}'; unset tok
```

`204` means it was queued.

## 5. Wait for the runs

Poll the public API in a background shell (Bash `run_in_background`), every 30 s and for at most about
25 minutes, until both runs are `completed`:

- the `Release` run whose `head_branch` is `vX.Y.Z`
- the `Pages` run whose `head_sha` is the commit you pushed

```
curl -s "https://api.github.com/repos/AndreaTorti-01/Spectrum/actions/runs?per_page=10"
```

Parse the JSON with Node (`python3` is the Microsoft Store alias here and can hang). Then check that the
site's one-click download points at the new installer:

```
curl -s -o /dev/null -w "%{redirect_url}\n" https://github.com/AndreaTorti-01/Spectrum/releases/latest/download/SpectrumSetup.exe
```

It should redirect to `.../releases/download/vX.Y.Z/SpectrumSetup.exe`. The site's Download buttons use
this `latest/download` URL (`DOWNLOAD` in `site/src/data/site.ts`), so the asset name `SpectrumSetup.exe`
has to stay the same in every release.

## 6. Report

Tell the owner the version, both run conclusions, the release URL and what the download link resolves to.
If a run failed, read its log on the run page and report the failing step instead of retrying blindly.
