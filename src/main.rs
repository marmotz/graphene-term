mod app;
mod banner;
#[cfg(test)]
mod build_version;
mod config;
mod frame;
mod i18n;
mod input;
mod layout;
mod opener;
mod palette;
mod renderer;
mod selection;
mod terminal;
mod theme;
mod update;

/// Version of this build, computed by `build.rs`.
pub const VERSION: &str = env!("GRAPHENE_VERSION");

fn main() -> anyhow::Result<()> {
    if std::env::args().skip(1).any(|arg| arg == "--version") {
        println!("graphene {VERSION}");
        return Ok(());
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    app::run()
}
