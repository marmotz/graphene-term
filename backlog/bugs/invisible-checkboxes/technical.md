# Invisible checkboxes - technical design

Product scope: [overview.md](overview.md).

## Cause

egui (0.37.0) draws a checkbox from the `WidgetVisuals` of its interaction state
(`egui-0.37.0/src/theme/default_style.rs`, `impl StyleProvider<CheckboxStyle> for
DefaultStyle`):

```rust
checkbox_frame: Frame {
    fill: widget_visuals.bg_fill,
    stroke: widget_visuals.bg_stroke,
    ..
},
check_stroke: widget_visuals.fg_stroke,
```

At rest, the state is `widgets.inactive`. The settings window derives it from the theme
in [src/settings/visuals.rs:48](../../../src/settings/visuals.rs):

```rust
for state in [&mut widgets.inactive, &mut widgets.open] {
    state.bg_fill = color(ui.tab_bar_background);
    state.weak_bg_fill = color(ui.tab_bar_background);
    state.fg_stroke.color = text;
}
```

`bg_stroke` is not set, so it keeps the value from `Visuals::dark()` / `Visuals::light()`,
which is `Stroke::default()`: no contour. An unchecked box is therefore only a
`tab_bar_background` square on the `background` panel, and the two colors are very close
in every built-in theme (e.g. graphene-night `#0f1115` on `#14161b`, catppuccin-mocha
`#11111b` on `#1e1e2e`, graphene-day `#e8eaee` on `#f6f7f9`).

A checked box stays visible because its check mark uses `fg_stroke`, the text color.

## Fix

In `visuals()` ([src/settings/visuals.rs](../../../src/settings/visuals.rs)), give the
`inactive` and `open` states a 1 px contour colored by the theme text dimmed halfway
toward the background:

```rust
let rest_border = Stroke::new(1.0, color(blend(palette.foreground, palette.background, 0.5)));
for state in [&mut widgets.inactive, &mut widgets.open] {
    // existing fills and fg_stroke
    state.bg_stroke = rest_border;
}
```

- `blend` is the existing helper `crate::frame::blend(from, to, amount)`
  ([src/frame.rs:154](../../../src/frame.rs)), which mixes each sRGB channel. It is
  currently private to the crate's `frame` module but `pub` inside it, so
  `crate::frame::blend` can be reached from `settings::visuals`.
- `hovered` / `active` keep their accent contour ([src/settings/visuals.rs:53](../../../src/settings/visuals.rs)), so hover and click feedback are unchanged.
- The checked box keeps its current look: only its frame gains the same contour.

### Affected widgets

`inactive.bg_stroke` is read by every egui widget at rest, so the contour appears on all
the controls of the settings window (decided in the overview):

- checkboxes: both call sites in [src/settings/view.rs](../../../src/settings/view.rs) (the
  `Widget::Toggle` branch of `field`, and the "Keep the text color" checkbox of the theme
  editor);
- buttons, combo boxes (`Widget::Choice`), sliders;
- text fields: egui's `TextEditStyle` already reads `widgets.inactive.bg_stroke` for its
  frame (`default_style.rs`, text edit provider), so text fields gain the same contour.

No other window uses these visuals: they are installed only on the settings context
([src/settings/mod.rs:174](../../../src/settings/mod.rs)).

## Alternatives considered

- **Checkbox-only style** (`ctx.add_widget_theme::<CheckboxStyle>` with a custom
  `StyleProvider`): limits the change to checkboxes. Rejected: the user chose to give all
  controls at rest a visible contour.
- **`pane_border` as contour color**: consistent with the other borders, but still close
  to the background in graphene-night (`#2a2e37` on `#14161b`) and catppuccin-latte
  (`#ccd0da` on `#eff1f5`). Rejected.
- **Full text color**: strongest contrast, but heavier than the rest of the UI. Rejected.
- **Changing the box fill** instead of adding a contour: any fill derived from the theme
  stays close to the panel in some theme, and a filled square reads as "checked".
  Rejected.

## Consequences

- Visual change on every settings control at rest; nothing changes in the terminal,
  chrome or command palette.
- Custom themes get the contour automatically: it is computed from `foreground` and
  `background`, no new theme key and no migration.
- Tests: extend `visuals_take_the_theme_colors` in
  [src/settings/visuals.rs](../../../src/settings/visuals.rs) to assert
  `widgets.inactive.bg_stroke` and `widgets.open.bg_stroke` equal the halfway blend of
  `foreground` and `background`, with a non-zero width.

## Découpage en tâches d'implémentation

1. [Paramètres] Contour visible des contrôles au repos:
   https://github.com/marmotz/graphene-term/issues/166
