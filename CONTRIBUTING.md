# Contributing

## Set up

Spectrum builds with the GNU toolchain. You don't need Visual Studio.

```
rustup default stable-x86_64-pc-windows-gnu
```

## Commands

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

The website lives in `site/`: `npm ci`, then `npm run build`.

## Rules

- Prefer the `--debug-*` switches over driving the UI for functional checks: they hit the same code path and are repeatable. Driving the UI is fine when you're exploring. Back up your color settings (`--debug-dump`) before any check that changes them.
- UI changes are easiest to review with screenshots in the light and dark themes.
- Text you write (UI strings, docs, templates, commit messages) reads best when it follows `.claude/rules/copy.md`.
- Windows API bindings come from `windows-sys`. Open an issue before adding other crates; the single dependency is deliberate.
- Nothing should need admin rights, so the install stays per-user.

## License

By contributing you agree your work is released under the MIT license.
