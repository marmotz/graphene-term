[Français](../fr/quick-terminal.md)

# Quick terminal

The quick terminal is a terminal window attached to a screen edge, which you show and
hide with a single command. It stays resident between two uses: its shell keeps running
while it is hidden, and it comes back at once, as you left it.

## Show and hide it

```bash
graphene --quick-toggle
```

The command shows the quick terminal when it is hidden and hides it when it is shown.
The first time, it starts the quick terminal process, which then stays in the background
until the end of your graphical session. Bind the command to a key in your window
manager or in the keyboard shortcuts of your system (for example `F12`).

There is only ever one quick terminal per user: running the command several times at
once never opens a second one.

## Where it appears

The quick terminal covers a share of the screen against one of its edges, set in the
[`[quick_terminal]`](configuration.md#quick_terminal) section of the configuration or in
the **Quick terminal** section of the settings window:

- `edge`: the edge it is attached to, `top` by default;
- `size`: the share of the screen it covers, 40% by default;
- `screen`: the screen it appears on, the one under the mouse cursor by default.

These settings apply the next time it is shown. It has no title bar, stays above the
other windows, and shows on every desktop. It is out of the taskbar on Linux, out of
the Dock and of the application switcher on macOS.

## Platforms

- **macOS**: the quick terminal appears on the current Space, above full screen
  applications, below the menu bar.
- **Linux under X11**: the quick terminal covers the whole screen edge, above the
  desktop panels.
- **Linux under Wayland**: not available. `graphene --quick-toggle` prints
  `the quick terminal is not available under Wayland` and exits with the code 1. A
  Wayland session that runs Graphene through X11 (with `WAYLAND_DISPLAY` empty) uses
  the X11 quick terminal.
