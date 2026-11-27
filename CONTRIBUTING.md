# Contributing to Graphene Term

Thanks for your interest in Graphene. Bug reports, ideas and pull requests are welcome.

## Reporting a problem

Open an [issue](https://github.com/marmotz/graphene-term/issues) with:

- your system (distribution or macOS version, Wayland or X11) and Graphene's version
  (`graphene --version`);
- what you did, what you expected, what happened;
- the logs when relevant: run `RUST_LOG=debug graphene` from another terminal.

## Prerequisites

- **Rust** (stable), installed with [rustup](https://rustup.rs). The repository pins
  the stable toolchain in `rust-toolchain.toml`, so rustup picks it automatically.
- **Git** and a **C compiler**: `build-essential` on Debian and Ubuntu, `gcc` on
  Fedora, the Xcode Command Line Tools on macOS (`xcode-select --install`).
- **Python 3** to run the scripts' tests and the terminal benchmark (optional).

## Build and run

```bash
git clone https://github.com/marmotz/graphene-term.git
cd graphene-term
cargo run                    # debug build
cargo run --release          # optimized build
```

## Before opening a pull request

Run the same checks as the CI:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

- Add or update tests with every change: unit tests next to the code in a
  `#[cfg(test)] mod tests`, integration tests in `tests/`. See
  [Tests](docs/dev/development.md#tests).
- When you add or change a configuration key, update `config.default.toml` and the
  configuration pages ([en](docs/en/configuration.md), [fr](docs/fr/configuration.md)).
- When you change a user-visible behaviour, update the
  [user documentation](docs/en/README.md) in English and French.

## Translations

The texts of the interface are in the Fluent files of [`i18n/`](i18n/). To review a
language, fix a translation or mark a language as reviewed, see
[Translating Graphene](i18n/CONTRIBUTING.md).

## Branches and pull requests

- `develop` is the default branch: create your branch from `develop` and open your
  pull request against `develop`. `main` only receives releases.
- Write commit messages in the [Conventional Commits](https://www.conventionalcommits.org)
  format: `feat: ...`, `fix: ...`, `docs: ...`, `refactor: ...`, `test: ...`. The
  release notes are generated from these prefixes (see
  [Releases](docs/dev/development.md#releases)).
- Keep a pull request to one subject, and describe what it changes and how you tested
  it.

## Contributor documentation

- [Development guide](docs/dev/development.md): toolchain, commands, tests,
  benchmarks, releases.
- [Architecture](docs/dev/architecture.md): modules, threads, data flow, rendering.
- [Technical decisions](docs/dev/decisions.md): the choices made and why.
- [Vision and roadmap](docs/dev/vision.md).

## License

By contributing, you agree that your contributions are licensed under the same terms as
the project: [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
