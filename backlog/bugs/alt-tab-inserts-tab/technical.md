# Technical design

## Current behavior

The issue has been reproduced on Linux with X11 and Cinnamon. Other environments have
not been tested.

The terminal window tracks whether it is focused in `src/app.rs:132`. Its
`WindowEvent::Focused` handler updates that state and may report focus changes to the
terminal (`src/app.rs:2299-2307`). The handler separately stores modifier state on
`ModifiersChanged` and sends every pressed `KeyboardInput` event to `key_press`
(`src/app.rs:2308-2318`); the keyboard path does not check the window's `focused` state.

`key_press` first checks Graphene's bindings, then eventually encodes an unhandled key
and writes it to the active pane (`src/app.rs:1536-1613`). In `src/input.rs:264-325`,
`NamedKey::Tab` is explicitly encoded as a tab byte (or an Alt-prefixed tab when Alt is
active). This explains how an incoming Tab event can reach the shell, but does not
establish why that event reaches Graphene during a window switch.

## Design

Fix the keyboard event delivery/focus lifecycle so Graphene forwards only key input
intended for its focused terminal window. Investigate the actual X11/Cinnamon event
sequence during Alt+Tab before choosing the exact change; the current code alone does
not identify whether the unexpected input is a late key event, a focus transition, or
stale modifier state.

Do not special-case Alt+Tab or add a configurable list of desktop shortcuts. The issue is
treated as incorrect input handling by Graphene, not as a request to reserve a particular
shortcut. The correction should preserve normal Tab and other legitimate keyboard input
when the terminal window is focused.

## Alternatives considered

- **Suppress Alt+Tab in Graphene:** rejected because it handles one symptom rather than
  correcting the event/focus handling, and reserves a shortcut that belongs to the
  desktop environment.
- **Add a configurable deny-list of shortcuts:** rejected because it shifts
  responsibility to users and requires them to duplicate desktop-environment settings.
- **Correct input delivery around focus transitions:** retained because it addresses
  the reported behavior without changing how ordinary terminal input is encoded. The
  exact event-path change must be chosen after observing the event sequence.

## Verification

- Reproduce on Linux/X11/Cinnamon and inspect the order of focus, modifier, and keyboard
  events around Alt+Tab.
- Confirm switching away from and back to Graphene does not add input to the shell.
- Confirm Tab still reaches the shell when Graphene's terminal window is focused.
- Check other window-switching shortcuts where available; no behavior on untested
  environments is assumed.

## Découpage en tâches d'implémentation

- [#117 — Prevent spurious terminal input on window focus switches](https://github.com/marmotz/graphene-term/issues/117)
