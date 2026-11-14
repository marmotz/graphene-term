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
| Command palette fuzzy search      | `nucleo-matcher`                                         | 0.3     |

## Modules

```
src/
  main.rs          binary entry point: --version, logger setup, calls lib::run
  lib.rs           library root (modules, VERSION, run); `bench` re-exports for benches/
  perf.rs          startup timing logs (`graphene::perf` target)
  app.rs           winit ApplicationHandler, Session (the window, its workspace and overlay)
  pane.rs          Pane: one shell and its view state (layout, scroll, selection, blocks)
  process.rs       /proc lookups: foreground process group, program name, current directory (pure parsing)
  workspace/
    mod.rs         Workspace: tabs, panes by PaneId, active tab and pane, zoom, title fallback (pure)
    tree.rs        binary split tree: split, remove, layout, neighbours, resize, equalise (pure)
  banner.rs        top banner: content, priority, update banner actions, pixel layout, hit-testing
  config.rs        config.toml location, model, key-by-key parsing and validation
  terminal.rs      PTY spawn, live Term, timeline and session token, event proxy to the UI thread
  pty.rs           PTY thread: poller, write queue, read, split (`process`) and parse (`feed`) into Term
  signals.rs       incremental scanner of the private block signals and screen clears (pure)
  graphics/
    mod.rs         Image (kitty / sixel sequence), Handler hook called by the PTY thread,
                   StoreHandler (clears the image store on a full reset)
    sequence.rs    incremental splitter of the kitty and sixel sequences in the PTY stream (pure)
    store.rs       ImageStore: images (RGBA frames, generation), placements, 256 MiB budget
                   with oldest-first eviction, terminal-assigned ids (pure)
    placeholder.rs Unicode placeholder cells: place an image at the cursor, erase it,
                   decode a row of cells into image cells (pure, on a Term)
    diacritics.rs  kitty's row / column diacritics table
  timeline.rs      command blocks: finished blocks frozen in their own Term, current block (pure)
  block_actions.rs target block, block navigation, copied block / command / output (pure)
  view.rs          view rows over the timeline: source grid and line of each row, caps,
                   block and timeline scrolls, mouse point mapping (pure)
  shell_integration.rs  zsh / bash / fish integration: scripts written at startup, injection env
  frame.rs         snapshot of the visible grid with resolved colors and block marks
  i18n.rs          UI language detection (LC_ALL, LC_MESSAGES, LANG) and label translations
  input.rs         key -> bytes encoding, app shortcuts, paste wrapping
  input_editor.rs  rich input editor: rich mode, prompt shortcuts, private key sequences, buffer distance, click moves, keyboard selection, selection replacement (pure)
  palette.rs       Palette (terminal, selection and UI colors), 256-color table, color resolution
  theme.rs         theme files: key-by-key parsing, built-in themes, name resolution
  layout.rs        pixels <-> grid geometry, card margins, rows in half-lines, hit-testing
  links.rs         OSC 8 and detected URL under a point, hover / click / open rules (pure)
  opener.rs        opens a URI with xdg-open from a detached thread
  selection.rs     click counting, mouse capture rule, selection updates, copied text cleanup
  chrome/
    command_palette.rs  palette entries, fuzzy matching, keys, layout, recent actions file (pure)
    dialog.rs      close confirmation: targeted panes, messages, box layout, hit-testing, keys (pure)
    scene.rs       interface layer: tab bar, dividers, active pane border, dialog or palette overlay (pure)
    tab_bar.rs     tab bar layout, hit-testing, dragged tab slots (pure)
  update/
    mod.rs         GitHub release check, release selection, notice and scheduling decisions
    state.rs       update.json state file (last check, latest release, ignored tag)
    checker.rs     update-check thread, pure tick (check or reuse the cache, next wake-up)
  renderer/
    mod.rs         wgpu device/surface, frame orchestration, headless renderer (benches)
    scene.rs       Frame -> rects + glyph instances (pure, no GPU)
    glyphs.rs      font system, cell metrics, glyph cache (char, bold, italic, ui) -> atlas slot,
                   interface font shaping
    ui_text.rs     interface text: truncation with an ellipsis, shaped line cache, glyph instances
    atlas.rs       shelf allocator of the mask and color atlas pages (pure, no GPU)
    cells.rs       instanced glyph pipeline: atlas textures, uploads, one quad per glyph
    cell.wgsl      shader for glyphs (mask tinted with the cell color, or color glyph)
    rects.rs       instanced solid rectangle pipeline
    rect.wgsl      shader for rectangles
    images.rs      textured image quad pipeline, one texture per image frame
    image.wgsl     shader for images
themes/            built-in themes, embedded with include_str!
shell-integration/ zsh, bash and fish scripts (block signals, editor key bindings), embedded with include_str!
benches/
  render.rs        criterion benchmarks: parse, frame, glyphs_cold/warm, gpu_prepare
  fixtures/        recorded PTY output (btop, cat, vim) and the recorder script
```

Pure modules (`banner`, `signals`, `graphics/sequence`, `graphics/store`, `timeline`, `block_actions`, `view`, `config`, `palette`, `theme`, `input`, `input_editor`, `layout`, `frame`, `selection`, `i18n`, `workspace`, `chrome`, `renderer/scene`, `renderer/atlas`) hold the logic
and are unit tested. GPU, window and HTTP code (`update::fetch_latest`) stay thin.

## Session and panes

- `Session` owns the window and what is shared by every pane: the renderer, the
  clipboard, the modifiers, the font size, the banners, the mouse gesture in progress
  (`Drag`) and the open overlay. Its `Workspace<Pane>` holds the tabs; each tab is a
  binary split tree (`workspace::tree::Node`) whose leaves are `PaneId`s, with its
  active pane and zoom flag.
- `Pane` owns everything about one shell: its `Terminal`, its `GridLayout` (the pane's
  rectangle), its scroll anchor, selection, selected block, mouse reporting state and
  title. It never touches the window: it raises a redraw flag and hands the text for
  the primary selection back, which `Session::flush_panes` applies after each event.
- Every terminal event carries the `PaneId` of its pane
  (`UserEvent::Terminal(PaneId, Event)`, set by `EventProxy`), so the session routes it
  to its pane; events of a closed pane are dropped. A redraw is only requested for a
  pane shown by the active tab.
- Keys go to the active pane; mouse presses and the wheel go to the pane under the
  pointer (a press in an inactive pane focuses it first). A press goes to the first of:
  the overlay, the banner, the tab bar, a divider, a pane.
- New panes (split, new tab) start their shell in the active pane's directory
  (`Terminal::current_dir`, from `/proc/<pid>/cwd`), else `$HOME`.

## Command palette

- `Ctrl+Shift+P` (`Action::CommandPalette`) opens `chrome::command_palette::
  CommandPalette` as the session overlay. Its entries are every `Action::ALL` (label
  from `i18n`, shortcut from `input::shortcut_label`), then each tab and, for a tab
  holding several panes, each of its panes.
- The query is matched with `nucleo-matcher` (fuzzy, smart case, Unicode
  normalization); the matches are sorted by score, then by rank in the recent actions,
  then in the default order, so an empty query lists the recent actions first. The
  matched characters are highlighted in the accent color.
- While it is open, keys and mouse presses go to it only: typed text edits the query,
  `Backspace`, `Up` / `Down` (wrapping), `Enter` runs the selected entry, `Escape` or a
  press outside the box closes it; a click on a row runs it, the wheel scrolls the rows.
  At most 12 rows are shown.
- Running an action goes through `Session::run_action`, so closing a pane still asks
  for confirmation. The action's id (`Action::id`, snake_case) is then written first in
  `$XDG_STATE_HOME/graphene/palette-recent` (default `~/.local/state/graphene/`), one id
  per line, at most 50; the file is read each time the palette opens, and a missing or
  unreadable file means no recent actions. Tab and pane entries are not stored.

## Threads

```
UI thread (winit)                         PTY thread (pty::PtyLoop)
-----------------                         -------------------------
keyboard / mouse  --pty::Sender(Msg)-->   writes to PTY (WriteQueue)
                                          reads PTY, graphics::sequence::Splitter
                                          cuts the image sequences out,
                                          signals::Scanner splits the VT bytes,
                                          pty::feed parses VT into Term,
                                          Timeline::apply handles each signal
request_redraw   <--UserEvent::Terminal-- EventProxy::send_event(Wakeup, Title, ...)
RedrawRequested, for each pane shown by the active tab:
  lock Term (FairMutex), then Timeline
  view::prepare + Frame::capture  (short critical section)
  unlock
Renderer::render(pane frames, interface rects)
```

- `Term` is shared as `Arc<FairMutex<Term<EventProxy>>>`. The UI thread only holds the
  lock to read the mode, scroll, resize or capture a frame.
- The `Timeline` (`src/timeline.rs`) is shared as `Arc<Mutex<Timeline>>` in `Terminal`,
  and so is the `graphics::ImageStore` (`Arc<Mutex<ImageStore>>`, written by the PTY
  thread, read when a frame is captured). Lock order, everywhere: `Term`, then
  `Timeline`, then `ImageStore`.
- Terminal events (`Wakeup`, `Title`, `PtyWrite`, clipboard and color queries,
  `ChildExit`...) are forwarded to winit through an `EventLoopProxy<UserEvent>`.
- When the shell of a pane exits, the pane closes, without confirmation; the last pane
  closes its tab, the last tab closes the window.
- Closing a pane (`ClosePane`), a tab (close button, middle click) or the window
  (`CloseRequested`) goes through `Session::request_close`: when a targeted pane runs a
  program other than its shell in the foreground (`Terminal::foreground_program`), a
  `chrome::dialog::ConfirmClose` opens as the session overlay instead. While it is open
  it takes every key (`Enter` closes, `Escape` cancels) and every mouse press (its
  buttons); the wheel is ignored.
- The PTY thread (`src/pty.rs`) is Graphene's own, with the structure of alacritty's
  `EventLoop`: one `polling::Poller` for the PTY, the child exit and the channel;
  `Msg::{Input, Resize, Shutdown}` sent through `pty::Sender` (mpsc + `poller.notify()`);
  a `WriteQueue` resumed on partial writes; `EIO` on read ignored until the child exit
  event; synchronized updates flushed with `stop_sync` on their timeout. Each read goes
  through `pty::handle_output`: `graphics::sequence::Splitter` first cuts the kitty
  graphics commands (`ESC _ G ... ESC \`) and sixel images (DCS with final `q`) out of
  the raw bytes, keeping its state across reads and passing every other APC / DCS
  through; for each image sequence, and after each full reset (`ESC c`), a synchronized
  update holding bytes is flushed and the `graphics::Handler` runs with the `Term` lock
  held: `graphics::StoreHandler` clears the pane's `ImageStore` on a full reset (the
  kitty and sixel decoders, still to come, log and drop the sequences). The VT bytes go through `pty::process`: `signals::Scanner` (built with the session token) splits it
  into byte chunks, block signals and screen clears; each byte chunk is parsed by
  `pty::feed` (the single parse step), and each signal or clear is applied by
  `Timeline::apply` right after the bytes of its sequence, so the live grid holds
  everything the shell wrote before it (a synchronized update holding bytes is flushed
  first, and its timeout flush also releases the signals held on the alternate screen). The `Term` lock is leased before reading and held for at most 64 KiB of parsing.
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

Design and rationale: [command-blocks technical design](../backlog/features/command-blocks/technical.md).

- Each block is a terminal of its own. The session's `Term` is the live grid: it holds
  the current block (`timeline::Current`). On a `start` signal the live grid is
  detached (`timeline::detach`) into the current block's frozen header (prompt and
  typed command); on `end` it is detached into its output and the block joins the
  finished ones (`timeline::Block`, oldest first), with its command line and status.
  Each detach hands the session a fresh grid, so a command can never redraw an older
  block. A frozen grid lives in a `Term<VoidListener>` sized to its content rows, so
  selection, `bounds_to_string` and reflow work on it unchanged.
- Signals received on the alternate screen wait until it is left. A screen clear on
  the main screen pins the current block at the top of the view (`pin_top`). Without
  signals (no hook, `ssh`), nothing is detached: the session is one block, drawn as one
  neutral card. The frozen rows are capped at the scrollback size, the oldest blocks
  dropped whole. Frozen blocks are reflowed for new columns only when the view shows
  them (`Timeline::reflow`, from `view::prepare`).
- The session token (32 random hex characters, made by `Terminal::spawn`) is the one
  the scanner expects in the signals. `shell_integration::prepare` passes it to the
  shell in `GRAPHENE_BLOCK_TOKEN`; each script moves it into a non-exported shell
  variable and unsets the environment variable, so commands run from the shell never
  see it. The hooks (zsh `preexec` / `precmd`, bash `PS0` / first `PROMPT_COMMAND`
  entry, fish `fish_preexec` / `fish_postexec`) send `start` with the base64 command
  line and `end` with the status, then `prompt` before each prompt (fish
  `fish_prompt`), for the rich input editor; they never read, wrap or rebuild the
  prompt.
- `view::rows(timeline, live, lines)` lists the view rows: the source each one takes
  its cells from (`RowSource::{Header, Output, Live, Summary}`), the line in that
  source, its block, its offset from the view top in half-lines (`top`) and whether a
  gap is above it (`gap_before`). A block never shows more than `view::cap` rows: its
  header, then the last output rows that fit, shifted by its own scroll. The scroll
  positions are Graphene's: `Timeline::scroll` (half-lines from the bottom) and one per
  block; the live grid stays at `display_offset` 0. `view::follow` keeps the view still
  while output arrives when scrolled up (its `Anchor` lives in the session).
  `view::prepare` (reflow, follow, rows) is what `Frame::capture` and the mouse
  (`GridLayout::hit` / `point_at`, then `view::point`) go through. On the alternate
  screen the view is the live screen, with no card.
- Folding shows a block's header and one summary row (`RowSource::Summary`, "N lines
  hidden") instead of its output. `Ctrl+Shift+F` toggles the fold of the target block;
  `Ctrl+Shift+Alt+F` folds every finished block when any is unfolded, otherwise unfolds
  them all. A click on the chevron disc (`Hit::Chevron`) toggles the fold, a click on
  the summary label unfolds. Nothing is folded automatically.
- The wheel (unless an application captures it) scrolls the output of the block under
  the pointer when it shows only part of it (`view::can_scroll`), revealing the whole
  block (`view::reveal`) so its header is never cut by the view top; at the bottom end
  of its output, scrolling down moves the timeline instead;
  elsewhere (short block, gap, margin, chevron, blank space below the cards) it scrolls
  the timeline (`mouse::wheel_target`). `Shift+PageUp/PageDown` scroll the selected
  block by a page when it can scroll, otherwise the timeline (`view::page_target`);
  `Shift+Home/End` go to the timeline's top / bottom. A key press or a paste brings the
  timeline and the current block back to the bottom.
- A block that shows only part of its output has a scroll indicator: a thin bar in the
  dim color in the card's right padding, beside its output rows (`frame::ScrollBar`).
- The window holds, from the left: the padding, a margin of half a cell, half a cell of
  card padding, the grid, the card padding, then a right reserve (the padding, or the
  chevron radius when larger), always reserved. The view rows of one block form a card
  (`frame::Card`, by `frame::marks` from the timeline): a rounded background (the
  background blended 6 % toward the foreground, 14 % when selected) behind the grid and
  its card padding, a status stripe in the margin (green, red, yellow, or
  `DimForeground` for `Success`, `Failure`, `Running`, `Unknown`; none for the neutral
  card of the current block at its prompt), and, on a finished block with output whose
  first row is shown, a fold chevron on the card's top-right corner: a disc in the card
  color with a triangle (right folded, down unfolded) in the status color. A card whose
  block starts above the view (or is cut by its cap) keeps its top corners above the
  view. Half a line separates two cards. The summary row of a folded block shows a dim
  label (`i18n::Msg::LinesHidden`). `GridLayout::hit` tells a margin pixel, a chevron
  disc, a gap between cards and a grid cell apart; a margin click selects the block (the
  margin below the cards counts as the last row's), a gap click does nothing.
- Text selection spans blocks (`timeline_selection::TimelineSelection`, kept as
  `Session::selection`): two ends, each a view source, a point and a side. It is split
  into one alacritty `Selection` per grid it covers, top to bottom
  (`view::sources`: each block's header, its output when not empty, the live grid
  last), the grids in between whole, and each part is set on its grid's `Term`
  (`timeline_selection::apply`), which the frame highlights. An end on a summary row
  takes the whole hidden output; live ends count lines from the top of the live
  history so they stay on their text while output scrolls. The copied text joins the
  parts with a line break, then `clean_copied_text`. The `SelectionSnapshot` applies to
  the live grid only; a selection reaching the live grid is dropped when that grid is
  detached into a block.
- Block actions (`block_actions`, finished blocks only): the target block is the
  selected one, otherwise the last finished block. A margin click or
  `Ctrl+Shift+PageUp/PageDown` (neighbour block, the last finished block on the first
  press, scrolled into view by `view::reveal`) select a block: the session keeps it as
  `selected_block`, its card is `selected` (`card_selected` color), and its text
  (`block_actions::block_text`: command line, then output) goes to the primary
  selection and to `Ctrl+Shift+C`. `Ctrl+Shift+Alt+C` / `O` copy the command (the
  signal's command line, or the last header row) / the output; an empty part leaves the
  clipboard untouched.

## Rendering pipeline

1. `Frame::capture` reads the rows given by `view::rows` from their source grid (frozen
   rows from their block's `Term`, live rows from the session's; colors overrides and
   cursor from the live `renderable_content()`, the selection of each grid) and
   produces one `FrameCell`
   per visible cell, plus one `RowDecoration` (hidden rows summary) per row and the
   block cards (`Frame::cards`, by `frame::marks`): character, final fg/bg (after DIM, INVERSE, HIDDEN, OSC overrides),
   bold/italic/underline/strikeout, wide-char spacer flag.
   The block cursor is baked into the cell colors; other shapes (beam, underline,
   hollow when unfocused) are returned separately. A placeholder cell (`U+10EEEE`, see
   "Images") becomes an image cell (`FrameCell::image`: image, placement, row, column)
   with no text and no background; `Frame::attach_images` then looks up each shown
   placement in the pane's `ImageStore` (`Frame::images`: box in cells, source rect,
   current frame pixels).
2. `scene::build` converts the frame into:
    - `under` rects: the card backgrounds (radius a quarter of a cell height) and
      status stripes (a sixth of a cell wide, at least 2 px, round ends), then the cell
      backgrounds, consecutive same-color cells merged, default background skipped (it
      is the clear color, so the card shows through); rows are placed at
      `GridLayout::row_top` of their view offset (`Frame::row_tops`, in half-lines);
    - image quads: one per run of image cells showing consecutive columns of one row of
      one placement; the image is scaled to fit its placement's box of cells, aspect
      ratio kept, centered, and each quad shows the part falling in its cells;
    - `over` rects: underline, strikeout, the fold chevrons (disc and triangle, SDF
      shapes of the rect pipeline), non-block cursor;
    - glyph instances: one per drawn cell (spaces and wide-char spacers skipped),
      anchored at the cell origin, plus the fold summaries (one character per cell);
    - overlay rects and glyph instances at pixel positions (banner).
3. `Session::redraw` captures one frame per pane shown by the active tab. When the tab
   shows several panes, the inactive ones go through `Frame::dimmed(INACTIVE_PANE_DIM)`
   (every color mixed 30 % toward the background). `chrome::scene::build` gives the
   interface layer (`UiLayer`: rects, then text items): the dividers (`ui.pane_border`)
   and a 2 px border inside the active pane (`ui.palette_match`, the accent color) when
   the tab shows several panes, and the tab bar (`chrome::tab_bar::TabBarLayout`) when
   there are several tabs. The panes area is the window minus the bar. The open dialog
   (`chrome::dialog::DialogLayout`, centered) or command palette
   (`chrome::command_palette::PaletteLayout`, at most 600 px wide, its top at 15 % of
   the window height) goes to the overlay rects and texts of the layer.
4. `Renderer::render` builds the scene of each pane with its own `GridLayout`, resolves
   the glyph instances through the glyph cache (one group per pane, then the interface
   text, the banner, the overlay text), uploads the newly rasterized glyphs, then draws
   in one pass: clear; for each pane, clipped to its rect by a scissor, `under` rects,
   images, glyphs, `over` rects; then over the whole window the interface rects, the interface
   text, the banner rects and glyphs, and the overlay rects and text, so the dialog
   or the palette covers everything. Colors are linearized when the surface
   format is sRGB.

The interface text (`renderer/ui_text.rs`) uses its own font: sans-serif, at a size
per `UiFont` (`Chrome`: `UI_FONT_SIZE`, 13 px, for the tab bar and the dialog;
`Palette`: `PALETTE_FONT_SIZE`, 16 px, for the command palette) times the window scale
factor, independent of the terminal font and its zoom. Each line is shaped once per
font, text and maximum width (truncated with `…`), and its characters go through the
same glyph cache and atlas as the grid, under keys marked with their `UiFont`.

### Banner

`Frame.banner` holds a `BannerLayout` (from `banner.rs`) and its theme colors
(`ui.banner_background`, `ui.banner_text`). It covers the top of the window, over the
grid (the grid does not move), in the frame of the active pane: its rect goes to the
scene's overlay rects and its text to its overlay glyphs (one character per cell
width), drawn after every pane and the interface, unclipped, under the dialog. The layout gives the pixel rect of each action,
and `Session::left_press` hit-tests the banner before starting a selection.

The configuration error banner shows the first error of `config.toml` and the theme
file ("(+N more)" when there are several); every error is also logged. A click hides it.

### Images

An image is anchored in the grid by placeholder cells, as kitty's Unicode placeholders:
`graphics::place` writes `U+10EEEE` at the cursor for each cell of the placement, the
image id in the foreground color (low 24 bits; its high byte in a third diacritic), the
placement id in the underline color, and the cell's row and column in the image as
combining diacritics (kitty's table, `graphics/diacritics.rs`). Rows go down from the
cursor line, scrolling at the bottom like text, and are clipped at the right edge. `Term`
then scrolls, erases, keeps in history and clears these cells like any other, and
`graphics::erase` blanks the cells of a deleted image or placement (screen and history).
A cell without a column diacritic continues its left neighbour (column + 1).

The pixels live in the pane's `ImageStore` as `Pixels` (immutable RGBA, a process-wide
uid). `ImageRenderer` (`renderer/images.rs`) keys one texture on each uid: uploaded the
first time a frame is drawn (sRGB format on an sRGB target), dropped once no store holds
those pixels. Quads of the same texture are drawn in one call. A frame larger than the
GPU texture limit is not drawn.

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

- `input::action_for` checks app shortcuts first (paste, scrollback, font size, tabs,
  panes, command palette). Letters are matched on the character typed, else on the
  physical key; `Alt+1..9` on the physical digit row. While an overlay is open, keys go
  to it only.
- Otherwise `input::key_to_bytes` encodes the key following xterm conventions:
  modifier parameter `1 + shift + 2*alt + 4*ctrl`, application cursor mode (`SS3`),
  Alt as ESC prefix, Ctrl+char control codes.
- Any input scrolls the view back to the bottom.
- Rich input editor (`input_editor`): active while the current block is at an editing
  prompt (`timeline::Current::editing`, set by the `prompt` signal, cleared when the
  command starts), on the main screen and with the mouse not reserved for an
  application (`input_editor::rich_mode`, read with `Term` then `Timeline` locked). The
  keyboard branch checks `input_editor::prompt_command` before `action_for`, so the
  editor shortcuts win over the app shortcuts at the prompt only; a move clears the
  selection, and each command writes its private sequence (`CSI <code> u`, bound by the
  integration scripts). A simple click on the live grid writes
  `input_editor::click_moves` (`CharLeft` / `CharRight` over the buffer distance from
  the cursor, the click bounded by `Current::prompt_start`) on release. `Extend` and
  `SelectAll` set the selection (`input_editor::extend_selection` / `select_all`) and
  an `input_editor::PendingExtend`; the next `Wakeup` where the cursor moved updates the
  selection end there, recaptures the `SelectionSnapshot` and writes the primary
  selection. A key typing text, `Backspace` / `Delete` (`input_editor::replacing_text`)
  or a `DeleteWord` / `InsertNewline` command (`command_replacing_text`) over a
  selection in the input (`input_editor::input_selection`, from
  `TimelineSelection::live_ends`) writes `input_editor::replacement_bytes` instead, then
  clears the selection.
- Mouse wheel: block or timeline scrolling on the primary screen (see Command blocks),
  arrow keys on the alternate screen when the application enabled alternate scroll.
- Mouse reporting only covers the live grid's rows (`mouse::reports_at`: a
  `Hit::Cell` whose source is `RowSource::Live`), or the whole window on the alternate
  screen; the cell sent is the live grid's viewport cell. Frozen blocks, the margin,
  the chevrons, the gaps and the blank space below the cards (`Hit::Blank`) are always
  Graphene's: there the pointer is the I-beam and clicks select. A gesture the
  application owns keeps being reported until its release, wherever the pointer goes.

### Mouse selection

- `layout::point_at` converts the cursor pixel to a viewport cell and side (the margin
  clamps to the first column), then `view::point` to a grid point through the view
  rows (the view may be scrolled).
- Left press: `ClickTracker` counts clicks (400 ms, same cell) and `selection::
  selection_type` picks a `Simple` (or `Block` with `Ctrl`), `Semantic` or `Lines`
  `TimelineSelection`, or `Shift` extends the existing one, across blocks too.
- `CursorMoved` while pressed updates the selection. Left release clears an empty
  selection (simple click), otherwise writes the cleaned text to the primary selection.
- Auto-scroll: in `about_to_wait`, while dragging with the cursor above or below the
  grid (`layout::vertical_overflow`), the timeline scrolls by `selection::
  auto_scroll_delta` lines (1 plus 1 per cell height of overflow, capped at 10) every
  50 ms (`ControlFlow::WaitUntil`) and the selection end follows the edge row. Disabled
  on the alternate screen, which has no history.
- Middle press pastes the primary selection through `input::paste_bytes` and, like
  every paste, clears the selection.
- Text change: the release stores a `selection::SelectionSnapshot` in the `Session`.
  On each `TermEvent::Wakeup`, `SelectionSnapshot::validate` restores the selection if
  `Term` erased it, then clears the whole selection (and drops the snapshot) when its live text
  differs from the snapshot. Every place that clears the selection drops the snapshot.
- Left and middle clicks over the live rows are ignored when the application enabled
  mouse reporting, unless `Shift` is held (`selection::mouse_reserved`).
- A left press in the margin (`layout::hit` gives `Hit::Margin`, the status stripe) or
  on a chevron disc (`Hit::Chevron`) is checked first and is always Graphene's, even
  under mouse reporting: the chevron toggles the fold of a card that has one, the
  margin selects the block of its row (see Command blocks), elsewhere it is ignored. A left press on a fold summary unfolds its block instead of selecting.

### Links

- `links::link_at(term, point)` on the grid of the cell under the pointer (the live
  grid, or the frozen header or output of a block, `Pane::link_at`): the OSC 8
  hyperlink of the cell (`Cell::hyperlink`), extended over the adjacent cells with the
  same id and URI across wrapped lines; otherwise a URL found by a `RegexSearch` over
  the logical line (`line_search_left` / `line_search_right`) for `http`, `https`,
  `file` and `mailto`, trailing punctuation and unbalanced `)` / `]` trimmed. A `Link`
  carries its cell range, its URI and `explicit` (OSC 8).
- Hover: `Session::update_hover` looks the link up again when the cell under the mouse
  changes (`CursorMoved`, wheel, release) and always on a `Wakeup` of the hovered pane,
  and keeps `hovered_link`; none while an overlay is open or a gesture is in progress.
  The link is Graphene's (`Session::active_link`, `links::active`) except over rows
  where the application captures the mouse (`Pane::captures`), where `Ctrl` must be
  held. An active link turns the pointer into the hand and is underlined by
  `Frame::underline_link` after the capture.
- Click: a left press on a link records a `LinkPress` (cell, click rank, modifiers,
  whether the application captured the mouse); the selection logic runs unchanged, so
  drag, double and triple-click from a link select as before. Where the application
  captures the mouse, a `Ctrl` + press on a link is consumed by Graphene (never
  reported). The release opens the link when `links::click_opens` allows it (same
  cell, same link, simple click without `Shift`, or the consumed `Ctrl` + click) and
  `links::may_open` too, through `opener::open`.
