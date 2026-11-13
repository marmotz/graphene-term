# Command blocks

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

Command blocks are the first of the four "modern" pillars listed in
`docs/vision.md`. In a classic terminal, a command and its output are just lines lost
in the scrollback: finding where an output starts, copying it cleanly or hiding a long
log takes manual scrolling and careful mouse selection. The roadmap places this item
after **Selection and copy** and **Mouse reporting**, which it builds on.

A first implementation cut a single terminal grid into blocks by reading the OSC 133
marks the shell puts in its prompt. Tried with a real configuration (powerlevel10k), it
broke: the theme rebuilds the prompt and its own marks, and the blocks, chevrons and
stripes went wrong or vanished. The block model is reworked so that it no longer
depends on how the shell draws its prompt.

## Objectif

Group each command and its output into a block that the user can select, copy or fold
as a unit. Each block is a small terminal of its own, drawn and managed entirely by
Graphene; the shell only tells Graphene when a command starts and ends. Full
compatibility with shells, prompt themes and full-screen programs is kept.

## Décisions actées

### Block model

- Each block is its own terminal: its own grid, holding the shell prompt, the typed
  command and the command's output. A command can only redraw its own block (progress
  bars, cursor moves up): it can never alter a previous block.
- Graphene owns the blocks: where they start and end, how they are drawn, folded,
  selected and copied. Nothing of this is read from the prompt.
- The shell keeps the command line: its prompt (any theme, powerlevel10k included),
  its line editor, completion and history are unchanged and shown inside the current
  block.
- The only signals taken from the shell are "a command starts" (with its command
  line) and "the command ended, with this exit status". They come from invisible
  hooks Graphene injects (before / after each command), which neither wrap nor parse
  the prompt.
- The hooks (zsh, bash, fish) are shipped and injected by Graphene when it launches
  the shell, without modifying the user's dotfiles. They can be turned off later
  through the configuration feature.
- Without those signals (unsupported shell, `sh`, an `ssh` session to a remote host),
  everything that follows goes into one single block, until the local shell signals
  again.
- A block's output starts in an empty terminal of its own: a program that clears the
  screen before drawing (without the alternate screen) only clears its own block.
- `clear` / `Ctrl+L` empties the view: previous blocks leave the screen but stay in
  the history, reachable by scrolling up.

### Timeline and scrolling

- The blocks form a timeline, scrolled as a whole and anchored at the bottom (the
  current block always visible) until the user scrolls up.
- A block is never taller than the window (later: its pane). When its content is
  taller, the content scrolls inside the block; its prompt and command stay fixed at
  the top of the block, only the output scrolls.
- Mouse wheel over a block that can scroll scrolls its content, and stops at its ends.
  Over a block that cannot scroll, over a gap or around the blocks, it scrolls the
  timeline.
- While a command runs, its block follows the new output, unless the user scrolled
  inside it.

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
- No header above a block for now: no cwd, duration or other information drawn by
  Graphene. The card, the stripe and the chevron are the whole decoration.
- The active prompt (nothing run yet) sits in a neutral card, with no status color; it
  turns "running" when the command starts.
- A running command shows a "running" stripe; its partial output can already be
  selected, copied and folded.
- Without the shell signals: the single block is drawn as one card.
- Full-screen programs (alternate screen: vim, htop, less) take the whole window while
  they run: no card and no gap, same number of columns (the margin stays reserved,
  empty). Once they exit, the block list comes back and their block holds the command
  and whatever remains of its output.

### Selecting a block

- Clicking the status stripe in the margin selects the whole block (from the command,
  prompt excluded). Margin clicks stay Graphene's even when an application captures
  the mouse.
- A selected block is shown by highlighting its card (accent), not by highlighting its
  text like a classic selection. What gets copied is unchanged.
- A mouse text selection can span several blocks, as in a classic terminal; the
  copied text joins the selected parts of each block.
- Keyboard shortcuts move to the previous / next block and select the current one.
  `Ctrl+Shift+PageUp` / `Ctrl+Shift+PageDown` keep this meaning at the prompt too.
  `Ctrl+Shift+Up` / `Ctrl+Shift+Down` were first chosen, then given to pane focus by
  [tabs-splits-palette](../tabs-splits-palette/overview.md).

### Copy

- Copying a block copies command + output by default; distinct actions copy only the
  command or only the output. The prompt is never copied. The command copied is the
  command line the shell reported when it ran it.
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
- No automatic folding: a long output is already capped to the window height and
  scrolls inside its block.
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
- [rich-input-editor](../rich-input-editor/overview.md) depends on this feature. It
  planned to locate the prompt and the input from OSC 133 prompt marks, which this
  rework drops: it must be revisited once the block model is settled.
- [images-links](../images-links/overview.md) depends on this feature: its image
  sequence splitter plugs into the own PTY loop built here (#6).
