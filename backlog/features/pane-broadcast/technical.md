# Broadcast input to several panes — Technical Design

Product decisions: [overview.md](overview.md).

## Current State

### Default shortcuts

- The built-in table is `DEFAULTS` in
  [src/keybindings.rs:93-155](../../../src/keybindings.rs): `Ctrl+Shift+N` / `B` are
  `NextPane` / `PreviousPane`, `Ctrl+Shift+Z` is `ToggleZoom`, `Ctrl+Tab` /
  `Ctrl+Shift+Tab` are `NextTab` / `PreviousTab` next to `Ctrl+PageDown` / `PageUp`.
  `Ctrl+Shift+X` is free and is even used as an example of an unbound letter.
- Tests pin these keys: `pane_actions` (N, B, Z,
  [src/keybindings.rs:677-704](../../../src/keybindings.rs)), `tab_actions` (Ctrl+Tab,
  [src/keybindings.rs:714-726](../../../src/keybindings.rs)),
  `unmapped_ctrl_shift_letters_stay_with_the_shell` (A, D, G, X,
  [src/keybindings.rs:768-774](../../../src/keybindings.rs)), and the settings model
  conflict test, which expects `["Ctrl+Tab", "Ctrl+PageDown", "Ctrl+Shift+T"]` for
  `NextTab` ([src/settings/model.rs:1240-1257](../../../src/settings/model.rs)).
- The README shortcut tables list them
  ([README.md:150-162](../../../README.md)).

### Actions

- `Action` and `Action::ALL` (the palette order, a fixed-size array) are in
  [src/input.rs:9-97](../../../src/input.rs); `id`, `from_id` and `label` map each
  variant to its snake_case name and its `Msg`
  ([src/input.rs:99-200](../../../src/input.rs)). Every `Msg` needs an English and a
  French text, an index and an entry in `all_messages` (`VARIANT_COUNT`,
  [src/i18n.rs:590-700](../../../src/i18n.rs)).
- `run_action` runs the session actions
  ([src/app.rs:1790](../../../src/app.rs)); `run_pane_action` lists every variant
  exhaustively ([src/app.rs:2269-2306](../../../src/app.rs)).
- The palette hides actions that cannot run: `available`
  ([src/chrome/command_palette.rs:158-190](../../../src/chrome/command_palette.rs)),
  `several_panes` for zoom, equalise and detach.

### Keyboard and paste routing

- `key_press` ([src/app.rs:1880-1966](../../../src/app.rs)): overlay first, then a
  dragged pane swallows keys, then the keybinding lookup. On the **active pane only**:
  at the prompt (`Pane::rich_mode`), an editor command (`input_editor::prompt_command`)
  wins over the shortcuts and is run (`run_editor_command`, or `replace_selection`);
  otherwise a shortcut runs its action; otherwise the key replaces an input selection,
  or is encoded by `input::key_to_bytes` with the pane's own `TermMode` and written by
  `Pane::type_bytes`.
- Both rich mode and the key encoding depend on the pane's state (prompt or running
  program, cursor-key mode...), so the same key gives different bytes in two panes.
- Pastes go through `Session::paste` (clipboard,
  [src/app.rs:1197-1205](../../../src/app.rs)) and `paste_primary` (middle click,
  [src/app.rs:1766-1788](../../../src/app.rs)); both call `Pane::send_paste` on the
  active pane, which brackets with the pane's own mode
  ([src/pane.rs:405-410](../../../src/pane.rs)). `Action::Paste` (`Ctrl+Shift+V`,
  `Shift+Insert`, `Paste`, palette, context menu) ends in `Session::paste`.
- These five calls (`type_bytes`, `run_editor_command`, `replace_selection` twice,
  `send_paste` twice) are the only places that write typed input to a pane.

### Mouse routing

- `mouse_press` ([src/app.rs:1268-1404](../../../src/app.rs)): overlay, banner, tab
  bar, divider, pane bar (`pane_bar_press`), then the pane content. A press in an
  inactive pane **focuses it first**. With `Ctrl` on a link where the application
  captures the mouse, the press is Graphene's (`LinkPress`); otherwise a captured press
  is reported to the application (`app_mouse.press`); otherwise a left press starts a
  selection (`Pane::left_press`, a rectangular one with `Ctrl`) and records a
  `LinkPress` when on a link.
- A link opens on release when `links::click_opens` says so: a simple click outside
  captured areas (with or without `Ctrl`), only `Ctrl` + click inside
  ([src/links.rs:218-233](../../../src/links.rs)).
- `pane_bar_press` ([src/app.rs:1407-1443](../../../src/app.rs)) focuses the pane and
  arms a `Drag::Pane`.
- `release_gesture` ends the drag of the press
  ([src/app.rs:1459-1498](../../../src/app.rs)).

### Workspace

- `Tab { tree, active, zoomed }`
  ([src/workspace/mod.rs:85-129](../../../src/workspace/mod.rs)); the module is generic
  over the pane type and unit-tested without shells.
- A pane leaves a tab through `close_pane`
  ([src/workspace/mod.rs:247-271](../../../src/workspace/mod.rs)), `take_out` (used by
  `detach`, `move_pane` across tabs and `move_pane_to_new_tab`,
  [src/workspace/mod.rs:474-485](../../../src/workspace/mod.rs)), `swap_panes` across
  tabs ([src/workspace/mod.rs:409-431](../../../src/workspace/mod.rs)), and
  `move_pane_to_new_tab` of a single-pane tab, which moves the whole tab
  ([src/workspace/mod.rs:457-472](../../../src/workspace/mod.rs)).
- `move_pane` **rebuilds** the target tab (`self.tabs[index] = Tab { tree, active,
  zoomed }`, [src/workspace/mod.rs:342-369](../../../src/workspace/mod.rs)), also for a
  move inside one tab: a new field would be reset there unless carried over.

### Pane bar drawing and colors

- `PaneBarLayout` carries the title, the close button and `active`
  ([src/chrome/pane_bar.rs:52-90](../../../src/chrome/pane_bar.rs)); `Session::pane_bars`
  builds them ([src/app.rs:313-327](../../../src/app.rs)).
- `pane_bar_colors` gives the accent (`palette_match`) for the active bar and a muted
  accent for the others ([src/chrome/scene.rs:164-180](../../../src/chrome/scene.rs));
  `pane_bar` draws background, title and `×`
  ([src/chrome/scene.rs:408-433](../../../src/chrome/scene.rs)).
- Interface colors are `UiColors` ([src/palette.rs:29-43](../../../src/palette.rs)),
  read from the `[ui]` table of a theme by `ui_slot`
  ([src/theme.rs:233-248](../../../src/theme.rs)). Tests check that every built-in
  theme sets every color and keeps the pane bars readable (contrast >= 4.5,
  [src/theme.rs:446-470](../../../src/theme.rs)). The 8 built-in themes are in
  [themes/](../../../themes).

## Design

### 1. Default shortcut remapping

`DEFAULTS` becomes:

| Action          | Keys                         |
|-----------------|------------------------------|
| `NextPane`      | `Ctrl+Tab`                   |
| `PreviousPane`  | `Ctrl+Shift+Tab`             |
| `NextTab`       | `Ctrl+PageDown`              |
| `PreviousTab`   | `Ctrl+PageUp`                |
| `ToggleZoom`    | `Ctrl+Shift+X`               |
| `ToggleBroadcastAll` (new) | `Ctrl+Shift+B`    |

`Ctrl+Shift+N` and `Ctrl+Shift+Z` are no longer bound and go back to the shell. No
migration: there are no other users, and a `config.toml` that still binds the old keys
keeps working since user bindings are applied on top of the defaults.

Tests to update: `pane_actions`, `tab_actions`, the unmapped letters test (X replaced
by N and Z), the AZERTY capture test (still `Ctrl+Shift+Z`, it only checks the label of
a captured key, unaffected) and the settings model conflict test (`NextTab` labels become
`["Ctrl+PageDown", "Ctrl+Shift+T"]`). README tables updated.

### 2. Broadcast set in the workspace (pure)

`Tab` gains `broadcast: BTreeSet<PaneId>` (empty by default). A `BTreeSet` gives a
stable order for writes and cheap membership tests; the set is tiny.

New `Workspace` methods, unit-tested in `src/workspace/mod.rs`:

- `toggle_broadcast(id) -> bool`: adds `id` to the set of its own tab, or removes it.
  `false` for an unknown pane.
- `toggle_broadcast_all() -> bool`: on the active tab, an empty set becomes every leaf
  (`tree.leaves()`), a non-empty one is emptied. `false` without active tab.
- `is_broadcast(id) -> bool` and `broadcast_count(index) -> usize`, for the drawing.
- `input_targets() -> Vec<PaneId>`: the panes typed input goes to. The active pane
  alone when it is not in its tab's set; else the whole set, active pane first. Zoom
  does not change it (product decision: broadcast continues while zoomed).

Upkeep, so that the set only ever holds panes of its own tab:

- `close_pane`: removes `id` from its tab's set.
- `take_out`: removes `id` from the source tab's set (covers `detach`, cross-tab
  `move_pane`, `move_pane_to_new_tab` of a pane among others).
- `swap_panes` across tabs: removes `a` and `b` from both sets.
- `move_pane_to_new_tab` of a single-pane tab: clears the moved tab's set.
- `move_pane`: carries the target tab's set over into the rebuilt `Tab`; a cross-tab
  move then lands without broadcast since `id` was never in the target set.
- New panes (`split`, `new_tab`) are never added.

Alternative considered: one set on the `Workspace`, filtered by tab. Rejected: every
tab keeps its own set by decision, and a per-tab field makes the moves above local.

### 3. Action `ToggleBroadcastAll`

- New `Action::ToggleBroadcastAll` (id `toggle_broadcast_all`), in `Action::ALL` after
  `ToggleZoom`, with `Msg::ToggleBroadcastAll`: "Broadcast input to all panes / stop" /
  "Diffuser la saisie à tous les panneaux / arrêter" (exact wording settled at
  implementation, in both languages).
- `run_action` calls `workspace.toggle_broadcast_all()` and redraws;
  `run_pane_action` lists it among the non-pane actions.
- `available`: only with several panes (`several_panes`), like zoom: a single-pane tab
  has nothing to broadcast to. The key is still swallowed there, as for zoom.

### 4. Keyboard and paste fan-out

`key_press` keeps its structure; only the delivery changes.

1. The **active pane** decides, as today, whether the key is a Graphene shortcut
   (focus, actions, editor commands win in the same order). A shortcut runs once, never
   broadcast.
2. When the key is input for the active pane, it is delivered to every pane of
   `workspace.input_targets()`, each one resolving it **in its own state**, through a
   new `Session::deliver_key(pane_id, event, modifiers)` that holds the current
   per-pane part of `key_press`: editor command at its prompt, else replacement of its
   input selection, else `key_to_bytes` with its own `TermMode` then `type_bytes`.
3. In another pane of the set, a key that is a Graphene shortcut there but not in the
   active pane (e.g. `Shift+Home`: editor command at the active pane's prompt,
   `ScrollToTop` in a pane running a program) is skipped for that pane: shortcuts are
   never broadcast.

`Session::paste` and `paste_primary` read the text once, then call `send_paste` on each
pane of `input_targets()`, each bracketing with its own mode.

The per-pane resolution is factored into a pure function in `input_editor` / `input`
where possible (key + modifiers + rich flag + mode → `Delivery::{Editor(cmd), Replace,
Bytes, Shortcut}`), so the skip rule is unit-tested without a PTY.

Alternative considered: encode the key once in the active pane and write the same bytes
everywhere. Rejected: wrong bytes for a pane at a different prompt state or in another
cursor-key mode, and it would bypass the rich editor of the other panes.

### 5. Ctrl+click toggling

A new gesture, `Drag::BroadcastClick { pane, press, cell }`, replaces the current
handling of a left press with `Ctrl` in a pane:

- **Pane bar**: in `pane_bar_press`, a left press with `Ctrl` on the title toggles the
  pane at once (`toggle_broadcast`), no focus change, no pane drag. `Ctrl` on `×` still
  closes.
- **Content, on a link**: unchanged, the link wins (`LinkPress` as today, opened on
  release by `click_opens`).
- **Content, elsewhere**: the press is Graphene's even where the application captures
  the mouse (never reported to it), and the pane is **not** focused. The gesture is
  recorded; on release on the same cell without movement past `PANE_DRAG_THRESHOLD`,
  the pane is toggled.
- **Ctrl+drag outside captured areas** keeps the rectangular selection: once the
  pointer passes the threshold, the pane is focused (as a press does today) and
  `Pane::left_press` is called with the recorded press position and modifiers, then
  the gesture becomes a `Drag::Selection`. In captured areas, a `Ctrl` drag does
  nothing.
- A closed pane drops a pending `BroadcastClick` on it (`close_pane`, as for
  `Drag::Selection`).

Alternative considered: toggle at press time in the content. Rejected: it cannot be
told apart from the start of a rectangular selection.

### 6. Visual indicator

- `UiColors` gains `pane_broadcast` (theme key `ui.pane_broadcast`), set in the 8
  built-in themes from each theme's warm accent (its ANSI yellow or orange family),
  chosen so the existing contrast test passes.
- `PaneBarLayout` gains `broadcast: Option<usize>`: the size of the tab's set when the
  pane is in it. `pane_bar_colors(colors, active, broadcast)`:
  - in the set and active: `pane_broadcast` background;
  - in the set and inactive: `pane_broadcast` blended into the tab bar background like
    the muted accent (`INACTIVE_PANE_BAR`);
  - otherwise unchanged.
- The bar shows a broadcast badge before the title: a glyph (`⇶`, checked for coverage
  in the UI font at implementation, `»` otherwise) followed by the count of panes in the
  set (`⇶ 3`). The title starts after it and is truncated in the remaining width.
- While zoomed, the only shown bar carries the same badge, so the user sees that input
  still goes to the hidden panes and how many.
- The contrast test extends to the broadcast colors (active and inactive).

## Consequences

- No config migration; the `config.toml` written by `config prune` and the settings
  window list the new defaults automatically (`keybindings::defaults()`).
- `Action::ALL` grows from 37 to 38; the tests on ids, `from_id` and the palette list
  cover the new variant.
- `Ctrl+click` is no longer reported to mouse-capturing applications (vim, tmux,
  htop). `Shift` still forces Graphene's selection, unchanged.
- A `Ctrl` + click in an inactive pane no longer focuses it.
- Docs: README (shortcut tables, a "Broadcast" paragraph), and
  [docs/architecture.md](../../../docs/architecture.md) (workspace and input routing
  lines).

## Testing

- `workspace`: toggle, toggle all, `input_targets` (active in/out of set, zoom), upkeep
  on close, detach, cross-tab move and swap, move of a single-pane tab, intra-tab move
  keeping the set, new panes not added.
- `keybindings`: new defaults, freed keys.
- Per-pane key resolution: shortcut skip, editor command vs bytes.
- `chrome::scene`: bar colors and badge for broadcast panes, zoomed bar badge.
- `theme`: every built-in theme sets `pane_broadcast` and keeps the bars readable.
- `command_palette`: `ToggleBroadcastAll` hidden with a single pane.
- Manual: two panes at the prompt and one in `vim`, typing, `Ctrl+Shift+V`, middle
  click, `Ctrl+click` on a link, `Ctrl+drag` block selection, zoom.

## Découpage en tâches d'implémentation

1. [#146](https://github.com/marmotz/graphene-term/issues/146) — Nouveaux raccourcis
   par défaut des panes, onglets et zoom (§1).
2. [#147](https://github.com/marmotz/graphene-term/issues/147) — Ensemble de diffusion
   par onglet dans le workspace (§2).
3. [#148](https://github.com/marmotz/graphene-term/issues/148) — Action `Ctrl+Shift+B`
   et diffusion de la frappe et du collage (§3, §4) ; dépend de #146 et #147.
4. [#149](https://github.com/marmotz/graphene-term/issues/149) — Ctrl+clic pour ajouter
   ou retirer un pane (§5) ; dépend de #147.
5. [#150](https://github.com/marmotz/graphene-term/issues/150) — Indicateur dans la
   barre de titre et couleur de thème (§6) ; dépend de #147.
