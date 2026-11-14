fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match command(&args) {
        Command::Version => {
            println!("graphene {}", graphene::VERSION);
            Ok(())
        }
        Command::PruneConfig { yes } => {
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
                .init();
            graphene::prune_config(yes)
        }
        Command::Run => {
            graphene::start_perf_clock();
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
                .init();
            graphene::run()
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Version,
    PruneConfig { yes: bool },
    Run,
}

fn command(args: &[String]) -> Command {
    if args.iter().any(|arg| arg == "--version") {
        Command::Version
    } else if args.iter().any(|arg| arg == "--prune-config") {
        Command::PruneConfig {
            yes: args.iter().any(|arg| arg == "--yes"),
        }
    } else {
        Command::Run
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_command_dispatches_before_the_gui_with_optional_yes() {
        assert_eq!(
            command(&["--prune-config".to_owned()]),
            Command::PruneConfig { yes: false }
        );
        assert_eq!(
            command(&["--prune-config".to_owned(), "--yes".to_owned()]),
            Command::PruneConfig { yes: true }
        );
    }
}
