//! Graphene Term: the application, built as a library so the benchmarks can reach its
//! internals. The `graphene` binary only calls [`run`].

mod app;
mod appearance;
mod atomic_file;
mod banner;
mod block_actions;
#[cfg(test)]
mod build_version;
mod chrome;
mod clipboard;
mod config;
mod config_edit;
mod cursor_blink;
mod easter_eggs;
mod fold_animation;
mod frame;
mod frame_cache;
mod graphics;
mod i18n;
mod input;
mod input_editor;
mod keybindings;
mod layout;
mod links;
mod mouse;
mod notifications;
mod opener;
mod palette;
mod pane;
mod perf;
mod process;
mod pty;
mod renderer;
mod selection;
mod settings;
mod shell_integration;
mod signals;
mod terminal;
mod theme;
mod theme_edit;
mod timeline;
mod timeline_selection;
mod update;
mod view;
mod watcher;
mod window_state;
mod workspace;
#[cfg(target_os = "linux")]
mod x11_window;

pub use app::GridSize;

/// Version of this build, computed by `build.rs`.
pub const VERSION: &str = env!("GRAPHENE_VERSION");

/// Opens the window and runs the event loop until its last pane exits. The first pane
/// runs `program` (the program then its arguments) instead of the shell when given;
/// the window is sized to show `grid` when it sets a dimension.
pub fn run(program: Option<Vec<String>>, grid: GridSize) -> anyhow::Result<()> {
    app::run(program, grid)
}

/// Lists and optionally removes unknown entries from the user's configuration file.
pub fn prune_config(yes: bool) -> anyhow::Result<()> {
    config_edit::prune_config(yes)
}

/// Starts the startup clock used by the `graphene::perf` logs. Call it first in `main`.
pub fn start_perf_clock() {
    perf::start();
}

/// Internals exercised by `benches/render.rs`. Not a stable API.
#[doc(hidden)]
pub mod bench {
    pub use crate::frame::Frame;
    pub use crate::frame_cache::FrameCache;
    pub use crate::layout::{CellSize, GridLayout, Rect};
    pub use crate::palette::Palette;
    pub use crate::pty::process;
    pub use crate::renderer::scene;
    pub use crate::renderer::{FontOptions, GlyphKey, Glyphs, Headless, pack_argb32};
    pub use crate::signals::Scanner;
    pub use crate::timeline::Timeline;
    pub use crate::view::{rows as view_rows, scroll_block};
}
