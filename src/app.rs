//! Window and event handling.

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::{Config as TermConfig, TermMode};
#[cfg(target_os = "linux")]
use arboard::{GetExtLinux, LinuxClipboardKind, SetExtLinux};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::appearance::{self, Appearance};
use crate::banner::{self, Banner, BannerHit, BannerLayout, Shown, UpdateBanner, UpdateEffect};
use crate::block_actions::{Direction, Part};
use crate::chrome;
use crate::chrome::command_palette::{
    self, Command, CommandPalette, Outcome, PaletteHit, PaletteLayout,
};
use crate::chrome::context_menu::{
    self, BlockState, Command as MenuCommand, ContextMenu, MenuContext, MenuHit, MenuLayout,
};
use crate::chrome::dialog::{self, Answer, ConfirmClose, DialogHit, DialogLayout, Target};
use crate::chrome::pane_bar::{self, PaneBarLayout};
use crate::chrome::tab_bar::{self, TabBarHit, TabBarLayout, UiMetrics};
use crate::chrome::tooltip::TooltipLayout;
use crate::clipboard::{self, Content};
use crate::config::{
    self, Config, ConfigError, FontConfig, MAX_FONT_SIZE, MIN_FONT_SIZE, StartupMode, ThemeConfig,
    WindowConfig,
};
use crate::config_edit::{self, ConfigEdit};
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
use crate::renderer::{Glyphs, Gpu, PaneFrame, Renderer, UiFont};
use crate::selection;
use crate::settings::{self, LoadErrors, SettingsWindow};
use crate::terminal::{EventProxy, PaneId, Terminal, UserEvent};
use crate::theme;
use crate::update::checker::Checker;
use crate::view::RowSource;
use crate::watcher;
use crate::window_state::{self, WindowState};
use crate::workspace::tree::{Axis, Geometry, Layout, NodePath};
use crate::workspace::{self, DIVIDER_THICKNESS, RESIZE_STEP_CELLS, TabBarPosition, Workspace};

const DEFAULT_TITLE: &str = "Graphene Term";

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
    /// A context menu.
    Menu(ContextMenu),
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
    /// Theme settings and system appearance used by the menu.
    theme: ThemeConfig,
    appearance: Appearance,
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
    /// Set by [`Action::OpenSettings`]: the window is owned by [`App`], which opens it
    /// after the event.
    settings_requested: bool,
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

    /// The window below the banner strip: where the tab bar and the panes are laid out.
    fn chrome_area(&self) -> Rect {
        let size = self.window.inner_size();
        let height = match self.shown_banner() {
            Some(_) => BannerLayout::height(self.renderer.cell_size()),
            None => 0.0,
        };
        banner::below_banner(Rect::from_size(size.width, size.height), height)
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
        // The banner may be gone: the panes take its strip back.
        self.relayout();
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
        Some(TabBarLayout::new(
            self.chrome_area(),
            self.tab_position,
            &titles,
            self.workspace.active_index(),
            self.ui_metrics(UiFont::Chrome),
        ))
    }

    /// Area the tabs divide between their panes, and the sizes they are laid out with.
    fn geometry(&self) -> Geometry {
        Geometry {
            area: self
                .tab_position
                .content_area(self.chrome_area(), self.tab_bar_thickness()),
            cell: self.renderer.cell_size(),
            divider: DIVIDER_THICKNESS,
            title_bar: pane_bar::height(self.ui_metrics(UiFont::Chrome)),
        }
    }

    /// Title bars of the panes of `layout`, the `active` one highlighted.
    fn pane_bars(&self, layout: &Layout, active: Option<PaneId>) -> Vec<PaneBarLayout> {
        let metrics = self.ui_metrics(UiFont::Chrome);
        layout
            .panes
            .iter()
            .map(|&(id, rect)| {
                let title = self
                    .workspace
                    .pane_title(id)
                    .unwrap_or_else(|| DEFAULT_TITLE.to_owned());
                PaneBarLayout::new(id, rect, title, Some(id) == active, metrics)
            })
            .collect()
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
    fn open_palette(&mut self, default_font_size: f32) {
        let recent = command_palette::recent_path()
            .map(|path| command_palette::load_recent(&path))
            .unwrap_or_default();
        let context = command_palette::Context {
            pane: self.workspace.active_pane().map(Pane::palette_context),
            paste: self
                .clipboard
                .as_mut()
                .and_then(|clipboard| clipboard.get_text().ok())
                .is_some_and(|text| !text.is_empty()),
            font_size: self.font_size,
            default_font_size,
        };
        let entries = command_palette::entries(
            &self.workspace,
            self.geometry(),
            &context,
            self.lang,
            &self.keybindings,
        );
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

    /// The open context menu laid out in the window.
    fn menu_layout(&mut self) -> Option<MenuLayout> {
        let Some(Overlay::Menu(menu)) = &self.overlay else {
            return None;
        };
        let size = self.window.inner_size();
        let metrics = self.ui_metrics(UiFont::Palette);
        let renderer = &mut self.renderer;
        Some(MenuLayout::new(
            menu,
            Rect::from_size(size.width, size.height),
            metrics,
            |text| renderer.ui_text_width(UiFont::Palette, text),
        ))
    }

    /// Opens a pane menu at the pointer, retaining the pane's text selection.
    fn open_pane_menu(&mut self, pane_id: PaneId, hit: Hit) {
        let tab_index = self
            .workspace
            .tab_of(pane_id)
            .unwrap_or(self.workspace.active_index());
        let Some(pane) = self.workspace.pane(pane_id) else {
            return;
        };
        let block = pane.block_at(&hit).and_then(|id| {
            let timeline = pane::lock(&pane.terminal.timeline);
            let block = timeline.block(id)?;
            Some(BlockState {
                id,
                command_copyable: crate::block_actions::text(
                    &timeline,
                    id,
                    crate::block_actions::Part::Command,
                )
                .is_some(),
                output_copyable: crate::block_actions::text(
                    &timeline,
                    id,
                    crate::block_actions::Part::Output,
                )
                .is_some(),
                folded: block.folded,
            })
        });
        let items = {
            let tab = self.workspace.tabs().get(tab_index);
            let themes = theme::available(config::config_dir().as_deref());
            let active_theme = self.theme.active(self.appearance).to_owned();
            let context = MenuContext {
                copyable: pane.copy_content().is_some(),
                last_pane: self
                    .workspace
                    .tabs()
                    .iter()
                    .map(|tab| tab.tree.leaves().len())
                    .sum::<usize>()
                    == 1,
                single_pane_tab: tab.is_some_and(|tab| tab.is_single()),
                zoomed: tab.is_some_and(|tab| tab.zoomed),
                only_tab: self.workspace.tabs().len() == 1,
                block,
                themes: &themes,
                active_theme: &active_theme,
            };
            context_menu::pane_menu(&context, &self.keybindings, self.lang)
        };
        self.open_menu(items);
    }

    /// Opens a tab menu at the pointer without selecting the clicked tab.
    fn open_tab_menu(&mut self, index: usize) {
        let Some(tab) = self.workspace.tabs().get(index) else {
            return;
        };
        let themes = theme::available(config::config_dir().as_deref());
        let active_theme = self.theme.active(self.appearance).to_owned();
        let context = MenuContext {
            copyable: false,
            last_pane: self
                .workspace
                .tabs()
                .iter()
                .map(|tab| tab.tree.leaves().len())
                .sum::<usize>()
                == 1,
            single_pane_tab: tab.is_single(),
            zoomed: tab.zoomed,
            only_tab: self.workspace.tabs().len() == 1,
            block: None,
            themes: &themes,
            active_theme: &active_theme,
        };
        let items = context_menu::tab_menu(index, &context, &self.keybindings, self.lang);
        self.open_menu(items);
    }

    fn open_menu(&mut self, items: Vec<context_menu::Item>) {
        let (x, y) = self.mouse_pixel();
        let active_theme = self.theme.active(self.appearance).to_owned();
        self.overlay = Some(Overlay::Menu(
            ContextMenu::new((x, y), items).with_active_theme(active_theme),
        ));
        self.window.request_redraw();
        self.update_pointer();
    }

    /// Applies a menu outcome, closing the menu before running its command.
    fn menu_outcome(
        &mut self,
        outcome: context_menu::Outcome,
        default_font_size: f32,
        event_loop: &ActiveEventLoop,
    ) {
        if !matches!(self.overlay, Some(Overlay::Menu(_))) {
            return;
        }
        self.overlay = None;
        self.window.request_redraw();
        self.update_pointer();
        let context_menu::Outcome::Run(command) = outcome else {
            return;
        };
        match command {
            MenuCommand::Action(action) => self.run_action(action, default_font_size, event_loop),
            MenuCommand::CloseTab(index) => {
                if let Some(target) = Target::tab(&self.workspace, index) {
                    self.request_close(target, event_loop);
                }
            }
            MenuCommand::BlockCopy(id, part) => {
                let text = self
                    .workspace
                    .active_pane()
                    .and_then(|pane| pane.block_text_of(id, part));
                self.set_clipboard(text);
            }
            MenuCommand::BlockFold(id) => {
                if let Some(pane) = self.workspace.active_pane_mut() {
                    pane.toggle_fold_of(id);
                }
            }
            MenuCommand::Theme(name) => self.write_theme(&name),
        }
    }

    fn write_theme(&mut self, name: &str) {
        let Some(file) = config::config_file() else {
            let error = ConfigError::read(
                PathBuf::from("config.toml"),
                "cannot resolve the configuration file path".to_owned(),
            );
            self.show_config_error(error);
            return;
        };
        let key = theme_key(&self.theme, self.appearance);
        if let Err(error) = config_edit::write(&file, |edit: &mut ConfigEdit| edit.set(key, name)) {
            self.show_config_error(error);
        }
    }

    fn show_config_error(&mut self, error: ConfigError) {
        log::warn!("{error}");
        self.config_banner = Banner::config_errors(std::slice::from_ref(&error));
        self.relayout();
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
        if matches!(self.overlay, Some(Overlay::Menu(_))) {
            let hit = self
                .menu_layout()
                .map_or(MenuHit::Outside, |layout| layout.hit(x, y));
            let outcome = match &self.overlay {
                Some(Overlay::Menu(menu))
                    if button == mouse::Button::Left || hit == MenuHit::Outside =>
                {
                    menu.choose(hit)
                }
                _ => None,
            };
            if let Some(outcome) = outcome {
                self.menu_outcome(outcome, default_font_size, event_loop);
            }
            return;
        }
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
            (mouse::Button::Right, TabBarHit::Tab(index) | TabBarHit::Close(index)) => {
                self.open_tab_menu(index);
            }
            _ => {}
        }
    }

    /// Lays out every tab, hidden ones included, so each shell keeps a correct size;
    /// only panes whose cell count changed resize their shell. Each grid fills the pane
    /// under its title bar. The hidden panes of a zoomed tab keep their last size.
    fn relayout(&mut self) {
        let geometry = self.geometry();
        let metrics = self.ui_metrics(UiFont::Chrome);
        let padding = self.padding * self.window.scale_factor() as f32;
        for index in 0..self.workspace.tabs().len() {
            for (id, rect) in self.workspace.layout(index, geometry).panes {
                let (_, content) = pane_bar::split(rect, metrics);
                if let Some(pane) = self.workspace.pane_mut(id) {
                    pane.resize(GridLayout::new(content, geometry.cell, padding));
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
        #[cfg(target_os = "macos")]
        self.window
            .set_transparent(window_transparent(new.window.background_opacity, true));
        self.palette = palette.clone();
        self.theme = new.theme.clone();
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

    /// Requests a redraw while a fold chevron of a shown pane is animated. Returns when
    /// its next frame is due, `None` when no chevron is animated.
    fn fold_animate(&mut self, now: Instant) -> Option<Instant> {
        let shown = self.workspace.active_tab().cloned();
        let deadline = self
            .workspace
            .panes_mut()
            .filter(|(id, _)| shown.as_ref().is_some_and(|tab| tab.shows(*id)))
            .filter_map(|(_, pane)| pane.fold_animations.borrow().deadline(now))
            .min();
        if deadline.is_some() {
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
                #[cfg(target_os = "linux")]
                let result = clipboard::set_text_and_image(text.clone(), png);
                #[cfg(target_os = "macos")]
                let result = self
                    .clipboard
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("clipboard unavailable"))
                    .and_then(|clipboard| clipboard::set_image(clipboard, &png));

                if let Err(error) = result {
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
            mouse::Button::Right => self.open_pane_menu(target, hit),
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
        let menu_layout = self.menu_layout();
        if let (Some(Overlay::Menu(menu)), Some(layout)) = (&mut self.overlay, menu_layout) {
            menu.hover_at(&layout, position.x as f32, position.y as f32);
            self.window.request_redraw();
            self.update_pointer();
            return;
        }
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
        #[cfg(target_os = "linux")]
        {
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
        #[cfg(not(target_os = "linux"))]
        let _ = text;
    }

    fn paste_primary(&mut self) {
        self.reset_blink();
        #[cfg(target_os = "linux")]
        {
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
    }

    fn run_action(&mut self, action: Action, default_font_size: f32, event_loop: &ActiveEventLoop) {
        match action {
            Action::NewTab => self.new_tab(),
            Action::CloseTab => {
                if let Some(target) = Target::tab(&self.workspace, self.workspace.active_index()) {
                    self.request_close(target, event_loop);
                }
            }
            Action::Quit => self.request_close(Target::Window, event_loop),
            Action::CommandPalette => self.open_palette(default_font_size),
            Action::OpenSettings => self.settings_requested = true,
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
            Some(Overlay::Menu(menu)) => {
                if let Some(outcome) = menu.key(&event.logical_key) {
                    self.menu_outcome(outcome, default_font_size, event_loop);
                } else {
                    self.window.request_redraw();
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
        if matches!(&self.overlay, Some(Overlay::Menu(_))) {
            let rows = match delta {
                MouseScrollDelta::LineDelta(_, y) => y,
                MouseScrollDelta::PixelDelta(position) => {
                    position.y as f32 / self.renderer.ui_line_height(UiFont::Palette)
                }
            };
            let visible = self
                .menu_layout()
                .and_then(|layout| layout.submenu.map(|submenu| submenu.visible_rows))
                .unwrap_or(0);
            if let Some(Overlay::Menu(menu)) = &mut self.overlay {
                menu.scroll_submenu(-rows.round() as isize, visible);
            }
            self.window.request_redraw();
            return;
        }
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
    /// then the pane title bars, the dividers, the active pane border and the tab bar, then the banner, then
    /// the open dialog or command palette.
    fn redraw(&mut self) {
        let dialog = self.dialog_layout();
        let palette = self.palette_layout();
        let menu = self.menu_layout();
        let tooltip = self.tooltip_layout();
        let overlay = match (&dialog, &palette, &menu) {
            (Some(dialog), _, _) => Some(chrome::scene::Overlay::Dialog(dialog)),
            (_, Some(palette), _) => Some(chrome::scene::Overlay::Palette(palette)),
            (_, _, Some(menu)) => Some(chrome::scene::Overlay::Menu(menu)),
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
        let now = Instant::now();
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
                pane.fold_animations
                    .borrow_mut()
                    .apply(&mut frame.cards, &rows, now);
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
        let pane_bars = self.pane_bars(&layout, active);
        let mut ui = chrome::scene::build(
            &layout,
            active,
            &pane_bars,
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
        | Action::CloseTab
        | Action::NextTab
        | Action::PreviousTab
        | Action::GoToTab(_)
        | Action::CommandPalette
        | Action::OpenSettings
        | Action::Quit => {}
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
    /// GPU context created with the terminal window, on which other windows create
    /// their surface.
    gpu: Option<Arc<Gpu>>,
    /// The settings window, while open.
    settings: Option<SettingsWindow>,
    update_checker: Checker,
    error: Option<anyhow::Error>,
    /// Banner reporting the errors of the configuration and theme files.
    config_banner: Option<Banner>,
    /// Errors of the last load of the configuration and theme files, shown by the
    /// settings window.
    load_errors: LoadErrors,
    /// System appearance, which picks the `light` or `dark` theme.
    appearance: Appearance,
    /// Whether the appearance thread runs: started once a configuration needs it.
    appearance_watched: bool,
}

/// Whether the window is created transparent for a background `opacity`. Elsewhere
/// always: winit only honours it at creation on X11, and the opacity can change by hot
/// reload. On macOS only below 1.0, and updated on reload: a transparent window also
/// clears the background of the title bar.
fn window_transparent(opacity: f32, macos: bool) -> bool {
    !macos || opacity < 1.0
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
            .with_transparent(window_transparent(
                self.config.window.background_opacity,
                cfg!(target_os = "macos"),
            ));
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
        // A single tab: no tab bar. A configuration error banner is already shown; the
        // update banner only comes later, from the check thread.
        let banner_height = match self.config_banner {
            Some(_) => BannerLayout::height(glyphs.cell_size()),
            None => 0.0,
        };
        let (_, content) = pane_bar::split(
            banner::below_banner(Rect::from_size(size.width, size.height), banner_height),
            UiMetrics {
                line_height: glyphs.ui_line_height(UiFont::Chrome),
                scale,
            },
        );
        let layout = GridLayout::new(
            content,
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

        let display = event_loop.owned_display_handle();
        let (gpu, surface) = pollster::block_on(Gpu::new(window.clone(), display.clone()))?;
        let renderer = Renderer::new(
            gpu,
            surface,
            window.clone(),
            display,
            glyphs,
            self.config.window.background_opacity,
        )?;

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
            theme: self.config.theme.clone(),
            appearance: self.appearance,
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
            settings_requested: false,
        })
    }

    /// Reads the configuration and its theme again after a change on disk, and applies
    /// them to the open window.
    fn reload_config(&mut self) {
        let (config, errors) = config::reload(&self.config);
        let (config, palette, banner, load_errors) = with_theme(config, errors, self.appearance);
        let old = std::mem::replace(&mut self.config, config);
        self.watch_appearance();
        self.palette = palette;
        self.config_banner = banner.clone();
        self.load_errors = load_errors;
        if let Some(settings) = self.settings.as_mut() {
            settings.apply(
                &self.palette,
                self.config.language.resolve(Lang::detect),
                &self.config,
                &self.load_errors,
            );
        }
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

    /// Opens the settings window, or brings it to the front when already open.
    fn open_settings(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(settings) = &self.settings {
            return settings.focus();
        }
        let Some(gpu) = self.gpu.clone() else {
            return;
        };
        let Some(file) = config::config_file() else {
            log::warn!("neither XDG_CONFIG_HOME nor HOME is set: no settings window");
            return;
        };
        let lang = self.config.language.resolve(Lang::detect);
        match SettingsWindow::open(
            event_loop,
            gpu,
            &self.palette,
            lang,
            &self.config,
            &self.load_errors,
            file,
        ) {
            Ok(settings) => self.settings = Some(settings),
            Err(error) => log::error!("cannot open the settings window: {error:#}"),
        }
    }

    /// Opens the settings window when the last event asked for it.
    fn open_requested_settings(&mut self, event_loop: &ActiveEventLoop) {
        let requested = self
            .session
            .as_mut()
            .is_some_and(|session| std::mem::take(&mut session.settings_requested));
        if requested {
            self.open_settings(event_loop);
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
                self.gpu = Some(session.renderer.gpu().clone());
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
        let now = Instant::now();
        let deadline = self.session.as_mut().and_then(|session| {
            let deadline = earliest(session.auto_scroll(now), session.animate(now));
            let deadline = earliest(deadline, session.blink(now));
            let deadline = earliest(deadline, session.fold_animate(now));
            session.flush_panes();
            deadline
        });
        let deadline = earliest(
            deadline,
            self.settings.as_mut().and_then(|settings| {
                settings.wake(now);
                settings.deadline()
            }),
        );
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
                // The banner may show or hide: the panes make room for it.
                session.relayout();
            }
            UserEvent::ConfigChanged | UserEvent::Appearance(_) => {}
        }
        session.flush_panes();
        self.open_requested_settings(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let session_window = self.session.as_ref().map(|session| session.window.id());
        let settings_window = self.settings.as_ref().map(SettingsWindow::id);
        match route(id, session_window, settings_window) {
            Route::Session => {
                self.session_window_event(event_loop, event);
                self.open_requested_settings(event_loop);
            }
            Route::Settings => {
                if let Some(settings) = self.settings.as_mut()
                    && settings.window_event(event_loop, &event) == settings::Outcome::Close
                {
                    self.settings = None;
                }
            }
            Route::Unknown => {}
        }
    }
}

/// Window an event is delivered to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    /// The terminal window.
    Session,
    /// The settings window.
    Settings,
    /// A window `App` does not know (already closed): the event is dropped.
    Unknown,
}

/// Routes an event of window `id`; `session` is the terminal window and `settings` the
/// settings window, when open.
fn route(id: WindowId, session: Option<WindowId>, settings: Option<WindowId>) -> Route {
    if session == Some(id) {
        Route::Session
    } else if settings == Some(id) {
        Route::Settings
    } else {
        Route::Unknown
    }
}

fn is_real_key_press(state: ElementState, is_synthetic: bool) -> bool {
    state == ElementState::Pressed && !is_synthetic
}

impl App {
    /// Handles an event of the terminal window.
    fn session_window_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
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
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } if is_real_key_press(event.state, is_synthetic) => {
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
fn load_config() -> (Config, Palette, Option<Banner>, LoadErrors) {
    let (config, errors) = config::load();
    with_theme(config, errors, Appearance::default())
}

/// Loads the theme of `config`; `errors`, those of the configuration file, and the
/// theme's are logged, reported by the returned banner and returned apart.
fn with_theme(
    config: Config,
    errors: Vec<ConfigError>,
    appearance: Appearance,
) -> (Config, Palette, Option<Banner>, LoadErrors) {
    let name = config.theme.active(appearance);
    let (palette, theme_errors) = theme::load(name, config::config_dir().as_deref());
    let all: Vec<ConfigError> = errors.iter().chain(&theme_errors).cloned().collect();
    for error in &all {
        log::warn!("{error}");
    }
    let load_errors = LoadErrors {
        config: errors,
        theme: theme_errors,
    };
    (config, palette, Banner::config_errors(&all), load_errors)
}

fn theme_key(theme: &ThemeConfig, appearance: Appearance) -> &'static [&'static str] {
    if theme.follows_appearance() {
        match appearance {
            Appearance::Light => &["theme", "light"],
            Appearance::Dark => &["theme", "dark"],
        }
    } else {
        &["theme", "name"]
    }
}

pub fn run() -> anyhow::Result<()> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let update_proxy = event_loop.create_proxy();
    let (config, palette, config_banner, load_errors) = load_config();
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
        gpu: None,
        settings: None,
        error: None,
        config_banner,
        load_errors,
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
    fn macos_window_transparent_only_below_full_opacity() {
        assert!(!window_transparent(1.0, true));
        assert!(window_transparent(0.9, true));
        assert!(window_transparent(1.0, false));
        assert!(window_transparent(0.9, false));
    }

    #[test]
    fn events_of_the_terminal_window_go_to_the_session() {
        let terminal = WindowId::from(1);
        assert_eq!(route(terminal, Some(terminal), None), Route::Session);
        let settings = WindowId::from(2);
        assert_eq!(
            route(terminal, Some(terminal), Some(settings)),
            Route::Session
        );
    }

    #[test]
    fn events_of_the_settings_window_go_to_it() {
        let terminal = WindowId::from(1);
        let settings = WindowId::from(2);
        assert_eq!(
            route(settings, Some(terminal), Some(settings)),
            Route::Settings
        );
        // Still routed once the terminal window is gone, until the app exits.
        assert_eq!(route(settings, None, Some(settings)), Route::Settings);
    }

    #[test]
    fn events_of_an_unknown_window_are_dropped() {
        let terminal = WindowId::from(1);
        assert_eq!(
            route(WindowId::from(2), Some(terminal), None),
            Route::Unknown
        );
        assert_eq!(route(terminal, None, None), Route::Unknown);
        // A closed settings window.
        assert_eq!(
            route(WindowId::from(3), Some(terminal), Some(WindowId::from(2))),
            Route::Unknown
        );
    }

    #[test]
    fn synthetic_focus_key_presses_are_not_forwarded() {
        assert!(is_real_key_press(ElementState::Pressed, false));
        assert!(!is_real_key_press(ElementState::Pressed, true));
        assert!(!is_real_key_press(ElementState::Released, false));
        assert!(!is_real_key_press(ElementState::Released, true));
    }

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

    #[test]
    fn theme_menu_writes_the_matching_appearance_key_when_following_system() {
        let theme = ThemeConfig {
            name: "base".to_owned(),
            light: Some("day".to_owned()),
            dark: Some("night".to_owned()),
        };
        assert_eq!(theme_key(&theme, Appearance::Light), &["theme", "light"]);
        assert_eq!(theme_key(&theme, Appearance::Dark), &["theme", "dark"]);
    }

    #[test]
    fn theme_menu_writes_name_when_not_following_system_appearance() {
        for theme in [
            ThemeConfig {
                name: "base".to_owned(),
                light: None,
                dark: None,
            },
            ThemeConfig {
                name: "base".to_owned(),
                light: Some("day".to_owned()),
                dark: None,
            },
            ThemeConfig {
                name: "base".to_owned(),
                light: None,
                dark: Some("night".to_owned()),
            },
        ] {
            for appearance in [Appearance::Light, Appearance::Dark] {
                assert_eq!(theme_key(&theme, appearance), &["theme", "name"]);
            }
        }
    }
}
