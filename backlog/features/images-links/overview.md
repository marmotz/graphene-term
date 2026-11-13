# Images and clickable links

**Statut** : tâches créées, voir [technical.md](technical.md)

## Contexte

Images and links are the fourth of the four "modern" pillars listed in
`docs/vision.md`. Today Graphene Term only displays text: a program cannot show an
image inline (a plot, a thumbnail, a preview), and a URL printed by a command is plain
text that has to be selected and copied by hand before it can be opened. The roadmap
places this item after **Tabs, splits, command palette**. Modern tools (image viewers,
file managers, `ls --hyperlink`, compilers, git) already emit the corresponding
escape sequences and expect the terminal to support them.

## Objectif

Display images inline in the terminal grid, using the kitty graphics protocol and
sixel, and make links clickable, both the explicit hyperlinks sent by programs (OSC 8)
and the URLs detected in the displayed text, while keeping full compatibility with
shells and full-screen programs.

## Décisions actées

### Images

- Both protocols are supported in this feature: the kitty graphics protocol and sixel.
- Kitty support covers still images, animations, and Unicode placeholders (so images
  also work through tmux and in editors that use them).
- Images scroll with the text and stay visible in the scrollback.
- Image memory is limited per terminal: when the limit is reached, the oldest images
  are removed first. The limit becomes configurable with the configuration feature.

### Links

- Clickable links: explicit hyperlinks sent by programs (OSC 8) and URLs detected in
  the text.
- Detected schemes: `http`, `https`, `file://` and `mailto:`. Other schemes are
  clickable only through OSC 8.
- A simple click opens the link, on release, if the mouse did not move. A drag that
  starts on a link selects text as usual; double and triple-click keep their selection
  behaviour.
- When an application captures the mouse (vim, htop, tmux), the click goes to the
  application; `Ctrl` + click forces the link to open, like `Shift` + drag forces a
  Graphene selection.
- On hover, the link is underlined and the mouse cursor becomes a hand. Nothing changes
  otherwise.
- On hover, the real target of the link is shown (tooltip), since an OSC 8 link can
  display a text that differs from its target.
- Links open with the system default application (browser, mail client, file manager).
  A custom opener belongs to the configuration feature.

### Copy

- `Ctrl+Shift+C` on a selection that contains an image puts both the text and the image
  in the clipboard; the pasting application picks the format.
- With several images in the selection, only the text is copied.

## Ordre des features

- Depends on [selection-copy](../selection-copy/overview.md): the click / drag rules
  and the copy behaviour extend it.
- Depends on [mouse-reporting](../mouse-reporting/overview.md): the `Ctrl` + click
  override only matters once clicks are reported to applications.
- Depends on [command-blocks](../command-blocks/overview.md): the image sequences are
  read through the own PTY loop that feature builds. Since the command-blocks rework,
  each block holds its own grid: links and images are looked up in the block under the
  pointer.
- Shares the link opening helper with
  [update-notification](../update-notification/overview.md).
