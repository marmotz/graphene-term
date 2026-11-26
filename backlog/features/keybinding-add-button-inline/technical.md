# Inline "+" button to add a keybinding: technical design

Product decisions: [overview.md](overview.md).

## Current code

The keyboard shortcuts section is drawn by `View::shortcuts`
([shortcuts.rs:157](../../../src/settings/view/shortcuts.rs)). Each row is laid out in
three absolutely positioned columns with `column()`
([view/mod.rs:466](../../../src/settings/view/mod.rs)):

1. the action label, `name_width` wide;
2. the shortcut chips, in a `left_to_right` wrapping layout, `slots_width` wide, each chip
   at least `slot_width` wide so they align across rows; "Aucun raccourci"
   (`Msg::NoShortcut`) fills one slot when the action has none and is not being captured
   ([shortcuts.rs:231-237](../../../src/settings/view/shortcuts.rs));
3. the "Ajouter" / "Annuler" button, right-aligned at `left + total - add_width`
   ([shortcuts.rs:252-267](../../../src/settings/view/shortcuts.rs)). `add_width` is the
   widest of the two labels ([shortcuts.rs:188-192](../../../src/settings/view/shortcuts.rs))
   and is subtracted from `slots_width` ([shortcuts.rs:213-214](../../../src/settings/view/shortcuts.rs)).

Column 3 is what drifts away from the chips when the settings tab is wide.

The capture hint (`Msg::PressShortcut`) is drawn under the row, at the left of the chips
column ([shortcuts.rs:268-284](../../../src/settings/view/shortcuts.rs)), and the conflict
row below it ([shortcuts.rs:289-291](../../../src/settings/view/shortcuts.rs)).

`Msg::AddShortcut` and `Msg::NoShortcut` are also used by `shortcut_field`
([view/mod.rs:405](../../../src/settings/view/mod.rs)), the single-shortcut field of the
Quick terminal section. That field is **out of scope** (decided: its button already sits
next to its shortcut), so both messages stay.

## Design

### Row layout

- Drop column 3 and `add_width`. `slots_width` becomes
  `(total - name_width - SHORTCUTS_GAP).max(0.0)`: the chips column extends to the row's
  right edge.
- In the chips column, after the `for shortcut in shortcuts` loop, add the row's button
  in the same wrapping layout, so it follows the last chip (or comes first when the
  action has none):
  - not capturing for this action: `+`, tooltip `Msg::AddShortcutHint`, click sets
    `capturing = Some(Capture::Bind(action))` and clears `conflict` (same effect as
    today's "Ajouter");
  - capturing for this action: `×` (`CHIP_REMOVE`, reused), tooltip `Msg::Cancel`, click
    sets `capturing = None` (same effect as today's "Annuler").
- Remove the "Aucun raccourci" label from the rows (block at
  [shortcuts.rs:231-237](../../../src/settings/view/shortcuts.rs)): an empty row shows the
  button alone.
- `slot_width` no longer needs "Aucun raccourci" as its minimum: its fold starts at `0.0`
  ([shortcuts.rs:184-187](../../../src/settings/view/shortcuts.rs)).
- Hint and conflict rows are unchanged; they keep starting at the chips column's left.

The button lives in a small helper next to `chip` in
[view/mod.rs](../../../src/settings/view/mod.rs):

```rust
/// Mark of the button adding a shortcut to an action.
const ADD_MARK: &str = "+";

/// A square button showing `mark`, as tall as a chip.
fn square_button(ui: &Ui, mark: &str) -> Button<'static> {
    let side = ui.spacing().interact_size.y;
    Button::new(mark.to_owned()).min_size(Vec2::splat(side))
}
```

`interact_size.y` is the minimum height egui gives a button, so the square matches the
chips' height in the default style.

### Wrapping

The chips layout wraps (`with_main_wrap(true)`). When the last chip ends close to the
right edge, the button wraps to the next line on its own. Accepted: it still sits right
after the last chip in reading order, and the row grows as it already does for many
chips.

### i18n

New message `Msg::AddShortcutHint` in [src/i18n.rs](../../../src/i18n.rs), next to
`AddShortcut`:

- doc comment: `/// Tooltip of the button adding a shortcut to an action.`
- En: `"Add shortcut"`, Fr: `"Ajouter un raccourci"`;
- `variant_index`: `104` (current highest is `103`);
- add it to `all_messages()` in the tests.

`Msg::AddShortcut` keeps its "Add" / "Ajouter" text for the Quick terminal field. Its doc
comment ("Button adding a shortcut to an action.") becomes "Button setting the shortcut of
an empty shortcut field." to match its only remaining use.

### Docs

The docs do not name the "Ajouter" button but describe adding a shortcut as "press a new
shortcut". Make the click explicit:

- [docs/en/keybindings.md:99](../../../docs/en/keybindings.md): "click **+** after an
  action's shortcuts and press the new one to add it, or remove one";
- [docs/fr/keybindings.md:104](../../../docs/fr/keybindings.md): "cliquez sur **+** après
  les raccourcis d'une action puis appuyez sur le nouveau pour l'ajouter, ou
  supprimez-en un";
- [docs/en/configuration.md:84](../../../docs/en/configuration.md) and
  [docs/fr/configuration.md:89](../../../docs/fr/configuration.md): same wording for the
  "Keyboard shortcuts" bullet.

Also update the doc comment of `View::shortcuts`
([shortcuts.rs:153-156](../../../src/settings/view/shortcuts.rs)): "the button that
captures a new one right after them".

## Alternatives considered

- **Keep "Ajouter" text, move it after the chips**: fixes the distance but takes a full
  word's width on each of ~70 rows and makes them visually heavy. Rejected in favor of the
  "+" (overview decision).
- **"+" as a fixed column right after the widest slots** (aligned across rows): stays
  aligned, but on rows with one shortcut the "+" would sit far from it again, recreating
  the problem on a smaller scale. Rejected.
- **Pending chip "Appuyez sur une touche..." in place of the "+" during capture**:
  rejected at product level; the "×" plus the hint under the row was chosen.

## Consequences

- Behaviour (`capturing`, `conflict`, `captured_key`) is unchanged; only the drawing of
  `View::shortcuts` changes. The existing capture and conflict tests in
  [view/mod.rs](../../../src/settings/view/mod.rs) keep passing as they are.
- Rows get wider room for chips (the former `add_width` + one `SHORTCUTS_GAP`), so fewer
  rows wrap.

## Tests

In the `tests` module of [view/mod.rs](../../../src/settings/view/mod.rs), using
`egui::Context::run_ui` and the text shapes of the output, as
`a_chip_centers_its_shortcut_left_of_a_right_aligned_removal_mark` does:

- `the_add_button_follows_the_last_shortcut_of_its_row`: draw the shortcuts section
  (`View { section: SettingsSection::KeyboardShortcuts, .. }`, `model("")`) and check:
  no "Ajouter" text is drawn; for a row with two default shortcuts (e.g. "Ctrl+=" /
  "Ctrl++" of font increase), the "+" text is on the same line, to the right of the last
  chip's text and before half of the screen width.
- `an_action_without_shortcut_shows_only_the_add_button`: with a filter keeping an
  action without default shortcut (`Action::DetachPane`), no "Aucun raccourci" text is
  drawn and one "+" is.
- `the_add_button_becomes_a_cancel_mark_while_capturing`: with
  `capturing: Some(Capture::Bind(Action::DetachPane))` and the same filter, no "+" is
  drawn, one "×" is (the action has no chip, so it is the button's), and the
  `PressShortcut` hint is drawn.
- `square_button` is exactly `interact_size.y` wide and tall for "+" and "×".

In [src/i18n.rs](../../../src/i18n.rs): the existing coverage tests pick up the new
variant once it is in `all_messages()`; `settings_texts` asserts both texts of
`Msg::AddShortcutHint`.

## Découpage en tâches d'implémentation

1. [#273](https://github.com/marmotz/graphene-term/issues/273): inline "+" button, i18n,
   docs and tests (single task: the change is confined to one view).
