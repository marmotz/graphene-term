[Français](../fr/tabs-and-panes.md)

# Tabs and panes

A window holds tabs, and each tab can be split into panes, each running its own shell.

- [Tabs](#tabs)
- [Panes](#panes)
- [Pane title bar](#pane-title-bar)
- [Moving panes with the mouse](#moving-panes-with-the-mouse)
- [Zoom](#zoom)
- [Command palette](#command-palette)
- [Closing and confirmation](#closing-and-confirmation)

All shortcuts below are the defaults; see [Keyboard shortcuts](keybindings.md) to change
them.

## Tabs

| Do this                                | With                                             |
|----------------------------------------|--------------------------------------------------|
| Open a tab                             | `Ctrl+Shift+T`, or click `+` in the tab bar      |
| Close the tab                          | `Ctrl+Alt+W`, middle click on the tab, or its `×` |
| Next / previous tab                    | `Ctrl+PageDown` / `Ctrl+PageUp` |
| Go to tab 1 to 9                       | `Alt+1` ... `Alt+9`                              |
| Show a tab                             | Click it                                         |
| Reorder the tabs                       | Drag a tab along the bar                         |
| Tab menu (new tab, close, settings, themes) | Right click on a tab                        |

The tab bar appears once there are two tabs. Its edge is set by `position` in the
`[tabs]` section of the [configuration](configuration.md#tabs): `"top"` (default),
`"bottom"`, `"left"` or `"right"`. On the left or right, the tabs are stacked.

## Panes

| Do this                                | With                                             |
|----------------------------------------|--------------------------------------------------|
| Split, new pane on the right           | `Ctrl+Shift+E`                                   |
| Split, new pane below                  | `Ctrl+Shift+O`                                   |
| Close the pane                         | `Ctrl+Shift+W`, or `×` on its title bar          |
| Focus a pane                           | Click in it, or `Ctrl+Shift+Arrow`               |
| Next / previous pane                   | `Ctrl+Tab` / `Ctrl+Shift+Tab`                  |
| Resize the pane                        | Drag the divider between two panes, or `Ctrl+Shift+Alt+Arrow` |
| Give all panes the same size           | **Equalise panes** in the command palette        |
| Move the pane to a new tab             | **Move pane to a new tab** in the command palette, or drag it (see below) |

- On Linux, a new pane starts in the current directory of the focused pane; on macOS,
  in your home directory.
- A pane needs at least 10 columns and 3 rows: a split that would make it smaller does
  nothing.
- The panes that do not have the focus are slightly dimmed.
- At the shell prompt, `Ctrl+Shift+Left` / `Right` select words in the command line
  (see [Input editor](input-editor.md)); with several panes in the tab, they move the
  focus instead.

## Pane title bar

Every pane has a title bar at the top showing its title (set by the shell or the
running program, otherwise the current directory) and a `×` button. The bar of the
focused pane is drawn in the accent color. Clicks on the bar are handled by Graphene
and never reach the program in the pane.

| On the title bar   | Does                                  |
|--------------------|---------------------------------------|
| Click              | Focus the pane                        |
| Double click       | Zoom the pane, or back                |
| `×`                | Close the pane                        |
| Right click        | Pane menu                             |
| Drag               | Move the pane (see below)             |

The pane menu, also opened by a right click inside the pane, offers copy and paste,
split, close, zoom, new tab, close tab, settings, themes and quit. Right-clicked on a
finished [command block](command-blocks.md), it also offers to copy its command or
output and to fold it. Where a program uses the mouse (see
[Selection and clipboard](selection-and-clipboard.md#when-a-program-uses-the-mouse)),
hold `Shift` while right-clicking inside the pane.

## Moving panes with the mouse

Drag a pane by its title bar and drop it:

- near an edge of another pane: it is placed on that side of it;
- in the middle of another pane: the two panes swap places;
- on a tab: it moves to that tab (hold it over a tab for a second to show that tab and
  drop it precisely);
- on `+` or the empty part of the tab bar: it moves to a new tab.

The tab bar shows during the drag even with a single tab, and a translucent highlight
shows where the pane will land. `Escape` cancels the drag.

## Zoom

`Ctrl+Shift+X`, or a double click on the title bar, makes the pane fill the whole tab;
the same again restores the layout. The other panes keep running meanwhile.

## Broadcast

`Ctrl+Shift+B` sends what you type and paste to every pane of the tab at once; the same
shortcut again stops it. Each pane handles the keys in its own state (prompt, running
program). Graphene shortcuts are never broadcast: they only act on the active pane.
Broadcast continues while a pane is zoomed.

`Ctrl`+click on a pane (or on its title bar) adds it to the broadcast set or removes it,
without focusing it. The first `Ctrl`+click also adds the active pane, so the broadcast
starts between the two panes; on a link, `Ctrl`+click still opens the link. `Ctrl`+drag still
makes a rectangular selection, except where the application captures the mouse.

The title bar of a pane in the set is drawn in the theme color `ui.pane_broadcast` and
starts with `»` and the number of panes in the set.

## Progress

Programs that report their progress with the `OSC 9;4` sequence (as `winget`,
`systemd`, some build tools and package managers do, or any script printing
`\e]9;4;1;50\a`) get a thin progress bar on the bottom edge of the pane title bar and,
while the tab bar is shown, of their tab (the tab follows its active pane). The bar is
drawn in the accent color, red after an error, yellow while paused; an unknown progress
shows a segment sliding along the bar. It disappears when the program clears it, when
the command ends, or after 15 seconds without news. `[window] progress = false` (see
[Configuration](configuration.md)) hides every progress bar.

On Linux, the progress of the active tab also shows on the Graphene icon of the taskbar
or dock, where the desktop supports it (KDE Plasma, Ubuntu Dock, Dash to Dock, Plank).
An error or a pause shows its value there; an unknown progress shows no bar.

## Command palette

`Ctrl+Shift+P` opens the command palette: every action with its shortcut, then the open
tabs and panes, to jump to one.

- Type to filter. The search is fuzzy (`spr` finds "Split right") and ignores case,
  unless you type an uppercase letter.
- `Up` / `Down` to choose, `Enter` to run, `Escape` to close.
- Actions you ran recently come first. They are remembered between launches.

Actions without a default shortcut, such as **Equalise panes** and
**Move pane to a new tab**, are run from here (or given a shortcut, see
[Keyboard shortcuts](keybindings.md#changing-shortcuts)).

## Closing and confirmation

Closing a pane, a tab or the window while a program other than the shell is running in
it (an editor, `ssh`, a build...) asks for confirmation and names the program. When
only the shell is running, it closes at once. On macOS, closing never asks.

When the shell of a pane exits (`exit`, `Ctrl+D`), the pane closes; when the last pane
of the window closes, Graphene quits.
