//! Window and event handling.

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::{Config as TermConfig, TermMode};
use arboard::{GetExtLinux, LinuxClipboardKind, SetExtLinux};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::appearance::{self, Appearance};
use crate::banner::{Banner, BannerHit, BannerLayout, Shown, UpdateBanner, UpdateEffect};
use crate::block_actions::{Direction, Part};
use crate::chrome;
use crate::chrome::command_palette::{
    self, Command, CommandPalette, Outcome, PaletteHit, PaletteLayout,
};
use crate::chrome::dialog::{self, Answer, ConfirmClose, DialogHit, DialogLayout, Target};
use crate::chrome::tab_bar::{self, TabBarHit, TabBarLayout, UiMetrics};
use crate::chrome::tooltip::TooltipLayout;
use crate::clipboard::{self, Content};
use crate::config::{
    self, Config, ConfigError, FontConfig, MAX_FONT_SIZE, MIN_FONT_SIZE, StartupMode, WindowConfig,
};
use crate::cursor_blink::CursorBlink;
use crate::frame::{Frame, FrameBanner, INACTIVE_PANE_DIM};
use crate::graphics;
use crate::i18n::Lang;
use crate::input::{self, Action};
use crate::input_editor;
use crate::keybindings::Keybindings;
use crate::layout::{GridLayout, Hit, Rect};
use crate::links::{self, Link};
use crate::mouse;
use crate::opener;
use crate::palette::Palette;
use crate::pane::{self, Pane};
use crate::perf;
use crate::renderer::{Glyphs, PaneFrame, Renderer, UiFont};
use crate::selection;
use crate::terminal::{EventProxy, PaneId, Terminal, UserEvent};
use crate::theme;
use crate::update::checker::Checker;
use crate::view::RowSource;
use crate::watcher;
use crate::window_state::{self, WindowState};
use crate::workspace::tree::{Axis, Geometry, Layout, NodePath};
use crate::workspace::{self, DIVIDER_THICKNESS, TabBarPosition, Workspace};

const DEFAULT_TITLE: &str = "Graphene Term";

/// Cells a pane grows by per resize shortcut.
const RESIZE_STEP_CELLS: usize = 2;

/// How long startup waits for the system appearance, so the first window opens with
/// the right `light`/`dark` theme. Without answer, it opens with the dark one and
/// switches when the answer comes.
const APPEARANCE_STARTUP_WAIT: Duration = Duration::from_millis(200);

/// A mouse gesture in progress, decided by the press: moves and the release go to it.
enum Drag {
    /// A text selection in the pane `pane`, auto-scrolled while the mouse is past its
    /// top or bottom edge.
    Selection {
        pane: PaneId,
        /// When the next auto-scroll step is due, `None` while auto-scroll is idle.
        auto_scroll_deadline: Option<Instant>,
    },
    /// A divider of the shown tab being moved.
    Divider { node_path: NodePath, axis: Axis },
    /// Tab `index` of the bar being moved, grabbed `grab_offset` pixels right of its
    /// left edge.
    Tab { index: usize, grab_offset: f32 },
}

/// Grid cell under the mouse: its pane, the grid shown there and the point in it.
type HoverCell = (PaneId, RowSource, Point);

/// The link under the mouse, in the grid shown by `source` in the pane `pane`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HoveredLink {
    pane: PaneId,
    source: RowSource,
    link: Link,
}

/// A left press on a link, waiting for its release to decide whether it opens.
struct LinkPress {
    pane: PaneId,
    press: links::Press<(RowSource, Point)>,
    link: Link,
}

/// A box over the whole window that takes the keyboard and the mouse while it is open.
enum Overlay {
    /// A close waiting for confirmation.
    Confirm(ConfirmClose),
    /// The command palette.
    Palette(CommandPalette),
}

struct Session {
    window: Arc<Window>,
    /// Hands the events of new panes' terminals to the event loop.
    proxy: EventLoopProxy<UserEvent>,
    /// Terminal settings new panes are spawned with.
    term_config: TermConfig,
    renderer: Renderer,
    workspace: Workspace<Pane>,
    /// Mouse gesture in progress.
    drag: Option<Drag>,
    /// Box open over the window, taking the keyboard and the mouse.
    overlay: Option<Overlay>,
    palette: Palette,
    clipboard: Option<arboard::Clipboard>,
    focused: bool,
    /// Blink phase of the active pane's cursor.
    cursor_blink: CursorBlink,
    modifiers: ModifiersState,
    font_size: f32,
    /// Minimum padding around the grid, in logical pixels.
    padding: f32,
    /// Last known cursor position, in physical pixels.
    mouse_position: PhysicalPosition<f64>,
    /// Grid cell `hovered_link` was looked up for; `None` off the cells.
    hovered_cell: Option<HoverCell>,
    /// Link under the mouse, Graphene's or not (see [`Session::active_link`]).
    hovered_link: Option<HoveredLink>,
    /// Left press on a link, until its release.
    link_press: Option<LinkPress>,
    /// Whether the link tooltip stays hidden: set by the wheel, cleared when the mouse
    /// moves.
    tooltip_hidden: bool,
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
    /// Edge of the window the tab bar sits on.
    tab_position: TabBarPosition,
    /// Shortcuts of the application actions.
    keybindings: Keybindings,
    /// Logs the first frame drawn after the shell's first output (`graphene::perf`).
    first_frame: perf::FirstFrame,
    /// Last inner size seen while the window was not maximized, in logical pixels:
    /// winit has no getter for the size a maximized window is restored to.
    restore_size: LogicalSize<u32>,
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

    /// Sizes of the interface text in `font`.
    fn ui_metrics(&self, font: UiFont) -> UiMetrics {
        UiMetrics {
            line_height: self.renderer.ui_line_height(font),
            scale: self.window.scale_factor() as f32,
        }
    }

    /// Size of the tab bar across its edge, 0 while it is hidden.
    fn tab_bar_thickness(&self) -> f32 {
        if tab_bar::shown(self.workspace.tabs().len()) {
            tab_bar::thickness(
                self.tab_position,
                self.window.inner_size().width as f32,
                self.ui_metrics(UiFont::Chrome),
            )
        } else {
            0.0
        }
    }

    /// The tab bar, `None` while it is hidden.
    fn tab_bar(&self) -> Option<TabBarLayout> {
        let count = self.workspace.tabs().len();
        if !tab_bar::shown(count) {
            return None;
        }
        let titles: Vec<String> = (0..count)
            .map(|index| {
                self.workspace
                    .tab_title(index)
                    .unwrap_or_else(|| DEFAULT_TITLE.to_owned())
            })
            .collect();
        let size = self.window.inner_size();
        Some(TabBarLayout::new(
            Rect::from_size(size.width, size.height),
            self.tab_position,
            &titles,
            self.workspace.active_index(),
            self.ui_metrics(UiFont::Chrome),
        ))
    }

    /// Area the tabs divide between their panes, and the sizes they are laid out with.
    fn geometry(&self) -> Geometry {
        let size = self.window.inner_size();
        let window = Rect::from_size(size.width, size.height);
        Geometry {
            area: self
                .tab_position
                .content_area(window, self.tab_bar_thickness()),
            cell: self.renderer.cell_size(),
            divider: DIVIDER_THICKNESS,
        }
    }

    /// Layout of the shown tab.
    fn shown_layout(&self) -> Layout {
        self.workspace
            .layout(self.workspace.active_index(), self.geometry())
    }

    /// The mouse position as a pixel, in physical pixels.
    fn mouse_pixel(&self) -> (f32, f32) {
        (self.mouse_position.x as f32, self.mouse_position.y as f32)
    }

    /// The pane of the shown tab under the mouse.
    fn pane_under_mouse(&self) -> Option<PaneId> {
        let (x, y) = self.mouse_pixel();
        self.shown_layout().pane_at(x, y)
    }

    /// Gives the keyboard focus to the pane `id`.
    fn focus_pane(&mut self, id: PaneId) {
        let old = self.workspace.active_pane_id();
        if self.workspace.focus(id) {
            self.focus_changed(old);
        }
    }

    /// After the active pane may have changed from `old`: reports the focus change to
    /// both panes when they asked for it and the window has the focus, lays the tabs
    /// out again (a focus change unzooms) and refreshes the title and the pointer.
    fn focus_changed(&mut self, old: Option<PaneId>) {
        let new = self.workspace.active_pane_id();
        if old != new && self.focused {
            for (id, focused) in [(old, false), (new, true)] {
                if let Some(pane) = id.and_then(|id| self.workspace.pane(id))
                    && let Some(bytes) = mouse::focus_report(focused, pane.mode())
                {
                    pane.terminal.write(bytes);
                }
            }
        }
        self.relayout();
        self.refresh_title();
        self.update_pointer();
    }

    /// What a new pane is spawned from: the directory its shell starts in (the active
    /// pane's, else the home directory) and a first layout, the active pane's (the new
    /// pane is laid out for real by `relayout`). `None` without active pane.
    fn new_pane_start(&self) -> Option<(Option<PathBuf>, GridLayout)> {
        let active = self.workspace.active_pane()?;
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let cwd = workspace::start_dir(active.terminal.current_dir(), home);
        Some((cwd, active.layout))
    }

    /// A closure spawning the pane of a given id, starting in `cwd` with `layout`.
    fn pane_spawner(
        &self,
        cwd: Option<PathBuf>,
        layout: GridLayout,
    ) -> impl FnOnce(PaneId) -> Option<Pane> + use<> {
        let (proxy, config) = (self.proxy.clone(), self.term_config.clone());
        move |id| match Terminal::spawn(
            EventProxy::new(proxy, id),
            layout.window_size(),
            config,
            cwd,
        ) {
            Ok(terminal) => Some(Pane::new(id, terminal, layout)),
            Err(error) => {
                log::error!("cannot start a shell for the new pane: {error:#}");
                None
            }
        }
    }

    /// Splits the active pane along `axis`; the new pane takes the focus.
    fn split(&mut self, axis: Axis) {
        let Some((cwd, layout)) = self.new_pane_start() else {
            return;
        };
        let old = self.workspace.active_pane_id();
        let spawn = self.pane_spawner(cwd, layout);
        if self.workspace.split(axis, self.geometry(), spawn).is_some() {
            self.focus_changed(old);
        }
    }

    /// Opens a tab after the last one and shows it.
    fn new_tab(&mut self) {
        let Some((cwd, layout)) = self.new_pane_start() else {
            return;
        };
        let old = self.workspace.active_pane_id();
        let id = self.workspace.allocate_id();
        if let Some(pane) = self.pane_spawner(cwd, layout)(id) {
            self.workspace.new_tab(id, pane);
            self.focus_changed(old);
        }
    }

    /// Shows the tab chosen by `select` on the workspace, when it changes the shown tab.
    fn change_tab(&mut self, select: impl FnOnce(&mut Workspace<Pane>) -> bool) {
        let (old, index) = (
            self.workspace.active_pane_id(),
            self.workspace.active_index(),
        );
        if select(&mut self.workspace) && self.workspace.active_index() != index {
            self.focus_changed(old);
        }
    }

    /// Asks to close `target`: at once when none of its panes runs a program other
    /// than its shell, otherwise once the user confirms in a dialog.
    /// Writes the window size and maximized state to `window.json`, whatever the startup
    /// mode, so that switching to `remember` later restores the last window. A failure
    /// is only logged. The position is not saved.
    fn save_window_state(&self) {
        let Some(file) = window_state::path() else {
            return;
        };
        let state = WindowState {
            width: self.restore_size.width,
            height: self.restore_size.height,
            maximized: self.window.is_maximized(),
        };
        if let Err(error) = state.save(&file) {
            log::warn!("cannot write {}: {error}", file.display());
        }
    }

    fn request_close(&mut self, target: Target, event_loop: &ActiveEventLoop) {
        let programs: Vec<_> = target
            .panes(&self.workspace)
            .into_iter()
            .map(|id| {
                self.workspace
                    .pane(id)
                    .and_then(|pane| pane.terminal.foreground_program())
            })
            .collect();
        match ConfirmClose::request(target.clone(), programs) {
            Some(confirm) => {
                self.overlay = Some(Overlay::Confirm(confirm));
                self.window.request_redraw();
                self.update_pointer();
            }
            None => self.close(&target, event_loop),
        }
    }

    /// Closes `target` without asking: the window, or each of its panes still open;
    /// the application exits with the last pane.
    fn close(&mut self, target: &Target, event_loop: &ActiveEventLoop) {
        if *target == Target::Window {
            event_loop.exit();
            return;
        }
        for id in target.panes(&self.workspace) {
            if event_loop.exiting() {
                return;
            }
            self.close_pane(id, event_loop);
        }
    }

    /// The open dialog laid out in the window.
    fn dialog_layout(&mut self) -> Option<DialogLayout> {
        let Some(Overlay::Confirm(confirm)) = &self.overlay else {
            return None;
        };
        let size = self.window.inner_size();
        let metrics = self.ui_metrics(UiFont::Chrome);
        let renderer = &mut self.renderer;
        Some(DialogLayout::new(
            confirm,
            self.lang,
            Rect::from_size(size.width, size.height),
            metrics,
            |text| renderer.ui_text_width(UiFont::Chrome, text),
        ))
    }

    /// Applies the user's `answer` to the open dialog and closes it.
    fn answer_dialog(&mut self, answer: Answer, event_loop: &ActiveEventLoop) {
        let Some(Overlay::Confirm(confirm)) = self.overlay.take() else {
            return;
        };
        self.window.request_redraw();
        self.update_pointer();
        if answer == Answer::Close {
            self.close(&confirm.target, event_loop);
        }
    }

    /// Opens the command palette over the window, its recent actions read from the
    /// recent file.
    fn open_palette(&mut self) {
        let recent = command_palette::recent_path()
            .map(|path| command_palette::load_recent(&path))
            .unwrap_or_default();
        let entries = command_palette::entries(&self.workspace, self.lang, &self.keybindings);
        self.overlay = Some(Overlay::Palette(CommandPalette::new(entries, recent)));
        self.window.request_redraw();
        self.update_pointer();
    }

    /// The open command palette laid out in the window.
    fn palette_layout(&mut self) -> Option<PaletteLayout> {
        let Some(Overlay::Palette(palette)) = &self.overlay else {
            return None;
        };
        let size = self.window.inner_size();
        let metrics = self.ui_metrics(UiFont::Palette);
        let renderer = &mut self.renderer;
        Some(PaletteLayout::new(
            palette,
            self.lang,
            Rect::from_size(size.width, size.height),
            metrics,
            |text| renderer.ui_text_width(UiFont::Palette, text),
        ))
    }

    /// Applies what a key or a click on the open palette asks: closes it, then runs the
    /// chosen entry. An action run is remembered in the recent file.
    fn palette_outcome(
        &mut self,
        outcome: Outcome,
        default_font_size: f32,
        event_loop: &ActiveEventLoop,
    ) {
        let Some(Overlay::Palette(palette)) = self.overlay.take() else {
            return;
        };
        self.window.request_redraw();
        self.update_pointer();
        let Outcome::Run(command) = outcome else {
            return;
        };
        match command {
            Command::Action(action) => {
                if let Some(path) = command_palette::recent_path() {
                    let recent = command_palette::remember(palette.recent(), action);
                    if let Err(error) = command_palette::save_recent(&path, &recent) {
                        log::warn!("cannot write {}: {error}", path.display());
                    }
                }
                self.run_action(action, default_font_size, event_loop);
            }
            Command::Tab(index) => self.change_tab(|workspace| workspace.select(index)),
            Command::Pane(id) => self.focus_pane(id),
        }
    }

    /// Handles a press while an overlay is open: a button of the dialog answers it, a
    /// row of the palette runs it and a press outside the palette closes it; anything
    /// else is ignored.
    fn overlay_press(
        &mut self,
        button: mouse::Button,
        default_font_size: f32,
        event_loop: &ActiveEventLoop,
    ) {
        let (x, y) = self.mouse_pixel();
        if !matches!(self.overlay, Some(Overlay::Palette(_))) {
            if button == mouse::Button::Left
                && let Some(DialogHit::Button(answer)) =
                    self.dialog_layout().and_then(|layout| layout.hit(x, y))
            {
                self.answer_dialog(answer, event_loop);
            }
            return;
        }
        let outcome = match self.palette_layout().and_then(|layout| layout.hit(x, y)) {
            None => Some(Outcome::Close),
            Some(PaletteHit::Row(index)) if button == mouse::Button::Left => match &self.overlay {
                Some(Overlay::Palette(palette)) => palette.choose(index),
                _ => None,
            },
            Some(_) => None,
        };
        if let Some(outcome) = outcome {
            self.palette_outcome(outcome, default_font_size, event_loop);
        }
    }

    /// Handles a press on the tab bar: a tab is shown and grabbed, its close button or a
    /// middle click closes it, the `+` button opens a tab.
    fn tab_bar_press(
        &mut self,
        button: mouse::Button,
        hit: TabBarHit,
        bar: &TabBarLayout,
        event_loop: &ActiveEventLoop,
    ) {
        match (button, hit) {
            (mouse::Button::Left, TabBarHit::Tab(index)) => {
                self.change_tab(|workspace| workspace.select(index));
                let (x, y) = self.mouse_pixel();
                let tab = bar.tabs[index].rect;
                self.drag = Some(Drag::Tab {
                    index,
                    grab_offset: bar.along(x, y) - bar.along(tab.x, tab.y),
                });
            }
            (mouse::Button::Left, TabBarHit::Close(index))
            | (mouse::Button::Middle, TabBarHit::Tab(index) | TabBarHit::Close(index)) => {
                if let Some(target) = Target::tab(&self.workspace, index) {
                    self.request_close(target, event_loop);
                }
            }
            (mouse::Button::Left, TabBarHit::NewTab) => self.new_tab(),
            _ => {}
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

    /// Applies a reloaded configuration, `old` being the one applied so far. The window
    /// size and startup mode only affect the windows created afterwards.
    fn apply_config(
        &mut self,
        old: &Config,
        new: &Config,
        palette: &Palette,
        banner: Option<Banner>,
    ) {
        let term_config = new.term_config();
        for (_, pane) in self.workspace.panes_mut() {
            // Lock order: `term`, then `timeline`.
            let mut term = pane.terminal.term.lock();
            term.set_options(term_config.clone());
            pane::lock(&pane.terminal.timeline).set_scrollback(new.scrollback_lines);
        }
        self.term_config = term_config;

        let font_size = reloaded_font_size(&old.font, &new.font, self.font_size);
        if new.font.family != old.font.family {
            self.font_size = font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
            let scale = self.window.scale_factor() as f32;
            self.renderer
                .set_font(new.font.family.clone(), self.font_size * scale);
        } else if font_size != self.font_size {
            self.apply_font_size(font_size);
        }
        self.padding = new.window.padding;
        self.renderer
            .set_background_opacity(new.window.background_opacity);
        self.palette = palette.clone();
        self.keybindings = Keybindings::new(&new.keybindings);
        self.lang = new.language.resolve(Lang::detect);
        self.tab_position = new.tab_position.into();
        self.config_banner = banner;
        // A changed cursor style shows at once, visible.
        self.reset_blink();
        self.relayout();
    }

    fn apply_font_size(&mut self, font_size: f32) {
        self.font_size = font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        let scale = self.window.scale_factor() as f32;
        self.renderer.set_font_size(self.font_size * scale);
        self.renderer.set_ui_scale(scale);
        self.relayout();
    }

    /// Applies what the panes asked for since the last call: a redraw when one of the
    /// visible ones changed, their text for the primary selection.
    /// Shows the due frames of the animated images of the shown panes, and requests a
    /// redraw when one changed. Returns when the next frame is due, `None` when no
    /// shown pane plays an animation.
    fn animate(&mut self, now: Instant) -> Option<Instant> {
        let shown = self.workspace.active_tab().cloned();
        let mut redraw = false;
        let mut deadline = None;
        for (id, pane) in self.workspace.panes_mut() {
            if !shown.as_ref().is_some_and(|tab| tab.shows(id)) {
                continue;
            }
            let mut images = graphics::lock(&pane.terminal.images);
            redraw |= images.advance(now);
            deadline = earliest(deadline, images.next_frame_deadline(now));
        }
        if redraw {
            self.window.request_redraw();
        }
        deadline
    }

    /// Updates the blink phase of the active pane's cursor, and requests a redraw when
    /// it toggled. Returns when the next toggle is due, `None` while the cursor does not
    /// blink: its style does not ask for it, or the window does not have the focus.
    fn blink(&mut self, now: Instant) -> Option<Instant> {
        let blinking = self.focused
            && self
                .workspace
                .active_pane()
                .is_some_and(|pane| pane.terminal.term.lock().cursor_style().blinking);
        let (changed, deadline) = self.cursor_blink.update(now, blinking);
        if changed {
            self.window.request_redraw();
        }
        deadline
    }

    /// Shows the cursor for a full blink interval: typing or pasting never hides it.
    fn reset_blink(&mut self) {
        if self.cursor_blink.reset(Instant::now()) {
            self.window.request_redraw();
        }
    }

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

    /// Hand over a fold chevron or an active link, arrow while the application captures
    /// the mouse under the pointer, I-beam otherwise; the window is only updated when the shape changes.
    /// Over a divider, or while dragging one, the resize arrows of its axis.
    fn update_pointer(&mut self) {
        let (x, y) = self.mouse_pixel();
        if self.overlay.is_some()
            || matches!(self.drag, Some(Drag::Tab { .. }))
            || !self.geometry().area.contains(x, y)
        {
            self.set_pointer(winit::window::CursorIcon::Default);
            return;
        }
        let layout = self.shown_layout();
        let divider = match &self.drag {
            Some(Drag::Divider { axis, .. }) => Some(*axis),
            _ => layout.divider_at(x, y).map(|divider| divider.axis),
        };
        let icon = if let Some(axis) = divider {
            mouse::divider_icon(axis)
        } else {
            let id = layout
                .pane_at(x, y)
                .or_else(|| self.workspace.active_pane_id());
            let Some(pane) = id.and_then(|id| self.workspace.pane(id)) else {
                return;
            };
            let (hit, over_chevron) = pane.pointer_hit(self.mouse_position);
            let mode = pane.mode();
            let captured = if mouse::reports_at(&hit, mode.contains(TermMode::ALT_SCREEN)) {
                mode
            } else {
                TermMode::empty()
            };
            let over_link = self
                .active_link()
                .is_some_and(|hovered| hovered.pane == pane.id);
            mouse::pointer_icon(captured, over_chevron || over_link)
        };
        self.set_pointer(icon);
    }

    /// Looks the link under the mouse up again when the grid cell under it changed, or
    /// always with `force` (the content may have changed). No link while an overlay is
    /// open or a gesture is in progress. Redraws when the link changed.
    fn update_hover(&mut self, force: bool) {
        let cell = if self.overlay.is_some() || self.drag.is_some() {
            None
        } else {
            let position = self.mouse_position;
            self.pane_under_mouse().and_then(|id| {
                let (source, point) = self.workspace.pane(id)?.cell_under(position)?;
                Some((id, source, point))
            })
        };
        if !force && cell == self.hovered_cell {
            return;
        }
        self.hovered_cell = cell;
        let hovered = cell.and_then(|(id, source, point)| {
            let link = self.workspace.pane(id)?.link_at(source, point)?;
            Some(HoveredLink {
                pane: id,
                source,
                link,
            })
        });
        if hovered != self.hovered_link {
            self.hovered_link = hovered;
            self.window.request_redraw();
        }
    }

    /// The link under the mouse when it is Graphene's (see [`links::active`]): pointed
    /// at, underlined and opened on a click.
    fn active_link(&self) -> Option<&HoveredLink> {
        let hovered = self.hovered_link.as_ref()?;
        let pane = self.workspace.pane(hovered.pane)?;
        links::active(pane.captures(hovered.source), self.modifiers).then_some(hovered)
    }

    /// Target shown in the link tooltip: the URI of the active OSC 8 link, unless the
    /// wheel hid the tooltip since the mouse last moved.
    fn tooltip_uri(&self) -> Option<&str> {
        let hovered = self.active_link()?;
        (hovered.link.explicit && !self.tooltip_hidden).then_some(hovered.link.uri.as_str())
    }

    /// The link tooltip laid out near the mouse, `None` while no tooltip is shown.
    fn tooltip_layout(&mut self) -> Option<TooltipLayout> {
        let uri = self.tooltip_uri()?.to_owned();
        let size = self.window.inner_size();
        let metrics = self.ui_metrics(UiFont::Chrome);
        let mouse = self.mouse_pixel();
        let renderer = &mut self.renderer;
        TooltipLayout::new(
            &uri,
            mouse,
            Rect::from_size(size.width, size.height),
            metrics,
            |text| renderer.ui_text_width(UiFont::Chrome, text),
        )
    }

    /// The mouse left the window: nothing is hovered any more.
    fn cursor_left(&mut self) {
        self.hovered_cell = None;
        if self.hovered_link.take().is_some() {
            self.window.request_redraw();
        }
        self.update_pointer();
    }

    /// End of a left press on a link: opens it when the click calls for it (see
    /// [`links::click_opens`]) and it may be opened (see [`links::may_open`]).
    fn release_link(&mut self, press: LinkPress) {
        if self.pane_under_mouse() != Some(press.pane) {
            return;
        }
        let Some(pane) = self.workspace.pane(press.pane) else {
            return;
        };
        let Some(cell) = pane.cell_under(self.mouse_position) else {
            return;
        };
        let same = pane.link_at(cell.0, cell.1).as_ref() == Some(&press.link);
        if !links::click_opens(&press.press, &cell, same) {
            return;
        }
        if links::may_open(&press.link, opener::hostname().as_deref()) {
            opener::open(&press.link.uri);
        } else {
            log::warn!("not opening {}: a file on another host", press.link.uri);
        }
    }

    /// Applies the pointer shape `icon` when it changed.
    fn set_pointer(&mut self, icon: winit::window::CursorIcon) {
        if self.pointer != Some(icon) {
            self.window.set_cursor(icon);
            self.pointer = Some(icon);
        }
    }

    fn paste(&mut self) {
        self.reset_blink();
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
        match self.workspace.active_pane().and_then(Pane::copy_content) {
            Some(Content::Text(text)) => self.set_clipboard(Some(text)),
            Some(Content::Image { text, png }) => {
                if let Err(error) = clipboard::set_text_and_image(text.clone(), png) {
                    log::warn!("image copy failed, copying the text only: {error:#}");
                    self.set_clipboard(text);
                }
            }
            None => {}
        }
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

    fn mouse_input(
        &mut self,
        state: ElementState,
        button: MouseButton,
        default_font_size: f32,
        event_loop: &ActiveEventLoop,
    ) {
        let Some(button) = mouse::button_from(button) else {
            return;
        };
        match state {
            ElementState::Pressed => self.mouse_press(button, default_font_size, event_loop),
            ElementState::Released => self.mouse_release(button),
        }
    }

    /// A press goes to the first of: the overlay, the banner, the tab bar, a divider, a
    /// pane. While an overlay is open, it takes every press.
    fn mouse_press(
        &mut self,
        button: mouse::Button,
        default_font_size: f32,
        event_loop: &ActiveEventLoop,
    ) {
        if self.overlay.is_some() {
            self.overlay_press(button, default_font_size, event_loop);
            return;
        }
        if button == mouse::Button::Left && self.banner_press() {
            return;
        }
        let (x, y) = self.mouse_pixel();
        if let Some(bar) = self.tab_bar()
            && bar.rect.contains(x, y)
        {
            if let Some(hit) = bar.hit(x, y) {
                self.tab_bar_press(button, hit, &bar, event_loop);
            }
            return;
        }
        let layout = self.shown_layout();
        if button == mouse::Button::Left
            && let Some(divider) = layout.divider_at(x, y)
        {
            self.drag = Some(Drag::Divider {
                node_path: divider.node_path.clone(),
                axis: divider.axis,
            });
            self.update_pointer();
            return;
        }
        // A press in an inactive pane makes it active, then acts as in the active one.
        let Some(target) = layout.pane_at(x, y) else {
            return;
        };
        if self.workspace.active_pane_id() != Some(target) {
            self.focus_pane(target);
        }
        let (position, modifiers, lang) = (self.mouse_position, self.modifiers, self.lang);
        let Some(pane) = self.workspace.pane_mut(target) else {
            return;
        };
        let (hit, _) = pane.pointer_hit(position);
        let link = match hit {
            Hit::Cell {
                source,
                line,
                column,
                ..
            } if button == mouse::Button::Left => {
                let point = Point::new(line, alacritty_terminal::index::Column(column));
                pane.link_at(source, point)
                    .map(|link| (source, point, link))
            }
            _ => None,
        };
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
        let captured = selection::mouse_reserved(mode, modifiers) && pane.reports_at(&hit);
        if captured
            && modifiers.control_key()
            && let Some((source, point, link)) = link
        {
            // `Ctrl` + press on a link: Graphene's, never reported; opened on release.
            self.link_press = Some(LinkPress {
                pane: target,
                press: links::Press {
                    cell: (source, point),
                    clicks: 1,
                    mods: modifiers,
                    captured: true,
                },
                link,
            });
            return;
        }
        if captured {
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
                let dragging = pane.left_press(position, modifiers, lang);
                let clicks = pane.last_clicks();
                // The selection starts as usual: the release tells whether it was a click
                // on the link.
                self.link_press = link.map(|(source, point, link)| LinkPress {
                    pane: id,
                    press: links::Press {
                        cell: (source, point),
                        clicks,
                        mods: modifiers,
                        captured: false,
                    },
                    link,
                });
                if dragging {
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

    /// Ends the gesture of `button`, then opens the link it was pressed on when the
    /// press and release make a click on it.
    fn mouse_release(&mut self, button: mouse::Button) {
        let link_press = match button {
            mouse::Button::Left => self.link_press.take(),
            _ => None,
        };
        self.release_gesture(button);
        if let Some(press) = link_press {
            self.release_link(press);
        }
        self.update_hover(false);
        self.update_pointer();
    }

    fn release_gesture(&mut self, button: mouse::Button) {
        if button == mouse::Button::Left && matches!(self.drag, Some(Drag::Divider { .. })) {
            self.drag = None;
            self.update_pointer();
            return;
        }
        if button == mouse::Button::Left
            && let Some(Drag::Tab { index, grab_offset }) = self.drag
        {
            self.drag = None;
            if let Some(mut bar) = self.tab_bar() {
                let (x, y) = self.mouse_pixel();
                let to = bar.dragged(index, bar.along(x, y) - grab_offset);
                self.workspace.move_tab(index, to);
            }
            self.window.request_redraw();
            self.update_pointer();
            return;
        }
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
        if std::mem::take(&mut self.tooltip_hidden) && self.tooltip_uri().is_some() {
            self.window.request_redraw();
        }
        self.update_hover(false);
        self.update_pointer();
        if matches!(self.drag, Some(Drag::Tab { .. })) {
            self.window.request_redraw();
            return;
        }
        if let Some(Drag::Divider { node_path, axis }) = &self.drag {
            let along = match axis {
                Axis::Horizontal => position.x,
                Axis::Vertical => position.y,
            } as f32;
            let geometry = self.geometry();
            let node_path = node_path.clone();
            if self.workspace.set_divider(&node_path, along, geometry) {
                self.relayout();
            }
            return;
        }
        if let Some(Drag::Selection { pane: id, .. }) = self.drag
            && let Some(pane) = self.workspace.pane_mut(id)
            && !pane.app_mouse.active()
        {
            pane.update_selection(position);
            return;
        }
        let modifiers = self.modifiers;
        let over_active = self.pane_under_mouse() == self.workspace.active_pane_id();
        let Some(pane) = self.workspace.active_pane_mut() else {
            return;
        };
        // A gesture the application owns follows the pointer anywhere; a bare motion is
        // only reported over the application's rows of the active pane.
        if !pane.app_mouse.active()
            && !(over_active && pane.reports_at(&pane.pointer_hit(position).0))
        {
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
        self.reset_blink();
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

    fn run_action(&mut self, action: Action, default_font_size: f32, event_loop: &ActiveEventLoop) {
        match action {
            Action::NewTab => self.new_tab(),
            Action::CommandPalette => self.open_palette(),
            Action::NextTab => self.change_tab(|workspace| {
                workspace.next();
                true
            }),
            Action::PreviousTab => self.change_tab(|workspace| {
                workspace.previous();
                true
            }),
            Action::GoToTab(number) => {
                let index = usize::from(number).saturating_sub(1);
                self.change_tab(|workspace| workspace.select(index));
            }
            Action::SplitRight => self.split(Axis::Horizontal),
            Action::SplitDown => self.split(Axis::Vertical),
            Action::ClosePane => {
                if let Some(id) = self.workspace.active_pane_id() {
                    self.request_close(Target::Pane(id), event_loop);
                }
            }
            Action::FocusPane(direction) => {
                if let Some(id) = self.workspace.neighbour(direction, self.geometry()) {
                    self.focus_pane(id);
                }
            }
            Action::NextPane => {
                if let Some(id) = self.workspace.next_pane() {
                    self.focus_pane(id);
                }
            }
            Action::PreviousPane => {
                if let Some(id) = self.workspace.previous_pane() {
                    self.focus_pane(id);
                }
            }
            Action::ResizePane(direction) => {
                let geometry = self.geometry();
                if self
                    .workspace
                    .resize_active(direction, RESIZE_STEP_CELLS, geometry)
                {
                    self.relayout();
                }
            }
            Action::ToggleZoom => {
                if self.workspace.toggle_zoom() {
                    self.relayout();
                    self.update_pointer();
                }
            }
            Action::EqualisePanes => {
                if self.workspace.equalise() {
                    self.relayout();
                    self.update_pointer();
                }
            }
            Action::DetachPane => {
                if let Some(id) = self.workspace.active_pane_id()
                    && self.workspace.detach(id)
                {
                    self.relayout();
                    self.refresh_title();
                    self.update_pointer();
                }
            }
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

    fn key_press(
        &mut self,
        event: &KeyEvent,
        default_font_size: f32,
        event_loop: &ActiveEventLoop,
    ) {
        self.reset_blink();
        // While an overlay is open, keys go to it only.
        match &mut self.overlay {
            Some(Overlay::Confirm(_)) => {
                if let Some(answer) = dialog::answer_for(&event.logical_key) {
                    self.answer_dialog(answer, event_loop);
                }
                return;
            }
            Some(Overlay::Palette(palette)) => {
                match palette.key(&event.logical_key, event.text.as_deref()) {
                    Some(outcome) => self.palette_outcome(outcome, default_font_size, event_loop),
                    None => self.window.request_redraw(),
                }
                return;
            }
            None => {}
        }
        let modifiers = self.modifiers;
        let action = self
            .keybindings
            .action(&event.logical_key, event.physical_key, modifiers);
        // With several panes, the pane focus wins over the editor's `Ctrl+Shift+Arrow`.
        if let Some(action @ Action::FocusPane(_)) = action
            && self
                .workspace
                .active_tab()
                .is_some_and(|tab| !tab.is_single())
        {
            self.run_action(action, default_font_size, event_loop);
            return;
        }
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
        if let Some(action) = action {
            self.run_action(action, default_font_size, event_loop);
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
        if let Some(Overlay::Palette(palette)) = &mut self.overlay {
            let rows = match delta {
                MouseScrollDelta::LineDelta(_, y) => y,
                MouseScrollDelta::PixelDelta(position) => {
                    position.y as f32 / self.renderer.ui_line_height(UiFont::Palette)
                }
            };
            // Wheel up shows the rows above.
            palette.scroll_by(-rows.round() as isize);
            self.window.request_redraw();
            return;
        }
        if self.overlay.is_some() {
            return;
        }
        let (position, modifiers) = (self.mouse_position, self.modifiers);
        if let Some(id) = self.pane_under_mouse()
            && let Some(pane) = self.workspace.pane_mut(id)
        {
            pane.mouse_wheel(delta, position, modifiers);
        }
        // Scrolling moves another cell under the mouse, and hides the link tooltip.
        if !self.tooltip_hidden && self.tooltip_uri().is_some() {
            self.window.request_redraw();
        }
        self.tooltip_hidden = true;
        self.update_hover(false);
        self.update_pointer();
    }

    /// Closes the pane `id`; the application exits with the last one.
    fn close_pane(&mut self, id: PaneId, event_loop: &ActiveEventLoop) {
        // The closed pane's selection, and a divider of the tree it leaves, are dropped.
        // A dragged tab may be the one closing: the drag is dropped.
        let shown_tab = self.workspace.tab_of(id) == Some(self.workspace.active_index());
        match self.drag {
            Some(Drag::Selection { pane, .. }) if pane == id => self.drag = None,
            Some(Drag::Divider { .. }) if shown_tab => self.drag = None,
            Some(Drag::Tab { .. }) => self.drag = None,
            _ => {}
        }
        let old = self.workspace.active_pane_id();
        if self.workspace.close_pane(id).is_err() {
            event_loop.exit();
            return;
        }
        // A dialog about panes that are all gone has nothing left to close.
        if let Some(Overlay::Confirm(confirm)) = &self.overlay
            && confirm.target != Target::Window
            && confirm.target.panes(&self.workspace).is_empty()
        {
            self.overlay = None;
        }
        // The new active pane gets the focus-in; the closed one is gone.
        self.focus_changed(old);
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
        // The tab bar shows the title of every tab.
        let bar_shown = tab_bar::shown(self.workspace.tabs().len());
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
                let hovered = self.hovered_cell.is_some_and(|(pane, ..)| pane == id);
                if hovered {
                    self.update_hover(true);
                }
                if active || hovered {
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
                if bar_shown {
                    self.window.request_redraw();
                }
            }
            TermEvent::ResetTitle => {
                pane.title = None;
                if active {
                    self.refresh_title();
                }
                if bar_shown {
                    self.window.request_redraw();
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

    /// Draws every pane of the shown tab, inactive ones dimmed when it shows several,
    /// then the dividers, the active pane border and the tab bar, then the banner, then
    /// the open dialog or command palette.
    fn redraw(&mut self) {
        let dialog = self.dialog_layout();
        let palette = self.palette_layout();
        let tooltip = self.tooltip_layout();
        let overlay = match (&dialog, &palette) {
            (Some(dialog), _) => Some(chrome::scene::Overlay::Dialog(dialog)),
            (_, Some(palette)) => Some(chrome::scene::Overlay::Palette(palette)),
            _ => None,
        };
        let mut banner = self.shown_banner().map(|shown| FrameBanner {
            layout: self.banner_layout(&shown),
            background: self.palette.ui.banner_background,
            text: self.palette.ui.banner_text,
        });
        let layout = self.shown_layout();
        let active = self.workspace.active_pane_id();
        let several = layout.panes.len() > 1;
        let link = self.active_link();
        let mut frames = Vec::with_capacity(layout.panes.len());
        for &(id, _) in &layout.panes {
            let Some(pane) = self.workspace.pane(id) else {
                continue;
            };
            let is_active = Some(id) == active;
            let mut frame = {
                let term = pane.terminal.term.lock();
                let mut timeline = pane::lock(&pane.terminal.timeline);
                let rows = pane::shown_rows(&pane.anchor, &term, &mut timeline);
                let mut frame = Frame::capture(
                    &*term,
                    &timeline,
                    &rows,
                    &self.palette,
                    self.focused && is_active,
                    !is_active || self.cursor_blink.visible(),
                    pane.selected_block,
                );
                frame.attach_images(&graphics::lock(&pane.terminal.images));
                if let Some(hovered) = link.filter(|hovered| hovered.pane == id) {
                    frame.underline_link(&rows, hovered.source, &hovered.link.range);
                }
                frame
            };
            frame.lang = self.lang;
            if several && !is_active {
                frame = frame.dimmed(INACTIVE_PANE_DIM);
            }
            if is_active {
                frame.banner = banner.take();
            }
            frames.push((frame, &pane.layout));
        }
        if frames.is_empty() {
            return;
        }
        let panes: Vec<_> = frames
            .iter()
            .map(|(frame, layout)| PaneFrame { frame, layout })
            .collect();
        let mut bar = self.tab_bar();
        if let (Some(bar), Some(Drag::Tab { index, grab_offset })) = (bar.as_mut(), &self.drag) {
            let (x, y) = self.mouse_pixel();
            bar.dragged(*index, bar.along(x, y) - grab_offset);
        }
        let mut ui = chrome::scene::build(
            &layout,
            active,
            bar.as_ref(),
            overlay,
            &self.palette.ui,
            self.renderer.is_srgb(),
        );
        if let Some(tooltip) = &tooltip {
            chrome::scene::tooltip(tooltip, &self.palette.ui, self.renderer.is_srgb(), &mut ui);
        }
        match self.renderer.render(&panes, &ui) {
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
        | Action::CopyOutput
        | Action::SplitRight
        | Action::SplitDown
        | Action::ClosePane
        | Action::FocusPane(_)
        | Action::ResizePane(_)
        | Action::NextPane
        | Action::PreviousPane
        | Action::ToggleZoom
        | Action::EqualisePanes
        | Action::DetachPane
        | Action::NewTab
        | Action::NextTab
        | Action::PreviousTab
        | Action::GoToTab(_)
        | Action::CommandPalette => {}
    }
}

/// The earlier of two optional deadlines.
/// Size and maximized state of a new window, from `window.startup_mode`. `remembered`
/// is the last window closed, only read in `remember` mode.
fn initial_window(
    config: &WindowConfig,
    remembered: Option<WindowState>,
) -> (LogicalSize<u32>, bool) {
    let configured = LogicalSize::new(config.width, config.height);
    match (config.startup_mode, remembered) {
        (StartupMode::Default, _) | (StartupMode::Remember, None) => (configured, false),
        (StartupMode::Maximized, _) => (configured, true),
        (StartupMode::Remember, Some(state)) => {
            (LogicalSize::new(state.width, state.height), state.maximized)
        }
    }
}

/// Font size of a session after a reload from `old` to `new`, `current` being the size
/// shown (zoom included): the configured size when it changed, dropping the zoom in
/// progress, else `current`.
fn reloaded_font_size(old: &FontConfig, new: &FontConfig, current: f32) -> f32 {
    if new.size == old.size {
        current
    } else {
        new.size
    }
}

fn earliest(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
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
    /// System appearance, which picks the `light` or `dark` theme.
    appearance: Appearance,
    /// Whether the appearance thread runs: started once a configuration needs it.
    appearance_watched: bool,
}

impl App {
    fn create_session(&self, event_loop: &ActiveEventLoop) -> anyhow::Result<Session> {
        let remembered = match self.config.window.startup_mode {
            StartupMode::Remember => window_state::path().and_then(|file| WindowState::load(&file)),
            StartupMode::Default | StartupMode::Maximized => None,
        };
        let (initial_size, maximized) = initial_window(&self.config.window, remembered);
        let attributes = Window::default_attributes()
            .with_title(DEFAULT_TITLE)
            .with_inner_size(initial_size)
            .with_maximized(maximized)
            // Always: winit only honours it at creation on X11, and the opacity can
            // change by hot reload.
            .with_transparent(true);
        let window = Arc::new(event_loop.create_window(attributes)?);
        perf::mark("window created");

        // Shell first, GPU second: the PTY reader thread parses the shell output while
        // the renderer is being created, so the first frame shows it complete.
        let scale = window.scale_factor() as f32;
        let mut glyphs = Glyphs::new(
            self.config.font.family.clone(),
            self.config.font.size * scale,
        );
        glyphs.set_ui_scale(scale);
        perf::mark("glyphs ready");

        let size = window.inner_size();
        // A single tab: no tab bar.
        let layout = GridLayout::new(
            Rect::from_size(size.width, size.height),
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
            self.config.window.background_opacity,
        ))?;

        let clipboard = arboard::Clipboard::new()
            .map_err(|e| log::warn!("clipboard unavailable: {e}"))
            .ok();

        Ok(Session {
            window,
            proxy: self.proxy.clone(),
            term_config: self.config.term_config(),
            renderer,
            workspace,
            drag: None,
            overlay: None,
            palette: self.palette.clone(),
            clipboard,
            focused: true,
            cursor_blink: CursorBlink::new(Instant::now()),
            modifiers: ModifiersState::empty(),
            font_size: self.config.font.size,
            padding: self.config.window.padding,
            mouse_position: PhysicalPosition::default(),
            hovered_cell: None,
            hovered_link: None,
            link_press: None,
            tooltip_hidden: false,
            pointer: None,
            window_title: DEFAULT_TITLE.to_owned(),
            config_banner: self.config_banner.clone(),
            update: UpdateBanner::default(),
            ignored_tag: None,
            lang: self.config.language.resolve(Lang::detect),
            tab_position: self.config.tab_position.into(),
            keybindings: Keybindings::new(&self.config.keybindings),
            first_frame: perf::FirstFrame::default(),
            restore_size: initial_size,
        })
    }

    /// Reads the configuration and its theme again after a change on disk, and applies
    /// them to the open window.
    fn reload_config(&mut self) {
        let (config, errors) = config::reload(&self.config);
        let (config, palette, banner) = with_theme(config, errors, self.appearance);
        let old = std::mem::replace(&mut self.config, config);
        self.watch_appearance();
        self.palette = palette;
        self.config_banner = banner.clone();
        if let Some(session) = self.session.as_mut() {
            session.apply_config(&old, &self.config, &self.palette, banner);
            if old.updates != self.config.updates {
                self.update_checker.apply(&self.config.updates);
            }
        }
    }

    /// Starts the appearance thread when the configuration sets `light` and `dark` and it
    /// does not run yet. The returned receiver gets the first appearance read, so the
    /// first window can open with the right theme.
    fn watch_appearance(&mut self) -> Option<mpsc::Receiver<Appearance>> {
        if self.appearance_watched || !self.config.theme.follows_appearance() {
            return None;
        }
        self.appearance_watched = true;
        let proxy = self.proxy.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        appearance::spawn(move |appearance| {
            // Only the first value is waited for; a full or closed channel is fine.
            let _ = sender.try_send(appearance);
            // The event loop may already be closed during shutdown.
            let _ = proxy.send_event(UserEvent::Appearance(appearance));
        });
        Some(receiver)
    }

    /// A new system appearance: the theme is loaded again when it depends on it.
    fn set_appearance(&mut self, appearance: Appearance) {
        if appearance == self.appearance {
            return;
        }
        self.appearance = appearance;
        if self.config.theme.follows_appearance() {
            self.reload_config();
        }
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

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(session) = &self.session {
            session.save_window_state();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let deadline = self.session.as_mut().and_then(|session| {
            let now = Instant::now();
            let deadline = earliest(session.auto_scroll(now), session.animate(now));
            let deadline = earliest(deadline, session.blink(now));
            session.flush_panes();
            deadline
        });
        event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::ConfigChanged => return self.reload_config(),
            UserEvent::Appearance(appearance) => return self.set_appearance(appearance),
            UserEvent::Terminal(..) | UserEvent::Update(_) => {}
        }
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
            UserEvent::ConfigChanged | UserEvent::Appearance(_) => {}
        }
        session.flush_panes();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let default_font_size = self.config.font.size;
        let Some(session) = self.session.as_mut() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => session.request_close(Target::Window, event_loop),
            WindowEvent::Resized(size) => {
                if !session.window.is_maximized() {
                    session.restore_size = size.to_logical(session.window.scale_factor());
                }
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
            WindowEvent::ModifiersChanged(modifiers) => {
                session.modifiers = modifiers.state();
                // `Ctrl` makes a link active where the application captures the mouse.
                if session.hovered_link.is_some() {
                    session.update_pointer();
                    session.window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                session.key_press(&event, default_font_size, event_loop);
            }
            WindowEvent::MouseWheel { delta, .. } => session.mouse_wheel(delta),
            WindowEvent::CursorMoved { position, .. } => session.cursor_moved(position),
            WindowEvent::CursorLeft { .. } => session.cursor_left(),
            WindowEvent::MouseInput { state, button, .. } => {
                session.mouse_input(state, button, default_font_size, event_loop);
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
    let (config, errors) = config::load();
    with_theme(config, errors, Appearance::default())
}

/// Loads the theme of `config`; `errors`, those of the configuration file, and the
/// theme's are logged and reported by the returned banner.
fn with_theme(
    config: Config,
    mut errors: Vec<ConfigError>,
    appearance: Appearance,
) -> (Config, Palette, Option<Banner>) {
    let name = config.theme.active(appearance);
    let (palette, theme_errors) = theme::load(name, config::config_dir().as_deref());
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
    if let Some(dir) = config::config_dir() {
        let proxy = event_loop.create_proxy();
        watcher::spawn(dir, move || {
            // The event loop may already be closed during shutdown.
            let _ = proxy.send_event(UserEvent::ConfigChanged);
        });
    }
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
        appearance: Appearance::default(),
        appearance_watched: false,
    };
    if let Some(first) = app.watch_appearance()
        && let Ok(appearance) = first.recv_timeout(APPEARANCE_STARTUP_WAIT)
    {
        app.set_appearance(appearance);
    }
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn earliest_keeps_the_sooner_deadline() {
        let now = Instant::now();
        let later = now + Duration::from_millis(5);
        assert_eq!(earliest(Some(later), Some(now)), Some(now));
        assert_eq!(earliest(None, Some(later)), Some(later));
        assert_eq!(earliest(Some(now), None), Some(now));
        assert_eq!(earliest(None, None), None);
    }

    fn font(family: Option<&str>, size: f32) -> FontConfig {
        FontConfig {
            family: family.map(str::to_owned),
            size,
        }
    }

    #[test]
    fn reload_keeps_the_zoom_when_the_font_size_did_not_change() {
        assert_eq!(
            reloaded_font_size(&font(None, 14.0), &font(None, 14.0), 18.0),
            18.0
        );
        // A new family alone keeps the zoom too.
        assert_eq!(
            reloaded_font_size(&font(None, 14.0), &font(Some("Fira Code"), 14.0), 18.0),
            18.0
        );
    }

    #[test]
    fn reload_applies_a_changed_font_size_and_drops_the_zoom() {
        assert_eq!(
            reloaded_font_size(&font(None, 14.0), &font(None, 12.0), 18.0),
            12.0
        );
    }

    fn window_config(startup_mode: StartupMode) -> WindowConfig {
        WindowConfig {
            startup_mode,
            width: 960,
            height: 600,
            padding: 8.0,
            background_opacity: 1.0,
        }
    }

    const REMEMBERED: WindowState = WindowState {
        width: 1280,
        height: 720,
        maximized: true,
    };

    #[test]
    fn default_mode_uses_the_configured_size() {
        let config = window_config(StartupMode::Default);
        let expected = (LogicalSize::new(960, 600), false);
        assert_eq!(initial_window(&config, None), expected);
        assert_eq!(initial_window(&config, Some(REMEMBERED)), expected);
    }

    #[test]
    fn maximized_mode_keeps_the_configured_size_for_unmaximizing() {
        let config = window_config(StartupMode::Maximized);
        let expected = (LogicalSize::new(960, 600), true);
        assert_eq!(initial_window(&config, None), expected);
        assert_eq!(initial_window(&config, Some(REMEMBERED)), expected);
    }

    #[test]
    fn remember_mode_restores_the_last_window() {
        let config = window_config(StartupMode::Remember);
        assert_eq!(
            initial_window(&config, Some(REMEMBERED)),
            (LogicalSize::new(1280, 720), true)
        );
        let unmaximized = WindowState {
            maximized: false,
            ..REMEMBERED
        };
        assert_eq!(
            initial_window(&config, Some(unmaximized)),
            (LogicalSize::new(1280, 720), false)
        );
    }

    #[test]
    fn remember_mode_without_state_is_the_default() {
        let config = window_config(StartupMode::Remember);
        assert_eq!(
            initial_window(&config, None),
            (LogicalSize::new(960, 600), false)
        );
    }
}
