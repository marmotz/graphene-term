# Selection and copy: technical design

Product decisions: [overview.md](overview.md).

## Building blocks already available

`alacritty_terminal` 0.26 already provides most of the model:

- `Term::selection: Option<Selection>` (public field) with
  `SelectionType::{Simple, Block, Semantic, Lines}`. `Selection::new(ty, point, side)`
  and `Selection::update(point, side)` cover drag, double-click (semantic expansion),
  triple-click (logical line, wrapped lines included) and block selection.
- The selection lives in grid coordinates and is rotated by `Term` when the content
  scrolls (`term/mod.rs` scroll_up/scroll_down), so it follows its text in the history.
- `Term::selection_to_string()` joins wrapped lines (`WRAPLINE` flag), handles wide
  characters and their spacers, expands tabs and appends zero-width characters
  (combining marks, variation selectors). This covers "every Unicode character is
  copied as is".
- `RenderableContent::selection: Option<SelectionRange>` (already computed by
  `term.renderable_content()` in [frame.rs:49](../../../src/frame.rs)) and
  `SelectionRange::contains(point)` for highlighting.
- `term::viewport_to_point(display_offset, Point<usize>)` converts a viewport cell to a
  grid point.

`arboard` 3.7 exposes the primary selection on Linux through
`SetExtLinux::clipboard(LinuxClipboardKind::Primary)` and
`GetExtLinux::clipboard(LinuxClipboardKind::Primary)`, on X11 and on Wayland (data
control protocol, compositor permitting). The `Clipboard` is already kept alive in
`Session` ([app.rs:54](../../../src/app.rs)), which X11 needs to keep serving the
selection.

## Alternatives considered

| Topic | Options | Retained |
|---|---|---|
| Selection model | Own model in Graphene / `Term::selection` | `Term::selection`: rotation on scroll, semantic and line expansion, block mode and wide chars come for free. |
| Text extraction | From `Frame` cells / `Term::selection_to_string` | `selection_to_string`: `Frame` only holds the viewport and drops zero-width characters ([frame.rs:83-92](../../../src/frame.rs)). |
| "Selected text changed" detection | `Term` only / damage tracking / shadow copy + text comparison | Shadow copy + text comparison, see below. |
| Highlight | Palette color / fg-bg swap | fg-bg swap (decided), no palette change until the themes feature. |

## Design

### New pure module `src/selection.rs`

Unit-tested, no window or GPU access.

- `ClickTracker`: counts consecutive clicks on the same cell within 400 ms (winit
  exposes no system double-click delay). Returns 1, 2 or 3; a fourth click starts over
  at 1.
- `selection_type(clicks, mods) -> SelectionType`: 1 = `Simple` (`Block` when `Ctrl`
  is held), 2 = `Semantic`, 3 = `Lines`.
- `clean_copied_text(&str) -> String`:
  1. split on `\n`, `trim_end` every line;
  2. remove the longest leading whitespace prefix common to all non-blank lines
     (dedent; tabs and spaces compared literally);
  3. remove every trailing `\n` (decided: a paste never runs a command by itself).
  Wrapped lines are already joined by `selection_to_string`.
- `SEMANTIC_ESCAPE_CHARS`: alacritty's default list without `:`, so that a URL
  (`https://...`) or `host:port` stays one word. Set through
  `Config::semantic_escape_chars` in [terminal.rs:60-63](../../../src/terminal.rs).

### Geometry: `layout.rs`

`GridLayout::point_at(x, y) -> (column, line, Side)`: viewport cell under a physical
pixel, clamped to the grid; `Side` is `Left`/`Right` depending on the half of the cell.
`GridLayout::vertical_overflow(y) -> i32`: number of lines above (negative) or below
(positive) the grid, 0 inside. Used by auto-scroll. The inverse of
`cell_origin` ([layout.rs:36](../../../src/layout.rs)).

### Mouse handling: `app.rs`

`Session` gains `mouse_position`, `left_pressed`, a `ClickTracker` and a
`SelectionSnapshot { selection: Selection, text: String }` (see "Text change").

Mouse capture rule (decided, final behaviour already now): when
`mode.intersects(TermMode::MOUSE_MODE)` and `Shift` is not held, left and middle clicks
are ignored; they are reserved for the mouse reporting feature.

- `CursorMoved`: store the position; when `left_pressed`, `selection.update(point,
  side)` and request a redraw.
- `MouseInput(Left, Pressed)`:
  - `Shift` held and a selection exists: `update` it to the clicked point (extend);
  - otherwise create `Selection::new(selection_type(..), point, side)`. A single click
    produces an empty `Simple` selection, which renders nothing.
- `MouseInput(Left, Released)`: if the selection is empty (simple click without drag),
  clear it. Otherwise compute `clean_copied_text(term.selection_to_string())`, store
  the snapshot and write it to the primary selection.
- `MouseInput(Middle, Pressed)`: read the primary selection and send it through the
  existing `input::paste_bytes` ([input.rs:172](../../../src/input.rs)), like
  `Session::paste` ([app.rs:96](../../../src/app.rs)).
- Auto-scroll: in `ApplicationHandler::about_to_wait`, while `left_pressed` and
  `vertical_overflow != 0` on the primary screen, scroll by `Scroll::Delta` (1 line,
  plus 1 per cell height of overflow, capped at 10), update the selection on the edge
  row, and set `ControlFlow::WaitUntil(now + 50 ms)`. Otherwise `ControlFlow::Wait`.
  The alternate screen has no history: no auto-scroll there.

### Copy and clearing

- `Action::Copy` ([app.rs:108](../../../src/app.rs)): write the cleaned selection text
  to the clipboard. No selection: still swallowed, as today.
- Keystrokes: in the `KeyboardInput` branch ([app.rs:283-292](../../../src/app.rs)),
  when `key_to_bytes` returns bytes, clear the selection. App shortcuts return earlier
  ([app.rs:279-282](../../../src/app.rs)) and keep it, as decided. `Action::Paste`
  also sends input to the shell and clears it.
- New simple click and new selection: handled by the press logic above.

### Text change

`Term` clears the selection on erase sequences (`ED`, `EL`, `RIS`: `term/mod.rs`
around clear_line / clear_screen) even when the same text is redrawn, and keeps it when
a character is overwritten by a different one (`input` does not touch it). Neither
matches the decision. Graphene therefore keeps its own snapshot and validates on each
`TermEvent::Wakeup` ([app.rs:149](../../../src/app.rs)), under the existing lock:

1. `term.selection` is `Some`: refresh the snapshot selection (it carries the
   rotation done by `Term`).
2. `term.selection` is `None` but a snapshot exists: put the snapshot selection back
   into `term.selection`.
3. Compare `clean_copied_text(term.selection_to_string())` with the snapshot text; if
   it differs or is `None`, clear both.

Known limit: if the content scrolls and is erased in the same PTY batch, the restored
selection points at stale coordinates; its text then differs and it is cleared, which
is the safe outcome. Cost: one `selection_to_string` per wakeup while a selection
exists, proportional to the selection size. Acceptable for usual selections; to revisit
only if profiling shows it.

### Rendering: `frame.rs`

In the cell loop ([frame.rs:73-93](../../../src/frame.rs)), when
`content.selection` contains `indexed.point`, swap `fg` and `bg` (spacer cells
included, so a wide character is fully highlighted). The block cursor bake
([frame.rs:114-123](../../../src/frame.rs)) runs afterwards and keeps priority.
`scene.rs` needs no change: highlighted cells are ordinary colored cells.

### Docs

- `docs/decisions.md`: replace "Ctrl+Shift+C is reserved", add the shadow selection
  decision and the semantic separators.
- `docs/architecture.md`: new module, mouse input section.
- `README.md`: shortcuts table (selection modes, `Ctrl+Shift+C`, middle click).
- `docs/vision.md`: roadmap row to "Done".

## Consequences checked against the code

- No change to `input::key_to_bytes` or `action_for`; `Action::Copy` already exists.
- `Frame::capture` signature unchanged: the selection comes from `renderable_content()`.
- Out of scope, noticed: `Frame` drops zero-width characters, so combining marks are
  copied correctly but not rendered. Separate rendering issue.
- Primary selection may be unsupported by a Wayland compositor: `arboard` returns
  `ClipboardNotSupported`, logged at debug level, copy to clipboard unaffected.

## Tests

- `selection.rs`: click counting (delay, other cell, 4th click), type per modifier,
  `clean_copied_text` (trailing spaces, dedent with mixed indentation, blank lines
  ignored by dedent, trailing newline removed, Powerline/Nerd Font glyphs, combining
  chars preserved).
- `layout.rs`: `point_at` (padding, sides, clamping), `vertical_overflow`.
- `frame.rs`: selected cells swapped, wide char fully highlighted, block cursor wins.
- Integration on a `Term<VoidListener>` (pattern of [frame.rs:149](../../../src/frame.rs)):
  wrapped line copied as one line, URL selected as one word, snapshot validation (same
  text redrawn after `EL` keeps the selection, different text clears it).

## Découpage en tâches d'implémentation

1. [#1](https://github.com/marmotz/graphene-term/issues/1) Pure helpers, semantic separators and pixel-to-cell geometry.
2. [#2](https://github.com/marmotz/graphene-term/issues/2) Highlight selected cells in the frame.
3. [#3](https://github.com/marmotz/graphene-term/issues/3) Mouse selection, copy, primary selection and middle-click paste (depends on #1, #2).
4. [#4](https://github.com/marmotz/graphene-term/issues/4) Auto-scroll while dragging past the grid edges (depends on #3).
5. [#5](https://github.com/marmotz/graphene-term/issues/5) Clear the selection only when its text really changes (depends on #3).
