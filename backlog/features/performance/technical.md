# Performance: technical design

Product decisions: [overview.md](overview.md).

## Measurements (current code)

Release build, `btop` 1.4.7 run as the shell (`SHELL=/usr/bin/btop`), NVIDIA RTX 4070 Ti
on Vulkan, X11, default window size. Timings from temporary logs (not committed), in ms
since process start, two runs:

| Step                                       | Time            |
|--------------------------------------------|-----------------|
| Window created                             | 15 - 38         |
| `Instance::new` + surface                  | ~65 - 78        |
| `request_adapter`                          | ~60 - 80        |
| `request_device`                           | ~60             |
| Pipelines, atlas, `FontSystem::new` (18)   | ~100            |
| Renderer ready, then shell spawned         | 337 - 386       |
| btop frame 1 (frame + layout, 795 runs)    | prepare 55      |
| btop frame 2 (data, 1386 runs)             | prepare 87 - 89 |
| Complete btop display                      | ~555 - 575      |

Steady state: a frame where btop redraws its graphs costs 60 - 80 ms of text
`prepare`; a frame where nothing changed still costs ~6 ms. `Frame::capture` (~60 µs)
and `scene::build` (~200 µs) are negligible.

Two causes, both confirmed by the numbers:

1. **Text rendering cost.** [scene.rs:136-163](../../../src/renderer/scene.rs) gives every
   non-ASCII character its own `TextRun` (needed so a fallback font advance does not
   shift the rest of the row). btop draws its boxes and graphs with box-drawing and
   braille characters, so a screen is ~1400 runs. [text.rs:155-181](../../../src/renderer/text.rs)
   then creates one glyphon `Buffer` per run and reshapes it with `Shaping::Advanced`
   whenever its row changed, and glyphon's `prepare` walks every `TextArea` on every
   frame. With btop, "frame then data" is visible because frame 1 takes 55 ms and
   frame 2 another 89 ms.
2. **Serial startup.** [app.rs:451-479](../../../src/app.rs) creates the window, then
   blocks on `Renderer::new` (GPU instance, adapter, device, pipelines, fonts: ~320 ms),
   and only then calls `Terminal::spawn`. The shell (or btop) starts ~350 ms after the
   window, while the GPU work and the shell startup are independent.

### Baseline (`cargo bench` and `graphene::perf` logs, before the new renderer)

Recorded with [#64](https://github.com/marmotz/graphene-term/issues/64)'s tooling:
AMD Ryzen 5 5600G, NVIDIA RTX 4070 Ti, X11, fixtures at 120x32 (see
`benches/fixtures/README.md`). Criterion median times:

| Bench                 | btop     | cat     | vim     | history |
|-----------------------|----------|---------|---------|---------|
| `parse`               | 285 µs   | 3.03 ms | 30 µs   | -       |
| `frame`               | 196 µs   | 152 µs  | 155 µs  | 151 µs  |
| `gpu_prepare/idle`    | 0.49 ms  | 0.33 ms | 0.34 ms | 0.34 ms |
| `gpu_prepare/changed` | 92.2 ms  | 69.9 ms | 63.7 ms | 70.6 ms |

Glyph cache alone ([#65](https://github.com/marmotz/graphene-term/issues/65), not yet
drawn by the renderer), same machine:

| Bench         | btop     | cat     | vim     | history |
|---------------|----------|---------|---------|---------|
| `glyphs_cold` | 13.0 ms  | 4.82 ms | 5.39 ms | 5.03 ms |
| `glyphs_warm` | 49.5 µs  | 17.5 µs | 24.0 µs | 17.6 µs |

`cold` shapes and rasterizes every distinct glyph of the screen once; `warm` is the
per-frame cost the new renderer pays for its lookups.

`changed` draws a blank frame then the fixture: nearly all of it is reshaping the text
runs of every row. `idle` reuses the shaped rows.

After the glyph pipeline ([#66](https://github.com/marmotz/graphene-term/issues/66),
glyphon removed), same machine, same benches:

| Bench                 | btop     | cat     | vim     | history |
|-----------------------|----------|---------|---------|---------|
| `frame`               | 182 µs   | 155 µs  | 161 µs  | 157 µs  |
| `gpu_prepare/idle`    | 0.29 ms  | 0.19 ms | 0.21 ms | 0.18 ms |
| `gpu_prepare/changed` | 0.42 ms  | 0.32 ms | 0.34 ms | 0.32 ms |

`changed` drops by about 99.5 %: a frame no longer reshapes anything, it looks up each
glyph in the cache and uploads one instance buffer. `idle` drops by about 45 %.

Startup with `RUST_LOG=graphene::perf=debug`, release build, `SHELL=/usr/bin/btop`,
default window size, three runs (ms since the start of `main`):

| Step                               | Time          |
|------------------------------------|---------------|
| window created                     | 15 - 22       |
| GPU instance and surface created   | 94 - 97       |
| GPU adapter ready                  | 172 - 180     |
| GPU device ready                   | 240 - 244     |
| renderer ready                     | 388 - 419     |
| shell spawned                      | 393 - 424     |
| first PTY output                   | 423 - 456     |
| first frame drawn after PTY output | 424 - 458     |

## Alternatives considered

- **Keep glyphon, cache one shaped buffer per character**: rejected (product decision).
  It removes the reshaping but keeps one `TextArea` per run, whose cost glyphon pays on
  every frame (the ~6 ms idle frame above).
- **`Shaping::Basic` for the single-character runs**: rejected for the same reason.
- **Restricting wgpu to Vulkan unconditionally**: rejected. Machines without Vulkan
  would no longer start. Kept as a measured option (see Startup).
- **Showing the window only once the first frame is drawn**: rejected (product
  decision). The window keeps appearing at once.
- **wgpu `PipelineCache` on disk**: not retained for now. The new renderer creates
  fewer pipelines; it can come back if the benchmarks show pipeline creation still
  dominates.

## Design

### Cell glyph renderer (replaces glyphon)

A terminal grid needs no shaping: one cell, one glyph. The renderer becomes a classic
glyph-atlas renderer.

- **New module `src/renderer/glyphs.rs` (CPU side)**: owns the `FontSystem` and a
  `SwashCache` (both from `cosmic-text`, already pulled by glyphon 0.12, re-exported as
  `glyphon::cosmic_text`; becomes a direct dependency `cosmic-text = "0.19"` once
  glyphon is removed). `GlyphKey = (char, bold, italic)` maps to a cached entry:
  - resolved once by shaping that single character (`Buffer`, `Shaping::Advanced`, so
    font fallback still works) and keeping its `CacheKey`;
  - rasterized once with `SwashCache::get_image`, placed in the atlas, with its
    placement offsets and content type (mask or color for emoji).
  - Owns cell metrics: the `measure` logic of [text.rs:90-119](../../../src/renderer/text.rs)
    moves here unchanged.
- **Atlas (`src/renderer/atlas.rs`)**: two textures, `R8Unorm` for masks and
  `Rgba8Unorm` for color glyphs, with a shelf allocator. When full, the atlas is cleared
  and the glyph cache dropped (rare: a terminal uses few distinct glyphs).
- **Glyph pipeline (`src/renderer/cells.rs`)**: one instanced quad per visible glyph
  (`position`, `size`, `uv`, `color`, `is_color`), same model as
  [rects.rs](../../../src/renderer/rects.rs). The quad is anchored on the cell origin
  plus the glyph placement: a fallback glyph wider than the cell overflows it instead of
  shifting the row, so the "isolate non-ASCII" rule is no longer needed.
- **Scene**: `TextRun` is replaced by `GlyphInstance` lists built per frame from the
  `Frame` cells (spaces skipped). Draw order is unchanged: `under` rects, glyphs, `over`
  rects, overlay glyphs.
- **Banner text** goes through the same pipeline at pixel positions (overlay list),
  which removes the second glyphon `TextRenderer` and the glyphon dependency.
- Behavior change: ASCII runs shaped with `Shaping::Advanced` could render a
  programming font's ligatures by accident. They no longer will. No ligature support is
  documented today.

Expected cost per frame: one hash lookup per non-blank cell plus one instance buffer
upload, well under 1 ms for a full btop screen; a never-seen glyph costs one
rasterization, once.

### Startup: shell spawned before the GPU

New order in `App::create_session`:

1. create the window;
2. create `Glyphs` (CPU only: `FontSystem::new` ~18 ms, metrics) to know the cell size;
3. compute the `GridLayout` and call `Terminal::spawn`: the PTY reader thread of
   `alacritty_terminal` starts parsing the shell output right away;
4. `Renderer::new(window, glyphs)` does the GPU work (~320 ms today) meanwhile.

`Renderer::new` takes the `Glyphs` instead of `(family, font_size)`. The first
`RedrawRequested` captures whatever the shell already wrote, so btop is drawn complete
on the first frame instead of after two slow ones. The window is still shown at once
(product decision).

On top of it, one measured experiment: create the wgpu instance with
`Backends::VULKAN` first and fall back to all backends when no adapter is found. Kept
only if the benchmark shows a gain on `Instance::new` + `request_adapter`.

### Benchmarks (local, criterion)

- `criterion` as a dev-dependency, version checked at implementation time; one bench
  target `benches/render.rs` (`harness = false`), run with `cargo bench`.
- Fixtures in `benches/fixtures/`: raw PTY output recorded once at a fixed size
  (btop screen, `cat` of a large file, vim on a source file, scrolled history), with the
  recording command written in a `README.md` next to them.
- Benches, each feeding a fixture into a `Term` through
  `alacritty_terminal::vte::ansi::Processor`:
  - `parse`: bytes into the `Term`;
  - `frame`: `Frame::capture` + `scene::build`;
  - `glyphs_cold` / `glyphs_warm`: glyph instance building with an empty and a filled
    glyph cache;
  - `gpu_prepare`: full renderer preparation on a headless wgpu device; skipped with a
    message when no adapter is available.
- Startup is measured outside criterion: `RUST_LOG=graphene::perf=debug` logs the time
  of each startup step and of the first complete frame (new `log` target, `debug` level,
  silent by default).

## Consequences checked against the code

- `glyphon` is only imported by [text.rs](../../../src/renderer/text.rs), driven from
  [renderer/mod.rs](../../../src/renderer/mod.rs): removing it touches only the
  renderer. `wgpu` 30 stays.
- [scene.rs](../../../src/renderer/scene.rs) tests on `TextRun`
  (`runs_group_by_style_and_isolate_non_ascii`, `blank_runs_are_dropped`, banner
  overlay) are rewritten for glyph instances.
- `Session` and `App` keep their shape; only `create_session` order and the
  `Renderer::new` signature change. Font size changes (`set_font_size`) clear the glyph
  cache and the atlas.
- Selection, mouse reporting and banners work on `Frame`/cells, untouched.

## Tests

- `glyphs.rs`: cache hit returns the same entry; bold/italic are distinct keys; a
  missing glyph falls back without panic; metrics match the current `measure`.
- `atlas.rs`: allocation, full atlas reset.
- `scene.rs`: glyph instances at the right cell origin, spaces skipped, wide character
  draws once, overlay glyphs after `over` rects.
- Startup order and frame times checked by hand with btop, htop, vim and the
  benchmarks; target: complete btop display close to Alacritty's, no "frame then data".

## Découpage en tâches d'implémentation

1. [#64](https://github.com/marmotz/graphene-term/issues/64) Local benchmarks and startup timing logs.
2. [#65](https://github.com/marmotz/graphene-term/issues/65) Glyph cache and atlas allocator.
3. [#66](https://github.com/marmotz/graphene-term/issues/66) Glyph pipeline replaces glyphon (depends on #65).
4. [#67](https://github.com/marmotz/graphene-term/issues/67) Spawn the shell before the GPU init (depends on #65).
5. [#68](https://github.com/marmotz/graphene-term/issues/68) Try the Vulkan backend first (depends on #64).
