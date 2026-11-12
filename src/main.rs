fn main() -> anyhow::Result<()> {
    graphene::start_perf_clock();
    if std::env::args().skip(1).any(|arg| arg == "--version") {
        println!("graphene {}", graphene::VERSION);
        return Ok(());
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    graphene::run()
}
