# CI tests: technical design

Product decisions: [overview.md](overview.md).

## Starting point

Checked locally on 2026-11-12 with cargo 1.99.1 (stable).

- No CI exists: there is no `.github/` folder in the repository.
- Single crate `graphene` ([Cargo.toml](../../../Cargo.toml)), edition 2024, no
  workspace, no `tests/` folder. Tests are unit tests in `#[cfg(test)]` modules of
  `src/input.rs`, `src/palette.rs`, `src/renderer/scene.rs`, `src/selection.rs`,
  `src/frame.rs` and `src/layout.rs`.
- `cargo test`: 72 passed, 0 failed. No test opens a window or a GPU device, so a
  headless runner is enough.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo fmt --check`: **fails**, 52 hunks across `src/app.rs`, `src/frame.rs`,
  `src/input.rs`, `src/layout.rs`, `src/palette.rs`, `src/renderer/mod.rs`,
  `src/renderer/scene.rs`, `src/renderer/text.rs` and `src/selection.rs`. There is no
  `rustfmt.toml`, so the default style applies.
- [rust-toolchain.toml](../../../rust-toolchain.toml) pins `channel = "stable"` with no
  `components`.
- Native libraries: `wayland-sys` and `wayland-backend` are built with their `dlopen`
  feature (`cargo tree -e features`), and X11/xkbcommon go through `x11-dl` and
  `xkbcommon-dl`. They are loaded at runtime, so building and testing need no system
  package on the runner.

## Alternatives considered

| Topic | Options | Retained |
|---|---|---|
| Existing formatting debt | Enable the rustfmt check as is / format the code base first | Format first, in its own commit before the workflow lands: otherwise the first PR is red for reasons unrelated to it. |
| Toolchain install | Third-party toolchain action / rustup already present on GitHub's Ubuntu image, driven by `rust-toolchain.toml` | `rust-toolchain.toml` plus `components = ["clippy", "rustfmt"]`: local and CI use the same toolchain from one file, with no extra action. |
| Job layout | One job running every step / one job per check | One job: a single compile cache, and the checks share build artifacts. Order: fmt (fastest), clippy, test, release build. The job still fails on the first failing step. |
| Build cache | None / `actions/cache` on `~/.cargo` and `target` / dedicated Rust cache action | `actions/cache` keyed on `Cargo.lock` and `rust-toolchain.toml`: first-party, no third-party action to trust. Exact action versions are checked against their release pages when the workflow is written. |
| Branch protection | Set with `gh api` / set manually | Manual (user decision). |

## Design

### 1. Formatting debt

Run `cargo fmt` on the whole crate and commit the result alone, with no other change,
so the diff is purely mechanical. `cargo test` and `cargo clippy` must stay green
afterwards.

### 2. Toolchain components

Add `components = ["clippy", "rustfmt"]` to `rust-toolchain.toml`. Rustup installs them
automatically on the first `cargo` call, locally and on the runner.

### 3. Workflow `.github/workflows/ci.yml`

- `name: CI`, job id `check` (this is the name to require in branch protection).
- Trigger: `pull_request` with `branches: [develop]`. No `push` trigger.
- `runs-on: ubuntu-latest`.
- `concurrency` grouped by PR with `cancel-in-progress: true`, so a new push to a PR
  cancels the previous run.
- `env: CARGO_TERM_COLOR: always`. `RUSTFLAGS=-D warnings` is not set globally (it
  would invalidate build artifacts between steps); warnings are denied in clippy only.
- Steps:
  1. Checkout.
  2. Cache `~/.cargo/registry`, `~/.cargo/git` and `target`, keyed on the OS,
     `hashFiles('Cargo.lock', 'rust-toolchain.toml')`.
  3. `cargo fmt --check`
  4. `cargo clippy --all-targets --locked -- -D warnings`
  5. `cargo test --locked`
  6. `cargo build --release --locked`

`--locked` makes the CI fail if `Cargo.lock` is out of date, instead of silently
resolving new versions.

### 4. Branch protection (manual)

Once the workflow has run at least once on a PR (GitHub only offers checks it has
seen), in the repository Settings > Branches, add a rule for `develop`:

- Require a pull request before merging.
- Require status checks to pass before merging, with the `check` check selected.
- Require branches to be up to date before merging.

## Consequences

- Every PR toward `develop` gets a `check` status. It blocks the merge once the
  protection rule is set.
- Pushing directly to `develop` no longer works once "Require a pull request" is on.
- Contributors must run `cargo fmt` before pushing. Configuring the IDE to format on
  save is enough.
- The workflow is not tested by the Rust test suite. It is verified by opening the PR
  that adds it: the run on that PR must be green.

## Découpage en tâches d'implémentation

1. [#50 CI: format the code base with rustfmt](https://github.com/marmotz/graphene-term/issues/50) (§1)
2. [#51 CI: GitHub Actions workflow for pull requests](https://github.com/marmotz/graphene-term/issues/51) (§2, §3, §4), depends on #50
