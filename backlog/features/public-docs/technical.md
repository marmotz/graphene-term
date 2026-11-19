# Public documentation — Technical Design

Product decisions: [overview.md](overview.md).

## Current State

### README

- [README.md](../../../README.md) (226 lines) has no introduction beyond two lines, no
  install section (only `cargo run --release`), no feature list, no license or
  contribution section. Configuration, shell integration, shortcuts and pane behaviour
  are long paragraphs mixing user behaviour and implementation details (file rewriting
  rules, OSC 133, `ZDOTDIR`, `--posix`).
- Its "Releases" section ([README.md:69-77](../../../README.md)) is maintainer
  procedure, duplicated in [docs/development.md](../../../docs/development.md#releases).
- Its "Performance" section ([README.md:107-118](../../../README.md)) documents
  `cargo bench` and the `graphene::perf` logs: contributor material.

### `docs/`

- Four contributor documents plus an index, all in English:
  [vision.md](../../../docs/vision.md) (46 lines),
  [architecture.md](../../../docs/architecture.md) (746),
  [decisions.md](../../../docs/decisions.md) (384),
  [development.md](../../../docs/development.md) (193),
  [README.md](../../../docs/README.md) (index).
- Known stale content, checked against the code:
  - `vision.md` lists mouse reporting and images/links as *Planned*
    ([docs/vision.md:34-38](../../../docs/vision.md)), but `src/mouse.rs`,
    `src/graphics/` (kitty graphics, sixel) and `src/links.rs` (OSC 8 and detected URLs)
    implement them. Its non-goals exclude macOS
    ([docs/vision.md:44](../../../docs/vision.md)) while releases ship macOS builds.
  - `development.md` "Shortcuts" says `Ctrl+Shift+C` is "reserved for copy (no-op until
    selection exists)" ([docs/development.md:56-65](../../../docs/development.md));
    copy has existed since selection landed. Its "Tests" table lists 9 modules; tests
    now also live in `tests/` (7 integration files) and most `src/` modules.
  - `development.md` "Configuration" names the theme as defined in `src/palette.rs`;
    built-in themes are now files in [themes/](../../../themes/), loaded by
    `src/theme.rs`.
- References to these paths from elsewhere in the repo: [README.md:116](../../../README.md),
  [README.md:223-226](../../../README.md),
  [shell-integration/bash/graphene.bash:134](../../../shell-integration/bash/graphene.bash)
  (comment), and two backlog technical designs
  ([e2e-tests](../e2e-tests/technical.md), [pane-mouse-rearrange](../pane-mouse-rearrange/technical.md)).

### Command line

- `graphene` only knows `--version` and `--prune-config [--yes]`; anything else runs
  the app ([src/main.rs:29-39](../../../src/main.rs)). There is no way to start a
  program other than the user's shell.
- The first pane is spawned in `App::create_session` with
  `Terminal::spawn(proxy, size, config, None)` ([src/app.rs:2423-2428](../../../src/app.rs));
  later panes go through `new_pane_start` ([src/app.rs:395-410](../../../src/app.rs)).
- `Terminal::spawn` builds `tty::Options { shell, .. }` where `shell` is the shell
  integration injection, or `None` so `alacritty_terminal` starts `$SHELL`
  ([src/terminal.rs:77-101](../../../src/terminal.rs)).
- The app exits when its last pane closes ([src/app.rs:2016-2033](../../../src/app.rs)),
  so a pane running a finite command closes the window when the command ends.

### Release assets and install paths

- The release workflow publishes `.deb`, `.rpm`, `.AppImage` and Linux `.tar.gz`, plus
  `.tar.gz` and `.dmg` for Apple Silicon and Intel macOS
  ([.github/workflows/release.yml](../../../.github/workflows/release.yml),
  [docs/development.md:136-148](../../../docs/development.md)).
- `scripts/package.sh` already renders the AUR package `graphene-term-bin`; pushing it
  to the AUR is commented out ([docs/development.md:176-193](../../../docs/development.md)).
- The macOS app is ad-hoc signed, not notarized: first launch needs **Open Anyway** or
  `xattr -dr com.apple.quarantine` ([README.md:73-77](../../../README.md)).

### Existing benchmarks and scripts

- `benches/render.rs` (criterion) measures internal stages on recorded PTY output;
  it cannot run other terminals. `benches/fixtures/record.py` is a Python 3 stdlib
  script: Python is already a dev dependency.
- Shell scripts in `scripts/` are tested by Rust integration tests that run them in a
  sandbox (e.g. [tests/release_notes_script.rs](../../../tests/release_notes_script.rs)).

## Technical Decisions

### File layout

```
README.md                  English entry page (shown by GitHub)
README.fr.md               French entry page
CONTRIBUTING.md            how to contribute (English)
docs/
  README.md                index: user docs (en, fr) and contributor docs
  images/                  screenshot / GIF used by the READMEs
  en/                      user documentation, English
    README.md              index of the user documentation
    installation.md
    getting-started.md
    configuration.md
    themes.md
    keybindings.md
    tabs-and-panes.md
    command-blocks.md
    input-editor.md
    selection-and-clipboard.md
    images-and-links.md
  fr/                      same files, French (English file names, for 1:1 links)
  dev/                     contributor documentation, English only
    README.md              index (current docs/README.md)
    vision.md
    architecture.md
    decisions.md
    development.md
scripts/
  benchmark-terminals.py   comparative benchmark, prints the README tables
tests/
  benchmark_script.rs      integration test of the table rendering
```

- `docs/fr/` mirrors `docs/en/` file for file; each user page links to its counterpart
  at the top (`[Français](../fr/<same>.md)` / `[English](../en/<same>.md)`), like the
  two READMEs.
- Contributor docs move with `git mv` (history kept). Every reference listed in
  *Current State* is updated to `docs/dev/...`.

### User documentation content

Each page is written for a user, without implementation details (no `ZDOTDIR`, OSC
numbers only where a user needs them, e.g. "programs that emit OSC 8 links"). Source
material per page:

| Page                         | Content                                                                                                                                         | From                                                                           |
|------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------|
| `installation.md`            | Packages per OS (deb, rpm, AppImage, tar.gz, dmg) and how to install each; macOS first-launch unblock; build from source (toolchain, system deps); AUR section added when the package is published | README Releases, development.md Releases/AUR, release.yml                       |
| `getting-started.md`         | First launch, what you see, the 5 things to try (blocks, palette, split, settings, theme), `graphene -e`, `--version`                              | new                                                                            |
| `configuration.md`           | File locations (Linux/macOS/XDG), auto-generated commented file, live reload, error banner, settings window, `--prune-config`, full reference of every key, updates check, `RUST_LOG`, state directory | README Configuration, `config.default.toml`                                    |
| `themes.md`                  | 8 built-ins, custom theme files and their format, partial themes, light/dark following, background opacity                                     | README, [themes/](../../../themes/), `src/theme.rs`                             |
| `keybindings.md`             | Every default shortcut by area, remapping syntax, `none`, action names                                                                          | README Shortcuts tables, `config.default.toml` `[keybindings]`, `src/keybindings.rs` |
| `tabs-and-panes.md`          | Tabs, tab bar position, splits, focus/resize/zoom, pane title bar, drag to rearrange, command palette, close confirmation                       | README Shortcuts prose                                                          |
| `command-blocks.md`          | Shell integration (supported shells, opt-out, what it changes), blocks, status colors, select/copy/fold                                         | README Shell integration                                                        |
| `input-editor.md`            | Editing at the prompt, its shortcuts, limits (macOS Bash 3.2)                                                                                   | README prompt table                                                             |
| `selection-and-clipboard.md` | Mouse selection modes, rectangular selection, primary selection, paste, selecting while an app captures the mouse                               | README Shortcuts                                                                |
| `images-and-links.md`        | Inline images (kitty graphics protocol, sixel), clickable links (OSC 8, detected URLs, schemes opened, how to click)                           | `docs/dev/decisions.md` (Links, Images), `src/links.rs`, `src/graphics/`       |

The configuration reference is the one place listing every key; other pages link to
it instead of repeating defaults. Its content must match `config.default.toml`
(the test in `src/config.rs` keeps that file in sync with the code; the docs are
checked by hand when a key changes, noted in `docs/dev/development.md`).

### README structure (both languages)

1. Title, one-line pitch, language switch link, screenshot/GIF
   (`docs/images/`, provided by the maintainer).
2. **Features**: short bullets, one per feature, each linking to its user page.
3. **Comparison**: performance table, then feature table, then the test machine, the
   tested versions and how to reproduce (`scripts/benchmark-terminals.py`).
4. **Install**: one line per package type, link to `installation.md`.
5. **Configuration**: file location, the settings window, link to `configuration.md`.
6. **Build from source**: prerequisites, `cargo build --release`, link to
   `CONTRIBUTING.md`.
7. **Contributing** (link), **Documentation** (links to user and contributor docs),
   **License** (MIT or Apache-2.0, from `Cargo.toml`).

The "Releases" and "Performance" sections leave the README for
`docs/dev/development.md`, which already holds the release procedure.

The comparison tables are English in both READMEs except labels: the script prints
English headers; `README.fr.md` uses the same numbers with translated headers, edited
by hand.

### Feature table

Rows: every user-facing Graphene feature, from the code:

| Area        | Rows                                                                                                                                                                              |
|-------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Rendering   | GPU rendering; truecolor; inline images (kitty graphics protocol, with animations); sixel images; translucent background                                                          |
| Shell       | Command blocks with exit status; fold / unfold outputs; copy a command or its output; automatic shell integration (zsh, bash, fish) without editing dotfiles; rich prompt editor (mouse cursor, selection, editor shortcuts) |
| Layout      | Tabs; split panes; drag panes to rearrange; zoom a pane; command palette (fuzzy)                                                                                                  |
| Interaction | Clickable links (OSC 8 and detected URLs); rectangular selection; mouse reporting to applications; remappable keybindings                                                        |
| Config      | Settings window (GUI); live config reload; themes following the system light/dark mode; built-in themes; interface translated (en, fr); remembered window size; update notification |

Cells are `✅` / `❌` / `⚠️` (partial, with a footnote), filled by hand by the
maintainer from tests on the version written in the table header. The table is
hand-maintained Markdown in both READMEs; the script does not produce it.

### `graphene -e` (new code)

The benchmark needs `graphene -e <program> [args...]`, the usual `-e` convention of
terminal emulators (Alacritty, Konsole...).

- `src/main.rs`: `Command::Run { program: Option<Vec<String>> }`. `-e` takes every
  following argument; `-e` with nothing after it is an error printed to stderr with exit
  code 2. `--version` and `--prune-config` keep priority only when they come before `-e`
  (after it they are the program's arguments). Unit tests next to the existing ones
  ([src/main.rs:41](../../../src/main.rs)).
- `graphene::run(program: Option<Vec<String>>)` → `app::run(program)`; `App` keeps it in
  a `first_program: Option<Vec<String>>` field, `take()`n when the first pane spawns
  ([src/app.rs:2423](../../../src/app.rs)). Later panes and tabs start the user's shell
  as today.
- `Terminal::spawn` gets a `program: Option<Vec<String>>` parameter. When `Some`, it
  builds `tty::Shell::new(program[0], program[1..])` and does not call
  `shell_integration::prepare` (a command is not a shell: no hooks, no blocks); `TERM`,
  `COLORTERM` and `TERM_PROGRAM` are still set.
- When the program exits, its pane closes and, being the last, the app exits (existing
  behaviour): `graphene -e true` opens a window and quits, which the startup benchmark
  relies on.
- Documented in `getting-started.md`. There is no `--help` today; adding one is out of
  scope.

### Benchmark script (`scripts/benchmark-terminals.py`)

Python 3, standard library only, Linux only. One file with three entry points:

```
scripts/benchmark-terminals.py run [--terminals a,b,..] [--runs N] [--out results.json]
scripts/benchmark-terminals.py render results.json
scripts/benchmark-terminals.py payload {throughput|fire|idle} --result FILE [...]
```

- `run` benchmarks each installed terminal (missing ones are skipped and reported),
  writes raw results to JSON, then prints the Markdown table (same code as `render`).
- `render` turns a results JSON into the Markdown tables; it is what the integration
  test exercises.
- `payload` is what runs *inside* the terminal under test; it writes its measurement to
  `--result` and exits, which closes the terminal.

**Terminal table.** A dict in the script: name, binary, version command, and how to
start a command (`argv` prefix). Graphene uses `target/release/graphene` when present,
else `graphene` on `PATH`. Each prefix is checked against the terminal's `--help` when
the script is written; the table is the only place to fix when a terminal changes.

GNOME Terminal starts windows through a shared `gnome-terminal-server`: its launcher
returns at once and the server holds the memory. The script uses `--wait` to keep the
launcher until the command ends, measures the server process, and refuses to run the
GNOME Terminal case while another `gnome-terminal-server` is running (it never kills
the user's terminals).

**Measurements** (each repeated `--runs` times, default 5; the median is reported):

| Metric            | How                                                                                                                                                                                                                                      | Unit   |
|-------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|--------|
| Throughput        | `payload throughput`: times `/bin/cat shakespeare.txt` written to the terminal (stdout is the PTY), monotonic clock around the child process                                                                                                 | s      |
| Refresh rate      | `payload fire`: renders DOOM-fire-style frames (fire algorithm, 256 colors, half-block characters, full terminal size read with `TIOCGWINSZ`) for 10 s and counts frames written per second. Same principle as const-void/DOOM-fire-zig, reimplemented (no Zig dependency, result written to a file) | fps    |
| Startup           | wall time of `<terminal> <exec prefix> true`, from spawn to exit                                                                                                                                                                         | ms     |
| Memory idle       | `payload idle` sleeps 5 s; the script samples PSS of the terminal's processes at 3 s                                                                                                                                                     | MiB    |
| Memory under load | peak PSS of the terminal's processes sampled every 100 ms during the throughput run                                                                                                                                                      | MiB    |

- PSS comes from `/proc/<pid>/smaps_rollup`, summed over the terminal process and its
  descendants, excluding the payload process and its children (the shell-less payload
  is started directly, so the tree is terminal + helpers only).
- `shakespeare.txt` is downloaded from
  `https://raw.githubusercontent.com/vizenc/terminal-benchmark/refs/heads/main/test/shakespeare.txt`
  into `$XDG_CACHE_HOME/graphene-benchmark/` (else `~/.cache/...`) and checked against
  SHA-256 `5d1293ba864d3f7200f8ed45c7fe75b9bfbc110726a438cdb28835b7c08662b3`; a
  mismatch aborts. Nothing is added to git.
- Each terminal runs with its own default configuration; the script records the
  versions (`--version`), the CPU, GPU (`lspci` VGA line when available), kernel,
  session type (`XDG_SESSION_TYPE`) and screen resolution, printed under the table.
- A payload that does not report within a timeout (60 s) marks the cell `n/a` and the
  terminal is killed (only the process the script started).

**Output.** Markdown, one row per terminal, one column per metric, Graphene first and
in bold; the best value of each column in bold too. Followed by the machine block.

### Tests

- `src/main.rs`: unit tests for `-e` parsing (program and args, `-e` alone, flags after
  `-e`, `--version` before `-e`).
- `src/terminal.rs`: the `Shell` choice is extracted into a pure function
  (`program` given → that program, no injection; else injection or `None`) and unit
  tested.
- `tests/benchmark_script.rs`: runs `python3 scripts/benchmark-terminals.py render` on a
  fixture JSON (`tests/fixtures/benchmark-results.json`) and checks the table (header,
  medians, bold best values, `n/a`, machine block). Skipped with a message when
  `python3` is missing, like the GPU-less skips in `benches/`.
- The payloads are covered by the same integration test: `payload fire --duration 0.2`
  and `payload throughput` on a small file, with stdout on a pipe, must write a positive
  value to the result file.
- Docs: no automated link checker (no tool in the stack); links are checked by hand
  during review. Out of scope to add one.

### Alternatives considered

- *`SHELL=script graphene` instead of `-e`*: rejected by the user; visible hack in a
  published benchmark, and `-e` is useful on its own.
- *vtebench for throughput*: rejected for a plain `cat` of a known 11 MB file, no
  external tool.
- *DOOM-fire-zig run by hand*: rejected; it prints its fps only on screen and stops on
  Ctrl+C, so results cannot be collected. The payload reimplements its principle.
- *X11 DAMAGE counting for refresh rate*: rejected; X11 only (foot is Wayland only)
  and unproven.
- *Typing latency*: excluded by product decision.
- *Committing `shakespeare.txt`*: rejected; 11 MB in every clone forever.
- *Bash for the benchmark script*: rejected; PSS sampling, JSON and table rendering are
  much simpler in Python, already used by `benches/fixtures/record.py`.
- *Translated contributor docs*: rejected (product decision), English only.
- *Generated docs site (mdBook)*: rejected (product decision), plain Markdown.

### Risks

- Default configurations differ (font, window size), which changes grid size and so
  throughput and fps. Mitigation: every result is published with the machine and
  versions, and the script is public so anyone can rerun it. Forcing an identical
  setup per terminal is out of scope.
- `cat` completing measures how fast a terminal *accepts* output, not what it draws;
  that is the usual meaning of this benchmark and is stated in the README.

## Découpage en tâches d'implémentation

- [#139 — [Docs] Option graphene -e pour lancer une commande](https://github.com/marmotz/graphene-term/issues/139)
- [#140 — [Docs] Docs contributeur : déplacement vers docs/dev et mise à jour](https://github.com/marmotz/graphene-term/issues/140)
- [#141 — [Docs] Script de benchmark comparatif des terminaux](https://github.com/marmotz/graphene-term/issues/141) — dépend de #139 et #140.
- [#142 — [Docs] Documentation utilisateur en anglais (docs/en)](https://github.com/marmotz/graphene-term/issues/142) — dépend de #139 et #140.
- [#143 — [Docs] Documentation utilisateur en français (docs/fr)](https://github.com/marmotz/graphene-term/issues/143) — dépend de #142.
- [#144 — [Docs] README anglais, CONTRIBUTING.md et comparatif](https://github.com/marmotz/graphene-term/issues/144) — dépend de #140, #141 et #142.
- [#145 — [Docs] README français (README.fr.md)](https://github.com/marmotz/graphene-term/issues/145) — dépend de #143 et #144.
