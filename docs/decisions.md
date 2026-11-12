# Technical decisions

## Rust + wgpu, native

Chosen over Tauri/Electron + xterm.js and GTK4/VTE: lowest latency and memory, full
control over the look. Cost: UI elements (tabs, palette, blocks) must be drawn by hand.

The window renderer creates its wgpu instance with the Vulkan backend alone first and
falls back to all backends when Vulkan has no compatible adapter: probing every backend
cost ~100 ms at startup (see `backlog/features/performance/technical.md`).

## `alacritty_terminal` for emulation

A correct VT emulator is a large amount of work (escape sequences, alternate screen,
scroll regions, wide chars, reflow on resize). `alacritty_terminal` provides it, plus
the PTY, and is battle-tested. We only depend on its public API (`Term`, `tty`, `vte`).
Graphene-specific features (blocks, input editor) will
be layered on top.

## Own PTY loop

alacritty's `event_loop::EventLoop` parses whole read buffers, sometimes several reads
at once, inside its own thread: the cursor position at a given byte of the stream is
unknown. Command blocks need it (an OSC 133 mark must be applied where the shell emitted
it, and `vte` 0.15 drops OSC 133 without a callback). Graphene therefore runs its own
PTY thread (`src/pty.rs`), a close copy of alacritty's loop on `polling`, whose parse
step `pty::feed` can be split around marks. alacritty's `tty` module still spawns the
shell and owns the PTY file descriptors. Poller keys `0` (read/write) and `1` (child
event) mirror alacritty's unix `tty`, where they are `pub(crate)`.

## Cell tags through hyperlinks

`Term` exposes no scroll counter: a grid line stored when a mark arrives is wrong after
the next scroll, history eviction or reflow. Blocks are anchored in the cells instead:
while the shell draws the prompt and the input, the cursor template carries an OSC 8
hyperlink with the reserved scheme `graphene-block` (`graphene-block://<id>/prompt`,
`right-prompt` or `input`), which `write_at_cursor` copies into every written cell.
Cells keep it on scroll, eviction and reflow, and erased cells lose it, so the rows of a
block are found again from the grid alone, through the public API. A real OSC 8 link
opened by the prompt is never overwritten: the tag comes back when it closes. Output
cells carry no tag; a row with no tag belongs to the block above it. The links feature
must ignore the `graphene-block` scheme. The right prompt has its own tag kind so it is
never taken for the command start.

## Gutter always reserved

The command block marks (status bar, fold chevron) are drawn in a gutter one
cell wide, left of the grid. It is reserved from the start, even when the shell sends
no marks, so the first block never changes the column count: no reflow mid-session.
The cost is one column fewer for every user.

## Folding as a row filter over `display_offset`

Folding a block hides its output rows from the view; the grid is untouched. Graphene
keeps alacritty's `display_offset` as the scroll position instead of its own:
alacritty already keeps the view stable when output arrives while scrolled up, and
`Shift+Home/End` keep working as is. The view is built from the bottom line of
alacritty's viewport, collecting rows upward and skipping the hidden ones, so a fold
costs nothing when there is none (the plain viewport). Scrolls given in visible rows
are converted into grid lines (`view::grid_delta`). The cost: the folded rows are still
in the history and count against the scrollback size, and the view has to be padded
with blank rows when folds hide more rows than the history can replace. The cursor row
is never hidden, so the shell's next prompt always shows.

## Own glyph pipeline on `cosmic-text` (glyphon dropped)

glyphon was used first: it handles shaping, font fallback and a glyph atlas through
`cosmic-text`. But it shapes text runs, and a terminal frame where a full screen changed
(btop) cost 60 to 90 ms of reshaping and `prepare`. A grid needs no shaping: one cell,
one glyph. Graphene now keeps `cosmic-text` for font loading, fallback and
rasterization, caches one rasterized glyph per `(char, bold, italic)` and draws one
instanced quad per glyph (see [architecture.md](architecture.md#glyphs)). Single
characters are still shaped with `Shaping::Advanced` so that fallback fonts work.
Consequence: programming-font ligatures are never drawn.

## Exact cell width, glyphs anchored per cell

`cosmic-text` 0.19 `Buffer::set_monospace_width` does not force glyph advances to the
cell width (it divides a pixel advance by an em width, so the rounding is ineffective).
Combined with a rounded cell width, text drifted to the right of its background.

Fix:

- the cell width is the exact advance of `M`, not rounded;
- each glyph is positioned at its own cell origin (see [architecture.md](architecture.md#glyphs)).

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
