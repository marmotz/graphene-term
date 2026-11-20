# Custom themes — Technical Design

Product decisions: [overview.md](overview.md).

## Current State

### Theme files and loading

- `theme::available` lists `themes/*.toml` (sorted) then the built-ins not overridden
  ([src/theme.rs:82-102](../../../src/theme.rs)). `theme::load` resolves a name: user
  file first, then built-in, else `graphene-night` with an error; a name holding `/`,
  `\` or starting with `.` is never read from disk
  ([src/theme.rs:104-143](../../../src/theme.rs)).
- `theme::parse` reads a theme on top of a base palette, key by key: `colors.*`,
  `colors.normal.*`, `colors.bright.*`, `ui.*`; `selection_foreground = ""` means
  `None` ([src/theme.rs:145-221](../../../src/theme.rs)). Key names are only known by
  the `match` arms of `colors_slot` / `ui_slot`
  ([src/theme.rs:223-250](../../../src/theme.rs)) and `ANSI_NAMES`; there is no list of
  every color key and no writer of a theme file.
- `Palette` / `UiColors` ([src/palette.rs:11-44](../../../src/palette.rs)): 6 terminal
  colors (one optional), 16 ANSI colors inside `indexed`, 11 UI colors, 33 keys in all.
- Every built-in sets every color (`every_built_in_theme_sets_every_color`), and the
  third-party ones carry their licence in a header comment
  (`third_party_themes_carry_their_licence`,
  [src/theme.rs:470-505](../../../src/theme.rs)).

### Hot reload

- The watcher reloads on any create/modify/remove of `config.toml` or `themes/*.toml`,
  debounced by 100 ms ([src/watcher.rs:19](../../../src/watcher.rs),
  [src/watcher.rs:93-111](../../../src/watcher.rs)), and watches `themes/` as soon as it
  exists ([src/watcher.rs:62-72](../../../src/watcher.rs)).
- `App::reload_config` reloads config + active theme, then calls
  `SettingsWindow::apply` and `Session::apply_config`
  ([src/app.rs:2557-2580](../../../src/app.rs)). The session stores the palette in
  `self.palette` ([src/app.rs:913](../../../src/app.rs)) and reads it at every frame
  ([src/app.rs:2129](../../../src/app.rs), [src/app.rs:2181](../../../src/app.rs)).
- The active theme key is `theme.name`, or `theme.light` / `theme.dark` when following
  the system appearance (`theme_key`, [src/app.rs:2861-2870](../../../src/app.rs)).

### Settings window

- Same process and event loop as the terminal: `App` owns `settings: Option<SettingsWindow>`
  and routes its window events ([src/app.rs:2722-2738](../../../src/app.rs)).
- Model/view split: `SettingsModel` turns inputs into `Change`s on `config.toml` key
  paths ([src/settings/model.rs:84-88](../../../src/settings/model.rs)); `View::show`
  returns them each frame and `SettingsWindow::write` writes them through
  `config_edit::write` ([src/settings/mod.rs:239-256](../../../src/settings/mod.rs),
  [src/settings/mod.rs:396](../../../src/settings/mod.rs)).
- The Theme section holds 4 fields: follow toggle, name, light, dark
  ([src/settings/model.rs:139-144](../../../src/settings/model.rs)), drawn in a 2-column
  grid by `section_body` ([src/settings/view.rs:149-189](../../../src/settings/view.rs)).
  `apply` refreshes the theme choices on every reload
  ([src/settings/mod.rs:156-170](../../../src/settings/mod.rs)).
- egui 0.37 ships `egui::color_picker::color_edit_button_srgb(ui, &mut [u8; 3]) -> Response`
  (a swatch button opening a picker popup).

### Text-preserving edits

- `ConfigEdit` wraps a `toml_edit::DocumentMut`; `set` creates missing parent tables
  (nested ones included) and keeps the decor of an existing key, `remove` drops a key
  ([src/config_edit.rs:1042-1152](../../../src/config_edit.rs)).
- `config_edit::write` reads the file again, applies the edit, writes atomically
  (temp + rename, seen as one change by the watcher). A missing file starts from the
  default `config.toml` text ([src/config_edit.rs:1165-1185](../../../src/config_edit.rs)),
  which is wrong for a theme file.

## Design

### 1. Color keys table and theme writer (`src/theme.rs`)

- Add `pub enum ColorKey` (33 variants) with `ColorKey::ALL`, `path(self) -> &'static [&'static str]`
  (`["colors", "normal", "red"]`, `["ui", "pane_border"]`, ...), `get(self, &Palette) -> Option<Rgb>`
  and `set(self, &mut Palette, Option<Rgb>)` (`None` only for `SelectionForeground`).
  `parse` keeps its structure but resolves keys through `ColorKey` instead of
  `colors_slot` / `ui_slot` / `ANSI_NAMES`, so the parser and the writer cannot drift.
- Add `pub fn format_color(Rgb) -> String` (`"#rrggbb"`), inverse of `parse_color`.
- Add `pub fn user_file(config_dir, name) -> Option<PathBuf>` returning
  `themes/<name>.toml` for a valid name, used by `load` and by the editor.
- Add `pub fn valid_name(name) -> Result<(), NameError>`: non-empty, no `/` or `\`, no
  leading `.`, not already in `available()` (user or built-in). Same rule for
  duplicate and rename.

Alternative rejected: serializing a fresh complete file with `toml` for every write.
It would drop the comments of hand-written files, against the overview.

### 2. Theme file operations (`src/theme_edit.rs`, new)

All writes go through `ConfigEdit` + `write_atomic`, so they keep comments and are seen
as one change by the watcher. `config_edit::write` gets a sibling
`config_edit::write_existing(file, change)` that fails with a read error on a missing
file instead of starting from the default config text.

- `set_color(dir, name, key, Option<Rgb>)`: `write_existing` + `edit.set(key.path(), ...)`;
  `None` writes `""`. Only the edited key changes (overview: "editing an existing user
  file only changes the edited keys").
- `duplicate(dir, from, to)`: start from the source text (the user file, or the built-in
  source), then `set` every `ColorKey` absent from the document to its effective value
  in `theme::load(from)`. Result: a complete file that keeps the source comments,
  including the licence header of third-party built-ins. Written with
  `create_new`-style semantics so an existing file is never overwritten.
- `rename(dir, from, to)`: `fs::rename` of the file (refused if `to` exists).
- `delete(dir, name)`: `fs::remove_file`.
- Config side, in the caller (section 4): rename rewrites every one of `theme.name`,
  `theme.light`, `theme.dark` equal to `from` to `to`; delete sets them to
  `graphene-night`; duplicate sets the active key (`theme_key`) to the new name. The
  config write comes first, then the file operation; both land within the 100 ms
  debounce, so one reload sees the final state.

### 3. Editor model (`src/settings/model.rs`)

- New state in `SettingsModel`: `edited_theme: String` (defaults to the active theme
  when the window opens), `edited_palette: Palette` (`theme::load` of it, refreshed in
  `refresh`/`written` and on every reload), `edited_is_built_in: bool` (no user file).
- New `ThemeOp` enum next to `Change`: `SetColor(ColorKey, Option<Rgb>)`,
  `Duplicate(String)`, `Rename(String)`, `Delete`. `View::show` returns
  `(Vec<Change>, Vec<ThemeOp>)`; `Change` stays specific to `config.toml`.
- `SettingsModel::theme_op(op) -> Result<(Vec<Change>, ThemeOp), NameError>`: validates
  names and computes the config `Change`s of section 2.
- On a reload, if `edited_theme` no longer exists (deleted or renamed by hand), it falls
  back to the active theme.

### 4. Editor view (`src/settings/view.rs`, `src/settings/mod.rs`)

- Below the 4 existing fields of the Theme section, a "Theme editor" block:
  - a theme selector (same combo as `choice`, over `available()`), independent from
    the active theme;
  - actions: Duplicate (name field + confirm), Rename and Delete (user themes only;
    Delete opens a confirmation, as decided);
  - for a built-in theme: the swatches are shown disabled with a hint to duplicate it;
  - color rows grouped Terminal / ANSI normal / ANSI bright / Interface, one translated
    label and one `color_edit_button_srgb` each. `selection_foreground` gets a "Keep the
    text color" checkbox that writes `""`.
- Live preview, in memory then file:
  - while a picker popup is open and its value changes, the view stores the color in a
    `preview: Option<(ColorKey, Rgb)>` and the model exposes the edited palette with it
    applied;
  - when the popup closes (response no longer open, `Escape`, or window close), the
    view emits `ThemeOp::SetColor` once and clears `preview`.
- `SettingsWindow::window_event` gains an `Outcome::Preview(Palette)` (or a
  `take_preview()` read by `App` after each settings event). `App` forwards it to
  `Session::preview_palette(&Palette)`, which sets `self.palette` and requests a redraw,
  only when `edited_theme == theme.active(appearance)`. The file write then triggers the
  normal reload, which lands on the same colors.
- `SettingsWindow::write` handles `ThemeOp`s: config `Change`s through
  `write_changes`, then the `theme_edit` call; errors go to `model.write_failed` like
  config write errors.

### 5. Texts and docs

- `i18n`: one `Msg::ThemeColor(ColorKey)` variant (33 EN/FR labels in one `match`),
  plus the editor's labels (editor heading, Duplicate, Rename, Delete, delete
  confirmation, name errors, built-in hint, "Keep the text color"); update
  `VARIANT_COUNT` and `all_messages` ([src/i18n.rs:653-740](../../../src/i18n.rs)).
- `docs/en/themes.md` and `docs/fr/themes.md`: a "Theme editor" section; "Custom themes"
  keeps the file format as the reference.

## Consequences

- No change to the theme file format, to `available`, `load` or the watcher: hand
  edits keep working and the editor goes through the same reload path.
- `parse` is refactored onto `ColorKey`; the existing theme tests pin its behavior.
- `config_edit::write` is untouched; `write_existing` is additive.
- Rename/delete of the active theme changes `config.toml` and the theme file in two
  writes; a reload between them (debounce missed) shows at worst one transient
  "unknown theme" banner, cleared by the next reload.
- Picking a color in a non-active theme gives no terminal preview, only the swatch
  changes (consequence of the "selector" choice).

## Testing

- `theme`: `ColorKey::ALL` round trip (`get`/`set`/`path`) for every key; `parse` of
  each built-in unchanged; `format_color` / `parse_color` inverse; `valid_name` cases
  (empty, separators, leading dot, existing user, existing built-in).
- `theme_edit` (temp dirs): `set_color` changes only one key and keeps comments and
  absent keys; `set_color` on `selection_foreground` with `None` writes `""`;
  `duplicate` of a partial user file is complete and keeps its comments; `duplicate`
  of a third-party built-in keeps the licence header and parses to the same palette;
  `duplicate`/`rename` refuse an existing target; `write_existing` fails on a missing
  file.
- `settings::model`: `theme_op` config changes for rename (name, light, dark) and
  delete (back to `graphene-night`), duplicate sets the active key; edited theme falls
  back after removal.
- `settings::view`: preview emitted while the picker is open, single `SetColor` on
  close.
- `i18n`: `all_messages_covers_every_variant` with the new messages.

## Découpage en tâches d'implémentation

1. [#160](https://github.com/marmotz/graphene-term/issues/160) — `ColorKey` table and
   name validation (§1).
2. [#161](https://github.com/marmotz/graphene-term/issues/161) — theme file operations,
   `theme_edit` (§2). Depends on #160.
3. [#162](https://github.com/marmotz/graphene-term/issues/162) — editor model (§3).
   Depends on #160.
4. [#163](https://github.com/marmotz/graphene-term/issues/163) — editor view, writes and
   texts (§4, §5). Depends on #161, #162.
5. [#164](https://github.com/marmotz/graphene-term/issues/164) — live preview in the
   terminal (§4). Depends on #163.
6. [#165](https://github.com/marmotz/graphene-term/issues/165) — theme editor docs (§5).
   Depends on #163, #164.
