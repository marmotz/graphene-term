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
- **History search** with `Ctrl+Shift+F`: every match highlighted, case, regex and
  whole-word toggles, command / output and exit status filters:
  [history search](docs/en/command-blocks.md#searching-the-history).
- **Automatic shell integration** for zsh, bash and fish, without editing your
  dotfiles: [shell integration](docs/en/command-blocks.md#shell-integration).
- **Rich prompt editor**: click to move the cursor, select with the mouse or
  `Shift+Arrow`, editor shortcuts: [input editor](docs/en/input-editor.md).
- **Tabs and split panes**, drag panes to rearrange them, zoom a pane, type in several
  panes at once: [tabs and panes](docs/en/tabs-and-panes.md),
  [broadcast](docs/en/tabs-and-panes.md#broadcast).
- **Session restore**: tabs, pane layout, directories and font sizes come back at the
  next launch: [configuration](docs/en/configuration.md#window).
- **Per-pane font size**, zoomed with `Ctrl+=` / `Ctrl+-` or `Ctrl` + mouse wheel:
  [keyboard shortcuts](docs/en/keybindings.md#scrolling-and-font-size).
- **Font ligatures** and OpenType features: [font settings](docs/en/configuration.md#font).
- **Command palette** with fuzzy search over every action:
  [command palette](docs/en/tabs-and-panes.md#command-palette).
- **Inline images** (kitty graphics protocol with animations, sixel) and **clickable
  links** (OSC 8 and detected URLs): [images and links](docs/en/images-and-links.md).
- **Mouse selection**, including rectangular selection, and mouse reporting to
  applications: [selection and clipboard](docs/en/selection-and-clipboard.md).
- **Remappable keybindings**: [keyboard shortcuts](docs/en/keybindings.md).
- **Desktop notifications** for long commands, programs (OSC 9, OSC 777) and the bell,
  and a **progress indicator** (OSC 9;4): [notifications](docs/en/configuration.md#notifications),
  [progress](docs/en/tabs-and-panes.md#progress).
- **Quick terminal** attached to a screen edge, opt-in (`enabled`), shown and hidden by
  `graphene --quick-toggle` (macOS and X11), started and restored with the session:
  [quick terminal](docs/en/quick-terminal.md).
- **Settings window**, live config reload, remembered window size, update
  notification: [configuration](docs/en/configuration.md).
- **Eight built-in themes**, your own theme files, a theme editor, and themes that follow
  the system light/dark mode: [themes](docs/en/themes.md).
- **Interface in English and French**, following the system language:
  [language setting](docs/en/configuration.md#general).

## Comparison

Graphene compared with other Linux terminals, each with its default configuration.

**Performance**: Graphene is in the same range as the other terminals: it takes 0.22 s
to `cat` an 11 MB text file (measured on 0.1.3), against 0.16 s to 0.48 s for the others. It starts slower
and uses more memory than Alacritty. Measurements, the test machine and how to
reproduce them are in [Comparing terminals](docs/dev/development.md#comparing-terminals).

✅ supported, ❌ not supported, ⚠️ partial (see the notes under the table).

| Feature | Graphene 0.4.1 | Alacritty 0.17.0 | Kitty 0.50.0 | WezTerm 20260716 | Terminator 2.1.6 | GNOME Terminal 3.60.0 |
|---|---|---|---|---|---|---|
| **Rendering** | | | | | | |
| GPU rendering | ✅ | ✅ | ✅ | ✅ | ❌ | ❌ |
| Truecolor | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Inline images (kitty graphics protocol, with animations) | ✅ | ❌ | ✅ | ⚠️¹ | ❌ | ❌ |
| Sixel images | ✅ | ❌ | ❌ | ✅ | ✅ | ⚠️² |
| Translucent background | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ |
| **Shell** | | | | | | |
| Command blocks with exit status | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Fold / unfold outputs | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Copy a command or its output | ✅ | ❌ | ⚠️³ | ⚠️⁴ | ❌ | ❌ |
| Automatic shell integration (zsh, bash, fish) without editing dotfiles | ✅ | ❌ | ✅ | ❌ | ❌ | ❌ |
| Rich prompt editor (mouse cursor, selection, editor shortcuts) | ✅ | ❌ | ⚠️⁵ | ❌ | ❌ | ❌ |
| **Layout** | | | | | | |
| Tabs | ✅ | ❌ | ✅ | ✅ | ✅ | ✅ |
| Split panes | ✅ | ❌ | ✅ | ✅ | ✅ | ❌ |
| Drag panes to rearrange | ✅ | ❌ | ✅ | ❌ | ✅ | ❌ |
| Zoom a pane | ✅ | ❌ | ✅ | ✅ | ✅ | ❌ |
| Command palette (fuzzy) | ✅ | ❌ | ✅ | ✅ | ❌ | ❌ |
| **Interaction** | | | | | | |
| Clickable links (OSC 8 and detected URLs) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Rectangular selection | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Mouse reporting to applications | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Remappable keybindings | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Configuration** | | | | | | |
| Settings window (GUI) | ✅ | ❌ | ❌ | ❌ | ✅ | ✅ |
| Live config reload | ✅ | ✅ | ⚠️⁶ | ✅ | ✅ | ✅ |
| Themes following the system light/dark mode | ✅ | ❌ | ✅ | ⚠️⁷ | ❌ | ⚠️⁸ |
| Built-in themes | ✅ | ❌ | ✅ | ✅ | ✅ | ✅ |
| Interface translated (en, fr) | ✅ | ❌ | ❌ | ❌ | ✅ | ✅ |
| Remembered window size | ✅ | ❌ | ✅ | ❌ | ❌ | ❌ |
| Update notification | ✅ | ❌ | ⚠️⁹ | ✅ | ❌ | ❌ |

1. WezTerm implements part of the kitty graphics protocol.
2. GNOME Terminal: off by default, to turn on in the profile preferences.
3. Kitty copies or shows the last command's output with a shortcut, not the command.
4. WezTerm selects a command or its output once you add its shell integration to your
   dotfiles.
5. Kitty moves the cursor to a click in the prompt, without mouse selection.
6. Kitty reloads its configuration with a shortcut (`Ctrl+Shift+F5`), not by itself.
7. WezTerm needs a few lines of Lua in the configuration.
8. GNOME Terminal follows the system mode only with the system theme colors.
9. Kitty checks for updates only in its official builds, not in distribution packages.

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
