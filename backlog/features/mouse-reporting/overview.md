# Mouse reporting

**Statut** : terminé, voir [technical.md](technical.md)

## Contexte

Full-screen applications (vim, htop, tmux...) can ask the terminal to report mouse
events so the user can click, drag and scroll inside them. Graphene Term does not
report the mouse yet: the selection feature already ignores a click without `Shift`
while an application has captured the mouse, waiting for this feature. It is the item
right after **Selection and copy** in the roadmap (`docs/vision.md`), and it keeps the
promise of full compatibility with existing full-screen programs.

## Objectif

Send clicks, drags and mouse wheel events to applications that request them, using the
SGR encoding (mode 1006), so vim, htop and tmux react to the mouse as in any modern
terminal, while `Shift` + drag still forces a Graphene selection as decided in
[selection-copy](../selection-copy/overview.md).

## Décisions actées

### Tracking modes and encoding

- Tracking modes supported: clicks (1000), drag (1002) and any motion (1003), as
  requested by the application.
- Encoding: SGR (1006) when the application asks for it; otherwise the historical X10
  encoding (limited to 223 columns/rows), so older applications still work. The UTF-8
  (1005) and urxvt (1015) encodings are not supported.
- Focus events (1004) are reported too: the application learns when the window gains
  or loses focus (used by vim and tmux).

### `Shift` gives the mouse back to Graphene

When an application captures the mouse, every mouse event goes to the application,
except with `Shift`:

- `Shift` + drag forces a Graphene selection (rule inherited from the selection
  feature); `Shift` + click extends it.
- `Shift` + middle click pastes the primary selection. A middle click without `Shift`
  goes to the application.
- `Shift` + wheel scrolls Graphene's history. The wheel without `Shift` goes to the
  application.
- The owner of a gesture (application or Graphene) is decided at press and kept until
  release: pressing `Shift` in the middle of a drag changes nothing.
- A click sent to the application clears the Graphene selection, like a simple click.
- The horizontal wheel is not reported.

### Other modifiers

- `Ctrl` + click on a link opens the link (decided in images-links) and is not sent.
- Anywhere else, `Ctrl` and `Alt` are sent to the application with the event.

### Wheel without mouse capture

- On the alternate screen (less, man) with no mouse capture, the wheel sends up/down
  arrow keys, so it scrolls the application.
- On the main screen, the wheel keeps scrolling the history.

### Visual feedback

- The mouse pointer is an I-beam by default and becomes an arrow while an application
  captures the mouse.

## Ordre des features

- Comes after [selection-copy](../selection-copy/overview.md).
- [images-links](../images-links/overview.md) depends on it: `Ctrl` + click forces a
  link to open while an application captures the mouse.
