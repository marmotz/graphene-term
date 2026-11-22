//! The window of the quick terminal process: its placement on a screen edge, shown and
//! hidden on the commands of its socket.

use winit::dpi::{Position, Size};
use winit::monitor::MonitorHandle;

use super::*;
use crate::config::QuickTerminalConfig;
use crate::quick_terminal::{self, Monitor, PixelRect, QuickCommand};

/// Whether the process holds an ordinary terminal window or the quick terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowMode {
    Normal,
    Quick(QuickState),
}

/// State of the quick terminal window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct QuickState {
    /// Whether the window is on screen.
    pub shown: bool,
    /// Started by `--quick --hidden`: the window stays hidden until a command shows it.
    pub start_hidden: bool,
}

/// Where the quick terminal window goes, in the units the platform positions windows
/// with.
#[derive(Debug, Clone, Copy)]
pub(super) struct Placement {
    pub position: Position,
    pub size: Size,
    /// The size in logical pixels, kept as the window's restore size.
    pub logical: LogicalSize<u32>,
}

impl Placement {
    /// `frame`, in physical pixels of a screen of `scale`. X11 positions windows in
    /// physical pixels; macOS in points, which a window on another screen would convert
    /// with its own scale.
    fn new(frame: PixelRect, scale: f64) -> Self {
        let physical_position = PhysicalPosition::new(frame.x, frame.y);
        let physical_size = PhysicalSize::new(frame.width, frame.height);
        let logical = physical_size.to_logical(scale);
        if cfg!(target_os = "macos") {
            Self {
                position: physical_position.to_logical::<f64>(scale).into(),
                size: physical_size.to_logical::<f64>(scale).into(),
                logical,
            }
        } else {
            Self {
                position: physical_position.into(),
                size: physical_size.into(),
                logical,
            }
        }
    }
}

/// Position of the mouse cursor in the physical pixels of the desktop.
fn cursor_position(monitors: &[MonitorHandle]) -> Option<(i32, i32)> {
    #[cfg(target_os = "linux")]
    {
        let _ = monitors;
        crate::x11_window::cursor_position()
            .map_err(|error| log::debug!("cannot read the cursor position: {error:#}"))
            .ok()
    }
    #[cfg(target_os = "macos")]
    {
        quick_terminal::macos::cursor_position(monitors)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = monitors;
        None
    }
}

/// The rectangle of `monitor` the quick terminal may cover: the whole screen on Linux
/// (`_NET_WORKAREA` spans every screen, and the window is above the panels), the part
/// the menu bar and the Dock leave on macOS.
fn usable_rect(monitor: &MonitorHandle) -> PixelRect {
    let position = monitor.position();
    let size = monitor.size();
    let rect = PixelRect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    };
    #[cfg(target_os = "macos")]
    let rect = rect.inset(quick_terminal::macos::insets(monitor));
    rect
}

/// Where the quick terminal goes now for `config`, on the screen it designates. `None`
/// when no screen is known.
pub(super) fn placement(
    event_loop: &ActiveEventLoop,
    config: &QuickTerminalConfig,
) -> Option<Placement> {
    let handles: Vec<MonitorHandle> = event_loop.available_monitors().collect();
    let primary = event_loop
        .primary_monitor()
        .and_then(|primary| handles.iter().position(|handle| *handle == primary));
    let monitors: Vec<Monitor> = handles
        .iter()
        .map(|handle| Monitor {
            name: handle.name(),
            rect: usable_rect(handle),
        })
        .collect();
    let index = quick_terminal::pick_monitor(
        &config.screen,
        &monitors,
        primary,
        cursor_position(&handles),
    )?;
    let frame = quick_terminal::frame(monitors[index].rect, config.edge, config.size);
    Some(Placement::new(frame, handles[index].scale_factor()))
}

/// Gives the hidden quick terminal `window` its platform states before it is shown.
pub(super) fn prepare_window(window: &Window) {
    #[cfg(target_os = "linux")]
    if let Err(error) = crate::x11_window::preset_states(window, &crate::x11_window::QUICK) {
        log::warn!("cannot set the quick terminal window states: {error:#}");
    }
    #[cfg(target_os = "macos")]
    quick_terminal::macos::join_all_spaces(window);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = window;
}

impl App {
    /// The quick terminal state, `None` for an ordinary window.
    pub(super) fn quick_state(&mut self) -> Option<&mut QuickState> {
        match &mut self.mode {
            WindowMode::Quick(state) => Some(state),
            WindowMode::Normal => None,
        }
    }

    /// Handles a command of the quick terminal socket.
    pub(super) fn quick_command(&mut self, event_loop: &ActiveEventLoop, command: QuickCommand) {
        let Some(shown) = self.quick_state().map(|state| state.shown) else {
            return;
        };
        match (command, shown) {
            (QuickCommand::Show, _) | (QuickCommand::Toggle, false) => self.show_quick(event_loop),
            (QuickCommand::Hide, _) | (QuickCommand::Toggle, true) => self.hide_quick(event_loop),
        }
    }

    /// Places the quick terminal on its screen edge, shows it and gives it the focus.
    pub(super) fn show_quick(&mut self, event_loop: &ActiveEventLoop) {
        let placement = placement(event_loop, &self.config.quick_terminal);
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let window = &session.window;
        if let Some(placement) = placement {
            let current = window.inner_size();
            let wanted = placement.size.to_physical::<u32>(window.scale_factor());
            if current != wanted {
                // The grid follows at the `Resized` event.
                let _ = window.request_inner_size(placement.size);
            }
            window.set_outer_position(placement.position);
        }
        window.set_visible(true);
        window.focus_window();
        window.request_redraw();
        if let Some(state) = self.quick_state() {
            state.shown = true;
        }
    }

    /// Hides the quick terminal; on macOS the focus goes back to the previous
    /// application.
    pub(super) fn hide_quick(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(session) = self.session.as_ref() {
            session.window.set_visible(false);
        }
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::ActiveEventLoopExtMacOS;
            event_loop.hide_application();
        }
        #[cfg(not(target_os = "macos"))]
        let _ = event_loop;
        if let Some(state) = self.quick_state() {
            state.shown = false;
        }
    }
}

/// Whether the quick terminal can run on the platform of `event_loop`: not under
/// Wayland.
pub(super) fn quick_supported(event_loop: &ActiveEventLoop) -> bool {
    #[cfg(target_os = "linux")]
    {
        use winit::platform::wayland::ActiveEventLoopExtWayland;
        !event_loop.is_wayland()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = event_loop;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_keeps_physical_pixels_on_linux_and_points_on_macos() {
        let frame = PixelRect {
            x: 2880,
            y: 0,
            width: 2880,
            height: 720,
        };
        let placement = Placement::new(frame, 2.0);
        assert_eq!(placement.logical, LogicalSize::new(1440, 360));
        let position = placement.position.to_physical::<i32>(2.0);
        let size = placement.size.to_physical::<u32>(2.0);
        assert_eq!(position, PhysicalPosition::new(2880, 0));
        assert_eq!(size, PhysicalSize::new(2880, 720));
        assert_eq!(
            matches!(placement.position, Position::Logical(_)),
            cfg!(target_os = "macos")
        );
    }
}
