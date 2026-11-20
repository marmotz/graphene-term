[Français](../fr/themes.md)

# Themes

A theme sets the colors of the terminal (text, background, cursor, selection, the 16
ANSI colors) and of Graphene's interface (tab bar, pane borders, command palette,
banners).

- [Choosing a theme](#choosing-a-theme)
- [Built-in themes](#built-in-themes)
- [Following the system light/dark mode](#following-the-system-lightdark-mode)
- [Custom themes](#custom-themes)
- [Theme file format](#theme-file-format)
- [Translucent background](#translucent-background)

## Choosing a theme

Any of these switches the theme at once:

- right click in the window, then **Themes**;
- the **Theme** section of the settings window (`Ctrl+,`);
- `name` in the `[theme]` section of the [configuration file](configuration.md):

  ```toml
  [theme]
  name = "gruvbox-dark"
  ```

The menu and the settings window write the same setting to the configuration file.

## Built-in themes

| Name               | Kind  |
|--------------------|-------|
| `graphene-night`   | dark (the default) |
| `graphene-day`     | light |
| `solarized-dark`   | dark  |
| `solarized-light`  | light |
| `gruvbox-dark`     | dark  |
| `gruvbox-light`    | light |
| `catppuccin-mocha` | dark  |
| `catppuccin-latte` | light |

Their files are in the [themes/](../../themes/) folder of the repository, a good
starting point for your own.

## Following the system light/dark mode

Set both `light` and `dark` in the `[theme]` section:

```toml
[theme]
light = "graphene-day"
dark = "graphene-night"
```

Graphene then uses the `light` theme while your system is in light mode and the `dark`
theme otherwise, and switches as soon as the system mode changes. `name` is not used
while both are set; with only one of them set, `name` is used.

Graphene reads the system mode from the desktop settings portal
(`xdg-desktop-portal`), which most Linux desktops provide. Where it is not available,
the `dark` theme is used.

## Custom themes

A theme is a `.toml` file in the `themes` folder next to your configuration file:

| System | Themes folder                                         |
|--------|-------------------------------------------------------|
| Linux  | `~/.config/graphene/themes/`                          |
| macOS  | `~/Library/Application Support/graphene/themes/`      |

The file name, without `.toml`, is the theme name: `themes/my-theme.toml` is used with
`name = "my-theme"`, and it shows in the theme menu and the settings window.

- A theme file with the name of a built-in theme replaces it.
- A theme file only needs the colors it changes: every missing color comes from
  `graphene-night`.
- Saving the theme file in use applies it at once.
- An invalid color or an unknown key is reported in the error banner and ignored; a
  name that matches no theme falls back to `graphene-night`.

## Theme file format

Colors are written `"#rrggbb"`. A complete theme has these keys:

```toml
[colors]
foreground = "#d7dae0"
background = "#14161b"
cursor = "#82aaff"
cursor_text = "#14161b"          # text under a block cursor
selection_background = "#33508a"
selection_foreground = ""        # empty: selected text keeps its own color

[colors.normal]                  # ANSI colors 0 to 7
black = "#1c1f26"
red = "#f0717b"
green = "#9ed072"
yellow = "#e7c664"
blue = "#6cb6ff"
magenta = "#c792ea"
cyan = "#5fd7d7"
white = "#c8ccd4"

[colors.bright]                  # ANSI colors 8 to 15
black = "#5c6370"
red = "#ff8b92"
green = "#b5e890"
yellow = "#ffd982"
blue = "#8ecaff"
magenta = "#dcb0ff"
cyan = "#82eaea"
white = "#f2f4f8"

[ui]
tab_bar_background = "#0f1115"
tab_active_background = "#1c1f26"
tab_active_text = "#d7dae0"
tab_inactive_text = "#7f8590"
pane_border = "#2a2e37"
pane_broadcast = "#5fd7d7"       # title bars of the broadcast panes
palette_background = "#1c1f26"
palette_text = "#d7dae0"
palette_match = "#82aaff"        # matched letters in the command palette
palette_selection_background = "#2e3440"
banner_background = "#5c2a30"
banner_text = "#f2f4f8"
```

These are the values of `graphene-night`. A theme that only changes the background:

```toml
[colors]
background = "#000000"
```

## Translucent background

`background_opacity` in the `[window]` section makes the background see-through, from
`0.0` (transparent) to `1.0` (opaque, the default):

```toml
[window]
background_opacity = 0.9
```

Only the default background becomes translucent: colored backgrounds drawn by
programs, the selection and the cursor stay opaque. On Linux, it needs a compositor
(built into most desktops).
