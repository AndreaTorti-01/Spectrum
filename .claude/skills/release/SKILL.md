---
name: release
description: Ship a new Spectrum version - bump the version, merge dev into main, tag it, let the Release workflow build the MSIX package for the Microsoft Store, hand the owner the Partner Center steps, and redeploy the GitHub Pages site. Use when the owner asks to ship, release, publish or tag a version, or to redeploy the website.
---

# Release

Releasing publishes to GitHub and the Store, so do it when the owner asks for it, and stop to ask if
something looks off (a failing check, an unexpected change in `git status`, a run that fails).

Repo: `https://github.com/AndreaTorti-01/Spectrum`. Site: `https://andreatorti-01.github.io/Spectrum/`.

Branches: `dev` collects work and test builds (Partner Center package flight), `main` only gets release
merges (public Store submission), `legacy` is the frozen 0.1 line with the Inno installer. Never push to
`legacy`.

## 1. Check the tree

```
git switch dev; git status -sb       # nothing unexpected
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
powershell -ExecutionPolicy Bypass -File packaging\build-msix.ps1   # needs the Windows SDK
cd site; npm run build
```

If the UI or the site visuals changed since the last screenshots, refresh them first with the `screenshots`
skill. Before a public submission, offer the owner the WACK run that `build-msix.ps1 -Register` prints (it
needs an elevated shell, so the owner runs it).

## 2. Bump the version

Only `Cargo.toml` holds it; `build-msix.ps1` writes `X.Y.Z.0` into the manifest (the Store reserves the
fourth field). A Store update must have a higher version than the live one, and a flight build higher than
the public one.

```
cargo build --release      # Cargo.lock follows
```

## 3. Commit, merge, tag

Andrea Torti is the sole author: no `Co-Authored-By` or other attribution lines.

```
git add -A
git commit -m "vX.Y.Z: <what changed>"
git push origin dev
git switch main; git merge --ff-only dev; git push origin main
git tag -a vX.Y.Z -m "Spectrum X.Y.Z"
git push origin vX.Y.Z
git switch dev
```

The `v*` tag starts `.github/workflows/release.yml`: it builds `Spectrum-X.Y.Z.msix` (unsigned; the Store
signs it) and `store-tile.png` into the `Spectrum-msix` artifact, and creates a GitHub release with notes
and a link to the site, no binaries, not marked latest (the 0.1 line keeps `releases/latest`).

Moving or re-pushing an existing tag rewrites a public release. Ask the owner before doing that.

## 4. Store submission (the owner clicks)

Download the `Spectrum-msix` artifact from the Release run page and give the owner these steps:

1. Partner Center › Apps and games › Spectrum › Start update (or the flight, for a `dev` build).
2. Packages: upload `Spectrum-X.Y.Z.msix`, remove the old package.
3. Store listings › English: fill "What's new in this version" from the release notes.
4. Submission options: leave the `runFullTrust` justification and certification notes from
   `packaging/store-listing.md`.
5. Submit for certification.

## 5. Redeploy the site

Only when the site changed, and only once the Store listing is live (the buttons link to it). `pages.yml`
only runs on `workflow_dispatch`. There is no `gh` CLI on this PC; the git credential that pushes also
dispatches. Keep the token out of the output:

```
tok=$(printf 'protocol=https\nhost=github.com\n\n' | git credential fill 2>/dev/null | sed -n 's/^password=//p'); curl -s -o /dev/null -w "%{http_code}\n" -X POST -H "Authorization: Bearer $tok" -H "Accept: application/vnd.github+json" https://api.github.com/repos/AndreaTorti-01/Spectrum/actions/workflows/pages.yml/dispatches -d '{"ref":"main"}'; unset tok
```

`204` means it was queued.

## 6. Wait for the runs

Poll the public API in a background shell (Bash `run_in_background`), every 30 s and for at most about
25 minutes, until the runs are `completed`:

- the `Release` run whose `head_branch` is `vX.Y.Z`
- the `Pages` run whose `head_sha` is the commit you pushed, if you dispatched one

```
curl -s "https://api.github.com/repos/AndreaTorti-01/Spectrum/actions/runs?per_page=10"
```

Parse the JSON with Node (`python3` is the Microsoft Store alias here and can hang).

## 7. Report

Tell the owner the version, the run conclusions, the release URL, where the `.msix` artifact is and the
Partner Center steps left. If a run failed, read its log on the run page and report the failing step
instead of retrying blindly.
