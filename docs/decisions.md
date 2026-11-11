# Technical decisions

## Rust + wgpu, native

Chosen over Tauri/Electron + xterm.js and GTK4/VTE: lowest latency and memory, full
control over the look. Cost: UI elements (tabs, palette, blocks) must be drawn by hand.

## `alacritty_terminal` for emulation

A correct VT emulator is a large amount of work (escape sequences, alternate screen,
scroll regions, wide chars, reflow on resize). `alacritty_terminal` provides it, plus
the PTY and its I/O thread, and is battle-tested. We only depend on its public API (`Term`, `tty`, `event_loop`).
Graphene-specific features (blocks, input editor) will
be layered on top.

## glyphon for text

Integrates directly with wgpu, handles shaping, font fallback and a glyph atlas through
`cosmic-text`. Shaping is `Shaping::Advanced` so that fallback fonts work.

## Exact cell width, runs anchored per column

`cosmic-text` 0.19 `Buffer::set_monospace_width` does not force glyph advances to the
cell width (it divides a pixel advance by an em width, so the rounding is ineffective).
Combined with a rounded cell width, text drifted to the right of its background.

Fix:

- the cell width is the exact advance of `M`, not rounded;
- each run is positioned at its own column (see [architecture.md](architecture.md#text-runs)).

## Block cursor baked into cells

Drawing the block cursor as a rect under the text and recoloring the glyph keeps the
text renderer unaware of the cursor. Non-block shapes are drawn as rects over the text.

## Environment for the shell

`TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=graphene`. We do not call
`alacritty_terminal::tty::setup_env`, which would select the `alacritty` terminfo.

## Ctrl+Shift+C is reserved

Copy is not implemented yet (no selection), but the shortcut is swallowed so that it
never sends `^C` to the shell by accident.

## IME disabled

Enabling winit IME risks double input (key event text + IME commit). Deferred until
it can be handled and tested properly.
