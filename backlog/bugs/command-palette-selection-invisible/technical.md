# Command palette selection is invisible — technical design

Product decisions: [overview.md](overview.md).

## Current state

- **Keyboard handling works.** `CommandPalette::key`
  ([src/chrome/command_palette.rs:201](../../../src/chrome/command_palette.rs)) moves
  the selection on `Up` / `Down` (wrapping, scrolling the visible rows) and runs it on
  `Enter`; `Session::key_press` ([src/app.rs:1319](../../../src/app.rs)) sends every key
  to the open palette first. Covered by the tests around
  [src/chrome/command_palette.rs:714](../../../src/chrome/command_palette.rs).
- **The selected row is painted with the active tab color.**
  `PaletteLayout::row` flags it (`selected: index == palette.selected()`,
  [src/chrome/command_palette.rs:559](../../../src/chrome/command_palette.rs)) and
  `scene::palette` fills its rect with `ui.tab_active_background`
  ([src/chrome/scene.rs:78](../../../src/chrome/scene.rs)):

  ```rust
  if let Some(row) = palette.rows.iter().find(|row| row.selected) {
      ui.overlay_rects
          .push(rect(row.rect, color(colors.tab_active_background)));
  }
  ```

  The palette box is filled with `ui.palette_background` just before.
- **Both colors are identical in several bundled themes**, so the selection is
  invisible:

  | Theme | `palette_background` | `tab_active_background` |
  |---|---|---|
  | graphene-night (default) | `#1c1f26` | `#1c1f26` |
  | solarized-dark | `#073642` | `#073642` |
  | gruvbox-dark | `#3c3836` | `#3c3836` |
  | graphene-day | `#ffffff` | `#f6f7f9` (contrast 1.08) |

- **Borders blend into the palette in three themes.** The palette, the close dialog and
  the link tooltip draw a 1 px border in `ui.pane_border` around a
  `ui.palette_background` box ([src/chrome/scene.rs:69](../../../src/chrome/scene.rs),
  [:119](../../../src/chrome/scene.rs), [:162](../../../src/chrome/scene.rs)); the
  settings window does the same with egui's `window_fill` / `window_stroke`
  ([src/settings/visuals.rs:35](../../../src/settings/visuals.rs)). `pane_border`
  equals `palette_background` in solarized-dark (`#073642`), solarized-light
  (`#eee8d5`) and gruvbox-dark (`#3c3836`).
- **Other uses checked, no problem found.** The active tab (`tab_active_background`
  over `tab_bar_background`, [src/chrome/scene.rs:183](../../../src/chrome/scene.rs))
  differs from the bar in every theme. In the settings window, hovered and active
  controls take `tab_active_background`, which equals the panel background in several
  themes, but they also get a 1 px `palette_match` stroke
  ([src/settings/visuals.rs:53](../../../src/settings/visuals.rs)), so they stay
  visible.
- **Theme loading.** `[ui]` keys are mapped by `ui_slot`
  ([src/theme.rs:211](../../../src/theme.rs)); a key missing from a theme keeps the
  value of the base palette, `graphene-night` (`theme::load`,
  [src/theme.rs:79](../../../src/theme.rs)). `every_built_in_theme_sets_every_color`
  ([src/theme.rs:378](../../../src/theme.rs)) fails when a built-in theme omits a
  color, through `UiColors::uniform` ([src/palette.rs:45](../../../src/palette.rs)).

## Decisions

### New theme color `ui.palette_selection_background`

- Added to `UiColors` ([src/palette.rs:29](../../../src/palette.rs)), to
  `UiColors::uniform` and to `ui_slot`. Documented next to `palette_match`.
- `scene::palette` fills the selected row with it instead of `tab_active_background`.
- A user theme without the key gets the `graphene-night` value, like every other
  missing key: no special rule in `theme::parse`.
- Values for the bundled themes, taken from each theme's own palette where it has one.
  Contrast against `palette_background` is low on purpose (a background tint); the text
  stays readable on it:

  | Theme | `palette_background` | `palette_selection_background` | vs background | text on it |
  |---|---|---|---|---|
  | graphene-night | `#1c1f26` | `#2e3440` | 1.32 | 8.92 |
  | graphene-day | `#ffffff` | `#e3e9f5` | 1.22 | 11.03 |
  | solarized-dark | `#073642` | `#002b36` (base03) | 1.15 | 5.61 |
  | solarized-light | `#eee8d5` | `#fdf6e3` (base3) | 1.14 | 4.99 |
  | gruvbox-dark | `#3c3836` | `#504945` (bg2) | 1.31 | 6.43 |
  | gruvbox-light | `#ebdbb2` | `#d5c4a1` (bg2) | 1.25 | 6.76 |
  | catppuccin-mocha | `#181825` | `#313244` (surface0) | 1.40 | 8.69 |
  | catppuccin-latte | `#e6e9ef` | `#ccd0da` (surface0) | 1.27 | 5.17 |

  Contrast is the WCAG ratio of relative luminances.

### Accent bar on the selected row

- A vertical bar on the left edge of the selected row, in `ui.palette_match` (the
  accent, already used for the active pane border), so the selection stays visible even
  when a user theme sets `palette_selection_background` too close to the background.
- Computed by the pure layout, not by the scene: `PaletteLayout` gets
  `marker: Option<Rect>`, `Some` when a row is selected, at the row's `x` / `y` /
  `height`, `MARKER_WIDTH * scale` wide (`MARKER_WIDTH = 2.0`, the width of
  `scene::ACTIVE_BORDER`), rounded to whole pixels and at least 1 px. The label starts
  `TEXT_PADDING` (12 px) into the row, so the bar never covers text.
- `scene::palette` pushes the marker rect right after the selected row's background.

### Border colors of three bundled themes

`ui.pane_border` changes to a color of the theme's palette that stands out from both
the palette box and the terminal background (it also draws the pane dividers):

| Theme | Old | New | vs palette | vs terminal |
|---|---|---|---|---|
| solarized-dark | `#073642` | `#586e75` (base01) | 2.42 | 2.79 |
| solarized-light | `#eee8d5` | `#93a1a1` (base1) | 2.18 | 2.48 |
| gruvbox-dark | `#3c3836` | `#665c54` (bg3) | 1.78 | 2.26 |

### Guard test on the bundled themes

A test in `theme.rs` parses every built-in theme and asserts a WCAG contrast of at
least 1.1 between `palette_background` and each of `palette_selection_background` and
`pane_border`, and at least 4.5 between `palette_text` and
`palette_selection_background`. The contrast helper lives in the test module only.

## Alternatives considered

- **Derive the selection color from `palette_background`** (lighten or darken it):
  no new key, but a rule specific to one key and no control for theme authors.
  Rejected in favor of a dedicated key.
- **Fall back to the theme's `tab_active_background` when the key is missing:** keeps
  existing user themes unchanged, but that is exactly the color that hides the
  selection. Rejected; the accent bar covers user themes either way.
- **Outline or accent-colored label as the marker:** an outline is heavier than the
  rest of the palette; an accent label clashes with the accent used for search matches.
  Rejected for the left bar.
- **Fix only the theme values, no new key:** changing `tab_active_background` would
  also change the tab bar. Rejected.

## Consequences

- `UiColors` gains a field: every full struct literal of it must set it. Today those
  are `UiColors::uniform` and the test helpers `colors()` and `tab_colors()`
  ([src/chrome/scene.rs:297](../../../src/chrome/scene.rs),
  [:389](../../../src/chrome/scene.rs)); the other test literals spread
  `..tab_colors()` and keep compiling.
- `palette_is_drawn_in_the_overlay`
  ([src/chrome/scene.rs:555](../../../src/chrome/scene.rs)) counts 8 overlay rects and
  checks the selected row color `ACTIVE_TAB`: it becomes 9 rects, the row in the new
  color and the marker in the accent.
- `every_built_in_theme_sets_every_color` fails until every bundled theme sets the new
  key, which keeps the eight themes complete.
- The settings window reads `UiColors` but does not draw the palette: no change there.
- Existing user themes: the selection takes `#2e3440` unless they set the key, and the
  accent bar shows it in any case. No migration.

## Découpage en tâches d'implémentation

1. [#98](https://github.com/marmotz/graphene-term/issues/98) Give the command palette
   selection its own theme color and fix invisible borders (new key, theme values,
   `pane_border` fixes, guard test).
2. [#99](https://github.com/marmotz/graphene-term/issues/99) Mark the selected command
   palette row with an accent bar. Depends on #98.
