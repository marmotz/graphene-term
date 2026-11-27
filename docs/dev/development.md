# Development

## Toolchain

The repository pins the toolchain in `rust-toolchain.toml` (`stable`). This is needed
on machines where rustup has no default toolchain. Edition: 2024.

## Commands

```bash
cargo run                         # debug build
cargo run --release               # optimized build
cargo run -- -e htop              # run a program instead of the shell in the first pane
cargo test                        # unit and integration tests
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo bench                       # criterion benchmarks (benches/render.rs)
```

CI runs `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`,
`cargo test --locked` and a release build on Linux, and the tests and release build on
Apple Silicon and Intel macOS (`.github/workflows/ci.yml`).

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
locale, see `Language::resolve`), font family and size, window startup mode, size,
padding and background opacity, scrollback, word separators, cursor shape and blinking,
tab bar position, theme, keybindings (`src/keybindings.rs`: default table, then the
configured entries; an invalid key or action is reported like any invalid value) and
the update check. The configuration directory is watched (`src/watcher.rs`) and a change
is applied without restarting. The settings window (`src/settings/`) writes each change
to `config.toml` (`src/config_edit.rs`), and the hot reload applies it.
`graphene --prune-config [--yes]` removes the keys the parser does not recognize.

The user documentation lists every key in `docs/en/configuration.md` and
`docs/fr/configuration.md`: update both pages when a key is added, removed or changes
meaning.

The cursor blinks (`src/cursor_blink.rs`) when its style asks for it (`cursor.blinking`,
or DECSCUSR from the application) and the window has the focus: the phase toggles every
500 ms, `App::about_to_wait` waits until the next toggle, and a key press or a paste
restarts the cycle on a visible phase.

In `config.default.toml`, settings and section headers are commented with `# ` and
documentation lines with `##`: a test uncomments the settings and checks that the result
equals `Config::default()`, so keep the file in sync with the code.

| Variable                       | Default | Effect                                   |
|--------------------------------|---------|------------------------------------------|
| `RUST_LOG`                     | `warn`  | Log level (`env_logger`)                 |
| `GRAPHENE_SHELL_INTEGRATION`   | unset   | `0` starts the shell without integration |

### Themes

Built-in themes are TOML files in [themes/](../../themes/), embedded in the binary
(`theme::BUILT_IN`) and parsed by `src/theme.rs` into a `Palette` (`src/palette.rs`
holds the palette type and the 256-color table). User themes are files in the `themes/`
subfolder of the configuration directory. A missing or invalid color keeps the value of
the base theme `graphene-night`, so a partial theme still works. `theme.light` and
`theme.dark` follow the system appearance (`src/appearance.rs`). To add a built-in
theme, add its file to `themes/` and an entry to `theme::BUILT_IN`.

### Startup and redraw timing

`RUST_LOG=graphene::perf=debug` (`src/perf.rs`) logs the time of each startup step since
the start of `main`: window created, glyphs ready, shell spawned, GPU instance and
surface created, adapter ready, device ready, renderer ready, window shown, then the
first PTY output and the first frame drawn after it. The GPU steps run on their own
thread, alongside the glyphs and the shell (in sequence on macOS), so their marks
interleave. The window is created hidden and shown once the renderer is ready; a
window opened maximized also holds its first frame until it has its maximized size
(150 ms at most). It then logs the mean and longest time of each
series of 120 redraws (a redraw presented by the GPU surface includes its wait for the
monitor refresh).

```bash
RUST_LOG=graphene::perf=debug cargo run --release
```

## Shortcuts

Keyboard shortcuts and mouse gestures are user documentation: see
[keybindings](../en/keybindings.md). The default table is `keybindings::DEFAULTS`
in `src/keybindings.rs`.

## Tests

- **Unit tests** live next to the code, in a `#[cfg(test)] mod tests` at the bottom of
  nearly every module of `src/`. Logic is kept in pure functions (no window, GPU, PTY
  or system access) so it can be tested there; many module doc comments state what the
  module does not touch.
- **Emulator tests** (`frame`, `timeline`, `selection`, `graphics`...) build a
  `Term<VoidListener>` (often through `timeline::testing::Session`) and feed it bytes,
  so they exercise the real emulator without a PTY or GPU.
- **Renderer tests** that need a GPU (headless render read back) are skipped with a
  message when no adapter is available.
- **Integration tests** in `tests/` run the built binary (`version.rs`,
  `config_prune_command.rs`) or the scripts of `scripts/` in a sandbox with stubbed
  external tools (`package_script.rs`, `macos_package_script.rs`,
  `arch_package_script.rs`, `release_notes_script.rs`), and check invariants of the
  release workflow (`release_workflow.rs`).

Run one module's tests with `cargo test <module>::`, one integration file with
`cargo test --test <name>`.

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
| `frame_panes` | the frames of four shown panes when one changed, through the `FrameCache` |
| `gpu_prepare` | upload, draw and submit on a headless device: `idle` (same frame), `changed` (blank frame then the fixture) |
| `search`      | a search over 10 000 lines of history: `typed` (a new query, every grid searched), `output` (the same query, only the live grid searched again) |
| `present`     | a 1920x1080 frame: `direct` (drawn and waited for, the GPU surface), `x11` (read back and packed, the X11 presentation), `x11_async` (the part of `x11` that keeps the event loop busy), `x11_pack/rgba` and `x11_pack/bgra` (packing alone) |

`gpu_prepare` and `present` are skipped with a message when no GPU adapter is
available. Run one group
with `cargo bench -- gpu_prepare`; compare two versions with
`cargo bench -- --save-baseline before`, then `cargo bench -- --baseline before`.
Re-recording the fixtures is described in `benches/fixtures/README.md`.

### Comparing terminals

`scripts/benchmark-terminals.py` (Python 3, standard library only, Linux only) compares
Graphene with other installed terminals: Alacritty, Kitty, WezTerm, Terminator and
GNOME Terminal. It opens real windows, so run it on an idle desktop.
When Graphene is selected, the script first runs `cargo build --release` on the current
checkout and benchmarks that binary (`$CARGO_TARGET_DIR/release/graphene`, default
`target/release/graphene`); a failed build aborts.

```bash
scripts/benchmark-terminals.py run                        # every installed terminal, 5 runs each
scripts/benchmark-terminals.py run --terminals graphene,alacritty --runs 3 --out results.json
scripts/benchmark-terminals.py run --columns 120 --lines 40   # another grid (default 100x30)
scripts/benchmark-terminals.py render results.json        # print the tables again
```

| Metric            | How                                                                         | Unit |
|-------------------|-----------------------------------------------------------------------------|------|
| Throughput        | time of `/bin/cat shakespeare.txt` inside the terminal                      | s    |
| Refresh rate      | DOOM-fire frames (256 colors, half blocks, full window) written for 10 s    | fps  |
| Startup           | wall time of `<terminal> <exec prefix> true`                                | ms   |
| Memory idle       | PSS of the terminal's processes 3 s after start                             | MiB  |
| Memory under load | peak PSS sampled every 100 ms during the throughput run                     | MiB  |

- Each metric is the median of `--runs` runs (default 5). Raw values go to `--out`
  (default `benchmark-results.json`); the Markdown table goes to stdout, Graphene first,
  best value of each column in bold, followed by the machine (CPU, GPU, kernel, session,
  resolution), the terminal versions and the grid.
- Every terminal opens with the same grid, `--columns` x `--lines` (default 100x30), so
  each one draws as many cells whatever its font: each `TERMINALS` entry passes the grid
  with the terminal's own option. Terminator only takes `--geometry` in pixels: the
  script probes two window sizes, deduces the cell size, then corrects the size until
  the grid matches. It is also given a copy of the user's configuration with
  `window_state = normal`, as a maximized window ignores `--geometry`.
- Each payload records the grid it ran in. A grid other than the requested one (a
  tiling window manager, a too small screen) is warned about on stderr, stored in the
  JSON (`grids`) and listed as `Grid mismatch` under the table.
- Throughput and fps measure how fast a terminal accepts output, not what it draws.
- PSS is read from `/proc/<pid>/smaps_rollup`, summed over the terminal and its
  descendants, the payload excluded.
- `shakespeare.txt` is downloaded once into `$XDG_CACHE_HOME/graphene-benchmark/` and
  checked against its SHA-256; a mismatch aborts.
- Missing terminals are skipped and listed on stderr. GNOME Terminal is skipped while
  another `gnome-terminal-server` runs: the script never kills the user's terminals.
- A payload that does not report within 60 s gives `n/a`; only the process the script
  started is killed.
- Each terminal's launch command is in the `TERMINALS` table at the top of the script;
  it is the only place to fix when a terminal changes its options.
- `tests/benchmark_script.rs` covers `render` (on `tests/fixtures/benchmark-results.json`),
  the `fire` and `throughput` payloads, the launch command of every terminal, the
  Terminator configuration copy, and the release build that `run` starts.

#### Latest results

These numbers track regressions; the README only gives a summary of them. Lower is
better, except refresh rate.

| Terminal       | Throughput (s) | Refresh rate (fps) | Startup (ms) | Memory idle (MiB) | Memory under load (MiB) |
|----------------|----------------|--------------------|--------------|-------------------|-------------------------|
| **Graphene**   | 0.22           | 3679.4             | 461          | 140.1             | 94.9                    |
| Alacritty      | 0.20           | **6412.5**         | **230**      | 39.4              | 64.5                    |
| Kitty          | **0.16**       | 4865.8             | 463          | 98.0              | 108.9                   |
| WezTerm        | 0.48           | 2452.4             | 231          | 66.7              | 80.3                    |
| Terminator     | 0.29           | 3765.5             | 463          | 52.6              | 54.4                    |
| GNOME Terminal | 0.28           | 3790.0             | 357          | **25.7**          | **26.3**                |

Machine:

- CPU: AMD Ryzen 5 5600G with Radeon Graphics
- GPU: NVIDIA Corporation AD104 [GeForce RTX 4070 Ti] (rev a1)
- Kernel: 7.3.4-1-cachyos
- Session: x11
- Resolution: 5120x1440
- Runs: median of 5
- Versions:
    - graphene 0.1.3-dev.20261119203421
    - alacritty 0.17.0 (94e7c887)
    - kitty 0.50.0 created by Kovid Goyal
    - wezterm 20260716-195552-76b606ec
    - terminator 2.1.6
    - GNOME Terminal 3.60.0 using VTE 0.84.1 +BIDI +GNUTLS +ICU +SYSTEMD

Two columns look wrong and need checking before they are trusted: startup values
cluster around multiples of about 230 ms, and Graphene's idle memory is higher than its
memory under load.

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
