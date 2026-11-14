# Settings screen

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

The configuration file and themes feature made Graphene Term configurable through
`~/.config/graphene/config.toml`, with hot reload and user themes. Every setting must
still be changed by editing that TOML file by hand: there is no way to browse the
available options, preview a theme or change a setting from inside the terminal.

## Objectif

Give the user a settings screen inside Graphene Term to view and change the
configuration without editing the TOML file by hand.

## Décisions actées

### Opening

- The settings screen opens in a separate window, not in a terminal tab.
- It opens with the default shortcut `Ctrl+,` (an ordinary action, so it can be
  rebound or disabled like any other; `Ctrl+Shift+,` was dropped because Shift changes
  the character of that key) and from a "Settings" entry in the command
  palette.

### Settings covered

- Every setting of the configuration file is editable from the screen: font, window
  (padding, initial size, startup mode, transparency), scrollback, cursor, selection,
  tabs, language, themes, keyboard shortcuts and update checks.

### Applying changes

- A change made in the screen applies immediately: it is written to the configuration
  file right away, and the existing hot reload applies it to the open windows. There
  is no Apply button.

### Writing the configuration file

- The screen preserves the file: only the changed value is rewritten. The user's
  comments, key order and formatting are kept, so the file can still be edited by
  hand alongside the screen.

### Configuration errors

- The screen shows the same error banner as the terminal windows.
- An invalid value is flagged on the matching field; the rest of the settings stay
  editable, and fixing the value from the screen repairs the file. An unknown key has
  no field: it is listed at the top of the window with its line.
- A syntax error makes the file unreadable: the screen shows the error and the file
  path, and editing is disabled until the file is fixed by hand, so the screen never
  writes over a file it cannot parse.
