# Keybindings file

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

Keyboard shortcuts currently live in the `[keybindings]` section of `config.toml`, next
to font, window, scrollback, theme and update settings (see `config.default.toml`,
`docs/en/keybindings.md`). Each line maps one shortcut to one action and comes on top of
the defaults; the settings window (`Ctrl+,`) writes the same section through
`src/config_edit.rs`. The whole config directory is already watched for live reload
(`watcher::spawn` in `src/app.rs`).

As users customise more shortcuts, this section grows and mixes with the rest of the
configuration, which makes `config.toml` harder to read and to maintain.

## Objectif

Move keyboard shortcuts into a dedicated configuration file, so that both
`config.toml` and the shortcut list are easier to read and edit.

## Décisions actées

- **File**: shortcuts live in `keybindings.toml`, next to `config.toml` in the config
  directory (`~/.config/graphene/` on Linux). It keeps the current `[keybindings]`
  header and line format, so a section copied from `config.toml` works as is. Same
  semantics as today: lines come on top of the defaults, `"none"` removes a shortcut.
- **Creation**: the file is created on first launch, like `config.toml`, from a
  commented template (shortcut syntax, list of actions, examples).
- **Migration**: on startup, a `[keybindings]` section found in `config.toml` is moved
  into `keybindings.toml`, then removed from `config.toml`; the error/info banner says
  so. If both define shortcuts, they are merged and `keybindings.toml` wins for the same
  shortcut. The commented keybindings block that Graphene wrote into `config.toml` is
  removed too, silently. A `[keybindings]` section added to `config.toml` while Graphene
  runs is ignored and reported, then migrated at the next start.
- **Hot reload**: `keybindings.toml` is reloaded when saved, like `config.toml`.
- **`prune-config`**: also removes the unknown keys of `keybindings.toml`.
- **Settings window**: shortcuts edited from the settings window (`Ctrl+,`) are always
  saved in `keybindings.toml`. A syntax error in one file only locks the settings that
  file holds.
- **Docs**: the keyboard shortcuts and configuration pages (EN and FR) point to the new
  file.
