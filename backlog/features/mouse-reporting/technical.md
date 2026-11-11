# Mouse reporting: technical design

Product decisions: [overview.md](overview.md).

## Building blocks already available

- `alacritty_terminal` 0.26 already parses the private modes and keeps them in
  `TermMode` (`term/mod.rs`): `MOUSE_REPORT_CLICK` (1000), `MOUSE_DRAG` (1002),
  `MOUSE_MOTION` (1003), the union `MOUSE_MODE`, `SGR_MOUSE` (1006), `UTF8_MOUSE`
  (1005), `FOCUS_IN_OUT` (1004) and `ALTERNATE_SCROLL` (1007). Setting one tracking
  mode removes the two others, so at most one of them is active. `ALTERNATE_SCROLL` is
  on by default. Mode 1015 (urxvt) is not parsed at all.
- `Term` does not encode mouse events: that is the frontend's job (Alacritty does it in
  its own `input` module, not in the crate).
- Focus events (1004) are already reported: [app.rs:441](../../../src/app.rs) writes
  `CSI I` / `CSI O` when `FOCUS_IN_OUT` is set. Only a test is missing.
- Alternate scroll is already done: [app.rs:291](../../../src/app.rs) turns the wheel
  into up/down arrows on the alternate screen when `ALTERNATE_SCROLL` is set. It does
  not look at the mouse mode yet, so today vim with `mouse=a` gets arrows instead of
  wheel events.
- The capture rule exists: `selection::mouse_reserved(mode, mods)`
  ([selection.rs:56](../../../src/selection.rs)) is true when a tracking mode is set and
  `Shift` is not held. Left press and middle click already return early on it
  ([app.rs:168](../../../src/app.rs), [app.rs:179](../../../src/app.rs)).
- `GridLayout::point_at` ([layout.rs:46](../../../src/layout.rs)) gives the viewport
  cell under a pixel, clamped to the grid: a drag outside the window reports the edge
  cell.
- winit 0.30: `Window::set_cursor(impl Into<Cursor>)` with `CursorIcon::Text` /
  `CursorIcon::Default`. The app never sets the pointer today, so it is always the
  default arrow.

## Alternatives considered

- **Encoding inside `app.rs`**: rejected. All the byte formats are pure functions of
  (event, cell, modifiers, mode); a separate pure module keeps them unit-tested without a
  window, like `input.rs` and `selection.rs`.
- **Supporting 1005 (UTF-8)**: rejected by the product decision. When an application
  asks for 1005 without 1006, the X10 encoding is used: identical below column/row 95,
  events beyond 223 are dropped.
- **Deciding the owner of a drag on every event** (switch to a Graphene selection when
  `Shift` is pressed mid-drag): rejected. The owner is fixed at press and kept until
  release, so the application never receives a press without its release.
- **Reporting horizontal wheel (buttons 66/67)**: rejected for now, horizontal deltas are
  ignored.

## Design

### New pure module `src/mouse.rs`

No window, clipboard or `Term` access.

```rust
pub enum Button { Left, Middle, Right }
pub enum MouseEvent {
    Press(Button),
    Release(Button),
    /// Motion with the held button, if any.
    Motion(Option<Button>),
    /// One wheel step: `true` = up.
    Wheel(bool),
}

/// Bytes for `event` at viewport cell (`column`, `line`), 0-based. `None` when the
/// current mode does not report this event, or when X10 cannot encode the cell.
pub fn encode(event: MouseEvent, column: usize, line: usize,
              mods: ModifiersState, mode: TermMode) -> Option<Vec<u8>>;

/// `CSI I` / `CSI O` when `FOCUS_IN_OUT` is set (moved from app.rs:441).
pub fn focus_report(focused: bool, mode: TermMode) -> Option<&'static [u8]>;

/// Arrow keys sent for `lines` wheel steps in alternate scroll (moved from app.rs:294).
pub fn alternate_scroll_bytes(lines: i32, mode: TermMode) -> Vec<u8>;
```

Encoding rules (xterm `ctlseqs`):

- Button code `cb`: left 0, middle 1, right 2, wheel up 64, wheel down 65. Motion adds
  32; motion without button is `3 + 32`.
- Modifier bits: `Alt` +8, `Ctrl` +16. `Shift` (+4) is never set: a `Shift` event
  never reaches the application.
- Which events a mode reports: press, release and wheel need any of `MOUSE_MODE`;
  motion with a button needs `MOUSE_DRAG` or `MOUSE_MOTION`; motion without button
  needs `MOUSE_MOTION`.
- SGR (`SGR_MOUSE` set): `CSI < cb ; column+1 ; line+1 M`, final `m` on release with
  the real button code.
- X10 otherwise: `CSI M` followed by three bytes `32+cb`, `32+column+1`, `32+line+1`.
  Release uses `cb = 3`. A cell whose 1-based coordinate exceeds 223 gives `None`
  (event dropped, as xterm does).

### Routing: `app.rs`

New `Session` fields:

- `app_button: Option<mouse::Button>`: button of the gesture owned by the
  application, set at press, cleared at release.
- `last_reported_cell: Option<(usize, usize)>`: motion is only reported when the cell
  changes.
- `pointer_captured: bool`: current pointer shape, to call `set_cursor` only on change.

Rules:

- **Press** (`mouse_input`): if `mouse_reserved(mode, mods)` is true, the gesture
  belongs to the application: clear the Graphene selection (a click clears it, as in
  selection-copy), store `app_button`, write `encode(Press)`. Otherwise the current
  Graphene path (selection, `Shift` + middle click paste) runs unchanged. The right
  button, ignored today, is only handled on the application path.
- **Release**: if it matches `app_button`, write `encode(Release)` (dropped by `encode`
  if the application left mouse mode meanwhile) and clear `app_button`. Otherwise the
  current `left_release`.
- **Motion** (`cursor_moved`): if `app_button` is set or no Graphene drag is running,
  compute the cell; if it differs from `last_reported_cell`, write
  `encode(Motion(app_button))` (`encode` filters per mode). The Graphene drag path is
  unchanged.
- **Wheel** (`mouse_wheel`), after the existing line accumulation, in this order:
  1. mouse mode set and `Shift` not held: one `encode(Wheel)` per line;
  2. alternate screen with `ALTERNATE_SCROLL`, and no mouse mode: arrows (existing
     behaviour, through `alternate_scroll_bytes`);
  3. otherwise: scroll the history (`Shift` + wheel in mouse mode lands here; on the
     alternate screen there is no history, so it does nothing).
  Horizontal deltas are ignored (already the case: only `y` is read).
- **Ctrl + click on a link**: not handled here. When images-links lands, its link check
  runs before the press is reported.

### Pointer shape

`update_pointer()` sets `CursorIcon::Default` (arrow) when `mode` intersects
`MOUSE_MODE`, `CursorIcon::Text` otherwise, only when it changes. Called at session
creation and on `TermEvent::Wakeup` (mode changes always come with PTY output).

### Focus

`WindowEvent::Focused` calls `mouse::focus_report` instead of the inline bytes. No
behaviour change.

### Docs

- `docs/vision.md`: move "Mouse reporting to applications" to Done when the feature
  ships.

## Consequences checked against the code

- `selection::mouse_reserved` keeps its meaning and stays the single capture rule.
- Mouse events are reported with viewport coordinates even when the history is
  scrolled (`display_offset > 0`); applications that capture the mouse almost always run
  on the alternate screen, which has no history.
- The existing alternate scroll changes in one case: with a mouse mode set, the wheel is
  now reported instead of turned into arrows (vim `mouse=a`, htop). less and man keep
  getting arrows.
- `left_pressed` keeps meaning "a Graphene selection drag is running": the auto-scroll
  ([app.rs:226](../../../src/app.rs)) is not triggered by an application drag.
- An application that requests 1005 receives X10 bytes: correct up to column 95, wrong
  beyond (accepted, 1005 is not supported).

## Tests

- `mouse.rs`: SGR press/release (`M` / `m`, real button on release), X10 press/release
  (`cb = 3` on release), wheel up/down, motion with and without button per mode
  (1000 reports none, 1002 only with button, 1003 both), `Alt` / `Ctrl` bits, `Shift`
  bit never set, X10 coordinate 223 encoded and 224 dropped, `focus_report` with and
  without `FOCUS_IN_OUT`, `alternate_scroll_bytes` in normal and app cursor mode.
- `selection.rs`: existing `mouse_reserved` tests unchanged.
- Routing and pointer shape live in `app.rs`, which needs a window: checked by hand
  with vim (`set mouse=a`), htop, tmux (`set -g mouse on`), less and `Shift` + drag /
  wheel / middle click in each.

## Découpage en tâches d'implémentation

1. [#45](https://github.com/marmotz/graphene-term/issues/45) Pure encoding module for mouse, focus and alternate scroll.
2. [#46](https://github.com/marmotz/graphene-term/issues/46) Report clicks, drag and motion to applications (depends on #45).
3. [#47](https://github.com/marmotz/graphene-term/issues/47) Report the wheel, `Shift` + wheel scrolls the history (depends on #45).
4. [#48](https://github.com/marmotz/graphene-term/issues/48) Arrow pointer while an application captures the mouse.
