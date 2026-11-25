# Quick terminal at full height goes under the desktop panel: technical design

See [overview.md](overview.md) for the product decisions.

## Diagnosis

The quick terminal frame is computed from the rectangle returned by `usable_rect`
([src/app/quick.rs:270](../../../src/app/quick.rs)):

```rust
/// The rectangle of `monitor` the quick terminal may cover: the whole screen on Linux
/// (`_NET_WORKAREA` spans every screen, and the window is above the panels), the part
/// the menu bar and the Dock leave on macOS.
fn usable_rect(monitor: &MonitorHandle) -> PixelRect {
    // ...
    #[cfg(target_os = "macos")]
    let rect = rect.inset(quick_terminal::macos::insets(monitor));
    rect
}
```

- **macOS**: the frame already excludes the menu bar and the Dock, through
  `NSScreen.visibleFrame` ([src/quick_terminal/macos.rs:52](../../../src/quick_terminal/macos.rs)).
  This matches the decisions (an auto-hidden Dock reserves no space in `visibleFrame`).
  No change.
- **Linux X11**: the whole monitor is used, on the assumption that the window stays above
  the panels. It does not: the quick terminal is a `_NET_WM_WINDOW_TYPE_DOCK` window
  ([src/app/application.rs:241](../../../src/app/application.rs)) with
  `_NET_WM_STATE_ABOVE` ([src/x11_window.rs:26](../../../src/x11_window.rs)), and Cinnamon
  draws its panels in the compositor, above every client window. At 100%, the part of
  the window behind a panel is hidden.

winit 0.30.13 exposes no work area on `MonitorHandle` (checked in its sources), so it has
to be read from the X server.

What the window manager publishes, observed on Cinnamon (Muffin) with two 2560x1440
screens and a 40 px bottom panel on each:

```
_NET_WORKAREA(CARDINAL)     = 0, 0, 5120, 1400, 0, 0, 5120, 1400
_GTK_WORKAREAS_D0(CARDINAL) = 0, 0, 2560, 1400, 2560, 0, 2560, 1400
```

No window of `_NET_CLIENT_LIST` declares `_NET_WM_STRUT_PARTIAL`: Cinnamon's panels are
not client windows.

## Retained design

### Source of the work area on X11: a fallback chain

For each monitor, in physical pixels (X11 root coordinates are physical pixels):

1. **`_GTK_WORKAREAS_D<n>`** on the root window, `<n>` being `_NET_CURRENT_DESKTOP`
   (0 when absent). A list of `x, y, width, height` quadruplets, one per monitor, set by
   Mutter and Muffin (GNOME, Cinnamon, Budgie). The monitor's work area is the
   quadruplet with the largest intersection with the monitor rectangle, clipped to it.
2. Otherwise, **the struts of the client windows**: for each window of
   `_NET_CLIENT_LIST`, `_NET_WM_STRUT_PARTIAL` (12 cardinals), else `_NET_WM_STRUT`
   (4 cardinals, each strut covering the whole edge). Each strut is a band reserved
   against an edge of the root window; its intersection with the monitor, when it
   touches the matching side of the monitor, gives the inset of that side. The largest
   inset wins per side. Covers KDE, XFCE, MATE, polybar, tint2, etc.
3. Otherwise, or on any X11 error (logged at `debug`), **the whole monitor**, as today.

A panel that reserves no space (auto-hide) appears in none of these sources, so it is not
avoided, as decided.

### Alternatives considered

- **`_NET_WORKAREA` intersected with the monitor**: rejected. It is one rectangle for the
  whole desktop; with screens of different sizes or a panel on an inner edge, it cuts
  space on screens that have no panel.
- **`_GTK_WORKAREAS` only**: rejected. KDE, XFCE and the light window managers would keep
  the bug.
- **Stacking the window above the panels**: rejected. The decision is that the quick
  terminal is never covered *and* does not cover the panels, and compositor-drawn panels
  cannot be stacked under a client window anyway.

### Code changes

**[src/quick_terminal/mod.rs](../../../src/quick_terminal/mod.rs)**: pure geometry, tested
without a display.

- `Insets` and `PixelRect::inset` (lines 72 to 103) lose their
  `cfg_attr(not(target_os = "macos"), allow(dead_code))`: Linux uses them now.
- `PixelRect::intersection(self, other) -> Option<PixelRect>`.
- `struct Strut { left, right, top, bottom, left_start_y, left_end_y, right_start_y,
  right_end_y, top_start_x, top_end_x, bottom_start_x, bottom_end_x }` with
  `Strut::from_partial(&[u32; 12])` and `Strut::from_full(&[u32; 4], root: (u32, u32))`.
- `strut_insets(monitor: PixelRect, root: (u32, u32), struts: &[Strut]) -> Insets`.
- `gtk_work_area(monitor: PixelRect, areas: &[PixelRect]) -> Option<PixelRect>`.
- `rects_from_cardinals(values: &[u32]) -> Vec<PixelRect>`: quadruplets to rectangles,
  an incomplete trailing group ignored.

**[src/x11_window.rs](../../../src/x11_window.rs)**: the X11 reads, kept thin.

- `pub fn work_areas(monitors: &[PixelRect]) -> anyhow::Result<Vec<PixelRect>>`: one
  connection, reads `_NET_CURRENT_DESKTOP`, `_GTK_WORKAREAS_D<n>`, else
  `_NET_CLIENT_LIST` and each window's struts, and returns one rectangle per monitor
  (the monitor itself where nothing applies) through the pure functions above. A window
  destroyed between the list and its read is skipped.

**[src/app/quick.rs](../../../src/app/quick.rs)**:

- `placement` (line 289) builds the monitor rectangles, then on Linux replaces them with
  `x11_window::work_areas` (one X connection per placement, not per monitor). On error,
  the monitor rectangles are kept and the error logged at `debug`.
- `usable_rect` (line 270) keeps only the macOS insets; its comment is rewritten.
- `placement` is already called at every `show_quick` and at the window creation
  ([src/app/application.rs:243](../../../src/app/application.rs)): the work area is read
  again at each show, as decided, with no extra code.

### Slide animation

`hidden_origin` ([src/quick_terminal/mod.rs:188](../../../src/quick_terminal/mod.rs)) is
derived from the frame. With a frame inside the work area, the slide starts at the edge of
the work area, just behind the panel, as decided. During the first frames, a band of the
window may overlap the panel. No change to `hidden_origin`.

### Consequences checked

- **Monitor selection**: `pick_monitor` tests the cursor against `Monitor::rect`. With
  work areas, a cursor over a panel would fall outside every rectangle and the active
  screen would fall back to the primary one. `Monitor` therefore keeps the physical
  monitor rectangle for `pick_monitor`, and gets a separate `work_area` used by `frame`.
  This also holds for macOS, where the cursor over the menu bar has the same flaw today.
- **Initial size**: `Placement::logical` is used as the window's restore size at
  creation; it now follows the work area. No other user.
- **Wayland**: no quick terminal, unaffected.
- **Settings**: nothing to change; the size setting keeps its meaning, as a share of the
  work area.

## Tests

- `quick_terminal::tests`:
  - `rects_from_cardinals` with 0, 1, 2 groups and an incomplete trailing group.
  - `gtk_work_area` picks the area of the right monitor among two, clips an area wider
    than the monitor, returns `None` without overlap.
  - `strut_insets`: bottom panel on one screen of two; top strut whose range misses the
    monitor; left panel on the inner edge of the right screen (no inset); full strut from
    `_NET_WM_STRUT`; largest strut wins per side.
  - `frame` on a work area at 100% equals the work area, for each edge.
  - `pick_monitor` with the cursor over a panel still picks that screen.
- `x11_window::tests`: `work_areas` on the test display returns one rectangle per monitor,
  each inside its monitor (skipped without an X11 display, like the existing tests).
- `app::quick::tests`: the existing `placement_keeps_physical_pixels...` test stays green.

## Découpage en tâches d'implémentation

1. [#260](https://github.com/marmotz/graphene-term/issues/260) Work area geometry and
   screen selection on the physical screen.
2. [#261](https://github.com/marmotz/graphene-term/issues/261) Read the X11 work area so
   the quick terminal fits beside the desktop panels. Depends on #260.
