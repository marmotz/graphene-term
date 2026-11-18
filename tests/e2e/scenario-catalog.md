# GUI E2E scenario catalogue

`gui-automation-probe.sh <backend> [scenario]` launches the real `graphene` executable with
an isolated home, XDG directories, a controlled shell fixture and a virtual desktop, then
drives it through the backend's input driver: `x11` (Xvfb, `xdotool`) or native `wayland`
(headless Sway, `wtype` and `wayland-automation`). Every scenario runs on both backends in
the `e2e` CI job.

The shell fixture gives each new shell a sequential pane ID. Assertions use what the
application exposes to the outside: commands typed into the focused shell (which pane
answers, its `stty size`), the palette's recent-actions file, the process tree, the window
list of the display server, and OCR of visible dialog and Settings text. Screenshots are
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

## Action coverage

Every action of `Action::ALL`, plus the routes the palette does not list, with the
scenario that exercises it. `pending` rows belong to the terminal (#115) and
configuration (#116) scenarios that are not written yet. A unit test in `src/input.rs`
fails when an action is missing from this table.

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
| `copy` | pending (#115) | — |
| `paste` | pending (#115) | — |
| `copy_command` | pending (#115) | — |
| `copy_output` | pending (#115) | — |
| `previous_block` | pending (#115) | — |
| `next_block` | pending (#115) | — |
| `toggle_fold` | pending (#115) | — |
| `toggle_fold_all` | pending (#115) | — |
| `scroll_page_up` | pending (#115) | — |
| `scroll_page_down` | pending (#115) | — |
| `scroll_to_top` | pending (#115) | — |
| `scroll_to_bottom` | pending (#115) | — |
| `font_size_increase` | pending (#115) | — |
| `font_size_decrease` | pending (#115) | — |
| `font_size_reset` | pending (#115) | — |
| `open_settings` | shortcut, pane context menu, palette | `settings` |
| `quit` | shortcut with confirmation dialog | `dialogs` |
| `go_to_tab_N` | `Alt+1`, `Alt+2`, palette tab entries | `tabs`, `palette` |
| `command_palette` | shortcut | `palette`, `panes` |

## Known coverage gaps

- Settings field edits, keybinding changes, themes and banners are left to #116.
- Reopening Settings while it is open is checked to reuse the window, not to raise it:
  winit 0.30's `Window::focus_window` is a no-op on Wayland, and Xvfb has no window
  manager to honour the X11 activation request.
- Tab dragging and pointer clicks on dialog buttons are not covered.
- Detach is observed through the moved pane's shell and the new palette tab entry; the
  tab-bar geometry itself is not measured.

These gaps stay listed here rather than being represented as covered behaviour.
