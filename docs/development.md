# Development

## Toolchain

The repository pins the toolchain in `rust-toolchain.toml` (`stable`). This is needed
on machines where rustup has no default toolchain. Edition: 2024.

## Commands

```bash
cargo run            # debug build
cargo run --release  # optimized build
cargo test           # unit tests
cargo clippy --all-targets
cargo bench          # criterion benchmarks (benches/render.rs)
```

The application is a library (`src/lib.rs`) plus a thin binary (`src/main.rs`), so the
benchmarks can reach its internals through `graphene::bench`.

Note: `cargo test` and `cargo clippy` do not rebuild `target/debug/graphene`. Run
`cargo build` before launching the binary directly.

## Configuration

`config.toml` in `$XDG_CONFIG_HOME/graphene/`, else `~/.config/graphene/`. On first
launch, when the file is missing, the embedded `config.default.toml` (every setting
commented out, default values) is written there; a write failure is only logged.

Parsing (`src/config.rs`) reads the file key by key: a missing key takes its default, an
invalid value or an unknown key is logged with its line and ignored, a TOML syntax error
ignores the whole file. Settings applied at startup: font family and size, padding,
initial window size, scrollback, word separators, cursor shape. The other keys
(`language`, `tabs`, `theme`, `keybindings`, `cursor.blinking`) are read but not applied
yet.

In `config.default.toml`, settings and section headers are commented with `# ` and
documentation lines with `##`: a test uncomments the settings and checks that the result
equals `Config::default()`, so keep the file in sync with the code.

| Variable   | Default | Effect                   |
|------------|---------|--------------------------|
| `RUST_LOG` | `warn`  | Log level (`env_logger`) |

`RUST_LOG=graphene::perf=debug` logs the time of each startup step since the start of
`main` (window, glyphs ready, shell spawned, GPU instance and surface, adapter, device,
renderer ready)
and of the first frame drawn after the first PTY output.

The theme ("graphene night") is defined in `src/palette.rs`.

## Shortcuts

| Keys                          | Action                                           |
|-------------------------------|--------------------------------------------------|
| Ctrl+Shift+V, Shift+Insert    | Paste (bracketed when the app requests it)       |
| Ctrl+Shift+C                  | Reserved for copy (no-op until selection exists) |
| Shift+PageUp / Shift+PageDown | Scroll the selected block, or the timeline       |
| Shift+Home / Shift+End        | Top / bottom of the timeline                     |
| Ctrl+= / Ctrl+- / Ctrl+0      | Font size up / down / reset                      |
| Mouse wheel                   | Block under the pointer, or the timeline         |

## Tests

Unit tests live next to the code (`#[cfg(test)]` modules):

| Module            | Covers                                                      |
|-------------------|-------------------------------------------------------------|
| `palette`         | 256-color table, overrides, inverse/hidden/dim              |
| `input`           | key encoding, control codes, cursor modes, shortcuts, paste |
| `layout`          | grid size from pixels, padding, minimum size                |
| `frame`           | capture from a real `Term` fed with escape sequences        |
| `renderer::scene` | background merging, glyph instances, decorations, sRGB conversion |
| `renderer::cells` | glyph quad placement, skipped glyphs, frame resolved again after an atlas reset |
| `renderer`        | headless render read back: glyph pixels land in their cell (skipped without GPU) |
| `perf`            | timing line format, first output / first frame tracking     |

`frame` tests build a `Term<VoidListener>` and feed bytes through
`vte::ansi::Processor`, so they exercise the real emulator without a PTY or GPU.

## Benchmarks

`cargo bench` runs `benches/render.rs` (criterion) on PTY output recorded at 120x32 in
`benches/fixtures/` (btop, `cat` of source files, vim, and the `cat` output scrolled
into the history):

| Group         | Measures                                                             |
|---------------|----------------------------------------------------------------------|
| `parse`       | the fixture bytes fed into a fresh `Term`                            |
| `frame`       | `Frame::capture` + `scene::build`                                    |
| `glyphs_cold` | glyph cache lookup of every drawn cell, cache emptied first (rasterization included) |
| `glyphs_warm` | the same lookups with the cache already filled                       |
| `gpu_prepare` | upload, draw and submit on a headless device: `idle` (same frame), `changed` (blank frame then the fixture) |

`gpu_prepare` is skipped with a message when no GPU adapter is available. Run one group
with `cargo bench -- gpu_prepare`; compare two versions with
`cargo bench -- --save-baseline before`, then `cargo bench -- --baseline before`.
Re-recording the fixtures is described in `benches/fixtures/README.md`.

## Manual check on X11

The app can be driven with `xdotool` and captured with `import` (ImageMagick):

```bash
cargo build
SHELL=/bin/bash ./target/debug/graphene &
W=$(xdotool search --sync --classname graphene | head -1)
xdotool windowactivate --sync $W
xdotool type "htop"; xdotool key Return
sleep 2; import -window $W shot.png
```

Things worth checking after rendering changes: htop bottom bar (background/text
alignment), wide characters, box drawing, colors, cursor in vim.

## Releases

The CI of each pull request uploads its release binary as the `graphene-release`
artifact (kept 30 days). Every push to `develop` (so every merged pull request) then
runs `.github/workflows/release.yml`, which publishes a GitHub pre-release tagged
`v<version>`, where `<version>` is what the binary prints with `graphene --version`.

The version is computed at build time by `build.rs`: `<crate version>-dev.<UTC
YYYYMMDDHHMMSS>` by default, `<crate version>` when built with `GRAPHENE_CHANNEL=stable`.
The timestamp comes from `SOURCE_DATE_EPOCH` when set (the CI release builds set it to
the current time), else the current time.

The release does not rebuild: `scripts/find-ci-binary.sh` finds the successful CI run
of the merged pull request and the binary is downloaded from it. This only happens
when the merged commit has the same tree as the pull request head (the branch was up
to date with `develop`, which the `develop` ruleset enforces). Otherwise (direct push,
expired artifact, ...) the workflow falls back to testing and building from source.

Assets:

- `graphene-<tag>-x86_64-unknown-linux-gnu.tar.gz`: binary, desktop entry, icon, licenses
- `graphene-term_<version>_amd64.deb` (package version `<crate version>~dev.<timestamp>`)
- `graphene-term-<crate version>-0.dev.<timestamp>.x86_64.rpm`
- `graphene-<tag>-x86_64.AppImage`
- `SHA256SUMS`

The AppImage icon `packaging/graphene.png` is rendered from `graphene.svg`; regenerate it
after changing the SVG with `rsvg-convert -w 256 -h 256 packaging/graphene.svg -o packaging/graphene.png`.

Packaging lives in `scripts/package.sh` (output in `dist/`) and `packaging/`. To run it
locally: `cargo install --locked cargo-deb cargo-generate-rpm`, put `appimagetool` on
`PATH` (or set `APPIMAGETOOL`), then:

```bash
cargo build --release --locked
scripts/package.sh
```

### AUR

`scripts/package.sh` also renders `dist/aur/PKGBUILD` and `.SRCINFO` for the AUR
package `graphene-term-bin`. The workflow step that pushes them to the AUR is
commented out for now; once uncommented it runs only when the `AUR_SSH_PRIVATE_KEY`
repository secret is set. One-time setup: create an AUR account, add the public half of a dedicated SSH key to
it, and store the private half in that secret. Install with `paru -S graphene-term-bin`
(or `yay`).
