# Inline "+" button to add a keybinding

**Statut** : terminé, voir [technical.md](technical.md)

## Contexte

Since the settings are displayed in a tab, the settings window is much wider. In the
keybindings section, the "Ajouter" button of each action is aligned on the far right of
the row, far from that action's shortcut chips. On a wide window it is hard to tell which
"Ajouter" button belongs to which action.

## Objectif

Make adding a shortcut to an action visually tied to that action's existing shortcuts,
whatever the window width.

## Décisions actées

- Replace the right-aligned "Ajouter" button with a compact square "+" button placed
  right after the last shortcut chip of the row.
- The "+" has a tooltip "Ajouter un raccourci" / "Add shortcut", consistent with the
  "remove" tooltip already shown on chips.
- An action without shortcut shows the "+" alone: the "Aucun raccourci" text is removed.
- While a shortcut is being captured for an action, its "+" turns into a "x" at the same
  place to cancel the capture; the capture hint stays displayed under the row as today.
- Scope: the keyboard shortcuts section only. The Quick terminal shortcut field keeps its
  "Ajouter" / "Modifier" button and "Aucun raccourci" text, since its button already sits
  next to its shortcut.
