# Architecture

## Stack

| Concern                           | Crate                                                    | Version |
|-----------------------------------|----------------------------------------------------------|---------|
| Window, input events              | `winit`                                                  | 0.30    |
| GPU                               | `wgpu`                                                   | 30      |
| Font loading, fallback, raster    | `cosmic-text` (glyphs drawn by our own wgpu pipeline)    | 0.19    |
| PTY, VT parsing, grid, scrollback | `alacritty_terminal`                                     | 0.26    |
| Clipboard                         | `arboard` (X11 + Wayland data control, no image support) | 3.7     |
| Config file                       | `toml` + `serde`                                         | 1       |
| Update check (HTTP, JSON, tags)   | `ureq` (rustls) + `serde_json` + `semver`                | 3, 1, 1 |

## Modules

```
src/
  main.rs          binary entry point: --version, logger setup, calls lib::run
  lib.rs           library root (modules, VERSION, run); `bench` re-exports for benches/
  perf.rs          startup timing logs (`graphene::perf` target)
  app.rs           winit ApplicationHandler, Session (one window + one shell)
  banner.rs        top banner: content, priority, update banner actions, pixel layout, hit-testing
  config.rs        config.toml location, model, key-by-key parsing and validation
  terminal.rs      PTY spawn, Term state, event proxy to the UI thread
  pty.rs           PTY thread: poller, write queue, read, split (`process`) and parse (`feed`) into Term
  osc133.rs        incremental OSC 133 / OSC 8 scanner over the PTY byte stream (pure)
  blocks.rs        ShellState (phase, blocks, status), cell tags, row roles, block bounds (pure)
  block_actions.rs target block, block navigation, block selection, copied command / output (pure)
  view.rs          view rows: grid line, block role and hidden rows of each row (folds),
                   scroll deltas across folds, mouse point mapping (pure)
  shell_integration.rs  zsh / bash / fish integration: scripts written at startup, injection env
  frame.rs         snapshot of the visible grid with resolved colors and block marks
  i18n.rs          UI language detection (LC_ALL, LC_MESSAGES, LANG) and label translations
  input.rs         key -> bytes encoding, app shortcuts, paste wrapping
  palette.rs       Palette (terminal, selection and UI colors), 256-color table, color resolution
  theme.rs         theme files: key-by-key parsing, built-in themes, name resolution
  layout.rs        pixels <-> grid geometry, card margins, rows in half-lines, hit-testing
  opener.rs        opens a URI with xdg-open from a detached thread
  selection.rs     click counting, mouse capture rule, selection updates, copied text cleanup
  update/
    mod.rs         GitHub release check, release selection, notice and scheduling decisions
    state.rs       update.json state file (last check, latest release, ignored tag)
    checker.rs     update-check thread, pure tick (check or reuse the cache, next wake-up)
  renderer/
    mod.rs         wgpu device/surface, frame orchestration, headless renderer (benches)
    scene.rs       Frame -> rects + glyph instances (pure, no GPU)
    glyphs.rs      font system, cell metrics, glyph cache (char, bold, italic) -> atlas slot
    atlas.rs       shelf allocator of the mask and color atlas pages (pure, no GPU)
    cells.rs       instanced glyph pipeline: atlas textures, uploads, one quad per glyph
    cell.wgsl      shader for glyphs (mask tinted with the cell color, or color glyph)
    rects.rs       instanced solid rectangle pipeline
    rect.wgsl      shader for rectangles
themes/            built-in themes, embedded with include_str!
shell-integration/ zsh, bash and fish scripts emitting OSC 133 marks, embedded with include_str!
benches/
  render.rs        criterion benchmarks: parse, frame, glyphs_cold/warm, gpu_prepare
  fixtures/        recorded PTY output (btop, cat, vim) and the recorder script
```

Pure modules (`banner`, `blocks`, `block_actions`, `osc133`, `view`, `config`, `palette`, `theme`, `input`, `layout`, `frame`, `selection`, `renderer/scene`, `renderer/atlas`) hold the logic
and are unit tested. GPU, window and HTTP code (`update::fetch_latest`) stay thin.

## Threads

```
UI thread (winit)                         PTY thread (pty::PtyLoop)
-----------------                         -------------------------
keyboard / mouse  --pty::Sender(Msg)-->   writes to PTY (WriteQueue)
                                          reads PTY, osc133::Scanner splits it,
                                          pty::feed parses VT into Term,
                                          ShellState::apply handles each mark
request_redraw   <--UserEvent::Terminal-- EventProxy::send_event(Wakeup, Title, ...)
RedrawRequested:
  lock Term (FairMutex)
  Frame::capture  (short critical section)
  unlock
  Renderer::render(frame)
```

- `Term` is shared as `Arc<FairMutex<Term<EventProxy>>>`. The UI thread only holds the
  lock to read the mode, scroll, resize or capture a frame.
- `ShellState` (`src/blocks.rs`) is shared as `Arc<Mutex<ShellState>>` in `Terminal`.
  Lock order, everywhere: `Term` first, then `ShellState`.
- Terminal events (`Wakeup`, `Title`, `PtyWrite`, clipboard and color queries,
  `ChildExit`...) are forwarded to winit through an `EventLoopProxy<UserEvent>`.
- When the shell exits, the event loop exits and the window closes.
- The PTY thread (`src/pty.rs`) is Graphene's own, with the structure of alacritty's
  `EventLoop`: one `polling::Poller` for the PTY, the child exit and the channel;
  `Msg::{Input, Resize, Shutdown}` sent through `pty::Sender` (mpsc + `poller.notify()`);
  a `WriteQueue` resumed on partial writes; `EIO` on read ignored until the child exit
  event; synchronized updates flushed with `stop_sync` on their timeout. Each read goes
  through `pty::process`: `osc133::Scanner` splits it into byte chunks and marks, each
  byte chunk is parsed by `pty::feed` (the single parse step), and each mark is applied
  by `ShellState::apply` right after the bytes of its sequence, so the cursor is where
  the shell put it (a synchronized update holding bytes is flushed first). The `Term` lock is leased before reading and held for at most 64 KiB of parsing.
- The `update-check` thread (started in `App::resumed` when `updates.check` is true)
  wakes up when a check is due, reloads `update.json`, calls GitHub only when the last
  check is one interval old (otherwise reuses the cached release), saves the state and
  sends `UserEvent::Update(Option<Release>)` when the release to show changes. The
  session stores it and requests a redraw. `Checker::apply` forwards new `[updates]`
  settings through an `mpsc` channel (`recv_timeout` is the sleep); `check = false` ends
  the thread, `true` again restarts it. A failed check is retried after one hour.
- The update banner shows once the configuration error banner is gone. Its click
  targets are hit-tested in `left_press` before any selection: "Open release" runs
  `opener::open`, "×" hides the banner until a newer release, "Ignore this version"
  hides it and sends `Command::Ignore` to the thread, which writes `ignored` in
  `update.json` (written directly when the thread is not running).

## Command blocks

- `ShellState::apply` turns the OSC 133 marks into blocks: `A` starts a block (on a
  fresh line when the cursor is not at column 0; a prompt drawn again in place keeps
  its block), `B` starts the input, `C` the command (`Status::Running`), `D;<code>` ends
  it (`Success` / `Failure(code)`). A block still running when the next one starts
  becomes `Unknown`. Marks are ignored on the alternate screen.
- During the prompt and the input, the cursor template carries a hyperlink
  `graphene-block://<id>/{prompt,right-prompt,input}` (`blocks::BLOCK_LINK_SCHEME`), so every
  cell the shell writes is tagged. The tag is applied again when something removed it
  (a real OSC 8 link closed inside the prompt, a reset) and dropped outside them.
- `blocks::index(term, lines)` gives each row a `RowRole` (`None`, `Prompt`, `Input`,
  `Output`) from its first tagged cell; untagged rows continue the block above.
  `blocks::block_bounds(term, id)` gives the first row, the command cells and the
  output rows of a block.
- `view::rows(term, shell, display_offset)` lists the view rows: the grid line each
  one shows, its `RowRole`, the folded rows hidden right below it, its offset from the
  view top in half-lines (`top`) and whether a gap is above it (`gap_before`). Rows have
  no role on the alternate screen or while no block is known, and then no row index is
  built. `Frame::capture` and the mouse (`GridLayout::hit` / `point_at`, then
  `view::point`) both go through these rows.
- The view height is a budget of `2 * screen_lines` half-lines: a row takes 2, the
  first row of a block 1 more when a row of another block shows above it (the gap
  between two cards). Whole rows only, the leftover at the top. At the top of the
  history, when rows would be cut there, the view starts from the top grid line instead
  (unless that cuts the cursor row shown at the bottom). Without blocks, no gap: the
  plain viewport.
- Folding (`BlockInfo::folded`) is a row filter over alacritty's `display_offset`: the
  view starts from the bottom grid line of alacritty's viewport and collects rows
  upward, skipping the output rows of folded blocks above the cursor; at the top of the
  history it completes downward, then pads with blank rows (`ViewRow::line` is `None`).
  Scrolls in visible rows (mouse wheel, `Shift+PageUp/PageDown` with a page of the
  current view's row count, selection auto-scroll) go through `view::grid_delta`, which adds the hidden rows
  crossed; `Shift+Home/End` are unchanged.
- A block is folded automatically on `D` when its output has more rows than the
  window. `Ctrl+Shift+F` toggles the fold of the target block; `Ctrl+Shift+Alt+F` folds
  every finished block when any is unfolded, otherwise unfolds them all. A click on the
  chevron disc (`Hit::Chevron`, on the card's top-right corner) toggles the fold, a
  click on the summary unfolds. Folding is visual
  only: a block selection and the copy actions still include the hidden rows.
- The window holds, from the left: the padding, a margin of half a cell, half a cell of
  card padding, the grid, the card padding, then a right reserve (the padding, or the
  chevron radius when larger), always reserved. The consecutive view rows of one block,
  up to the cursor row, form a card (`frame::Card`): a rounded background (the
  background blended 6 % toward the foreground, 14 % when selected) behind the grid and
  its card padding, a status stripe in the margin (green, red, yellow, or
  `DimForeground` for `Success`, `Failure`, `Running`, `Unknown`; none for the neutral
  card of the active prompt), and, on a finished block with output whose first command
  row is shown, a fold chevron on the card's top-right corner: a disc in the card color
  with a triangle (right folded, down unfolded) in the status color. A card cut by the
  view top keeps its top corners above the view. Half a line separates two cards. The
  last command row of a folded block gets a dim summary (`i18n::Msg::LinesHidden`) two columns after its last non-blank cell, cut at
  the grid edge. Rows below the cursor get no mark. `GridLayout::hit` tells a margin
  pixel, a chevron disc, a gap between cards and a grid cell apart; a margin click
  selects the block, a gap click does nothing.
- Block actions (`block_actions`): the target block is the one holding the selection
  start, otherwise the last finished block (`ShellState::is_finished`: any block but the
  one being drawn or run). A block selection is a `Simple` selection from the start of
  the command to the end of the block's last row, with a `SelectionSnapshot` and the
  primary selection written, as after a mouse release. A margin click selects the
  block of its row; `Ctrl+Shift+PageUp/PageDown` select the finished neighbour among
  `blocks::blocks_in_grid` (the last finished block on the first press) and scroll by
  `view::reveal` (through `view::grid_delta`) until its first row is visible. `Ctrl+Shift+Alt+C` / `O` copy the
  cleaned command (`input_start..=input_end`) / output rows; an empty part leaves the
  clipboard untouched.

## Rendering pipeline

1. `Frame::capture` reads the grid lines given by `view::rows` (colors overrides,
   selection and cursor from `term.renderable_content()`) and produces one `FrameCell`
   per visible cell, plus one `RowDecoration` (hidden rows summary) per row and the
   block cards (`Frame::cards`, by `frame::marks`): character, final fg/bg (after DIM, INVERSE, HIDDEN, OSC overrides),
   bold/italic/underline/strikeout, wide-char spacer flag.
   The block cursor is baked into the cell colors; other shapes (beam, underline,
   hollow when unfocused) are returned separately.
2. `scene::build` converts the frame into:
    - `under` rects: the card backgrounds (radius a quarter of a cell height) and
      status stripes (a sixth of a cell wide, at least 2 px, round ends), then the cell
      backgrounds, consecutive same-color cells merged, default background skipped (it
      is the clear color, so the card shows through); rows are placed at
      `GridLayout::row_top` of their view offset (`Frame::row_tops`, in half-lines);
    - `over` rects: underline, strikeout, the fold chevrons (disc and triangle, SDF
      shapes of the rect pipeline), non-block cursor;
    - glyph instances: one per drawn cell (spaces and wide-char spacers skipped),
      anchored at the cell origin, plus the fold summaries (one character per cell);
    - overlay glyph instances at pixel positions (banner).
3. `Renderer::render` resolves the glyph instances through the glyph cache, uploads the
   newly rasterized glyphs, then draws in one pass: clear, `under` rects, glyphs,
   `over` rects, overlay glyphs. Colors are linearized when the surface format is sRGB.

### Banner

`Frame.banner` holds a `BannerLayout` (from `banner.rs`) and its theme colors
(`ui.banner_background`, `ui.banner_text`). It covers the top of the window, over the
grid (the grid does not move): its rect is the last `over` rect and its text goes to the
overlay glyphs (one character per cell width), drawn after it. The layout gives the pixel rect of each action,
and `Session::left_press` hit-tests the banner before starting a selection.

The configuration error banner shows the first error of `config.toml` and the theme
file ("(+N more)" when there are several); every error is also logged. A click hides it.

### Glyphs

A cell holds one character, so nothing is shaped per frame. `Glyphs` (`glyphs.rs`)
resolves each `(char, bold, italic)` once, by shaping that single character with
`Shaping::Advanced` (font fallback included), rasterizes it once and places it in the
atlas (`atlas.rs`: an `R8Unorm` mask page and an RGBA color page for emoji).

`CellRenderer` (`cells.rs`) turns each glyph instance into one quad at the cell origin
plus the glyph placement, rounded to whole pixels, and copies the pixels of new glyphs
into the atlas textures. A glyph wider than its cell (fallback fonts, CJK, symbols)
overflows the cell instead of shifting the rest of the line. When the atlas is full it
is reset with the glyph cache, and the frame is resolved again.

## Input

- `input::action_for` checks app shortcuts first (paste, scrollback, font size).
- Otherwise `input::key_to_bytes` encodes the key following xterm conventions:
  modifier parameter `1 + shift + 2*alt + 4*ctrl`, application cursor mode (`SS3`),
  Alt as ESC prefix, Ctrl+char control codes.
- Any input scrolls the view back to the bottom.
- Mouse wheel: scrollback on the primary screen, arrow keys on the alternate screen
  when the application enabled alternate scroll.

### Mouse selection

- `layout::point_at` converts the cursor pixel to a viewport cell and side (the margin
  clamps to the first column), then `view::point` to a grid point through the view
  rows (the view may be scrolled).
- Left press: `ClickTracker` counts clicks (400 ms, same cell) and `selection::
  start_selection` creates a `Simple` (or `Block` with `Ctrl`), `Semantic` or `Lines`
  selection in `Term::selection`, or extends it with `Shift`.
- `CursorMoved` while pressed updates the selection. Left release clears an empty
  selection (simple click), otherwise writes the cleaned text to the primary selection.
- Auto-scroll: in `about_to_wait`, while dragging with the cursor above or below the
  grid (`layout::vertical_overflow`), the display scrolls by `selection::
  auto_scroll_delta` lines (1 plus 1 per cell height of overflow, capped at 10) every
  50 ms (`ControlFlow::WaitUntil`) and the selection end follows the edge row. Disabled
  on the alternate screen, which has no history.
- Middle press pastes the primary selection through `input::paste_bytes` and, like
  every paste, clears the selection.
- Text change: the release stores a `selection::SelectionSnapshot` in the `Session`.
  On each `TermEvent::Wakeup`, `SelectionSnapshot::validate` restores the selection if
  `Term` erased it, then clears it (and drops the snapshot) when its cleaned text
  differs from the snapshot. Every place that clears the selection drops the snapshot.
- Left and middle clicks are ignored when the application enabled mouse reporting,
  unless `Shift` is held (`selection::mouse_reserved`).
- A left press in the margin (`layout::hit` gives `Hit::Margin`, the status stripe) or
  on a chevron disc (`Hit::Chevron`) is checked first and is always Graphene's, even
  under mouse reporting: the chevron toggles the fold of a card that has one, the
  margin selects the block of its row (see Command blocks), elsewhere it is ignored. A left press on a fold summary unfolds its block instead of selecting.
