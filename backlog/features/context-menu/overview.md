# Context menu (right click)

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

Like most terminals, Graphene Term should open a context menu on right click, giving
access to the common actions without having to know their keyboard shortcuts. Today
these actions are only reachable through shortcuts and the command palette.

## Objectif

A right click opens a menu of the common clipboard, pane, tab and application actions,
targeting what was clicked.

## Décisions actées

### Where the menu opens

- Right click in a pane opens the pane menu; right click on a tab of the tab bar opens
  the tab menu.
- A right click in a pane gives it the focus first: the "this pane" actions target the
  clicked pane.
- When the program of the pane reports the mouse (vim, htop, tmux...), the right click
  goes to the program; Shift+right click always opens the menu.

### Pane menu

- Copy, Paste.
- Split vertical, Split horizontal, Close this pane, Zoom / unzoom this pane (the
  existing zoom: the pane fills the whole tab, or goes back to its place in the split).
- New tab, Close this tab.
- Settings, Themes, Quit Graphene.
- When the click lands on a command block, the block actions are added at the top:
  copy the command, copy the output, fold / unfold.

### Tab menu

- New tab, Close this tab (the clicked tab), Settings, Themes, Quit Graphene. No
  clipboard or pane action.

### Entries

- An action that cannot apply right now (Copy without a selection, Close this pane on
  the only pane...) stays visible but greyed out, so the menu keeps a stable shape.
- Each entry shows its keyboard shortcut on the right, following the user's
  keybindings.
- "Themes" is a submenu listing the available themes; choosing one applies it, saved
  to the configuration like a change made in the settings screen.
- Closing a pane, a tab or the application asks for the same confirmation as the
  existing close requests when a program is still running.

### New actions

- "Close this tab" and "Quit Graphene" become ordinary actions: listed in the command
  palette and bindable in the keybindings like any other, with the default shortcuts
  `Ctrl+Alt+W` and `Ctrl+Shift+Q`.
- When the theme follows the system appearance, choosing a theme in the submenu
  replaces the theme of the current appearance (light or dark) and keeps following.
- The zoom entry reads "Zoom the pane" or "Unzoom the pane" by the tab's state.

## Dépend de

- [settings-screen](../settings-screen/overview.md): the "Settings" entry opens its
  window, and the theme change goes through the same configuration write.
