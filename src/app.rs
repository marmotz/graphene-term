//! Window and event handling.

use std::sync::Arc;

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::term::TermMode;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::frame::Frame;
use crate::input::{self, Action};
use crate::layout::GridLayout;
use crate::palette::Palette;
use crate::renderer::Renderer;
use crate::terminal::{EventProxy, Terminal, UserEvent};

const DEFAULT_TITLE: &str = "Graphene Term";
const DEFAULT_FONT_SIZE: f32 = 14.0;
const MIN_FONT_SIZE: f32 = 6.0;
const MAX_FONT_SIZE: f32 = 72.0;
/// Padding around the grid, in logical pixels.
const PADDING: f32 = 8.0;

/// User-facing settings read from the environment.
#[derive(Debug, Clone)]
pub struct Settings {
    pub font_family: Option<String>,
    pub font_size: f32,
}

impl Settings {
    pub fn from_env() -> Self {
        Self {
            font_family: std::env::var("GRAPHENE_FONT").ok().filter(|f| !f.is_empty()),
            font_size: std::env::var("GRAPHENE_FONT_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_FONT_SIZE),
        }
    }
}

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
    /// Fractional lines accumulated from touchpad scrolling.
    scroll_accumulator: f64,
}

impl Session {
    fn compute_layout(&self) -> GridLayout {
        let size = self.window.inner_size();
        let scale = self.window.scale_factor() as f32;
        GridLayout::new(size.width, size.height, self.renderer.cell_size(), PADDING * scale)
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
        let bytes = input::paste_bytes(&text, self.mode());
        self.scroll_display(Scroll::Bottom);
        self.terminal.write(bytes);
    }

    fn run_action(&mut self, action: Action, default_font_size: f32) {
        match action {
            // Selection is not implemented yet; swallow the shortcut so it never sends ^C.
            Action::Copy => log::debug!("copy: no selection support yet"),
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
            self.terminal.write(arrow.repeat(whole.unsigned_abs() as usize));
        } else {
            self.scroll_display(Scroll::Delta(whole));
        }
    }

    fn handle_terminal_event(&mut self, event: TermEvent, event_loop: &ActiveEventLoop) {
        match event {
            TermEvent::Wakeup | TermEvent::MouseCursorDirty | TermEvent::CursorBlinkingChange => {
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
                self.terminal.write(format(self.layout.window_size()).into_bytes());
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
    settings: Settings,
    session: Option<Session>,
    error: Option<anyhow::Error>,
}

impl App {
    fn create_session(&self, event_loop: &ActiveEventLoop) -> anyhow::Result<Session> {
        let attributes = Window::default_attributes()
            .with_title(DEFAULT_TITLE)
            .with_inner_size(LogicalSize::new(960.0, 600.0));
        let window = Arc::new(event_loop.create_window(attributes)?);

        let scale = window.scale_factor() as f32;
        let renderer = pollster::block_on(Renderer::new(
            window.clone(),
            event_loop.owned_display_handle(),
            self.settings.font_family.clone(),
            self.settings.font_size * scale,
        ))?;

        let size = window.inner_size();
        let layout = GridLayout::new(size.width, size.height, renderer.cell_size(), PADDING * scale);
        let terminal = Terminal::spawn(EventProxy::new(self.proxy.clone()), layout.window_size())?;

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
            font_size: self.settings.font_size,
            scroll_accumulator: 0.0,
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
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let UserEvent::Terminal(event) = event;
        if let Some(session) = self.session.as_mut() {
            session.handle_terminal_event(event, event_loop);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let default_font_size = self.settings.font_size;
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
                    session
                        .terminal
                        .write(if focused { &b"\x1b[I"[..] } else { &b"\x1b[O"[..] });
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
                    session.scroll_display(Scroll::Bottom);
                    session.terminal.write(bytes);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => session.mouse_wheel(delta),
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

pub fn run() -> anyhow::Result<()> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let mut app = App {
        proxy: event_loop.create_proxy(),
        settings: Settings::from_env(),
        session: None,
        error: None,
    };
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
