[Français](../fr/images-and-links.md)

# Images and links

- [Inline images](#inline-images)
- [Clickable links](#clickable-links)

## Inline images

Programs can draw images right in the terminal. Graphene supports the two common
formats:

- the **kitty graphics protocol**, still images and animations, including the
  placeholder mode used by `tmux` and some editors;
- **sixel** images.

To try it, use a program that supports either format, such as `chafa` or `img2sixel`
(libsixel):

```bash
chafa picture.png
img2sixel picture.png
```

How images behave:

- An image sits in the text like a block of characters: it scrolls with the output,
  stays in the history, and is erased when the program clears that part of the screen.
- Text printed over an image replaces that part of it.
- Zooming the font (`Ctrl+=` / `Ctrl+-`) resizes images with the text.
- Making the window narrower can cut an image; widening it back restores it.
- Each pane keeps up to 256 MiB of image data. A very large image or animation over
  that limit is refused.
- Selecting and copying a part of the screen holding one image also copies the image
  (see [Selection and clipboard](selection-and-clipboard.md#what-gets-copied)).

## Clickable links

Graphene makes two kinds of links clickable:

- **links set by programs**, which mark a piece of text as a link (the OSC 8 escape
  sequence; `ls --hyperlink`, `gcc`, `systemd` tools and others can emit them);
- **URLs detected in the text**, starting with `http://`, `https://`, `file:` or
  `mailto:`. Punctuation right after a URL (a final period, a closing quote or bracket)
  is left out of it.

A link under the mouse pointer is underlined.

| Do this                                    | With                                        |
|--------------------------------------------|---------------------------------------------|
| Open a link                                | Click it (without moving the mouse)         |
| Open a link where a program uses the mouse | `Ctrl` + click                              |
| Select text inside a link                  | Drag, double or triple click, or `Shift` + click |

Links open with your desktop's default application for them: your browser for web
links, your mail program for `mailto:`, your file manager or the matching application
for `file:`.

For safety, a `file://` link naming another machine (`file://other-host/...`) is not
opened: only files on your own machine are. Links set by programs may use any other
scheme (`ssh:`, `vscode:`...), as long as your desktop knows how to open it; detected
URLs are limited to the four schemes above.
