# Rich input editor: technical design

Product decisions: [overview.md](overview.md).

## Starting point

- `alacritty_terminal` 0.26 does not handle OSC 133: `vte` 0.15 `osc_dispatch` logs
  unknown OSC as `[unhandled osc_dispatch]` and drops them. Graphene has no notion of
  prompt, input or running command yet.
- Keys are translated in [input.rs](../../../src/input.rs): `action_for` (app shortcuts,
  [input.rs:21](../../../src/input.rs)) then `key_to_bytes`
  ([input.rs:111](../../../src/input.rs)). `Shift+Enter` currently sends `\r` like
  `Enter`; `Shift+Home` / `Shift+End` are bound to `ScrollToTop` / `ScrollToBottom`.
- The keyboard branch of [app.rs:451-467](../../../src/app.rs) clears the selection on
  any key that produces bytes, then writes them to the PTY.
- Selection lives in `Term::selection`, highlighted by `frame.rs`
  ([frame.rs:149](../../../src/frame.rs)), copied by `selection::selected_text`, kept
  alive by `SelectionSnapshot` ([selection.rs:115](../../../src/selection.rs)). A
  simple click produces an empty selection cleared on release
  ([app.rs:191](../../../src/app.rs)).
- The shell is spawned in [terminal.rs:50-75](../../../src/terminal.rs) with a fixed
  environment (`TERM`, `COLORTERM`, `TERM_PROGRAM`).

## Prerequisite provided by command-blocks

This feature consumes, and does not implement, the shell integration owned by
[command-blocks](../command-blocks/overview.md):

- integration scripts for zsh, bash and fish, **injected by Graphene** (decided here,
  kitty-style: `ZDOTDIR` for zsh, `ENV` + POSIX mode for bash, `XDG_DATA_DIRS` for fish;
  the user's dotfiles are never modified): folder `shell-integration/`
  (`zsh/.zshenv`, `bash/graphene.bash`, `fish/vendor_conf.d/graphene.fish`), see
  [command-blocks technical.md](../command-blocks/technical.md#shell-integration-scripts)
  and issue #8. The fish script exists only to carry the bindings below;
- `ShellState` in `src/blocks.rs`
  ([command-blocks technical.md](../command-blocks/technical.md#shell-state-srcblocksrs-pure-tested-on-termvoidlistener),
  issue #9), locked after `Term` (fixed order):
  - `phase: Idle | Prompt | Input | Running` (`B` -> `Input`, `C` -> `Running`);
  - `block_bounds(term, id).input_start`: first cell tagged `input`, falling back to the
    cell after the last `prompt` cell before anything is typed. Input cells are tagged
    through the template hyperlink, so the point follows scroll and reflow.

Recording the grid position of a mark is why command-blocks replaces the alacritty
`EventLoop`, which parses whole read buffers (`pty_read` in
[event_loop.rs](https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/src/event_loop.rs)).
This feature only adds key bindings to the scripts command-blocks ships.

## Alternatives considered

| Topic | Options | Retained |
|---|---|---|
| Who edits the buffer | Graphene's own editor / shell line editor | Shell line editor (product decision). |
| Integration source | Injected by Graphene / user dotfiles | Injected: zsh and bash emit no OSC 133 natively, and the bindings below must be guaranteed. |
| Cursor moves | Plain arrows / dedicated passive keys | Passive keys: a plain `Right` at the end of the buffer accepts the autosuggestion (fish, zsh-autosuggestions); passive keys are bound to "move one char, clamped to the buffer, no side effect". |
| fish clicks | fish native `click_events=1` / same path as zsh and bash | Same path: one mechanism to test. Known limit below (fish indentation). |
| Buffer knowledge | Shell reports buffer + cursor / Graphene reads the grid | Grid: bash has no per-keystroke hook to report its buffer. Only *distances* are read from the grid; every edit is done by the shell. |
| Input selection | Shell region (zle `MARK`, fish visual mode) / `Term::selection` | `Term::selection`: highlight, copy and primary selection come from selection-copy unchanged (product decision). |

## Design

### Graphene key sequences

Graphene sends private sequences the integration scripts bind, using CSI u with
code points from the Unicode private use area so no real key collides. One constant
table in `src/input_editor.rs`, mirrored in the three scripts:

| Command | Sequence | zsh | bash | fish |
|---|---|---|---|---|
| `CharLeft` | `CSI 59136 u` | widget `CURSOR-=1` (zsh clamps) | `bind -x`, `READLINE_POINT--` if > 0 | `backward-char-passive` |
| `CharRight` | `CSI 59137 u` | widget `CURSOR+=1` | `bind -x`, bounded by `${#READLINE_LINE}` | `forward-char-passive` |
| `WordLeft` / `WordRight` | `CSI 59138 u` / `CSI 59139 u` | `backward-word` / `forward-word` | same readline functions | same |
| `DeleteWordLeft` / `DeleteWordRight` | `CSI 59140 u` / `CSI 59141 u` | `backward-kill-word` / `kill-word` | same | same |
| `LineStart` / `LineEnd` | `CSI 59142 u` / `CSI 59143 u` | `beginning-of-line` / `end-of-line` | `bind -x`: previous / next `\n` in `READLINE_LINE` | same as zsh |
| `LineUp` / `LineDown` | `CSI 59144 u` / `CSI 59145 u` | `up-line` / `down-line` (no history) | `bind -x`: same column on the previous / next line | `up-line` / `down-line` |
| `BufferStart` / `BufferEnd` | `CSI 59146 u` / `CSI 59147 u` | widget `CURSOR=0` / `CURSOR=$#BUFFER` | `bind -x` on `READLINE_POINT` | `beginning-of-buffer` / `end-of-buffer` |
| `InsertNewline` | `CSI 59148 u` | widget `LBUFFER+=$'\n'` | `bind -x`, insert `\n` at `READLINE_POINT` | `commandline -i \n` |
| `WordEnd` | `CSI 59149 u` | `emacs-forward-word` | `forward-word` | `forward-word` |

Bindings are installed in the emacs and vi insert keymaps (zsh `emacs`, `viins`;
bash `emacs`, `vi-insert`; fish default and insert modes).

To verify while implementing (not asserted here): bash `READLINE_POINT` unit in
multibyte locales (bytes vs characters), fish 4 `bind` syntax for raw sequences, the
oldest fish version providing the `-passive` functions (fall back to
`backward-char` / `forward-char` otherwise, as kitty does).

### Pure module `src/input_editor.rs`

Unit-tested, no window or PTY access.

- `enum EditorCommand { Move(Motion), Extend(Motion), SelectAll, InsertNewline,
  DeleteWord(Direction) }` and `enum Motion { Char, Word, Line, LineVertical, Buffer }`
  with a direction.
- `editor_command(key, mods) -> Option<EditorCommand>`: the v1 shortcut table of the
  overview (`Ctrl+Left/Right`, `Ctrl+Backspace/Delete`, `Home/End`, `Ctrl+Home/End`,
  `Shift+` arrows, `Ctrl+Shift+Left/Right` (extend to the word start / end), `Shift+Home/End`,
  `Ctrl+A`, `Shift+Enter`; `Ctrl+Shift+Up/Down` are not bound). Keys not
  in the table return `None` and follow the normal path (`Tab`, `Up/Down`, `Enter`,
  `Backspace` reach the shell unchanged).
- `motion_bytes(motion) -> &[u8]` and `repeat(CharLeft | CharRight, n)`.
- `buffer_distance(term, from: Point, to: Point) -> isize`: number of buffer
  characters between two grid points, walking cells: one per cell, wide-char spacers
  skipped, a line end without `WRAPLINE` counts as one `\n` and its trailing blank cells
  are ignored, cells tagged `prompt` (right prompt) are skipped. Negative when `to` is
  before `from`.

### Wiring in `app.rs`

Rich mode is active when `ShellState::phase == Input`, the terminal is not on the
alternate screen, and `selection::mouse_reserved` is false.

- **Keyboard**: in the `KeyboardInput` branch, before `action_for`, when rich mode is
  active and `editor_command` matches, run it and return. `Shift+Home/End` therefore
  win over the scroll shortcuts only while the user is at the prompt.
  - `Move(m)`: clear the selection, write the sequence.
  - `Extend(m)`: if no selection, start a `Simple` selection at the cursor point
    (`Side::Left`); write the sequence and set `extend_pending`. On the next
    `TermEvent::Wakeup` where the cursor moved, `selection.update(cursor, Side::Left)`,
    recapture the `SelectionSnapshot` and write the primary selection, as a mouse
    release does.
  - `SelectAll`: selection anchored at `input_start`, write `BufferEnd`, extend on
    wakeup as above.
  - `InsertNewline`, `DeleteWord`: write the sequence (with a selection inside the
    input, apply the replacement below first, for `DeleteWord` the deletion of the
    selection replaces the word deletion).
- **Selection replacement**: when rich mode is active, a selection exists and starts
  at or after `input_start`, and the key produces text, `Backspace` or `Delete`:
  1. `d = buffer_distance(cursor, selection_end)`: write `|d|` `CharLeft` or `CharRight`;
  2. `n = buffer_distance(selection_start, selection_end)`: write `n` `0x7f`
     (`Backspace`);
  3. write the typed text, nothing for `Backspace` / `Delete`;
  4. clear the selection.
  All in one PTY write so no shell redraw interleaves. A selection not entirely in the
  input keeps today's behavior (cleared, key sent as is).
- **Click**: in `left_release`, when the selection is empty (simple click), rich mode
  is active and the click point is at or after `input_start`: write
  `buffer_distance(cursor, click)` `CharLeft` / `CharRight`. A click past the end of
  the text (autosuggestion, empty space) is clamped by the shell. Drag, double and
  triple click keep the selection-copy behavior.

### Rendering

Nothing new: the input selection is a `Term::selection`, highlighted by `frame.rs`.

### Docs

- `docs/architecture.md`: `input_editor` module, dependency on `ShellState`.
- `docs/decisions.md`: shell keeps the buffer, passive private sequences, grid
  distances only.
- `README.md`: shortcuts at the prompt, `Shift+Home/End` meaning at the prompt.
- `docs/vision.md`: roadmap row to "Done".

## Consequences checked against the code

- `key_to_bytes` and its tests are unchanged: interception happens before it, in
  `app.rs`.
- `action_for` loses `Shift+Home/End` only while rich mode is active, by ordering in
  `app.rs`, not by changing `action_for`.
- `Ctrl+A` no longer reaches the shell at the prompt (`beginning-of-line` in emacs
  mode); `Home` covers it. Outside the prompt (running command, full-screen apps) it is
  sent as `0x01` as today.
- Without Graphene's integration (shell started by hand inside Graphene, ssh, other
  shell), `phase` never reaches `Input`: rich mode stays inactive and every key behaves
  as today.

## Known limits

- `buffer_distance` reads the grid: fish indents continuation lines with spaces not
  in the buffer, and trailing spaces before a newline are invisible. Distances can be
  off by these characters in multi-line inputs; moves stay clamped by the shell.
- Right prompts are tagged `prompt` by command-blocks (`OSC 133;P;k=r`), so
  `buffer_distance` skips them. Autosuggestions are drawn during the `Input` phase and
  tagged `input` although they are not in the buffer: harmless for clicks (clamped),
  but a selection dragged over them counts extra characters, so the replacement may
  erase too many characters. Detecting the real buffer end is out of scope for v1; the
  shell's undo restores the buffer.

## Tests

- `input_editor.rs`: shortcut table (each v1 shortcut, unmapped keys return `None`),
  sequence table, `buffer_distance` on a `Term<VoidListener>` (pattern of
  [frame.rs tests](../../../src/frame.rs)): plain line, wrapped line, wide characters,
  hard line break with trailing blanks, negative distance.
- Replacement byte stream: given cursor, selection and typed text, the exact bytes
  written (moves, backspaces, text).
- Integration scripts: one test per shell, skipped when the shell is not installed,
  spawning it in a PTY with the integration, sending the sequences and checking the
  resulting buffer (`print -r -- $BUFFER` / `echo $READLINE_LINE` / `commandline`).

## Découpage en tâches d'implémentation

| Order | Issue | Task | Depends on |
|---|---|---|---|
| 1 | [#27](https://github.com/marmotz/graphene-term/issues/27) | Shortcut table, private sequences and buffer distance | - |
| 1 | [#29](https://github.com/marmotz/graphene-term/issues/29) | Shell bindings for the private sequences | #8 |
| 2 | [#32](https://github.com/marmotz/graphene-term/issues/32) | Rich mode, editor moves and click to move the cursor | #27, #29, #9 |
| 3 | [#33](https://github.com/marmotz/graphene-term/issues/33) | Keyboard selection in the input | #32 |
| 3 | [#34](https://github.com/marmotz/graphene-term/issues/34) | Typing replaces the input selection | #32 |
