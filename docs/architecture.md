# Architecture

## Stack

| Concern                           | Crate                                                    | Version |
|-----------------------------------|----------------------------------------------------------|---------|
| Window, input events              | `winit`                                                  | 0.30    |
| GPU                               | `wgpu`                                                   | 30      |
| Text shaping and rendering        | `glyphon` (on `cosmic-text` 0.19)                        | 0.12    |
| PTY, VT parsing, grid, scrollback | `alacritty_terminal`                                     | 0.26    |
| Clipboard                         | `arboard` (X11 + Wayland data control, no image support) | 3.7     |

## Modules

```
src/
  main.rs          entry point, logger setup
  app.rs           winit ApplicationHandler, Session (one window + one shell)
  terminal.rs      PTY spawn, Term state, event proxy to the UI thread
  frame.rs         snapshot of the visible grid with resolved colors
  input.rs         key -> bytes encoding, app shortcuts, paste wrapping
  palette.rs       theme, 256-color table, color resolution
  layout.rs        pixels <-> grid geometry
  selection.rs     click counting, mouse capture rule, selection updates, copied text cleanup
  renderer/
    mod.rs         wgpu device/surface, frame orchestration
    scene.rs       Frame -> rects + text runs (pure, no GPU)
    rects.rs       instanced solid rectangle pipeline
    rect.wgsl      shader for rectangles
    text.rs        glyphon text grid with per-row caching
```

Pure modules (`palette`, `input`, `layout`, `frame`, `selection`, `renderer/scene`) hold the logic
and are unit tested. GPU and window code stay thin.

## Threads

```
UI thread (winit)                         PTY thread (alacritty_terminal EventLoop)
-----------------                         ----------------------------------------
keyboard / mouse  --Notifier(Msg)-->      writes to PTY
                                          reads PTY, parses VT into Term
request_redraw   <--UserEvent::Terminal-- EventProxy::send_event(Wakeup, Title, ...)
RedrawRequested:
  lock Term (FairMutex)
  Frame::capture  (short critical section)
  unlock
  Renderer::render(frame)
```

- `Term` is shared as `Arc<FairMutex<Term<EventProxy>>>`. The UI thread only holds the
  lock to read the mode, scroll, resize or capture a frame.
- Terminal events (`Wakeup`, `Title`, `PtyWrite`, clipboard and color queries,
  `ChildExit`...) are forwarded to winit through an `EventLoopProxy<UserEvent>`.
- When the shell exits, the event loop exits and the window closes.

## Rendering pipeline

1. `Frame::capture` walks `term.renderable_content()` and produces one `FrameCell` per
   visible cell: character, final fg/bg (after DIM, INVERSE, HIDDEN, OSC overrides),
   bold/italic/underline/strikeout, wide-char spacer flag.
   The block cursor is baked into the cell colors; other shapes (beam, underline,
   hollow when unfocused) are returned separately.
2. `scene::build` converts the frame into:
    - `under` rects: cell backgrounds, consecutive same-color cells merged, default
      background skipped (it is the clear color);
    - `over` rects: underline, strikeout, non-block cursor;
    - text runs per row, each anchored at a grid column.
3. `Renderer::render` draws in one pass: clear, `under` rects, text, `over` rects.
   Colors are linearized when the surface format is sRGB.

### Text runs

A run groups consecutive ASCII cells sharing color, bold and italic. Every non-ASCII
character gets its own run. Each run is a separate glyphon `Buffer` placed at its
column origin, so a glyph whose advance differs from the cell width (fallback fonts,
CJK, symbols) cannot shift the rest of the line. Blank runs are dropped.

`TextGrid` caches the buffers per row and reshapes a row only when the hash of its
runs changes.

## Input

- `input::action_for` checks app shortcuts first (paste, scrollback, font size).
- Otherwise `input::key_to_bytes` encodes the key following xterm conventions:
  modifier parameter `1 + shift + 2*alt + 4*ctrl`, application cursor mode (`SS3`),
  Alt as ESC prefix, Ctrl+char control codes.
- Any input scrolls the view back to the bottom.
- Mouse wheel: scrollback on the primary screen, arrow keys on the alternate screen
  when the application enabled alternate scroll.

### Mouse selection

- `layout::point_at` converts the cursor pixel to a viewport cell and side, then
  `term::viewport_to_point` to a grid point (the view may be scrolled).
- Left press: `ClickTracker` counts clicks (400 ms, same cell) and `selection::
  start_selection` creates a `Simple` (or `Block` with `Ctrl`), `Semantic` or `Lines`
  selection in `Term::selection`, or extends it with `Shift`.
- `CursorMoved` while pressed updates the selection. Left release clears an empty
  selection (simple click), otherwise writes the cleaned text to the primary selection.
- Middle press pastes the primary selection through `input::paste_bytes`.
- Left and middle clicks are ignored when the application enabled mouse reporting,
  unless `Shift` is held (`selection::mouse_reserved`).
