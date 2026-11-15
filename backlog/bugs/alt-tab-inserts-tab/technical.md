# Technical design

## Current behavior

The issue has been reproduced on Linux with X11 and Cinnamon. Other environments have
not been tested.

The terminal window tracks whether it is focused in `src/app.rs:132`. Its
`WindowEvent::Focused` handler updates that state and may report focus changes to the
terminal (`src/app.rs:2303-2311`). The handler separately stores modifier state on
`ModifiersChanged` and sends pressed `KeyboardInput` events to `key_press`
(`src/app.rs:2312-2325`).

`key_press` first checks Graphene's bindings, then eventually encodes an unhandled key
and writes it to the active pane (`src/app.rs:1536-1613`). In `src/input.rs:264-325`,
`NamedKey::Tab` is explicitly encoded as a tab byte (or an Alt-prefixed tab when Alt is
active). `winit` marks focus-recovery key events as synthetic; on X11 it generates
synthetic presses for keys held when a window gains focus. Graphene previously ignored
that marker, allowing such a press to follow the normal terminal input path. See
[`WindowEvent::KeyboardInput` in winit 0.30.13](https://github.com/rust-windowing/winit/blob/v0.30.13/src/event.rs#L199-L218).

## Design

Do not forward `KeyboardInput` events marked `is_synthetic` to Graphene's key handling.
These events reconcile held-key state across focus changes and are not fresh user input;
in particular, they must not be encoded and written to the PTY. Continue handling
non-synthetic key presses normally.

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
- **Ignore winit's synthetic focus key events:** retained because the X11 backend marks
  generated focus-recovery events explicitly, allowing Graphene to distinguish them from
  ordinary key presses without reserving desktop shortcuts.

## Verification

- Reproduce on Linux/X11/Cinnamon and inspect the order of focus, modifier, and keyboard
  events around Alt+Tab.
- Confirm switching away from and back to Graphene does not add input to the shell.
- Confirm Tab still reaches the shell when Graphene's terminal window is focused.
- Check other window-switching shortcuts where available; no behavior on untested
  environments is assumed.

## Découpage en tâches d'implémentation

- [#117 — Prevent spurious terminal input on window focus switches](https://github.com/marmotz/graphene-term/issues/117)
