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
uncomment a line to change it. See [config.default.toml](config.default.toml).

Themes are set with `name` in the `[theme]` section. Eight are built in (see
[themes/](themes/)); a file `themes/<name>.toml` in the config directory is used first,
with the same format, and only needs the colors it changes.

Errors in these files are logged and shown in a banner at the top of the window (click
to hide it); the faulty settings keep their default value.

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

At the shell prompt (zsh, bash or fish with the integration, not inside a running
command or a full-screen application), the command line is edited like in an editor.
The shell keeps the command line: Graphene sends it key sequences its integration
script binds, and completion (`Tab`) and history (`Up` / `Down`) stay the shell's.

| Keys at the prompt            | Action                                          |
|-------------------------------|-------------------------------------------------|
| Left click in the input       | Move the cursor there                           |
| Ctrl+Left / Ctrl+Right        | Previous / next word                            |
| Ctrl+Backspace / Ctrl+Delete  | Erase the word before / after the cursor        |
| Home / End                    | Start / end of the line                         |
| Ctrl+Home / Ctrl+End          | Start / end of a multi-line input               |
| Shift+Left / Shift+Right      | Extend the selection by a character             |
| Shift+Up / Shift+Down         | Extend the selection by a line                  |
| Shift+Home / Shift+End        | Extend the selection to the line start / end    |
| Ctrl+A                        | Select the whole input                          |
| Shift+Enter                   | New line, without running the command           |

The input selection is an ordinary selection: `Ctrl+Shift+C` copies it and it fills the
primary selection. `Ctrl+Shift+` arrows are not editor keys.

## Documentation

- [Vision and roadmap](docs/vision.md)
- [Architecture](docs/architecture.md)
- [Technical decisions](docs/decisions.md)
- [Development guide](docs/development.md)
