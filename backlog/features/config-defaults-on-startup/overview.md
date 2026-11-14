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

- At startup, complete the existing file in place, adding missing options to their
  existing sections without duplicate options or section headers. Add a section header
  only when that section is absent.
- User-configured values remain active with their existing values; options not configured
  by the user remain commented with their defaults. The complete built-in shortcut list
  is included.
- Preserve the user's comments, key order, and formatting. Normalize only recognized
  comments from the previous template/generated help and malformed commented settings to
  the current concise wording; preserve custom comments.
- Give each setting one concise comment explaining its format and range, and listing
  allowed values when they form a finite set. Existing and automatically added supported
  settings include their matching help without repeating descriptions or changing values.
- Keys unknown to the current version (including obsolete keys) are preserved during
  automatic updates.
- The explicit `graphene --prune-config` command can remove unknown keys: it lists them
  and asks for confirmation before deleting them. `--yes` explicitly confirms without
  interaction.
- If no configuration file exists when `--prune-config` is run, the default file is
  created before pruning.
- If the file contains invalid TOML syntax, neither startup updates nor pruning may
  rewrite it.
