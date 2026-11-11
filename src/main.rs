mod app;
mod frame;
mod input;
mod layout;
mod palette;
mod renderer;
mod terminal;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    app::run()
}
