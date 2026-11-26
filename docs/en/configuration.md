[Français](../fr/configuration.md)

# Configuration

Graphene works without any configuration. To change it, use the
[settings tab](#settings-tab) (`Ctrl+,`) or edit the configuration file by hand:
both write the same file, and every change applies to the open window at once.

- [File locations](#file-locations)
- [The configuration file](#the-configuration-file)
- [Live reload and errors](#live-reload-and-errors)
- [Settings tab](#settings-tab)
- [Removing unknown settings](#removing-unknown-settings)
- [Settings reference](#settings-reference)
- [Update check](#update-check)
- [Saved state](#saved-state)
- [Logs](#logs)

## File locations

| System | Configuration file                                         | Keyboard shortcuts file                                       |
|--------|------------------------------------------------------------|---------------------------------------------------------------|
| Linux  | `~/.config/graphene/config.toml`                           | `~/.config/graphene/keybindings.toml`                         |
| macOS  | `~/Library/Application Support/graphene/config.toml`       | `~/Library/Application Support/graphene/keybindings.toml`     |

When `XDG_CONFIG_HOME` is set, `$XDG_CONFIG_HOME/graphene/config.toml` and
`$XDG_CONFIG_HOME/graphene/keybindings.toml` are used instead, on both systems. Your
own [themes](themes.md#custom-themes) go in a `themes` folder next to `config.toml`.

## The configuration file

The file uses the [TOML](https://toml.io) format. Graphene creates it on the first
launch, with every setting commented out and set to its default value, each one with a
short explanation:

```toml
# [font]
## Size in points, from 6 to 72.
# size = 14.0
```

To change a setting, remove the `#` in front of it and of its section header, then
change the value:

```toml
[font]
size = 12.0
```

When a new version of Graphene adds settings, it adds them, commented out, to your
existing file at startup. Your values, your comments and the settings it does not know
are kept as they are.

## Live reload and errors

Saved changes to `config.toml`, `keybindings.toml` and your theme files apply to the open windows
without restarting, with a few exceptions:

- a pane whose font was zoomed with `Ctrl+=` / `Ctrl+-` keeps its gap from the new
  `font.size`;
- `window.width`, `window.height` and `window.startup_mode` only affect windows opened
  afterwards.

A mistake in the file does not stop Graphene. A banner at the top of the window
describes each problem (click it to hide it):

- a setting with an invalid value keeps its default value, or its previous value when
  the file is reloaded;
- a number out of range is brought back into range;
- an unknown setting is ignored;
- a TOML syntax error ignores the whole file until it is fixed.

## Settings tab

The settings are the first tab of the window, the sliders icon at the start of the tab
bar. Show it with a click on the icon, `Ctrl+,`, **Settings** in the
[command palette](tabs-and-panes.md#command-palette), or the right-click menu; leave it
with a click on another tab or with `Escape`. It is never closed. It shows the same sections as the file (language, font,
window, scrollback, cursor, selection, tabs, theme, keyboard shortcuts, notifications,
quick terminal, updates), and each change is written at once to `config.toml`, or to
`keybindings.toml` for keyboard shortcuts, keeping the files' comments, then applied like
a hand edit.

- **Keyboard shortcuts**: lists every action with its shortcuts. Add a shortcut by
  pressing it (`Escape` cancels), or remove one.
- Problems in the file are shown under the setting concerned; unknown settings are
  listed at the top.
- While a file has a syntax error, the tab shows it and locks what depends on that
  file (the keyboard shortcuts for `keybindings.toml`, the other sections for
  `config.toml`) until you fix the file by hand.

## Removing unknown settings

Settings that Graphene does not know (a typo, or a setting from an older version) are
ignored and kept in the file. This covers `config.toml` and `keybindings.toml`. To list
and remove them:

```bash
graphene --prune-config
```

It lists the unknown settings of both files and asks for confirmation. Outside an interactive
terminal it changes nothing unless you add `--yes`:

```bash
graphene --prune-config --yes
```

Known settings are always kept, even with an invalid value, and a file with a TOML
syntax error is never changed. A missing `keybindings.toml` is created from its
template.

## Settings reference

This is the complete list. Values shown are the defaults.

### General

| Setting    | Default  | Values                                                                 |
|------------|----------|------------------------------------------------------------------------|
| `language` | `"auto"` | `"auto"`, `"en"`, `"fr"`: language of the interface (menus, palette, settings, banners). `"auto"` follows the system language: French when it is French, English otherwise. |

`language` is at the top of the file, before any section.

### `[font]`

| Setting  | Default | Values                                                                                     |
|----------|---------|--------------------------------------------------------------------------------------------|
| `family` | `""`    | Font family name. Empty uses the system monospace font. On Linux, `fc-list : family` lists installed fonts. |
| `size`   | `14.0`  | Size in points, from 6 to 72.                                                              |
| `ligatures` | `true` | Shows the ligatures of the fonts that have some (Fira Code, JetBrains Mono...): `->`, `===` and the like are drawn as one symbol. |
| `features` | `""`   | OpenType features of the terminal text, separated by spaces or commas: `"ss01"` or `"+ss01"` enables a feature, `"-calt"` disables it, `"cv05=2"` sets a value. A tag is 4 characters; when a tag is repeated, the last one wins. Example: `"zero ss01"`. |

### `[window]`

| Setting              | Default     | Values                                                                                     |
|----------------------|-------------|--------------------------------------------------------------------------------------------|
| `startup_mode`       | `"default"` | `"default"`: open at `width` × `height`. `"maximized"`: open maximized. `"remember"`: open at the size and maximized state of the last window closed. |
| `width`              | `960`       | Width of a new window, in logical pixels (greater than zero).                              |
| `height`             | `600`       | Height of a new window, in logical pixels (greater than zero).                             |
| `padding`            | `8`         | Minimum space around the text, in logical pixels (zero or more).                           |
| `background_opacity` | `1.0`       | From `0.0` (transparent) to `1.0` (opaque). See [Themes](themes.md#translucent-background). |
| `progress`           | `true`      | Shows the progress reported by programs (OSC 9;4) in tabs, pane bars, blocks and the taskbar. `false` hides it everywhere. |
| `restore_session`    | `false`     | `true`: saves the open window while it runs (its tabs, their names, the layout of the panes, the directory of each shell and the font size of each pane) and reopens it at the next launch, at its size and, on the same screen, its place. Windows that closed together, within 15 seconds of the last one (logout, shutdown, crash), all come back, each in its own window; a window closed long before the others does not. A launch while another window is open opens a new window. Each pane starts a new shell, in its saved directory or, when it is gone, its nearest parent still there. Read at launch only. Windows started with `-e`, `--columns`, `--lines` or `--no-restore` are left out. The [quick terminal](quick-terminal.md#session-restore) is restored apart. |

### `[scrollback]`

| Setting | Default | Values                                          |
|---------|---------|-------------------------------------------------|
| `lines` | `10000` | Number of history lines to keep, up to 100000.  |

### `[cursor]`

| Setting    | Default   | Values                                                                                  |
|------------|-----------|-----------------------------------------------------------------------------------------|
| `shape`    | `"block"` | `"block"`, `"beam"` or `"underline"`.                                                   |
| `blinking` | `false`   | `true` or `false`. Programs can still change the cursor shape and blinking themselves. |

### `[selection]`

| Setting           | Default     | Values                                            |
|-------------------|-------------|---------------------------------------------------|
| `word_separators` | (see below) | Characters that end a word when you double-click. |

The default is:

```toml
word_separators = ",│`|\"' ()[]{}<>\t"
```

`:` is not in the list, so a URL or `host:port` is selected whole by a double-click.

### `[tabs]`

| Setting    | Default | Values                                                                     |
|------------|---------|----------------------------------------------------------------------------|
| `position` | `"top"` | Edge of the tab bar: `"top"`, `"bottom"`, `"left"` or `"right"`. See [Tabs and panes](tabs-and-panes.md#tabs). |

### `[theme]`

| Setting | Default            | Values                                                                        |
|---------|--------------------|-------------------------------------------------------------------------------|
| `name`  | `"graphene-night"` | Theme name: a built-in theme or one of your theme files.                      |
| `light` | not set            | Theme used when the system is in light mode. Needs `dark` too.                |
| `dark`  | not set            | Theme used when the system is in dark mode. Needs `light` too.                |

With both `light` and `dark` set, Graphene follows the system appearance and `name` is
not used. See [Themes](themes.md).

### Keyboard shortcuts

Shortcuts are not set in `config.toml`: they live in their own file, `keybindings.toml`
(a `[keybindings]` section, same format). See the
[Keyboard shortcuts](keybindings.md#changing-shortcuts) page for the syntax, the action
names and the default shortcuts. A `[keybindings]` section found in `config.toml` is
moved to `keybindings.toml` at startup.

### `[updates]`

| Setting    | Default  | Values                                                                                          |
|------------|----------|-------------------------------------------------------------------------------------------------|
| `check`    | `true`   | `true` or `false`. Check GitHub for a newer release; `false` makes no network call at all.      |
| `interval` | `"auto"` | `"auto"`, `"6h"`, `"12h"`, `"24h"`, `"2d"`, `"3d"` or `"1w"`. `"auto"` is 24 hours for a release, 6 hours for a development build. |

### `[notifications]`

| Setting             | Default | Values                                                                                     |
|---------------------|---------|--------------------------------------------------------------------------------------------|
| `program`           | `true`  | `true` or `false`. Show the notifications requested by programs (OSC 9, OSC 777).          |
| `command_finished`  | `true`  | `true` or `false`. Notify when a command that ran at least `command_threshold` seconds finishes. |
| `command_threshold` | `10`    | Whole number of seconds, at least 1. A value below 1 is rejected and the previous one kept. |
| `bell`              | `false` | `true` or `false`. Turn the terminal bell into a notification.                             |

A change applies to the next notification.

On Linux, notifications go to the desktop's notification server over D-Bus: each pane
shows at most one, clicking it brings back the pane, and the window's unseen count is
shown on the taskbar icon by docks that support it (KDE Plasma, the Ubuntu dock, Dash to
Dock). Without a notification server, only the counters on the tabs and pane bars are
shown. On Wayland, whether clicking a notification raises the window depends on the
compositor.

On macOS, notifications go to the Notification Center, which asks for permission at the
first one: each pane shows at most one, clicking it brings back the pane, and the
window's unseen count is shown on the Dock icon. Notifications need Graphene to run from
`Graphene.app`; run another way, only the Dock badge and the counters on the tabs and
pane bars are shown.

### `[quick_terminal]`

A terminal that slides from a screen edge and stays resident between two uses. Available
on macOS and on Linux under X11; under Wayland it does not start and its settings
section is disabled. See [Quick terminal](quick-terminal.md) for the command that shows
it.

| Setting              | Default    | Values                                                                                     |
|----------------------|------------|--------------------------------------------------------------------------------------------|
| `hotkey`             | `""`       | Native global shortcut showing or hiding the quick terminal, written like a [keyboard shortcut](#keyboard-shortcuts) (`"Ctrl+F12"`). Empty: none. |
| `edge`               | `"top"`    | Screen edge it slides from: `"top"`, `"bottom"`, `"left"` or `"right"`.                    |
| `size`               | `40`       | Percentage of the screen, from 10 to 100: the height for `top` and `bottom`, the width for `left` and `right`. The other dimension takes the whole screen. |
| `screen`             | `"active"` | `"active"` (the screen under the mouse cursor), `"primary"`, or the name of a screen. A name that matches no screen falls back to `"active"`. |
| `animation_ms`       | `150`      | Duration of the slide in milliseconds, `0` for none, at most 1000.                         |
| `hide_on_focus_loss` | `true`     | `true` or `false`. Hide the quick terminal when it loses the focus.                        |
| `autostart`          | `false`    | `true` or `false`. Start the quick terminal hidden when you log in. See [Start with the session](quick-terminal.md#start-with-the-session). |

`edge`, `size`, `screen` and `animation_ms` apply the next time the quick terminal is
shown.

## Update check

While `updates.check` is `true`, Graphene asks GitHub in the background whether a newer
release exists, at most once per `updates.interval`. It is the only network request
Graphene makes. When a newer release exists, a banner at the top of the window offers:

- **Open release**: opens the release page in your browser;
- **Ignore this version**: hides the banner for good, until an even newer release;
- **×**: hides it until the next launch.

## Saved state

Graphene keeps a few files between launches: the last window size (for
`startup_mode = "remember"`), the result of the last update check, and the actions
recently run from the command palette. With `window.restore_session = true`, the
`sessions/` subfolder holds one file per open window, rewritten while it runs and kept
after it closes for the next launch.

| System | State folder                                |
|--------|---------------------------------------------|
| Linux  | `~/.local/state/graphene/`                  |
| macOS  | `~/Library/Application Support/graphene/`   |

When `XDG_STATE_HOME` is set, `$XDG_STATE_HOME/graphene/` is used instead. Deleting the
folder is harmless.

## Logs

Graphene writes warnings and errors to its standard error. The `RUST_LOG` environment
variable sets the level (default `warn`), for example:

```bash
RUST_LOG=info graphene
```

Start Graphene from another terminal to see them.
