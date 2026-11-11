# Configuration file and themes: technical design

Product decisions: [overview.md](overview.md).

This feature lands after [tabs-splits-palette](../tabs-splits-palette/overview.md).
Code references below describe the current single-session code; where tabs and panes
will exist by then, "the session" means "every pane" and the settings are applied to
each of them.

## What is hard-coded today

| Setting | Where | Current value |
|---|---|---|
| Font family, size | `Settings::from_env` [app.rs:36-51](../../../src/app.rs) | `GRAPHENE_FONT`, `GRAPHENE_FONT_SIZE`, default 14 ([app.rs:28](../../../src/app.rs)) |
| Padding | `PADDING` [app.rs:32](../../../src/app.rs) | 8 logical px |
| Initial window size | `create_session` [app.rs:358](../../../src/app.rs) | 960x600 logical px |
| Scrollback | `SCROLLBACK_LINES` [terminal.rs:41](../../../src/terminal.rs) | 10 000 |
| Word separators | `SEMANTIC_ESCAPE_CHARS` [selection.rs:15](../../../src/selection.rs), passed to `Term` at [terminal.rs:64](../../../src/terminal.rs) | `` ,│`|"' ()[]{}<>\t `` |
| Palette | `impl Default for Palette` [palette.rs:30](../../../src/palette.rs) | "graphene night" |
| Selection highlight | fg/bg swap [frame.rs:86](../../../src/frame.rs) | - |
| Cursor | `Config::default()` of `Term`: block, no blinking; blinking is never drawn (no blink timer in `app.rs`) | - |
| Shortcuts | `input::action_for` [input.rs:21](../../../src/input.rs) | `match` on key + modifiers |

`alacritty_terminal` 0.26 already supports live changes of its own part:
`Term::set_options(Config)` replaces `scrolling_history`, `semantic_escape_chars` and
`default_cursor_style` (`CursorStyle { shape, blinking }`), resizes the history and
damages the whole grid.

## Alternatives considered

| Topic | Options | Retained |
|---|---|---|
| Parsing | `serde` derive with `deny_unknown_fields` / per-key reading of a `toml::Table` | Per-key reading: one invalid value or unknown key must not discard the rest of the file (overview: "default for whatever is invalid"). `serde` is still used to deserialize each value. |
| Config directory | `dirs` / `etcetera` / `std::env` | `std::env`: `$XDG_CONFIG_HOME`, else `$HOME/.config`. Linux only, two lines, no dependency. |
| File watching | `notify` / mtime polling | `notify` (decided): immediate reaction, no periodic wakeup. |
| Light/dark detection | winit `Window::theme` / `dark-light` / XDG portal through `zbus` | XDG portal (decided). winit 0.30 documents `theme()` and `ThemeChanged` as unsupported on X11 and Wayland. |
| Error display | Banner / text in the grid / desktop notification | Banner drawn over the grid (decided). |
| Initial window size | Columns x lines / pixels | Pixels (decided), with the leftover centring below. |
| Built-in themes storage | Rust constants / embedded TOML files | Embedded TOML files (`include_str!`): same format and same parser as user themes, so every built-in theme also tests the parser. |

## Design

### Dependencies

- `toml` and `serde` (with `derive`): parsing.
- `notify`: file watching. Pin the latest stable release at implementation time (the
  9.x line was still a release candidate when this was written).
- `zbus` (blocking API): XDG desktop portal.

Exact versions are checked with `cargo search` when the task is implemented.

### Files and locations

- `config_dir()`: `$XDG_CONFIG_HOME/graphene`, else `$HOME/.config/graphene`.
- `config.toml` in that directory, user themes in `themes/<name>.toml`.
- New repo folder `themes/` with the built-in themes, embedded with `include_str!`.
- New repo file `config.default.toml`: the fully commented default configuration,
  embedded and written to `config.toml` on first launch when the file is missing (the
  directory is created; a write failure is only logged).

### Configuration model: new module `src/config.rs`

```toml
language = "auto"            # "auto" (follows LANG), "en", "fr"

[font]
family = ""                  # empty: system monospace
size = 14.0

[window]
width = 960                  # logical pixels
height = 600
padding = 8                  # minimum, logical pixels

[scrollback]
lines = 10000

[cursor]
shape = "block"              # "block", "beam", "underline"
blinking = false

[selection]
word_separators = ",│`|\"' ()[]{}<>\t"

[tabs]
position = "top"             # "top", "bottom", "left", "right"

[theme]
name = "graphene-night"      # single theme
# light = "graphene-day"     # light + dark: follow the system appearance
# dark = "graphene-night"

[keybindings]
"Ctrl+Shift+C" = "copy"
"Shift+PageUp" = "none"      # disables a default shortcut: the key goes to the shell
```

- `Config` is a plain struct with `Default` holding today's values, so a missing file
  or an empty file behaves exactly like today.
- `Config::parse(text: &str, previous: &Config) -> (Config, Vec<ConfigError>)`: parses
  the TOML, then reads each known key with a small helper
  (`fn field<T: DeserializeOwned>(table, path, errors) -> Option<T>`). A key that is
  missing keeps the default; a key with an invalid value keeps the value of
  `previous` and records an error; an unknown key records an error. At startup
  `previous` is `Config::default()`, on a hot reload it is the configuration in
  effect, which gives "the last valid configuration stays in effect" key by key. A
  TOML syntax error returns `previous` unchanged with one error.
- `ConfigError { file: PathBuf, line: Option<usize>, message: String }`: the line
  comes from the `toml` span of the key.
- Validation: `font.size` clamped to the existing `MIN_FONT_SIZE..=MAX_FONT_SIZE`
  ([app.rs:29-30](../../../src/app.rs)), `scrollback.lines` capped (100 000),
  `window.*` and `padding` non-negative; an out-of-range value is an error.
- `Settings` and `Settings::from_env` ([app.rs:36-51](../../../src/app.rs)) are removed,
  as are the `GRAPHENE_FONT*` sections of `README.md` and `docs/development.md`.

### Themes: `src/palette.rs` and new module `src/theme.rs`

Theme file format:

```toml
[colors]
foreground = "#d7dae0"
background = "#14161b"
cursor = "#82aaff"
cursor_text = "#14161b"
selection_background = "#3a4252"
selection_foreground = ""    # empty: keep the cell's own foreground

[colors.normal]  # black, red, green, yellow, blue, magenta, cyan, white
[colors.bright]  # same keys

[ui]             # tab bar, active/inactive tab, pane border, palette background,
                 # palette text, palette match highlight, banner background/text
```

- `Palette` ([palette.rs:20](../../../src/palette.rs)) gains `selection_background`,
  `selection_foreground: Option<Rgb>` and a `ui: UiColors` struct. `build_indexed` is
  reused unchanged to derive the 256 colors from the 16 ANSI ones.
- A theme is parsed with the same per-key approach: a missing or invalid color falls
  back to the same key of `graphene-night`, so a partial user theme still works.
- Resolution of a theme name: `themes/<name>.toml` in the config directory first, then
  the built-in themes. A user file therefore overrides a built-in theme of the same
  name. Unknown name: error + `graphene-night`.
- Built-in themes: `graphene-night` (today's `Palette::default` values),
  `graphene-day` (new light theme), `solarized-dark`, `solarized-light`,
  `gruvbox-dark`, `gruvbox-light`, `catppuccin-mocha`, `catppuccin-latte`. All three
  upstream projects are MIT licensed: each file carries the upstream copyright and
  licence notice in a header comment.
- `Palette::default()` becomes the parse of the embedded `graphene-night.toml`, so the
  existing palette tests keep running against the same values.

### Selection color: `src/frame.rs`

The swap at [frame.rs:86](../../../src/frame.rs) becomes
`bg = palette.selection_background; fg = palette.selection_foreground.unwrap_or(fg)`.
The block cursor keeps priority over the selection, as today.

### Light/dark: new module `src/appearance.rs`

- A thread opens a blocking `zbus` connection, calls
  `org.freedesktop.portal.Settings.ReadOne("org.freedesktop.appearance",
  "color-scheme")` (0 no preference, 1 dark, 2 light) and listens to the
  `SettingChanged` signal for that key.
- Each value is sent to the event loop as `UserEvent::Appearance(Appearance)`.
- No portal, or "no preference": `dark` is used. The thread is only started when the
  configuration sets `light`/`dark`.

### Hot reload: new module `src/watcher.rs`

- A `notify` watcher on the config directory (not on the file: editors save through a
  rename, which replaces the watched inode), non-recursive plus `themes/`.
- Events touching `config.toml` or the active theme files are debounced (~100 ms) and
  sent as `UserEvent::ConfigChanged`.
- `UserEvent` ([terminal.rs:19](../../../src/terminal.rs)) gains `ConfigChanged` and
  `Appearance(Appearance)`; `App::user_event` ([app.rs:418](../../../src/app.rs))
  dispatches them.
- Missing config directory at startup: it is created by the first-launch write; if
  that fails, the watcher is not started and a warning is logged.

### Applying a configuration: `src/app.rs`

`App` keeps the effective `Config`, its errors and the current `Appearance`.
`Session::apply_config(&Config, &Palette)`:

- `term.set_options(...)` with `scrolling_history`, `semantic_escape_chars` and
  `default_cursor_style`. `Terminal::spawn` ([terminal.rs:49](../../../src/terminal.rs))
  takes these values instead of the constants, which are removed
  (`SEMANTIC_ESCAPE_CHARS` stays as the default value in `config.rs`).
- Font family: new `Renderer::set_font(family, size)` / `TextGrid::set_font`, next to
  `set_font_size` ([text.rs:97](../../../src/renderer/text.rs)). Font size: when
  `font.size` changed, it is applied and the current zoom is reset; otherwise the zoom
  in progress is kept. `Action::FontSizeReset` resets to the configured size.
- Padding: stored in the session instead of `PADDING`, then `relayout()`.
- Palette and keybindings replaced, `request_redraw()`.
- `window.width/height` only affect windows created after the change.

### Grid centring: `src/layout.rs`

`GridLayout::new` ([layout.rs:26](../../../src/layout.rs)) computes the cell count as
today with `padding` as a minimum, then stores an `origin: (f32, f32)` equal to
`padding + leftover / 2` on each axis. `cell_origin`, `point_at` and
`vertical_overflow` ([layout.rs:37-67](../../../src/layout.rs)) use `origin` instead
of `padding`. Panes built by the tabs feature use the same `GridLayout`, so they are
centred the same way.

### Cursor blinking: `src/app.rs`, `src/frame.rs`

- Blinking is active when `term.cursor_style().blinking` is true (config default or
  DECSCUSR from the application).
- `App::about_to_wait` ([app.rs:413](../../../src/app.rs)) already uses
  `ControlFlow::WaitUntil` for auto-scroll: it takes the earliest of the auto-scroll
  deadline and the next blink toggle (500 ms).
- `Frame::capture` takes a `cursor_visible` flag; a key press or a paste resets the
  phase to visible, so the cursor never disappears while typing. An unfocused window
  does not blink (hollow block, as today).

### Keybindings: `src/input.rs`

- `action_for` ([input.rs:21](../../../src/input.rs)) is replaced by a `Keybindings`
  table: `HashMap<(KeyId, Mods), Action>`, built from the defaults then the
  `[keybindings]` section. `"none"` removes the entry.
- Key syntax: modifiers `Ctrl`, `Shift`, `Alt`, `Super` joined with `+`, then a winit
  `NamedKey` name (`PageUp`, `F5`, `Insert`...) or a single character. Letters are
  matched case-insensitively. For non-letter characters `Shift` is ignored, because
  the layout already used it to produce the character (`Ctrl+=` and `Ctrl++` today).
  Digits bound with `Alt` (`Alt+1` ... `Alt+9`, tab selection) are matched on the
  physical digit row, so they work on AZERTY.
- Action names are the snake_case of `Action` ([input.rs:8](../../../src/input.rs)):
  `copy`, `paste`, `scroll_page_up`, ..., `font_size_reset`, plus the actions added by
  the tabs feature. The command palette shows the shortcut read from this table.
- The default table reproduces the current `match` exactly.

### Error banner: `src/renderer/`

- `Frame` gains `banner: Option<String>`, built from the first error ("config.toml:12:
  unknown key `font.sise`", plus "(+N more)" when there are several).
- The renderer draws it as a rect in `ui.banner_background` and one line of text at the
  top of the window, over the grid (the grid does not move).
- Hidden when a reload yields no error, or on a click on the banner. All errors are
  also logged with `log::warn!`.

### Language

`language = "auto"` keeps the `LANG` detection of the tabs feature; `"en"` / `"fr"`
force it. Applied on hot reload like the other settings.

### Tab bar position

`tabs.position` is passed to the tab bar layout of the tabs feature, which places the
bar on the matching edge and gives the remaining area to the panes.

## Consequences checked against the code

- `GRAPHENE_FONT` and `GRAPHENE_FONT_SIZE` stop working: removed from `README.md`
  (lines 14-15) and `docs/development.md` (lines 26-27), which document the config file
  instead.
- `selected_cells_swap_colors` and `selected_wide_char_is_fully_highlighted`
  ([frame.rs](../../../src/frame.rs) tests) assert the fg/bg swap: they are updated to
  the theme selection colors.
- The test `escape_chars_are_alacritty_defaults_without_colon`
  ([selection.rs:322](../../../src/selection.rs)) compares
  `SEMANTIC_ESCAPE_CHARS` with Alacritty's list: it moves with the default value to
  `config.rs`.
- `Term::set_options` sends a `Title`/`ResetTitle` event: harmless, it re-applies the
  current title.
- `Term::set_options` with a smaller `scrolling_history` drops the oldest lines: the
  expected behaviour of lowering the setting.
- Centring changes `cell_origin` for every frame: the `layout.rs` tests that assume
  `padding` as the origin are updated.

## Tests

- `config.rs`: empty file = `Config::default()`; every key read; unknown key, invalid
  value and syntax error each produce an error with the right line and keep the
  previous value; the uncommented `config.default.toml` parses to `Config::default()`
  with no error (keeps the commented file in sync with the code).
- `theme.rs`: every built-in theme parses with no error; `graphene-night.toml` equals
  today's palette; a partial theme falls back to `graphene-night`; user theme overrides
  a built-in name; light/dark selection by `Appearance`.
- `input.rs`: the existing `app_actions` test runs against the default table; remap,
  `"none"`, invalid key syntax, `Shift` handling for letters and symbols.
- `layout.rs`: leftover split on both axes; `point_at` / `cell_origin` stay inverse.
- `frame.rs`: selection colors with and without `selection_foreground`; blink phase
  hides the cursor; banner propagation.
- Portal and `notify` threads are thin adapters around tested pure functions
  (color-scheme value to `Appearance`, path filter and debounce); checked manually:
  edit `config.toml` in vim and with `sed -i`, switch the system appearance.

## Découpage en tâches d'implémentation

| Issue | Task | Depends on |
|---|---|---|
| [#13](https://github.com/marmotz/graphene-term/issues/13) | Centre the grid with the leftover pixels | - |
| [#14](https://github.com/marmotz/graphene-term/issues/14) | Configuration file, model and startup settings | - |
| [#15](https://github.com/marmotz/graphene-term/issues/15) | Themes, built-in themes and selection color | #14 |
| [#16](https://github.com/marmotz/graphene-term/issues/16) | Configurable keybindings | #14 |
| [#17](https://github.com/marmotz/graphene-term/issues/17) | Cursor blinking | #14 |
| [#18](https://github.com/marmotz/graphene-term/issues/18) | Error banner | #14, #15 |
| [#19](https://github.com/marmotz/graphene-term/issues/19) | Hot reload of the configuration and themes | #14, #15, #16 |
| [#20](https://github.com/marmotz/graphene-term/issues/20) | Follow the system light/dark appearance | #15, #19 |
| [#21](https://github.com/marmotz/graphene-term/issues/21) | Language, tab bar position and UI colors | #13, #15, #19, feature tabs-splits-palette |
