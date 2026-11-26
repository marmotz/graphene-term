[Français](../fr/selection-and-clipboard.md)

# Selection and clipboard

- [Selecting with the mouse](#selecting-with-the-mouse)
- [Copying](#copying)
- [What gets copied](#what-gets-copied)
- [Pasting](#pasting)
- [When a program uses the mouse](#when-a-program-uses-the-mouse)

## Selecting with the mouse

| Do this                          | With                                         |
|----------------------------------|----------------------------------------------|
| Select text                      | Left drag                                    |
| Select a word                    | Double click                                 |
| Select a whole line              | Triple click                                 |
| Select a rectangle (columns)     | `Ctrl` + left drag                           |
| Extend the selection             | `Shift` + left click                         |

- Dragging past the top or bottom of a pane scrolls it.
- What counts as a word for a double click is set by `word_separators` in the
  [configuration](configuration.md#selection). By default a URL or a `host:port` is one
  word.
- To select a whole [command block](command-blocks.md#selecting-a-block), click its
  status bar.
- The selection stays while the program redraws the same text; it is cleared when you
  type, when you paste, or when the selected text changes.

At the prompt, you can also select your command line with the keyboard: see
[Input editor](input-editor.md).

## Copying

| Copy to                          | How                                          |
|----------------------------------|----------------------------------------------|
| The clipboard                    | `Ctrl+Shift+C` (or the `Copy` key), or **Copy** in the right-click menu |
| The primary selection (Linux)    | Automatic: releasing the mouse after a selection copies it there |

`Ctrl+Shift+C` never sends `Ctrl+C` to the program, even with nothing selected, so it
cannot interrupt a command by mistake.

Some Wayland desktops do not support the primary selection; the clipboard still works.

## What gets copied

Copied text is cleaned up so it pastes well:

- spaces at the end of each line are removed;
- indentation common to all lines is removed;
- there is no newline at the end, so pasting a copied command never runs it by
  itself.

A selection holding one image (see [Images and links](images-and-links.md)) also puts
the image on the clipboard, next to the text: pasting into an image editor or a chat
gives the image.

## Pasting

| Paste                            | With                                         |
|----------------------------------|----------------------------------------------|
| The clipboard                    | `Ctrl+Shift+V`, `Shift+Insert` (or the `Paste` key), or **Paste** in the right-click menu |
| The primary selection (Linux)    | Middle click                                 |

When the program asks for it (most shells and editors do), the pasted text is marked as
a paste ("bracketed paste"), so the program does not run pasted lines one by one.

## When a program uses the mouse

Some programs (`vim`, `htop`, `tmux`, `mc`...) use the mouse themselves. In their
screen, clicks, drags and the wheel go to the program, not to Graphene.

- Hold `Shift` to select, extend or right-click as usual anyway.
- To open a [link](images-and-links.md#clickable-links) there, `Ctrl` + click it.
- `Ctrl` + wheel always changes the font size of the pane, it never goes to the
  program.
- In a full-screen program that does not use the mouse (`less`, `man`...), the wheel
  scrolls the program by sending it arrow keys.
- Clicks on a pane's title bar, on a block's status bar or chevron, and on earlier
  command blocks are always Graphene's.
