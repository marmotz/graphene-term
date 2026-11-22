//! The window of the quick terminal process: its placement on a screen edge, shown and
//! hidden on the commands of its socket.

use winit::dpi::{Position, Size};
use winit::monitor::MonitorHandle;

use super::*;
use crate::config::QuickTerminalConfig;
use crate::quick_terminal::{
    self, BLUR_DELAY, Monitor, PixelRect, Point, QuickCommand, SLIDE_FRAME, Slide,
};

/// Whether the process holds an ordinary terminal window or the quick terminal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum WindowMode {
    Normal,
    Quick(QuickState),
}

/// State of the quick terminal window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct QuickState {
    /// Whether the window is on screen or sliding in; `false` once it starts sliding
    /// out.
    pub shown: bool,
    /// Started by `--quick --hidden`: the window stays hidden until a command shows it.
    pub start_hidden: bool,
    /// The slide in progress.
    pub slide: Option<Slide>,
    /// When the window hides after losing the focus, see [`BLUR_DELAY`].
    pub blur_deadline: Option<Instant>,
    /// Where the window was last shown, which it slides out from.
    pub placed: Option<Placed>,
    /// When the slide last moved the window: each move wakes the event loop again, so
    /// moves are spaced by [`SLIDE_FRAME`].
    pub moved_at: Option<Instant>,
}

/// The two ends of the slides of the window on its current screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Placed {
    pub shown: Point,
    pub hidden: Point,
    pub scale: f64,
}

impl QuickState {
    /// A hidden window, `start_hidden` for `--quick --hidden`.
    pub(super) fn new(start_hidden: bool) -> Self {
        Self {
            shown: false,
            start_hidden,
            slide: None,
            blur_deadline: None,
            placed: None,
            moved_at: None,
        }
    }

    /// The window gains or loses the focus at `now`: a shown window that loses it hides
    /// after [`BLUR_DELAY`] when `hide_on_focus_loss` is set; gaining it back cancels
    /// that.
    pub(super) fn focus_changed(&mut self, focused: bool, now: Instant, hide_on_focus_loss: bool) {
        self.blur_deadline =
            (!focused && hide_on_focus_loss && self.shown).then(|| now + BLUR_DELAY);
    }

    /// Another window of the process, the settings window, takes the focus: the window
    /// stays.
    pub(super) fn other_window_focused(&mut self) {
        self.blur_deadline = None;
    }

    /// Whether the window hides now for having lost the focus; the deadline is spent.
    pub(super) fn blur_due(&mut self, now: Instant) -> bool {
        let due = self.blur_deadline.is_some_and(|deadline| now >= deadline);
        if due {
            self.blur_deadline = None;
        }
        due
    }
}

/// What the slide in progress does at a tick of the event loop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum SlideStep {
    /// No slide.
    Idle,
    /// Too early for the next move: come back then.
    Wait(Instant),
    /// Move the window to `to`; `over` once it is the last move of the slide.
    Move { to: Point, over: bool, hiding: bool },
}

impl QuickState {
    /// Starts `slide`, or ends the slide in progress with `None`.
    pub(super) fn set_slide(&mut self, slide: Option<Slide>) {
        self.slide = slide;
        self.moved_at = None;
    }

    /// The step of the slide at `now`: a move at most every [`SLIDE_FRAME`], and always
    /// the last one, which ends the slide.
    pub(super) fn slide_step(&mut self, now: Instant) -> SlideStep {
        let Some(slide) = self.slide else {
            return SlideStep::Idle;
        };
        let over = slide.is_over(now);
        if !over
            && let Some(next) = self.moved_at.map(|moved| moved + SLIDE_FRAME)
            && now < next
        {
            return SlideStep::Wait(next);
        }
        self.moved_at = Some(now);
        if over {
            self.set_slide(None);
        }
        SlideStep::Move {
            to: slide.position(now),
            over,
            hiding: slide.hiding,
        }
    }
}

/// What closing the last pane, or the window, does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowEnd {
    /// The application exits.
    Exit,
    /// The quick terminal hides and starts over with a fresh tab.
    HideAndReset,
}

impl WindowEnd {
    pub(super) fn of(quick: bool) -> Self {
        if quick {
            Self::HideAndReset
        } else {
            Self::Exit
        }
    }
}

/// Replaces every tab of `workspace` with a single one holding the pane `spawn` makes.
/// `false` when it makes none: the workspace is then left empty.
pub(super) fn reset_workspace<P>(
    workspace: &mut Workspace<P>,
    spawn: impl FnOnce(PaneId) -> Option<P>,
) -> bool {
    workspace.clear();
    let id = workspace.allocate_id();
    match spawn(id) {
        Some(pane) => {
            workspace.new_tab(id, pane);
            true
        }
        None => false,
    }
}

/// `point`, in physical pixels of a screen of `scale`, in the units the platform
/// positions windows with: physical pixels on X11, points on macOS (a window on another
/// screen would convert physical pixels with its own scale).
fn platform_position(point: Point, scale: f64) -> Position {
    let physical = PhysicalPosition::new(point.0, point.1);
    if cfg!(target_os = "macos") {
        physical.to_logical::<f64>(scale).into()
    } else {
        physical.into()
    }
}

/// Where the quick terminal window goes, in the units the platform positions windows
/// with.
#[derive(Debug, Clone, Copy)]
pub(super) struct Placement {
    pub position: Position,
    pub size: Size,
    /// The size in logical pixels, kept as the window's restore size.
    pub logical: LogicalSize<u32>,
    /// The window frame, in physical pixels.
    pub frame: PixelRect,
    /// Scale of the screen of the frame.
    pub scale: f64,
}

impl Placement {
    /// `frame`, in physical pixels of a screen of `scale`. X11 positions windows in
    /// physical pixels; macOS in points, which a window on another screen would convert
    /// with its own scale.
    fn new(frame: PixelRect, scale: f64) -> Self {
        let physical_size = PhysicalSize::new(frame.width, frame.height);
        let size = if cfg!(target_os = "macos") {
            physical_size.to_logical::<f64>(scale).into()
        } else {
            physical_size.into()
        };
        Self {
            position: platform_position((frame.x, frame.y), scale),
            size,
            logical: physical_size.to_logical(scale),
            frame,
            scale,
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
        state_of(&mut self.mode)
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

    /// Places the quick terminal on its screen edge, shows it, gives it the focus and
    /// slides it in from that edge, from where it is when it was sliding out.
    pub(super) fn show_quick(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let config = &self.config.quick_terminal;
        let (edge, full) = (
            config.edge,
            Duration::from_millis(config.animation_ms.into()),
        );
        let placement = placement(event_loop, config);
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let Some(state) = state_of(&mut self.mode) else {
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
            let placed = Placed {
                shown: (placement.frame.x, placement.frame.y),
                hidden: quick_terminal::hidden_origin(placement.frame, edge),
                scale: placement.scale,
            };
            let from = state
                .slide
                .map_or(placed.hidden, |slide| slide.position(now));
            let slide = Slide::new(
                from,
                placed.shown,
                now,
                full,
                (placed.hidden, placed.shown),
                false,
            );
            if slide.is_over(now) {
                window.set_outer_position(placement.position);
                state.set_slide(None);
            } else {
                window.set_outer_position(platform_position(from, placed.scale));
                state.set_slide(Some(slide));
            }
            state.placed = Some(placed);
        }
        window.set_visible(true);
        window.focus_window();
        window.request_redraw();
        state.shown = true;
        state.blur_deadline = None;
    }

    /// Slides the quick terminal out to its edge, from where it is, then hides it.
    pub(super) fn hide_quick(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let full = Duration::from_millis(self.config.quick_terminal.animation_ms.into());
        let Some(state) = state_of(&mut self.mode) else {
            return;
        };
        state.shown = false;
        state.blur_deadline = None;
        let slide = state.placed.map(|placed| {
            let from = state
                .slide
                .map_or(placed.shown, |slide| slide.position(now));
            Slide::new(
                from,
                placed.hidden,
                now,
                full,
                (placed.hidden, placed.shown),
                true,
            )
        });
        match slide {
            Some(slide) if self.session.is_some() && !slide.is_over(now) => {
                state.set_slide(Some(slide));
            }
            _ => self.finish_hide(event_loop),
        }
    }

    /// Hides the window at the end of its slide out; on macOS the focus goes back to
    /// the previous application.
    fn finish_hide(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(state) = state_of(&mut self.mode) {
            state.set_slide(None);
        }
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
    }

    /// Moves the sliding window and hides it once it lost the focus long enough;
    /// returns when to come back.
    pub(super) fn quick_tick(
        &mut self,
        event_loop: &ActiveEventLoop,
        now: Instant,
    ) -> Option<Instant> {
        let state = state_of(&mut self.mode)?;
        if state.blur_due(now) {
            self.hide_quick(event_loop);
        }
        let state = state_of(&mut self.mode)?;
        let scale = state.placed.map_or(1.0, |placed| placed.scale);
        match state.slide_step(now) {
            SlideStep::Idle => {}
            SlideStep::Wait(next) => return Some(next),
            SlideStep::Move { to, over, hiding } => {
                if let Some(session) = self.session.as_ref() {
                    session
                        .window
                        .set_outer_position(platform_position(to, scale));
                }
                if !over {
                    return Some(now + SLIDE_FRAME);
                }
                if hiding {
                    self.finish_hide(event_loop);
                }
            }
        }
        state_of(&mut self.mode)?.blur_deadline
    }

    /// A focus change of the quick terminal window.
    pub(super) fn quick_focus_changed(&mut self, focused: bool) {
        let hide_on_focus_loss = self.config.quick_terminal.hide_on_focus_loss;
        if let Some(state) = state_of(&mut self.mode) {
            state.focus_changed(focused, Instant::now(), hide_on_focus_loss);
        }
    }

    /// After an event of the session: a quick terminal whose last pane, or the window,
    /// was closed hides and starts over with a fresh tab.
    pub(super) fn reset_closed_quick(&mut self, event_loop: &ActiveEventLoop) {
        let closed = self
            .session
            .as_mut()
            .is_some_and(|session| std::mem::take(&mut session.quick_closed));
        if !closed {
            return;
        }
        self.hide_quick(event_loop);
        if let Some(session) = self.session.as_mut()
            && !session.reset_workspace()
        {
            self.fail(
                event_loop,
                anyhow::anyhow!("cannot start a shell for the quick terminal"),
            );
        }
    }
}

/// The quick terminal state of `mode`, `None` for an ordinary window.
fn state_of(mode: &mut WindowMode) -> Option<&mut QuickState> {
    match mode {
        WindowMode::Quick(state) => Some(state),
        WindowMode::Normal => None,
    }
}

impl Session {
    /// Asks the end of the window: the application exits, or the quick terminal
    /// (whose `App` handles it after the event, see [`App::reset_closed_quick`]) hides
    /// and starts over.
    pub(super) fn end_window(&mut self, event_loop: &ActiveEventLoop) {
        match WindowEnd::of(self.quick) {
            WindowEnd::Exit => event_loop.exit(),
            WindowEnd::HideAndReset => self.quick_closed = true,
        }
    }

    /// Replaces the tabs with a single fresh one, its shell in the home directory, and
    /// drops what the old ones left (dialog, gesture, hover). `false` when the shell
    /// cannot start.
    fn reset_workspace(&mut self) -> bool {
        self.overlay = None;
        self.sync_menu_theme();
        self.drag = None;
        self.link_press = None;
        self.hovered_cell = None;
        self.hovered_link = None;
        self.hovered_button = None;
        let geometry = self.geometry();
        let padding = self.padding * self.window.scale_factor() as f32;
        let layout = GridLayout::new(geometry.area, geometry.cell, padding);
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let spawn = self.pane_spawner(home, layout);
        if !reset_workspace(&mut self.workspace, spawn) {
            return false;
        }
        self.sync_badge();
        self.focus_changed(None);
        true
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

    fn shown() -> QuickState {
        QuickState {
            shown: true,
            ..QuickState::new(false)
        }
    }

    #[test]
    fn focus_loss_arms_the_deadline_only_when_asked_and_shown() {
        let now = Instant::now();
        let mut state = shown();
        state.focus_changed(false, now, true);
        assert_eq!(state.blur_deadline, Some(now + BLUR_DELAY));
        assert!(!state.blur_due(now + BLUR_DELAY / 2));
        assert!(state.blur_due(now + BLUR_DELAY));
        // Spent once due.
        assert_eq!(state.blur_deadline, None);
        assert!(!state.blur_due(now + BLUR_DELAY * 2));

        let mut state = shown();
        state.focus_changed(false, now, false);
        assert_eq!(state.blur_deadline, None);

        let mut state = QuickState::new(false);
        state.focus_changed(false, now, true);
        assert_eq!(state.blur_deadline, None);
    }

    #[test]
    fn focus_back_cancels_the_deadline() {
        let now = Instant::now();
        let mut state = shown();
        state.focus_changed(false, now, true);
        state.focus_changed(true, now + BLUR_DELAY / 3, true);
        assert_eq!(state.blur_deadline, None);
        assert!(!state.blur_due(now + BLUR_DELAY * 2));
    }

    #[test]
    fn settings_window_taking_the_focus_cancels_the_deadline() {
        let now = Instant::now();
        let mut state = shown();
        state.focus_changed(false, now, true);
        state.other_window_focused();
        assert!(!state.blur_due(now + BLUR_DELAY * 2));
    }

    #[test]
    fn closing_the_window_exits_except_for_the_quick_terminal() {
        assert_eq!(WindowEnd::of(false), WindowEnd::Exit);
        assert_eq!(WindowEnd::of(true), WindowEnd::HideAndReset);
    }

    #[test]
    fn last_pane_closed_in_quick_mode_starts_over_with_a_fresh_tab() {
        let mut workspace = Workspace::<&str>::new();
        let first = workspace.allocate_id();
        workspace.new_tab(first, "old");
        assert!(workspace.close_pane(first).is_err());
        assert_eq!(WindowEnd::of(true), WindowEnd::HideAndReset);
        assert!(reset_workspace(&mut workspace, |_| Some("fresh")));
        assert_eq!(workspace.tabs().len(), 1);
        let id = workspace.active_pane_id().unwrap();
        assert_ne!(id, first);
        assert_eq!(workspace.active_pane(), Some(&"fresh"));
    }

    #[test]
    fn closed_quick_window_drops_every_tab_for_a_fresh_one() {
        let mut workspace = Workspace::<&str>::new();
        for name in ["one", "two", "three"] {
            let id = workspace.allocate_id();
            workspace.new_tab(id, name);
        }
        assert!(reset_workspace(&mut workspace, |_| Some("fresh")));
        assert_eq!(workspace.tabs().len(), 1);
        assert_eq!(workspace.panes().count(), 1);
        assert_eq!(workspace.active_pane(), Some(&"fresh"));
        // A shell that cannot start leaves no pane.
        assert!(!reset_workspace(&mut workspace, |_| None));
        assert!(workspace.tabs().is_empty());
    }

    #[test]
    fn platform_position_is_physical_on_linux_and_points_on_macos() {
        let position = platform_position((-2880, 0), 2.0);
        assert_eq!(
            position.to_physical::<i32>(2.0),
            PhysicalPosition::new(-2880, 0)
        );
        assert_eq!(
            matches!(position, Position::Logical(_)),
            cfg!(target_os = "macos")
        );
    }

    #[test]
    fn slide_moves_once_per_frame_and_always_ends() {
        let start = Instant::now();
        let full = Duration::from_millis(100);
        let (hidden, shown) = ((0, -400), (0, 0));
        let mut state = QuickState::new(false);
        assert_eq!(state.slide_step(start), SlideStep::Idle);
        state.set_slide(Some(Slide::new(
            hidden,
            shown,
            start,
            full,
            (hidden, shown),
            false,
        )));
        let first = start + Duration::from_millis(1);
        assert!(matches!(
            state.slide_step(first),
            SlideStep::Move { over: false, .. }
        ));
        // Woken again by the move itself: too early.
        assert_eq!(
            state.slide_step(first + Duration::from_millis(5)),
            SlideStep::Wait(first + SLIDE_FRAME)
        );
        assert!(matches!(
            state.slide_step(first + SLIDE_FRAME),
            SlideStep::Move { over: false, .. }
        ));
        // The end is never put off.
        let end = start + full;
        assert_eq!(
            state.slide_step(end),
            SlideStep::Move {
                to: shown,
                over: true,
                hiding: false
            }
        );
        assert_eq!(state.slide, None);
        assert_eq!(state.slide_step(end), SlideStep::Idle);
    }
}
