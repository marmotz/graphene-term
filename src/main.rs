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
        Command::Version => {
            println!("graphene {}", graphene::VERSION);
            Ok(())
        }
        Command::PruneConfig { yes } => {
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
                .init();
            graphene::prune_config(yes)
        }
        Command::Run { program, grid } => {
            graphene::start_perf_clock();
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
                .init();
            graphene::run(program, grid)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Version,
    PruneConfig {
        yes: bool,
    },
    /// Opens the window; its first pane runs `program` instead of the shell when given,
    /// and the window is sized to `grid` when it sets a dimension.
    Run {
        program: Option<Vec<String>>,
        grid: GridSize,
    },
}

/// Parses the arguments. `-e` takes every following argument as the program and its
/// arguments, so options after it belong to the program.
fn command(args: &[String]) -> Result<Command, &'static str> {
    let (options, program) = match args.iter().position(|arg| arg == "-e") {
        Some(index) => {
            let program = &args[index + 1..];
            if program.is_empty() {
                return Err("-e requires a program to run");
            }
            (&args[..index], Some(program.to_vec()))
        }
        None => (args, None),
    };
    Ok(if options.iter().any(|arg| arg == "--version") {
        Command::Version
    } else if options.iter().any(|arg| arg == "--prune-config") {
        Command::PruneConfig {
            yes: options.iter().any(|arg| arg == "--yes"),
        }
    } else {
        Command::Run {
            program,
            grid: GridSize {
                columns: count(options, "--columns")?,
                lines: count(options, "--lines")?,
            },
        }
    })
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
    fn no_argument_runs_the_shell() {
        assert_eq!(
            command(&[]),
            Ok(Command::Run {
                program: None,
                grid: GridSize::default()
            })
        );
    }

    #[test]
    fn execute_takes_the_program_and_its_arguments() {
        assert_eq!(
            command(&args(&["-e", "cat", "file.txt"])),
            Ok(Command::Run {
                program: Some(args(&["cat", "file.txt"])),
                grid: GridSize::default()
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
                grid: GridSize::default()
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
                }
            })
        );
        assert_eq!(
            command(&args(&["--lines", "40"])),
            Ok(Command::Run {
                program: None,
                grid: GridSize {
                    columns: None,
                    lines: Some(40)
                }
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
                grid: GridSize::default()
            })
        );
    }
}
