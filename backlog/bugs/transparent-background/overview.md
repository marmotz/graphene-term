# Transparent terminal background does not work

**Statut** : terminé — voir [technical.md](technical.md)

## Context

The `window.background_opacity` setting is intended to make the terminal background
transparent. Regardless of its value, the background remains black and opaque.

## Goal

Make the content behind the terminal visible when `window.background_opacity` is set, and
make its value control the background's opacity.

## Decisions made

- Transparency must reveal the content behind the terminal.
- The value must control the full opacity range, from fully opaque to fully transparent.
- The behavior must work on all supported platforms.
