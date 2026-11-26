# `graphene --help` does not show the CLI help: technical design

See [overview.md](overview.md) for the product decisions.

## Diagnosis

The arguments are parsed by hand in `command` ([src/main.rs:82](../../../src/main.rs)).
Each command is found with `options.iter().any(|arg| arg == "...")`, and whatever
matches none falls through to `Command::Run`:

```rust
Ok(if options.iter().any(|arg| arg == "--version") {
    Command::Version
} else if /* --prune-config, --quick-toggle, --quick */ {
    ...
} else {
    Command::Run { program, grid: ..., no_restore: ..., restore_session: ... }
})
```

- `--help` and `-h` are not handled, so `graphene --help` opens a window.
- Nothing checks the arguments against the known options: `graphene --foo`, a typo such
  as `--no-restor`, or a bare word such as `graphene htop` are ignored without a message
  and open a window.

## Retained design

Keep the hand-written parser in `src/main.rs`, with no new dependency.

### Alternatives considered

- **clap**: generates the help and rejects unknown arguments. Rejected: a large
  dependency for about ten options, and the current behavior has to be rebuilt around it:
  `-e` takes every following argument, commands found anywhere in the options take
  priority over each other (`options_before_execute_keep_priority`), and the messages
  and exit codes are already tested.
- **Hand-written parser** (retained): three small additions to the existing function, and
  every existing test keeps its meaning.

### 1. `Command::Help`

Add a `Help` variant to `Command` ([src/main.rs:55](../../../src/main.rs)). In
`command`, once `options` (the arguments before `-e`) are split off, check for `--help`
or `-h` first, before `--version`:

- `graphene --version --help` and `graphene --foo --help` show the help, as most CLIs
  do: asking for help never fails.
- `graphene -e vim --help` still runs `vim --help`, because only `options` are scanned.

`main` prints the help to stdout and returns `Ok(())` (exit code 0), without starting the
logger or a window.

### 2. Rejecting unknown arguments

After the help check, `command` walks `options` once and rejects any argument that is not
one of the known options:

- Flags: `--version`, `--prune-config`, `--yes`, `--quick-toggle`, `--quick`, `--hidden`,
  `--no-restore`.
- Options taking a value: `--columns`, `--lines`, `--restore-session`. The argument after
  them is their value and is skipped by the walk. Checking it stays the job of `count`
  ([src/main.rs:130](../../../src/main.rs)) and `value`
  ([src/main.rs:119](../../../src/main.rs)), which already reject a missing, negative,
  zero or non-numeric value. So `--lines -3` keeps its current message.

An unknown option (`--foo`) and a bare word (`htop`) give the same error:

```text
graphene: unknown argument 'htop'; run 'graphene --help' for the options
```

The message names the argument, so `command` returns `Result<Command, String>` instead of
`Result<Command, &'static str>`. `main` ([src/main.rs:7](../../../src/main.rs)) already
prints `graphene: {error}` and exits with code 2, so it does not change.

Flags that are known but do nothing on their own stay accepted, as today: `--yes` without
`--prune-config`, and `--hidden` without `--quick` (the existing test
`quick_runs_the_quick_terminal_optionally_hidden` checks that `--hidden` alone opens an
ordinary window).

### 3. The help text

A `const HELP: &str` in `src/main.rs`, in English, like the other CLI messages (the i18n
feature puts the CLI out of scope: [overview.md](../../features/i18n/overview.md)).
It lists every option, including those Graphene usually passes to itself:

```text
Usage: graphene [options] [-e <program> [args...]]

Options:
  -e <program> [args...]    Run <program> in the first pane instead of the shell;
                            every argument after it belongs to the program
  --columns <n>             Size the window to <n> columns
  --lines <n>               Size the window to <n> lines
  --no-restore              Open a new window outside the saved session
  --restore-session <file>  Reopen the window saved in <file>
  --quick-toggle            Show or hide the quick terminal, starting it if needed
  --quick                   Run the quick terminal
  --hidden                  With --quick, start with the window hidden
  --prune-config            Remove unknown settings from the configuration file
  --yes                     With --prune-config, apply without asking
  --version                 Print the version and exit
  -h, --help                Print this help and exit
```

## Consequences checked against the code

Graphene passes only known options to itself, so the stricter parsing breaks none of
these launches:

- Session restore: `--restore-session <file>`
  ([src/app/application.rs:1023](../../../src/app/application.rs)).
- Quick terminal autostart: `--quick --hidden`
  ([src/quick_terminal/autostart.rs:16](../../../src/quick_terminal/autostart.rs)).
- Quick terminal start from `--quick-toggle`: `--quick [--hidden]`
  ([src/quick_terminal/control.rs:247](../../../src/quick_terminal/control.rs)).
- The desktop entry runs `graphene` with no argument
  ([packaging/graphene.desktop:6](../../../packaging/graphene.desktop)).

A user launch that passes a mistyped option or an extra word now fails with code 2
instead of opening a window. This is the intended change.

## Documentation

- `docs/en/getting-started.md` and `docs/fr/getting-started.md`, table "Command line
  options" / "Options de la ligne de commande": add a `graphene --help` row, and one
  sentence saying that an unknown argument is an error.
- `docs/dev/architecture.md:50`: add `--help` to the list of the entry point's options.

## Tests

In `src/main.rs` `tests`:

- `--help` and `-h` give `Command::Help`, alone and combined with other options
  (`--version --help`, `--foo -h`).
- `-e vim --help` gives `Command::Run` with `program: ["vim", "--help"]`.
- `--foo`, `--no-restor`, `-x` and a bare word `htop` are errors, and the message
  names the argument.
- The value of `--columns 120` and of `--restore-session <file>` is not taken for an
  unknown argument.
- The existing tests stay green unchanged, except for the `&'static str` to `String`
  error type in their assertions if they compare messages (they use `is_err()` today).

## Découpage en tâches d'implémentation

1. [#263](https://github.com/marmotz/graphene-term/issues/263) Show the CLI help with
   `--help` and reject unknown arguments (parser, help text, docs, tests).
