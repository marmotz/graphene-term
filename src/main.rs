use std::path::PathBuf;

use graphene::GridSize;

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let command = match command(&args) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("graphene: {error}");
            std::process::exit(2);
        }
    };
    match command {
        Command::Help => {
            print!("{HELP}");
            Ok(())
        }
        Command::Version => {
            println!("graphene {}", graphene::VERSION);
            Ok(())
        }
        Command::PruneConfig { yes } => {
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
                .init();
            graphene::prune_config(yes)
        }
        Command::QuickToggle => match graphene::quick_toggle() {
            Ok(()) => Ok(()),
            Err(message) => {
                eprintln!("graphene: {message}");
                std::process::exit(1);
            }
        },
        Command::Quick { hidden } => {
            graphene::start_perf_clock();
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
                .init();
            match graphene::run_quick(hidden)? {
                0 => Ok(()),
                code => std::process::exit(code),
            }
        }
        Command::Run {
            program,
            grid,
            no_restore,
            restore_session,
        } => {
            graphene::start_perf_clock();
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
                .init();
            graphene::run(program, grid, no_restore, restore_session)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    /// Prints the command line help.
    Help,
    Version,
    PruneConfig {
        yes: bool,
    },
    /// Shows or hides the quick terminal, starting it when it does not run.
    QuickToggle,
    /// Runs the resident quick terminal process (internal, started by `QuickToggle`),
    /// its window hidden until a command shows it when `hidden`.
    Quick {
        hidden: bool,
    },
    /// Opens the window; its first pane runs `program` instead of the shell when given,
    /// and the window is sized to `grid` when it sets a dimension. With `no_restore`
    /// (`--no-restore`), the window stays out of the saved session. `restore_session`
    /// (`--restore-session <file>`, internal) is a saved window handed over by the
    /// launch that restored the session, reopened by this one.
    Run {
        program: Option<Vec<String>>,
        grid: GridSize,
        no_restore: bool,
        restore_session: Option<PathBuf>,
    },
}

/// Command line help, printed by `--help` and `-h`.
const HELP: &str = "\
Usage: graphene [options] [-e <program> [args...]]

Options:
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

Running a program:
  -e <program> [args...]    Run <program> in the first pane instead of the shell.
                            -e must come last: every argument after it, options
                            included, is passed to <program>, not to graphene.
                            Example: `graphene -e vim --help` runs `vim --help`.
";

/// Options without a value.
const FLAGS: [&str; 7] = [
    "--version",
    "--prune-config",
    "--yes",
    "--quick-toggle",
    "--quick",
    "--hidden",
    "--no-restore",
];

/// Options followed by a value, checked by `count` and `value`.
const VALUED: [&str; 3] = ["--columns", "--lines", "--restore-session"];

/// Parses the arguments. `-e` takes every following argument as the program and its
/// arguments, so options after it belong to the program. `--help` wins over every other
/// option, and any argument before `-e` that is not an option is an error.
fn command(args: &[String]) -> Result<Command, String> {
    let (options, program) = match args.iter().position(|arg| arg == "-e") {
        Some(index) => {
            let program = &args[index + 1..];
            if program.is_empty() {
                return Err("-e requires a program to run".to_owned());
            }
            (&args[..index], Some(program.to_vec()))
        }
        None => (args, None),
    };
    if options.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(Command::Help);
    }
    let mut walk = options.iter();
    while let Some(arg) = walk.next() {
        if VALUED.contains(&arg.as_str()) {
            walk.next();
        } else if !FLAGS.contains(&arg.as_str()) {
            return Err(format!(
                "unknown argument '{arg}'; run 'graphene --help' for the options"
            ));
        }
    }
    Ok(if options.iter().any(|arg| arg == "--version") {
        Command::Version
    } else if options.iter().any(|arg| arg == "--prune-config") {
        Command::PruneConfig {
            yes: options.iter().any(|arg| arg == "--yes"),
        }
    } else if options.iter().any(|arg| arg == "--quick-toggle") {
        Command::QuickToggle
    } else if options.iter().any(|arg| arg == "--quick") {
        Command::Quick {
            hidden: options.iter().any(|arg| arg == "--hidden"),
        }
    } else {
        Command::Run {
            program,
            grid: GridSize {
                columns: count(options, "--columns").map_err(str::to_owned)?,
                lines: count(options, "--lines").map_err(str::to_owned)?,
            },
            no_restore: options.iter().any(|arg| arg == "--no-restore"),
            restore_session: value(options, "--restore-session")
                .map_err(str::to_owned)?
                .map(PathBuf::from),
        }
    })
}

/// Value of the option `name`, given as the next argument.
fn value<'a>(options: &'a [String], name: &str) -> Result<Option<&'a str>, &'static str> {
    let Some(index) = options.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    match options.get(index + 1) {
        Some(value) if !value.starts_with('-') => Ok(Some(value)),
        _ => Err("--restore-session requires a file"),
    }
}

/// Value of the option `name`, a count of cells given as the next argument.
fn count(options: &[String], name: &str) -> Result<Option<u16>, &'static str> {
    let Some(index) = options.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    match options.get(index + 1).map(|value| value.parse::<u16>()) {
        Some(Ok(value)) if value > 0 => Ok(Some(value)),
        _ => Err("--columns and --lines require a number of cells from 1 to 65535"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn help_wins_over_every_other_option() {
        for list in [
            &["--help"][..],
            &["-h"],
            &["--version", "--help"],
            &["--foo", "-h"],
            &["--help", "-e", "vim"],
        ] {
            assert_eq!(command(&args(list)), Ok(Command::Help), "{list:?}");
        }
    }

    #[test]
    fn help_after_execute_belongs_to_the_program() {
        assert_eq!(
            command(&args(&["-e", "vim", "--help"])),
            Ok(Command::Run {
                program: Some(args(&["vim", "--help"])),
                grid: GridSize::default(),
                no_restore: false,
                restore_session: None
            })
        );
    }

    #[test]
    fn unknown_arguments_are_errors_naming_them() {
        for unknown in ["--foo", "--no-restor", "-x", "htop"] {
            assert_eq!(
                command(&args(&[unknown])),
                Err(format!(
                    "unknown argument '{unknown}'; run 'graphene --help' for the options"
                ))
            );
        }
        assert!(command(&args(&["--version", "--foo"])).is_err());
        assert!(command(&args(&["htop", "-e", "true"])).is_err());
    }

    #[test]
    fn option_values_are_not_unknown_arguments() {
        assert!(command(&args(&["--columns", "120"])).is_ok());
        assert!(command(&args(&["--restore-session", "/state/pending-0.json"])).is_ok());
    }

    #[test]
    fn help_lists_every_option() {
        for option in FLAGS.iter().chain(&VALUED).chain(&["-e", "--help", "-h"]) {
            assert!(HELP.contains(option), "{option} missing from the help");
        }
    }

    #[test]
    fn help_shows_that_options_after_execute_go_to_the_program() {
        // The example in the help matches `help_after_execute_belongs_to_the_program`.
        assert!(HELP.contains("`graphene -e vim --help` runs `vim --help`"));
        assert!(HELP.contains("-e must come last"));
    }

    #[test]
    fn prune_command_dispatches_before_the_gui_with_optional_yes() {
        assert_eq!(
            command(&args(&["--prune-config"])),
            Ok(Command::PruneConfig { yes: false })
        );
        assert_eq!(
            command(&args(&["--prune-config", "--yes"])),
            Ok(Command::PruneConfig { yes: true })
        );
    }

    #[test]
    fn quick_toggle_dispatches_before_the_gui() {
        assert_eq!(
            command(&args(&["--quick-toggle"])),
            Ok(Command::QuickToggle)
        );
    }

    #[test]
    fn quick_runs_the_quick_terminal_optionally_hidden() {
        assert_eq!(
            command(&args(&["--quick"])),
            Ok(Command::Quick { hidden: false })
        );
        assert_eq!(
            command(&args(&["--quick", "--hidden"])),
            Ok(Command::Quick { hidden: true })
        );
        // `--hidden` alone is not an option of an ordinary window.
        assert_eq!(
            command(&args(&["--hidden"])),
            Ok(Command::Run {
                program: None,
                grid: GridSize::default(),
                no_restore: false,
                restore_session: None
            })
        );
    }

    #[test]
    fn no_argument_runs_the_shell() {
        assert_eq!(
            command(&[]),
            Ok(Command::Run {
                program: None,
                grid: GridSize::default(),
                no_restore: false,
                restore_session: None
            })
        );
    }

    #[test]
    fn execute_takes_the_program_and_its_arguments() {
        assert_eq!(
            command(&args(&["-e", "cat", "file.txt"])),
            Ok(Command::Run {
                program: Some(args(&["cat", "file.txt"])),
                grid: GridSize::default(),
                no_restore: false,
                restore_session: None
            })
        );
    }

    #[test]
    fn execute_without_a_program_is_an_error() {
        assert!(command(&args(&["-e"])).is_err());
    }

    #[test]
    fn options_after_execute_belong_to_the_program() {
        assert_eq!(
            command(&args(&["-e", "vim", "--version", "--prune-config"])),
            Ok(Command::Run {
                program: Some(args(&["vim", "--version", "--prune-config"])),
                grid: GridSize::default(),
                no_restore: false,
                restore_session: None
            })
        );
    }

    #[test]
    fn options_before_execute_keep_priority() {
        assert_eq!(
            command(&args(&["--version", "-e", "true"])),
            Ok(Command::Version)
        );
        assert_eq!(
            command(&args(&["--prune-config", "-e", "true"])),
            Ok(Command::PruneConfig { yes: false })
        );
    }

    #[test]
    fn grid_options_set_the_window_size_in_cells() {
        assert_eq!(
            command(&args(&["--columns", "120", "--lines", "36", "-e", "top"])),
            Ok(Command::Run {
                program: Some(args(&["top"])),
                grid: GridSize {
                    columns: Some(120),
                    lines: Some(36)
                },
                no_restore: false,
                restore_session: None
            })
        );
        assert_eq!(
            command(&args(&["--lines", "40"])),
            Ok(Command::Run {
                program: None,
                grid: GridSize {
                    columns: None,
                    lines: Some(40)
                },
                no_restore: false,
                restore_session: None
            })
        );
    }

    #[test]
    fn no_restore_keeps_the_window_out_of_the_session() {
        assert_eq!(
            command(&args(&["--no-restore"])),
            Ok(Command::Run {
                program: None,
                grid: GridSize::default(),
                no_restore: true,
                restore_session: None
            })
        );
        // After `-e`, it belongs to the program.
        assert_eq!(
            command(&args(&["-e", "tool", "--no-restore"])),
            Ok(Command::Run {
                program: Some(args(&["tool", "--no-restore"])),
                grid: GridSize::default(),
                no_restore: false,
                restore_session: None
            })
        );
    }

    #[test]
    fn restore_session_reopens_a_handed_over_window() {
        assert_eq!(
            command(&args(&[
                "--restore-session",
                "/state/sessions/pending-0.json"
            ])),
            Ok(Command::Run {
                program: None,
                grid: GridSize::default(),
                no_restore: false,
                restore_session: Some(PathBuf::from("/state/sessions/pending-0.json"))
            })
        );
        assert!(command(&args(&["--restore-session"])).is_err());
        assert!(command(&args(&["--restore-session", "--no-restore"])).is_err());
        // After `-e`, it belongs to the program.
        assert_eq!(
            command(&args(&["-e", "tool", "--restore-session", "x"])),
            Ok(Command::Run {
                program: Some(args(&["tool", "--restore-session", "x"])),
                grid: GridSize::default(),
                no_restore: false,
                restore_session: None
            })
        );
    }

    #[test]
    fn grid_options_need_a_positive_count() {
        for invalid in [
            &["--columns"][..],
            &["--columns", "0"],
            &["--lines", "-3"],
            &["--lines", "many"],
            &["--columns", "70000"],
            &["--columns", "-e", "true"],
        ] {
            assert!(command(&args(invalid)).is_err(), "{invalid:?} accepted");
        }
    }

    #[test]
    fn grid_options_after_execute_belong_to_the_program() {
        assert_eq!(
            command(&args(&["-e", "less", "--lines", "5"])),
            Ok(Command::Run {
                program: Some(args(&["less", "--lines", "5"])),
                grid: GridSize::default(),
                no_restore: false,
                restore_session: None
            })
        );
    }
}
