# Graphene Term

A modern, GPU-accelerated terminal emulator for Linux, written in Rust.
The binary is called `graphene`.

## Run

```bash
cargo run --release
```

## Configuration

Settings live in `~/.config/graphene/config.toml` (`$XDG_CONFIG_HOME/graphene/` when
set). The file is created on first launch, fully commented, with the default values:
uncomment a line to change it. On startup, Graphene adds comments for supported options
missing from an existing file inside their existing sections, creating a section only
when needed. It preserves configured settings, unknown keys, custom comments and formatting;
legacy template help and malformed comments for supported settings are normalized. The
comments explain each setting's format and range, and list finite allowed values
without repeating the same help. Automatically added settings include their matching
documentation. The file also lists the built-in keybindings. See
[config.default.toml](config.default.toml).

Run `graphene --prune-config` to list and remove keys not recognized by this version.
The command asks for confirmation in a terminal; when run non-interactively it makes no
changes unless `--yes` is also provided (`graphene --prune-config --yes`). Recognized
settings are preserved even if their values are invalid, and a file with invalid TOML is
never rewritten.

Themes are set with `name` in the `[theme]` section. Eight are built in (see
[themes/](themes/)); a file `themes/<name>.toml` in the config directory is used first,
with the same format, and only needs the colors it changes. Setting both `light` and
`dark` instead follows the system light/dark appearance (read from the XDG desktop
portal; dark without portal), switching the theme as soon as it changes.

`[window] startup_mode` opens new windows at the configured size (`"default"`),
maximized (`"maximized"`), or at the size and maximized state of the last window closed
(`"remember"`, kept in `~/.local/state/graphene/window.json`).
`[window] background_opacity` (0.0 to 1.0) makes the default background translucent,
with a compositor; explicit cell backgrounds, the selection and the cursor stay opaque.

Changes to `config.toml` and to the theme files apply to the open window as soon as
they are saved, without restart. A changed `font.size` resets the zoom; `width`,
`height` and `startup_mode` only affect windows opened afterwards.

Errors in these files are logged and shown in a banner at the top of the window (click
to hide it); the faulty settings keep their default value, or their previous value on a
reload.

Graphene checks GitHub for a newer release in the background (by default every 6 hours for dev
builds, every 24 hours for stable builds) and caches the result in
`~/.local/state/graphene/update.json` (`$XDG_STATE_HOME/graphene/` when set). The only
network call is a GET to the GitHub releases API; set `check = false` in the `[updates]`
section to turn it off. A newer release shows a banner at the top of the window: "Open
release" opens its GitHub page, "Ignore this version" hides it for good (until an even
newer release), and "×" hides it until the next launch.

The `RUST_LOG` environment variable sets the log level (default: `warn`).

## Shell integration

When `$SHELL` is zsh, bash or fish, Graphene loads a small integration script into the
shell it starts, so the shell marks where each prompt, command and output starts (OSC
133). Each command and its output then form a block: a colored bar in the left margin
shows its status (green: success, red: failure, yellow: running), and a block can be
selected, copied and folded (see [Shortcuts](#shortcuts)). An output taller than the
window is folded when the command ends. Your dotfiles are not modified, and they are
loaded as usual:

- zsh: started with `ZDOTDIR` set to the integration folder, whose `.zshenv` restores
  `ZDOTDIR` and sources yours;
- bash: started with `--posix` and `ENV` set to the integration script, which turns
  POSIX mode off and sources your startup files like a regular interactive bash;
- fish: the integration folder is added to `XDG_DATA_DIRS`; fish 4.0 and later already
  emit the marks, the script only adds the missing ones.

The scripts are written to `$XDG_RUNTIME_DIR/graphene/shell-integration/` (a private
folder of the system temporary directory when that variable is unset). Other shells,
and shells started by hand inside Graphene, are left as they are. Set
`GRAPHENE_SHELL_INTEGRATION=0` in Graphene's environment to start the shell without
integration.

## Performance

```bash
cargo bench                                          # rendering benchmarks
RUST_LOG=graphene::perf=debug cargo run --release    # startup timing logs
```

`cargo bench` measures parsing, frame building and GPU preparation on recorded terminal
output (see [docs/development.md](docs/development.md#benchmarks)). The `graphene::perf`
logs print the time of each startup step and of the first frame drawn after the shell's
first output.

## Shortcuts

| Keys                          | Action                                          |
|-------------------------------|-------------------------------------------------|
| Left drag                     | Select text (copied to the primary selection)   |
| Double / triple click         | Select a word / a whole line                    |
| Ctrl+left drag                | Block (rectangular) selection                   |
| Shift+left click              | Extend the selection                            |
| Shift+mouse                   | Select even when the app captures the mouse     |
| Ctrl+Shift+C                  | Copy the selection to the clipboard             |
| Left click on a status stripe | Select the command block of the row             |
| Ctrl+Shift+PageUp / PageDown  | Select the previous / next command block        |
| Ctrl+Shift+Alt+C              | Copy the command of the selected or last block  |
| Ctrl+Shift+Alt+O              | Copy the output of the selected or last block   |
| Click on a card's chevron     | Fold / unfold the output of the block           |
| Click on "n lines hidden"     | Unfold the block                                |
| Ctrl+Shift+F                  | Fold / unfold the selected or last block        |
| Ctrl+Shift+Alt+F              | Fold all finished blocks / unfold them all      |
| Middle click                  | Paste the primary selection                     |
| Ctrl+Shift+V, Shift+Insert    | Paste (bracketed when the app requests it)      |
| Shift+PageUp / Shift+PageDown | Scroll the selected block, or the history       |
| Shift+Home / Shift+End        | Top / bottom of the history (not at the prompt) |
| Ctrl+= / Ctrl+- / Ctrl+0      | Font size up / down / reset                     |
| Mouse wheel                   | Scroll the block under the pointer, or history  |

Tabs and split panes:

| Keys                          | Action                                          |
|-------------------------------|-------------------------------------------------|
| Ctrl+Shift+P                  | Command palette                                 |
| Ctrl+Shift+T                  | New tab                                         |
| Ctrl+Tab / Ctrl+Shift+Tab     | Next / previous tab                             |
| Ctrl+PageDown / Ctrl+PageUp   | Next / previous tab                             |
| Alt+1 ... Alt+9               | Go to tab 1 to 9                                |
| Ctrl+Shift+E / Ctrl+Shift+O   | Split the pane right / down                     |
| Ctrl+Shift+W                  | Close the pane                                  |
| Ctrl+Shift+Arrow              | Focus the pane in that direction                |
| Ctrl+Shift+Alt+Arrow          | Resize the pane toward that direction           |
| Ctrl+Shift+N / Ctrl+Shift+B   | Next / previous pane                            |
| Ctrl+Shift+Z                  | Zoom the pane to the whole tab, or back         |
| Click on a tab                | Show the tab; drag it to move it                |
| Middle click or × on a tab    | Close the tab                                   |
| Click on +                    | New tab                                         |
| Click in a pane               | Focus the pane                                  |
| Drag a divider                | Resize the panes on both sides                  |

"Equalise panes" and "Move pane to a new tab" have no key: run them from the command
palette, which also lists the open tabs and panes. Type to filter (fuzzy, case
sensitive only with an uppercase letter), `Up` / `Down` to choose, `Enter` to run,
`Escape` to close; recently run actions come first. The tab bar shows once there are
two tabs, on the edge set by `tabs.position` (`top`, `bottom`, `left` or `right`; on a
side, the tabs are stacked). Closing a pane, a tab or the window asks for confirmation while a program
other than the shell runs in it. With several panes in the tab, `Ctrl+Shift+Left /
Right` moves the focus instead of extending the input selection.

Every key-bound action can be remapped in the `[keybindings]` section of `config.toml`:
`"Ctrl+Alt+T" = "new_tab"` adds a shortcut, `"Shift+PageUp" = "none"` gives a default
one back to the shell. Keys are modifiers (`Ctrl`, `Shift`, `Alt`, `Super`) joined with
`+`, then a key name (`PageUp`, `F5`, `Left`...) or one character; actions are the
snake_case names of the palette entries (`copy`, `split_right`, `focus_pane_left`,
`go_to_tab_3`, `command_palette`...). The palette shows the configured shortcut.

At the shell prompt (zsh, bash or fish with the integration, not inside a running
command or a full-screen application), the command line is edited like in an editor.
The shell keeps the command line: Graphene sends it key sequences its integration
script binds, and completion (`Tab`) and history (`Up` / `Down`) stay the shell's.

| Keys at the prompt            | Action                                          |
|-------------------------------|-------------------------------------------------|
| Left click in the input       | Move the cursor there                           |
| Ctrl+Left / Ctrl+Right        | Start of the previous / next word               |
| Ctrl+Backspace / Ctrl+Delete  | Erase the word before / after the cursor        |
| Home / End                    | Start / end of the line                         |
| Ctrl+Home / Ctrl+End          | Start / end of a multi-line input               |
| Shift+Left / Shift+Right      | Extend the selection by a character             |
| Shift+Up / Shift+Down         | Extend the selection by a line                  |
| Shift+Home / Shift+End        | Extend the selection to the line start / end    |
| Ctrl+Shift+Left / Right       | Extend the selection to the word start / end    |
| Ctrl+A                        | Select the whole input                          |
| Shift+Enter                   | New line, without running the command           |

The input selection is an ordinary selection: `Ctrl+Shift+C` copies it and it fills the
primary selection. Typing, `Backspace`, `Delete`, `Ctrl+Backspace` / `Ctrl+Delete` or
`Shift+Enter` over a selection in the input replaces it, as in an editor.

## Documentation

- [Vision and roadmap](docs/vision.md)
- [Architecture](docs/architecture.md)
- [Technical decisions](docs/decisions.md)
- [Development guide](docs/development.md)
