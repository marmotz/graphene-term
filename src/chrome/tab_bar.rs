//! Tab bar (pure, no GPU access): where the settings tab, each tab, its close button
//! and the new tab button sit, which one the mouse is over, and where a dragged tab
//! lands.
//!
//! The bar is always shown: its first slot, a settings icon, is the settings tab. It
//! sits on the edge the configuration picks. Along the top or bottom edge, tabs share its width equally,
//! up to a maximum width; along the left or right edge, they are stacked, one text line
//! high each, in a bar as wide as that maximum. Titles are truncated by the interface
//! text.

use super::badge::Pill;
use crate::layout::Rect;
use crate::progress::{self, ProgressBar, ProgressColors, ProgressView};
use crate::workspace::TabBarPosition;

/// Widest a tab gets, in logical pixels: also the width of a side bar.
const TAB_MAX_WIDTH: f32 = 220.0;
/// Space above and below the tab titles, in logical pixels.
const VERTICAL_PADDING: f32 = 6.0;
/// Space before the title and around the close button, in logical pixels.
const HORIZONTAL_PADDING: f32 = 10.0;
/// Largest share of the window width a side bar takes.
const SIDE_BAR_MAX_SHARE: f32 = 1.0 / 3.0;

/// Sizes of the interface the bar is laid out with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiMetrics {
    /// Line height of the interface font, in physical pixels.
    pub line_height: f32,
    /// Scale factor of the window.
    pub scale: f32,
}

impl UiMetrics {
    /// Height of the bar: a line of interface text and its padding.
    pub fn bar_height(self) -> f32 {
        (self.line_height + 2.0 * VERTICAL_PADDING * self.scale).round()
    }

    /// Space before a title and around a close button, in pixels.
    pub fn padding(self) -> f32 {
        HORIZONTAL_PADDING * self.scale
    }
}

/// Whether a left click on a tab, `clicks` being its rank in its series (see
/// [`crate::selection::ClickTracker`]), opens the tab's rename box rather than grabbing
/// the tab: the second click of a double click.
pub fn opens_rename(clicks: u8) -> bool {
    clicks == 2
}

/// Size of the bar across its edge, in pixels: its height along the top or bottom edge,
/// its width along a side of a window `window_width` pixels wide.
pub fn thickness(position: TabBarPosition, window_width: f32, metrics: UiMetrics) -> f32 {
    if position.is_vertical() {
        (TAB_MAX_WIDTH * metrics.scale)
            .min(window_width * SIDE_BAR_MAX_SHARE)
            .max(0.0)
            .floor()
    } else {
        metrics.bar_height()
    }
}

/// What the mouse is over in the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabBarHit {
    /// The settings tab, first in the bar.
    Settings,
    Tab(usize),
    Close(usize),
    NewTab,
}

/// One tab of the bar.
#[derive(Debug, Clone, PartialEq)]
pub struct TabSlot {
    pub rect: Rect,
    pub title: String,
    /// Left edge of the title, in pixels.
    pub title_x: f32,
    /// Top of the title line, in pixels.
    pub title_y: f32,
    /// Width the title is truncated to.
    pub title_width: f32,
    /// Square close button, at the right end of the tab.
    pub close: Rect,
    pub active: bool,
    /// Whether the mouse is over the close button.
    pub close_hovered: bool,
    /// Unseen notifications of the panes of the tab.
    pub badge: u32,
    /// The badge counting them, before the close button; `None` at 0.
    pub pill: Option<Pill>,
    /// Progress of the tab's active pane, on the tab's bottom edge.
    pub progress: Option<ProgressBar>,
}

/// The bar on its edge of the window, in pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct TabBarLayout {
    pub rect: Rect,
    /// The settings tab: a square icon before the tabs, as big as the `+` button.
    pub settings: Rect,
    /// Whether the settings tab is the one shown: no tab is then active.
    pub settings_active: bool,
    /// Whether the mouse is over the settings tab.
    pub settings_hovered: bool,
    /// Tabs in order; while one is dragged, see [`TabBarLayout::dragged`].
    pub tabs: Vec<TabSlot>,
    /// `+` button after the last tab.
    pub new_tab: Rect,
    /// Whether the mouse is over the `+` button.
    pub new_tab_hovered: bool,
    /// Width of every tab.
    pub tab_width: f32,
    /// Height of every tab.
    pub tab_height: f32,
    /// Whether the tabs are stacked (left or right edge) rather than side by side.
    vertical: bool,
    metrics: UiMetrics,
}

impl TabBarLayout {
    /// Lays out the tabs titled `titles`, tab `active` shown, on the `position` edge of
    /// `window`: side by side with equal widths up to the maximum, or stacked one line
    /// high each, shrunk to fit the window, then the `+` button.
    pub fn new(
        window: Rect,
        position: TabBarPosition,
        titles: &[String],
        active: usize,
        metrics: UiMetrics,
    ) -> Self {
        let thickness = thickness(position, window.width, metrics);
        let line = metrics.bar_height();
        let count = titles.len().max(1) as f32;
        // The settings tab and the `+` button take a line each.
        let buttons = 2.0 * line;
        let rect = match position {
            TabBarPosition::Top => Rect {
                height: thickness,
                ..window
            },
            TabBarPosition::Bottom => Rect {
                y: window.y + window.height - thickness,
                height: thickness,
                ..window
            },
            TabBarPosition::Left => Rect {
                width: thickness,
                ..window
            },
            TabBarPosition::Right => Rect {
                x: window.x + window.width - thickness,
                width: thickness,
                ..window
            },
        };
        let vertical = position.is_vertical();
        let (tab_width, tab_height, new_tab_size) = if vertical {
            let tab_height = ((rect.height - buttons) / count).min(line).max(0.0).floor();
            (rect.width, tab_height, (rect.width, line))
        } else {
            let tab_width = ((rect.width - buttons) / count)
                .min(TAB_MAX_WIDTH * metrics.scale)
                .max(0.0)
                .floor();
            (tab_width, rect.height, (line, rect.height))
        };
        let mut layout = Self {
            rect,
            settings: Rect {
                width: new_tab_size.0,
                height: new_tab_size.1,
                ..rect
            },
            settings_active: false,
            settings_hovered: false,
            tabs: Vec::with_capacity(titles.len()),
            new_tab: rect,
            new_tab_hovered: false,
            tab_width,
            tab_height,
            vertical,
            metrics,
        };
        let (x, y) = layout.at(layout.slot_start(titles.len()));
        layout.new_tab = Rect {
            x,
            y,
            width: new_tab_size.0,
            height: new_tab_size.1,
        };
        for (index, title) in titles.iter().enumerate() {
            let slot = layout.slot(layout.slot_start(index), title.clone(), index == active, 0);
            layout.tabs.push(slot);
        }
        layout
    }

    /// Coordinate of the pixel `(x, y)` along the bar: `x` side by side, `y` stacked.
    pub fn along(&self, x: f32, y: f32) -> f32 {
        if self.vertical { y } else { x }
    }

    /// Size of a tab along the bar.
    fn step(&self) -> f32 {
        if self.vertical {
            self.tab_height
        } else {
            self.tab_width
        }
    }

    /// Start of the tabs along the bar, after the settings tab.
    fn start(&self) -> f32 {
        self.along(self.rect.x, self.rect.y) + self.along(self.settings.width, self.settings.height)
    }

    /// Shows the settings tab rather than the active tab, when `active`.
    pub fn set_settings_active(&mut self, active: bool) {
        self.settings_active = active;
        if active {
            for tab in &mut self.tabs {
                tab.active = false;
            }
        }
    }

    /// Start of slot `index` along the bar.
    fn slot_start(&self, index: usize) -> f32 {
        self.start() + index as f32 * self.step()
    }

    /// Top-left corner of a tab starting at `start` along the bar.
    fn at(&self, start: f32) -> (f32, f32) {
        if self.vertical {
            (self.rect.x, start)
        } else {
            (start, self.rect.y)
        }
    }

    /// Counts the unseen notifications of each tab, in order, in its badge: the title
    /// gives the pill its room. A missing count is 0.
    pub fn set_badges(&mut self, badges: &[u32]) {
        let tabs = std::mem::take(&mut self.tabs);
        self.tabs = tabs
            .into_iter()
            .enumerate()
            .map(|(index, tab)| {
                let badge = badges.get(index).copied().unwrap_or(0);
                self.moved(self.slot_start(index), TabSlot { badge, ..tab })
            })
            .collect();
    }

    /// Shows on each tab, in order, its progress from `progress` (`None`: no bar), on
    /// its bottom edge, along a horizontal bar as along a side one.
    pub fn set_progress(&mut self, progress: &[Option<ProgressView>], colors: ProgressColors) {
        let thickness = progress::bar_thickness(self.metrics.scale);
        for (tab, view) in self.tabs.iter_mut().zip(progress) {
            tab.progress =
                view.map(|view| ProgressBar::along_bottom(tab.rect, thickness, view, colors));
        }
    }

    /// `tab` laid out again starting at `start` along the bar, with its progress.
    fn moved(&self, start: f32, tab: TabSlot) -> TabSlot {
        let slot = self.slot(start, tab.title, tab.active, tab.badge);
        let progress = tab
            .progress
            .map(|bar| ProgressBar::along_bottom(slot.rect, bar.rect.height, bar.view, bar.colors));
        TabSlot { progress, ..slot }
    }

    /// The tab of `title` starting at `start` along the bar, with `badge` unseen
    /// notifications.
    fn slot(&self, start: f32, title: String, active: bool, badge: u32) -> TabSlot {
        let (x, y) = self.at(start);
        let (width, height) = (self.tab_width, self.tab_height);
        let padding = self.metrics.padding();
        let side = self.metrics.line_height.min(width).min(height);
        let close = Rect {
            x: x + width - padding / 2.0 - side,
            y: y + ((height - side) / 2.0).round(),
            width: side,
            height: side,
        };
        let title_x = x + padding;
        let title_y = y + ((height - self.metrics.line_height) / 2.0).round();
        let pill = Pill::new(badge, close.x, title_y, self.metrics);
        let room = pill.as_ref().map_or(0.0, |pill| pill.room(self.metrics));
        TabSlot {
            rect: Rect {
                x,
                y,
                width,
                height,
            },
            title,
            title_x,
            title_y,
            title_width: (close.x - padding / 2.0 - room - title_x).max(0.0),
            close,
            active,
            close_hovered: false,
            badge,
            pill,
            progress: None,
        }
    }

    /// What is under the pixel `(x, y)`: a tab's close button before the tab itself.
    pub fn hit(&self, x: f32, y: f32) -> Option<TabBarHit> {
        if !self.rect.contains(x, y) {
            return None;
        }
        if self.settings.contains(x, y) {
            return Some(TabBarHit::Settings);
        }
        if self.new_tab.contains(x, y) {
            return Some(TabBarHit::NewTab);
        }
        self.tabs.iter().enumerate().find_map(|(index, tab)| {
            if tab.close.contains(x, y) {
                Some(TabBarHit::Close(index))
            } else if tab.rect.contains(x, y) {
                Some(TabBarHit::Tab(index))
            } else {
                None
            }
        })
    }

    /// Slot a dragged tab whose center is at `center` along the bar lands in.
    pub fn drop_index(&self, center: f32) -> usize {
        let last = self.tabs.len().saturating_sub(1);
        if self.step() <= 0.0 {
            return 0;
        }
        (((center - self.start()) / self.step()).floor().max(0.0) as usize).min(last)
    }

    /// Start a dragged tab follows the mouse with, kept inside the tabs.
    fn drag_start(&self, start: f32) -> f32 {
        let max = self.slot_start(self.tabs.len().saturating_sub(1));
        start.clamp(self.start(), max)
    }

    /// Tab `from` dragged with its start along the bar at `start`: it follows the mouse
    /// while the others move to the slots they take once it is dropped. Returns the slot
    /// it lands in; the dragged tab is last in `tabs`, so it is drawn over the others.
    pub fn dragged(&mut self, from: usize, start: f32) -> usize {
        if from >= self.tabs.len() {
            return from;
        }
        let start = self.drag_start(start);
        let to = self.drop_index(start + self.step() / 2.0);
        let mut order: Vec<TabSlot> = self.tabs.clone();
        let moving = order.remove(from);
        order.insert(to, moving);
        let mut tabs = Vec::with_capacity(order.len());
        let mut dragged = None;
        for (index, tab) in order.into_iter().enumerate() {
            if index == to {
                dragged = Some(self.moved(start, tab));
            } else {
                tabs.push(self.moved(self.slot_start(index), tab));
            }
        }
        tabs.extend(dragged);
        self.tabs = tabs;
        to
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::ClickTracker;
    use std::time::{Duration, Instant};

    #[test]
    fn a_double_click_on_a_tab_opens_its_rename_box() {
        let mut clicks = ClickTracker::<usize>::default();
        let start = Instant::now();
        assert!(!opens_rename(clicks.click(1, start)));
        assert!(opens_rename(
            clicks.click(1, start + Duration::from_millis(100))
        ));
        // A third click grabs the tab again.
        assert!(!opens_rename(
            clicks.click(1, start + Duration::from_millis(200))
        ));
        // Two clicks on different tabs are no double click.
        let mut clicks = ClickTracker::<usize>::default();
        clicks.click(0, start);
        assert!(!opens_rename(
            clicks.click(1, start + Duration::from_millis(100))
        ));
    }

    const METRICS: UiMetrics = UiMetrics {
        line_height: 18.0,
        scale: 1.0,
    };

    fn titles(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("tab {i}")).collect()
    }

    /// The bar on top of a window `width` pixels wide.
    fn top(width: f32, titles: &[String], active: usize, metrics: UiMetrics) -> TabBarLayout {
        let window = Rect {
            x: 0.0,
            y: 0.0,
            width,
            height: 600.0,
        };
        TabBarLayout::new(window, TabBarPosition::Top, titles, active, metrics)
    }

    fn window() -> Rect {
        Rect::from_size(1000, 600)
    }

    fn bar_at(position: TabBarPosition, count: usize) -> TabBarLayout {
        TabBarLayout::new(window(), position, &titles(count), 0, METRICS)
    }

    fn progress_colors() -> ProgressColors {
        let gray = |v| alacritty_terminal::vte::ansi::Rgb { r: v, g: v, b: v };
        ProgressColors {
            normal: gray(1),
            error: gray(2),
            paused: gray(3),
            track: gray(4),
        }
    }

    fn view(value: u8) -> ProgressView {
        ProgressView {
            state: crate::progress::ProgressState::Normal,
            value,
            offset: 0.0,
        }
    }

    #[test]
    fn progress_sits_on_the_bottom_edge_of_its_tab() {
        for position in [TabBarPosition::Top, TabBarPosition::Left] {
            let mut bar = bar_at(position, 3);
            bar.set_progress(&[None, Some(view(40))], progress_colors());
            assert_eq!(bar.tabs[0].progress, None);
            assert_eq!(bar.tabs[2].progress, None);
            let tab = &bar.tabs[1];
            let progress = tab.progress.expect("a progress bar");
            assert_eq!(
                progress.rect,
                Rect {
                    x: tab.rect.x,
                    y: tab.rect.y + tab.rect.height - 2.0,
                    width: tab.rect.width,
                    height: 2.0,
                }
            );
            assert_eq!(progress.view, view(40));
            assert_eq!(progress.colors, progress_colors());
        }
    }

    #[test]
    fn progress_bar_scales_with_the_window() {
        let hidpi = UiMetrics {
            line_height: 36.0,
            scale: 2.0,
        };
        let mut bar = TabBarLayout::new(window(), TabBarPosition::Top, &titles(2), 0, hidpi);
        bar.set_progress(&[Some(view(10))], progress_colors());
        assert_eq!(bar.tabs[0].progress.unwrap().rect.height, 4.0);
    }

    #[test]
    fn progress_follows_its_tab_through_badges_and_drags() {
        let mut bar = bar_at(TabBarPosition::Top, 3);
        bar.set_progress(&[Some(view(40)), None, None], progress_colors());
        bar.set_badges(&[2, 0, 0]);
        let tab = &bar.tabs[0];
        assert_eq!(tab.progress.unwrap().rect.x, tab.rect.x);
        let to = bar.dragged(0, bar.tab_width * 1.6);
        let dragged = bar.tabs.last().unwrap();
        assert_eq!(dragged.title, "tab 0");
        assert_ne!(to, 0);
        let progress = dragged
            .progress
            .expect("the dragged tab keeps its progress");
        assert_eq!(progress.rect.x, dragged.rect.x);
        assert_eq!(progress.rect.width, dragged.rect.width);
        assert!(bar.tabs[..2].iter().all(|tab| tab.progress.is_none()));
    }

    #[test]
    fn the_settings_tab_is_a_square_before_the_tabs() {
        let bar = top(2000.0, &titles(2), 1, METRICS);
        assert_eq!(
            bar.settings,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 30.0,
                height: 30.0
            }
        );
        assert!(!bar.settings_active && !bar.settings_hovered);
        assert_eq!(bar.tabs[0].rect.x, bar.settings.width);
        // A single tab still gets the bar, the settings tab first.
        let single = top(2000.0, &titles(1), 0, METRICS);
        assert_eq!(single.tabs.len(), 1);
        assert_eq!(single.hit(5.0, 15.0), Some(TabBarHit::Settings));
    }

    #[test]
    fn the_shown_settings_tab_leaves_no_tab_active() {
        let mut bar = top(2000.0, &titles(3), 1, METRICS);
        assert!(bar.tabs[1].active);
        bar.set_settings_active(true);
        assert!(bar.settings_active);
        assert!(bar.tabs.iter().all(|tab| !tab.active));
        // Left, the tabs keep what the workspace shows.
        let mut bar = top(2000.0, &titles(3), 1, METRICS);
        bar.set_settings_active(false);
        assert!(!bar.settings_active && bar.tabs[1].active);
    }

    #[test]
    fn bar_height_is_a_text_line_and_its_padding() {
        assert_eq!(METRICS.bar_height(), 30.0);
        let hidpi = UiMetrics {
            line_height: 36.0,
            scale: 2.0,
        };
        assert_eq!(hidpi.bar_height(), 60.0);
    }

    #[test]
    fn tabs_share_the_width_up_to_a_maximum() {
        let wide = top(2000.0, &titles(3), 0, METRICS);
        assert_eq!(wide.tab_width, TAB_MAX_WIDTH);
        let xs: Vec<f32> = wide.tabs.iter().map(|t| t.rect.x).collect();
        // After the square settings tab.
        assert_eq!(xs, [30.0, 250.0, 470.0]);
        // The `+` button follows the last tab.
        assert_eq!(wide.new_tab.x, 690.0);
        assert_eq!(wide.new_tab.width, wide.rect.height);

        // Shrunk to fit: the settings tab, 4 tabs and the button in 460 px.
        let narrow = top(460.0, &titles(4), 0, METRICS);
        assert_eq!(narrow.tab_width, 100.0);
        assert!(narrow.new_tab.x + narrow.new_tab.width <= 460.0);
    }

    #[test]
    fn titles_get_the_room_left_by_the_padding_and_the_close_button() {
        let bar = top(2000.0, &titles(2), 1, METRICS);
        let tab = &bar.tabs[1];
        assert!(tab.active && !bar.tabs[0].active);
        assert_eq!(tab.title_x, tab.rect.x + 10.0);
        assert!(tab.title_x + tab.title_width <= tab.close.x);
        assert!(tab.close.x + tab.close.width <= tab.rect.x + tab.rect.width);
        assert_eq!(tab.title, "tab 1");
        // Shrunk tabs leave less room to their title, never a negative one.
        let narrow = top(130.0, &titles(4), 0, METRICS);
        assert!(narrow.tabs[0].title_width < tab.title_width);
        assert!(narrow.tabs.iter().all(|t| t.title_width >= 0.0));
    }

    #[test]
    fn a_badge_takes_its_room_from_the_title() {
        let plain = top(2000.0, &titles(3), 0, METRICS);
        let mut bar = plain.clone();
        bar.set_badges(&[0, 5, 1000]);
        // No badge: the tab is unchanged.
        assert_eq!(bar.tabs[0], plain.tabs[0]);
        assert_eq!(bar.tabs[0].badge, 0);
        let tab = &bar.tabs[1];
        let pill = tab.pill.as_ref().unwrap();
        assert_eq!(tab.badge, 5);
        assert_eq!(pill.label, "5");
        assert_eq!(
            tab.title_width,
            plain.tabs[1].title_width - pill.room(METRICS)
        );
        assert!(tab.title_x + tab.title_width < pill.rect.x);
        assert!(pill.rect.x + pill.rect.width < tab.close.x);
        assert_eq!(bar.tabs[2].pill.as_ref().unwrap().label, "99+");
        // A missing count is 0.
        bar.set_badges(&[]);
        assert_eq!(bar.tabs, plain.tabs);
    }

    #[test]
    fn a_dragged_tab_keeps_its_badge() {
        let mut bar = top(2000.0, &titles(3), 0, METRICS);
        bar.set_badges(&[4, 0, 0]);
        let to = bar.dragged(0, 400.0);
        assert_eq!(to, 2);
        let dragged = bar.tabs.last().unwrap();
        assert_eq!(dragged.title, "tab 0");
        assert_eq!(dragged.badge, 4);
        assert!(dragged.pill.is_some());
        assert!(bar.tabs[..2].iter().all(|t| t.pill.is_none()));
    }

    #[test]
    fn hit_finds_the_settings_the_tab_its_close_button_and_the_new_tab_button() {
        let bar = top(2000.0, &titles(2), 0, METRICS);
        assert_eq!(bar.hit(5.0, 15.0), Some(TabBarHit::Settings));
        assert_eq!(bar.hit(35.0, 15.0), Some(TabBarHit::Tab(0)));
        assert_eq!(bar.hit(255.0, 15.0), Some(TabBarHit::Tab(1)));
        let close = bar.tabs[1].close;
        assert_eq!(
            bar.hit(close.x + 1.0, close.y + 1.0),
            Some(TabBarHit::Close(1))
        );
        assert_eq!(bar.hit(475.0, 15.0), Some(TabBarHit::NewTab));
        // Past the button, and below the bar: nothing.
        assert_eq!(bar.hit(1000.0, 15.0), None);
        assert_eq!(bar.hit(5.0, 30.0), None);
    }

    #[test]
    fn drop_index_is_the_slot_under_the_dragged_tab_center() {
        let bar = top(2000.0, &titles(3), 0, METRICS);
        assert_eq!(bar.drop_index(-50.0), 0);
        assert_eq!(bar.drop_index(249.0), 0);
        assert_eq!(bar.drop_index(250.0), 1);
        assert_eq!(bar.drop_index(500.0), 2);
        assert_eq!(bar.drop_index(5000.0), 2);
    }

    #[test]
    fn dragged_tab_follows_the_mouse_and_the_others_shift() {
        let mut bar = top(2000.0, &titles(3), 0, METRICS);
        // Tab 0 dragged right, its center over the last slot.
        let to = bar.dragged(0, 400.0);
        assert_eq!(to, 2);
        let placed: Vec<(&str, f32)> = bar
            .tabs
            .iter()
            .map(|t| (t.title.as_str(), t.rect.x))
            .collect();
        assert_eq!(
            placed,
            [("tab 1", 30.0), ("tab 2", 250.0), ("tab 0", 400.0)]
        );
        assert!(bar.tabs[2].active);
    }

    #[test]
    fn dragged_tab_stays_inside_the_tabs() {
        let mut bar = top(2000.0, &titles(2), 0, METRICS);
        // Never over the settings tab.
        assert_eq!(bar.dragged(1, -100.0), 0);
        assert_eq!(bar.tabs.last().unwrap().rect.x, 30.0);
        let mut bar = top(2000.0, &titles(2), 0, METRICS);
        assert_eq!(bar.dragged(0, 1500.0), 1);
        assert_eq!(bar.tabs.last().unwrap().rect.x, 250.0);
    }

    #[test]
    fn thickness_is_a_line_high_or_a_tab_wide() {
        assert_eq!(thickness(TabBarPosition::Top, 1000.0, METRICS), 30.0);
        assert_eq!(thickness(TabBarPosition::Bottom, 1000.0, METRICS), 30.0);
        assert_eq!(
            thickness(TabBarPosition::Left, 1000.0, METRICS),
            TAB_MAX_WIDTH
        );
        assert_eq!(
            thickness(TabBarPosition::Right, 1000.0, METRICS),
            TAB_MAX_WIDTH
        );
        // A side bar never takes more than a third of a narrow window.
        assert_eq!(thickness(TabBarPosition::Left, 300.0, METRICS), 100.0);
    }

    #[test]
    fn bar_sits_on_its_edge() {
        let rect = |x, y, width, height| Rect {
            x,
            y,
            width,
            height,
        };
        assert_eq!(
            bar_at(TabBarPosition::Top, 2).rect,
            rect(0.0, 0.0, 1000.0, 30.0)
        );
        assert_eq!(
            bar_at(TabBarPosition::Bottom, 2).rect,
            rect(0.0, 570.0, 1000.0, 30.0)
        );
        assert_eq!(
            bar_at(TabBarPosition::Left, 2).rect,
            rect(0.0, 0.0, 220.0, 600.0)
        );
        assert_eq!(
            bar_at(TabBarPosition::Right, 2).rect,
            rect(780.0, 0.0, 220.0, 600.0)
        );
    }

    #[test]
    fn bar_and_content_area_share_the_window() {
        for position in [
            TabBarPosition::Top,
            TabBarPosition::Bottom,
            TabBarPosition::Left,
            TabBarPosition::Right,
        ] {
            let bar = bar_at(position, 2);
            let area = position.content_area(window(), thickness(position, 1000.0, METRICS));
            assert_eq!(
                bar.rect.width * bar.rect.height + area.width * area.height,
                1000.0 * 600.0,
                "{position:?}"
            );
            // The area starts where the bar ends, or ends where it starts.
            let touches = area.x + area.width == bar.rect.x
                || bar.rect.x + bar.rect.width == area.x
                || area.y + area.height == bar.rect.y
                || bar.rect.y + bar.rect.height == area.y;
            assert!(touches, "{position:?}");
        }
    }

    #[test]
    fn bottom_tabs_are_side_by_side_on_the_bottom_edge() {
        let bar = bar_at(TabBarPosition::Bottom, 2);
        assert!(bar.tabs.iter().all(|t| t.rect.y == 570.0));
        assert_eq!(bar.tabs[1].rect.x, 250.0);
        assert_eq!(bar.new_tab.y, 570.0);
        assert_eq!(bar.settings.y, 570.0);
        assert_eq!(bar.tabs[0].title_y, 576.0);
        assert_eq!(bar.hit(5.0, 580.0), Some(TabBarHit::Settings));
        assert_eq!(bar.hit(35.0, 580.0), Some(TabBarHit::Tab(0)));
        assert_eq!(bar.hit(5.0, 10.0), None);
    }

    #[test]
    fn side_tabs_are_stacked_one_line_high() {
        for (position, x) in [(TabBarPosition::Left, 0.0), (TabBarPosition::Right, 780.0)] {
            let bar = bar_at(position, 3);
            assert_eq!((bar.tab_width, bar.tab_height), (220.0, 30.0));
            let corners: Vec<(f32, f32)> = bar.tabs.iter().map(|t| (t.rect.x, t.rect.y)).collect();
            // Below the settings tab, a line high and as wide as the bar.
            assert_eq!(
                bar.settings,
                Rect {
                    x,
                    y: 0.0,
                    width: 220.0,
                    height: 30.0
                }
            );
            assert_eq!(corners, [(x, 30.0), (x, 60.0), (x, 90.0)]);
            // The `+` button follows the last tab, as wide as the bar.
            assert_eq!(
                bar.new_tab,
                Rect {
                    x,
                    y: 120.0,
                    width: 220.0,
                    height: 30.0
                }
            );
            let tab = &bar.tabs[2];
            assert_eq!((tab.title_x, tab.title_y), (x + 10.0, 96.0));
            assert_eq!(tab.close.y, 96.0);
            assert!(tab.close.x + tab.close.width <= x + 220.0);
            assert_eq!(bar.hit(x + 5.0, 5.0), Some(TabBarHit::Settings));
            assert_eq!(bar.hit(x + 5.0, 75.0), Some(TabBarHit::Tab(1)));
            assert_eq!(bar.hit(x + 5.0, 125.0), Some(TabBarHit::NewTab));
            assert_eq!(bar.hit(x + 5.0, 300.0), None);
        }
    }

    #[test]
    fn side_tabs_shrink_to_fit_the_height() {
        let window = Rect::from_size(1000, 360);
        let bar = TabBarLayout::new(window, TabBarPosition::Left, &titles(20), 0, METRICS);
        assert_eq!(bar.tab_height, 15.0);
        assert!(bar.new_tab.y + bar.new_tab.height <= 360.0);
    }

    #[test]
    fn side_tabs_are_dragged_along_the_height() {
        let mut bar = bar_at(TabBarPosition::Left, 3);
        assert_eq!(bar.along(5.0, 45.0), 45.0);
        assert_eq!(bar.drop_index(75.0), 1);
        let to = bar.dragged(0, 80.0);
        assert_eq!(to, 2);
        let placed: Vec<(&str, f32)> = bar
            .tabs
            .iter()
            .map(|t| (t.title.as_str(), t.rect.y))
            .collect();
        assert_eq!(placed, [("tab 1", 30.0), ("tab 2", 60.0), ("tab 0", 80.0)]);
        assert!(bar.tabs.iter().all(|t| t.rect.x == 0.0));
    }

    #[test]
    fn bar_starts_below_the_banner() {
        // The window below a 40 px banner strip.
        let window = Rect {
            x: 0.0,
            y: 40.0,
            width: 1000.0,
            height: 560.0,
        };
        for position in [
            TabBarPosition::Top,
            TabBarPosition::Bottom,
            TabBarPosition::Left,
            TabBarPosition::Right,
        ] {
            let bar = TabBarLayout::new(window, position, &titles(3), 0, METRICS);
            assert!(bar.rect.y >= 40.0, "{position:?}: bar at {}", bar.rect.y);
            assert!(
                bar.rect.y + bar.rect.height <= 600.0,
                "{position:?}: bar past the window bottom"
            );
            for tab in &bar.tabs {
                assert!(tab.rect.y >= 40.0, "{position:?}: tab at {}", tab.rect.y);
            }
        }
        let top = TabBarLayout::new(window, TabBarPosition::Top, &titles(3), 0, METRICS);
        assert_eq!(top.rect.y, 40.0);
        let left = TabBarLayout::new(window, TabBarPosition::Left, &titles(3), 0, METRICS);
        assert_eq!(left.rect.y, 40.0);
        assert_eq!(left.rect.height, 560.0);
        assert_eq!(left.settings.y, 40.0);
        assert_eq!(left.tabs[0].rect.y, 70.0);
    }
}
