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
- [Searching the history](#searching-the-history)

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

While the command runs, a progress it reports (see
[Tabs and panes](tabs-and-panes.md#progress)) is also drawn on the bottom edge of its
card.

Full-screen programs (`vim`, `htop`, `less`...) run as usual; their screen is not
split into blocks.

## Selecting a block

| Do this                                | With                                                |
|----------------------------------------|-----------------------------------------------------|
| Select a block                         | Click its status bar                                |
| Select the previous / next block       | `Ctrl+Shift+PageUp` / `Ctrl+Shift+PageDown`         |

The current block, the last one of the pane (the running command or the prompt waiting
for input), can be selected too: `Ctrl+Shift+PageDown` moves to it from the last
finished block, and selects it directly when no block is selected. On the current
block, `Ctrl+Shift+PageDown` does nothing more. Its output can only be copied once the
command has finished.

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
| Fold / unfold a block                  | Click the chevron at the top right of its card, `Ctrl+Shift+Z`, or **Fold block** / **Unfold block** in the right-click menu |
| Unfold a folded block                  | Click its "n lines hidden" line                     |
| Fold all finished blocks, or unfold them all | `Ctrl+Shift+Alt+Z`                            |

An output taller than the window is folded automatically when its command ends.
Unfold it to read it.

## Scrolling

A block shows at most one window height; a longer output scrolls inside its card.

| Do this                                         | With                                     |
|-------------------------------------------------|------------------------------------------|
| Scroll the block under the pointer, or the history | Mouse wheel                           |
| Scroll the selected block, or the history       | `Shift+PageUp` / `Shift+PageDown`        |
| Top / bottom of the history                     | `Shift+Home` / `Shift+End` (not at the prompt) |

## Searching the history

`Ctrl+Shift+F` opens a search bar at the top right of the active pane. It searches the
whole history of the pane as you type, and highlights every match; the current match
stands out in another color (two [theme colors](themes.md), **Search matches** and
**Current search match**).

| Do this                                | With                                                |
|----------------------------------------|-----------------------------------------------------|
| Open the search bar, or give it the keyboard back | `Ctrl+Shift+F`, or **Search the history** in the command palette |
| Go to the older / more recent match    | `Enter` / `Shift+Enter`, or the `↑` / `↓` buttons   |
| Close the bar                          | `Escape`, or the `×` button                         |
| Give the keyboard back to the shell, bar left open | Click in the pane content             |
| Give the keyboard back to the bar      | Click the bar, or `Ctrl+Shift+F`                    |

**Matching.** The search ignores the case by default. Three toggles next to the field
change it:

| Toggle | When on                                                         |
|--------|-----------------------------------------------------------------|
| `Aa`   | Upper and lower case differ                                     |
| `.*`   | The text is a regular expression (`Invalid regex` is shown while it does not parse) |
| `ab`   | Only whole words match                                          |

A match can span a line the terminal wrapped.

**Filters.** Two selectors keep part of the history; click one to go to its next value:

| Selector | Values                                                        |
|----------|---------------------------------------------------------------|
| Zone     | **All**, **Commands** (the command text, without the prompt), **Output** |
| Status   | **Any**, **Success**, **Failure** (blocks that are still running are left out of the last two) |

They are greyed out without [shell integration](#shell-integration): the search then
covers the whole history, which is not split into blocks.

**Counter.** The bar shows the current match and the number of matches, for example
`2/7`. Matches are counted from the bottom: `1` is the most recent. `Enter` goes up,
toward older matches, and wraps around at the oldest. Past 10,000 matches the counter
shows `10000+`.

**Moving to a match.** When the bar opens, the current match is the most recent one
above the bottom of the view. Moving to a match scrolls the history, or the output of
its block, to show it; a folded block unfolds when a match in its output becomes the
current one. Output added while the bar is open joins the matches without moving the
view. `Escape` leaves the view on the last match shown.

**The field.** `Left` / `Right`, `Home` / `End`, `Backspace`, `Delete` and
`Ctrl+Backspace` (erase a word) edit the text; `Ctrl+Shift+V` pastes, line breaks
becoming spaces. While the bar has the keyboard, nothing you type goes to the shell, but
the other Graphene shortcuts still work. A narrow pane hides the selectors first, then
the toggles.

**Not searched:** the command line you are typing, and the screen of a full-screen
program (`vim`, `less`, `htop`...). While such a program runs, `Ctrl+Shift+F` does
nothing, and an open bar closes.

Shortcuts can be changed: see [Keyboard shortcuts](keybindings.md).
