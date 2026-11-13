# Command blocks

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

Command blocks are the first of the four "modern" pillars listed in
`docs/vision.md`. In a classic terminal, a command and its output are just lines lost
in the scrollback: finding where an output starts, copying it cleanly or hiding a long
log takes manual scrolling and careful mouse selection. The roadmap places this item
after **Selection and copy** and **Mouse reporting**, which it builds on.

## Objectif

Group each command and its output into a block that the user can select, copy or fold
as a unit, using the shell integration protocol OSC 133 (prompt, command start, output
start, command end with exit status) to know where each block begins and ends, while
keeping full compatibility with shells and full-screen programs.

## Décisions actées

### Shell integration

- Block boundaries come from shell integration (OSC 133).
- The shell integration scripts (zsh, bash, fish) are shipped and injected by
  Graphene, without modifying the user's dotfiles (decided while designing
  [rich-input-editor](../rich-input-editor/technical.md), which needs it).
- Graphene injects the integration automatically when it launches bash, zsh or fish:
  no action needed from the user. It can be turned off later through the
  configuration feature.
- Other shells get blocks only if they emit OSC 133 themselves; otherwise the terminal
  behaves as a classic one, with no block.

### Display

- Blocks are drawn as cards, as Graphene UI around the terminal text rather than marks
  over it. This replaces the first rendering (a separator line over the grid and a
  one-cell mark), judged too close to console content once tried.
- A card holds its prompt, command and output. It has a background slightly tinted
  from the terminal background and rounded corners, with no border.
- Cards are separated by a gap of about half a line. The view stays anchored at the
  bottom (the prompt always visible): the gaps push the top of the screen into the
  history, so slightly fewer lines show at once. The shell keeps a fixed number of
  lines, nothing is lost.
- The left margin shows only a status stripe beside each card, colored by the exit
  status (success / failure / running). It is always reserved, even without shell
  integration, so the number of columns never changes mid-session.
- The active prompt (nothing run yet) sits in a neutral card, with no status color; it
  turns "running" when the command starts.
- A running command shows a "running" stripe; its partial output can already be
  selected, copied and folded.
- Without shell integration: no cards, the terminal renders as a classic one.
- Full-screen programs (alternate screen: vim, htop, less) are outside blocks: no card
  and no gap while they run, same number of columns (the margin stays reserved, empty).
  Once they exit, the block holds the command and whatever remains in the main history.

### Selecting a block

- Clicking the status stripe in the margin selects the whole block (from the command,
  prompt excluded). Margin clicks stay Graphene's even when an application captures
  the mouse.
- A selected block is shown by highlighting its card (accent), not by highlighting its
  text like a classic selection. What gets copied is unchanged.
- Keyboard shortcuts move to the previous / next block and select the current one.
  `Ctrl+Shift+PageUp` / `Ctrl+Shift+PageDown` keep this meaning at the prompt too.
  `Ctrl+Shift+Up` / `Ctrl+Shift+Down` were first chosen, then given to pane focus by
  [tabs-splits-palette](../tabs-splits-palette/overview.md).

### Copy

- Copying a block copies command + output by default; distinct actions copy only the
  command or only the output. The prompt is never copied.
- Copied text follows the same cleaning rules as
  [selection-copy](../selection-copy/overview.md): trailing spaces removed, common
  indentation removed, wrapped lines joined, no final line break, Unicode preserved.

### Folding

- A folded block keeps its command line visible, followed by a summary such as
  "342 lines hidden". A click on the summary, the chevron or a shortcut unfolds it.
- A chevron sits on the top-right corner of the card, straddling its top edge in the
  gap above, so it never covers text nor takes a column. It is always visible on a
  finished block with output, folded or not, and folds / unfolds the block on click.
- Actions: fold / unfold one block, fold / unfold all blocks at once.
- Long outputs fold automatically when the command ends, when they are taller than the
  window. The threshold becomes configurable with the configuration feature.
- Folding is visual only: copying a folded block copies its whole output.

### Shortcuts

- `Ctrl+Shift+PageUp` / `Ctrl+Shift+PageDown`: previous / next block.
- `Ctrl+Shift+C`: copies the selection, so command + output on a selected block.
- `Ctrl+Shift+Alt+C`: command only. `Ctrl+Shift+Alt+O`: output only.
- `Ctrl+Shift+F`: fold / unfold the block. `Ctrl+Shift+Alt+F`: all blocks.
- These act on the selected block, otherwise on the last finished one.

## Ordre des features

- Depends on [selection-copy](../selection-copy/overview.md): block selection and copy
  reuse its selection, highlight and copied-text rules.
- [rich-input-editor](../rich-input-editor/overview.md) depends on this feature: it
  reuses the OSC 133 shell integration to locate the prompt and the input.
- [images-links](../images-links/overview.md) must ignore the internal hyperlinks
  this feature puts on prompt and command cells (scheme `graphene-block`, see
  [technical.md](technical.md)).
- [images-links](../images-links/overview.md) depends on this feature: its image
  sequence splitter plugs into the own PTY loop built here (#6).
