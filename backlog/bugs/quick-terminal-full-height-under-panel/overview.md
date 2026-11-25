# Quick terminal at full height goes under the desktop panel

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

When the quick terminal height is set to 100%, the quick terminal window extends under
the desktop environment panel (observed on Cinnamon), so part of it is hidden behind the
panel. Panel position (top, bottom, left, right) and size depend on the user's desktop
configuration.

## Objectif

The quick terminal always fits within the usable screen area and is never covered by the
desktop environment's reserved areas (panels, dock, menu bar), whatever its configured
edge and size.

## Décisions actées

- **Size reference**: the configured size (%) is computed against the usable area of the
  target screen (screen minus the areas reserved by the desktop), not the full screen.
  100% means the whole usable area along the chosen axis.
- **Edges**: applies to all four edges (top, bottom, left, right); the quick terminal is
  anchored to the edge of the usable area, not of the physical screen.
- **Platforms**: every platform where the quick terminal exists: Linux X11 on any desktop
  environment (not only Cinnamon) and macOS (menu bar and Dock).
- **Panel changes**: the usable area is read again each time the quick terminal is shown.
  No live resize while it is visible.
- **Auto-hide panels**: follow what the system reports as usable area. A panel that
  reserves no space (typically auto-hide) is not avoided.

## Liens

- Feature: [Quick terminal](../../features/quick-terminal/overview.md)
