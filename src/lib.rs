//! Graphene Term: the application, built as a library so the benchmarks can reach its
//! internals. The `graphene` binary only calls [`run`].

mod app;
mod banner;
#[cfg(test)]
mod build_version;
mod config;
mod frame;
mod i18n;
mod input;
mod layout;
mod mouse;
mod opener;
mod palette;
mod perf;
mod renderer;
mod selection;
mod terminal;
mod theme;
mod update;

/// Version of this build, computed by `build.rs`.
pub const VERSION: &str = env!("GRAPHENE_VERSION");

/// Opens the window and runs the event loop until the shell exits.
pub fn run() -> anyhow::Result<()> {
    app::run()
}

/// Starts the startup clock used by the `graphene::perf` logs. Call it first in `main`.
pub fn start_perf_clock() {
    perf::start();
}

/// Internals exercised by `benches/render.rs`. Not a stable API.
#[doc(hidden)]
pub mod bench {
    pub use crate::frame::Frame;
    pub use crate::layout::{CellSize, GridLayout};
    pub use crate::palette::Palette;
    pub use crate::renderer::Headless;
    pub use crate::renderer::scene;
}
