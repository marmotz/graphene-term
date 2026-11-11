# Tabs, splits and command palette: technical design

Product decisions: [overview.md](overview.md).

This feature lands after selection-copy, mouse-reporting, command-blocks and
rich-input-editor (roadmap order). Code references below describe the current
single-session code of the `feature-selection-copy` branch; the changes those features
bring to `terminal.rs` (own PTY loop) and `app.rs` (blocks, rich mode) keep the same
`Session` shape, so the split described here applies to them unchanged.

## Starting point

- One window, one shell: `Session` ([app.rs:53-74](../../../src/app.rs)) mixes window
  state (`window`, `renderer`, `clipboard`, `focused`, `modifiers`, `mouse_position`)
  and shell state (`terminal`, `layout`, `scroll_accumulator`, `click_tracker`,
  `selection_snapshot`, `left_pressed`, `auto_scroll_deadline`).
- `EventProxy` ([terminal.rs:24-38](../../../src/terminal.rs)) forwards
  `UserEvent::Terminal(Event)` with no origin: events cannot be routed to a pane.
- `ChildExit` / `Exit` end the event loop ([app.rs:342](../../../src/app.rs)), and
  `Title` sets the window title directly ([app.rs:315](../../../src/app.rs)).
- `GridLayout` ([layout.rs:16-34](../../../src/layout.rs)) places one grid at `padding`
  from the window's top-left corner; `cell_origin` / `point_at` assume that origin.
- The renderer draws one `Frame` with one `GridLayout`
  ([renderer/mod.rs:104-169](../../../src/renderer/mod.rs)): clear, `under` rects,
  text, `over` rects. `TextGrid` caches shaped rows per visible line
  ([renderer/text.rs:15-29](../../../src/renderer/text.rs)) and clips to the whole
  screen ([text.rs:152-157](../../../src/renderer/text.rs)). The rect pipeline already
  uses alpha blending ([rects.rs:101](../../../src/renderer/rects.rs)).
- `action_for` ([input.rs:21-46](../../../src/input.rs)) matches the logical key only.
  `Ctrl+Shift+letter` that is not an action is sent as a control code
  ([input.rs:154-161](../../../src/input.rs)), `Ctrl+PageUp` as `CSI 5;5~`.
- alacritty_terminal 0.26: `tty::Options::working_directory` sets the start directory;
  `Pty::child()` exposes the shell `Child` (its pid) before the `Pty` moves into the
  PTY loop. vte 0.15 does not handle OSC 7 (`osc_dispatch` only knows 0, 2, 4, 8, 10-12,
  22, 50, 52, 104, 110-112), so the shell's directory must come from elsewhere.

## Alternatives considered

| Topic | Options | Retained |
|---|---|---|
| Split model | Binary tree / n-ary tree (siblings share a parent) | Binary tree: every operation (split, remove, resize, equalise) is local to one node. Equalise computes ratios from leaf counts, so a 3-column layout made of two splits still gets equal widths. |
| Current directory | OSC 7 / `/proc/<shell pid>/cwd` | `/proc`: works with every shell, no integration, and vte drops OSC 7 anyway. |
| Foreground program | `tcgetpgrp` on the PTY fd (libc) / `tpgid` field of `/proc/<shell pid>/stat` | `/proc` stat: std only, and `/proc/<tpgid>/comm` gives the program name for the dialog. |
| Event routing | One winit proxy per pane carrying a `PaneId` / one channel per pane polled by the UI | `PaneId` in `UserEvent`: same mechanism as today, nothing to poll. |
| Inactive pane dimming | Translucent rect over the pane / colors mixed toward the background in the frame | Colors mixed in a pure function on `Frame`: no extra draw, testable, glyph colors dim with the cells. |
| UI text | Reuse `TextGrid` / second glyphon `TextRenderer` sharing the font system and atlas | Second renderer: the tab bar and palette use a proportional font with free positions, not the cell grid. One `FontSystem` and one atlas for both. |
| Fuzzy search | `nucleo-matcher` / own matcher | `nucleo-matcher` 0.3 (user decision): proven scoring and match indices for highlighting. |
| Translations | Fluent / own table | Own table (user decision): `match` per language, a missing translation does not compile. |
| Recent palette entries | Memory / state file | State file (user decision), XDG state directory. |

## Design

### Pane identity and events: `src/terminal.rs`

- `PaneId(u64)`, allocated by a counter in the workspace.
- `UserEvent::Terminal(PaneId, Event)`; `EventProxy::new(proxy, pane_id)`.
- `Terminal::spawn(proxy, size, cwd: Option<PathBuf>)` sets
  `tty::Options::working_directory` and stores `shell_pid: u32` (from
  `pty.child().id()` before the `Pty` is handed to the PTY loop).
- `Terminal::current_dir()` and `Terminal::foreground_program()` call `process.rs`.

### Processes: new pure module `src/process.rs`

- `parse_tpgid(stat: &str) -> Option<i32>`: fields counted after the last `)`, because
  `comm` may contain spaces and parentheses.
- `foreground_program(shell_pid) -> Option<String>`: reads `/proc/<pid>/stat`; when
  `tpgid` is not the shell's pid, returns `/proc/<tpgid>/comm` trimmed. `None` when the
  shell is in the foreground or `/proc` is unreadable (then no confirmation: never
  block a close on an error).
- `current_dir(shell_pid) -> Option<PathBuf>`: `read_link("/proc/<pid>/cwd")`.

### Pane: new module `src/pane.rs`

`Pane` takes the shell part of `Session`: `id`, `terminal`, `layout: GridLayout`,
`scroll_accumulator`, `click_tracker`, `selection_snapshot`, `title: Option<String>`
(last OSC 0/2, `None` after `ResetTitle`). Its methods are the current `Session`
methods that only touch that state: `scroll_display`, `clear_selection`,
`validate_selection`, `mouse_point`, `update_selection`, `send_paste`, `copy`,
`mouse_wheel`, `resize(layout)`.

`Session` keeps the window part plus `workspace: Workspace`, `drag: Option<Drag>` (see
mouse) and `overlay: Option<Overlay>` (palette or dialog). `left_pressed` and
`auto_scroll_deadline` become part of `Drag::Selection { pane }`.

### Split tree: new pure module `src/workspace/tree.rs`

```rust
enum Node { Leaf(PaneId), Split { axis: Axis, ratio: f32, first: Box<Node>, second: Box<Node> } }
enum Axis { Horizontal /* side by side */, Vertical /* stacked */ }
```

- `split(target, axis, new) -> bool`: the leaf becomes a `Split` with `ratio = 0.5`,
  new pane second. Refused (returns `false`) when either half would be under
  `MIN_PANE_COLUMNS = 10` or `MIN_PANE_LINES = 3`.
- `remove(target)`: the sibling replaces the parent.
- `layout(area: Rect, cell: CellSize, divider: f32) -> Layout { panes: Vec<(PaneId, Rect)>, dividers: Vec<Divider> }`:
  each split position is rounded to a whole cell so the ratio survives resizes without
  drift; `Divider { node_path, axis, rect }` is 1 physical pixel, its hit area 3 px
  on each side.
- `neighbour(from, direction, layout) -> Option<PaneId>`: among panes whose edge
  touches `from`'s edge in that direction, the one overlapping the most along the other
  axis, ties broken by the one containing `from`'s centre line.
- `next(from)` / `previous(from)`: depth-first leaf order, wrapping.
- `resize(target, direction, step)`: moves the nearest ancestor split on that axis
  (the one where `target` is on the side facing `direction`) by `step` cells, clamped
  to the minimum sizes.
- `set_divider(node_path, position)`: same clamp, used by mouse drag.
- `equalise()`: `ratio = leaves(first) / leaves(first + second)` along the same axis.

### Tabs: new pure module `src/workspace/mod.rs`

- `Tab { tree: Node, active: PaneId, zoomed: bool }`; `Workspace { tabs: Vec<Tab>,
  active: usize, panes: HashMap<PaneId, Pane> }`.
- Operations: `new_tab`, `close_pane(id)` (last pane closes the tab, last tab returns
  `WorkspaceEmpty` and the app exits), `select(index)`, `next` / `previous` (wrapping),
  `move_tab(from, to)`, `detach(pane)` (removed from its tree, new tab after the
  current one, `false` when it is the only pane of its tab).
- `tab_title(tab)`: active pane's `title`, else `current_dir` with `$HOME` shown as
  `~`, else the shell name. The window title follows the active tab.
- Zoom: when `zoomed`, `layout` returns only the active pane at the full area and no
  divider. Changing focus to another pane, splitting or closing unzooms first.
- Area: window minus the tab bar when `tabs.len() > 1`. The bar edge is a parameter
  (`TabBarPosition::Top` for now) so config-themes only has to pass its setting.

### Layout per pane: `src/layout.rs`

- `Rect { x, y, width, height }` in physical pixels.
- `GridLayout::new(rect: Rect, cell, padding)` replaces `(width, height, ...)`: the
  grid origin is `rect.x/y + padding`, and `cell_origin`, `point_at`,
  `vertical_overflow` use it. config-themes then adds its centring on top of this
  origin. `contains(x, y)` tells whether a pixel is in the pane's rect.
- On every window resize, font zoom, split, close, zoom or divider move, the app
  recomputes the layout of every tab (hidden ones included) and calls `Pane::resize`
  only for panes whose cell count changed, so hidden shells keep a correct size.
  A zoomed tab's hidden panes keep their last size.

### Keyboard: `src/input.rs`

- `action_for(key, physical: PhysicalKey, mods)`: letters and named keys as today
  (logical); `Alt+1..9` matched on `KeyCode::Digit1..Digit9` so they work on AZERTY.
- New `Action`s and default keys (overview table): `NewTab`, `ClosePane`, `NextTab`,
  `PreviousTab`, `GoToTab(u8)`, `SplitRight`, `SplitDown`, `FocusPane(Direction)`,
  `ResizePane(Direction)`, `NextPane`, `PreviousPane`, `ToggleZoom`, `CommandPalette`,
  plus palette-only `EqualisePanes`, `DetachPane`.
- `Action::ALL` lists every palette-visible action with its `Msg` label;
  `shortcut_label(action)` gives the text shown in the palette. config-themes replaces
  the default `match` by its keybindings table and must keep physical digit keys.
- While an overlay is open, keys go to the overlay, never to the shell or `action_for`.
- Focus events (`FOCUS_IN_OUT`, [app.rs:441-449](../../../src/app.rs)): on a pane
  change, `ESC [O` to the old pane and `ESC [I` to the new one; a window focus change
  only notifies the active pane.

### Mouse: `src/app.rs`

Hit test order: overlay, tab bar, dividers, panes.

- `Drag` enum: `Selection { pane }`, `Divider { node_path, axis }`,
  `Tab { index, grab_offset }`. A press decides the drag; moves and release go to it,
  so a selection never leaves its pane (`point_at` clamps to that pane's grid) and
  auto-scroll uses that pane's `vertical_overflow`.
- Press in an inactive pane: it becomes active, and the same press starts a selection
  (or goes to mouse reporting) as in the active one.
- Wheel: to the pane under the mouse, not the active one.
- Divider hover / drag sets `CursorIcon::ColResize` / `RowResize`.

### Tab bar: new pure module `src/chrome/tab_bar.rs`

- `TabBarLayout::new(width, titles, active, ui_metrics)`: equal-width tabs, capped at
  a maximum width, shrunk when they do not fit (titles truncated with `…`), then a `+`
  button. Height: UI line height plus vertical padding.
- `hit(x, y) -> Option<TabBarHit { Tab(i) | Close(i) | NewTab }>`.
- Reorder: during `Drag::Tab`, the dragged tab follows the mouse on x and
  `drop_index(x)` gives its slot; other tabs shift to their new slots; release calls
  `move_tab`.
- Middle click on a tab closes it, with the same confirmation as `ClosePane` on each
  of its panes.

### Command palette: new pure module `src/chrome/command_palette.rs`

- Entries: every `Action::ALL` entry (label, shortcut) then one entry per tab and pane
  (`Tab 2: ~/dev`, `Tab 2 > pane 3: vim`). Pane entries jump to the tab and focus the
  pane.
- Query matched with `nucleo-matcher` (smart case; exact API checked against the
  crate docs at implementation time); score descending, then recent rank. Empty query: recent entries first, then
  the default order. Match indices are returned for highlighting.
- Keys: printable text edits the query, `Backspace`, `Up` / `Down` move, `Enter` runs,
  `Escape` or a click outside closes. A click on an entry runs it.
- Geometry: width `min(600 px * scale, window width - 2 * margin)`, top at 15 % of
  the window height, at most 12 visible rows with scrolling.
- Recent entries: action ids (the snake_case names config-themes also uses) in
  `$XDG_STATE_HOME/graphene/palette-recent` (default `~/.local/state/...`), one per
  line, most recent first, capped at 50, written on every run. Tab and pane entries are
  not stored. A missing or unreadable file means an empty list.

### Confirmation dialog: new pure module `src/chrome/dialog.rs`

- `ConfirmClose { target: Pane(id) | Tab(i) | Window, programs: Vec<String> }`. The
  message lists the program names ("vim is still running", "2 programs are still
  running: vim, cargo").
- Opened by `ClosePane`, the tab close button / middle click and `CloseRequested`
  ([app.rs:432](../../../src/app.rs)) when at least one targeted pane has a
  `foreground_program`; otherwise the close happens at once.
- Buttons "Close" / "Cancel"; `Enter` closes, `Escape` cancels.
- Shell exit (`ChildExit` / `Exit`) never asks: `close_pane(id)` directly.

### Translations: new pure module `src/i18n.rs`

- `enum Lang { En, Fr }`; `Lang::detect()` reads `LC_ALL`, then `LC_MESSAGES`, then
  `LANG`, takes the part before `_` / `.`; `fr` gives `Fr`, anything else `En`.
- `enum Msg` (action labels, dialog texts, `Close`, `Cancel`, `NewTab`...) and
  `fn text(lang, msg) -> Cow<'static, str>`, with a `match` per language. Messages
  with a value (program list, counts) are variants with fields.
- config-themes later sets the language from its `language` key.

### Rendering: `src/renderer/`

- `TextContext` (new, `text.rs`): the `FontSystem`, `SwashCache`, `Cache`,
  `Viewport` and `TextAtlas`, shared by `TextGrid` and `UiText`.
- `TextGrid` keeps one row cache per pane: `HashMap<PaneId, Vec<RowBuffers>>`, entries
  of closed panes dropped after the frame. `TextBounds` is the pane's rect, so a
  glyph wider than its cell never draws over a neighbour.
- `UiText` (new, `ui_text.rs`): glyphon buffers for tab titles, palette rows and dialog
  text, family `Family::SansSerif`, fixed size `13 px * scale` (independent of the
  terminal font zoom), cached by text and width.
- `scene::build` is called once per visible pane with that pane's `GridLayout`, and
  the results are concatenated. Inactive panes go through `Frame::dimmed(0.3)` first
  (fg and bg mixed 30 % toward the background) when the tab has more than one pane.
- New pure `chrome/scene.rs`: tab bar, dividers, active pane border (2 px, accent
  color, only when the tab shows more than one pane), palette and dialog boxes, as
  `RectInstance`s and `UiTextItem { text, x, y, color, max_width }`.
- Draw order: clear, `under`, (images, from images-links), grid text, `over`, UI rects,
  UI text, overlay rects, overlay text. The overlay (palette, dialog, images-links
  tooltip) gets a second `UiText` pass so its background covers the tab bar text.
- UI colors: `UiColors` in `palette.rs` (tab bar background, active / inactive tab,
  text, divider, active border, overlay background, selected row, match highlight,
  dim factor), with hard-coded defaults; config-themes reads them from the `[ui]`
  section of a theme.

### Dependencies

- `nucleo-matcher = "0.3.1"` (latest release on crates.io at the time of writing).

### Docs

- `docs/architecture.md`: new modules (`pane`, `workspace`, `chrome`, `process`,
  `i18n`), `Session` vs `Pane`, events carrying a `PaneId`, draw order with the UI
  layers; "When the shell exits, the window closes" becomes "the pane closes".
- `docs/decisions.md`: binary split tree; `/proc` for cwd and foreground program;
  dimming in the frame; UI font separate from the terminal font; own i18n table.
- `README.md`: shortcut table, mouse actions on tabs and dividers.
- `docs/vision.md`: roadmap row set to "Done".

## Consequences checked against the code

- These keys stop reaching applications: `Ctrl+Shift+T/W/E/O/N/B/Z/P` (today sent as
  control codes by [input.rs:154-161](../../../src/input.rs)), `Ctrl+Tab`,
  `Ctrl+Shift+Tab`, `Ctrl+PageUp/PageDown`, `Ctrl+Shift+Arrow`,
  `Ctrl+Shift+Alt+Arrow`, `Alt+1..9`.
- Shortcut conflicts already resolved in the other designs: command-blocks moves block
  navigation to `Ctrl+Shift+PageUp/PageDown`; rich-input-editor no longer uses
  `Ctrl+Shift+Arrow`.
- `GridLayout::new` changes signature: its tests and `compute_layout`
  ([app.rs:77-81](../../../src/app.rs)) move to `Rect`. With a single pane the rect is
  the whole window, so pixel positions stay identical (non-regression test).
- `Frame::capture`, selection, copy and paste logic are unchanged; they run on the
  target pane instead of the session.
- `TextAreaSizeRequest` ([app.rs:336](../../../src/app.rs)) answers with the pane's
  size, not the window's.
- Closing a pane drops its `Terminal`, whose `Drop` already shuts its PTY loop down
  ([terminal.rs:90-94](../../../src/terminal.rs)).
- A pane's `/proc` lookups happen only on close requests and title fallback refreshes
  (on `Wakeup` of a pane without OSC title, throttled to once per second per pane),
  never per frame.

## Tests

- `process`: `parse_tpgid` on stat lines with spaces and `)` in `comm`; foreground vs
  shell; malformed input returns `None`.
- `workspace::tree`: split / remove keep the other leaves; minimum size refusal;
  `layout` rounds to cells and fills the area exactly; `neighbour` in the four
  directions on an L-shaped layout; `resize` picks the right ancestor and clamps;
  `equalise` gives equal widths to three columns built from two splits; `next` /
  `previous` wrap.
- `workspace`: close of the last pane closes the tab, last tab returns
  `WorkspaceEmpty`; `detach` of a single pane is refused; `move_tab` keeps the active
  tab active; zoom is cleared by focus change and split; title fallback order.
- `layout`: `GridLayout` in an offset rect; `point_at` / `cell_origin` stay inverse;
  full-window rect matches today's values.
- `input`: every default shortcut maps to its action; `Alt+Digit1` on an AZERTY
  logical key (`&`) maps to `GoToTab(1)`; unmapped `Ctrl+Shift+letters` still produce
  control codes.
- `chrome::tab_bar`: widths, truncation, hit test of tab / close / `+`, `drop_index`.
- `chrome::command_palette`: fuzzy order, recent first on empty query, pane entries,
  recent file round trip and cap, unreadable file.
- `chrome::dialog`: message for one and several programs, in both languages.
- `i18n`: `detect` precedence and fallback; every `Msg` has a non-empty text in each
  language (exhaustive `match` plus a test over a list of all variants).
- `frame::dimmed`: colors mixed toward the background, factor 0 is the identity.
- `chrome::scene`: no tab bar with one tab; border only with several panes; overlay
  rects after the bar.

## Découpage en tâches d'implémentation

| Issue | Task | Depends on |
|---|---|---|
| [#35](https://github.com/marmotz/graphene-term/issues/35) | Translation table and language detection | - |
| [#36](https://github.com/marmotz/graphene-term/issues/36) | Pane identity in terminal events, shell pid, cwd and foreground program | - |
| [#37](https://github.com/marmotz/graphene-term/issues/37) | Grid layout inside a pane rectangle | - |
| [#38](https://github.com/marmotz/graphene-term/issues/38) | Binary split tree | #37 |
| [#39](https://github.com/marmotz/graphene-term/issues/39) | Pane extracted from Session, workspace model and event routing | #36, #38 |
| [#40](https://github.com/marmotz/graphene-term/issues/40) | Multi-pane rendering, dimming, dividers and active border | #39 |
| [#41](https://github.com/marmotz/graphene-term/issues/41) | Split, focus, resize, zoom shortcuts and pane mouse handling | #40 |
| [#42](https://github.com/marmotz/graphene-term/issues/42) | Tab bar, UI text and tab shortcuts | #41 |
| [#43](https://github.com/marmotz/graphene-term/issues/43) | Close confirmation dialog | #35, #42 |
| [#44](https://github.com/marmotz/graphene-term/issues/44) | Command palette and docs | #35, #43 |
