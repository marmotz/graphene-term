# Top banner hides the first prompt — technical design

See [overview.md](overview.md) for the product decisions.

## Current code state

- The banner is one line at the top of the window. Its geometry comes from
  `BannerLayout::new` ([src/banner.rs:204](../../../src/banner.rs)): the rect always starts
  at `(0, 0)`, spans the window width, and is `cell.height + 2 * margin` high, where
  `margin = round(cell.height / 4)`. The height depends only on the cell size, never on
  the text.
- Only one banner is shown at a time: `Shown::pick` ([src/banner.rs:66](../../../src/banner.rs))
  gives the configuration error banner priority over the update banner.
  `Session::shown_banner` ([src/app.rs:171](../../../src/app.rs)) calls it.
- The tab bar and the panes ignore the banner and are laid out from the full window:
  - `Session::tab_bar` ([src/app.rs:227](../../../src/app.rs)) passes
    `Rect::from_size(size.width, size.height)` to `TabBarLayout::new`.
  - `Session::geometry` ([src/app.rs:250](../../../src/app.rs)) builds the pane area with
    `tab_position.content_area(window, thickness)` from the same full-window rect.
  - The first pane is spawned in `create_session` ([src/app.rs:1733](../../../src/app.rs))
    with a `GridLayout` over the full window, before the `Session` exists.
- Rendering: the banner is attached to the active pane's `Frame`
  ([src/app.rs:1537](../../../src/app.rs), [src/frame.rs:68](../../../src/frame.rs)) and drawn
  in the overlay pass, over the grid and the interface
  ([src/renderer/scene.rs:443](../../../src/renderer/scene.rs)). The scissor is reset to the
  full screen before that pass ([src/renderer/mod.rs:228](../../../src/renderer/mod.rs)),
  so the banner is drawn even outside the pane it is attached to.
- Mouse: `mouse_press` gives the press to the banner before the tab bar and the panes
  ([src/app.rs:931](../../../src/app.rs)), using the same `BannerLayout`.
- The banner shows or hides on these events:
  - the update check thread reports a release: `UserEvent::Update`
    ([src/app.rs:1935](../../../src/app.rs)) calls `update.set` and only requests a redraw;
  - a click on the banner (`banner_press`, [src/app.rs:183](../../../src/app.rs)): it closes
    the configuration error banner, or runs "Ignore this version" or "×" on the update
    banner, then only requests a redraw;
  - a configuration reload: `apply_config` ([src/app.rs:596](../../../src/app.rs)) sets
    `config_banner` and already calls `relayout`.

## Diagnosis

Nothing reserves the banner's height: the tab bar and the panes start at `y = 0`, the
same row as the banner, and the overlay pass draws the banner over them. Since the shell
prints its first prompt on the first grid row, the banner covers it.

## Chosen design

1. **Reserved strip.** Add to `banner.rs` a pure function that removes the banner strip
   from the top of a rect:

   ```rust
   /// `window` without the strip of `banner_height` pixels the banner takes at its top.
   pub fn below_banner(window: Rect, banner_height: f32) -> Rect
   ```

   It clamps the height to the window, like `TabBarPosition::content_area`
   ([src/workspace/mod.rs:48](../../../src/workspace/mod.rs)), and adds a
   `BannerLayout::height(cell: CellSize) -> f32` helper that `BannerLayout::new` also
   uses, so the reserved height and the drawn height are always the same value.

2. **One area for the chrome.** Add `Session::chrome_area() -> Rect`: the full window,
   then `below_banner` with the shown banner's height (0 without a banner). Use it in
   `tab_bar` and in `geometry` instead of `Rect::from_size`. `content_area` and
   `TabBarLayout::new` already honour `window.x` and `window.y`, so with the tab bar on
   any edge (top, bottom, left or right), the bar and the panes start under the banner,
   as the overview requires. The banner itself keeps the full window width and stays at
   `(0, 0)`. Its drawing and its hit test do not change.

3. **Relayout when the banner shows or hides.** `relayout` already resizes only the panes
   whose cell count changed. Call it where the banner visibility can change:
   - in `UserEvent::Update`, after `update.set`;
   - in `banner_press`, after the configuration error is dismissed or an update action
     runs (the "Open release" action does not hide the banner; the relayout is then a
     no-op);
   - `apply_config` already calls it.

   Font size changes already relayout, and they change the banner height with the cell
   height, so no extra call is needed for them.

4. **First pane.** In `create_session`, compute the first `GridLayout` over
   `below_banner(full window, height)`, where the height is computed from
   `self.config_banner` and `glyphs.cell_size()`. This way a configuration error present
   at startup does not make the shell start one grid too tall. The update banner
   always appears later, from the check thread, through step 3.

5. Update the docs that say the banner is drawn "over the grid"
   ([src/banner.rs:1](../../../src/banner.rs), [src/frame.rs:68](../../../src/frame.rs),
   [src/renderer/scene.rs:443](../../../src/renderer/scene.rs)): it is now drawn in its
   own strip, above the tab bar and the panes.

### Alternatives

- **Draw the banner in the top padding of each pane.** Rejected: with several panes or a
  top tab bar, the banner would no longer span the window above the tab bar, which the
  overview requires.
- **Move the banner rendering from the active pane's frame to the interface scene.**
  Not needed for this bug: the overlay pass already draws it unclipped at its absolute
  position. It stays a possible cleanup, out of scope.
- **Shrink only the active pane.** Rejected: the other panes and the tab bar would still
  be under the banner.

## Consequences

- When the update banner appears after startup, every pane of every tab loses one banner
  height. Each shell whose row count changes receives a resize, as with a window
  resize. Closing the banner gives the height back the same way.
- The divider drag, the pane hit test and the tab bar hit test all go through
  `shown_layout` and `tab_bar`, so they follow the shifted area with no extra change.
- The dialog, the command palette and the tooltip are laid out from the full window and
  drawn above everything. They are not affected.
- The settings window (egui, `src/settings/`) does not use `BannerLayout`: it is out of
  scope.

## Verification

- `banner.rs` unit tests: `below_banner` with a zero height (unchanged rect), a normal
  height, and a height larger than the window (clamped). `BannerLayout::new` uses
  `BannerLayout::height`.
- `tab_bar.rs` unit test: a top, left and right bar laid out in a window whose `y` is not
  0 starts at that `y`.
- `workspace` unit test: `content_area` keeps the window offset for each position.
- Manual check: with `updates` enabled and a newer release, the first prompt is visible
  under the banner. "×" gives the line back. Same with a configuration error at
  startup, with a single tab and with several tabs on each edge.

## Découpage en tâches d'implémentation

- [#97](https://github.com/marmotz/graphene-term/issues/97) Reserve the top banner strip
  so it no longer hides the prompt: steps 1 to 5 of the chosen design, with their tests.
