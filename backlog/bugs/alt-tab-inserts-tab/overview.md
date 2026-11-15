# Alt+Tab inserts a tab character in the terminal

**Statut** : terminé — voir [technical.md](technical.md)

## Context

When the user presses Alt+Tab to switch to another window while using Graphene, then
returns to Graphene, a tab character has been inserted in the terminal. The issue occurs
every time.

## Goal

Switching windows with a keyboard shortcut and returning to Graphene should not insert
that shortcut's keystroke in the terminal.

## Decisions actées

- The bug concerns Graphene's keyboard input/focus handling, not a request to reserve
  Alt+Tab specifically.
- The issue is reproduced on Linux/X11/Cinnamon. Other environments have not been tested.
