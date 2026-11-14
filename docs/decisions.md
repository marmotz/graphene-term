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
unknown. Command blocks need it (a block signal must be applied where the shell emitted
it, and `vte` 0.15 drops unknown OSC without a callback). Graphene therefore runs its
own PTY thread (`src/pty.rs`), a close copy of alacritty's loop on `polling`, whose
parse step `pty::feed` can be split around signals. alacritty's `tty` module still spawns the
shell and owns the PTY file descriptors. Poller keys `0` (read/write) and `1` (child
event) mirror alacritty's unix `tty`, where they are `pub(crate)`.

## One terminal per block: live grid and frozen blocks

The first implementation cut one grid into blocks with the OSC 133 marks the shell
puts around its prompt, and anchored them in the cells with internal hyperlinks.
Prompt themes (powerlevel10k) rebuild the prompt and their own marks after every hook,
so the blocks broke, and any cursor move up from a command could overwrite older
blocks. Each block is now its own terminal: the session's `Term` is the live grid of
the current block, detached into a frozen `Term<VoidListener>` when the shell signals
that a command starts (the header: prompt and typed command) and ends (the output).
One parser and one set of modes stay with the session (nothing to replay into a new
`Term`), and frozen blocks keep alacritty's selection, copy and reflow. The only
signals are two private OSC sequences carrying a session token (`start` with the
command line, `end` with the status), so the 133 marks of fish, powerlevel10k, a remote
shell or a printed file never matter. The prompt is never wrapped or parsed.

## Card space always reserved

The command blocks are drawn as cards: a margin for the status stripe and half a cell
of card padding left of the grid, the card padding and room for the fold chevron
(on the card's top-right corner) right of it. This space is reserved from the start,
even when the shell sends no signal, so the first block never changes the column count:
no reflow mid-session. The cost is about one and a half columns fewer for every user.

## Cards replace the gutter marks

The first rendering put a one-cell gutter bar, a 1 px separator over the grid and a font
chevron (`▸` / `▾`) on the command row. With a real prompt (powerlevel10k) it read as
console content, and the chevron glyph came from a fallback font, a few pixels wide,
half under the status bar. Each block is now a card: a rounded background derived from
the palette, a status stripe in the margin and a chevron drawn as SDF shapes (disc and
triangle) in the rect pipeline, so it is crisp at any size and needs no font.

## Gaps between cards keep the PTY size

Half a line separates two cards. The PTY size never changes: the gaps push the top rows
of the view out, reachable by scrolling, rather than shrinking the PTY per block (which
would reflow and redraw full-screen programs). The view is laid out in half-lines with
whole rows only; a block is capped one line short of the view, so the gap above it
always fits and a capped block is always shown whole.

## Scroll positions owned by Graphene

The view spans several grids, and a block shows at most a view height (its header
pinned, its output scrolled), which is not alacritty's viewport. The scroll positions
are Graphene's: one for the timeline (half-lines from its bottom) and one per block
(output rows hidden below). The live grid stays at `display_offset` 0. When scrolled
up, the timeline offset grows by what was added below (`view::follow`), so the view
stays still while output arrives, which alacritty used to do for its viewport. Folding
a block replaces its output with one summary row; the grids are untouched.

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
- When the application enabled mouse reporting, left and middle clicks over its rows
  (the live grid, the whole window on the alternate screen) are reserved for it;
  `Shift` forces a local selection. Over frozen blocks the mouse stays Graphene's.
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

## Input editor: shell bindings for private sequences

The shell keeps the command line buffer; Graphene never edits it. Editor moves are
private `CSI <code> u` sequences (code points 59136 to 59148, Unicode private use
area, table in `src/input_editor.rs`) bound by the integration scripts in the emacs
and vi insert keymaps.

- Every move is passive: clamped at both ends of the buffer, never leaving it for the
  history, never accepting an autosuggestion. A plain `Right` at the end of the buffer
  would accept it (fish, zsh-autosuggestions).
- zsh: own widgets calling the builtin (dot) widgets (`.forward-word`,
  `.end-of-line`, ...), which zsh-autosuggestions does not wrap, so none of them
  accepts its suggestion. Bound again before each prompt, after the user's files and
  plugins.
- bash: `bind -x` functions on `READLINE_LINE` / `READLINE_POINT`, readline
  functions for words. Verified on bash 5.3: `READLINE_POINT` counts characters in a
  UTF-8 locale, like `${#READLINE_LINE}` and substring expansion (bytes in the C
  locale, consistently for both), so the functions never mix units.
- fish: a raw sequence starting with `\e` keeps the old `bind` syntax in fish 4
  (verified on 4.9.3, per the 4.0 changelog), so one syntax covers fish 3 and 4.
  `forward-char-passive` / `backward-char-passive` exist since fish 4.0; older fish
  falls back to `backward-char` / `forward-char`, detected with
  `bind --function-names`. `forward-word`, `kill-word`, `end-of-line` and
  `end-of-buffer` accept the autosuggestion at the end of the buffer, the only place
  it shows: they only run when the cursor is not there. Bound again before a prompt
  when gone (`fish_vi_key_bindings` erases bindings).

## Input editor: rich mode at the prompt

- The shell keeps the buffer: Graphene never knows its content. A click, a shortcut or
  a selection becomes private sequences only (see above); every edit is the shell's.
- The rich mode needs to know the shell's line editor is reading a command line with
  Graphene's bindings. Since each block is its own terminal, the hooks no longer mark
  the prompt, so they send one more signal, `prompt`, before each prompt (zsh
  `precmd`, bash's first `PROMPT_COMMAND` entry, fish `fish_prompt`). The current block
  is then `editing` until its command starts. Without the integration (shell started by
  hand, `ssh`), no `prompt` comes: every key behaves as in a plain terminal.
- Only distances are read from the grid (`input_editor::buffer_distance`): a click
  writes as many `CharLeft` / `CharRight` as buffer characters lie between the cursor
  and the click, in one PTY write. The shell clamps them at the buffer ends, so a click
  on the prompt or past the end of the text lands on the nearest buffer end. Where the
  prompt starts (the cursor when `prompt` came) bounds a click on the output above it.
- The prompt and the input are not told apart in the grid (bash has no hook between
  drawing the prompt and reading the line): a click on the prompt counts its
  characters, all absorbed by the clamp.
