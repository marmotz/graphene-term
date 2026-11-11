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
```

Note: `cargo test` and `cargo clippy` do not rebuild `target/debug/graphene`. Run
`cargo build` before launching the binary directly.

## Configuration

No configuration file yet. Environment variables:

| Variable             | Default          | Effect                                                  |
|----------------------|------------------|---------------------------------------------------------|
| `GRAPHENE_FONT`      | system monospace | Font family name                                        |
| `GRAPHENE_FONT_SIZE` | `14`             | Font size in points (scaled by the window scale factor) |
| `RUST_LOG`           | `warn`           | Log level (`env_logger`)                                |

The theme ("graphene night") is defined in `src/palette.rs`.

## Shortcuts

| Keys                          | Action                                           |
|-------------------------------|--------------------------------------------------|
| Ctrl+Shift+V, Shift+Insert    | Paste (bracketed when the app requests it)       |
| Ctrl+Shift+C                  | Reserved for copy (no-op until selection exists) |
| Shift+PageUp / Shift+PageDown | Scroll the history by a page                     |
| Shift+Home / Shift+End        | Top / bottom of the history                      |
| Ctrl+= / Ctrl+- / Ctrl+0      | Font size up / down / reset                      |
| Mouse wheel                   | History, or arrow keys in full-screen apps       |

## Tests

Unit tests live next to the code (`#[cfg(test)]` modules):

| Module            | Covers                                                      |
|-------------------|-------------------------------------------------------------|
| `palette`         | 256-color table, overrides, inverse/hidden/dim              |
| `input`           | key encoding, control codes, cursor modes, shortcuts, paste |
| `layout`          | grid size from pixels, padding, minimum size                |
| `frame`           | capture from a real `Term` fed with escape sequences        |
| `renderer::scene` | background merging, text runs, decorations, sRGB conversion |

`frame` tests build a `Term<VoidListener>` and feed bytes through
`vte::ansi::Processor`, so they exercise the real emulator without a PTY or GPU.

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
