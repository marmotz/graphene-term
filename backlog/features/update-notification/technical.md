# Update notification: technical design

Product decisions: [overview.md](overview.md).

## Dependencies on other features

- [config-themes](../config-themes/technical.md): the `[updates]` settings live in
  `config.toml` (issue [#14](https://github.com/marmotz/graphene-term/issues/14), hot
  reload [#19](https://github.com/marmotz/graphene-term/issues/19)), and the update
  banner reuses the banner drawing of
  [#18](https://github.com/marmotz/graphene-term/issues/18).
- [images-links](../images-links/technical.md) §5 opens links with `xdg-open` from a
  detached thread ([#24](https://github.com/marmotz/graphene-term/issues/24)). The same
  helper opens the release page: whichever feature lands first creates it.
- [tabs-splits-palette](../tabs-splits-palette/technical.md): banner strings go through
  its translation table ([#35](https://github.com/marmotz/graphene-term/issues/35)).

## The problem: the binary does not know its own release

- The released binary is built by the pull request CI
  ([ci.yml:39-48](../../../.github/workflows/ci.yml)) and reused by the Release workflow
  ([release.yml:29-47](../../../.github/workflows/release.yml),
  [find-ci-binary.sh](../../../scripts/find-ci-binary.sh)).
- The tag is computed afterwards from the Release run number:
  `tag="v${version}-dev.${build}"` with `build=$GITHUB_RUN_NUMBER`
  ([package.sh:24](../../../scripts/package.sh),
  [release.yml:93](../../../.github/workflows/release.yml)).
- The binary only knows `CARGO_PKG_VERSION` (`0.1.0`,
  [Cargo.toml:3](../../../Cargo.toml)), so it cannot tell whether `v0.1.0-dev.7` is
  newer than itself.

## Alternatives considered

| Topic | Options | Retained |
|---|---|---|
| Build identity | Commit count on develop / git tree hash / rebuild in Release / **build timestamp** | Build timestamp (decided). It is known when the binary is compiled, in the PR CI, in the Release fallback and locally. The tag is then derived from the binary, not the other way around. |
| HTTP client | `reqwest` / `ureq` / spawn `curl` | `ureq` (blocking, rustls): no async runtime, no dependency on a system `curl` (the deb only depends on `libc6, libgcc-s1`). |
| Version comparison | String compare / `semver` crate | `semver`: numeric pre-release identifiers compare numerically, and `0.1.0-dev.X < 0.1.0`. |
| Check scheduling | Timer inside the event loop / background thread | Background thread: the network call never blocks the UI. It wakes the event loop through `EventLoopProxy`. |
| Shared state between instances | In memory / state file | State file under `$XDG_STATE_HOME`. Several windows share the last check and the ignored version. |

## Design

### 1. Build version: `build.rs` and `--version`

- New `build.rs` emits `cargo:rustc-env=GRAPHENE_VERSION=<version>`:
  - channel `dev` (default): `<CARGO_PKG_VERSION>-dev.<UTC YYYYMMDDHHMMSS>`, for example
    `0.1.0-dev.20261112163249`;
  - channel `stable` (`GRAPHENE_CHANNEL=stable` at build time): `<CARGO_PKG_VERSION>`.
- Timestamp source: `SOURCE_DATE_EPOCH` when set, else the current time. Epoch to
  UTC date conversion is a small days-to-civil function (no build dependency), tested
  through a module shared with `build.rs` via `include!`.
- Rerun rules: `rerun-if-env-changed` for `SOURCE_DATE_EPOCH` and `GRAPHENE_CHANNEL`,
  `rerun-if-changed` for `src` and `Cargo.toml`. A local incremental build refreshes
  the timestamp whenever the sources change.
- The CI release build sets `SOURCE_DATE_EPOCH=$(date +%s)` so a restored `target/`
  cache never carries an older timestamp.
- `main.rs` handles `--version` (prints `graphene <GRAPHENE_VERSION>`) before creating
  the event loop.
- A local `cargo build` is a `dev` build with a local timestamp: it is alerted only of
  releases built after it (decided: local builds go through the same rules).

### 2. Packaging derives the tag from the binary

- [package.sh](../../../scripts/package.sh) drops its `<build-number>` argument and
  reads the version from `target/release/graphene --version`. `tag="v${version}"`;
  for a dev build `deb_version`, `rpm_release` and `aur_pkgver` use the timestamp
  where they use `${build}` today.
- [release.yml](../../../.github/workflows/release.yml) stops passing
  `$GITHUB_RUN_NUMBER`.
- Ordering stays increasing for every package manager: `dev.20261112163249 > dev.3`
  (existing tags `v0.1.0-dev.1` to `dev.3`), same for the deb `~dev.N`, the rpm
  `0.dev.N` and the AUR `rN`.
- Monotonicity: a reused CI binary requires the branch to be up to date with develop
  ([find-ci-binary.sh:34-36](../../../scripts/find-ci-binary.sh)), so it was built after
  the previous merge; the fallback build happens at release time.
- No stable workflow exists yet. When it does, it builds with
  `GRAPHENE_CHANNEL=stable` and publishes without `--prerelease`.

### 3. Configuration: `[updates]` in `config.toml`

```toml
[updates]
check = true                 # false: no network call at all
interval = "auto"            # "auto", "6h", "12h", "24h", "2d", "3d", "1w"
                             # auto: 6h for dev builds, 24h for stable builds
```

Read with the per-key helper of `config.rs`; an invalid `interval` falls back to
`"auto"` with a config error, like any other key.

### 4. State file: `src/update/state.rs`

- `$XDG_STATE_HOME/graphene/update.json`, else `$HOME/.local/state/graphene/update.json`.
- Content: `last_check` (Unix seconds), `latest` (`{ tag, url }` or null),
  `ignored` (tag or null).
- Written to a temporary file then renamed (atomic for concurrent instances). A missing
  or unreadable file means "never checked"; errors are logged, never shown.

### 5. Check: `src/update/mod.rs`

- Repository from `env!("CARGO_PKG_REPOSITORY")` (`owner/name`).
- Dev channel: `GET https://api.github.com/repos/<owner>/<name>/releases?per_page=30`.
  Drafts are skipped, `tag_name` without its `v` is parsed as `semver::Version`
  (unparsable tags skipped), the highest one wins. Stable releases are included: a
  stable `0.1.0` is newer than `0.1.0-dev.X`.
- Stable channel: `GET .../releases/latest`, which excludes pre-releases.
- Headers: `User-Agent: graphene/<version>`, `Accept: application/vnd.github+json`.
  Timeout 10 s. Any network or parse error is logged at `debug` and the next check
  waits for the next interval.
- Pure decision function, the core of the tests:
  `fn notice(current: &Version, latest: Option<&Release>, ignored: Option<&str>) -> Option<Release>`
  returns the release to show when `latest > current` and `latest.tag != ignored`.

### 6. Scheduling

- Thread `update-check` started in `App::resumed` when `updates.check` is true.
- Loop: `due = last_check + interval`. If `now >= due`, call GitHub and write the state;
  otherwise reuse `latest` from the state (decided: the cache lasts one interval, so
  windows opened in a row make a single call). Then compute `notice` and send
  `UserEvent::Update(Option<Release>)` through the `EventLoopProxy`, and sleep until the
  next `due`.
- Settings changes (hot reload, ignore action) reach the thread through an `mpsc`
  channel; `recv_timeout(due - now)` doubles as the sleep. `check = false` ends the
  thread; `true` again restarts it.
- `UserEvent` ([terminal.rs:32](../../../src/terminal.rs)) gains
  `Update(Option<Release>)`; `App::user_event`
  ([app.rs:443](../../../src/app.rs)) stores it on the session and requests a redraw.

### 7. Banner

- Reuses the banner of config-themes #18 (rect in `ui.banner_background`, one line of
  text over the grid). The config error banner has priority; the update banner shows
  once the error banner is gone.
- Text: "Graphene <tag> is available", then three click targets: "Open release",
  "Ignore this version", "×". The renderer returns their pixel rects so the left press
  ([app.rs:184](../../../src/app.rs)) hit-tests them before starting a selection.
- Open release: `xdg-open <html_url>` (helper shared with images-links). The banner
  stays.
- × (close for the session): hides the banner until the next launch; a later check
  finding an even newer release shows it again.
- Ignore this version: writes `ignored = tag` in the state, hides the banner. A newer
  tag shows again.

## Consequences checked against the code

- `package.sh` usage changes (`<build-number>` removed): the usage comment, the
  argument check ([package.sh:12-15](../../../scripts/package.sh)) and the call in
  `release.yml` are updated together.
- The existing tags `v0.1.0-dev.1..3` stay; the next tag is a timestamp and sorts
  after them for GitHub, semver, dpkg, rpm and pacman.
- The release binary of the CI now embeds its build time: two CI runs on the same tree
  produce different binaries. Nothing relies on byte-identical builds today.
- The banner click targets must be hit-tested before `selection::start_selection`
  ([app.rs:193](../../../src/app.rs)), otherwise a click on "×" starts a selection.
- First network access of the application: `README.md` documents the check, the
  endpoint called and `updates.check = false`.

## Tests

- `build` helper: days-to-civil conversion on known epochs (epoch 0, leap day, end of
  year); version string for both channels.
- `update`: `notice` (newer, equal, older, ignored, newer than ignored, stable newer than
  dev); release list selection (drafts, unparsable tags, highest semver); interval
  parsing and `auto` per channel; `due` computation.
- `state`: round trip, missing file, corrupt file, atomic write.
- Release JSON parsing from a recorded GitHub response fixture.
- `package.sh`: run on a fake binary printing a known `--version`, check the produced
  file names.
- HTTP and `xdg-open` are thin adapters, checked manually: install `v0.1.0-dev.N`,
  publish a newer pre-release, check the banner and the three actions.

## Découpage en tâches d'implémentation

| Issue | Task | Depends on |
|---|---|---|
| [#56](https://github.com/marmotz/graphene-term/issues/56) | Build timestamp version and tag derived from the binary (§1, §2) | - |
| [#57](https://github.com/marmotz/graphene-term/issues/57) | Settings, state file and GitHub release check (§3, §4, §5) | #56, #14 |
| [#58](https://github.com/marmotz/graphene-term/issues/58) | Background check thread and update event (§6) | #57 |
| [#59](https://github.com/marmotz/graphene-term/issues/59) | Banner with open, ignore and close actions (§7) | #58, #18, #35 |
