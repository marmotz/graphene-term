# Settings screen — technical design

Product decisions: [overview.md](overview.md).

## Current state

- **One window only.** `App` holds a single `session: Option<Session>`
  ([src/app.rs:1684](../../../src/app.rs)); `App::window_event` ignores its `WindowId`
  ([src/app.rs:1891](../../../src/app.rs)) and `App::resumed` creates the one session.
  `Session::close` exits the event loop with the window
  ([src/app.rs:410](../../../src/app.rs)). `detach_pane` moves a pane to a new tab, not
  to a new window.
- **One GPU context per window.** `Renderer::new`
  ([src/renderer/mod.rs:296](../../../src/renderer/mod.rs)) creates the
  `wgpu::Instance`, the surface, the adapter, the device and the queue together, and
  keeps the instance to recreate a lost surface
  ([src/renderer/mod.rs:440](../../../src/renderer/mod.rs)).
- **No widget toolkit.** The chrome (tab bar, command palette, close dialog) is laid out
  by pure modules in `src/chrome/` and drawn as rects and interface text by the
  renderer. There is no text field with a caret, no list or toggle.
- **Configuration read-only.** `Config::parse` ([src/config.rs:385](../../../src/config.rs))
  reads the file with `toml::de::DeTable` (spans, no comments kept). Nothing writes
  `config.toml` except `load_from`, which writes `DEFAULT_CONFIG` when the file is
  missing ([src/config.rs:117](../../../src/config.rs)). Every option of
  [config.default.toml](../../../config.default.toml) is commented out, including the
  section headers (`# [font]`).
- **Errors have no key.** `ConfigError` ([src/config.rs:142](../../../src/config.rs))
  carries a file, a line and a message. `Reader::set` knows the key path (`font.size`)
  but only writes it into the message.
- **Hot reload.** The `config-watcher` thread ([src/watcher.rs](../../../src/watcher.rs))
  watches the directory, so a write through a rename is seen; files other than
  `config.toml` and `themes/*.toml` are ignored (`watched_file`). `App::reload_config`
  ([src/app.rs:1786](../../../src/app.rs)) parses with the applied configuration as
  `previous` and applies the result to the session.
- **Shortcuts.** `Shortcut::character` ([src/keybindings.rs](../../../src/keybindings.rs))
  drops `Shift` for any character that is not a letter, since the layout already used
  it to produce the character. `Ctrl+Shift+,` would therefore be stored as `Ctrl+,`, and
  never match: with Shift held the key event carries `<` (QWERTY) or `?` (AZERTY).

## Decisions

### Shortcut and action

- New `Action::OpenSettings`, id `open_settings`, label `Msg::OpenSettings`
  ("Settings" / "Paramètres"), added to `Action::ALL` (35 entries) and so to the command
  palette ([src/input.rs:56](../../../src/input.rs)).
- Default shortcut **`Ctrl+,`** in `DEFAULTS`
  ([src/keybindings.rs:95](../../../src/keybindings.rs)), replacing the `Ctrl+Shift+,`
  of the overview for the reason above. It can be rebound or set to `"none"` like any
  other.
- The actions list of `config.default.toml` and the shortcut table of the README gain
  the new entry.

### Window: same process, shared GPU

Alternatives:

- *Separate process* (`graphene --settings`): decoupled, the file is the only channel.
  Rejected: a second GPU initialisation (the slow step, see `perf` marks), and a lock
  file to keep one settings window.
- **Same process, second winit window** (retained): one settings window at most, the
  settings window reads the configuration, the theme and the language already loaded
  by `App`.

Changes:

- `renderer::Gpu { instance, adapter, device, queue }` is extracted from
  `Renderer::new`. It is created with the first window (the `first_success` loop over
  `BACKEND_ATTEMPTS` needs a surface to pick a compatible adapter), kept in `App` as an
  `Arc<Gpu>`, and passed to every window. `Renderer` keeps its own surface, surface
  configuration and layers, and recreates a lost surface with `gpu.instance`.
- `App` gains `settings: Option<SettingsWindow>`. `window_event` routes by `WindowId`:
  the settings window's id goes to `SettingsWindow::window_event`, any other to the
  session as today.
- `Session::run_action` cannot create a window (the window is owned by `App`):
  `Action::OpenSettings` sets `Session.settings_requested`, read by `App` after each
  window or user event, which calls `App::open_settings(event_loop)`. When the window
  is already open it is focused (`Window::focus_window`) instead.
- Closing the settings window drops it. Closing the terminal window exits the event
  loop as today, which closes the settings window too.

### Widgets: egui

Alternatives:

- *Hand-made widgets* in `src/chrome/`: consistent look, no dependency, but text
  fields with caret and selection, focus, scrolling, combo boxes and key capture are a
  lot of new code.
- **egui** (retained): `egui` 0.37.0, `egui-winit` 0.37.0 and `egui-wgpu` 0.37.0.
  `egui-wgpu` 0.37.0 requires `wgpu` 30.0 and `egui-winit` 0.37.0 `winit` 0.30.13, the
  versions already used. MSRV 1.95, below the stable toolchain of the repo.

Dependencies:

```toml
egui = "0.37.0"
egui-winit = { version = "0.37.0", default-features = false, features = ["clipboard", "wayland", "x11"] }
egui-wgpu = { version = "0.37.0", default-features = false }
```

`links` (webbrowser) is left out. `egui-wgpu` without default features does not turn on
`wgpu/default` or `wgpu/webgl`. The window is driven by `egui_winit::State` (events
in, platform output out) and painted by `egui_wgpu::Renderer::new(&device, format,
RendererOptions)` on its own surface from `gpu.instance`.

Look: egui's bundled fonts; the `Visuals` are built from the active theme (`ui.*`
colors of [src/palette.rs](../../../src/palette.rs): background, text, accent,
borders), dark or light from the theme background, and rebuilt on each reload.

### Layout

- Left sidebar: Font, Window, Scrollback, Cursor, Selection, Tabs, Language, Theme,
  Keyboard shortcuts, Updates. The `updates` section of the file is covered too, since
  the screen covers every setting.
- Right panel: the fields of the selected section, each with its label, its widget
  and, under it, its error when it has one.
- Labels are translated through `i18n::Msg` (new variants, English and French), in
  the resolved `Lang` of the configuration.

Widgets per setting:

| Setting | Widget |
|---|---|
| `language` | combo: auto, en, fr |
| `font.family` | combo of the monospace families (`fontdb::FaceInfo::monospaced` from the cosmic-text font database), plus "System monospace" (empty) |
| `font.size` | number, 6 to 72 |
| `window.startup_mode` | combo: default, maximized, remember |
| `window.width`, `window.height` | number, > 0 |
| `window.padding` | number, >= 0 |
| `window.background_opacity` | slider 0.0 to 1.0 |
| `scrollback.lines` | number, 0 to 100000 |
| `cursor.shape` | combo: block, beam, underline |
| `cursor.blinking` | toggle |
| `selection.word_separators` | text |
| `tabs.position` | combo: top, bottom, left, right |
| `theme` | toggle "Follow the system appearance"; off: one combo (`name`); on: two combos (`light`, `dark`). Choices: `theme::BUILT_IN` names and the `themes/*.toml` stems, user themes first |
| `keybindings` | see below |
| `updates.check` | toggle |
| `updates.interval` | combo: `UpdateInterval::VALUES` |

Ranges and choices come from the constants of `config.rs` (`MIN_FONT_SIZE`,
`MAX_SCROLLBACK_LINES`, `UpdateInterval::VALUES`...), not duplicated.

### Keyboard shortcuts section

- One row per `Action::ALL` entry: its label, its effective shortcuts (defaults with
  the `[keybindings]` entries applied, as `Keybindings::new` does), an "Add" button
  (capture a shortcut) and a "Remove" button on each shortcut.
- Removing a configured shortcut deletes its `[keybindings]` entry; removing a default
  one writes `"<keys>" = "none"`.
- Capture: the next key press in the settings window (winit `KeyEvent`, not egui's
  event, to reuse the matching rules) becomes a shortcut through a new pure
  `Shortcut::from_event(key, physical, mods)` in `keybindings.rs`, the inverse of
  `Keybindings::action` (letter by character else physical key, Shift dropped for
  other characters, Alt+digit on the digit row). `Escape` alone cancels. Its label
  (`Shortcut::label`) is the key written to the file.
- A captured shortcut already bound to another action shows "Already used by <action>"
  with "Replace" and "Cancel"; "Replace" writes the new binding, which overrides the
  other one.

### When a change is written

The overview says a change applies immediately. Per widget:

- combo, toggle, keybinding: written on change;
- text and number fields: written on `Enter` or when the field loses focus, so typing
  `14` does not write `1` first;
- slider: written when the drag ends (and on each keyboard step), not at every frame of
  the drag.

### Writing `config.toml`: `toml_edit`

Alternatives:

- *Serialize `Config` again*: loses the comments and the order. Rejected by the
  overview.
- *Uncomment the matching default line* (`# size = 14.0` under `# [font]`): keeps the
  value next to its documentation, but means parsing comments by hand and handling a
  section header that is itself commented. Rejected: fragile.
- **`toml_edit`** (retained): `toml_edit = "0.25.19"`, already in `Cargo.lock` (pulled
  by `proc-macro-crate`), same TOML 1.1 parser family as `toml` 1.2. `DocumentMut`
  keeps comments, whitespace and key order and only changes the edited item.

New pure module `src/config_edit.rs`:

- `ConfigEdit::parse(text) -> Result<ConfigEdit, ConfigError>` over `DocumentMut`.
- `set(path: &[&str], value)`: sets the value, creating the section table when missing
  (appended at the end of the document, after the commented defaults); keeps the
  decor (comment after the value) of an existing key.
- `remove(path)`: removes a key, and the `[keybindings]` table when it becomes empty
  and has no comment.
- `to_string()`.

Writing (`config_edit::write`, thin): the file is read again before each change so hand
edits made meanwhile are kept; a missing file starts from `DEFAULT_CONFIG`; the result
is written to `config.toml.tmp` in the same directory, then renamed over
`config.toml`. The watcher ignores the temporary file and sees the rename, so the hot
reload path applies the change to the terminal window, as for a hand edit; the settings
window is refreshed by the same `UserEvent::ConfigChanged`.

### Errors

- `ConfigError` gains `key: Option<String>`: the key path (`font.size`,
  `keybindings."Ctrl+X"`), set by `Reader::set` and by the unknown-key errors, `None`
  for a syntax or read error. The banner text does not change.
- The settings model maps each error to its field by key and shows it under the field.
  Writing a valid value removes the error on the next reload.
- An unknown key has no field: it is listed at the top of the window, with its line.
  The overview is corrected accordingly.
- Theme file errors are shown in the Theme section.
- Syntax error (`Config::parse` fails on `DeTable::parse`, or `ConfigEdit::parse` fails):
  the window shows the error, the file path and a note that the file must be fixed by
  hand; every widget is disabled. The next `ConfigChanged` with a readable file
  unlocks it.

### Module layout

```
src/
  config_edit.rs        DocumentMut wrapper: set, remove, write through a rename (pure but write)
  settings/
    mod.rs              SettingsWindow: winit window, surface, egui_winit::State, egui_wgpu::Renderer
    model.rs            SettingsModel: sections, field values from Config, errors by key,
                        locked state, edits -> (path, value) (pure, unit tested)
    view.rs             egui drawing of the model (thin)
```

`model.rs` holds the logic (what a field shows, what an edit writes, which error goes
where, shortcut rows and conflicts) and is unit tested like `config` and `chrome`.
`view.rs` and `mod.rs` stay thin, as the GPU and window code today.

## Consequences

- `Renderer::new` changes signature (takes `&Gpu`); `Headless`
  ([src/renderer/mod.rs:465](../../../src/renderer/mod.rs)) and `benches/render.rs`
  are checked against it.
- `ConfigError` gains a field: every construction site in `config.rs` and `theme.rs`
  is updated, the banner is unchanged.
- `Action::ALL` grows to 35: the command palette and `input.rs` tests that count or
  list actions are updated.
- Binary size and build time grow with egui; to measure in the first task.
- `docs/architecture.md`: stack table (egui, toml_edit), modules list, a "Settings
  window" section; README: shortcut and a short "Settings" paragraph.

## Découpage en tâches d'implémentation

1. [#85](https://github.com/marmotz/graphene-term/issues/85) Key path on configuration errors.
2. [#86](https://github.com/marmotz/graphene-term/issues/86) Write `config.toml` preserving comments (`toml_edit`).
3. [#87](https://github.com/marmotz/graphene-term/issues/87) Shared GPU context and window routing.
4. [#88](https://github.com/marmotz/graphene-term/issues/88) Settings window with egui, action and `Ctrl+,` (after #87).
5. [#89](https://github.com/marmotz/graphene-term/issues/89) Settings fields, immediate writes and errors (after #85, #86, #88).
6. [#90](https://github.com/marmotz/graphene-term/issues/90) Keyboard shortcuts section (after #89).

#85, #86 and #87 are independent and can be done in any order.
