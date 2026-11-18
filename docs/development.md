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
ignores the whole file. Every key is applied: interface language (`auto` resolves the
locale, see `Language::resolve`), font family and size, padding, initial window size,
background opacity, scrollback, word separators, cursor shape and blinking, tab bar
position, theme, and keybindings (`src/keybindings.rs`: default table, then the
configured entries; an invalid key or action is reported like any invalid value).

The cursor blinks (`src/cursor_blink.rs`) when its style asks for it (`cursor.blinking`,
or DECSCUSR from the application) and the window has the focus: the phase toggles every
500 ms, `App::about_to_wait` waits until the next toggle, and a key press or a paste
restarts the cycle on a visible phase.

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
| `input`           | key encoding, control codes, cursor modes, action ids, paste |
| `keybindings`     | default shortcuts, remapping, `none`, key syntax, Shift and physical-key matching |
| `layout`          | grid size from pixels, padding, minimum size                |
| `frame`           | capture from a real `Term` fed with escape sequences        |
| `renderer::scene` | background merging, glyph instances, decorations, sRGB conversion |
| `renderer::cells` | glyph quad placement, skipped glyphs, frame resolved again after an atlas reset |
| `renderer`        | headless render read back: glyph pixels land in their cell (skipped without GPU) |
| `perf`            | timing line format, first output / first frame tracking     |

`frame` tests build a `Term<VoidListener>` and feed bytes through
`vte::ansi::Processor`, so they exercise the real emulator without a PTY or GPU.

## GUI E2E scenarios

Pull-request CI runs every scenario of
[`tests/e2e/scenario-catalog.md`](../tests/e2e/scenario-catalog.md) on both X11 (Xvfb)
and native Wayland (headless Sway) with `tests/e2e/gui-automation-probe.sh`. Each run
starts the built `graphene` executable as a separate process, gives it a private home and
XDG config/state/runtime directories, and disables update checks; the `update-banner`
scenario turns them on with a recent cached release, so no request reaches GitHub, and
links open through a recording `xdg-open`. Input goes through the
backend's driver (`xdotool` on X11, `wtype` and `wayland-automation` on Wayland). The
controlled shell fixture assigns each pane a unique ID, so navigation is asserted by
typing a command into whichever shell has focus; pane sizes are read with `stty size`,
palette actions through the recent-actions file, and dialog and Settings text with OCR
(`tesseract`). Terminal scenarios locate words of a known text with OCR, then assert the
selection and clipboard Graphene fills (`xclip`, `wl-clipboard`). The `shell-*`,
`blocks` and `scroll` scenarios start a real bash, zsh or fish with Graphene's integration and
startup files written to the private home. Configuration scenarios edit the private
`config.toml` and themes from Settings or by hand, then read the result back from the
files, from `stty size` and from the background colour a shell reads with an OSC 11
query. Screenshots are not compared. The application and its shells are stopped
at the end of each run. Software rendering is enabled for Graphene and the Wayland
compositor. On failure, CI uploads the application/compositor logs and a screenshot for
seven days.

The catalogue lists every scenario and every palette action with the scenario covering
it; a unit test fails when a new action has no row there. Add or update the scenario and
its catalogue row with any user-facing interaction change.

To run locally after installing `xvfb`, `xdotool`, `xclip`, `sway`, `wtype`,
`wl-clipboard`, `grim`, `imagemagick`, `tesseract-ocr`, `zsh`, `fish`, `python3-pip`, `libgl1-mesa-dri`, `libx11-6`,
`libxkbcommon-x11-0`, `libxcursor1`, `libxi6`, `libxinerama1`, and `libxrandr2`:

```bash
python3 -m pip install --no-deps --target /tmp/wayland-automation wayland-automation==0.3.1
cargo build
export PYTHONPATH=/tmp/wayland-automation
export GRAPHENE_BIN="$PWD/target/debug/graphene"
export GRAPHENE_E2E_ARTIFACTS=/tmp/graphene-e2e-artifacts
for scenario in smoke tabs palette panes menu dialogs settings selection clipboard scroll \
  font mouse blocks shell-bash shell-zsh shell-fish startup config-reload settings-fields \
  settings-keys update-banner; do
  xvfb-run -a -s "-screen 0 1280x800x24 +extension GLX +render -noreset" \
    tests/e2e/gui-automation-probe.sh x11 "$scenario"
  tests/e2e/gui-automation-probe.sh wayland "$scenario"
done
```

Pass a single scenario ID after the backend to run it alone (for example
`tests/e2e/gui-automation-probe.sh x11 tabs`); without one, `smoke` runs.

The GitHub Actions job check is named **`e2e`**. Add that exact status check as required in
the repository's branch protection rule or ruleset (Settings → Branches / Rulesets) so a
failed GUI scenario blocks merging. Required checks are a repository setting and cannot
be enabled by the workflow itself.

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

Pull requests run the test suite and release build on Linux, Apple Silicon macOS and
Intel macOS. To release, merge `develop` into `main`, create and push a `vX.Y.Z` tag on
that commit, then launch **Actions → Release → Run workflow** with the tag selected. The
workflow rejects branch refs, tags not on `main`, and tags that do not match the
`Cargo.toml` version. It builds from the tagged commit in the stable channel, then creates
the GitHub release using that same tag.

The version is computed at build time by `build.rs`: `<crate version>-dev.<UTC
YYYYMMDDHHMMSS>` by default, `<crate version>` when built with `GRAPHENE_CHANNEL=stable`.
All release builds use the stable channel, so the app version matches the tag.

The release description is generated from commit subjects between the latest published
regular release tag and the selected tag. On the first regular release, it covers the full
history (earlier pre-releases are not used as the starting point). Conventional `feat:`
commits go under **Nouveautés**, `fix:` commits under **Correctifs**, and all other
commits under **Modifications**. Pull-request numbers in subjects link to their PR; other
entries link to their commit.

Assets:

- Linux:
  - `graphene-<tag>-x86_64-unknown-linux-gnu.tar.gz`: binary, desktop entry, icon, licenses
  - `graphene-term_<version>_amd64.deb`
  - `graphene-term-<crate version>-0.dev.<timestamp>.x86_64.rpm`
  - `graphene-<tag>-x86_64.AppImage`
- macOS:
  - `graphene-<tag>-aarch64-apple-darwin.tar.gz` and `.dmg` for Apple Silicon
  - `graphene-<tag>-x86_64-apple-darwin.tar.gz` and `.dmg` for Intel
  - DMGs contain an ad-hoc signed, unnotarized `Graphene.app`
- `SHA256SUMS`

The macOS package is built by `scripts/package-macos.sh`; it needs the macOS tools
`sips`, `iconutil`, `codesign` and `hdiutil`. To package a local Apple Silicon build:

```bash
cargo build --release --locked
scripts/package-macos.sh aarch64-apple-darwin
```

For Intel, run the same commands on an Intel Mac with `x86_64-apple-darwin`.

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

To build that package locally from a published release (needs `curl`, `jq`, `makepkg`),
pass a tag, or nothing for the most recent release (pre-releases included):

```bash
scripts/build-arch-package.sh v0.1.0-dev.20261113205256
sudo pacman -U dist/arch/graphene-term-bin-*.pkg.tar.zst
```
