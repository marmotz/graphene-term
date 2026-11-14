# Transparent background — technical design

Product decisions: [overview.md](overview.md).

## Current code state

- The configuration accepts `window.background_opacity` in `[0.0, 1.0]` and clamps
  out-of-range values ([src/config.rs:530](../../../src/config.rs)). It is passed to the
  renderer at creation ([src/app.rs:1740](../../../src/app.rs)) and applied again on
  hot reload ([src/app.rs:617](../../../src/app.rs)), so the configuration format is
  unlikely to be the cause.
- The window is created with `with_transparent(true)`
  ([src/app.rs:1702](../../../src/app.rs)). The renderer chooses `PreMultiplied` if
  available, otherwise `Inherit`, otherwise the first available mode; in the last case,
  it logs a warning ([src/renderer/mod.rs:257](../../../src/renderer/mod.rs),
  [src/renderer/mod.rs:329](../../../src/renderer/mod.rs)).
- If no transparent mode is selected, `set_background_opacity` forces the value to
  `1.0` ([src/renderer/mod.rs:374](../../../src/renderer/mod.rs)). If `Inherit` is
  selected, the code treats the surface as transparent but does not explicitly
  configure the native window system's alpha mode.
- The renderer premultiplies the background color by its opacity and uses it to clear the
  surface ([src/renderer/scene.rs:94](../../../src/renderer/scene.rs),
  [src/renderer/mod.rs:171](../../../src/renderer/mod.rs),
  [src/renderer/mod.rs:194](../../../src/renderer/mod.rs)). Other drawn elements remain
  opaque, as described in the user documentation ([README.md:27](../../../README.md)).

## Diagnosis

The configuration path passes the value through to the renderer. Two cases could explain
an opaque background: the surface exposes no usable alpha mode, which forces opacity to
`1.0`, or `Inherit` is selected without an explicit guarantee that the native system
interprets alpha as expected. The wgpu documentation states that `Inherit` leaves alpha
handling undefined and that the application must configure the native compositing mode
([wgpu 30.0.1 source](https://github.com/gfx-rs/wgpu/blob/v30.0.1/wgpu-types/src/surface.rs)).

The graphics system and launch mode were not specified, so the exact path has not been
confirmed. The diagnosis must be validated on supported Linux environments, particularly
X11 and Wayland with an active compositor.

## Chosen design

Handle surface alpha modes separately instead of treating `Inherit` as equivalent to
`PreMultiplied`:

- For `PreMultiplied`, keep the current premultiplied background color calculation.
- For `Inherit`, explicitly configure the expected alpha mode through the backend's native
  integration before treating the surface as transparent. Verify the result on every
  supported backend.
- If no transparent compositing path is available, keep the rendering opaque and show a
  clear warning. This is a platform or compositor limitation, not a startup-blocking
  error.
- Do not change the `window.background_opacity` key or its meaning: every valid value from
  `0.0` to `1.0` continues to control the default background opacity, including after hot
  reload. Explicit cell backgrounds, the selection, and the cursor remain opaque.

### Alternatives

- **Retained — handle `Inherit` using the backend's native alpha mode.** This supports the
  agreed scope (all supported platforms) without assuming semantics that wgpu leaves
  undefined.
- **Treat `Inherit` as `PreMultiplied` without additional configuration.** Rejected: this
  is the current assumption and does not guarantee that the compositor interprets alpha
  as expected.
- **Accept only `PreMultiplied` and remain opaque with `Inherit`.** Rejected: this would
  make transparency unavailable on backends that only offer `Inherit`.

## Verification

- Unit tests: prefer `PreMultiplied`, recognize `Inherit` as requiring native
  configuration, and retain the opaque fallback with a warning when no usable mode is
  available. Also verify the expected alpha for `0.0`, an intermediate value, and `1.0`.
- Graphical verification on supported Linux backends with an active compositor: test
  several values (including `0.0`, an intermediate value, and `1.0`), then change the
  value through configuration hot reload. Confirm that explicitly drawn backgrounds
  remain opaque.
- Without a compositor, or on a surface that does not support alpha, confirm the
  background remains opaque and a clear warning is shown without preventing startup.

## Implementation task breakdown

- [#92 — Fix transparent terminal background opacity](https://github.com/marmotz/graphene-term/issues/92)
