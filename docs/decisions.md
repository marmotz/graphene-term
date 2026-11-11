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

## Selection and copy

- The selection is `alacritty_terminal`'s `Term::selection`: it follows its text when
  the content scrolls, and handles word, line and block selection, wide characters and
  wrapped lines for free.
- Copied text is cleaned: trailing whitespace removed from every line, common
  indentation removed, no trailing newline (a paste never runs a command by itself).
- Releasing a drag copies to the primary selection; `Ctrl+Shift+C` copies to the
  clipboard. Without a selection, `Ctrl+Shift+C` is still swallowed so it never sends
  `^C`. The primary selection may be unsupported by a Wayland compositor: logged at
  debug level, clipboard copy is unaffected.
- Word separators are alacritty's defaults without `:`, so a URL or `host:port` is one
  word on double-click.
- When the application enabled mouse reporting, left and middle clicks are reserved
  for it; `Shift` forces a local selection.
- Keystrokes sent to the shell and pastes clear the selection; app shortcuts keep it.
- Program output clears the selection only when the selected text really changes. `Term`
  clears it on erase sequences even when the same text is redrawn (prompt redraws,
  status lines), and keeps it when a cell is overwritten by a different character.
  Graphene keeps a shadow `SelectionSnapshot` (selection + cleaned text) taken on
  release: on each `Wakeup` it restores the selection if `Term` dropped it, then
  compares the selected text with the snapshot and clears both when it differs. Cost:
  one `selection_to_string` per wakeup while a selection exists. Known limit: content
  scrolled and erased in the same PTY batch leaves stale coordinates; the text then
  differs and the selection is cleared, the safe outcome.

## IME disabled

Enabling winit IME risks double input (key event text + IME commit). Deferred until
it can be handled and tested properly.
