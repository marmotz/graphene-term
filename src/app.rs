//! Window and event handling.

use std::sync::Arc;
use std::time::Instant;

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::{Column, Point, Side};
use alacritty_terminal::term::{self, TermMode};
use arboard::{GetExtLinux, LinuxClipboardKind, SetExtLinux};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::config::{self, Config, MAX_FONT_SIZE, MIN_FONT_SIZE};
use crate::frame::Frame;
use crate::input::{self, Action};
use crate::layout::GridLayout;
use crate::palette::Palette;
use crate::renderer::Renderer;
use crate::selection::{self, ClickTracker, SelectionSnapshot};
use crate::terminal::{EventProxy, Terminal, UserEvent};
use crate::update::Release;
use crate::update::checker::Checker;

const DEFAULT_TITLE: &str = "Graphene Term";

struct Session {
    window: Arc<Window>,
    renderer: Renderer,
    terminal: Terminal,
    layout: GridLayout,
    palette: Palette,
    clipboard: Option<arboard::Clipboard>,
    focused: bool,
    modifiers: ModifiersState,
    font_size: f32,
    /// Minimum padding around the grid, in logical pixels.
    padding: f32,
    /// Fractional lines accumulated from touchpad scrolling.
    scroll_accumulator: f64,
    /// Last known cursor position, in physical pixels.
    mouse_position: PhysicalPosition<f64>,
    /// A selection drag is in progress.
    left_pressed: bool,
    click_tracker: ClickTracker,
    /// When the next auto-scroll step is due during a drag past the grid edges.
    auto_scroll_deadline: Option<Instant>,
    /// Finished selection kept while its text is unchanged.
    selection_snapshot: Option<SelectionSnapshot>,
    /// Newer release reported by the update check thread.
    #[allow(dead_code, reason = "drawn by the update banner task")]
    update: Option<Release>,
}

impl Session {
    fn compute_layout(&self) -> GridLayout {
        let size = self.window.inner_size();
        let scale = self.window.scale_factor() as f32;
        GridLayout::new(
            size.width,
            size.height,
            self.renderer.cell_size(),
            self.padding * scale,
        )
    }

    /// Recomputes the grid and resizes the PTY when the cell count changed.
    fn relayout(&mut self) {
        let layout = self.compute_layout();
        let changed = (layout.columns, layout.lines) != (self.layout.columns, self.layout.lines);
        self.layout = layout;
        if changed {
            self.terminal.resize(layout.window_size());
        }
        self.window.request_redraw();
    }

    fn apply_font_size(&mut self, font_size: f32) {
        self.font_size = font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        let scale = self.window.scale_factor() as f32;
        self.renderer.set_font_size(self.font_size * scale);
        self.relayout();
    }

    fn mode(&self) -> TermMode {
        *self.terminal.term.lock().mode()
    }

    fn scroll_display(&self, scroll: Scroll) {
        self.terminal.term.lock().scroll_display(scroll);
        self.window.request_redraw();
    }

    fn paste(&mut self) {
        let Some(text) = self.clipboard.as_mut().and_then(|c| c.get_text().ok()) else {
            return;
        };
        self.send_paste(&text);
    }

    /// Sends a paste to the shell. Like a keystroke, it clears the selection.
    fn send_paste(&mut self, text: &str) {
        self.clear_selection();
        let bytes = input::paste_bytes(text, self.mode());
        self.scroll_display(Scroll::Bottom);
        self.terminal.write(bytes);
    }

    /// Writes the cleaned selection to the clipboard. Without selection, the shortcut is
    /// still swallowed so it never sends `^C`.
    fn copy(&mut self) {
        let Some(text) = selection::selected_text(&self.terminal.term.lock()) else {
            return;
        };
        if let Some(Err(error)) = self.clipboard.as_mut().map(|c| c.set_text(text)) {
            log::warn!("copy failed: {error}");
        }
    }

    fn clear_selection(&mut self) {
        self.selection_snapshot = None;
        let mut term = self.terminal.term.lock();
        if term.selection.take().is_some() {
            self.window.request_redraw();
        }
    }

    /// After new PTY output: restores a selection `Term` erased while its text was
    /// redrawn unchanged, and clears it when the selected text really changed.
    fn validate_selection(&mut self) {
        let Some(snapshot) = self.selection_snapshot.as_mut() else {
            return;
        };
        if !snapshot.validate(&mut self.terminal.term.lock()) {
            self.selection_snapshot = None;
        }
    }

    /// Grid point and cell side under the mouse cursor.
    fn mouse_point(&self, display_offset: usize) -> (Point, Side) {
        let (column, line, side) = self
            .layout
            .point_at(self.mouse_position.x as f32, self.mouse_position.y as f32);
        let point = term::viewport_to_point(display_offset, Point::new(line, Column(column)));
        (point, side)
    }

    fn mouse_input(&mut self, state: ElementState, button: MouseButton) {
        match (button, state) {
            (MouseButton::Left, ElementState::Pressed) => self.left_press(),
            (MouseButton::Left, ElementState::Released) => self.left_release(),
            (MouseButton::Middle, ElementState::Pressed)
                if !selection::mouse_reserved(self.mode(), self.modifiers) =>
            {
                self.paste_primary()
            }
            _ => {}
        }
    }

    fn left_press(&mut self) {
        let mut term = self.terminal.term.lock();
        if selection::mouse_reserved(*term.mode(), self.modifiers) {
            return;
        }
        let (point, side) = self.mouse_point(term.grid().display_offset());
        let clicks = self.click_tracker.click(point, Instant::now());
        // The selection changes until the release: no text to keep meanwhile.
        self.selection_snapshot = None;
        selection::start_selection(&mut term, point, side, clicks, self.modifiers);
        self.left_pressed = true;
        self.window.request_redraw();
    }

    fn left_release(&mut self) {
        if !std::mem::take(&mut self.left_pressed) {
            return;
        }
        let text = {
            let mut term = self.terminal.term.lock();
            let text = selection::finish_selection(&mut term);
            self.selection_snapshot = SelectionSnapshot::capture(&term);
            text
        };
        self.window.request_redraw();
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            self.set_primary(text);
        }
    }

    fn cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        self.mouse_position = position;
        if self.left_pressed {
            self.update_selection(&mut self.terminal.term.lock());
        }
    }

    /// Moves the selection end to the cell under the mouse cursor.
    fn update_selection(&self, term: &mut term::Term<EventProxy>) {
        let (point, side) = self.mouse_point(term.grid().display_offset());
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side);
            self.window.request_redraw();
        }
    }

    /// Auto-scroll while dragging a selection past the top or bottom edge: once per
    /// `AUTO_SCROLL_INTERVAL`, scrolls the display and moves the selection end onto the
    /// edge row. Returns when the next step is due, `None` when auto-scroll is idle.
    fn auto_scroll(&mut self, now: Instant) -> Option<Instant> {
        let mut term = self.terminal.term.lock();
        let overflow = self.layout.vertical_overflow(self.mouse_position.y as f32);
        let delta = selection::auto_scroll_delta(overflow, *term.mode());
        if !self.left_pressed || delta == 0 || term.selection.is_none() {
            self.auto_scroll_deadline = None;
            return None;
        }
        match self.auto_scroll_deadline {
            Some(deadline) if now < deadline => {}
            _ => {
                term.scroll_display(Scroll::Delta(delta));
                self.update_selection(&mut term);
                self.auto_scroll_deadline = Some(now + selection::AUTO_SCROLL_INTERVAL);
            }
        }
        self.auto_scroll_deadline
    }

    fn set_primary(&mut self, text: String) {
        let Some(clipboard) = self.clipboard.as_mut() else {
            return;
        };
        // Some Wayland compositors do not support the primary selection.
        if let Err(error) = clipboard
            .set()
            .clipboard(LinuxClipboardKind::Primary)
            .text(text)
        {
            log::debug!("primary selection unavailable: {error}");
        }
    }

    fn paste_primary(&mut self) {
        let Some(clipboard) = self.clipboard.as_mut() else {
            return;
        };
        match clipboard
            .get()
            .clipboard(LinuxClipboardKind::Primary)
            .text()
        {
            Ok(text) => self.send_paste(&text),
            Err(error) => log::debug!("primary selection unavailable: {error}"),
        }
    }

    fn run_action(&mut self, action: Action, default_font_size: f32) {
        match action {
            Action::Copy => self.copy(),
            Action::Paste => self.paste(),
            Action::ScrollPageUp => self.scroll_display(Scroll::PageUp),
            Action::ScrollPageDown => self.scroll_display(Scroll::PageDown),
            Action::ScrollToTop => self.scroll_display(Scroll::Top),
            Action::ScrollToBottom => self.scroll_display(Scroll::Bottom),
            Action::FontSizeIncrease => self.apply_font_size(self.font_size + 1.0),
            Action::FontSizeDecrease => self.apply_font_size(self.font_size - 1.0),
            Action::FontSizeReset => self.apply_font_size(default_font_size),
        }
    }

    fn mouse_wheel(&mut self, delta: MouseScrollDelta) {
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, y) => y as f64 * 3.0,
            MouseScrollDelta::PixelDelta(pos) => pos.y / self.layout.cell.height as f64,
        };
        self.scroll_accumulator += lines;
        let whole = self.scroll_accumulator.trunc() as i32;
        if whole == 0 {
            return;
        }
        self.scroll_accumulator -= whole as f64;

        let mode = self.mode();
        if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            // Full-screen apps (less, vim...) get arrow keys instead of scrollback.
            let arrow: &[u8] = match (whole > 0, mode.contains(TermMode::APP_CURSOR)) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            self.terminal
                .write(arrow.repeat(whole.unsigned_abs() as usize));
        } else {
            self.scroll_display(Scroll::Delta(whole));
        }
    }

    fn handle_terminal_event(&mut self, event: TermEvent, event_loop: &ActiveEventLoop) {
        match event {
            TermEvent::Wakeup => {
                self.validate_selection();
                self.window.request_redraw()
            }
            TermEvent::MouseCursorDirty | TermEvent::CursorBlinkingChange => {
                self.window.request_redraw()
            }
            TermEvent::Title(title) => self.window.set_title(&title),
            TermEvent::ResetTitle => self.window.set_title(DEFAULT_TITLE),
            TermEvent::PtyWrite(text) => self.terminal.write(text.into_bytes()),
            TermEvent::ClipboardStore(_, text) => {
                if let Some(clipboard) = self.clipboard.as_mut() {
                    let _ = clipboard.set_text(text);
                }
            }
            TermEvent::ClipboardLoad(_, format) => {
                let text = self
                    .clipboard
                    .as_mut()
                    .and_then(|c| c.get_text().ok())
                    .unwrap_or_default();
                self.terminal.write(format(&text).into_bytes());
            }
            TermEvent::ColorRequest(index, format) => {
                let color = self.terminal.term.lock().colors()[index]
                    .unwrap_or_else(|| self.palette.by_index(index));
                self.terminal.write(format(color).into_bytes());
            }
            TermEvent::TextAreaSizeRequest(format) => {
                self.terminal
                    .write(format(self.layout.window_size()).into_bytes());
            }
            TermEvent::Bell => self
                .window
                .request_user_attention(Some(winit::window::UserAttentionType::Informational)),
            TermEvent::Exit | TermEvent::ChildExit(_) => event_loop.exit(),
        }
    }
}

pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    config: Config,
    session: Option<Session>,
    update_checker: Checker,
    error: Option<anyhow::Error>,
}

impl App {
    fn create_session(&self, event_loop: &ActiveEventLoop) -> anyhow::Result<Session> {
        let attributes = Window::default_attributes()
            .with_title(DEFAULT_TITLE)
            .with_inner_size(LogicalSize::new(
                self.config.window.width,
                self.config.window.height,
            ));
        let window = Arc::new(event_loop.create_window(attributes)?);

        let scale = window.scale_factor() as f32;
        let renderer = pollster::block_on(Renderer::new(
            window.clone(),
            event_loop.owned_display_handle(),
            self.config.font.family.clone(),
            self.config.font.size * scale,
        ))?;

        let size = window.inner_size();
        let layout = GridLayout::new(
            size.width,
            size.height,
            renderer.cell_size(),
            self.config.window.padding * scale,
        );
        let terminal = Terminal::spawn(
            EventProxy::new(self.proxy.clone()),
            layout.window_size(),
            self.config.term_config(),
        )?;

        let clipboard = arboard::Clipboard::new()
            .map_err(|e| log::warn!("clipboard unavailable: {e}"))
            .ok();

        Ok(Session {
            window,
            renderer,
            terminal,
            layout,
            palette: Palette::default(),
            clipboard,
            focused: true,
            modifiers: ModifiersState::empty(),
            font_size: self.config.font.size,
            padding: self.config.window.padding,
            scroll_accumulator: 0.0,
            mouse_position: PhysicalPosition::default(),
            left_pressed: false,
            click_tracker: ClickTracker::default(),
            auto_scroll_deadline: None,
            selection_snapshot: None,
            update: None,
        })
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.error = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.session.is_some() {
            return;
        }
        match self.create_session(event_loop) {
            Ok(session) => self.session = Some(session),
            Err(error) => return self.fail(event_loop, error),
        }
        self.update_checker.apply(&self.config.updates);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let deadline = self
            .session
            .as_mut()
            .and_then(|s| s.auto_scroll(Instant::now()));
        event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        match event {
            UserEvent::Terminal(event) => session.handle_terminal_event(event, event_loop),
            UserEvent::Update(release) => {
                session.update = release;
                session.window.request_redraw();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let default_font_size = self.config.font.size;
        let Some(session) = self.session.as_mut() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                session.renderer.resize(size.width, size.height);
                session.relayout();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let font_size = session.font_size;
                session.apply_font_size(font_size);
            }
            WindowEvent::Focused(focused) => {
                session.focused = focused;
                if session.mode().contains(TermMode::FOCUS_IN_OUT) {
                    session.terminal.write(if focused {
                        &b"\x1b[I"[..]
                    } else {
                        &b"\x1b[O"[..]
                    });
                }
                session.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => session.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(action) = input::action_for(&event.logical_key, session.modifiers) {
                    session.run_action(action, default_font_size);
                    return;
                }
                let mode = session.mode();
                if let Some(bytes) = input::key_to_bytes(
                    &event.logical_key,
                    event.text.as_deref(),
                    session.modifiers,
                    mode,
                ) {
                    session.clear_selection();
                    session.scroll_display(Scroll::Bottom);
                    session.terminal.write(bytes);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => session.mouse_wheel(delta),
            WindowEvent::CursorMoved { position, .. } => session.cursor_moved(position),
            WindowEvent::MouseInput { state, button, .. } => session.mouse_input(state, button),
            WindowEvent::RedrawRequested => {
                let frame = {
                    let term = session.terminal.term.lock();
                    Frame::capture(&term, &session.palette, session.focused)
                };
                if let Err(error) = session.renderer.render(&frame, &session.layout) {
                    log::error!("render failed: {error:#}");
                }
            }
            _ => {}
        }
    }
}

/// Loads the configuration file. Errors are only logged for now.
fn load_config() -> Config {
    let (config, errors) = config::load();
    for error in &errors {
        log::warn!("{error}");
    }
    config
}

pub fn run() -> anyhow::Result<()> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let update_proxy = event_loop.create_proxy();
    let mut app = App {
        update_checker: Checker::new(move |release| {
            // The event loop may already be closed during shutdown.
            let _ = update_proxy.send_event(UserEvent::Update(release));
        }),
        proxy: event_loop.create_proxy(),
        config: load_config(),
        session: None,
        error: None,
    };
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
