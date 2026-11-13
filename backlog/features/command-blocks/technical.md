# Command blocks: technical design

Product decisions: [overview.md](overview.md).

This document describes the reworked block model: one terminal per block. The first
implementation (one alacritty grid cut into blocks by OSC 133 marks and cell tags,
issues #6 to #12 and #70 to #73, branch `feature-command-blocks`, never merged) is
summarised in [First implementation](#first-implementation-what-is-kept), with what
the rework keeps from it.

## Starting point

Current code on `feature-command-blocks`:

- **PTY loop.** [pty.rs](../../../src/pty.rs) replaces alacritty's `EventLoop`.
  `process` ([pty.rs:74](../../../src/pty.rs)) splits the byte stream with the OSC
  scanner and applies each mark right after the bytes before it, under the `Term`
  lock, then the `ShellState` lock (fixed order, [terminal.rs:46](../../../src/terminal.rs)).
- **Scanner.** [osc133.rs](../../../src/osc133.rs) reports `133;A/P/B/C/D` and `8;…`
  (`Mark`, [osc133.rs:19](../../../src/osc133.rs)).
- **Shell state.** [blocks.rs](../../../src/blocks.rs) tags the cells of the prompt and
  the input with a template hyperlink `graphene-block://<id>/<kind>`, then rebuilds a
  row index from those tags (`index`, [blocks.rs:433](../../../src/blocks.rs)).
- **Injection.** [shell_integration.rs:56](../../../src/shell_integration.rs) `prepare`
  injects zsh (`ZDOTDIR`), bash (`--posix` + `ENV`) and fish (`XDG_DATA_DIRS`). The zsh
  and bash scripts wrap `PS1` / `RPS1` in `133;A` … `133;B`
  ([graphene.bash:51-56](../../../shell-integration/bash/graphene.bash)).
- **View.** [view.rs:151](../../../src/view.rs) `rows` walks alacritty's viewport
  (`display_offset`) and skips folded output rows; gaps between cards are half lines
  (`ViewRow::top`, `gap_before`).
- **Rendering.** [frame.rs:173](../../../src/frame.rs) `Frame::capture` reads one `Term`;
  [frame.rs:298](../../../src/frame.rs) `marks` builds the `Card`s;
  [scene.rs](../../../src/renderer/scene.rs) draws card backgrounds, status stripes and
  chevrons with the rounded rect pipeline ([rects.rs](../../../src/renderer/rects.rs)).
- **Mouse.** [app.rs:441](../../../src/app.rs) `mouse_press` routes `Hit::{Margin,
  Chevron, Gap, Cell}` ([layout.rs:138](../../../src/layout.rs));
  [app.rs:663](../../../src/app.rs) `mouse_wheel` scrolls alacritty's viewport through
  `view::grid_delta`.

Why it broke: everything depends on the marks the shell writes around its prompt.
powerlevel10k rebuilds `PS1` in a `precmd` hook that always runs last and emits its own
`133;A/B` marks when asked (`/usr/share/zsh-theme-powerlevel10k/internal/p10k.zsh:8408-8421`),
so the wrapping does not stick, and a second `133;B` after the right prompt moves the
input start. fish emits `133;A/B/C/D` natively as well. And since all blocks share one
grid, any cursor move up from a command can overwrite older blocks.

`alacritty_terminal` 0.26 API used by the rework (checked in the crate sources):

- `Term::grid_mut(&mut self) -> &mut Grid<Cell>` (`term/mod.rs:650`): the active grid.
- `Grid<T>` is `Clone`, with `pub cursor` / `pub saved_cursor`, `Grid::new(lines,
  columns, max_scroll_limit)` (`grid/mod.rs:141`), `clear_history`, `reset_region`,
  and `Grid::resize(reflow, lines, columns)` (`grid/resize.rs:14`).
- `Term::new(config, dims, listener)`, `Term::resize`, `selection`,
  `selection_to_string`, `bounds_to_string`, `swap_alt`, `mode`.
- `clear_screen(ClearMode::All)` on the main screen moves the viewport content into the
  history (`clear_viewport`, `term/mod.rs:1788-1797`); `ClearMode::Saved` (`CSI 3 J`)
  clears the history.

## Alternatives considered

| Topic | Options | Retained |
|---|---|---|
| Terminal per block | One fresh `Term` per block, replaying the shell's modes (bracketed paste, mouse, keypad, palette, title, kitty keyboard flags) into each new one / one live `Term` whose grid is detached into a frozen block at each signal | Live `Term` + frozen blocks (user decision): one parser and one set of modes, nothing to replay; frozen blocks keep alacritty's selection, copy and reflow. |
| Block split | Header and output in one grid, with an output start mark / two grids, the header frozen when the command starts | Two grids (user decision): the output start is the grid boundary, it survives reflow without any tag, and a command can never redraw its prompt or command line. |
| Block boundaries | OSC 133 marks around the prompt / OSC 133 `C` / `D` only / private OSC with a session token | Private OSC + token (user decision): no collision with the 133 marks of fish, powerlevel10k, a remote shell, or a file printed by `cat`. |
| Cross-block selection | Limited to one block / composed by Graphene over several blocks | Composed (user decision): one alacritty `Selection` per block involved. |
| Scroll position | alacritty `display_offset` / Graphene offsets (timeline + one per block) | Graphene offsets: the timeline spans several grids, and a block's visible rows (capped, header pinned) are not alacritty's viewport. The live grid stays at `display_offset` 0. |
| Frozen block storage | Rows copied into our own structure / a small `Term<VoidListener>` holding the detached grid | `Term<VoidListener>`: selection, `bounds_to_string`, reflow and the frame capture code work on it unchanged. |

## Design

### Signals: private OSC and hooks

Sequence, `ST` being `ESC \`:

```
ESC ] 6973 ; <token> ; start ; <command line, base64> ST
ESC ] 6973 ; <token> ; end ; <exit status> ST
```

- `6973` is `GRAPHENE_OSC`. Before implementing, check it is unused in xterm's
  ctlseqs, kitty, iTerm2, WezTerm and foot; pick another number otherwise.
- `<token>`: 32 random hex characters per session, passed in `GRAPHENE_BLOCK_TOKEN`.
  The hook scripts copy it into a non-exported shell variable and unset the
  environment variable, so commands run from the shell (ssh, cat) never see it. A
  sequence with another token is passed through to the parser untouched.
- The command line is base64 so it may hold `;`, `BEL` or `ESC`. It is capped at
  64 KiB; longer ones are sent empty (copy then falls back to the header text).

Hooks, injected with the mechanisms that already exist
([shell_integration.rs:56](../../../src/shell_integration.rs)). None of them wraps,
reads or rebuilds the prompt:

- **zsh** (`.zshenv`): `preexec` emits `start` with `$1`; `precmd` emits `end` with `$?`,
  only when a command ran since the last prompt (`_graphene_command_ran`, already in
  the script). The `PS1` / `RPS1` wrapping, `zle-line-init` redraw and the
  powerlevel10k branch are removed, and `__p9k_force_term_shell_integration` is no
  longer set (commit `bf12533` is reverted): powerlevel10k's own 133 marks are now
  ignored.
- **bash** (`graphene.bash`): `PS0` emits `start`; the command line comes from
  `$(HISTTIMEFORMAT= builtin history 1)` with the number stripped, empty when history
  is off (to verify during implementation). The first `PROMPT_COMMAND` entry emits
  `end` with `$?` when a command ran. The `PS1` wrapping is removed.
- **fish** (`graphene.fish`): `fish_preexec` emits `start` with `$argv`,
  `fish_postexec` emits `end` with `$status`. fish's native 133 marks are ignored.
- `GRAPHENE_SHELL_INTEGRATION=0` still turns the injection off.

An empty command line or `Ctrl+C` at the prompt runs `precmd` but not `preexec`: no
`start`, so the `end` is not sent, and the new prompt is drawn below the old one in the
same header.

### Scanner: `src/osc133.rs` becomes `src/signals.rs` (pure)

`Scanner::split(bytes) -> impl Iterator<Item = Chunk>`, same incremental structure
(sequences split across reads, 4 KiB cap on an OSC):

- `Chunk::Signal(Signal::{Start { command: String }, End { status: Option<i32> }})`
  for `GRAPHENE_OSC` with the session token. The scanner is built with the token.
- `Chunk::Clear` for `CSI 2 J` and `CSI 3 J`, which needs a small CSI state: `ESC [`,
  parameter bytes, final byte. Only the final `J` with parameter 2 or 3 is reported.
- Every byte is still passed to the `vte` parser, the signal sequences included (`vte`
  drops unknown OSC, so they show nothing).
- `133;…` and `8;…` are no longer reported.

### Timeline: `src/timeline.rs` (pure, tested on `Term<VoidListener>`)

Replaces `blocks.rs` (`ShellState`, cell tags, row index, `BlockBounds`).

```rust
pub struct Timeline {
    blocks: VecDeque<Block>,          // finished blocks, oldest first
    current: Current,                 // the block whose grid is live
    next_id: u64,
    pub scroll: TimelineScroll,       // offset from the bottom, in half lines
}
pub struct Block {
    pub id: BlockId,
    pub header: Term<VoidListener>,   // prompt + typed command, frozen at `start`
    pub output: Term<VoidListener>,   // frozen at `end`
    pub command: String,              // from the `start` signal
    pub status: Status,               // Success | Failure(i32) | Unknown
    pub folded: bool,
    pub scroll: usize,                // output rows hidden below, 0 = bottom
}
pub struct Current {
    pub id: BlockId,
    pub phase: Phase,                 // Prompt | Running
    pub header: Option<Term<VoidListener>>, // frozen once running
    pub command: String,
    pub folded: bool,
    pub scroll: usize,
    pub pin_top: bool,                // set by a clear: show this block at the top
}
```

The live grid is the session's `Term` (`Terminal::term`); `Timeline` is locked after it
(`Terminal::shell` becomes `Terminal::timeline`, same order).

**Detaching the live grid** (`detach(term) -> Term<VoidListener>`):

1. `rows` = content rows: `history_size` + last line holding the cursor or a non-blank
   cell, plus one.
2. `let old = mem::replace(term.grid_mut(), Grid::new(screen_lines, columns,
   scrollback))`; the new grid gets `old.cursor` with its point reset to the origin, so
   the SGR template and charsets carry over.
3. `old` is resized to `rows` lines (`Grid::resize(false, rows, columns)`, the cursor
   first moved to the last content row so no content row is dropped) and placed in a
   `Term<VoidListener>` created with `rows` × `columns` (`*frozen.grid_mut() = old`),
   so its alternate grid is that small too.

**Signals**, applied by the PTY thread like the marks today (after `stop_sync` when a
synchronized update holds bytes):

- `Start { command }` in phase `Prompt`: the live grid is detached into
  `current.header`; phase `Running`.
- `End { status }` in phase `Running`: the live grid is detached into the output; the
  current block is pushed to `blocks` with its status; a new `Current` starts in phase
  `Prompt`. `End` in phase `Prompt` is ignored.
- A signal received on the alternate screen is kept and applied when the alternate
  screen is left (a program that never leaves it delays the block until `reset`).
- `Clear` on the main screen sets `current.pin_top`. When the block of a `clear`
  command ends with an empty output, `pin_top` moves to the next block, so the new
  prompt is at the top of the view.
- Without signals (unsupported shell, `ssh` to a remote host), nothing is ever
  detached: the current block keeps growing in the live grid and is drawn as one card,
  with no chevron and a neutral stripe.

**Eviction.** The frozen rows (headers + outputs) are capped at the scrollback size
(`scrolling_history`, [config.rs:337](../../../src/config.rs)); the oldest blocks are
dropped whole.

**Resize.** The live `Term` is resized as today. Frozen blocks keep the columns they
were reflowed for; a block is reflowed (`Term::resize` with its own row count) the next
time it is captured, selected or copied with other columns. A resize never runs over
every block at once.

### Block geometry and view: `src/view.rs` (pure)

- A block's rows: header rows, then output rows (none while `Prompt`). A folded block
  shows its header and one summary row ("342 lines hidden"). For the current block,
  output rows end at the live cursor row or the last non-blank row.
- `cap` = view lines minus the card padding. A block taller than `cap` shows its whole
  header, then the last `cap - header` output rows shifted up by `block.scroll`. A
  header taller than `cap` (rare) is cut to `cap - 1` rows, keeping its last rows (the
  command).
- `rows(timeline, live, lines) -> Vec<ViewRow>` walks the blocks from the bottom with
  `timeline.scroll`, as today but over blocks: each `ViewRow` gets `source:
  RowSource::{Header(BlockId), Output(BlockId), Live, Summary(BlockId)}`, the grid
  `Line` in that source, `top` and `gap_before` (half-line gaps unchanged). With
  `pin_top`, the view starts at the pinned block and leaves blank rows below it.
- The view stays anchored at the bottom while `timeline.scroll` is 0. When scrolled up,
  the offset grows by the rows added below, so the view does not move while output
  arrives (alacritty did this for us; now the growth of the current block's row count
  between two captures is added).
- The current block follows its new output while `current.scroll` is 0; otherwise its
  offset grows by the added rows, like the timeline.
- Alternate screen: the view is the live `Term`'s screen, full window, no card, no gap,
  as today.

### PTY loop: `src/pty.rs`

`process` keeps its shape: `Chunk::Bytes` to the parser, `Chunk::Signal` /
`Chunk::Clear` to `Timeline::apply(&mut term, …)`. The PTY size stays the window's
(`columns`, view lines): programs see the full window height, and a block shows what
fits under its cap and scrolls the rest.

### Frame and rendering: `src/frame.rs`, `src/renderer/scene.rs`

- `Frame::capture(live, timeline, view, …)` builds each row from its source: frozen
  rows by indexing the block's `Term` grid (`grid()[line]`), live rows and the cursor
  through `renderable_content` as today.
- `Card` is unchanged (`rows`, `clipped_top`, `status`, `chevron`, `selected`); `marks`
  now reads the blocks instead of the tags. A chevron is shown on a finished block with
  output, as before. `clipped_top` also covers a block shown from its middle.
- A scrollable block shows a thin scroll indicator on the card's right edge (the
  output part only), in the dim color.
- The card drawing from #70 and #72 (rounded rects, stripe, chevron disc and triangle)
  is unchanged.

### Mouse: `src/app.rs`, `src/layout.rs`

`Hit` gains the block: `Cell { block: RowSource, line, column, side }`.

- **Wheel** ([app.rs:663](../../../src/app.rs)): over a block whose output can scroll,
  scroll `block.scroll`, stop at its top end and reveal the whole block (its header
  would otherwise stay cut by the view top when the block is anchored at the bottom);
  at the bottom end of its output, scroll the timeline down instead; otherwise (short block, gap, margin,
  blank rows) scroll `timeline.scroll`. Over the live output with an application
  capturing the mouse, the wheel is reported to it as today.
- **Mouse reporting**: only over the live output rows (or the whole window on the
  alternate screen); the cell sent is the live grid's viewport cell under the pointer.
  Everywhere else, the mouse is Graphene's.
- **Text selection**: `TimelineSelection { start, end }`, each end a `(RowSource, Point,
  Side)`. For capture and copy it is split into one alacritty `Selection` per block
  involved (whole blocks in the middle), set on each block's `Term`. The copied text
  joins the parts with a line break and goes through `clean_copied_text`
  ([selection.rs:142](../../../src/selection.rs)). Auto-scroll at the view edges
  scrolls the timeline.
- Margin click, chevron click and its hand pointer, summary click: unchanged, mapped to
  the block of the row.

### Keyboard: `src/input.rs`, `src/app.rs`

- `Ctrl+Shift+PageUp/PageDown`: previous / next block, scrolled into view.
- `Shift+PageUp/PageDown`: scroll the selected block when it can scroll, otherwise the
  timeline, one view height. `Shift+Home/End`: top / bottom of the timeline.
- Copy actions: whole block = `command` + output text; command only = `command` (or
  the header's last rows when the signal had none); output only = output text.
  Folding: unchanged, auto-fold removed.

## Consequences checked against the code

- `blocks.rs` is replaced: cell tags (`BLOCK_LINK_SCHEME`), `RowRole`, `index`,
  `block_bounds` and the `after_bytes` re-tagging disappear. images-links no longer has
  to filter internal hyperlinks (its overview and technical notes are updated).
- `view.rs`, `frame.rs::marks`, `block_actions.rs` and the mouse / wheel paths of
  `app.rs` are rewritten on the timeline. `view::grid_delta` and `view::reveal` go away
  with `display_offset`.
- `osc133.rs` becomes `signals.rs`; the zsh and bash scripts lose the prompt wrapping
  and the zsh tests on prompt marks (`zsh_powerlevel10k_emits_the_marks`,
  `RightPrompt` checks) are replaced by tests on the start / end signals.
- rich-input-editor located the input from the `133;B` mark: it must be revisited (its
  overview says so). The live grid only holds the prompt and the input before `start`,
  which may help.
- Memory: each frozen block is a `Term` sized to its content (header and output), not
  to the window.
- Benchmarks ([benches/render.rs](../../../benches/render.rs)) build frames from one
  `Term`: they need a timeline fixture.

## Known limits

- zsh's `PROMPT_SP` mark after an output without a final newline (`%` in reverse
  video, then spaces up to the line end) is printed before the `precmd` hooks, so
  before `end`: it stays at the end of the command's own output, not in the next
  block's header (checked by `zsh_prompt_sp_mark_before_the_end`).
- A command that moves the cursor above its first output row hits the top of its own
  grid instead of rewriting the command line, unlike a classic terminal.
- bash reads the command line back from its history: when the line is not added
  (history off, `ignorespace`, `ignoredups`) the hook sends it empty, and copying the
  command falls back to the header text. zsh sends `$1`, also empty with the history
  off.
- A remote shell over `ssh` produces one block until the local shell signals again.

## Tests

- `signals.rs`: start / end with the token, a wrong token passed through, base64 and
  64 KiB cap, sequences split across reads, `CSI 2 J` / `3 J` detection, other CSI and
  OSC 133 ignored.
- `timeline.rs`: detach keeps content rows and the SGR template, header / output split,
  `End` without `Start` ignored, signals deferred on the alternate screen, eviction,
  `pin_top` after `clear` and after `Ctrl+L`, no signal = one block.
- `view.rs`: cap with pinned header, block scroll and its ends, timeline anchoring and
  stability while output arrives, folded summary, pinned top, alternate screen.
- `frame.rs` / `scene.rs`: rows from frozen and live sources, cards, scroll indicator.
- `app.rs` routing helpers kept pure where possible: wheel target, mouse reporting
  region, cross-block selection split and copied text.
- `shell_integration.rs`: real zsh (with powerlevel10k when installed), bash and fish
  sessions emit exactly one start and one end per command, nothing for an empty line,
  and the token is not in the command's environment.

## First implementation: what is kept

| Piece | Issue | Fate |
|---|---|---|
| Own PTY loop | #6 | Kept; `process` routes signals instead of marks. |
| OSC 133 scanner | #7 | Reworked into `signals.rs` (same state machine). |
| Injection and scripts | #8 | Injection kept; scripts reduced to the start / end hooks. |
| Shell state, cell tags, row index | #9 | Replaced by `timeline.rs`. |
| Margin, view rows, decorations | #10, #71 | Layout (margin, card padding, gaps, chevron disc) kept; view rewritten. |
| Block selection, navigation, copy | #11 | Behaviour kept, rewritten on the timeline. |
| Folding, chevron, summary | #12 | Kept, auto-fold removed. |
| Rounded rects, card frame, selected card | #70, #72, #73 | Kept. |

## Découpage en tâches d'implémentation

Rework tasks, in order (the first implementation's issues #6 to #12 and #70 to #73
close when the branch merges):

| Issue | Task | Depends on |
|---|---|---|
| [#74](https://github.com/marmotz/graphene-term/issues/74) | Private OSC signals scanner (`signals.rs`) | — |
| [#75](https://github.com/marmotz/graphene-term/issues/75) | Timeline model with live grid and frozen blocks (`timeline.rs`) | #74 |
| [#76](https://github.com/marmotz/graphene-term/issues/76) | View over the timeline with capped blocks | #75 |
| [#77](https://github.com/marmotz/graphene-term/issues/77) | Switch the PTY loop and rendering to the timeline | #76 |
| [#78](https://github.com/marmotz/graphene-term/issues/78) | Start and end hooks for zsh, bash and fish | #74, #77 |
| [#79](https://github.com/marmotz/graphene-term/issues/79) | Block and timeline scrolling, mouse routing | #77 |
| [#80](https://github.com/marmotz/graphene-term/issues/80) | Cross-block selection, block actions and copy | #79 |
