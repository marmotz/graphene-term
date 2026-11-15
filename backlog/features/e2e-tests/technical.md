# End-to-End Tests — Technical Design

Product decisions: [overview.md](overview.md).

## Current State

- Graphene is a Linux desktop application ([Cargo.toml:5](../../../Cargo.toml),
  [README.md:3](../../../README.md)). Its window/input stack is `winit`; the
  `egui-winit` dependency enables both X11 and Wayland
  ([Cargo.toml:39-51](../../../Cargo.toml)).
- The executable dispatches normal startup to `graphene::run`
  ([src/main.rs:13-18](../../../src/main.rs),
  [src/lib.rs:48-51](../../../src/lib.rs)). `app::run` owns a `winit` event loop; session
  creation opens the real window, spawns a PTY shell and creates the GPU renderer
  ([src/app.rs:1971-2029](../../../src/app.rs)).
- Most current tests are module-local unit tests; the development guide lists the
  covered modules and notes that terminal-frame tests do not need a PTY or GPU
  ([docs/development.md:69-86](../../../docs/development.md)). The `tests/` integration
  tests currently exercise command-line behavior and packaging scripts, not the GUI
  ([tests/config_prune_command.rs:17-27](../../../tests/config_prune_command.rs)).
- The only documented GUI automation is a manual X11 check using `xdotool` and
  ImageMagick; it uses fixed sleeps and captures a screenshot
  ([docs/development.md:107-121](../../../docs/development.md)).
- Pull-request CI currently runs on `ubuntu-latest`, formats, lints, runs `cargo test`,
  and builds the release binary ([.github/workflows/ci.yml:14-41](../../../.github/workflows/ci.yml)).
  There is no GUI test job yet.
- Test data can be isolated using `XDG_CONFIG_HOME`, `XDG_STATE_HOME` and `HOME`;
  Graphene resolves its configuration and state paths from these variables
  ([src/config.rs:39-79](../../../src/config.rs)). Shell integration is selected through
  `SHELL` for bash, zsh and fish ([src/shell_integration.rs:58-85](../../../src/shell_integration.rs),
  [src/shell_integration.rs:121-149](../../../src/shell_integration.rs)).
- `Action::ALL` lists 37 palette actions, while `GoToTab`, the palette itself, pointer
  interactions and settings fields have additional UI routes
  ([src/input.rs:57-98](../../../src/input.rs),
  [src/app.rs:1446-1533](../../../src/app.rs),
  [src/i18n.rs:88-130](../../../src/i18n.rs)).

## Technical Decisions

### Test boundary and isolation

- End-to-end scenarios launch the built `graphene` executable as a separate process and
  interact with its real windows and PTY. They do not call application internals or
  replace the `winit` event loop; existing unit tests remain the fast checks for pure
  logic.
- General UI scenarios use a controlled shell fixture so that terminal output and shell
  lifetime are predictable. Shell-integration scenarios separately exercise bash, zsh
  and fish, since `shell_integration::prepare` has distinct startup injection for each.
- Each run uses a disposable home and XDG config, state and runtime directories. Disable
  update checks in the test configuration, avoid user dotfiles, and terminate the app
  and child shell at the end of each scenario. This prevents tests from changing real
  user state or depending on GitHub/network availability.

### GUI automation tool evaluation

- The exact GUI automation tool is intentionally not selected yet. Before building the
  complete suite, add a small evaluation step to verify a dedicated tool against the
  app's custom-rendered `wgpu`/`egui` UI.
- The evaluation must show that the tool can launch and stop the real app, send keyboard
  and pointer input, synchronize on observable UI state rather than fixed sleeps, and
  run in both virtual X11 and virtual Wayland sessions with software rendering.
- Prefer assertions on behavior visible to a user (for example, a command's displayed
  result, a changed setting, a tab/pane transition, or clipboard contents). Save
  screenshots and logs on failure for diagnosis, but do not use pixel snapshots as the
  pass/fail oracle.
- The existing `xdotool` workflow is a useful X11 baseline but is not sufficient alone
  because the required matrix also includes native Wayland. The evaluation should choose
  a tool that meets both backend requirements rather than silently reducing coverage.

### Scenario inventory and coverage

- Maintain a readable E2E scenario catalogue that maps each user-facing feature and
  action to its scenario, expected observable result, shell fixture or shell variant,
  and the X11/Wayland runs that cover it.
- Seed the catalogue from `Action::ALL`, then include routes not represented there:
  palette opening/search/selection, numbered tab selection, pointer-based tab and pane
  interactions, selection and clipboard actions, settings fields, dialogs and banners.
  The README interaction tables are a useful baseline, not the coverage authority
  ([README.md:104-190](../../../README.md)).
- Include dedicated shell-integration scenarios for bash, zsh and fish, in addition to
  the controlled fixture used for general UI scenarios. Run each catalogue scenario on
  both X11 and Wayland, as required by the product scope.
- Update the catalogue whenever an action or user-facing interaction is added or
  changed. A change is not considered fully covered until its scenario and expected
  result are present and pass on both backends.

### CI and blocking behavior

- Add E2E execution to pull-request CI in `.github/workflows/ci.yml`, using virtual
  display sessions and software GPU rendering for both backends. Keep the existing unit,
  lint and build checks; E2E complements rather than replaces them.
- Make the E2E check blocking as soon as its first scenario is introduced. Coverage will
  therefore grow incrementally: the catalogue must clearly show which scenarios are
  still missing until the full inventory passes on both backends.
- The workflow currently runs on pull requests, but required status checks are a
  repository ruleset/branch-protection setting rather than a workflow file setting.
  Configure the new E2E check as required there so an E2E failure prevents validation
  and merge.

### Alternatives considered

- *Keep GUI validation manual*: rejected because it does not provide repeatable
  protection on each change.
- *Use only unit tests or inject events directly into application internals*: rejected
  because those paths do not exercise the real window, input backend, PTY and renderer
  together; those layers are what the E2E suite must validate.
- *Use only the current `xdotool` approach*: rejected because it covers X11 but does not
  meet the chosen Wayland coverage requirement.
- *Use screenshot comparisons as the main assertion*: rejected because display backend,
  font and rendering differences can make pixel baselines brittle. Screenshots remain
  failure artifacts; behavior is the test oracle.
- **Evaluate a dedicated GUI automation tool, then use it for the X11 and Wayland
  matrix**: retained because it follows the chosen cross-backend scope while avoiding a
  premature commitment to a tool that may not work with Graphene's custom rendering.

## Découpage en tâches d’implémentation

- [#112 — Évaluer un outil d’automatisation graphique sur X11 et Wayland](https://github.com/marmotz/graphene-term/issues/112)
- [#113 — Créer le harness et activer le premier contrôle CI bloquant](https://github.com/marmotz/graphene-term/issues/113) — dépend de #112.
- [#114 — Couvrir la navigation, la palette et le chrome de l’application](https://github.com/marmotz/graphene-term/issues/114) — dépend de #113.
- [#115 — Couvrir les interactions du terminal et les intégrations shell](https://github.com/marmotz/graphene-term/issues/115) — dépend de #113.
- [#116 — Couvrir les réglages, la configuration et les états de bannière](https://github.com/marmotz/graphene-term/issues/116) — dépend de #113 et #114.
