# Images and clickable links: technical design

Product decisions: [overview.md](overview.md).

Protocol references:

- Kitty graphics protocol:
  <https://sw.kovidgoyal.net/kitty/graphics-protocol/> (Unicode placeholders section,
  diacritics table `rowcolumn-diacritics.txt` in the kitty repository).
- Sixel: DEC VT330/VT340 graphics, DCS `ESC P <p1>;<p2>;<p3> q <data> ESC \`.
- OSC 8 hyperlinks:
  <https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda>.

## Building blocks already available

- **OSC 8** is fully parsed by `alacritty_terminal` 0.26: `Cell::hyperlink() ->
  Option<Hyperlink>` with `Hyperlink::uri()` / `id()` (`term/cell.rs:219`). Nothing to
  parse on our side.
- **URL search**: `alacritty_terminal::term::search::RegexSearch::new(&str)`
  (`term/search.rs:34`), `Term::regex_search_right`, `RegexIter`, and
  `Term::line_search_left` / `line_search_right` (logical line across `WRAPLINE`,
  `term/search.rs:593`). Same engine alacritty uses for its own URL hints.
- **Grid writes from Rust**: `Term` implements `vte::ansi::Handler`
  (`term/mod.rs:1059`): `input`, `goto`, `linefeed`, `terminal_attribute(Attr::
  Foreground | UnderlineColor)`. `Grid::cursor` (`point`, `template`) is public
  (`grid/mod.rs:113`), `Term::grid_mut()` too. A zero-width char passed to `input` is
  attached to the previous cell (`term/mod.rs:1069`), which is how the placeholder
  diacritics are stored.
- **Pixel cell size** is already sent to the PTY (`layout.rs:70`, `cell_width` /
  `cell_height`), so `TIOCGWINSZ` gives programs the pixel size they need to size images.
- **Timer**: `about_to_wait` already drives a `ControlFlow::WaitUntil` deadline for
  auto-scroll (`app.rs:413`); animations reuse it.
- **Cursor icon**: `Window::set_cursor(CursorIcon::Pointer)` (winit 0.30).

What is missing:

- `vte` 0.15 drops APC strings (`State::SosPmApcString` goes to `anywhere`,
  `vte/src/lib.rs:182`) and `alacritty_terminal`'s `Performer::hook/put/unhook` only
  log DCS (`vte/src/ansi.rs:1311`). Kitty graphics (APC `ESC _ G`) and sixel (DCS `q`)
  never reach us.
- `alacritty_terminal::event_loop::EventLoop` reads the PTY and parses straight into
  `Term` while holding the lock (`event_loop.rs:104`): no hook to see the bytes.
- `arboard` 3.7 sets one format at a time (`text`, `image`, or `html` + alt text,
  `lib.rs:229-256`): no text + image offer.

## Alternatives considered

| Topic | Options | Retained |
|---|---|---|
| Reaching image sequences | Own PTY loop / fork `alacritty_terminal` / filtering `EventedPty` wrapper | Own PTY loop. A fork must be rebased on every release. A filtering reader cannot know the cursor position and would deadlock trying to lock `Term` (the loop keeps the lock across reads). |
| Image anchoring | Cells in the grid (kitty Unicode placeholders) / side table with line anchors | Placeholder cells for every image. `Term` then handles scrolling, scrollback, erase, alternate screen, RIS, and the selection sees images. A side table would need scroll and erase events `Term` does not expose. |
| Image decoding | `image` crate / `png` + `flate2` | `png` + `flate2`: kitty only needs PNG and zlib; `image` pulls many codecs. Sixel decoder written in house (small, pure). |
| Text + image clipboard | arboard HTML with data URI / own multi-format owner | Own owner: `x11rb` (X11) and `wl-clipboard-rs` `copy_multi` (Wayland), both already in `Cargo.lock`. Offers `text/plain` and `image/png`. |
| URL detection | Own scanner / `RegexSearch` | `RegexSearch`, restricted to the hovered logical line. |
| Tooltip | All links / OSC 8 only | OSC 8 only (decided): a detected URL already shows its target. |

## Design

### 1. PTY loop: `src/pty.rs` (replaces `EventLoop`)

The own PTY loop is shared with command blocks and built there:
[command-blocks technical.md, "Own PTY loop"](../command-blocks/technical.md#own-pty-loop-srcptyrs),
issue [#6](https://github.com/marmotz/graphene-term/issues/6). It replaces
`alacritty_terminal::event_loop::EventLoop` (`polling` thread, write queue,
`Msg::{Input, Resize, Shutdown}`, synchronized updates) and exposes a parse step
`feed(processor, term, bytes)` meant to be split by later tasks. This feature only adds
the image splitter to that read path.

Read path, once both features are in:

1. the image splitter runs first on the raw buffer (so an APC or DCS payload is never
   seen by the OSC 133 scanner of command blocks);
2. its `Vt` chunks go through the OSC 133 scanner, then `processor.advance`;
3. for a `Kitty` / `Sixel` chunk: `stop_sync` first when a synchronized update holds
   bytes (`sync_bytes_count() > 0`), so the cursor is where the image belongs, then the
   graphics handler runs with the `Term` lock held (lock order of command blocks kept:
   `Term`, then `ShellState`, then `ImageStore`).

The splitter, `graphics::sequence::Splitter`:

```rust
pub enum Chunk<'a> {
    Vt(&'a [u8]),            // passed to Processor::advance(&mut term, bytes)
    Kitty(Vec<u8>),          // payload of ESC _ G ... ESC \
    Sixel(Vec<u8>),          // ESC P <params> q ... ESC \, params included
}
impl Splitter { pub fn feed<'a>(&mut self, bytes: &'a [u8]) -> Vec<Chunk<'a>>; }
```

- State survives across reads (a sequence split over two reads is reassembled).
- Only `ESC _ G` and DCS whose final byte is `q` are extracted; any other APC or DCS is
  passed through untouched, so `vte` keeps its current behaviour.
- A payload limit (same as the image store budget) aborts runaway sequences.

Chunks are handled in order with the `Term` lock held, so an image lands exactly where
the cursor is at that point of the stream. Replies to kitty commands (`OK`, `ENOENT`...,
honouring `q=1/2`) are written to the PTY directly by the loop.

### 2. Image model: `src/graphics/`

- `kitty.rs` (pure): parses the control keys (`a`, `f`, `t`, `o`, `m`, `i`, `I`, `p`,
  `c`, `r`, `C`, `U`, `q`, `d`, animation keys `a=f/a/c`, `z` gap...), reassembles
  chunked transmissions (`m=1`), decodes base64 (`base64`, already in the lock), zlib
  (`o=z`, `flate2`), PNG (`f=100`, `png`) and raw RGB/RGBA (`f=24/32`). Media: `t=d`,
  `t=f`, `t=t` (only files under the temp directory, deleted after reading as the spec
  requires), `t=s` (POSIX shared memory through `libc`, already in the lock).
- `sixel.rs` (pure): decodes sixel data to RGBA (palette `#`, repeat `!`, `$`, `-`,
  raster attributes `"`), background from `p2`.
- `store.rs`: `ImageStore` shared as `Arc<Mutex<ImageStore>>` between the PTY thread
  (writes) and the UI thread (reads at render):
  - images by id: RGBA frames, per-frame gap, animation state (`a=a` running, stopped,
    loop count), a monotonically increasing generation for texture invalidation;
  - placements by (image id, placement id): columns, rows, source rectangle, virtual
    (`U=1`) or internal;
  - byte budget per terminal: **256 MiB**; when exceeded, the oldest images (insertion
    order) are removed first. Cells still pointing at a removed image render as blank.
  - cleared on full reset (RIS): `Term` reports nothing, so the splitter also flags
    `ESC c` in passthrough bytes and the loop clears the store after advancing past it.
  Terminal-assigned ids (`I=`) are allocated from the top of the 32-bit range so they do
  not collide with client ids.

### 3. Anchoring: placeholder cells

`graphics::place(term: &mut Term<_>, image: u32, placement: u32, cols, rows,
move_cursor)`, called by the loop for every kitty direct placement (`a=T`/`a=p`
without `U=1`) and every sixel image:

1. save `grid().cursor` (point and template);
2. for each row: `linefeed` when not the first row (scrolls at the bottom, like a real
   image), `goto` the start column, set `Foreground(Spec(id low 24 bits))` and
   `UnderlineColor(Spec(placement))`, then `input('\u{10EEEE}')` followed by the row,
   column and id high-byte diacritics, for each column that fits before the right edge
   (the image is clipped, never wrapped);
3. restore the template; place the cursor as the protocol says (kitty: after the image
   unless `C=1`; sixel: on the line below the image).

Kitty virtual placements (`U=1`) write nothing: the client prints the placeholders
itself (tmux, editors). Both kinds therefore render through the same path.

Deletion (`a=d`, all `d=` variants) removes the image/placement from the store and, for
internal placements, resets the matching cells to blank by scanning `grid_mut()` (screen
and history). Deleting by position (`d=c/p/x/y/z`) uses the same scan.

Accepted limits, inherent to the cell model:

- text printed over an image replaces that part of the image (like sixel; kitty would
  keep the image above the text);
- no image under the text (`z < 0` is drawn like `z >= 0`);
- a column resize reflows placeholder rows like text, which can cut an image on a narrow
  window; growing the window back restores it.

### 4. Rendering

`frame.rs`: `FrameCell` gains `image: Option<ImageCell { image, placement, row, col }>`,
decoded from cells whose char is `U+10EEEE` (diacritics from `cell.zerowidth()`, ids
from `fg` / underline color; a missing column diacritic is inferred from the left
neighbour, as the spec allows). These cells produce no text run and no background rect.

`renderer/images.rs` (new): textured quad pipeline (one texture per image frame,
uploaded on first use or when the generation changes, dropped when the image leaves the
store). `scene::build` groups consecutive image cells of one placement into quads with
their source rectangle. The image is scaled to fit the placement's cell box, keeping its
aspect ratio and centered, as the spec defines for placeholders; a font zoom therefore
rescales images with the text. Draw order: clear, `under` rects, **images**, text, `over`
rects, **overlay** (tooltip).

Animations: the store exposes `next_frame_deadline(now)`; `about_to_wait` takes the
earliest of it and the auto-scroll deadline, advances due frames and requests a redraw.
Nothing runs while no visible animation exists.

### 5. Links: `src/links.rs` (pure) and `app.rs`

- `link_at(term, point) -> Option<Link { range, uri, explicit: bool }>`:
  1. OSC 8: `cell.hyperlink()`; the range extends left and right over adjacent cells
     with the same hyperlink id (across wrapped lines). Hyperlinks whose scheme is
     command blocks' `BLOCK_LINK_SCHEME` (`graphene-block`) are internal cell tags:
     treated as no hyperlink, so URL detection still applies to those cells;
  2. otherwise a `RegexSearch` over the logical line around `point`
     (`line_search_left/right`) with the URL pattern for `http`, `https`, `file` and
     `mailto`; trailing punctuation and unbalanced closing brackets are trimmed.
- Hover: `cursor_moved` recomputes `link_at` (only when the cell under the mouse
  changed), stores `hovered_link` in `Session`, sets `CursorIcon::Pointer` /
  `Default`, and requests a redraw. `Frame::capture` takes the hovered range and sets
  `underline` on its cells. On `Wakeup` the hovered link is recomputed (content may have
  changed).
- Tooltip: when `explicit`, the renderer draws an overlay box near the mouse with the
  URI (one glyphon `Buffer` outside the `TextGrid` row cache, plus a background rect),
  truncated to the window width.
- Click ([app.rs:177](../../../src/app.rs) / [:191](../../../src/app.rs)): the press
  records the cell; the release opens the link when the button was not dragged (same
  cell), `clicks == 1`, `Shift` not held, and `link_at` still matches. The selection
  logic is unchanged, so drag, double and triple-click from a link select as before.
  When `selection::mouse_reserved` is true, a `Ctrl` + press on a link is consumed by
  Graphene (not reported to the application) and opens on release.
- Opening: `std::process::Command::new("xdg-open").arg(uri)`, spawned from a detached
  thread that waits for the child (no zombie). Only the four schemes are opened for
  detected URLs; OSC 8 URIs are opened as given, except `file://` with a host other than
  empty, `localhost` or the local hostname, which is refused (logged).

### 6. Copy with images: `src/clipboard.rs`

- `selection::selected_text` strips placeholder chars and their diacritics before
  `clean_copied_text`, so text copies never contain `U+10EEEE`.
- `selection::selected_images(term) -> Vec<ImageRef>` lists the distinct placements
  whose cells are inside the selection range.
- `Ctrl+Shift+C`: no image or several images → text only through `arboard` (current
  path). Exactly one image → `clipboard::set_text_and_image(text, png)`: the image's
  current frame encoded to PNG (`png`), offered with the text:
  - X11: an `x11rb` connection on a dedicated thread owns `CLIPBOARD` and answers
    `TARGETS`, `UTF8_STRING`, `text/plain;charset=utf-8` and `image/png` until another
    client takes ownership. A selection with only an image offers only `image/png`.
  - Wayland: `wl_clipboard_rs::copy::Options::copy_multi` with both MIME sources.
  The primary selection keeps receiving text only (`set_primary`, unchanged).

### 7. Docs

- `docs/architecture.md`: modules `pty`, `graphics/*`, `links`, `clipboard`,
  `renderer/images`; image steps in the PTY read path; rendering pipeline order.
- `docs/decisions.md`: image splitter before the OSC 133 scanner, placeholder anchoring and its limits, own
  clipboard owner, xdg-open.
- `docs/vision.md`: roadmap status when done.

## Consequences checked against the code

- The PTY loop itself comes from command blocks (#6); this feature adds the splitter
  and the graphics handler to its read path. `app.rs` is unaffected by the loop.
- `Frame::capture` ([frame.rs:48](../../../src/frame.rs)) gets two inputs (image store
  snapshot for ids, hovered link range); existing tests keep passing with `None`.
- `scene::build` must skip text for image cells; existing scene tests are unaffected
  (no placeholder chars).
- `selection::selected_text` / `SelectionSnapshot` compare cleaned text: stripping
  placeholders keeps snapshots stable while an animation runs (cells do not change, only
  the texture).
- The `Ctrl` + click rule in mouse mode adds a branch before the `mouse_reserved` early
  return in `left_press` ([app.rs:179](../../../src/app.rs)); the mouse-reporting
  feature must keep it.
- New dependencies: `png`, `flate2`; direct use of `base64`, `x11rb`, `wl-clipboard-rs`,
  `libc` (already resolved in `Cargo.lock`, versions pinned to the locked ones).

## Tests

- `graphics::sequence`: kitty and sixel extraction, split across reads, other APC/DCS
  passed through, oversize abort.
- `graphics::kitty`: key parsing, chunked transfer, base64/zlib/PNG/raw decoding,
  quiet modes, error replies, id allocation, delete variants, animation frame timing.
- `graphics::sixel`: palette, repeat, line feeds, raster size, against small fixtures.
- `graphics::store`: budget eviction order, RIS clear.
- `graphics::place` on a `Term<VoidListener>`: cells written with the right diacritics
  and colors, clipping at the right edge, scrolling at the bottom, cursor positions,
  template restored; image follows scrolling and survives in history; erase removes it.
- `frame`: placeholder decoding (with inferred column), hovered link underlined.
- `links`: OSC 8 range across cells and wrapped lines, URL detection per scheme,
  trailing punctuation, no match on plain text.
- `selection`: placeholders stripped from text, `selected_images` count.
- Click rules: pure decision function (press cell, release cell, clicks, modifiers, mouse
  mode, link) → open / ignore.
- GPU texture upload, xdg-open and the clipboard owners are thin and tested manually
  (`kitty +kitten icat`, `chafa -f sixel`, `img2sixel`, `ls --hyperlink=auto`, tmux with
  `allow-passthrough`, paste into GIMP and a text editor).

## Découpage en tâches d'implémentation

| Issue | Task | Depends on |
|---|---|---|
| [#22](https://github.com/marmotz/graphene-term/issues/22) | Image sequence splitter wired into the PTY loop (§1) | #6 (command blocks) |
| [#23](https://github.com/marmotz/graphene-term/issues/23) | Image store, placeholder anchoring and rendering (§2-4) | #22 |
| [#25](https://github.com/marmotz/graphene-term/issues/25) | Kitty graphics protocol, still images (§2-3) | #22, #23 |
| [#26](https://github.com/marmotz/graphene-term/issues/26) | Sixel (§2-3) | #22, #23 |
| [#30](https://github.com/marmotz/graphene-term/issues/30) | Kitty animations (§2, §4) | #25 |
| [#24](https://github.com/marmotz/graphene-term/issues/24) | Links: OSC 8 and URL detection, hover and click to open (§5) | — |
| [#28](https://github.com/marmotz/graphene-term/issues/28) | Links: tooltip with the OSC 8 target (§5) | #24 |
| [#31](https://github.com/marmotz/graphene-term/issues/31) | Copy text and image to the clipboard (§6) | #23, #25 |
