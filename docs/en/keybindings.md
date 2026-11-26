[Français](../fr/keybindings.md)

# Keyboard shortcuts

Every action below has a default shortcut, and every shortcut can be changed. The
[command palette](tabs-and-panes.md#command-palette) (`Ctrl+Shift+P`) lists all actions
with their current shortcut. The same shortcuts are used on Linux and macOS (with
`Ctrl`, not `Cmd`).

- [Default shortcuts](#default-shortcuts)
- [Changing shortcuts](#changing-shortcuts)
- [Action names](#action-names)

Shortcuts that only work while editing the command line are on the
[Input editor](input-editor.md) page; mouse gestures are on the pages of each feature.

## Default shortcuts

### Clipboard

| Keys                                  | Action                                     |
|---------------------------------------|--------------------------------------------|
| `Ctrl+Shift+C`, `Copy` key            | Copy the selection to the clipboard        |
| `Ctrl+Shift+V`, `Shift+Insert`, `Paste` key | Paste                                |

### Scrolling and font size

| Keys                                | Action                                                     |
|-------------------------------------|------------------------------------------------------------|
| `Shift+PageUp` / `Shift+PageDown`   | Scroll up / down a page (the selected block, or the history) |
| `Shift+Home` / `Shift+End`          | Top / bottom of the history                                |
| `Ctrl+=` or `Ctrl++`                | Bigger font in the active pane                             |
| `Ctrl+-` or `Ctrl+_`                | Smaller font in the active pane                            |
| `Ctrl+0`                            | Active pane back to the configured font size               |
| `Ctrl` + mouse wheel                | Bigger / smaller font in the pane under the mouse          |

Each pane has its own font size. A split starts at the size of the pane it comes from, a
new tab at the configured size.

At the prompt, `Shift+Home` and `Shift+End` select in the command line instead (see
[Input editor](input-editor.md)).

### Command blocks

| Keys                                      | Action                                              |
|-------------------------------------------|-----------------------------------------------------|
| `Ctrl+Shift+PageUp` / `Ctrl+Shift+PageDown` | Select the previous / next command block          |
| `Ctrl+Shift+Alt+C`                        | Copy the command of the selected (or last) block    |
| `Ctrl+Shift+Alt+O`                        | Copy the output of the selected (or last) block     |
| `Ctrl+Shift+F`                            | Fold / unfold the selected (or last) block          |
| `Ctrl+Shift+Alt+F`                        | Fold all finished blocks, or unfold them all        |

See [Command blocks](command-blocks.md).

### Panes

| Keys                                         | Action                                     |
|----------------------------------------------|--------------------------------------------|
| `Ctrl+Shift+E`                               | Split the pane, new pane on the right      |
| `Ctrl+Shift+O`                               | Split the pane, new pane below             |
| `Ctrl+Shift+W`                               | Close the pane                             |
| `Ctrl+Shift+Left` / `Right` / `Up` / `Down`  | Focus the pane in that direction           |
| `Ctrl+Shift+Alt+Left` / `Right`              | Resize the pane toward the left / right    |
| `Ctrl+Shift+Alt+Up` / `Down`, or `Ctrl+Shift+Alt+PageUp` / `PageDown` | Resize the pane upward / downward |
| `Ctrl+Tab` / `Ctrl+Shift+Tab`              | Next / previous pane                       |
| `Ctrl+Shift+X`                               | Zoom the pane to the whole tab, or back    |
| `Ctrl+Shift+B`                               | Broadcast input to every pane of the tab, or stop |

Some desktops (Cinnamon, for example) use `Ctrl+Shift+Alt+Up` / `Down` for themselves:
the `PageUp` / `PageDown` variants do the same.

See [Tabs and panes](tabs-and-panes.md).

### Tabs

| Keys                                    | Action                |
|-----------------------------------------|-----------------------|
| `Ctrl+Shift+T`                          | New tab               |
| `Ctrl+Alt+W`                            | Close the tab         |
| `Ctrl+PageDown`                         | Next tab              |
| `Ctrl+PageUp`                         | Previous tab          |
| `Alt+1` ... `Alt+9`                     | Go to tab 1 to 9      |

`Alt+1` to `Alt+9` use the digit row of the keyboard on any layout (on AZERTY too,
without `Shift`).

### Application

| Keys            | Action                     |
|-----------------|----------------------------|
| `Ctrl+Shift+P`  | Command palette            |
| `Ctrl+,`        | Settings tab               |
| `Ctrl+Shift+Q`  | Quit Graphene              |

## Changing shortcuts

Use the **Keyboard shortcuts** section of the settings tab (`Ctrl+,`): press a new
shortcut to add it, or remove one. Type in the field above the list to keep only the
actions whose name contains the text, or click **Find a shortcut** and press a shortcut
to see the actions that use it. Or edit `keybindings.toml`, which sits next to the
[configuration file](configuration.md#file-locations):

```toml
[keybindings]
"Ctrl+Alt+T" = "new_tab"        # adds a shortcut; Ctrl+Shift+T still works
"Ctrl+Shift+W" = "none"         # removes a default shortcut
"F11" = "toggle_zoom"
```

- Each line maps **one shortcut** to **one action**. Your lines come on top of the
  defaults: a default shortcut stays unless you map it to something else or to
  `"none"`.
- `"none"` removes a shortcut: the key goes to the program running in the terminal, as
  if Graphene had no shortcut for it. Use it to give a key back to your shell or to an
  application.
- An action can have several shortcuts.
- An invalid shortcut or action name is reported in the error banner and ignored.
- The file has the same format as `config.toml`, with a `[keybindings]` header. Graphene
  creates it on the first launch, with the defaults documented in comments.
- Saved changes apply to the open windows without restarting.
- Shortcuts that were in the `[keybindings]` section of `config.toml` are moved to
  `keybindings.toml` automatically at startup. Entries already in `keybindings.toml`
  win over the moved ones.

### Writing a shortcut

A shortcut is zero or more modifiers joined with `+`, then a key:

- **Modifiers**: `Ctrl`, `Shift`, `Alt`, `Super`.
- **Key**: one character (`T`, `5`, `,`, `+`), or a key name:
  `Left`, `Right`, `Up`, `Down`, `PageUp`, `PageDown`, `Home`, `End`, `Insert`,
  `Delete`, `Backspace`, `Tab`, `Enter`, `Escape`, `Space`, `F1` to `F24`, `Copy`,
  `Paste`, `Cut`, `ContextMenu`, `PrintScreen`, `Pause`, `ScrollLock`.

Rules:

- Names are not case-sensitive, and letters match either case: `"Ctrl+Shift+t"` is
  `Ctrl+Shift+T`.
- For a character other than a letter, `Shift` is ignored, because your keyboard
  layout already uses it to type the character: write `"Ctrl++"`, not
  `"Ctrl+Shift+="`.
- `Alt` + a digit matches the digit row on any keyboard layout.
- On a keyboard layout without Latin letters (Cyrillic, Greek...), a letter shortcut
  matches the key where that letter is on a US keyboard.

## Action names

| Action                                   | What it does                                      |
|------------------------------------------|---------------------------------------------------|
| `copy`                                   | Copy the selection to the clipboard               |
| `paste`                                  | Paste from the clipboard                          |
| `scroll_page_up`, `scroll_page_down`     | Scroll a page                                     |
| `scroll_to_top`, `scroll_to_bottom`      | Top / bottom of the history                       |
| `font_size_increase`, `font_size_decrease`, `font_size_reset` | Font size                    |
| `previous_block`, `next_block`           | Select the previous / next command block          |
| `copy_command`, `copy_output`            | Copy a block's command / output                   |
| `toggle_fold`, `toggle_fold_all`         | Fold a block / all blocks                         |
| `split_right`, `split_down`              | Split the pane                                    |
| `close_pane`                             | Close the pane                                    |
| `focus_pane_left`, `focus_pane_right`, `focus_pane_up`, `focus_pane_down` | Focus a neighbour pane |
| `resize_pane_left`, `resize_pane_right`, `resize_pane_up`, `resize_pane_down` | Resize the pane |
| `next_pane`, `previous_pane`             | Cycle through the panes of the tab                |
| `toggle_zoom`                            | Zoom the pane, or back                            |
| `equalise_panes`                         | Give all panes of the tab the same size (no default shortcut) |
| `detach_pane`                            | Move the pane to a new tab (no default shortcut)  |
| `new_tab`, `close_tab`                   | Open / close a tab                                |
| `rename_tab`                             | Rename the tab (no default shortcut)              |
| `next_tab`, `previous_tab`               | Cycle through the tabs                            |
| `go_to_tab_1` ... `go_to_tab_255`        | Go to a tab by its number                         |
| `command_palette`                        | Open the command palette                          |
| `open_settings`                          | Show the settings tab                             |
| `quit`                                   | Quit Graphene                                     |
| `none`                                   | Remove the shortcut                               |
