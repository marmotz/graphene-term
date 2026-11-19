# Vision

Graphene Term is a modern, GPU-accelerated terminal emulator for Linux.

The idea: using a terminal should not mean living in the 80s. Graphene Term keeps
full compatibility with existing shells and full-screen programs (vim, htop, tmux...)
while adding the ergonomics people expect from modern tools.

## Name

- Display name: **Graphene Term**
- Binary and crate: `graphene`
- Graphene is a single layer of carbon atoms arranged in a hexagonal grid: a light,
  fast grid of cells, like a terminal.

## Target features

Selected as the project's "modern" pillars:

1. **Command blocks**: each command and its output form a block that can be selected,
   copied, folded (relies on shell integration hooks).
2. **Rich input editor**: multi-line input with mouse cursor placement, selection and
   editor-like shortcuts.
3. **Tabs and splits**: tabs, split panes, command palette.
4. **Images and links**: inline images (kitty graphics protocol, sixel), clickable
   links (OSC 8 and detected URLs).

## Roadmap

| Status  | Item                                                                                 |
|---------|--------------------------------------------------------------------------------------|
| Done    | Working terminal: PTY, VT emulation, truecolor, scrollback, resize, paste, font zoom |
| Done    | Selection and copy                                                                   |
| Planned | Mouse reporting to applications                                                      |
| Done    | Command blocks                                                                       |
| Done    | Rich input editor                                                                    |
| Done    | Tabs, splits, command palette                                                        |
| Planned | Images and clickable links                                                           |
| Done    | Configuration file and themes                                                        |
| Planned | IME support                                                                          |

## Non-goals (for now)

- Windows and macOS support: Linux first. The stack is portable, but nothing is
  tested elsewhere.
- Being a multiplexer replacement for remote sessions (tmux still has its place).
