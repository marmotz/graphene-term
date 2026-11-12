# Command blocks: technical design

Product decisions: [overview.md](overview.md).

## Starting point

- `alacritty_terminal` 0.26 does not handle OSC 133: `vte` 0.15 `osc_dispatch` logs
  unknown OSC as `[unhandled osc_dispatch]` and drops them, with no handler callback.
- The shell runs under alacritty's `EventLoop` ([terminal.rs:70-72](../../../src/terminal.rs)).
  Its `pty_read` reads into a buffer, possibly several reads in a row while the `Term`
  lock is busy, then parses the whole buffer at once. The cursor position at the moment
  a mark is parsed is therefore unknown.
- `Term` exposes no scroll counter: a grid `Line` taken now is wrong after the next
  scroll, history eviction or reflow on resize. Only the selection is rotated by `Term`
  (`term/mod.rs` `scroll_up_relative` / `scroll_down_relative`).
- Cells carry an `extra: Option<Arc<CellExtra>>` holding an OSC 8 `Hyperlink`.
  `write_at_cursor` copies the cursor template's `extra` into every written cell, and
  `SGR 0` (`Attr::Reset`) keeps the template's hyperlink. Cells move with the grid on
  scroll, eviction and reflow.
- The view is alacritty's `display_offset` ([app.rs:105](../../../src/app.rs)).
  `Frame::capture` walks `renderable_content().display_iter`
  ([frame.rs:76-99](../../../src/frame.rs)), and mouse points go through
  `term::viewport_to_point` ([app.rs:156-162](../../../src/app.rs)).
- `GridLayout` places the grid at `padding` with no side area
  ([layout.rs:26-53](../../../src/layout.rs)).
- fish 4.9.3 (checked locally, `fish-terminal-compatibility(1)`) emits `OSC 133;A`
  (with `click_events=1`), `B`, `C` (with `cmdline_url=`) and `D;<status>` natively.
  zsh 5.9 and bash 5.3 emit nothing.

## Alternatives considered

| Topic | Options | Retained |
|---|---|---|
| Getting the mark position | Wrap the PTY reader under alacritty's `EventLoop` / own PTY loop / fork `alacritty_terminal` | Own PTY loop: the wrapper cannot know when its bytes are parsed. A fork contradicts "public API only" ([decisions.md](../../../docs/decisions.md)). |
| Anchoring blocks in the grid | Grid line + own scroll counter / per-row mark in a fork / cell tag through the template hyperlink | Cell tag (user decision): follows scroll, eviction and reflow for free, public API only. Cost: hyperlinks with a reserved scheme must be ignored by the links feature. |
| Block granularity | Cell ranges / whole rows | Rows for display, folding and navigation. Cells only for the command start and end, needed by copy and by rich-input-editor. |
| Folding | Own scroll position / alacritty `display_offset` plus a row filter at capture time | `display_offset` kept: alacritty keeps the view stable when output arrives while scrolled up. Graphene only skips hidden rows and converts scroll deltas. |
| Copy of a whole block | `Lines` selection / `Simple` selection from the command start | `Simple` from the command start: the prompt is never copied, and the highlight shows exactly what is copied. |
| Left margin | Always reserved / added with the first block | Always reserved (user decision): no reflow mid-session. |

## Design

### Shell integration scripts

New repo folder `shell-integration/` (`zsh/.zshenv`, `bash/graphene.bash`,
`fish/vendor_conf.d/graphene.fish`), embedded with `include_str!` and written at
startup to `$XDG_RUNTIME_DIR/graphene/shell-integration/`, or `std::env::temp_dir()`
when that variable is missing. The injection follows the mechanisms already set in
[rich-input-editor/technical.md](../rich-input-editor/technical.md):

- **zsh**: `ZDOTDIR` points to the integration folder, and the original value is kept
  in `GRAPHENE_ZDOTDIR`. The `.zshenv` there restores `ZDOTDIR`, sources the user's
  `.zshenv`, then installs the hooks:
  - `precmd` emits `D;$?`, except before the first prompt;
  - `PS1` is wrapped in `%{\e]133;A\e\\%}` … `%{\e]133;B\e\\%}`;
  - `RPS1` is wrapped in `%{\e]133;P;k=r\e\\%}` … `%{\e]133;B\e\\%}`;
  - `preexec` emits `C`.
- **bash**: the shell is started with `--posix` and `ENV` pointing to `graphene.bash`.
  The script turns POSIX mode off, sources the usual rc files like an interactive bash,
  then installs the hooks:
  - `PROMPT_COMMAND` emits `D;$?` first;
  - `PS1` is wrapped in `\[…A…\]` … `\[…B…\]`;
  - `PS0` emits `C` (bash 4.4 and later).
- **fish**: `XDG_DATA_DIRS` gets the integration folder prepended. fish emits the marks
  itself, so the script emits none and only carries rich-input-editor's bindings. For a
  fish without native marks, the script emits them from `fish_prompt`, `fish_preexec`
  and `fish_postexec`, guarded by a version check. The oldest fish version with native
  marks is to be verified during implementation: only 4.9.3 is checked here.
- Shell detection: basename of `$SHELL`. `tty::Options::shell` is set to
  `Shell::new(program, args)` in `Terminal::spawn` ([terminal.rs:50-60](../../../src/terminal.rs)).
  Other shells are started as today, with no injection.
- Interim opt-out until the configuration feature exists: `GRAPHENE_SHELL_INTEGRATION=0`.
  This follows `GRAPHENE_FONT` / `GRAPHENE_FONT_SIZE` ([app.rs:42](../../../src/app.rs)).

`OSC 133;P;k=r` (right prompt) is borrowed from kitty's extension. Graphene treats it
as a prompt start for the current block, so `RPROMPT` cells are tagged as prompt and
not as input. This also removes the `RPROMPT` limit listed by rich-input-editor.

### OSC 133 scanner: `src/osc133.rs` (pure)

`Scanner::split(&mut self, bytes) -> impl Iterator<Item = Chunk>` with
`Chunk::Bytes(&[u8])` and `Chunk::Mark(Mark)`:

- An incremental state machine for `ESC ] … (BEL | ESC \)`. A sequence split across
  two reads is still found.
- Only `133;…` and `8;…` are reported:
  - 133 gives `Mark::{PromptStart, RightPrompt, InputStart, OutputStart,
    CommandEnd(Option<i32>)}`;
  - 8 gives `Mark::Hyperlink`, used only to know the template may have changed.
- Options such as `aid=`, `click_events=` and `cmdline_url=` are ignored.
- An OSC longer than 4 KiB is abandoned.
- Every byte, marks included, is still passed to the `vte` parser, so other OSC
  sequences keep working.

### Shell state: `src/blocks.rs` (pure, tested on `Term<VoidListener>`)

```rust
pub struct ShellState {
    pub phase: Phase,                 // Idle | Prompt | Input | Running
    current: Option<BlockId>,
    next_id: u64,
    blocks: BTreeMap<BlockId, BlockInfo>, // status, folded
}
pub enum Status { Running, Success, Failure(i32), Unknown }
```

`ShellState::apply(&mut self, term: &mut Term<_>, mark)` runs in the PTY thread under
the `Term` lock:

- `PromptStart`:
  - when the cursor column is not 0, `carriage_return()` then `linefeed()` (OSC 133 `A`
    implies a fresh line), so every block starts on its own row;
  - a previous block still `Running` becomes `Unknown`;
  - allocate a new id, set the template hyperlink to the tag
    `graphene-block://<id>/prompt`, and set the phase to `Prompt`.
- `RightPrompt`: the tag becomes `prompt` again.
- `InputStart`: the tag becomes `graphene-block://<id>/input`, and the phase becomes
  `Input`.
- `OutputStart`: the template hyperlink is cleared, but only when it is our tag (a real
  OSC 8 link is never touched). The phase becomes `Running`.
- `CommandEnd(code)`: same clearing, and the status is set from `code`. Auto-fold applies
  when the block's output has more rows than `term.screen_lines()` (user decision:
  window height, until the configuration feature exists). The phase becomes `Idle`.
- After each `Bytes` chunk, when the phase is `Prompt` or `Input` and the template has
  no hyperlink, the tag is applied again. This covers a real OSC 8 link closed inside a
  prompt.
- Marks are ignored on the alternate screen (`TermMode::ALT_SCREEN`).
- `blocks` is capped at 10 000 entries (the scrollback size): the oldest ids are dropped
  first, because their rows have left the history by then.

`BLOCK_LINK_SCHEME = "graphene-block"` is public: the images-links feature must ignore
these hyperlinks.

**Row index.** `index(term, lines: Range<Line>) -> Vec<RowRole>` with
`RowRole::{None, Prompt(id), Input(id), Output(id)}`. A row takes the tag of its first
non-blank cell. A row with no tag continues the previous block as `Output`; rows before
the first block are `None`. A range that starts inside an output first walks up to the
nearest tagged row. The cost is one row-start check per row (about 10 000 for a full
history), done again on each wakeup only when it is needed.

**Block geometry.** `block_bounds(term, id)` returns:

- `first_row`;
- `input_start` / `input_end`: first and last cell tagged `input`;
- `output`: rows after the last `Input` row, up to the row before the next block, or up
  to the cursor row while the block runs.

`input_start` is the `ShellState` input point expected by rich-input-editor. It is
derived from the tags rather than stored as a rotated `Point`, with the same meaning.
Before anything is typed, it falls back to the cell after the last `prompt` cell.

### Own PTY loop: `src/pty.rs`

Replaces `alacritty_terminal::event_loop::EventLoop`. It keeps that loop's structure:

- a `polling::Poller` thread, using `polling` 3.11, already in `Cargo.lock` and to be
  added to `Cargo.toml`;
- a write queue, `Msg::{Input, Resize, Shutdown}`, `ChildExit`, and `EIO` ignored on
  Linux;
- synchronized updates through `Processor::sync_timeout` / `stop_sync`.

Read path:

1. Read a buffer.
2. Take the `Term` lock, then the `ShellState` lock (fixed order, everywhere).
3. For each `Chunk::Bytes`, call `processor.advance(&mut term, bytes)`.
4. For each `Chunk::Mark`: first `stop_sync` if a synchronized update holds bytes
   (`sync_bytes_count() > 0`), so the cursor is correct, then `shell.apply(&mut term,
   mark)`.
5. Send `Event::Wakeup` as today.

`Terminal` keeps `term: Arc<FairMutex<Term<_>>>`, gains
`shell: Arc<Mutex<ShellState>>`, and its `Notifier` becomes `pty::Sender` (mpsc sender +
`poller.notify()`). `alacritty_terminal::event_loop` is no longer used.

### Layout: gutter

`GridLayout` gains `gutter: f32`, one cell width, always reserved (user decision).

- `columns` = `fit(width - gutter)`.
- `cell_origin` x = `padding + gutter + column * width`.
- `hit(x, y) -> Hit::{Gutter { line }, Cell { column, line, side }}`.
- `point_at` stays for cells, and its clamping starts at the gutter's right edge.

### View with folding: `src/view.rs` (pure)

- `rows(term, shell, display_offset) -> Vec<ViewRow { line: Line, role: RowRole }>`:
  - start from the bottom grid line of alacritty's viewport and walk up;
  - skip `Output` rows of folded blocks, until `screen_lines` rows are collected;
  - at the top of the history, complete downward, then pad with blank rows.
  - Without folds or blocks, this is exactly today's viewport.
- `grid_delta(term, shell, display_offset, view_delta) -> i32` converts a scroll in
  visible rows into a `Scroll::Delta` in grid lines. It is used by:
  - the mouse wheel;
  - `Shift+PageUp/PageDown`, where a page is `screen_lines` visible rows;
  - selection auto-scroll ([app.rs:226-240](../../../src/app.rs)).
  `Top` and `Bottom` are unchanged.
- `point(view_rows, line_index, column) -> Point` replaces `viewport_to_point` in
  `mouse_point`.

### Frame and rendering

- `Frame::capture` takes the `ViewRow`s and reads `term.grid()[line]` directly instead
  of `display_iter`. Selection highlight and cursor are mapped through the same rows.
  On the alternate screen, the view is the plain viewport with no decoration.
- `Frame` gains a `RowDecoration` per row:
  - `bar: Option<Status>`;
  - `separator: bool`, on a block's first row;
  - `chevron: Option<Folded | Unfolded>`, on the first `Input` row;
  - `summary: Option<usize>`, the hidden row count of a folded block, on its last
    `Input` row.
- `scene::build` adds:
  - **bar**: a gutter rect a quarter of a cell wide, full row height, on every row of
    a block;
  - **separator**: a 1 px rect across the grid width at the top of the block's first
    row, in `DimForeground`;
  - **chevron**: `▸` / `▾` as a gutter text run, in the status color;
  - **summary**: `"<n> lines hidden"` as a dim text run two columns after the last
    non-blank cell of its row, cut at the grid edge.
- `TextRun` gains an optional pixel `x` for gutter runs. As implemented: `TextRun` was
  removed by the glyph pipeline rewrite (#69), whose `GlyphInstance` is already placed
  in pixels, so the chevron is a glyph at the gutter origin and the summary one glyph
  per cell; nothing was added to the renderer. Status colors:
  - `Success`: ANSI green;
  - `Failure`: ANSI red;
  - `Running`: ANSI yellow;
  - `Unknown`: `DimForeground`.
  A dedicated theme is left to config-themes.

### Mouse: `app.rs`

`left_press` ([app.rs:177](../../../src/app.rs)) checks `layout.hit` first. A gutter
hit is always Graphene's, even when an application captures the mouse, because the
gutter is outside the application's grid:

- chevron row: toggle the fold;
- any other row of a block: select the block;
- outside blocks: ignored.

Clicking the summary text unfolds the block.

Selecting a block:

1. Create a `Simple` selection from `input_start` (`Side::Left`) to the end of the
   block's last row (`Side::Right`).
2. Capture `SelectionSnapshot`.
3. Write the primary selection, as a mouse release does.

A folded block's hidden rows are inside the selection, so they are copied: folding is
visual only.

### Keyboard: `input.rs` / `app.rs`

New `Action`s in `action_for` ([input.rs:21-46](../../../src/input.rs)), with a
`ctrl_shift_alt` branch next to `ctrl_shift`:

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+PageUp` / `Ctrl+Shift+PageDown` | `PreviousBlock` / `NextBlock` |
| `Ctrl+Shift+Alt+C` | `CopyCommand` |
| `Ctrl+Shift+Alt+O` | `CopyOutput` |
| `Ctrl+Shift+F` | `ToggleFold` |
| `Ctrl+Shift+Alt+F` | `ToggleFoldAll` |

- **Target block**: the block that contains the start of the selection, otherwise the
  last finished block.
- **Previous / Next**: select the neighbour of the target block. The first press with
  no selection selects the last finished block. The view then scrolls with `grid_delta`
  until the block's first row is visible.
- **Copy**: `Ctrl+Shift+C` stays unchanged, and on a block selection it copies the
  command and the output. `CopyCommand` copies `bounds_to_string(input_start,
  input_end)`. `CopyOutput` copies the `output` rows. Both go through
  `selection::clean_copied_text` to the clipboard. An empty output leaves the clipboard
  untouched.
- **ToggleFoldAll**: folds every finished block if any is unfolded, otherwise unfolds
  them all.
- rich-input-editor intercepts keys before `action_for`. Its `editor_command` must not
  match `Ctrl+Shift+PageUp/PageDown` nor any `Ctrl+Shift+Arrow`, which belongs to pane
  focus ([tabs-splits-palette](../tabs-splits-palette/overview.md)).

### Docs

- `docs/architecture.md`:
  - new modules `osc133`, `blocks`, `view`, `pty`;
  - threads section: own PTY loop, lock order `Term` then `ShellState`.
- `docs/decisions.md`:
  - own PTY loop;
  - cell tags through hyperlinks;
  - gutter always reserved;
  - folding as a row filter over `display_offset`.
- `README.md`: block shortcuts and mouse actions, `GRAPHENE_SHELL_INTEGRATION=0`.
- `docs/vision.md`: roadmap row set to "Done".

## Consequences checked against the code

- One column fewer for every user, even without shell integration (gutter).
- `terminal.rs` loses the alacritty `EventLoop` and `Notifier`. `Terminal::write` /
  `resize` keep their signatures, so `app.rs` callers are unchanged.
- `Frame::capture` changes signature, so its tests move to the `ViewRow` input. With no
  block and no fold, the output stays identical (non-regression test).
- `mouse_point`, `mouse_wheel`, `auto_scroll` and the `ScrollPage*` actions go through
  `view`. Without folds, `grid_delta` returns its input unchanged.
- A shell started by hand inside Graphene, or an ssh session, emits no marks: no block,
  and the gutter stays empty.
- `clear` / `ESC c` erase cells and their tags, so the blocks vanish with them. Orphan
  `BlockInfo` entries are dropped by the cap.

## Known limits

- An empty row printed before a multi-line prompt (starship `add_newline`) is not
  written, so it carries no tag and belongs to the previous block's output.
- A mark received during a synchronized update ends the update early. Visible only if a
  redraw happens before the next wakeup.
- A prompt drawn on the alternate screen gets no block (by design).

## Tests

- `osc133.rs`:
  - BEL and ST terminators;
  - a sequence split across two and three reads;
  - options ignored, unrelated OSC passed through untouched;
  - overlong OSC abandoned;
  - `D` with and without a status.
- `blocks.rs`: feed byte streams to `Term<VoidListener>` + `ShellState`:
  - tags on prompt and input cells, none on output;
  - fresh line on `A` mid-row;
  - tag reapplied after an OSC 8 close inside a prompt;
  - a real OSC 8 link kept at `C`;
  - `RightPrompt` cells tagged as prompt;
  - index roles after scroll, history eviction and a resize with reflow;
  - `input_start` / `input_end`, auto-fold threshold, `Unknown` status, id cap.
- `view.rs`:
  - rows with zero, one and several folded blocks;
  - top-of-history completion;
  - `grid_delta` across a fold;
  - `point` mapping.
- `layout.rs`: gutter offset, `hit` in gutter and grid.
- `scene.rs`: bar, separator, chevron and summary rects or runs for a decorated frame.
- `input.rs`: the new shortcuts; `Ctrl+Shift+C/V` unchanged.
- Integration scripts: one test per shell, skipped when the shell is missing. Spawn it
  in a PTY with the injection and run `true` then `false`. Check the stream holds
  `A, B, C, D;0` then `A, B, C, D;1`. The user's rc files must still be sourced (marker
  variable set in a temporary `HOME`).

## Découpage en tâches d'implémentation

| Order | Issue | Task | Depends on |
|---|---|---|---|
| 1 | [#6](https://github.com/marmotz/graphene-term/issues/6) | Own PTY loop replacing alacritty's `EventLoop` | - |
| 1 | [#7](https://github.com/marmotz/graphene-term/issues/7) | OSC 133 stream scanner | - |
| 1 | [#8](https://github.com/marmotz/graphene-term/issues/8) | Shell integration scripts and automatic injection | - |
| 2 | [#9](https://github.com/marmotz/graphene-term/issues/9) | Shell state, cell tags and row index | #6, #7 |
| 3 | [#10](https://github.com/marmotz/graphene-term/issues/10) | Gutter, view rows and block decorations | #9 |
| 4 | [#11](https://github.com/marmotz/graphene-term/issues/11) | Block selection, navigation and copy actions | #10 |
| 5 | [#12](https://github.com/marmotz/graphene-term/issues/12) | Folding, chevron, summary and auto-fold | #11 |
