//! Window and event handling.

use std::sync::Arc;
use std::time::Instant;

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::term::TermMode;
use arboard::{GetExtLinux, LinuxClipboardKind, SetExtLinux};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::banner::{Banner, BannerHit, BannerLayout, Shown, UpdateBanner, UpdateEffect};
use crate::block_actions::{Direction, Part};
use crate::config::{self, Config, MAX_FONT_SIZE, MIN_FONT_SIZE};
use crate::frame::{Frame, FrameBanner};
use crate::i18n::Lang;
use crate::input::{self, Action};
use crate::input_editor;
use crate::layout::{GridLayout, Hit, Rect};
use crate::mouse;
use crate::opener;
use crate::palette::Palette;
use crate::pane::{self, Pane};
use crate::perf;
use crate::renderer::{Glyphs, Renderer};
use crate::selection;
use crate::terminal::{EventProxy, PaneId, Terminal, UserEvent};
use crate::theme;
use crate::update::checker::Checker;
use crate::workspace::tree::Geometry;
use crate::workspace::{DIVIDER_THICKNESS, TabBarPosition, Workspace};

const DEFAULT_TITLE: &str = "Graphene Term";

/// Edge of the window the tab bar sits on.
const TAB_BAR: TabBarPosition = TabBarPosition::Top;

/// A mouse gesture in progress, decided by the press: moves and the release go to it.
enum Drag {
    /// A text selection in the pane `pane`, auto-scrolled while the mouse is past its
    /// top or bottom edge.
    Selection {
        pane: PaneId,
        /// When the next auto-scroll step is due, `None` while auto-scroll is idle.
        auto_scroll_deadline: Option<Instant>,
    },
}

struct Session {
    window: Arc<Window>,
    renderer: Renderer,
    workspace: Workspace<Pane>,
    /// Mouse gesture in progress.
    drag: Option<Drag>,
    palette: Palette,
    clipboard: Option<arboard::Clipboard>,
    focused: bool,
    modifiers: ModifiersState,
    font_size: f32,
    /// Minimum padding around the grid, in logical pixels.
    padding: f32,
    /// Last known cursor position, in physical pixels.
    mouse_position: PhysicalPosition<f64>,
    /// Pointer shape last applied to the window; `None` before the first update.
    pointer: Option<winit::window::CursorIcon>,
    /// Title last applied to the window.
    window_title: String,
    /// Configuration error banner; `None` once dismissed or without error.
    config_banner: Option<Banner>,
    /// Update banner: the newer release reported by the check thread.
    update: UpdateBanner,
    /// Tag the user just chose to ignore, forwarded to the update check thread.
    ignored_tag: Option<String>,
    lang: Lang,
    /// Logs the first frame drawn after the shell's first output (`graphene::perf`).
    first_frame: perf::FirstFrame,
}

impl Session {
    /// Banner shown at the top of the window: the configuration errors first, then the
    /// update notice.
    fn shown_banner(&self) -> Option<Shown> {
        Shown::pick(self.config_banner.as_ref(), &self.update, self.lang)
    }

    /// Places `shown` in pixels.
    fn banner_layout(&self, shown: &Shown) -> BannerLayout {
        let width = self.window.inner_size().width as f32;
        BannerLayout::new(shown.banner(), self.renderer.cell_size(), width)
    }

    /// Handles a left press on the banner, before any selection. Returns whether the
    /// banner took it.
    fn banner_press(&mut self) -> bool {
        let Some(shown) = self.shown_banner() else {
            return false;
        };
        let (x, y) = (self.mouse_position.x as f32, self.mouse_position.y as f32);
        let Some(hit) = self.banner_layout(&shown).hit(x, y) else {
            return false;
        };
        match (shown, hit) {
            // The configuration error banner has no action: any click dismisses it.
            (Shown::Config(_), _) => self.config_banner = None,
            (Shown::Update(_), BannerHit::Action(index)) => match self.update.press(index) {
                Some(UpdateEffect::Open(url)) => opener::open(&url),
                Some(UpdateEffect::Ignore(tag)) => self.ignored_tag = Some(tag),
                None => {}
            },
            (Shown::Update(_), BannerHit::Body) => {}
        }
        self.window.request_redraw();
        true
    }

    /// Area the tabs divide between their panes, and the sizes they are laid out with.
    fn geometry(&self) -> Geometry {
        let size = self.window.inner_size();
        let window = Rect::from_size(size.width, size.height);
        Geometry {
            // No tab bar is drawn yet: it takes no room.
            area: TAB_BAR.content_area(window, 0.0),
            cell: self.renderer.cell_size(),
            divider: DIVIDER_THICKNESS,
        }
    }

    /// Lays out every tab, hidden ones included, so each shell keeps a correct size;
    /// only panes whose cell count changed resize their shell. The hidden panes of a
    /// zoomed tab keep their last size.
    fn relayout(&mut self) {
        let geometry = self.geometry();
        let padding = self.padding * self.window.scale_factor() as f32;
        for index in 0..self.workspace.tabs().len() {
            for (id, rect) in self.workspace.layout(index, geometry).panes {
                if let Some(pane) = self.workspace.pane_mut(id) {
                    pane.resize(GridLayout::new(rect, geometry.cell, padding));
                }
            }
        }
        self.window.request_redraw();
    }

    fn apply_font_size(&mut self, font_size: f32) {
        self.font_size = font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        let scale = self.window.scale_factor() as f32;
        self.renderer.set_font_size(self.font_size * scale);
        self.relayout();
    }

    /// Applies what the panes asked for since the last call: a redraw when one of the
    /// visible ones changed, their text for the primary selection.
    fn flush_panes(&mut self) {
        let shown = self.workspace.active_tab().cloned();
        let mut redraw = false;
        let mut primary = None;
        for (id, pane) in self.workspace.panes_mut() {
            if pane.take_redraw() && shown.as_ref().is_some_and(|tab| tab.shows(id)) {
                redraw = true;
            }
            if let Some(text) = pane.take_primary() {
                primary = Some(text);
            }
        }
        if redraw {
            self.window.request_redraw();
        }
        if let Some(text) = primary {
            self.set_primary(text);
        }
    }

    /// Sets the window title to the shown tab's title, when it changed.
    fn refresh_title(&mut self) {
        let title = self
            .workspace
            .tab_title(self.workspace.active_index())
            .unwrap_or_else(|| DEFAULT_TITLE.to_owned());
        if title != self.window_title {
            self.window.set_title(&title);
            self.window_title = title;
        }
    }

    /// Hand over a fold chevron, arrow while the application captures the mouse under
    /// the pointer, I-beam otherwise; the window is only updated when the shape changes.
    fn update_pointer(&mut self) {
        let Some(pane) = self.workspace.active_pane() else {
            return;
        };
        let (hit, over_chevron) = pane.pointer_hit(self.mouse_position);
        let mode = pane.mode();
        let captured = if mouse::reports_at(&hit, mode.contains(TermMode::ALT_SCREEN)) {
            mode
        } else {
            TermMode::empty()
        };
        let icon = mouse::pointer_icon(captured, over_chevron);
        if self.pointer != Some(icon) {
            self.window.set_cursor(icon);
            self.pointer = Some(icon);
        }
    }

    fn paste(&mut self) {
        let Some(text) = self.clipboard.as_mut().and_then(|c| c.get_text().ok()) else {
            return;
        };
        if let Some(pane) = self.workspace.active_pane_mut() {
            pane.send_paste(&text);
        }
    }

    /// Writes the cleaned selection to the clipboard. Without selection, the shortcut is
    /// still swallowed so it never sends `^C`.
    fn copy(&mut self) {
        let text = self.workspace.active_pane().and_then(Pane::selected_text);
        self.set_clipboard(text);
    }

    /// Copies one part of the target block to the clipboard. An empty part leaves the
    /// clipboard untouched.
    fn copy_block_part(&mut self, part: Part) {
        let text = self
            .workspace
            .active_pane()
            .and_then(|pane| pane.block_part_text(part));
        self.set_clipboard(text);
    }

    fn set_clipboard(&mut self, text: Option<String>) {
        let Some(text) = text else {
            return;
        };
        if let Some(Err(error)) = self.clipboard.as_mut().map(|c| c.set_text(text)) {
            log::warn!("copy failed: {error}");
        }
    }

    fn mouse_input(&mut self, state: ElementState, button: MouseButton) {
        let Some(button) = mouse::button_from(button) else {
            return;
        };
        match state {
            ElementState::Pressed => self.mouse_press(button),
            ElementState::Released => self.mouse_release(button),
        }
    }

    fn mouse_press(&mut self, button: mouse::Button) {
        if button == mouse::Button::Left && self.banner_press() {
            return;
        }
        let (position, modifiers, lang) = (self.mouse_position, self.modifiers, self.lang);
        let Some(pane) = self.workspace.active_pane_mut() else {
            return;
        };
        let (hit, _) = pane.pointer_hit(position);
        // The margin, the chevrons and the gaps are outside the application's grid:
        // always Graphene's.
        if button == mouse::Button::Left {
            match hit {
                Hit::Margin { row } => {
                    pane.margin_press(row);
                    return;
                }
                Hit::Chevron { row } if pane.chevron_press(row) => return,
                Hit::Gap => return,
                Hit::Chevron { .. } | Hit::Cell { .. } | Hit::Blank => {}
            }
        }
        let mode = pane.mode();
        // Only the live rows are the application's: elsewhere the mouse is Graphene's.
        if selection::mouse_reserved(mode, modifiers) && pane.reports_at(&hit) {
            // The gesture belongs to the application; a click clears the selection.
            pane.clear_selection();
            let cell = pane.mouse_cell(position);
            if let Some(bytes) = pane.app_mouse.press(button, cell, modifiers, mode) {
                pane.terminal.write(bytes);
            }
            return;
        }
        match button {
            mouse::Button::Left => {
                let id = pane.id;
                if pane.left_press(position, modifiers, lang) {
                    self.drag = Some(Drag::Selection {
                        pane: id,
                        auto_scroll_deadline: None,
                    });
                }
            }
            mouse::Button::Middle => self.paste_primary(),
            mouse::Button::Right => {}
        }
    }

    fn mouse_release(&mut self, button: mouse::Button) {
        let (position, modifiers) = (self.mouse_position, self.modifiers);
        let Some(pane) = self.workspace.active_pane_mut() else {
            return;
        };
        if pane.app_mouse.owns(button) {
            let cell = pane.mouse_cell(position);
            let mode = pane.mode();
            if let Some(bytes) = pane.app_mouse.release(button, cell, modifiers, mode) {
                pane.terminal.write(bytes);
            }
        } else if button == mouse::Button::Left
            && let Some(Drag::Selection { pane: id, .. }) = self.drag.take()
            && let Some(pane) = self.workspace.pane_mut(id)
        {
            pane.left_release(position, modifiers);
        }
    }

    fn cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        self.mouse_position = position;
        self.update_pointer();
        if let Some(Drag::Selection { pane: id, .. }) = self.drag
            && let Some(pane) = self.workspace.pane_mut(id)
            && !pane.app_mouse.active()
        {
            pane.update_selection(position);
            return;
        }
        let modifiers = self.modifiers;
        let Some(pane) = self.workspace.active_pane_mut() else {
            return;
        };
        // A gesture the application owns follows the pointer anywhere; a bare motion is
        // only reported over the application's rows.
        if !pane.app_mouse.active() && !pane.reports_at(&pane.pointer_hit(position).0) {
            return;
        }
        let cell = pane.mouse_cell(position);
        let mode = pane.mode();
        if let Some(bytes) = pane.app_mouse.motion(cell, modifiers, mode) {
            pane.terminal.write(bytes);
        }
    }

    /// Auto-scroll while dragging a selection past the top or bottom edge of its pane:
    /// once per `AUTO_SCROLL_INTERVAL`, scrolls the timeline and moves the selection end
    /// onto the edge row. Returns when the next step is due, `None` when auto-scroll is
    /// idle.
    fn auto_scroll(&mut self, now: Instant) -> Option<Instant> {
        let position = self.mouse_position;
        let Some(Drag::Selection {
            pane: id,
            auto_scroll_deadline,
        }) = &mut self.drag
        else {
            return None;
        };
        let pane = self.workspace.pane_mut(*id)?;
        let delta = pane.auto_scroll_delta(position.y);
        if delta == 0 {
            *auto_scroll_deadline = None;
            return None;
        }
        match *auto_scroll_deadline {
            Some(deadline) if now < deadline => {}
            _ => {
                pane.scroll_timeline(delta as isize);
                pane.update_selection(position);
                *auto_scroll_deadline = Some(now + selection::AUTO_SCROLL_INTERVAL);
            }
        }
        *auto_scroll_deadline
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
        let text = match clipboard
            .get()
            .clipboard(LinuxClipboardKind::Primary)
            .text()
        {
            Ok(text) => text,
            Err(error) => {
                log::debug!("primary selection unavailable: {error}");
                return;
            }
        };
        if let Some(pane) = self.workspace.active_pane_mut() {
            pane.send_paste(&text);
        }
    }

    fn run_action(&mut self, action: Action, default_font_size: f32) {
        match action {
            Action::Copy => self.copy(),
            Action::Paste => self.paste(),
            Action::FontSizeIncrease => self.apply_font_size(self.font_size + 1.0),
            Action::FontSizeDecrease => self.apply_font_size(self.font_size - 1.0),
            Action::FontSizeReset => self.apply_font_size(default_font_size),
            Action::CopyCommand => self.copy_block_part(Part::Command),
            Action::CopyOutput => self.copy_block_part(Part::Output),
            _ => {
                if let Some(pane) = self.workspace.active_pane_mut() {
                    run_pane_action(pane, action);
                }
            }
        }
    }

    fn key_press(&mut self, event: &KeyEvent, default_font_size: f32) {
        let modifiers = self.modifiers;
        let Some(pane) = self.workspace.active_pane_mut() else {
            return;
        };
        // At the prompt, editor shortcuts win over the app shortcuts (`Shift+Home/End`)
        // and the key encoding.
        let rich = pane.rich_mode(modifiers);
        if let Some(command) = input_editor::prompt_command(&event.logical_key, modifiers, rich) {
            let replaced = input_editor::command_replacing_text(command)
                .is_some_and(|typed| pane.replace_selection(typed));
            if !replaced {
                pane.run_editor_command(command);
            }
            return;
        }
        if let Some(action) = input::action_for(&event.logical_key, modifiers) {
            self.run_action(action, default_font_size);
            return;
        }
        // At the prompt, typing over a selection in the input replaces it.
        if rich
            && let Some(typed) =
                input_editor::replacing_text(&event.logical_key, event.text.as_deref(), modifiers)
            && pane.replace_selection(&typed)
        {
            return;
        }
        let mode = pane.mode();
        if let Some(bytes) =
            input::key_to_bytes(&event.logical_key, event.text.as_deref(), modifiers, mode)
        {
            pane.type_bytes(bytes);
        }
    }

    fn mouse_wheel(&mut self, delta: MouseScrollDelta) {
        let (position, modifiers) = (self.mouse_position, self.modifiers);
        if let Some(pane) = self.workspace.active_pane_mut() {
            pane.mouse_wheel(delta, position, modifiers);
        }
    }

    /// Closes the pane `id`; the application exits with the last one.
    fn close_pane(&mut self, id: PaneId, event_loop: &ActiveEventLoop) {
        if matches!(self.drag, Some(Drag::Selection { pane, .. }) if pane == id) {
            self.drag = None;
        }
        if self.workspace.close_pane(id).is_err() {
            event_loop.exit();
            return;
        }
        self.relayout();
        self.refresh_title();
        self.update_pointer();
    }

    /// Handles an event from the terminal of the pane `id`; events of a pane already
    /// closed are dropped.
    fn handle_terminal_event(
        &mut self,
        id: PaneId,
        event: TermEvent,
        event_loop: &ActiveEventLoop,
    ) {
        let visible = self.workspace.is_visible(id);
        let active = self.workspace.active_pane_id() == Some(id);
        let Some(pane) = self.workspace.pane_mut(id) else {
            return;
        };
        match event {
            TermEvent::Wakeup => {
                if self.first_frame.output() {
                    perf::mark("first PTY output");
                }
                pane.validate_selection();
                pane.finish_extend();
                // Without OSC title, the title follows the shell's directory.
                let title = active && pane.title_refresh_due(Instant::now());
                if visible {
                    self.window.request_redraw();
                }
                if active {
                    self.update_pointer();
                }
                if title {
                    self.refresh_title();
                }
            }
            TermEvent::MouseCursorDirty | TermEvent::CursorBlinkingChange => {
                if visible {
                    self.window.request_redraw();
                }
            }
            TermEvent::Title(title) => {
                pane.title = Some(title);
                if active {
                    self.refresh_title();
                }
            }
            TermEvent::ResetTitle => {
                pane.title = None;
                if active {
                    self.refresh_title();
                }
            }
            TermEvent::PtyWrite(text) => pane.terminal.write(text.into_bytes()),
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
                pane.terminal.write(format(&text).into_bytes());
            }
            TermEvent::ColorRequest(index, format) => {
                let color = pane.terminal.term.lock().colors()[index]
                    .unwrap_or_else(|| self.palette.by_index(index));
                pane.terminal.write(format(color).into_bytes());
            }
            TermEvent::TextAreaSizeRequest(format) => {
                pane.terminal
                    .write(format(pane.layout.window_size()).into_bytes());
            }
            TermEvent::Bell => self
                .window
                .request_user_attention(Some(winit::window::UserAttentionType::Informational)),
            TermEvent::Exit | TermEvent::ChildExit(_) => self.close_pane(id, event_loop),
        }
    }

    /// Draws the active pane of the shown tab.
    fn redraw(&mut self) {
        let banner = self.shown_banner().map(|shown| FrameBanner {
            layout: self.banner_layout(&shown),
            background: self.palette.ui.banner_background,
            text: self.palette.ui.banner_text,
        });
        let Some(pane) = self.workspace.active_pane() else {
            return;
        };
        let mut frame = {
            let term = pane.terminal.term.lock();
            let mut timeline = pane::lock(&pane.terminal.timeline);
            let rows = pane::shown_rows(&pane.anchor, &term, &mut timeline);
            Frame::capture(
                &*term,
                &timeline,
                &rows,
                &self.palette,
                self.focused,
                pane.selected_block,
            )
        };
        frame.lang = self.lang;
        frame.banner = banner;
        match self.renderer.render(&frame, &pane.layout) {
            Ok(true) if self.first_frame.frame_drawn() => {
                perf::mark("first frame drawn after PTY output");
            }
            Ok(_) => {}
            Err(error) => log::error!("render failed: {error:#}"),
        }
    }
}

/// Runs an action that only concerns `pane`; the other actions are ignored.
fn run_pane_action(pane: &mut Pane, action: Action) {
    match action {
        Action::ScrollPageUp => pane.scroll_page(1),
        Action::ScrollPageDown => pane.scroll_page(-1),
        Action::ScrollToTop => pane.scroll_to_top(),
        Action::ScrollToBottom => pane.scroll_to_bottom(),
        Action::PreviousBlock => pane.navigate_block(Direction::Previous),
        Action::NextBlock => pane.navigate_block(Direction::Next),
        Action::ToggleFold => pane.toggle_fold(),
        Action::ToggleFoldAll => pane.change_folds(|timeline, _| timeline.toggle_fold_all()),
        Action::Copy
        | Action::Paste
        | Action::FontSizeIncrease
        | Action::FontSizeDecrease
        | Action::FontSizeReset
        | Action::CopyCommand
        | Action::CopyOutput => {}
    }
}

pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    config: Config,
    /// Palette of the configured theme.
    palette: Palette,
    session: Option<Session>,
    update_checker: Checker,
    error: Option<anyhow::Error>,
    /// Banner reporting the errors of the configuration and theme files.
    config_banner: Option<Banner>,
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
        perf::mark("window created");

        // Shell first, GPU second: the PTY reader thread parses the shell output while
        // the renderer is being created, so the first frame shows it complete.
        let scale = window.scale_factor() as f32;
        let glyphs = Glyphs::new(
            self.config.font.family.clone(),
            self.config.font.size * scale,
        );
        perf::mark("glyphs ready");

        let size = window.inner_size();
        let layout = GridLayout::new(
            TAB_BAR.content_area(Rect::from_size(size.width, size.height), 0.0),
            glyphs.cell_size(),
            self.config.window.padding * scale,
        );
        let mut workspace = Workspace::new();
        let id = workspace.allocate_id();
        let terminal = Terminal::spawn(
            EventProxy::new(self.proxy.clone(), id),
            layout.window_size(),
            self.config.term_config(),
            None,
        )?;
        workspace.new_tab(id, Pane::new(id, terminal, layout));
        perf::mark("shell spawned");

        let renderer = pollster::block_on(Renderer::new(
            window.clone(),
            event_loop.owned_display_handle(),
            glyphs,
        ))?;

        let clipboard = arboard::Clipboard::new()
            .map_err(|e| log::warn!("clipboard unavailable: {e}"))
            .ok();

        Ok(Session {
            window,
            renderer,
            workspace,
            drag: None,
            palette: self.palette.clone(),
            clipboard,
            focused: true,
            modifiers: ModifiersState::empty(),
            font_size: self.config.font.size,
            padding: self.config.window.padding,
            mouse_position: PhysicalPosition::default(),
            pointer: None,
            window_title: DEFAULT_TITLE.to_owned(),
            config_banner: self.config_banner.clone(),
            update: UpdateBanner::default(),
            ignored_tag: None,
            lang: Lang::detect(),
            first_frame: perf::FirstFrame::default(),
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
            Ok(mut session) => {
                session.update_pointer();
                self.session = Some(session);
            }
            Err(error) => return self.fail(event_loop, error),
        }
        self.update_checker.apply(&self.config.updates);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let deadline = self.session.as_mut().and_then(|session| {
            let deadline = session.auto_scroll(Instant::now());
            session.flush_panes();
            deadline
        });
        event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        match event {
            UserEvent::Terminal(pane, event) => {
                session.handle_terminal_event(pane, event, event_loop)
            }
            UserEvent::Update(release) => {
                session.update.set(release);
                session.window.request_redraw();
            }
        }
        session.flush_panes();
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
                if let Some(pane) = session.workspace.active_pane()
                    && let Some(bytes) = mouse::focus_report(focused, pane.mode())
                {
                    pane.terminal.write(bytes);
                }
                session.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => session.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                session.key_press(&event, default_font_size);
            }
            WindowEvent::MouseWheel { delta, .. } => session.mouse_wheel(delta),
            WindowEvent::CursorMoved { position, .. } => session.cursor_moved(position),
            WindowEvent::MouseInput { state, button, .. } => {
                session.mouse_input(state, button);
                if let Some(tag) = session.ignored_tag.take() {
                    self.update_checker.ignore(&tag);
                }
            }
            WindowEvent::RedrawRequested => session.redraw(),
            _ => {}
        }
        session.flush_panes();
    }
}

/// Loads the configuration file and its theme. Errors are logged and reported by the
/// returned banner.
fn load_config() -> (Config, Palette, Option<Banner>) {
    let (config, mut errors) = config::load();
    let (palette, theme_errors) = theme::load(&config.theme.name, config::config_dir().as_deref());
    errors.extend(theme_errors);
    for error in &errors {
        log::warn!("{error}");
    }
    (config, palette, Banner::config_errors(&errors))
}

pub fn run() -> anyhow::Result<()> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let update_proxy = event_loop.create_proxy();
    let (config, palette, config_banner) = load_config();
    let mut app = App {
        update_checker: Checker::new(move |release| {
            // The event loop may already be closed during shutdown.
            let _ = update_proxy.send_event(UserEvent::Update(release));
        }),
        proxy: event_loop.create_proxy(),
        config,
        palette,
        session: None,
        error: None,
        config_banner,
    };
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
