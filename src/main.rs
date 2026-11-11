mod app;
mod frame;
mod input;
mod layout;
mod palette;
mod renderer;
// Wired to mouse input by the selection feature; helpers are tested on their own until then.
#[cfg_attr(not(test), allow(dead_code))]
mod selection;
mod terminal;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    app::run()
}
