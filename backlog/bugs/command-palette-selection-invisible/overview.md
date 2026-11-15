# Command palette selection is invisible

**Statut** : tâches créées — voir [technical.md](technical.md)

## Context

In the command palette, the user cannot tell which action is selected, so choosing an
action with the keyboard is not possible in practice. `Up` / `Down` do move the
selection and `Enter` runs it, but the selected row is not highlighted.

The bug exists on `develop`, not only on the settings screen branch.

Likely cause: the selected row is painted with the active tab background color, which is
identical to the palette background in several bundled themes (Solarized Dark, Gruvbox
Dark) and nearly identical in others (Graphene Day).

## Goal

The selected row of the command palette is always clearly visible, in every bundled
theme and in user themes, so an action can be picked and run with the keyboard.

## Decisions made

- The palette selection gets its own theme color, set in every bundled theme, instead of
  reusing the active tab background.
- On top of that color, the selected row gets a marker that stays visible even when a
  theme sets the selection color too close to the palette background (for example an
  accent bar on the left edge).
- A user theme without the new selection color gets the `graphene-night` value, like
  any other missing theme color.
- The marker is a thin accent-colored bar on the left edge of the selected row.
- Other UI elements that rely on a background contrast with the same colors were
  checked. The active tab and the settings screen are fine; the border of the palette,
  dialog and tooltip is invisible in Solarized Dark, Solarized Light and Gruvbox Dark,
  and is fixed as part of this bug.
- A test on the bundled themes prevents these colors from becoming indistinguishable
  again.
