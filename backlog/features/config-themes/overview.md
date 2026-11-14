# Configuration file and themes

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

Graphene Term has no configuration file yet: the font, colors and behaviours are
hard-coded, and only the font family and size can be changed, through the
`GRAPHENE_FONT` and `GRAPHENE_FONT_SIZE` environment variables. Several features
already postpone their customisation to this one:
[selection-copy](../selection-copy/overview.md) keeps a fixed list of word separators
for double-click and highlights the selection by swapping foreground and background,
leaving a dedicated selection color to the themes feature. The roadmap
(`docs/vision.md`) lists **Configuration file and themes** as a planned item, after the
four "modern" pillars.

## Objectif

Let the user customise Graphene Term through a configuration file and pick or write
color themes, while keeping sensible defaults so the terminal works with no
configuration at all.

## Décisions actées

### Configuration file

- Format: TOML, at `~/.config/graphene/config.toml` (XDG config directory).
- On first launch, if the file does not exist, Graphene Term writes it fully
  commented: every option is listed with its default value.
- The file is the only source of settings: the `GRAPHENE_FONT` and
  `GRAPHENE_FONT_SIZE` environment variables are removed.
- Hot reload: Graphene Term watches the file and applies changes without a restart.
- Errors (syntax, invalid value, unknown key): the terminal still starts, uses the
  default for whatever is invalid, and shows a banner at the top of the window. On a
  hot reload, the last valid configuration stays in effect.

### Settings covered

- **Font**: family and size.
- **Window**: inner padding (a minimum), initial size in pixels. The pixels left over
  once whole columns and lines are placed are split evenly on both sides, top/bottom
  and left/right, so the grid is centred and every cell is fully visible. This applies
  to the single terminal as well as to each tab and pane.
- **Scrollback**: number of history lines.
- **Cursor**: shape and blinking.
- **Selection**: word separators used by double-click (replaces the fixed list decided
  in [selection-copy](../selection-copy/overview.md)).
- **Tabs**: position of the tab bar (top, bottom, left, right).
- **Language**: UI language, overriding the system locale used by default in
  [tabs-splits-palette](../tabs-splits-palette/overview.md).
- **Keyboard shortcuts**: any action can be bound to a key, and any default shortcut
  can be disabled so the key goes to the shell.

### Window startup and transparency

Added after the first tasks were created (#13 to #21); designed in the "Window startup
and transparency" section of [technical.md](technical.md).

- **Startup mode**: a window setting with three values:
  - `default`: the window opens at the initial size in pixels from the configuration;
  - `maximized`: the window opens maximized;
  - `remember`: the window reopens with the size and the maximized state of the last
    window closed. The position is not remembered (not possible on Wayland). With no
    previous window on record, it falls back to `default`.
- **Transparency**: a background opacity setting, from 0.0 (fully transparent) to 1.0
  (opaque, the default). It applies to the terminal background only: text stays
  opaque, and the UI drawn by Graphene Term (tab bar, command palette, banners) stays
  opaque to remain readable. No background blur.

### Themes

- A theme sets the terminal palette (16 ANSI colors, foreground, background, cursor),
  the selection color (replaces the current foreground/background swap decided in
  [selection-copy](../selection-copy/overview.md)) and the colors of the UI drawn by
  Graphene Term (tab bar, command palette).
- Built-in themes: a house pair, **Graphene night** (the current default) and
  **Graphene day**, plus a few popular themes (Solarized, Gruvbox, Catppuccin...),
  subject to their licences.
- User themes: TOML files in `~/.config/graphene/themes/`, selected by name like the
  built-in ones.
- Light/dark: the configuration names either one theme, or a light theme and a dark
  theme; in the second case Graphene Term follows the system appearance.

## Ordre des features

- Comes after [tabs-splits-palette](../tabs-splits-palette/overview.md), as in the
  roadmap: the tab position and the UI colors only exist once tabs and the command
  palette do.
- Completes [selection-copy](../selection-copy/overview.md) (word separators,
  selection color).
- Required by [update-notification](../update-notification/overview.md): its
  `[updates]` settings and its banner build on this feature.
