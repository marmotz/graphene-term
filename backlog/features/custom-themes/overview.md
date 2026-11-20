# Custom themes

**Statut** : conception technique, voir [technical.md](technical.md)

## Contexte

Users want to add their own themes. Graphene already supports user theme files: a
`.toml` file in `themes/` next to the config file (`~/.config/graphene/themes/` on
Linux) is picked up by name, listed in the theme menu and the settings window, can
override a built-in theme, only needs the colors it changes (the rest comes from
`graphene-night`), and is reloaded live when added, saved or removed (see
`docs/en/themes.md`, `src/theme.rs`, `src/watcher.rs`).

Adding a theme today therefore means writing a TOML file by hand in the right folder,
with no help from the app.

## Objectif

Let users create, edit, rename and delete their own themes from inside Graphene, without
writing TOML by hand, while the hand-written theme files keep working as they do today.

## Décisions actées

- **Theme files**: no change. `themes/*.toml` stays the single storage of user themes; the
  editor reads and writes these files, and files added or edited by hand keep being
  picked up live as today.
- **Location**: the editor lives in the **Thème** section of the settings window
  (`Ctrl+,`), below the theme choice. No separate window.
- **Edited theme**: the editor has its own theme selector, independent from the active
  theme (it opens on the active one). The terminal previews the changes only when the
  edited theme is the active one.
- **Live edit**: every color change is written to the theme file immediately and applied
  to the terminal at once, like the rest of the settings window. There is no "unsaved"
  state, no save or cancel button.
- **Scope**: every color of a theme is editable: terminal colors (foreground,
  background, cursor, cursor text, selection, the 16 ANSI colors) and UI colors (tab
  bar, pane borders, command palette, banners).
- **Creating a theme**: by duplicating an existing theme (built-in or user) under a new
  name. There is no blank theme. The copy becomes the active theme at once.
- **Names**: a name already taken by a user or built-in theme is refused when
  duplicating or renaming.
- **Built-in themes**: never edited in place. Editing one requires duplicating it under
  a new name first; the built-in stays intact.
- **File content**:
  - a theme created by the editor (duplicate) is written with every color, so the file
    is complete and standalone;
  - editing an existing user file only changes the edited keys, keeping its comments,
    layout and the keys it leaves out.
- **Managing themes**: user themes can be renamed and deleted from the editor. Built-in
  themes cannot.
  - Renaming a theme referenced by `name`, `light` or `dark` in the config updates
    those keys to the new name.
  - Deleting a theme always asks for confirmation; the config keys that referenced it
    (`name`, `light`, `dark`) go back to `graphene-night`.
