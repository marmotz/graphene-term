# Rich input editor

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

The rich input editor is the second of the four "modern" pillars listed in
`docs/vision.md`. In a classic terminal, the command line is edited by the shell's line
editor (readline, zle...): the mouse cannot place the cursor, text cannot be selected
like in an editor, multi-line input is awkward, and shortcuts differ from the ones
people use everywhere else. The roadmap places this item after **Command blocks**, and
it relies on shell integration (OSC 133) to know where the prompt and the command input
are.

## Objectif

Let the user write commands in an editor-like input: multi-line input, cursor placement
with the mouse, text selection, and editor-like shortcuts, while keeping full
compatibility with shells and full-screen programs.

## Décisions actées

- **Model**: the shell keeps ownership of the command line. Graphene does not replace
  the shell's line editor (zle, readline, fish); it translates mouse actions, selections
  and shortcuts into the keys the shell's line editor understands.
- **Activation**: only while the shell reports a prompt via OSC 133. Inactive without
  shell integration, while a command runs, and in full-screen applications.
- **Supported shells (v1)**: zsh, bash and fish.
- **Multi-line input**: `Enter` runs the command, `Shift+Enter` inserts a new line.
- **Mouse**: a click in the input moves the shell cursor there; drag selects text in
  the input.
- **Selection replacement**: typing replaces the input selection, `Backspace` / `Delete`
  erase it, as in an editor.
- **Shortcuts (v1)**:
  - Words: `Ctrl+Left` / `Ctrl+Right` move by word, `Ctrl+Backspace` / `Ctrl+Delete`
    erase a word.
  - Lines: `Home` / `End` go to line start / end, `Ctrl+Home` / `Ctrl+End` go to the
    start / end of a multi-line input.
  - Keyboard selection: `Shift+arrows`, `Ctrl+A` selects the whole input.
    `Ctrl+Shift+arrows` are not used: they belong to pane focus
    ([tabs-splits-palette](../tabs-splits-palette/overview.md)), at the prompt too.
- **Copy**: an input selection is copied exactly like any other selection, with the
  behavior defined in [selection-copy](../selection-copy/overview.md).
- **Shell features stay with the shell**: completion (`Tab`) and history (`Up` /
  `Down`) are handled by the shell's line editor as usual.
- **Out of scope (v1)**: clipboard shortcuts `Ctrl+X/C/V` and undo `Ctrl+Z` in the
  input (they collide with terminal conventions such as `Ctrl+C`).

## Dépend de

- [command-blocks](../command-blocks/overview.md): relies on the same OSC 133 shell
  integration to know where the prompt and the input are.
- [selection-copy](../selection-copy/overview.md): input selection builds on the
  existing selection mechanism.
