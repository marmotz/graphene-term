# Graphene Term

A modern, GPU-accelerated terminal emulator for Linux and macOS, written in Rust, with
command blocks, a mouse-friendly prompt, tabs and split panes.

[Français](README.fr.md)

<!-- Screenshot: put the image in docs/images/ (see docs/images/README.md), then
uncomment the line below.
![Graphene Term](docs/images/screenshot.png)
-->

- [Features](#features)
- [Comparison](#comparison)
- [Install](#install)
- [Configuration](#configuration)
- [Build from source](#build-from-source)
- [Contributing](#contributing)
- [Documentation](#documentation)
- [License](#license)

## Features

- **GPU rendering** with truecolor, and a translucent background:
  [themes](docs/en/themes.md#translucent-background).
- **Command blocks**: each command and its output form a block with its exit status;
  select, copy, fold and unfold it: [command blocks](docs/en/command-blocks.md).
- **Automatic shell integration** for zsh, bash and fish, without editing your
  dotfiles: [shell integration](docs/en/command-blocks.md#shell-integration).
- **Rich prompt editor**: click to move the cursor, select with the mouse or
  `Shift+Arrow`, editor shortcuts: [input editor](docs/en/input-editor.md).
- **Tabs and split panes**, drag panes to rearrange them, zoom a pane:
  [tabs and panes](docs/en/tabs-and-panes.md).
- **Command palette** with fuzzy search over every action:
  [command palette](docs/en/tabs-and-panes.md#command-palette).
- **Inline images** (kitty graphics protocol with animations, sixel) and **clickable
  links** (OSC 8 and detected URLs): [images and links](docs/en/images-and-links.md).
- **Mouse selection**, including rectangular selection, and mouse reporting to
  applications: [selection and clipboard](docs/en/selection-and-clipboard.md).
- **Remappable keybindings**: [keyboard shortcuts](docs/en/keybindings.md).
- **Settings window**, live config reload, remembered window size, update
  notification: [configuration](docs/en/configuration.md).
- **Eight built-in themes**, your own theme files, and themes that follow the system
  light/dark mode: [themes](docs/en/themes.md).
- **Interface in English and French**, following the system language:
  [language setting](docs/en/configuration.md#general).

## Comparison

Graphene compared with other Linux terminals, each with its default configuration.

### Performance

<!-- Paste the table printed by `scripts/benchmark-terminals.py run` here. -->

| Terminal | Throughput (s) | Refresh rate (fps) | Startup (ms) | Memory idle (MiB) | Memory under load (MiB) |
|---|---|---|---|---|---|
| **Graphene** |  |  |  |  |  |
| Alacritty |  |  |  |  |  |
| Kitty |  |  |  |  |  |
| WezTerm |  |  |  |  |  |
| foot |  |  |  |  |  |
| Terminator |  |  |  |  |  |
| GNOME Terminal |  |  |  |  |  |
| Konsole |  |  |  |  |  |

Lower is better, except refresh rate. Each value is the median of several runs.
Throughput (time to `cat` an 11 MB text file) and refresh rate measure how fast a
terminal accepts output, not what it draws on screen.

### Features

✅ supported, ❌ not supported, ⚠️ partial (see the note).

<!-- Write the tested version of each terminal in the header, then fill the cells. -->

| Feature | Graphene | Alacritty | Kitty | WezTerm | foot | Terminator | GNOME Terminal | Konsole |
|---|---|---|---|---|---|---|---|---|
| **Rendering** | | | | | | | | |
| GPU rendering | | | | | | | | |
| Truecolor | | | | | | | | |
| Inline images (kitty graphics protocol, with animations) | | | | | | | | |
| Sixel images | | | | | | | | |
| Translucent background | | | | | | | | |
| **Shell** | | | | | | | | |
| Command blocks with exit status | | | | | | | | |
| Fold / unfold outputs | | | | | | | | |
| Copy a command or its output | | | | | | | | |
| Automatic shell integration (zsh, bash, fish) without editing dotfiles | | | | | | | | |
| Rich prompt editor (mouse cursor, selection, editor shortcuts) | | | | | | | | |
| **Layout** | | | | | | | | |
| Tabs | | | | | | | | |
| Split panes | | | | | | | | |
| Drag panes to rearrange | | | | | | | | |
| Zoom a pane | | | | | | | | |
| Command palette (fuzzy) | | | | | | | | |
| **Interaction** | | | | | | | | |
| Clickable links (OSC 8 and detected URLs) | | | | | | | | |
| Rectangular selection | | | | | | | | |
| Mouse reporting to applications | | | | | | | | |
| Remappable keybindings | | | | | | | | |
| **Configuration** | | | | | | | | |
| Settings window (GUI) | | | | | | | | |
| Live config reload | | | | | | | | |
| Themes following the system light/dark mode | | | | | | | | |
| Built-in themes | | | | | | | | |
| Interface translated (en, fr) | | | | | | | | |
| Remembered window size | | | | | | | | |
| Update notification | | | | | | | | |

### Test machine and how to reproduce

<!-- Paste the "Machine" block printed by the script here. -->

The tables come from [scripts/benchmark-terminals.py](scripts/benchmark-terminals.py)
(Python 3, Linux). It runs every installed terminal and prints the performance table
and the machine:

```bash
cargo build --release
scripts/benchmark-terminals.py run
```

See [Comparing terminals](docs/dev/development.md#comparing-terminals) for what each
metric measures.

## Install

Download a package from the
[releases page](https://github.com/marmotz/graphene-term/releases):

- **Debian, Ubuntu**: `.deb`
- **Fedora, openSUSE, RHEL**: `.rpm`
- **Any Linux**: `.AppImage` or `.tar.gz`
- **macOS** (Apple Silicon or Intel): `.dmg`, or `.tar.gz` for the command line

Install commands, the macOS first launch and uninstalling are in the
[installation guide](docs/en/installation.md).

## Configuration

Settings live in `~/.config/graphene/config.toml` on Linux, or
`~/Library/Application Support/graphene/config.toml` on macOS. The file is created on
first launch with every setting commented out, and changes apply as soon as it is
saved. The settings window (`Ctrl+,`) edits the same file.

Every setting is described in the [configuration guide](docs/en/configuration.md).

## Build from source

You need Rust (stable, through [rustup](https://rustup.rs)), Git and a C compiler.

```bash
git clone https://github.com/marmotz/graphene-term.git
cd graphene-term
cargo build --release
```

The program is `target/release/graphene`. Details per system are in the
[installation guide](docs/en/installation.md#build-from-source); to work on Graphene
itself, see [CONTRIBUTING.md](CONTRIBUTING.md).

## Contributing

Bug reports and pull requests are welcome: read [CONTRIBUTING.md](CONTRIBUTING.md).

## Documentation

- [User guide](docs/en/README.md) ([en français](docs/fr/README.md)): installation,
  configuration, themes, shortcuts, tabs and panes, command blocks and more.
- [Contributor documentation](docs/dev/README.md): vision, architecture, technical
  decisions, development guide.

## License

Graphene Term is licensed under either of [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE), at your option.
