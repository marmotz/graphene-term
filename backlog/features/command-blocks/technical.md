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

## Card rendering (redesign)

The first rendering (#10, #12) put a one-cell gutter mark, a 1 px separator over the
grid and a font chevron on the command row. Tried with a real powerlevel10k prompt, it
read as console content, and the chevron glyph (`▸`, from a fallback font, under a
status bar of the same color) was a few pixels wide. The overview now asks for cards.
This section supersedes, for rendering only, the bar / separator / chevron parts of
"Layout: gutter", "Frame and rendering" and "Mouse" above. Shell state, row index,
folding, selection and copy are unchanged.

### Starting point

- Rows are evenly spaced: `GridLayout::cell_origin` is `padding + line * cell.height`
  ([layout.rs:59-64](../../../src/layout.rs)), and `point_at`, `hit` and
  `vertical_overflow` invert that ([layout.rs:72-110](../../../src/layout.rs)).
- `view::rows` collects exactly `screen_lines` rows upward from the viewport bottom,
  skipping folded rows, then completes downward and pads
  ([view.rs:118-188](../../../src/view.rs)).
- Decorations are per row: `RowDecoration { bar, separator, chevron, summary }`
  ([frame.rs:61-70](../../../src/frame.rs)), drawn by `scene::build` as a gutter rect,
  a 1 px rect and a glyph ([scene.rs:107-151](../../../src/renderer/scene.rs)).
- The rect pipeline draws plain axis-aligned quads with alpha blending, no corner
  radius and no shape ([rect.wgsl](../../../src/renderer/rect.wgsl),
  [rects.rs:12-18, 101](../../../src/renderer/rects.rs)).
- A block selection is an ordinary `Simple` selection, highlighted cell by cell by
  `Frame::capture` ([frame.rs:141-161](../../../src/frame.rs)); the app keeps no
  "selected block" ([app.rs:284-327](../../../src/app.rs)).
- Default window padding is 8 logical pixels and may be set to 0
  ([config.rs:255, 428-430](../../../src/config.rs)).

### Alternatives considered

| Topic | Options | Retained |
|---|---|---|
| Gaps vs fixed PTY size | Shrink the PTY when cards add gaps / keep the PTY size and push rows into the history | Keep the PTY size (user decision): the gaps push the top rows above the view, reachable by scrolling. Resizing the PTY per block would reflow and redraw full-screen programs. |
| Leftover height at the top | Partial top row, clipped / whole rows only, blank leftover | Whole rows (user decision): integer layout in half-lines, exact mouse mapping, no scissor. |
| Vertical unit | Pixels / half-lines | Half-lines: a row is 2 units, a gap 1, so `view` stays pure and integer; pixels only in `layout`. |
| Rounded corners and chevron | Icon font embedded / SDF shapes in the rect shader | SDF in the rect shader (user decision): crisp at any size, no font dependency, antialiased. |
| Horizontal inner padding | None / half a cell each side | Half a cell (user decision): text never touches the rounded corners. Columns are fixed once at layout time. |
| Card colors | New theme keys / derived from the palette | Derived (user decision): blend of background and foreground, overridable later by config-themes. |
| Selected block highlight | Cell highlight / card accent | Card accent (overview), with the cell highlight skipped for a block selection only. |

### Layout: `src/layout.rs`

From the left: `padding`, margin (status stripe), card padding, grid, card padding,
right reserve.

- `margin` = `round(cell.width / 2)`: holds the stripe; replaces `gutter`.
- `card_pad` = `round(cell.width / 2)` on each side of the grid.
- `chevron_radius` = `round(cell.height * 0.35)`. The chevron is centered on the
  card's top-right corner, so it needs `chevron_radius` free pixels right of the card:
  `right_reserve = max(padding, chevron_radius)`. With the default padding (8 px) and a
  14 px font, no extra column is lost.
- `columns = fit(width - padding - margin - 2 * card_pad - right_reserve)`. Always
  reserved, with or without blocks, alternate screen included: the column count never
  changes mid-session.
- `grid_left() = padding + margin + card_pad`.
- `lines` (PTY lines) unchanged: `fit(height)` in whole rows.
- Vertical placement moves out of `GridLayout`: `row_top(units) = padding + units *
  cell.height / 2`, where `units` comes from the view (below). `point_at`, `hit` and
  `vertical_overflow` take the view geometry (`&[ViewRow]`) to map `y` to a view row.
- `Hit` gains `Margin { row }` (replaces `Gutter`), `Chevron { row }` (inside the
  chevron circle), `Gap` (between cards: ignored) and keeps `Cell`.

### View: `src/view.rs`

- `ViewRow` gains `top: u32`, its offset from the view top in half-lines, and
  `gap_before: bool`.
- `rows` takes a height budget of `2 * screen_lines` half-lines instead of a row count.
  Walking up from the viewport bottom (folds still skipped), each row costs 2 units, and
  a block's first row costs 1 more when a row of another block (or a blank row) shows
  above it. The walk stops when the next row does not fit; the leftover units (0 or 1,
  2 only when padding is needed) go to the top. `top` is assigned after the walk.
- On the alternate screen or without blocks: no gap, so exactly today's viewport.
- `grid_delta` and `reveal` keep their contract in grid lines. A page
  (`Shift+PageUp/PageDown`) becomes "the number of rows of the current view", which is
  `screen_lines` without blocks.
- `point(rows, row_index, column)` unchanged; the row index now comes from `y` through
  the view geometry.

### Frame: `src/frame.rs`

- `RowDecoration` keeps `summary` only. `bar`, `separator` and `chevron` move to cards.
- `Frame` gains `cards: Vec<Card>` and `row_tops: Vec<u32>` (from the view):

  ```rust
  pub struct Card {
      pub rows: Range<usize>,       // view rows covered
      pub clipped_top: bool,        // the block continues above the view
      pub status: Option<Status>,   // None: active prompt, neutral
      pub chevron: Option<Fold>,    // finished block with output
      pub selected: bool,
  }
  ```

  A card covers the consecutive view rows of one block up to the cursor row (as the bar
  does today). The active prompt card (current block, phase `Prompt` / `Input`) has
  `status: None`.
- `Frame::capture` takes `selected_block: Option<BlockId>`. When the selection is that
  block's selection, its cells are not highlighted and its card gets `selected`.
- `BlockColors` gains `card` and `card_selected`: background blended toward the
  foreground by 6 % and 14 %. Status colors unchanged.

### Rect pipeline: `src/renderer/rects.rs`, `rect.wgsl`

`RectInstance` gains `radius: f32` and `shape: u32` (0 rectangle, 1 triangle right,
2 triangle down), padded to keep `Pod`. The fragment shader computes a signed distance
from the local pixel position (passed from the vertex shader): rounded box for shape 0,
triangle for 1 and 2. Coverage is `clamp(0.5 - d, 0, 1)` times the color alpha (1 px
antialiasing). A rect with `radius: 0, shape: 0` renders as today, so cell backgrounds,
cursor and underlines are unaffected.

### Scene: `src/renderer/scene.rs`

Per card, in `under` (below cell backgrounds and glyphs):

1. Card background: from `grid_left - card_pad` to `grid_right + card_pad`, rows'
   `top..bottom`, `radius = round(cell.height / 4)`, color `card` or `card_selected`.
   A card clipped at the top starts `radius` above the view so its top corners stay
   off screen.
2. Status stripe: in the margin, `round(cell.width / 6)` wide (at least 2 px), full card
   height, rounded ends, status color; none for a neutral card.
3. Chevron (in `over`, above the glyphs): a disc of `chevron_radius` in the card color,
   centered on the card's top-right corner, then a triangle (shape 1 folded, 2
   unfolded) of about half its size, in the status color, or the dim color for a
   neutral or `Unknown` status.

Cell backgrounds equal to the terminal background are already skipped, so the card
shows behind them. The summary run and the cursor are unchanged, placed with
`row_top`.

### Mouse: `src/app.rs`

- `mouse_press` uses `layout.hit(x, y, &view)`:
  - `Chevron`: toggle the fold (replaces the chevron-row test in `gutter_press`);
  - `Margin`: select the block, as `gutter_press` does today;
  - `Gap`: nothing;
  - `Cell`: unchanged (summary click, selection, mouse reporting).
- `Session` gains `selected_block: Option<BlockId>`, set by a margin click and
  `PreviousBlock` / `NextBlock`, cleared by `clear_selection`, by any new mouse
  selection and when `validate_selection` drops the snapshot.
- `mouse_point`, `mouse_cell` and `auto_scroll` map `y` through the view geometry
  built for the last frame (recomputed under the lock as today).

### Consequences checked against the code

- About one more column lost than with the one-cell gutter (`margin + 2 * card_pad` ≈
  1.5 cells, plus `right_reserve - padding` when the padding is smaller than the
  chevron radius).
- Fewer rows visible at once when blocks are shown: one half-line per card boundary.
  The PTY size never changes.
- `RowDecoration` loses three fields: frame and scene tests about bar, separator and
  chevron are rewritten on `cards`.
- `GridLayout::hit` / `point_at` / `vertical_overflow` change signature; their callers
  are `mouse_press`, `mouse_point`, `mouse_cell` and `auto_scroll` in `app.rs`.
- Mouse reporting to applications uses `mouse_cell`: on the alternate screen the view
  has no gap, so reported cells are unchanged.
- The banner draws over the grid and is unaffected.

### Tests

- `layout.rs`: column count with margin, card padding and right reserve; `grid_left`;
  `hit` for margin, chevron disc, gap and cell with a view containing gaps.
- `view.rs`: budget in half-lines with 0, 1 and several card boundaries; leftover at
  the top; no gap without blocks or on the alternate screen; folds and gaps together.
- `frame.rs`: cards for finished, failed, running and active-prompt blocks; card clipped
  at the top; chevron only on finished blocks with output; block selection marks the
  card and leaves the cells unhighlighted, a mouse selection still highlights them.
- `scene.rs`: card background, stripe and chevron instances (positions, radius, shape,
  colors); no card on a frame without blocks.
- `renderer/mod.rs` (headless GPU, skipped without adapter): a rounded card leaves its
  corner pixel at the background color; a triangle chevron lights pixels inside its
  disc only.

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

Card rendering redesign:

| Order | Issue | Task | Depends on |
|---|---|---|---|
| 6 | [#70](https://github.com/marmotz/graphene-term/issues/70) | Rounded corners and vector shapes in the rect pipeline | - |
| 6 | [#71](https://github.com/marmotz/graphene-term/issues/71) | Card layout and view geometry with gaps | - |
| 7 | [#72](https://github.com/marmotz/graphene-term/issues/72) | Card frame and scene rendering | #70, #71 |
| 8 | [#73](https://github.com/marmotz/graphene-term/issues/73) | Selected block shown as a highlighted card | #72 |
