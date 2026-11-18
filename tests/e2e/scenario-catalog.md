# GUI E2E scenario catalogue

`gui-automation-probe.sh <backend> [scenario]` launches the real `graphene` executable with
an isolated home, XDG directories, a controlled shell fixture and a virtual desktop, then
drives it through the backend's input driver: `x11` (Xvfb, `xdotool`) or native `wayland`
(headless Sway, `wtype` and `wayland-automation`). Every scenario runs on both backends in
the `e2e` CI job.

The shell fixture gives each new shell a sequential pane ID; it runs bash without
Graphene's integration. The `shell-*`, `blocks` and `scroll` scenarios instead start a
real bash, zsh or fish with the integration, and write that shell's startup files to the private home
for a fixed prompt and the same readiness markers. Assertions use what the application
exposes to the outside: commands typed into the focused shell (which pane answers, its
`stty size`), the palette's recent-actions file, the process tree, the window list of the
display server, OCR of visible dialog, Settings and terminal text, and the clipboard and
primary selection read with `xclip` or `wl-paste`. Terminal words are located by OCR on a
known text, then the selection Graphene makes from them is read back, so the selection
also proves what is displayed. Screenshots are
failure artifacts, never pixel-comparison oracles. Waits poll for a state change with a
bounded timeout instead of sleeping for a fixed duration.

## Scenarios

| Scenario | User action exercised | Observable result asserted | Backends |
| --- | --- | --- | --- |
| `smoke` | Type a command into the controlled shell | The shell writes the computed `E2E_RESULT_42` marker | X11, Wayland |
| `tabs` | New/next/previous tab shortcuts, `Alt+1`/`Alt+2`, tab click, `+` button, middle-click close, tab context menu New tab and Close tab, `Ctrl+Alt+W` | The shell answering in the focused pane is the one of the expected tab after every step | X11, Wayland |
| `palette` | Open with `Ctrl+Shift+P`, search and click New tab; search and run Equalise panes and Move pane to a new tab; select tab entries | A new shell answers, the action is first in the recent-actions file, the detached pane keeps its shell and is reached through the new `Tab 3` entry | X11, Wayland |
| `panes` | Split right/down, focus by click, direction and next/previous, drag a divider, resize in all four directions, zoom/unzoom, palette Equalise panes and Close pane | `stty size` grows or shrinks as expected, zoom restores the previous width, equalised widths differ by at most one column, the expected pane answers after each focus change | X11, Wayland |
| `menu` | Right-click a pane, choose Split right with the keyboard | A second shell starts and answers as the active pane | X11, Wayland |
| `dialogs` | Close a pane, a tab and the window (`Ctrl+Shift+Q`) while `sleep` runs; cancel with Escape, then confirm with Enter | The confirmation text is shown; cancel keeps the program and shell alive; confirm closes the pane, the tab and finally the application process | X11, Wayland |
| `settings` | Open with `Ctrl+,`, pane context menu and palette; click each of the ten sidebar sections; press `Ctrl+,` again while open; close with Escape | Each selected section heading is read back, a single Settings window exists, Escape closes it, `config.toml` is unchanged and the terminal accepts input again | X11, Wayland |
| `selection` | Show a known text, then double-click a word, triple-click a line, drag across two words, `Shift`+click past them, `Ctrl`+drag a rectangle, copy with `Ctrl+Shift+C`, click to clear and copy again | The primary selection holds the word, the line, the dragged words, the extended range and the two-row rectangle; the clipboard holds the copy, and is left alone by a copy without selection | X11, Wayland |
| `clipboard` | Paste a command with `Ctrl+Shift+V` and with `Shift+Insert`; select a word and paste it with a middle click into a typed command | Each pasted command runs and writes its marker; the middle-click paste completes the command with the selected word | X11, Wayland |
| `scroll` | In bash with the integration, run a short command then print 300 rows; wheel up and down over that block; select it and press `Shift+PageUp`/`PageDown`; while `sleep` runs, press `Shift+Home` and `Shift+End` | OCR of the numbered rows: the capped block shows earlier rows, then row 300 again; `Shift+Home` brings the first block back into view and `Shift+End` returns to the bottom | X11, Wayland |
| `font` | `Ctrl+=`, `Ctrl+-` twice, `Ctrl+0` | `stty size` loses columns and rows with a larger font, gains columns past the initial width with a smaller one, and returns to the initial width on reset | X11, Wayland |
| `mouse` | Run a program that turns on SGR mouse reporting; `Shift`+drag a word, then drag without modifier | `Shift`+drag selects the word for Graphene; the plain drag reaches the program as a left press on column 1, row 1 and leaves the selection unchanged | X11, Wayland |
| `blocks` | In bash with the integration, run two commands; fold and unfold the last block with `Ctrl+Shift+F`, fold and unfold all with `Ctrl+Shift+Alt+F`, fold again and click the hidden-lines marker | OCR reads `3 lines hidden` and `1 line hidden` while folded and no marker once unfolded; the output copied afterwards is complete | X11, Wayland |
| `shell-bash` | In bash with the integration and startup files in the private home: type a command, press `Home` and complete it; run two commands; copy the command and output of the last block, select the previous and next blocks with `Ctrl+Shift+PageUp`/`PageDown` and copy them | The edited command runs; `Ctrl+Shift+Alt+C`/`O` copy the exact command and output of the last, previous and next blocks | X11, Wayland |
| `shell-zsh` | Same as `shell-bash` with zsh | Same as `shell-bash` | X11, Wayland |
| `shell-fish` | Same as `shell-bash` with fish | Same as `shell-bash` | X11, Wayland |

## Action coverage

Every action of `Action::ALL`, plus the routes the palette does not list, with the
scenario that exercises it, then the pointer and prompt routes that are not actions.
A unit test in `src/input.rs` fails when an action is missing from this table.

| Action | Routes covered | Scenario |
| --- | --- | --- |
| `new_tab` | shortcut, palette click, `+` button, tab context menu | `tabs`, `palette` |
| `close_tab` | shortcut, middle click, tab context menu, confirmation dialog | `tabs`, `dialogs` |
| `close_pane` | shortcut with confirmation dialog, palette | `dialogs`, `panes` |
| `split_right` | shortcut, pane context menu | `panes`, `palette`, `menu` |
| `split_down` | shortcut | `panes` |
| `next_tab` | shortcut | `tabs` |
| `previous_tab` | shortcut | `tabs` |
| `next_pane` | shortcut | `panes` |
| `previous_pane` | shortcut | `panes` |
| `focus_pane_left` | shortcut, pointer click | `panes` |
| `focus_pane_right` | shortcut, pointer click | `panes` |
| `focus_pane_up` | shortcut | `panes` |
| `focus_pane_down` | shortcut | `panes` |
| `resize_pane_left` | shortcut, divider drag | `panes` |
| `resize_pane_right` | shortcut | `panes` |
| `resize_pane_up` | shortcut | `panes` |
| `resize_pane_down` | shortcut | `panes` |
| `toggle_zoom` | shortcut | `panes` |
| `equalise_panes` | palette | `palette`, `panes` |
| `detach_pane` | palette | `palette` |
| `copy` | shortcut on a range and a block selection, without selection | `selection` |
| `paste` | `Ctrl+Shift+V`, `Shift+Insert`, middle click (primary) | `clipboard` |
| `copy_command` | shortcut on the last, previous and next blocks | `shell-bash`, `shell-zsh`, `shell-fish` |
| `copy_output` | shortcut on the last, previous and unfolded blocks | `shell-bash`, `shell-zsh`, `shell-fish`, `blocks` |
| `previous_block` | shortcut | `shell-bash`, `shell-zsh`, `shell-fish` |
| `next_block` | shortcut | `shell-bash`, `shell-zsh`, `shell-fish` |
| `toggle_fold` | shortcut, click on the hidden-lines marker | `blocks` |
| `toggle_fold_all` | shortcut | `blocks` |
| `scroll_page_up` | shortcut on the selected block | `scroll` |
| `scroll_page_down` | shortcut on the selected block | `scroll` |
| `scroll_to_top` | shortcut while a command runs | `scroll` |
| `scroll_to_bottom` | shortcut while a command runs | `scroll` |
| `font_size_increase` | shortcut | `font` |
| `font_size_decrease` | shortcut | `font` |
| `font_size_reset` | shortcut | `font` |
| `open_settings` | shortcut, pane context menu, palette | `settings` |
| `quit` | shortcut with confirmation dialog | `dialogs` |
| `go_to_tab_N` | `Alt+1`, `Alt+2`, palette tab entries | `tabs`, `palette` |
| `command_palette` | shortcut | `palette`, `panes` |
| text selection | double click (word), triple click (line), drag, `Shift`+click, `Ctrl`+drag (block), click (clear), `Shift`+drag while an application captures the mouse | `selection`, `mouse` |
| mouse reporting | press sent to an application that requested SGR reporting | `mouse` |
| block scrolling | wheel up and down over a capped block | `scroll` |
| prompt editing | `Home` through the shell integration | `shell-bash`, `shell-zsh`, `shell-fish` |

## Known coverage gaps

- Settings field edits, keybinding changes, themes and banners are left to #116.
- Reopening Settings while it is open is checked to reuse the window, not to raise it:
  winit 0.30's `Window::focus_window` is a no-op on Wayland, and Xvfb has no window
  manager to honour the X11 activation request.
- Tab dragging and pointer clicks on dialog buttons are not covered.
- Block selection by a click on a status stripe, folding by a click on a card's chevron,
  `Shift+PageUp`/`PageDown` on the timeline without a selected block and the other
  prompt-editor keys (word moves and erasure, input selection, click to move the cursor)
  are not covered; the scenarios cover the keyboard routes of the same actions and
  `Home`.
- Copying an image and pasting an image are not covered.
- Scrolling is only covered with the shell integration, where output forms blocks.
  With the controlled fixture (no integration) `Shift+Home` was observed to leave the
  view at the bottom: the history is then one live block that the timeline scroll does
  not move. This is recorded as an observation, not asserted.
- Detach is observed through the moved pane's shell and the new palette tab entry; the
  tab-bar geometry itself is not measured.

These gaps stay listed here rather than being represented as covered behaviour.
