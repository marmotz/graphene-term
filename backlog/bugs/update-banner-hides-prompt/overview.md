# Top banner hides the first prompt

**Statut** : terminé — voir [technical.md](technical.md)

## Context

When a new version is available, the update banner is drawn on top of the terminal
content. It covers the first prompt, which is then not visible. The configuration error
banner uses the same one-line banner at the top of the window and covers the terminal in
the same way.

## Goal

A banner at the top of the window must never cover terminal content. It takes its own
space at the top of the window and pushes the rest of the window down, so the prompt stays
visible.

## Decisions made

- The fix applies to every top banner: the update banner and the configuration error
  banner.
- The banner sits at the very top of the window, across its full width. When the tab bar
  is on the top edge, the banner is above it; the tab bar and the panes move down below the
  banner.
- When the banner disappears (closed with ×, or the configuration error dismissed), the
  panes take back the freed height, as if no banner had been shown.
