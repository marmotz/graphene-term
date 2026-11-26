[Français](../fr/getting-started.md)

# Getting started

## First launch

Start **Graphene Term** from your applications menu, or run `graphene` from another
terminal.

A window opens with your usual shell (the one in `$SHELL`). On the first launch,
Graphene also writes its configuration file with every setting commented out (see
[Configuration](configuration.md)); you do not need to edit it to get started.

If your shell is zsh, bash or fish, Graphene sets up its
[shell integration](command-blocks.md#shell-integration) by itself: your dotfiles are
not modified.

## Five things to try

1. **Command blocks.** Run a few commands, for example `ls` then `false`. Each
   command and its output form a block with a colored bar on the left: green for
   success, red for failure, yellow while it runs. Click the bar to select the block,
   then press `Ctrl+Shift+Alt+O` to copy its output, or `Ctrl+Shift+F` to fold it.
   See [Command blocks](command-blocks.md).
2. **The command palette.** Press `Ctrl+Shift+P`, type a few letters (`split`,
   `theme`, `tab`...) and press `Enter`. Every action is there, with its shortcut.
   See [Tabs and panes](tabs-and-panes.md#command-palette).
3. **Split the window.** Press `Ctrl+Shift+E` to split right, `Ctrl+Shift+O` to split
   down. Click a pane or use `Ctrl+Shift+Arrow` to move between them, and drag a
   pane's title bar to move it. See [Tabs and panes](tabs-and-panes.md).
4. **The settings tab.** Press `Ctrl+,` to change the font, the cursor, the
   keyboard shortcuts and more. Changes apply at once. See
   [Configuration](configuration.md#settings-tab).
5. **A theme.** Right click in the window, open **Themes** and pick one; or set it in
   the settings tab. See [Themes](themes.md).

At the prompt, you can also click in your command line to move the cursor, and select
text with `Shift+Arrow` like in an editor: see [Input editor](input-editor.md).

## Command line options

| Command                            | What it does                                                         |
|------------------------------------|----------------------------------------------------------------------|
| `graphene`                         | Opens a window running your shell                                    |
| `graphene -e <program> [args...]`  | Opens a window whose first pane runs `<program>` instead of the shell |
| `graphene -E <program> [args...]`  | Like `-e`, but the window stays open once `<program>` ends, until a key is pressed |
| `graphene --columns <n> --lines <n>` | Opens the window sized to show a grid of `<n>` columns and `<n>` lines |
| `graphene --no-restore`            | Opens a new window without restoring the saved session, and keeps it out of the session (see `window.restore_session` in [Configuration](configuration.md#window)) |
| `graphene --version`               | Prints the version and exits                                         |
| `graphene --help`                  | Prints the command line options and exits                            |
| `graphene --prune-config [--yes]`  | Removes the settings this version does not know from the configuration file (see [Configuration](configuration.md#removing-unknown-settings)) |

An argument before `-e` or `-E` that is not one of these options is an error: Graphene prints a message and exits with code 2 instead of opening a window.

### Running a program with `-e`

`-e` takes everything after it as the program and its arguments:

```bash
graphene -e htop
graphene -e ssh my-server
graphene -e vim --clean notes.txt
```

- The program runs directly, without a shell, so it has no command blocks.
- When it exits, its pane closes; if it was the last pane, Graphene quits.
- New panes and tabs opened afterwards start your shell as usual.
- Options placed after `-e` belong to the program: `graphene -e less --version` runs
  `less --version`. `graphene -e` with nothing after it is an error.

### Keeping the window open with `-E`

`-E` works like `-e`, except that the window does not close when the program ends, so
its output can still be read:

```bash
graphene -E ls -la
```

- Once the program ends, the pane shows `Program ended (code 0). Press any key to
  close.` below its output, with the exit code (or the signal that killed it).
- Any key closes the pane, shortcuts included; a modifier alone (`Shift`, `Ctrl`, `Alt`,
  `Super`) does not. Scrolling and selecting with the mouse still work.
- Use `-e` for interactive programs and for tools that expect the window to close
  (application launchers, `$TERMINAL -e`); use `-E` to read what a command printed.
- Like `-e`, `-E` comes last. If both are given, the first one wins and the other is
  passed to the program.

### Window size in cells

`--columns` and `--lines` size the first window to hold that grid, whatever the font
and the `window` settings: `graphene --columns 120 --lines 40 -e htop`.

- Either one can be given alone; the other dimension keeps the configured size.
- The window opens unmaximized, and its size is not remembered for the next start
  (`window.startup_mode = "remember"`).
- The window manager may still refuse the size, a tiling one for instance.

## Next steps

- [Keyboard shortcuts](keybindings.md): the full list, and how to change them.
- [Configuration](configuration.md): every setting.
- [Selection and clipboard](selection-and-clipboard.md) and
  [Images and links](images-and-links.md).
