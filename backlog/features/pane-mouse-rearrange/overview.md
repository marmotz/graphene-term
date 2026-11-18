# Rearrange panes with the mouse

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

Panes can be split, resized and closed, but once a layout exists there is no way to
move a pane to another place in it with the mouse. Changing the layout means closing
and re-splitting panes, which loses their content. Tabs can already be reordered by
dragging them in the tab bar; panes have no equivalent.

## Objectif

The user can grab a pane with the mouse and drop it elsewhere in the layout, in another
tab or in a new tab, without losing the pane's session.

## Décisions actées

### Pane title bar

- Every pane gets a title bar at its top, always shown, even for the only pane of a
  tab, and kept on the zoomed pane.
- It shows the pane's current title (the one the tab bar already uses: terminal title,
  else directory, else shell name). The user cannot rename it.
- The active pane's bar uses the color of the active pane border; the bars of the
  other panes use a darker version of it. The active border stays.
- A close button `×` closes the pane, with the usual confirmation when a program runs.
- A double click on the bar zooms or unzooms the pane.
- A right click on the bar opens the pane menu.
- A click on the bar gives the focus to the pane.

### Grabbing a pane

- A pane is grabbed by dragging its title bar with the left button. No modifier is
  needed, and a program reporting the mouse (vim, htop, tmux...) never gets these
  clicks: the bar is outside the terminal grid.
- Grabbing the zoomed pane unzooms the tab first, so the drop targets are visible.
- While a pane is dragged, the tab bar is shown even when the window has a single tab,
  so the pane can be dropped on a new tab; the panes are resized while it is shown.

### Rejected: modifier + drag

A modifier + drag anywhere in the pane (no title bar) was considered first. Rejected:
the natural default, Ctrl+Shift, already extends the selection, Ctrl+Alt and Super are
taken by window managers, and a hidden gesture is not discoverable.

### Dropping on a pane of the same tab

- Dropping on an edge (top, bottom, left, right) of a target pane splits it on that
  side and inserts the moved pane there.
- Dropping on the center of a target pane swaps the two panes.
- While dragging, the zone where the pane will land (half of the target, or its center)
  is highlighted with a translucent overlay. No ghost of the pane follows the cursor.

### Dropping outside the current tab

- Dropping on an existing tab of the tab bar moves the pane into that tab, splitting
  its active pane, and makes that tab active.
- Hovering an existing tab for more than 1 second while dragging switches to that tab:
  the drag goes on, and the pane can be dropped anywhere in it with the same rules as
  in the current tab.
- Dropping on the empty area of the tab bar, or on its `+` button, creates a new tab
  holding the pane, placed last.
- When the moved pane was the last one of its tab, that tab closes.

### Cancelling

- Pressing Escape while dragging cancels the move: the layout stays as it was before
  the drag (including the active tab).
