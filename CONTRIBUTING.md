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

- Tests never change your real color settings. Exercise flows headlessly with the `--debug-*` switches. Don't drive the UI to test function.
- UI changes need screenshots in the light and dark themes.
- Text you write (UI strings, docs, templates, commit messages) follows `.claude/rules/copy.md`.
- Windows API bindings come from `windows-sys` only. Don't add other crates.
- Nothing may need admin rights.

## License

By contributing you agree your work is released under the MIT license.
