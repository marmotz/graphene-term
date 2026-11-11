# Tabs, splits and command palette

**Statut** : tâches créées, voir [technical.md](technical.md)

## Contexte

Tabs and splits are the third of the four "modern" pillars listed in `docs/vision.md`.
Today Graphene Term runs a single shell per window: working on several things at once
means opening several windows or relying on a multiplexer such as tmux. The roadmap
places this item after **Command blocks** and **Rich input editor**. The vision keeps
tmux as the tool for remote sessions: this feature targets local, everyday ergonomics,
not a multiplexer replacement.

## Objectif

Let the user run several shells in one window, organised in tabs and split panes, and
reach every terminal action from a command palette, with all of this UI drawn by
Graphene Term itself with wgpu (no native toolkit widgets), while each pane keeps full
compatibility with shells and full-screen programs.

## Décisions actées

### General

- The whole UI (tab bar, pane borders and dividers, command palette, confirmation dialogs) is
  drawn by hand with wgpu, like the terminal grid.
- Keyboard shortcuts use the `Ctrl+Shift+...` family, like the existing
  `Ctrl+Shift+C` / `Ctrl+Shift+V`, so they never collide with keys applications use.
- One window per process: several windows, or moving a tab to another window, are out
  of scope.
- UI labels (palette entries, dialogs, tab bar) are translatable from the start:
  English and French are shipped, the language follows the system locale (`LANG`),
  English is the fallback. A manual choice belongs to the configuration feature.
- The UI text (tab bar, palette, dialogs) uses the system sans-serif font at a fixed
  size; the terminal font zoom (`Ctrl+=` / `Ctrl+-`) applies to every pane of the
  window at once.

### Tabs

- The tab bar is at the top. Its position is not hard-coded: it becomes configurable
  (top, bottom, left, right) with the configuration feature.
- The tab bar is hidden while there is a single tab and appears from the second one.
- Tab title: the title sent by the shell or program (OSC 0/2), otherwise the current
  directory.
- Mouse: click to activate, close button and middle click to close, `+` button to open
  a new tab, drag to reorder.

### Splits

- Free tree: any pane can be split horizontally or vertically, with no depth limit.
- Focus: mouse click, shortcut + arrow to reach the neighbour pane in that direction,
  and next/previous shortcuts.
- Resize: drag a divider, shortcut + arrow, zoom a pane to the full tab and back,
  equalise all panes of a tab.
- Active pane: inactive panes are slightly dimmed and the active pane has a
  highlighted border.
- A pane can be detached into its own new tab.
- Selection and scrollback are per pane: the mouse wheel scrolls the pane under the
  mouse, and a selection never crosses a pane boundary.

### Opening and closing

- A new tab or pane starts its shell in the current directory of the active pane,
  otherwise in the home directory.
- Closing a tab or pane asks for confirmation only when a program other than the shell
  is running in the foreground (vim, a build...).
- Closing the window asks for confirmation when a program is running in any pane,
  and lists them.
- When the shell of a pane exits, the pane closes; the last pane closes its tab, the
  last tab closes the window.

### Command palette

- Overlay at the top centre of the window; `Escape` closes it, `Enter` runs the
  selected entry.
- Lists every terminal action (with its shortcut shown) and the open tabs and panes,
  to jump to one by its title.
- Fuzzy search, recently used entries first. The recent order is kept across restarts.

- No command history palette: it was considered and dropped, because shells usually
  write their history file only when they exit, so recent commands would be missing.

### Default shortcuts

Letters are matched by the character they type (`Ctrl+Shift+T` is the key labelled T
on any layout). `Alt+1` ... `Alt+9` are matched by the physical digit row, so they
work on AZERTY where that row types `&`, `é`... without `Shift`.

| Action                           | Shortcut                                  |
|----------------------------------|-------------------------------------------|
| New tab                          | `Ctrl+Shift+T`                            |
| Close active pane (and its tab if it is the last one) | `Ctrl+Shift+W`       |
| Next / previous tab              | `Ctrl+Tab` / `Ctrl+Shift+Tab`, `Ctrl+PageDown` / `Ctrl+PageUp` |
| Go to tab 1 to 9                 | `Alt+1` ... `Alt+9`                       |
| Split right / split down         | `Ctrl+Shift+E` / `Ctrl+Shift+O`           |
| Focus neighbour pane             | `Ctrl+Shift+Arrow`                        |
| Resize active pane               | `Ctrl+Shift+Alt+Arrow`                    |
| Next / previous pane             | `Ctrl+Shift+N` / `Ctrl+Shift+B`           |
| Zoom / unzoom active pane        | `Ctrl+Shift+Z`                            |
| Command palette                  | `Ctrl+Shift+P`                            |

- Equalise panes and detach a pane into a new tab have no default shortcut: they are
  reached through the command palette.
- These keys are no longer sent to applications (`Alt+digit`, `Ctrl+Shift+Arrow`...).
  Rebinding or disabling them belongs to the configuration feature.

## Ordre des features

- Comes after [command-blocks](../command-blocks/overview.md) and
  [rich-input-editor](../rich-input-editor/overview.md) (roadmap). `Ctrl+Shift+Arrow`
  belongs to pane focus: block navigation uses `Ctrl+Shift+PageUp/PageDown`, and the
  rich input editor does not use `Ctrl+Shift+Arrow`.

- Comes before [config-themes](../config-themes/overview.md), which makes the tab
  position (top, bottom, left, right) and the UI colors (tab bar, command palette)
  configurable.
