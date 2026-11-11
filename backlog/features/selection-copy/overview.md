# Selection and copy

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

The terminal works (PTY, VT emulation, scrollback, paste) but text cannot be selected
or copied. `Ctrl+Shift+C` is reserved and currently does nothing. This is the next
item of the roadmap (`docs/vision.md`).

## Objectif

Select text with the mouse and copy it, with the behaviour Linux users expect from a
modern terminal: several selection modes, clipboard and primary selection, and copied
text that is clean and faithful (no padding spaces, no artificial line breaks, every
Unicode character preserved).

## Décisions actées

### Selection modes

- **Drag**: left click + drag selects character by character.
- **Double-click**: selects a word; dragging afterwards extends word by word.
- **Triple-click**: selects a whole logical line (wrapped lines included); dragging
  afterwards extends line by line.
- **Block**: `Ctrl` + drag selects a rectangle.
- **Shift+click** extends the existing selection up to the clicked point.
- **Auto-scroll**: dragging past the top or bottom edge scrolls the history while
  selecting.
- Word separators for double-click are a fixed list for now (a path or a URL stays a
  single word). Making them configurable belongs to the configuration feature.

### Copy and paste

- `Ctrl+Shift+C` copies the selection to the clipboard.
- Any selection also feeds the primary selection (X11 / Wayland).
- Middle click pastes the primary selection.

### Copied text

- Trailing spaces of each line are removed.
- Leading spaces: only the indentation common to all lines is removed (dedent). A
  single line loses its leading spaces; a code block keeps its relative indentation.
- A long line wrapped by the terminal is copied as one line, with no added line break.
- Copied text never ends with a line break, whatever the mode: a paste never runs a
  command by itself.
- Every Unicode character is copied as is, including wide characters, combining
  characters and the symbols used by fancy prompts (Powerline, Nerd Font glyphs).

### When the selection disappears

- On a simple click.
- When a new selection starts.
- When the selected text changes: only when the rewritten content differs from what
  was there; a program redrawing the same text keeps the selection.
- On any keystroke sent to the shell. Modifiers alone, `Ctrl+Shift+C`, font zoom and
  history scrolling (`Shift+PageUp`...) keep the selection.
- Otherwise the selection stays visible and follows its text when the history scrolls.

### Interaction with full-screen applications

- When an application captures the mouse (vim, htop), `Shift` + drag forces a
  Graphene selection. The mouse reporting itself is a separate, later feature that
  must respect this rule.
- This rule applies from this feature on: in mouse mode, a click without `Shift` is
  ignored until mouse reporting exists.

### Highlight

- Selected cells swap their foreground and background colors. A dedicated color
  belongs to the themes feature.

## Ordre des features

- Comes before **mouse reporting** (roadmap), which shares mouse events and must keep
  the `Shift` override defined here. See [mouse-reporting](../mouse-reporting/overview.md).
- Comes before [rich-input-editor](../rich-input-editor/overview.md), whose input
  selection builds on the selection mechanism defined here.
- Comes before [command-blocks](../command-blocks/overview.md), whose block selection
  and copy reuse the selection, highlight and copied-text rules defined here.
- Comes before [images-links](../images-links/overview.md): a click without movement on
  a link opens it, a drag from a link still selects, and copy also carries images.
