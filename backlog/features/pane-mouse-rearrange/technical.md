# Rearrange panes with the mouse — Technical Design

Product decisions: [overview.md](overview.md).

## Current State

### Split tree and workspace

- Each tab is a binary split tree, `Node::Leaf(PaneId)` or `Node::Split { axis, ratio,
  first, second }` ([src/workspace/tree.rs:213-223](../../../src/workspace/tree.rs)).
  `Node::split` always puts the new pane **second** and refuses when either half is
  under `MIN_PANE_COLUMNS` / `MIN_PANE_LINES`
  ([src/workspace/tree.rs:11-15](../../../src/workspace/tree.rs),
  [src/workspace/tree.rs:288-314](../../../src/workspace/tree.rs)). `Node::remove`
  puts the sibling in place of the parent
  ([src/workspace/tree.rs:315-336](../../../src/workspace/tree.rs)).
- The minimum pane size is in cells only (`Axis::min_pane`,
  [src/workspace/tree.rs:49-55](../../../src/workspace/tree.rs)); `Geometry` carries the
  area, the cell size and the divider thickness
  ([src/workspace/tree.rs:139-146](../../../src/workspace/tree.rs)).
- `Tab { tree, active, zoomed }`; a zoomed tab lays out only its active pane over the
  whole area ([src/workspace/mod.rs:80-127](../../../src/workspace/mod.rs)).
- `Workspace` already has the building blocks the moves need: `close_pane` (closes the
  tab of a last pane, [src/workspace/mod.rs:243-269](../../../src/workspace/mod.rs)),
  `remove_tab` (keeps the shown tab shown,
  [src/workspace/mod.rs:270-278](../../../src/workspace/mod.rs)), `move_tab`
  ([src/workspace/mod.rs:303-320](../../../src/workspace/mod.rs)), `detach` (moves a pane
  out of a several-pane tab into a new tab right after its own,
  [src/workspace/mod.rs:321-341](../../../src/workspace/mod.rs)), `focus` and
  `toggle_zoom` ([src/workspace/mod.rs:343-357](../../../src/workspace/mod.rs),
  [src/workspace/mod.rs:457-467](../../../src/workspace/mod.rs)). The module is generic
  over the pane type and unit-tested without shells.
- `Workspace::pane_title` gives the title the tab bar shows (OSC title, else directory,
  else shell name) ([src/workspace/mod.rs:470-487](../../../src/workspace/mod.rs)).

### Mouse routing

- `Session::drag: Option<Drag>` holds the gesture decided by the press: `Selection`,
  `Divider` or `Tab` ([src/app.rs:72-85](../../../src/app.rs)).
- A press goes to the first of: overlay, banner, tab bar, divider, pane
  ([src/app.rs:1171-1209](../../../src/app.rs)). In a pane, a press is routed to the
  application when it reports the mouse (`selection::mouse_reserved` and
  `Pane::reports_at`), else starts a selection
  ([src/app.rs:1244-1300](../../../src/app.rs)).
- Moves of a `Drag` are handled in `cursor_moved`
  ([src/app.rs:1355-1385](../../../src/app.rs)), the drop in `release_gesture`
  (tab drop through `TabBarLayout::dragged` + `Workspace::move_tab`,
  [src/app.rs:1316-1333](../../../src/app.rs)). The pointer shape is chosen by
  `update_pointer` ([src/app.rs:958-994](../../../src/app.rs)).
- A pane closing drops the drags it invalidates
  ([src/app.rs:1700-1710](../../../src/app.rs)).
- Keys go to the open overlay first, then to the keybindings and the active pane
  ([src/app.rs:1573-1605](../../../src/app.rs)). Timed work (auto-scroll, blink,
  animations) reports a deadline to `about_to_wait`, which sets
  `ControlFlow::WaitUntil` ([src/app.rs:2243-2260](../../../src/app.rs)).
- `ClickTracker` counts double and triple clicks on the same grid point
  ([src/selection.rs:18-39](../../../src/selection.rs)).

### Layout and drawing

- `relayout` lays out every tab and gives each pane a `GridLayout` built from its
  rectangle ([src/app.rs:797-811](../../../src/app.rs),
  [src/layout.rs:111-114](../../../src/layout.rs)). `geometry` takes the area left by
  the banner and the tab bar ([src/app.rs:268-276](../../../src/app.rs)).
- The tab bar is only shown with more than one tab (`tab_bar::shown`,
  [src/chrome/tab_bar.rs:43-45](../../../src/chrome/tab_bar.rs),
  [src/app.rs:232-265](../../../src/app.rs)). `TabBarLayout::hit` returns `Tab`,
  `Close` or `NewTab`, and `None` on the empty part of the bar
  ([src/chrome/tab_bar.rs:232-248](../../../src/chrome/tab_bar.rs)).
- `chrome::scene::build` draws the pane chrome, then the tab bar, then the overlay
  ([src/chrome/scene.rs:43-68](../../../src/chrome/scene.rs)). `panes` draws the
  dividers in `ui.pane_border` and the active border (`ACTIVE_BORDER` = 2 px) in
  `ui.palette_match`, only with several panes
  ([src/chrome/scene.rs:19](../../../src/chrome/scene.rs),
  [src/chrome/scene.rs:330-354](../../../src/chrome/scene.rs)). `blend` mixes two
  colors ([src/chrome/scene.rs:148](../../../src/chrome/scene.rs)). Interface rectangles
  are alpha-blended ([src/renderer/rects.rs:169](../../../src/renderer/rects.rs)).
- The pane context menu opens with `open_pane_menu(pane, hit)`
  ([src/app.rs:534](../../../src/app.rs)); a pane close with confirmation goes through
  `request_close(Target::Pane(id))` ([src/chrome/dialog.rs:30](../../../src/chrome/dialog.rs)).

## Technical Decisions

### Pane title bar layout (`src/chrome/pane_bar.rs`, new, pure)

- A new pure module, like `tab_bar.rs`, lays out one pane's bar from its rectangle and
  the chrome UI metrics: `PaneBarLayout { rect, title_x, title_y, title_width, close,
  active }` and `fn split(pane: Rect, metrics) -> (bar: Rect, content: Rect)`.
- Height: one chrome text line plus the tab padding (`UiFont::Chrome`), the same
  vertical rhythm as a horizontal tab. The bar is the top strip of the pane rectangle
  the tree gives; the rest is the content.
- `PaneBarLayout::hit(x, y) -> Option<PaneBarHit>` with `PaneBarHit::{Title, Close}`.
- The title is truncated by the interface text, like tab titles.

### Grid and minimum size

- `relayout` builds each `GridLayout` from the **content** rectangle, so the shell is
  resized to the rows left under the bar. `pointer_hit` and every grid mapping use
  `pane.layout`, so they follow without change.
- `Geometry` gains `title_bar: f32` (pixels). `Axis::min_pane` along `Vertical` becomes
  `MIN_PANE_LINES * cell.height + title_bar`, so a split never leaves a pane whose
  grid is below the minimum. The tree tests get a `title_bar` in their `geometry`
  helpers.
- The zoomed layout is unchanged: the zoomed pane's rectangle holds its bar.

### Drawing

- `chrome::scene::panes` draws a bar for every pane of the layout (single pane
  included), then, with several panes, the dividers and the active border as today.
  It returns rectangles and title texts, so it now writes into the `UiLayer` like
  `tabs` does.
- Active bar background: `ui.palette_match` (the active border color), title and `×`
  in `ui.palette_background` (like the default dialog button). Inactive bar:
  `blend(ui.tab_bar_background, ui.palette_match, INACTIVE_PANE_BAR)` (0.15) — a muted
  version of the same hue, no new theme key — title and `×` in `ui.tab_active_text`.
  (`tab_active_text` on the accent and `tab_inactive_text` anywhere stayed under 4.5 in
  every built-in theme.) `chrome::scene::pane_bar_colors` holds the choice.
- `build` takes the laid-out bars (`&[PaneBarLayout]`, built by `Session::pane_bars`
  from `Workspace::pane_title`), like it takes the `TabBarLayout`.
- A theme test checks, for every built-in theme, a contrast of at least 4.5 between the
  bar text and both bar backgrounds (same helper as
  `every_built_in_theme_keeps_the_palette_readable`); a theme failing it gets its
  colors adjusted in the same task.
- `build` also takes the drop highlight (below).

### Mouse on the bar

- `mouse_press` checks the pane bars after the dividers and before the pane content:
  overlay, banner, tab bar, divider, **pane bar**, pane. The bar is never reported to
  the application.
- Left on `Close` → `request_close(Target::Pane(id))`. Right on the bar →
  `open_pane_menu(id, Hit::Blank)` (no block actions). Left on `Title` → focuses the
  pane, counts the click with a `ClickTracker` keyed by pane (double click →
  `toggle_zoom`), and arms a pending grab.
- A grab only becomes a drag once the pointer moved `PANE_DRAG_THRESHOLD` (4 logical
  pixels) from the press, so a click or a double click never shows the drag chrome
  (in particular the single-tab bar).

### Drag state

`Drag` gains:

```rust
/// A pane grabbed by its title bar: pending until the pointer moves past the threshold.
Pane {
    pane: PaneId,
    press: PhysicalPosition<f64>,
    started: bool,
    /// The tab was zoomed when the pane was grabbed: restored on cancel.
    was_zoomed: bool,
    /// Tab of the bar under the pointer and since when, for the 1 s switch.
    tab_hover: Option<(usize, Instant)>,
    target: Option<DropTarget>,
},
```

- On start: unzoom the tab if zoomed (remember `was_zoomed`), set `started`, and when
  the window has a single tab, show the tab bar (`tab_bar::shown(count) ||
  pane_dragging`) and `relayout` — the shells get resized, as decided.
- Pointer: `CursorIcon::Grabbing` while started.

### Drop targets (pure, `src/workspace/drop.rs`, new)

```rust
pub enum DropTarget {
    /// Split `pane` on `side`, the moved pane on that side.
    Edge { pane: PaneId, side: Direction },
    /// Swap with `pane`.
    Center { pane: PaneId },
    /// Into tab `index`, splitting its active pane.
    Tab(usize),
    /// Into a new tab, last.
    NewTab,
}
```

- `zone(rect, x, y) -> Zone`: the center is the inner rectangle at 25 % margins;
  elsewhere the nearest edge, distances normalised by the width or height. Reuses
  `i18n::Direction` and `Axis::of`.
- Over a pane of the shown tab: `Edge` or `Center`. Over the dragged pane itself: no
  target. A target the move would refuse (too small, see below) is no target.
- Over the tab bar: `Tab(i)` on a tab or its close button, `NewTab` on `+` or the empty
  part of the bar.
- Highlight: `Edge` → the half of the target pane on that side; `Center` → the whole
  target pane; `Tab(i)` → its slot; `NewTab` → the `+` button. Drawn as one
  translucent `ui.palette_match` rectangle (`DROP_HIGHLIGHT` opacity) after the pane
  chrome and the tab bar, before the overlay.

### Tab switch on hover

- Entering a tab of the bar other than the shown one sets `tab_hover = (i, now)`;
  leaving it clears it. `Session::pane_drag_tick(now)` returns the deadline
  `since + 1 s` to `about_to_wait`; when reached, `workspace.select(i)` + `relayout`,
  the drag going on. The dragged pane stays in its tab until the drop.

### Workspace moves (pure, `src/workspace/`)

- `Node::insert(target, side, moved, geometry) -> bool`: like `split`, with `moved`
  first for `Left`/`Up`, second for `Right`/`Down`, along `Axis::of(side)`.
- `Node::replace(old, new) -> bool` (leaf swap) and `Node::swap(a, b) -> bool`.
- `Workspace::move_pane(id, target, side, geometry) -> bool`: works on clones of the
  trees involved (`remove` from the source, `insert` at the target, the same clone when
  both are in the same tab), and only commits when both succeed, so a refused move
  changes nothing. An emptied source tab is removed with `remove_tab`. The moved pane
  becomes the active pane of the target tab, which is shown; both tabs are unzoomed.
- `Workspace::swap_panes(a, b) -> bool`: within a tab, `Node::swap`; across tabs,
  `replace` in both trees and fix each tab's `active`. The moved pane gets the focus in
  the target tab, which is shown.
- `Workspace::move_pane_to_tab(id, index, geometry) -> bool`: `move_pane` onto the
  active pane of tab `index`, on its longer side (`Right` when wider than tall in
  pixels, else `Down`). Dropping on the pane's own tab does nothing.
- `Workspace::move_pane_to_new_tab(id) -> bool`: a new last tab holding the pane; for
  the only pane of its tab, this is `move_tab(index, last)`. `detach` keeps its
  behaviour (tab right after its own).
- All these are covered by unit tests in `workspace/mod.rs` and `workspace/tree.rs`,
  including the refused cases and the active-tab index after a source tab closes.

### Drop and cancel

- Release with a target: run the matching workspace move, then `relayout` (also hides
  the temporary single-tab bar). Release without target: same as cancel.
- Cancel (`Escape`, the window losing the focus, or a pane closing during the drag):
  `workspace.focus(pane)` (shows its tab again), re-zoom when `was_zoomed`, clear the
  drag, `relayout`.
- While a pane drag is started, `key_press` handles `Escape` and swallows every other
  key, so no shortcut (close, split, palette) changes the workspace under the drag.
- The close cleanup ([src/app.rs:1700-1710](../../../src/app.rs)) cancels a pane drag
  whichever pane closes: tab indices in `tab_hover` and `target` may have shifted.

### Documentation

- README: the pane title bar and the drag and drop.
- `docs/dev/architecture.md`: the new press order (pane bar before the pane), the
  `pane_bar` and `workspace::drop` modules, the `Drag::Pane` gesture.

### Alternatives considered

- *Modifier + drag anywhere in the pane*: rejected (see overview): Ctrl+Shift+click
  already extends the selection (`pane.rs:682`, `selection.rs:48-61`), the other
  modifiers clash with window managers.
- *Live preview of the layout while dragging*: rejected; it resizes every shell on
  each move. The highlight shows the result without touching the panes.
- *Tab bar drawn over the panes for a single tab*: rejected by the user in favour of
  showing it normally (panes resized while dragging).
- *No drag threshold*: rejected; every click on a title bar would show and hide the
  single-tab bar, resizing the shells twice.
- **Title bar + pure `pane_bar` / `drop` modules + clone-then-commit workspace moves**:
  retained; it keeps the geometry and the tree operations unit-testable without a
  window, like the tab bar and the split tree today.

## Découpage en tâches d'implémentation

- [#133 — Barre de titre des panes : mise en page et dessin](https://github.com/marmotz/graphene-term/issues/133)
- [#134 — Opérations de déplacement et d'échange de panes dans le workspace](https://github.com/marmotz/graphene-term/issues/134)
- [#135 — Cibles de dépôt d'un pane glissé](https://github.com/marmotz/graphene-term/issues/135)
- [#136 — Interactions de la barre de titre : focus, fermeture, zoom, menu](https://github.com/marmotz/graphene-term/issues/136) — dépend de #133.
- [#137 — Glisser-déposer d'un pane : geste, surbrillance, onglets, annulation](https://github.com/marmotz/graphene-term/issues/137) — dépend de #134, #135 et #136.
