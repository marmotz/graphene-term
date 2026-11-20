# Dev builds report a new version as available: technical design

See [overview.md](overview.md) for the product decisions.

## Root cause

`build.rs` embeds the version produced by `build_version`
([src/build_version.rs:54](../../../src/build_version.rs)):

```rust
BuildChannel::Dev => format!("{pkg_version}-dev.{}", utc_timestamp(epoch_secs)),
BuildChannel::Stable => pkg_version.to_string(),
```

A local build therefore runs as `0.1.3-dev.<timestamp>` while `Cargo.toml` is at `0.1.3`,
the version already published as `v0.1.3`.

`notice` ([src/update/mod.rs:99](../../../src/update/mod.rs)) compares the latest release
directly with the running version:

```rust
let version = parse_tag(&latest.tag)?;
(version > *current).then(|| latest.clone())
```

Under semver precedence a pre-release ranks below its release: `0.1.3 > 0.1.3-dev.X`. The
published `v0.1.3` is thus reported as newer than any dev build of `0.1.3`. The existing
test `notice_stable_release_newer_than_dev`
([src/update/mod.rs:295](../../../src/update/mod.rs)) asserts this exact behavior, which is
the defect.

The pre-release suffix of a dev build does not mean "before `X.Y.Z`": the crate version is
bumped only at release time, so dev builds are made from code at or after the published
`X.Y.Z`.

## Retained design

Compare the latest release with the **release part** of the running version: its
`major.minor.patch`, with the pre-release and build metadata dropped.

In `src/update/mod.rs`:

- Add a private helper:

  ```rust
  /// `current` without its pre-release and build metadata: a dev build of `X.Y.Z` is
  /// made from code at or after the published `X.Y.Z`.
  fn release_floor(current: &Version) -> Version {
      Version::new(current.major, current.minor, current.patch)
  }
  ```

- In `notice`, compare against it: `(version > release_floor(current)).then(...)`.
- Update the doc comment of `notice` to state the rule.

Behavior:

| Running build             | Latest release          | Notice |
| ------------------------- | ----------------------- | ------ |
| `0.1.3-dev.X`             | `v0.1.3`                | no     |
| `0.1.3-dev.X`             | `v0.1.3-dev.Y` (any Y)  | no     |
| `0.1.3-dev.X`             | `v0.1.4`, `v0.1.4-dev.Y`| yes    |
| `0.1.3` (stable)          | `v0.1.3`                | no     |
| `0.1.3` (stable)          | `v0.1.4`                | yes    |

Stable builds are unchanged: their version has no pre-release, so `release_floor` is the
identity. The rule is applied in `notice` without a channel parameter, so `tick`
([src/update/checker.rs:32](../../../src/update/checker.rs)) and `run`
([src/update/checker.rs:128](../../../src/update/checker.rs)) keep their signatures.

Unchanged: the embedded version string (`GRAPHENE_VERSION`, `graphene --version`, the
`User-Agent` header), `select_latest`, the dev channel endpoint (all releases,
pre-releases included), check intervals, and the ignored-tag handling.

## Alternatives considered

- **Semver build metadata for dev builds (`0.1.3+dev.<timestamp>`).** The `semver` crate
  orders `0.1.3+dev.X` after `0.1.3`, which would also fix the comparison. Rejected: it
  changes the visible version string the overview keeps (`X.Y.Z-dev.<timestamp>`), the
  package file names derived from it (RPM `0.dev.<timestamp>`), and
  [docs/dev/development.md](../../../docs/dev/development.md); and `+` is awkward in file
  names and tags.
- **Bump `Cargo.toml` to the next version right after each release** (dev builds become
  `0.1.4-dev.X`). Rejected: it relies on a manual step after every release, and the next
  version number (patch, minor, major) is not known in advance.
- **Skip the check for dev builds.** Rejected by the overview: dev builds keep checking.

## Consequences

- A dev build is no longer notified of a pre-release of its own `X.Y.Z`. The release
  workflow only publishes stable releases (`GRAPHENE_CHANNEL: stable` in
  [.github/workflows/release.yml](../../../.github/workflows/release.yml)), so nothing
  observable is lost.
- An already-stored `latest` in the shared state file needs no migration: the notice is
  recomputed by `notice` on each tick.

## Tests

In `src/update/mod.rs`:

- Replace `notice_stable_release_newer_than_dev` with
  `notice_skips_the_release_of_a_dev_build`: current `0.1.0-dev.X`, latest `v0.1.0`, no
  notice.
- Update `notice_newer_release` to a strictly greater release: current `0.1.0-dev.X`,
  latest `v0.1.1` and `v0.1.1-dev.Y`, notice shown.
- Update `notice_equal_or_older_release`: current `0.1.0-dev.X`, latest
  `v0.1.0-dev.Y` with Y later than X gives no notice, as do an equal and an older tag.
- Add `notice_stable_build` covering a stable current version against an equal and a
  greater release.
- Check the `checker.rs` tests that build a dev `current()`
  ([src/update/checker.rs:190](../../../src/update/checker.rs)) against fixtures of the
  same `X.Y.Z`, and adjust the expected notice if one relies on the old ordering.

## Découpage en tâches d'implémentation

1. [#159](https://github.com/marmotz/graphene-term/issues/159): compare the latest
   release with the release part of the running version in `notice`, with tests.
