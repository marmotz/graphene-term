# Update Configuration at Startup

**Statut** : terminé — voir [technical.md](technical.md)

## Context

When a new release adds a configuration option, existing configuration files do not
contain it. Users may not quickly discover the option in their file, even though the
application supports it—for example, `background_opacity`, added to control background
transparency.

## Goal

Update the existing configuration file at startup so that newly supported options and
their default values are visible, without losing or replacing settings the user has
configured manually.

## Decisions

- At startup, missing options are added to the existing file with their default values.
- All supported options that are not already present in the file are added as commented
  lines, following the configuration template. This includes the complete list of
  built-in keyboard shortcuts.
- Existing user-configured values are preserved.
- Keys unknown to the current version (including obsolete keys) are preserved during
  automatic updates.
- The explicit `graphene --prune-config` command can remove unknown keys: it lists them
  and asks for confirmation before deleting them. `--yes` explicitly confirms without
  interaction.
- If no configuration file exists when `--prune-config` is run, the default file is
  created before pruning.
- If the file contains invalid TOML syntax, neither startup updates nor pruning may
  rewrite it.
- Updates should preserve the user's file presentation as much as possible, including
  comments, key order, and formatting.
