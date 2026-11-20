[Français](../fr/command-blocks.md)

# Command blocks

With zsh, bash or fish, Graphene groups each command and its output into a **block**:
a card with a colored bar showing whether the command succeeded, that you can select,
copy and fold.

- [Shell integration](#shell-integration)
- [Blocks](#blocks)
- [Selecting a block](#selecting-a-block)
- [Copying a command or its output](#copying-a-command-or-its-output)
- [Folding](#folding)
- [Scrolling](#scrolling)

## Shell integration

Blocks need to know where each command starts and ends. Graphene gets this from a small
integration script it loads into your shell when it starts it.

- **Supported shells**: zsh, bash and fish, read from the `SHELL` environment variable.
- **Nothing to install**: your dotfiles (`.zshrc`, `.bashrc`, `config.fish`...) are
  not modified, and they are loaded as usual. Prompt themes such as powerlevel10k keep
  working.
- **Only the shell Graphene starts** is integrated. A shell you start by hand inside
  Graphene (typing `bash`, `ssh` to another machine) runs as in any terminal: its
  commands are not split into blocks.
- **macOS**: the shell starts as a login shell, like in Terminal.app. The system Bash
  (3.2) gets blocks but not the [input editor](input-editor.md); zsh, the macOS
  default, has full support.
- Other shells (nushell, xonsh...) run normally, without blocks.

To start the shell without the integration, set `GRAPHENE_SHELL_INTEGRATION=0` in
Graphene's environment:

```bash
GRAPHENE_SHELL_INTEGRATION=0 graphene
```

## Blocks

Each block is a card holding the prompt and the command you typed, then its output.
The bar in its left margin shows its status:

| Bar color | Meaning                         |
|-----------|---------------------------------|
| Yellow    | The command is running          |
| Green     | It ended successfully (exit status 0) |
| Red       | It failed (any other exit status) |

Full-screen programs (`vim`, `htop`, `less`...) run as usual; their screen is not
split into blocks.

## Selecting a block

| Do this                                | With                                                |
|----------------------------------------|-----------------------------------------------------|
| Select a block                         | Click its status bar                                |
| Select the previous / next block       | `Ctrl+Shift+PageUp` / `Ctrl+Shift+PageDown`         |

The selected block is the one the copy and fold shortcuts act on. Without a selected
block, they act on the last finished block.

## Copying a command or its output

| Copy                                   | With                                                |
|----------------------------------------|-----------------------------------------------------|
| The command of the block               | `Ctrl+Shift+Alt+C`, or **Copy block command** in the right-click menu |
| The output of the block                | `Ctrl+Shift+Alt+O`, or **Copy block output** in the right-click menu  |

The text goes to the clipboard, cleaned like any copy (see
[Selection and clipboard](selection-and-clipboard.md#what-gets-copied)).

## Folding

A folded block hides its output behind a single "n lines hidden" line.

| Do this                                | With                                                |
|----------------------------------------|-----------------------------------------------------|
| Fold / unfold a block                  | Click the chevron at the top right of its card, `Ctrl+Shift+F`, or **Fold block** / **Unfold block** in the right-click menu |
| Unfold a folded block                  | Click its "n lines hidden" line                     |
| Fold all finished blocks, or unfold them all | `Ctrl+Shift+Alt+F`                            |

An output taller than the window is folded automatically when its command ends.
Unfold it to read it.

## Scrolling

A block shows at most one window height; a longer output scrolls inside its card.

| Do this                                         | With                                     |
|-------------------------------------------------|------------------------------------------|
| Scroll the block under the pointer, or the history | Mouse wheel                           |
| Scroll the selected block, or the history       | `Shift+PageUp` / `Shift+PageDown`        |
| Top / bottom of the history                     | `Shift+Home` / `Shift+End` (not at the prompt) |

Shortcuts can be changed: see [Keyboard shortcuts](keybindings.md).
