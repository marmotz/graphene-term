//! Interface drawn around and between the panes, laid out without GPU access.

pub mod command_palette;
#[allow(dead_code)] // Runtime integration is tracked by issue #103.
pub mod context_menu;
pub mod dialog;
pub mod scene;
pub mod tab_bar;
pub mod tooltip;
