# Keybindings file — Technical Design

Product decisions: [overview.md](overview.md).

## Current State

### Loading

- [src/config.rs:22](../../../src/config.rs) embeds `config.default.toml` as
  `DEFAULT_CONFIG`; [src/config.rs:25](../../../src/config.rs) names the file
  `CONFIG_FILE = "config.toml"`.
- `load_from` ([src/config.rs:137](../../../src/config.rs)) runs only at startup: it
  completes the file with `config_edit::complete_defaults` and writes it back, or
  creates it from `config_edit::default_file_text()` when missing.
- `reload_from` ([src/config.rs:120](../../../src/config.rs)) runs on every change seen
  by the watcher and never writes a file.
- `Config::parse_with_unknown_keys` ([src/config.rs:516](../../../src/config.rs)) accepts
  `keybindings` as one of the sections ([src/config.rs:552](../../../src/config.rs));
  each key is validated by `keybindings::parse_binding` in `read_section_key`
  ([src/config.rs:768](../../../src/config.rs)) and lands in
  `Config::keybindings: BTreeMap<String, String>`
  ([src/config.rs:455](../../../src/config.rs)). An invalid binding keeps the previous
  action; a TOML syntax error keeps the whole previous `Config`.
- Errors carry `file`, `line` and a `key` path such as `keybindings."Ctrl+X"`
  ([src/config.rs:174](../../../src/config.rs)); the settings window routes them by
  `key`.

### Default file and completion

- [config.default.toml:54-68](../../../config.default.toml) holds the commented
  `# [keybindings]` block: `##` documentation (syntax, allowed actions, example).
- `complete_defaults` ([src/config_edit.rs:25](../../../src/config_edit.rs)) adds the
  keybindings documentation to that block, and `option_lines`
  ([src/config_edit.rs:923](../../../src/config_edit.rs)) adds one commented line per
  default shortcut (`# "Ctrl+Shift+C" = "copy"`) from `keybindings::defaults()`.
  `keybinding_documentation` ([src/config_edit.rs:335](../../../src/config_edit.rs))
  reads the `##` lines of the `keybindings` block of `DEFAULT_CONFIG`.
- So every `config.toml` written by Graphene holds a commented keybindings block, even
  for users who never changed a shortcut.

### Writes

- `config_edit::write(file, change)` ([src/config_edit.rs:1165](../../../src/config_edit.rs))
  re-reads the file, applies a `ConfigEdit`, writes atomically; a missing file starts
  from `default_file_text()`.
- `ConfigEdit::remove` ([src/config_edit.rs:1129](../../../src/config_edit.rs)) drops an
  emptied `[keybindings]` table unless it carries a comment.
- The settings window gets one `file` ([src/app.rs:2619](../../../src/app.rs)) and
  writes every `Change` to it in one write (`write_changes`,
  [src/settings/mod.rs:396](../../../src/settings/mod.rs)), then re-parses that file
  ([src/settings/mod.rs:239](../../../src/settings/mod.rs)).
- `SettingsModel::locked` ([src/settings/model.rs:347](../../../src/settings/model.rs))
  locks the whole window on any `Syntax` or `Read` error.
- `prune_file` / `prune_config` ([src/config_edit.rs:696](../../../src/config_edit.rs),
  [src/config_edit.rs:872](../../../src/config_edit.rs)) remove the unknown keys of
  `config.toml` only.

### Watcher and banner

- `watched_file` ([src/watcher.rs:105](../../../src/watcher.rs)) reacts to
  `config.toml` and `themes/*.toml` only.
- `Banner::config_errors` ([src/banner.rs:28](../../../src/banner.rs)) shows the first
  `ConfigError` plus a count; there is no informational banner.

## Design

### New file and template

- Add `KEYBINDINGS_FILE = "keybindings.toml"` and
  `DEFAULT_KEYBINDINGS = include_str!("../keybindings.default.toml")` in
  `src/config.rs`, next to `CONFIG_FILE`.
- Create `keybindings.default.toml` at the repo root: the `##` documentation moved out of
  `config.default.toml:54-68`, then an active `[keybindings]` header (kept, per the
  overview), then one commented line per default shortcut, generated at runtime from
  `keybindings::defaults()` as today.
- Remove the `# [keybindings]` block from `config.default.toml`, and the keybindings
  branches from `complete_defaults`, `option_lines`, `active_documentation_insertions`
  and `normalize_legacy_documentation` in `src/config_edit.rs`. Move
  `keybinding_documentation` / `missing_keybinding_documentation` to a
  `complete_keybindings_defaults(text)` that does the same completion on
  `keybindings.toml`, reading `DEFAULT_KEYBINDINGS`.

### Parsing

- `Config` keeps its `keybindings` field: `Keybindings::new(&config.keybindings)` and
  every consumer (settings model, session `apply_config`) stay unchanged.
- Add `Config::parse_keybindings(text, previous, file) -> (BTreeMap<String, String>,
  Vec<ConfigError>, Vec<UnknownConfigKey>)` reusing `Reader` and the existing
  `("keybindings", keys)` branch of `read_section_key`. In `keybindings.toml`, the only
  known root key is the `keybindings` table; anything else is an unknown key. A syntax
  error keeps `previous.keybindings` (not the whole `Config`).
- Error key paths stay `keybindings."Ctrl+X"` because the header is kept: the settings
  routing by key needs no change. Errors carry `file = keybindings.toml`.
- In `config.toml`, the `keybindings` section is no longer read. It is reported with a
  new `ConfigErrorKind::Moved` ("`[keybindings]` moved to keybindings.toml"), and not
  pushed to `unknown_keys`, so `prune-config` never deletes a user's shortcuts.
- `load()` / `reload()` read both files and merge: `Config` from `config.toml`,
  `keybindings` from `keybindings.toml`, errors concatenated (config first).

### Startup: creation and migration

In `load_from`, before parsing:

1. Read `config.toml` with `toml_edit`. Collect the entries of an active `[keybindings]`
   table (values and their comments) and the ranges of the generated commented block
   (`# [keybindings]` header, `##` lines matching `keybinding_documentation`, commented
   lines matching a default shortcut). Commented lines not recognized as generated stay.
2. If `keybindings.toml` is missing, start from the template; otherwise from its text.
   Insert each collected entry whose key is not already active in `keybindings.toml`
   (`keybindings.toml` wins), with `ConfigEdit::set`, keeping the entry's comments.
3. Write `keybindings.toml` first, then `config.toml` without the section and the
   generated block. Writing in this order means a crash between the two leaves the
   shortcuts in both files, never in neither; the next start finds nothing new to merge
   and only cleans `config.toml`.
4. If an active section was migrated, return a `ConfigError` of kind `Moved` with the
   message "keyboard shortcuts moved to keybindings.toml": it reaches the existing
   banner. Removing only the commented block is silent.
5. If a write fails (read-only directory), keep the merged shortcuts in memory, leave
   the files untouched and report a `Write` error.

Then complete `keybindings.toml` with `complete_keybindings_defaults`, like
`config.toml` today.

`reload_from` never writes: a `[keybindings]` section added by hand to `config.toml`
while Graphene runs is ignored and reported (`Moved`), and migrated at the next start.

### Watcher

`watched_file` also returns `true` for `dir.join(KEYBINDINGS_FILE)`; the existing
`ConfigChanged` event and `reload_config` ([src/app.rs:2556](../../../src/app.rs)) then
cover it. This gives hot reload of `keybindings.toml` for free.

### Settings window

- `SettingsWindow::open` takes the config directory instead of one file, and derives
  both paths.
- `write_changes` splits the changes by first path segment: `["keybindings", ..]` go to
  `keybindings.toml`, the rest to `config.toml`, one `config_edit::write` per touched
  file. `write` then re-parses both files.
- `config_edit::write` takes the default text as a parameter (or a per-file variant), so
  a missing `keybindings.toml` starts from its template.
- `ConfigEdit::remove` keeps the `[keybindings]` table in `keybindings.toml`: the
  template's header carries comments, so the existing rule already keeps it.
- `SettingsModel::locked` becomes per section: a `Syntax`/`Read` error of
  `keybindings.toml` locks `Section::KeyboardShortcuts` only; one of `config.toml`
  locks every other section. Errors are told apart by `ConfigError::file`.

### prune-config

`prune_config` runs `prune_file` on both files, prints the unknown keys prefixed with
their file name, asks one confirmation, then prunes each file.

### Docs

Update `docs/en/keybindings.md` ("Changing shortcuts") and `docs/en/configuration.md`
(`[keybindings]` section, file locations table, hot reload sentence), and their
`docs/fr/` counterparts: shortcuts live in `keybindings.toml`, migration is automatic.

## Alternatives Considered

- **Separate `KeybindingsConfig` struct instead of the `Config.keybindings` field.**
  Rejected: every consumer reads `config.keybindings`; keeping the field limits the
  change to loading and writing.
- **Shortcuts at the root of `keybindings.toml`.** Rejected by the product decision: the
  `[keybindings]` header is kept so a section can be copied as is. It also keeps the
  error key paths and `ConfigEdit` paths unchanged.
- **Migrating on hot reload too.** Rejected: reload never writes files (an editor may
  be replacing them); the section is reported and migrated at the next start.
- **Reporting the old section as an unknown key.** Rejected: `prune-config` would then
  delete the user's shortcuts. A dedicated `Moved` kind avoids it.

## Consequences

- Existing `config.toml` files lose their keybindings block at the next start; the
  completion no longer re-adds it. Tests asserting keybindings in the default
  `config.toml` (`missing_file_is_created_from_the_default`,
  `completion_adds_all_documented_options_and_default_keybindings`,
  `commented_binding_is_not_duplicated_or_replaced`, `every_key_is_read`) move to
  `keybindings.toml` equivalents.
- Tests parsing `[keybindings]` through `Config::parse`
  (`keybinding_error_carries_a_quoted_key_path`, `invalid_keybinding_keeps_the_previous_action`,
  `unknown_keybinding_keys_or_actions_are_reported`) move to `parse_keybindings`; their
  messages now name `keybindings.toml`.
- `SettingsWindow::open` and `write_changes` change signature: the tests in
  [src/settings/mod.rs](../../../src/settings/mod.rs) follow.
- `theme.rs:130` uses `CONFIG_FILE` for theme errors and is unaffected.

## Découpage en tâches d'implémentation

1. [#152](https://github.com/marmotz/graphene-term/issues/152) — `keybindings.toml`:
   template, parsing, creation, reload, watcher.
2. [#153](https://github.com/marmotz/graphene-term/issues/153) — Startup migration of
   the `[keybindings]` section of `config.toml` (depends on #152).
3. [#154](https://github.com/marmotz/graphene-term/issues/154) — Settings window: writes
   to `keybindings.toml`, per-file locking (depends on #152).
4. [#155](https://github.com/marmotz/graphene-term/issues/155) — `prune-config` on both
   files (depends on #152).
5. [#156](https://github.com/marmotz/graphene-term/issues/156) — EN/FR docs (depends on
   #153, #154, #155).
