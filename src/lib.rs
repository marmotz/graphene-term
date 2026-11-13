//! Graphene Term: the application, built as a library so the benchmarks can reach its
//! internals. The `graphene` binary only calls [`run`].

mod app;
mod banner;
mod block_actions;
mod blocks;
#[cfg(test)]
mod build_version;
mod config;
mod frame;
mod i18n;
mod input;
mod layout;
mod mouse;
mod opener;
mod osc133;
mod palette;
mod perf;
mod pty;
mod renderer;
mod selection;
mod shell_integration;
// Used once the PTY loop switches to the timeline (#77).
#[allow(dead_code)]
mod signals;
mod terminal;
// Used once the PTY loop switches to the timeline (#77).
mod theme;
#[allow(dead_code)]
mod timeline;
mod update;
mod view;

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
    pub use crate::blocks::ShellState;
    pub use crate::frame::Frame;
    pub use crate::layout::{CellSize, GridLayout};
    pub use crate::palette::Palette;
    pub use crate::renderer::scene;
    pub use crate::renderer::{GlyphKey, Glyphs, Headless};
    pub use crate::view::rows as view_rows;
}
