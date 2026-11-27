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
mod hotkey;
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
mod pane_search;
mod perf;
mod process;
mod progress;
mod pty;
mod quick_terminal;
mod renderer;
mod search;
mod selection;
mod session;
mod settings;
mod shell_integration;
mod signals;
mod taskbar;
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

pub use app::{GridSize, Program};

/// Version of this build, computed by `build.rs`.
pub const VERSION: &str = env!("GRAPHENE_VERSION");

/// Opens the window and runs the event loop until its last pane exits. The first pane
/// runs `program` instead of the shell when given, and stays open once it ends when
/// `program.hold` (`-E`);
/// the window is sized to show `grid` when it sets a dimension. `no_restore` keeps the
/// window out of the saved session (`--no-restore`). `restore_session` is a saved
/// window handed over by the launch that restored the session (`--restore-session`).
pub fn run(
    program: Option<Program>,
    grid: GridSize,
    no_restore: bool,
    restore_session: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    app::run(program, grid, no_restore, restore_session)
}

/// Runs the resident quick terminal process (`graphene --quick`), hidden at startup
/// when `hidden`, and returns its exit code: `4` when the quick terminal is disabled,
/// `3` under Wayland, where it cannot run.
/// Exits at once, with `0`, when another quick terminal process runs, after asking it
/// to toggle its window unless `hidden`.
pub fn run_quick(hidden: bool) -> anyhow::Result<i32> {
    app::run_quick(hidden)
}

/// The message to print for the exit code `code` of [`run_quick`], if any.
pub fn quick_exit_message(code: i32) -> Option<String> {
    (code == quick_terminal::control::DISABLED_EXIT_CODE)
        .then(|| quick_terminal::control::ToggleError::Disabled.to_string())
}

/// `graphene --quick-toggle`: shows or hides the quick terminal, starting it when it
/// does not run. The error is the message to print. A disabled quick terminal fails
/// before the socket is touched or a process started.
pub fn quick_toggle() -> Result<(), String> {
    let (config, _) = config::load();
    toggle_gate(&config.quick_terminal)?;
    let dir = config::runtime_dir().map_err(|error| format!("no runtime folder: {error}"))?;
    quick_terminal::control::toggle(&dir).map_err(|error| error.to_string())
}

/// Whether `--quick-toggle` may go on for `quick`: the error message when the quick
/// terminal is disabled.
fn toggle_gate(quick: &config::QuickTerminalConfig) -> Result<(), String> {
    if quick.enabled {
        Ok(())
    } else {
        Err(quick_terminal::control::ToggleError::Disabled.to_string())
    }
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
    pub use crate::renderer::{FontOptions, GlyphKey, Glyphs, GridSize, Headless, pack_argb32};
    pub use crate::search::{Query, Searcher};
    pub use crate::signals::Scanner;
    pub use crate::timeline::Timeline;
    pub use crate::view::{rows as view_rows, scroll_block};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_fails_with_the_activation_message_when_disabled() {
        let mut quick = config::Config::default().quick_terminal;
        quick.enabled = false;
        assert_eq!(
            toggle_gate(&quick),
            Err(quick_terminal::control::ToggleError::Disabled.to_string())
        );
        quick.enabled = true;
        assert_eq!(toggle_gate(&quick), Ok(()));
    }

    #[test]
    fn only_the_disabled_exit_code_has_a_message() {
        assert_eq!(
            quick_exit_message(quick_terminal::control::DISABLED_EXIT_CODE),
            Some(quick_terminal::control::ToggleError::Disabled.to_string())
        );
        assert_eq!(quick_exit_message(0), None);
        assert_eq!(
            quick_exit_message(quick_terminal::control::WAYLAND_EXIT_CODE),
            None
        );
    }
}
