# Context menu — technical design

Product decisions: [overview.md](overview.md).

## Current state

- **Right click does nothing.** `Session::mouse_press`
  ([src/app.rs:925](../../../src/app.rs)) routes a press to the overlay, the banner,
  the tab bar, a divider, then a pane. In a pane, `mouse::Button::Right => {}`
  ([src/app.rs:1050](../../../src/app.rs)); on the tab bar, `tab_bar_press`
  ([src/app.rs:550](../../../src/app.rs)) ignores the right button.
- **Mouse reporting already honours Shift.** A press goes to the application when
  `selection::mouse_reserved(mode, mods) && pane.reports_at(&hit)`
  ([src/app.rs:998](../../../src/app.rs)); `mouse_reserved`
  ([src/selection.rs:59](../../../src/selection.rs)) is false while Shift is held. A
  press in an inactive pane focuses it first ([src/app.rs:962](../../../src/app.rs)).
- **Overlays.** `enum Overlay { Confirm, Palette }`
  ([src/app.rs:104](../../../src/app.rs)) takes every key (`key_press`,
  [src/app.rs:1304](../../../src/app.rs)) and every press (`overlay_press`,
  [src/app.rs:519](../../../src/app.rs)). The palette is a pure module
  ([src/chrome/command_palette.rs](../../../src/chrome/command_palette.rs)): entries,
  `PaletteLayout::new`, `hit`, drawn by `chrome::scene::palette`
  ([src/chrome/scene.rs:64](../../../src/chrome/scene.rs)) with a right-aligned
  shortcut per row, taken from `Keybindings::label(action)`
  ([src/keybindings.rs:339](../../../src/keybindings.rs)).
- **Close requests.** `request_close(Target)` ([src/app.rs:392](../../../src/app.rs))
  asks `ConfirmClose` when a foreground program runs; `Target::{Pane, Tab, Window}`
  ([src/chrome/dialog.rs:30](../../../src/chrome/dialog.rs)), `Target::tab(workspace,
  index)`. `Target::Window` exits the event loop.
- **Missing actions.** `Action` ([src/input.rs:10](../../../src/input.rs)) has no
  close-tab or quit action; closing a tab is only the tab bar's close button or middle
  click.
- **Zoom.** `Workspace::toggle_zoom` ([src/workspace/mod.rs:435](../../../src/workspace/mod.rs))
  returns `false` on a single-pane tab; the state is `Tab::zoomed`.
- **Blocks.** `pane::Hit::Cell { block, .. }` carries the block of the row
  ([src/layout.rs:78](../../../src/layout.rs)); `Hit::Margin { row }` and
  `Hit::Chevron { row }` reach it through `rows[row].block`, as `Pane::margin_press`
  does ([src/pane.rs:451](../../../src/pane.rs)). The block actions target the selected
  block or the last finished one (`block_actions::target`,
  [src/block_actions.rs:36](../../../src/block_actions.rs)).
- **Themes.** `theme::BUILT_IN` ([src/theme.rs:26](../../../src/theme.rs)) and
  `<config_dir>/themes/*.toml` (`THEMES_DIR`); no function lists them yet (the settings
  screen plans the same list, see its
  [technical.md](../settings-screen/technical.md)). `ThemeConfig::follows_appearance` /
  `active(appearance)` ([src/config.rs:372](../../../src/config.rs)), the session keeps
  `appearance` ([src/app.rs:1701](../../../src/app.rs)).
- **Config writes.** `config_edit::write(file, change)` with `ConfigEdit::set(path,
  value)` ([src/config_edit.rs:1168](../../../src/config_edit.rs)) rewrites one key and
  keeps the rest of the file; the watcher reloads it.

## Decisions

### New actions

- `Action::CloseTab` (id `close_tab`, `Msg::CloseTab`: "Close the tab" / "Fermer
  l'onglet"), default **`Ctrl+Alt+W`**: `request_close(Target::tab(workspace,
  active_index))`.
- `Action::Quit` (id `quit`, `Msg::Quit`: "Quit Graphene" / "Quitter Graphene"),
  default **`Ctrl+Shift+Q`**: `request_close(Target::Window)`, the same path as the
  window close button.
- Both added to `DEFAULTS` ([src/keybindings.rs:95](../../../src/keybindings.rs)) and
  `Action::ALL` (35 → 37), so to the palette. Neither shortcut is bound today.
- `config.default.toml` (allowed actions list) and the README shortcut table gain them.
- The keybindings test using `quit` as the example of an unknown action
  ([src/keybindings.rs:778](../../../src/keybindings.rs)) switches to another name.

### Rendering: an overlay of the chrome

Alternatives:

- *Native menu* (`muda`): on Linux it needs GTK and its own event loop integration
  with winit, and ignores the theme colors. Rejected.
- *egui*, as the settings window: the terminal window is not drawn with egui. Rejected.
- **Chrome overlay** (retained): a pure module like the palette, drawn as overlay rects
  and interface text, themed by `UiColors`, testable without a window.

New module `src/chrome/context_menu.rs`:

- `enum Command { Action(Action), CloseTab(usize), BlockCopy(BlockId, Part),
  BlockFold(BlockId), Theme(String) }`.
- `enum Item { Entry { label, shortcut: Option<String>, enabled: bool, command },
  Submenu { label, entries: Vec<Item> }, Separator }`.
- `struct MenuContext { copyable, last_pane, single_pane_tab, zoomed, only_tab, block:
  Option<BlockState>, themes: Vec<String>, active_theme: String }`, filled by the
  session when the menu opens; `pane_menu(&MenuContext, &Keybindings, Lang)` and
  `tab_menu(index, &MenuContext, &Keybindings, Lang)` build the items. Pure, unit
  tested.
- `struct ContextMenu { anchor: (f32, f32), items, hovered: Option<usize>, submenu:
  Option<SubmenuState> }` with `key(&Key) -> Option<Outcome>`, `hover(x, y)`,
  `choose(hit)`, and `MenuLayout::new(menu, window size, metrics, measure)` / `hit(x,
  y)` as `PaletteLayout`.
- `Overlay::Menu(ContextMenu)` in the session, `scene::Overlay::Menu(&MenuLayout)` in
  `scene::build`.

### Contents

Pane menu, sections split by separators:

1. Block (only when the click lands on a finished block): copy the command, copy the
   output, fold / unfold (label by `Timeline::is_folded`).
2. Copy, Paste.
3. Split vertical (`SplitRight`), Split horizontal (`SplitDown`), Close this pane,
   Zoom / Unzoom this pane.
4. New tab, Close this tab.
5. Settings, Themes ›, Quit Graphene.

Tab menu: New tab, Close this tab (`Command::CloseTab(index)` of the clicked tab), then
Settings, Themes ›, Quit Graphene.

Enabled rules (an entry that would close the whole window is disabled: Quit does that):

| Entry | Disabled when |
|---|---|
| Copy | `Pane::copy_content()` is `None` |
| Paste | never: reading the clipboard when opening could block on the selection owner, and an empty paste is a no-op |
| Close this pane | it is the only pane of the window |
| Zoom / Unzoom | the tab has a single pane (`toggle_zoom` would return `false`) |
| Close this tab | it is the only tab |
| Block copy | `block_actions::text` is `None` for that part |

Labels: the palette's `Msg` of each action, except the new `Msg::ZoomPane` /
`Msg::UnzoomPane` ("Zoom the pane" / "Unzoom the pane", "Agrandir le panneau" /
"Réduire le panneau") chosen by `Tab::zoomed`, `Msg::SplitVertical` /
`Msg::SplitHorizontal`, and `Msg::Themes`. New variants are added to the list of
every `Msg` used by the i18n tests ([src/i18n.rs:444](../../../src/i18n.rs)).
Shortcuts: `Keybindings::label(action)`, so rebinding shows the user's keys; block and
theme entries have none.

### Opening

- Pane: in `mouse_press`, the `mouse::Button::Right` branch
  ([src/app.rs:1050](../../../src/app.rs)) opens the pane menu. It is only reached when
  the press is not reported to the application, so Shift+right click and the rows
  outside the live grid already open it. The focus change above it already makes the
  clicked pane the target. The text selection is kept, so Copy copies it.
- The margin branch ([src/app.rs:985](../../../src/app.rs)) only handles the left
  button, so a right press on `Hit::Margin` / `Hit::Chevron` reaches the same branch;
  the block comes from the hit (`Hit::Cell::block`, or
  `rows[row].block`) through a new `Pane::block_at(&Hit) -> Option<BlockId>`, filtered
  to finished blocks.
- Tab bar: `tab_bar_press` gains `(Right, TabBarHit::Tab(i) | TabBarHit::Close(i))`,
  which opens the tab menu for `i` without selecting the tab.
- The menu opens on press at the pointer, shifted left and up when it would overflow
  the window, clamped to the window.

### Interaction

- Hover highlights an enabled entry; hovering "Themes" opens its submenu at the right
  of the row (left when it overflows). The submenu height is capped to the window and
  scrolls with the wheel, as the palette (`mouse_wheel`,
  [src/app.rs:1376](../../../src/app.rs)).
- Keys (the overlay takes them all): Up / Down move over the enabled entries, Right or
  Enter opens the submenu, Left closes it, Enter runs, Escape closes the submenu then
  the menu.
- A left press on an enabled entry runs it; a press outside the menu closes it and is
  not passed through. Disabled entries and separators ignore presses.
- `cursor_moved` forwards the pointer to `ContextMenu::hover` while the menu is open.

### Running a command

The menu closes first, then:

- `Command::Action(a)` → `run_action(a)`, on the now focused pane.
- `Command::CloseTab(i)` → `request_close(Target::tab(workspace, i))`.
- `Command::BlockCopy(id, part)` / `BlockFold(id)`: new `Pane::block_text_of(id, part)`
  and `Pane::toggle_fold_of(id)` (over `change_folds`). Rejected alternative: selecting
  the block and running `CopyCommand` / `ToggleFold`, because `select_block` clears the
  text selection and overwrites the primary selection.
- `Command::Theme(name)` → `config_edit::write(config_file(), |edit| edit.set(key,
  name))` with `key` = `["theme", "light"]` or `["theme", "dark"]` by the session's
  `appearance` when `follows_appearance()`, else `["theme", "name"]`. The watcher's
  reload applies it. A write error is logged and shown with the config error banner.

### Themes list

- New `theme::available(config_dir: Option<&Path>) -> Vec<String>`: the `themes/*.toml`
  stems sorted, then the `BUILT_IN` names not already listed. It is the list the
  settings screen plans for its theme combos: whichever feature lands first adds it,
  the other reuses it.
- The active theme (`theme.active(appearance)`) is drawn in the accent color
  (`palette_match`), avoiding a check-mark glyph the interface font may lack.

### Drawing

`scene::menu`: box in `palette_background` with a `pane_border` border, hovered row in
`tab_active_background`, labels in `palette_text`, shortcuts and disabled labels in
`tab_inactive_text`, separators as 1 px `pane_border` rects, the submenu arrow as text
`›`. Font: `UiFont::Palette`.

## Consequences

- `Action::ALL` grows: the palette's default order and the keybindings tests
  (`labels_are_the_first_default_shortcut`) cover the new actions.
- A right click inside a mouse-reporting application still goes to the application:
  no behaviour change for vim, htop or tmux users.
- The release of the opening right press reaches `mouse_release` with no gesture:
  `release_gesture` ignores it, nothing to change.
- Depends on [settings-screen](../settings-screen/overview.md) for `Action::OpenSettings`
  and `config_edit::write`, both on the current branch.

## Découpage en tâches d'implémentation

1. [#100](https://github.com/marmotz/graphene-term/issues/100) Close tab and quit
   actions.
2. [#101](https://github.com/marmotz/graphene-term/issues/101) List the available
   themes.
3. [#102](https://github.com/marmotz/graphene-term/issues/102) Menu module, layout and
   drawing (depends on #100).
4. [#103](https://github.com/marmotz/graphene-term/issues/103) Open, interact and run
   from panes and tabs (depends on #102, #100, #101, and settings-screen #86, #88).
