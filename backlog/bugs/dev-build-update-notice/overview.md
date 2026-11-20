# Dev builds report a new version as available

**Statut** : terminé, voir [technical.md](technical.md)

## Contexte

When running a locally built version (`cargo run`), the application reports that a new
version is available, even though the dev build is by definition the most recent code.
The update notice is wrong and noisy for anyone working on the project.

Observed with a local build of `0.1.3` while `v0.1.3` is the latest published release:
the local build carries the version `0.1.3-dev.<timestamp>`, which semver orders before
`0.1.3`, so the published release is seen as newer.

## Objectif

Dev builds must be distinguishable from published releases, so that a dev build no
longer reports a published release as a newer version.

## Décisions actées

- Dev builds keep checking for updates.
- A dev build of version `X.Y.Z` counts as at least as recent as the published `X.Y.Z`:
  it is only notified of a release strictly greater than `X.Y.Z`.
- Published (stable) builds keep their current behavior.
- No extra dev indicator in the UI: the version string (`X.Y.Z-dev.<timestamp>`) is
  enough to tell a dev build from a published release.
