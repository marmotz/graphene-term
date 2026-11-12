# Architecture

## Stack

| Concern                           | Crate                                                    | Version |
|-----------------------------------|----------------------------------------------------------|---------|
| Window, input events              | `winit`                                                  | 0.30    |
| GPU                               | `wgpu`                                                   | 30      |
| Text shaping and rendering        | `glyphon` (on `cosmic-text` 0.19)                        | 0.12    |
| PTY, VT parsing, grid, scrollback | `alacritty_terminal`                                     | 0.26    |
| Clipboard                         | `arboard` (X11 + Wayland data control, no image support) | 3.7     |
| Config file                       | `toml` + `serde`                                         | 1       |
| Update check (HTTP, JSON, tags)   | `ureq` (rustls) + `serde_json` + `semver`                | 3, 1, 1 |

## Modules

```
src/
  main.rs          binary entry point: --version, logger setup, calls lib::run
  lib.rs           library root (modules, VERSION, run); `bench` re-exports for benches/
  app.rs           winit ApplicationHandler, Session (one window + one shell)
  banner.rs        top banner: content, priority, update banner actions, pixel layout, hit-testing
  config.rs        config.toml location, model, key-by-key parsing and validation
  terminal.rs      PTY spawn, Term state, event proxy to the UI thread
  frame.rs         snapshot of the visible grid with resolved colors
  i18n.rs          UI language detection (LC_ALL, LC_MESSAGES, LANG) and label translations
  input.rs         key -> bytes encoding, app shortcuts, paste wrapping
  palette.rs       Palette (terminal, selection and UI colors), 256-color table, color resolution
  theme.rs         theme files: key-by-key parsing, built-in themes, name resolution
  layout.rs        pixels <-> grid geometry
  opener.rs        opens a URI with xdg-open from a detached thread
  selection.rs     click counting, mouse capture rule, selection updates, copied text cleanup
  update/
    mod.rs         GitHub release check, release selection, notice and scheduling decisions
    state.rs       update.json state file (last check, latest release, ignored tag)
    checker.rs     update-check thread, pure tick (check or reuse the cache, next wake-up)
  renderer/
    mod.rs         wgpu device/surface, frame orchestration, headless renderer (benches)
    scene.rs       Frame -> rects + text runs (pure, no GPU)
    rects.rs       instanced solid rectangle pipeline
    rect.wgsl      shader for rectangles
    text.rs        glyphon text grid with per-row caching, overlay text
themes/            built-in themes, embedded with include_str!
benches/
  render.rs        criterion benchmarks: parse, frame, gpu_prepare
  fixtures/        recorded PTY output (btop, cat, vim) and the recorder script
```

Pure modules (`banner`, `config`, `palette`, `theme`, `input`, `layout`, `frame`, `selection`, `renderer/scene`) hold the logic
and are unit tested. GPU, window and HTTP code (`update::fetch_latest`) stay thin.

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
- The `update-check` thread (started in `App::resumed` when `updates.check` is true)
  wakes up when a check is due, reloads `update.json`, calls GitHub only when the last
  check is one interval old (otherwise reuses the cached release), saves the state and
  sends `UserEvent::Update(Option<Release>)` when the release to show changes. The
  session stores it and requests a redraw. `Checker::apply` forwards new `[updates]`
  settings through an `mpsc` channel (`recv_timeout` is the sleep); `check = false` ends
  the thread, `true` again restarts it. A failed check is retried after one hour.
- The update banner shows once the configuration error banner is gone. Its click
  targets are hit-tested in `left_press` before any selection: "Open release" runs
  `opener::open`, "×" hides the banner until a newer release, "Ignore this version"
  hides it and sends `Command::Ignore` to the thread, which writes `ignored` in
  `update.json` (written directly when the thread is not running).

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
    - text runs per row, each anchored at a grid column;
    - overlay text at pixel positions (banner).
3. `Renderer::render` draws in one pass: clear, `under` rects, text, `over` rects,
   overlay text. Colors are linearized when the surface format is sRGB.

### Banner

`Frame.banner` holds a `BannerLayout` (from `banner.rs`) and its theme colors
(`ui.banner_background`, `ui.banner_text`). It covers the top of the window, over the
grid (the grid does not move): its rect is the last `over` rect and its text goes to a
second glyphon renderer drawn after it. The layout gives the pixel rect of each action,
and `Session::left_press` hit-tests the banner before starting a selection.

The configuration error banner shows the first error of `config.toml` and the theme
file ("(+N more)" when there are several); every error is also logged. A click hides it.

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
- Auto-scroll: in `about_to_wait`, while dragging with the cursor above or below the
  grid (`layout::vertical_overflow`), the display scrolls by `selection::
  auto_scroll_delta` lines (1 plus 1 per cell height of overflow, capped at 10) every
  50 ms (`ControlFlow::WaitUntil`) and the selection end follows the edge row. Disabled
  on the alternate screen, which has no history.
- Middle press pastes the primary selection through `input::paste_bytes` and, like
  every paste, clears the selection.
- Text change: the release stores a `selection::SelectionSnapshot` in the `Session`.
  On each `TermEvent::Wakeup`, `SelectionSnapshot::validate` restores the selection if
  `Term` erased it, then clears it (and drops the snapshot) when its cleaned text
  differs from the snapshot. Every place that clears the selection drops the snapshot.
- Left and middle clicks are ignored when the application enabled mouse reporting,
  unless `Shift` is held (`selection::mouse_reserved`).
