# CI tests

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

The repository has no continuous integration yet (no `.github/workflows/`): tests,
typecheck and lint only run locally, at the discretion of whoever opens a pull request.
As the Rust codebase grows feature by feature, nothing prevents a pull request from
being merged into `develop` with failing tests.

## Objectif

Check every pull request automatically on GitHub (tests, lint, formatting, release
build), so that a regression is caught and blocks the merge.

## Décisions actées

- **Checks**: tests, clippy lint, rustfmt formatting check, and a release build. Any
  failure fails the CI.
- **Platform**: Linux only. macOS and Windows are out of scope for now.
- **Trigger**: pull requests targeting `develop` only. No run on plain pushes.
- **Blocking**: a red CI blocks the merge into `develop` (required status check via
  branch protection).
