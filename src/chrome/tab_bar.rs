//! Tab bar (pure, no GPU access): where each tab, its close button and the new tab
//! button sit, which one the mouse is over, and where a dragged tab lands.
//!
//! The bar is only shown while the window has more than one tab. It sits on the edge
//! the configuration picks. Along the top or bottom edge, tabs share its width equally,
//! up to a maximum width; along the left or right edge, they are stacked, one text line
//! high each, in a bar as wide as that maximum. Titles are truncated by the interface
//! text.

use crate::layout::Rect;
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

    fn padding(self) -> f32 {
        HORIZONTAL_PADDING * self.scale
    }
}

/// Whether a window with `tab_count` tabs shows the bar.
pub fn shown(tab_count: usize) -> bool {
    tab_count > 1
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
}

/// The bar on its edge of the window, in pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct TabBarLayout {
    pub rect: Rect,
    /// Tabs in order; while one is dragged, see [`TabBarLayout::dragged`].
    pub tabs: Vec<TabSlot>,
    /// `+` button after the last tab.
    pub new_tab: Rect,
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
            let tab_height = ((rect.height - line) / count).min(line).max(0.0).floor();
            (rect.width, tab_height, (rect.width, line))
        } else {
            let tab_width = ((rect.width - line) / count)
                .min(TAB_MAX_WIDTH * metrics.scale)
                .max(0.0)
                .floor();
            (tab_width, rect.height, (line, rect.height))
        };
        let mut layout = Self {
            rect,
            tabs: Vec::with_capacity(titles.len()),
            new_tab: rect,
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
            let slot = layout.slot(layout.slot_start(index), title.clone(), index == active);
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

    /// Start of the bar along it.
    fn start(&self) -> f32 {
        self.along(self.rect.x, self.rect.y)
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

    /// The tab of `title` starting at `start` along the bar.
    fn slot(&self, start: f32, title: String, active: bool) -> TabSlot {
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
        TabSlot {
            rect: Rect {
                x,
                y,
                width,
                height,
            },
            title,
            title_x,
            title_y: y + ((height - self.metrics.line_height) / 2.0).round(),
            title_width: (close.x - padding / 2.0 - title_x).max(0.0),
            close,
            active,
        }
    }

    /// What is under the pixel `(x, y)`: a tab's close button before the tab itself.
    pub fn hit(&self, x: f32, y: f32) -> Option<TabBarHit> {
        if !self.rect.contains(x, y) {
            return None;
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
                dragged = Some(self.slot(start, tab.title, tab.active));
            } else {
                tabs.push(self.slot(self.slot_start(index), tab.title, tab.active));
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

    #[test]
    fn bar_is_shown_from_two_tabs() {
        assert!(!shown(0));
        assert!(!shown(1));
        assert!(shown(2));
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
        assert_eq!(xs, [0.0, 220.0, 440.0]);
        // The `+` button follows the last tab.
        assert_eq!(wide.new_tab.x, 660.0);
        assert_eq!(wide.new_tab.width, wide.rect.height);

        // Shrunk to fit: 4 tabs and the button in 430 px.
        let narrow = top(430.0, &titles(4), 0, METRICS);
        assert_eq!(narrow.tab_width, 100.0);
        assert!(narrow.new_tab.x + narrow.new_tab.width <= 430.0);
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
    fn hit_finds_the_tab_its_close_button_and_the_new_tab_button() {
        let bar = top(2000.0, &titles(2), 0, METRICS);
        assert_eq!(bar.hit(5.0, 15.0), Some(TabBarHit::Tab(0)));
        assert_eq!(bar.hit(225.0, 15.0), Some(TabBarHit::Tab(1)));
        let close = bar.tabs[1].close;
        assert_eq!(
            bar.hit(close.x + 1.0, close.y + 1.0),
            Some(TabBarHit::Close(1))
        );
        assert_eq!(bar.hit(445.0, 15.0), Some(TabBarHit::NewTab));
        // Past the button, and below the bar: nothing.
        assert_eq!(bar.hit(1000.0, 15.0), None);
        assert_eq!(bar.hit(5.0, 30.0), None);
    }

    #[test]
    fn drop_index_is_the_slot_under_the_dragged_tab_center() {
        let bar = top(2000.0, &titles(3), 0, METRICS);
        assert_eq!(bar.drop_index(-50.0), 0);
        assert_eq!(bar.drop_index(219.0), 0);
        assert_eq!(bar.drop_index(220.0), 1);
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
        assert_eq!(placed, [("tab 1", 0.0), ("tab 2", 220.0), ("tab 0", 400.0)]);
        assert!(bar.tabs[2].active);
    }

    #[test]
    fn dragged_tab_stays_inside_the_tabs() {
        let mut bar = top(2000.0, &titles(2), 0, METRICS);
        assert_eq!(bar.dragged(1, -100.0), 0);
        assert_eq!(bar.tabs.last().unwrap().rect.x, 0.0);
        let mut bar = top(2000.0, &titles(2), 0, METRICS);
        assert_eq!(bar.dragged(0, 1500.0), 1);
        assert_eq!(bar.tabs.last().unwrap().rect.x, 220.0);
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
        assert_eq!(bar.tabs[1].rect.x, 220.0);
        assert_eq!(bar.new_tab.y, 570.0);
        assert_eq!(bar.tabs[0].title_y, 576.0);
        assert_eq!(bar.hit(5.0, 580.0), Some(TabBarHit::Tab(0)));
        assert_eq!(bar.hit(5.0, 10.0), None);
    }

    #[test]
    fn side_tabs_are_stacked_one_line_high() {
        for (position, x) in [(TabBarPosition::Left, 0.0), (TabBarPosition::Right, 780.0)] {
            let bar = bar_at(position, 3);
            assert_eq!((bar.tab_width, bar.tab_height), (220.0, 30.0));
            let corners: Vec<(f32, f32)> = bar.tabs.iter().map(|t| (t.rect.x, t.rect.y)).collect();
            assert_eq!(corners, [(x, 0.0), (x, 30.0), (x, 60.0)]);
            // The `+` button follows the last tab, as wide as the bar.
            assert_eq!(
                bar.new_tab,
                Rect {
                    x,
                    y: 90.0,
                    width: 220.0,
                    height: 30.0
                }
            );
            let tab = &bar.tabs[2];
            assert_eq!((tab.title_x, tab.title_y), (x + 10.0, 66.0));
            assert_eq!(tab.close.y, 66.0);
            assert!(tab.close.x + tab.close.width <= x + 220.0);
            assert_eq!(bar.hit(x + 5.0, 45.0), Some(TabBarHit::Tab(1)));
            assert_eq!(bar.hit(x + 5.0, 95.0), Some(TabBarHit::NewTab));
            assert_eq!(bar.hit(x + 5.0, 300.0), None);
        }
    }

    #[test]
    fn side_tabs_shrink_to_fit_the_height() {
        let window = Rect::from_size(1000, 330);
        let bar = TabBarLayout::new(window, TabBarPosition::Left, &titles(20), 0, METRICS);
        assert_eq!(bar.tab_height, 15.0);
        assert!(bar.new_tab.y + bar.new_tab.height <= 330.0);
    }

    #[test]
    fn side_tabs_are_dragged_along_the_height() {
        let mut bar = bar_at(TabBarPosition::Left, 3);
        assert_eq!(bar.along(5.0, 45.0), 45.0);
        assert_eq!(bar.drop_index(45.0), 1);
        let to = bar.dragged(0, 50.0);
        assert_eq!(to, 2);
        let placed: Vec<(&str, f32)> = bar
            .tabs
            .iter()
            .map(|t| (t.title.as_str(), t.rect.y))
            .collect();
        assert_eq!(placed, [("tab 1", 0.0), ("tab 2", 30.0), ("tab 0", 50.0)]);
        assert!(bar.tabs.iter().all(|t| t.rect.x == 0.0));
    }
}
