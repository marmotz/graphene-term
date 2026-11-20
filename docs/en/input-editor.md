[Français](../fr/input-editor.md)

# Input editor

At the shell prompt, Graphene lets you edit your command line like in a text editor:
click to move the cursor, select with the keyboard, replace a selection by typing.

- [When it is available](#when-it-is-available)
- [Shortcuts at the prompt](#shortcuts-at-the-prompt)
- [Selecting and replacing](#selecting-and-replacing)
- [Limits](#limits)

## When it is available

The input editor works at the prompt of zsh, bash or fish started by Graphene with its
[shell integration](command-blocks.md#shell-integration). It is off while a command
runs, inside full-screen programs, and in shells without the integration: there, every
key goes to the program as in any terminal.

Your shell keeps full control of the command line: completion (`Tab`), history
(`Up` / `Down`) and your own key bindings behave as usual.

## Shortcuts at the prompt

| Keys                            | Action                                        |
|---------------------------------|-----------------------------------------------|
| Left click in the command line  | Move the cursor there                         |
| `Ctrl+Left` / `Ctrl+Right`      | Start of the previous / next word             |
| `Ctrl+Backspace` / `Ctrl+Delete` | Erase the word before / after the cursor     |
| `Home` / `End`                  | Start / end of the line                       |
| `Ctrl+Home` / `Ctrl+End`        | Start / end of a multi-line command           |
| `Shift+Left` / `Shift+Right`    | Extend the selection by a character           |
| `Shift+Up` / `Shift+Down`       | Extend the selection by a line                |
| `Shift+Home` / `Shift+End`      | Extend the selection to the start / end of the line |
| `Ctrl+Shift+Left` / `Ctrl+Shift+Right` | Extend the selection to the start / end of a word |
| `Ctrl+A`                        | Select the whole command line                 |
| `Shift+Enter`                   | New line, without running the command         |

With several panes in the tab, `Ctrl+Shift+Left` / `Right` move the focus between panes
instead (see [Tabs and panes](tabs-and-panes.md#panes)).

At the prompt, `Ctrl+A` selects the command line instead of moving to its start as
shells usually do: use `Home` for that. These prompt shortcuts cannot be changed in the
configuration.

## Selecting and replacing

A selection in the command line is an ordinary selection: `Ctrl+Shift+C` copies it, and
on Linux it also fills the primary selection (middle-click paste).

Typing, `Backspace`, `Delete`, `Ctrl+Backspace`, `Ctrl+Delete` or `Shift+Enter` over a
selection in the command line replaces it, as in an editor. A selection that reaches
the output above the prompt is not replaced: it is cleared and the key goes to the
shell as usual.

## Limits

- `Ctrl+A` also selects the prompt text in front of your command, since Graphene does
  not know where the prompt ends. Typing over it only erases your command.
- A gray autosuggestion (fish, zsh-autosuggestions) is not part of your command line
  yet: a selection dragged over it may erase more than selected. Your shell's undo
  restores the line.
- On macOS, the system Bash (3.2) cannot run the input editor: the prompt behaves as
  in a plain terminal. Use zsh (the macOS default) or a newer Bash.
