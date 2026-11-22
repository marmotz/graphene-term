//! Window and event handling.

mod application;
mod geometry;
mod notices;
mod panes;
mod refresh;
mod terminal_events;
pub use self::application::*;

mod copy_paste;
mod hover;
mod keyboard;
mod overlays;
mod pane_drag;
mod pointer;
mod redraw;

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::{Config as TermConfig, TermMode};
#[cfg(target_os = "linux")]
use arboard::{GetExtLinux, LinuxClipboardKind, SetExtLinux};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
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
use crate::chrome::pane_bar::{self, PaneBarAction, PaneBarHit, PaneBarLayout};
use crate::chrome::tab_bar::{self, TabBarHit, TabBarLayout, UiMetrics};
use crate::chrome::tooltip::TooltipLayout;
use crate::clipboard::{self, Content};
use crate::config::{
    self, Config, ConfigError, FontConfig, MAX_FONT_SIZE, MIN_FONT_SIZE, NotificationsConfig,
    StartupMode, ThemeConfig, WindowConfig,
};
use crate::config_edit::{self, ConfigEdit};
use crate::cursor_blink::CursorBlink;
use crate::easter_eggs::secrets::{self, Secret};
use crate::easter_eggs::{
    self,
    art::ArtLayout,
    honeycomb::Honeycomb,
    konami::KonamiDetector,
    special_days::{self, Decoration},
};
use crate::frame::{Frame, FrameBanner, INACTIVE_PANE_DIM};
use crate::frame_cache::FrameCache;
use crate::graphics;
use crate::i18n::Lang;
use crate::input::{self, Action, Delivery};
use crate::input_editor;
use crate::keybindings::Keybindings;
use crate::layout::{CellSize, GridLayout, Hit, Rect};
use crate::links::{self, Link};
use crate::mouse;
use crate::notifications::backend::{Command as NotificationCommand, Notifier};
use crate::notifications::{self, Badge, Notice};
use crate::opener;
use crate::palette::Palette;
use crate::pane::{self, Pane};
use crate::perf;
use crate::renderer::{Glyphs, Gpu, PaneFrame, Renderer, UiFont};
use crate::selection::{self, ClickTracker};
use crate::settings::{self, LoadErrors, SettingsWindow};
use crate::terminal::{EventProxy, PaneId, Terminal, UserEvent};
use crate::theme;
use crate::update::checker::Checker;
use crate::view::RowSource;
use crate::watcher;
use crate::window_state::{self, WindowState};
use crate::workspace::drop::{self, DropTarget};
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
    /// A pane grabbed by its title bar: pending until the pointer moves past
    /// [`pane_bar::PANE_DRAG_THRESHOLD`], then dragged onto a drop target.
    Pane {
        pane: PaneId,
        /// Where the title was pressed.
        press: PhysicalPosition<f64>,
        started: bool,
        /// The tab was zoomed when the drag started: restored on cancel.
        was_zoomed: bool,
        /// Tab of the bar under the pointer and since when, for the switch after
        /// [`drop::TAB_HOVER_DELAY`].
        tab_hover: Option<(usize, Instant)>,
        /// Where the pane would land if released now.
        target: Option<DropTarget>,
    },
    /// A left press with `Ctrl` in the content of a pane, never reported to the
    /// application: a click toggles the pane in the broadcast set; a drag past
    /// [`pane_bar::PANE_DRAG_THRESHOLD`] outside the captured areas becomes a rectangular
    /// selection, and does nothing inside them.
    BroadcastClick {
        pane: PaneId,
        /// Where the content was pressed.
        press: PhysicalPosition<f64>,
        /// The press was in an area the application owns.
        captured: bool,
    },
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
    /// The ASCII drawing of graphene.
    Art,
}

/// A theme of the context menu previewed in the terminal, and the colors to restore
/// when the menu closes without choosing it.
#[derive(Debug, Clone)]
struct MenuTheme {
    name: String,
    saved: Palette,
    /// Whether the panes had scanlines before the preview.
    saved_scanlines: bool,
}

/// What the terminal colors do for the theme the context menu points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuThemeStep<'a> {
    /// The colors shown are the right ones.
    Keep,
    /// Show this theme.
    Show(&'a str),
    /// Show the colors saved before the preview again.
    Restore,
}

/// The step from the theme previewed so far, `shown`, to the one the menu points at.
fn menu_theme_step<'a>(shown: Option<&str>, wanted: Option<&'a str>) -> MenuThemeStep<'a> {
    match (shown, wanted) {
        (shown, wanted) if shown == wanted => MenuThemeStep::Keep,
        (_, Some(name)) => MenuThemeStep::Show(name),
        (Some(_), None) => MenuThemeStep::Restore,
        (None, None) => MenuThemeStep::Keep,
    }
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
    /// The theme of the context menu previewed in the terminal.
    menu_theme: Option<MenuTheme>,
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
    /// Clicks on the pane bars, per pane: a double click on a title toggles the zoom.
    pane_bar_clicks: ClickTracker<PaneId>,
    /// Whether the link tooltip stays hidden: set by the wheel, cleared when the mouse
    /// moves.
    tooltip_hidden: bool,
    /// Pointer shape last applied to the window; `None` before the first update.
    pointer: Option<winit::window::CursorIcon>,
    /// Close button of a tab or a pane bar, or `+` button, under the mouse: drawn
    /// hovered.
    hovered_button: Option<chrome::ChromeButton>,
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
    redraw_stats: perf::RedrawStats,
    /// Redraw put off until the renderer takes a new frame.
    redraw_at: Option<Instant>,
    /// First frame held back until the window is maximized, see [`redraw::MaximizeHold`].
    maximize_hold: Option<redraw::MaximizeHold>,
    /// Frame of each shown pane drawn last, reused while the pane is unchanged.
    frames: FrameCache<PaneId, redraw::FrameKey>,
    /// Last inner size seen while the window was not maximized, in logical pixels:
    /// winit has no getter for the size a maximized window is restored to.
    restore_size: LogicalSize<u32>,
    /// Set by [`Action::OpenSettings`]: the window is owned by [`App`], which opens it
    /// after the event.
    settings_requested: bool,
    /// Whether the window was sized by `--columns` or `--lines`: its size is then not
    /// saved to `window.json`.
    sized_by_grid: bool,
    /// Follows the keys typed at the prompt towards the Konami code.
    konami: KonamiDetector,
    /// The honeycomb shown over the active pane once the Konami code is typed.
    honeycomb: Option<Honeycomb>,
    /// Decoration of the window title for the special day today, `None` on an
    /// ordinary day.
    today: Option<Decoration>,
    /// Next local midnight, when `today` is computed again; `None` if the local time is
    /// unknown.
    midnight: Option<Instant>,
    /// Which notices raise a desktop notification.
    notifications: NotificationsConfig,
    /// Hands the notifications and the badge to the desktop.
    notifier: Notifier,
    /// Unseen notices shown on the window's icon.
    badge: Badge,
}

impl Session {}

#[cfg(test)]
mod tests {
    #[test]
    fn the_menu_theme_is_shown_then_restored() {
        use super::{MenuThemeStep, menu_theme_step};
        assert_eq!(menu_theme_step(None, None), MenuThemeStep::Keep);
        assert_eq!(
            menu_theme_step(None, Some("dark")),
            MenuThemeStep::Show("dark")
        );
        assert_eq!(
            menu_theme_step(Some("dark"), Some("dark")),
            MenuThemeStep::Keep
        );
        assert_eq!(
            menu_theme_step(Some("dark"), Some("light")),
            MenuThemeStep::Show("light")
        );
        assert_eq!(menu_theme_step(Some("dark"), None), MenuThemeStep::Restore);
    }

    use super::*;

    #[test]
    fn theme_menu_writes_the_matching_appearance_key_when_following_system() {
        let theme = ThemeConfig {
            name: "base".to_owned(),
            light: Some("day".to_owned()),
            dark: Some("night".to_owned()),
            unlocked: Vec::new(),
        };
        assert_eq!(theme.active_key(Appearance::Light), &["theme", "light"]);
        assert_eq!(theme.active_key(Appearance::Dark), &["theme", "dark"]);
    }

    #[test]
    fn theme_menu_writes_name_when_not_following_system_appearance() {
        for theme in [
            ThemeConfig {
                name: "base".to_owned(),
                light: None,
                dark: None,
                unlocked: Vec::new(),
            },
            ThemeConfig {
                name: "base".to_owned(),
                light: Some("day".to_owned()),
                dark: None,
                unlocked: Vec::new(),
            },
            ThemeConfig {
                name: "base".to_owned(),
                light: None,
                dark: Some("night".to_owned()),
                unlocked: Vec::new(),
            },
        ] {
            for appearance in [Appearance::Light, Appearance::Dark] {
                assert_eq!(theme.active_key(appearance), &["theme", "name"]);
            }
        }
    }
}
