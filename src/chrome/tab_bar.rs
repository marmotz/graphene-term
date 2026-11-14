//! Tab bar (pure, no GPU access): where each tab, its close button and the new tab
//! button sit, which one the mouse is over, and where a dragged tab lands.
//!
//! The bar is only shown while the window has more than one tab. Tabs share its width
//! equally, up to a maximum width; their titles are truncated by the interface text.

use crate::layout::Rect;

/// Widest a tab gets, in logical pixels.
const TAB_MAX_WIDTH: f32 = 220.0;
/// Space above and below the tab titles, in logical pixels.
const VERTICAL_PADDING: f32 = 6.0;
/// Space before the title and around the close button, in logical pixels.
const HORIZONTAL_PADDING: f32 = 10.0;

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
    /// Width the title is truncated to.
    pub title_width: f32,
    /// Square close button, at the right end of the tab.
    pub close: Rect,
    pub active: bool,
}

/// The bar at the top of a window of `width` pixels, in pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct TabBarLayout {
    pub rect: Rect,
    /// Tabs in order; while one is dragged, see [`TabBarLayout::dragged`].
    pub tabs: Vec<TabSlot>,
    /// Square `+` button after the last tab.
    pub new_tab: Rect,
    /// Width of every tab.
    pub tab_width: f32,
    /// Top of the title line.
    pub text_y: f32,
    metrics: UiMetrics,
}

impl TabBarLayout {
    /// Lays out the tabs titled `titles`, tab `active` shown: equal widths up to the
    /// maximum, shrunk to fit the window, then the `+` button.
    pub fn new(width: f32, titles: &[String], active: usize, metrics: UiMetrics) -> Self {
        let height = metrics.bar_height();
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        let button = height;
        let count = titles.len().max(1) as f32;
        let tab_width = ((width - button) / count)
            .min(TAB_MAX_WIDTH * metrics.scale)
            .max(0.0)
            .floor();
        let mut layout = Self {
            rect,
            tabs: Vec::with_capacity(titles.len()),
            new_tab: Rect {
                x: tab_width * titles.len() as f32,
                y: 0.0,
                width: button,
                height,
            },
            tab_width,
            text_y: ((height - metrics.line_height) / 2.0).round(),
            metrics,
        };
        for (index, title) in titles.iter().enumerate() {
            let slot = layout.slot(layout.slot_x(index), title.clone(), index == active);
            layout.tabs.push(slot);
        }
        layout
    }

    fn slot_x(&self, index: usize) -> f32 {
        self.rect.x + index as f32 * self.tab_width
    }

    /// The tab of `title` with its left edge at `x`.
    fn slot(&self, x: f32, title: String, active: bool) -> TabSlot {
        let height = self.rect.height;
        let padding = self.metrics.padding();
        let side = self.metrics.line_height.min(self.tab_width);
        let close = Rect {
            x: x + self.tab_width - padding / 2.0 - side,
            y: ((height - side) / 2.0).round(),
            width: side,
            height: side,
        };
        let title_x = x + padding;
        TabSlot {
            rect: Rect {
                x,
                y: self.rect.y,
                width: self.tab_width,
                height,
            },
            title,
            title_x,
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

    /// Slot a dragged tab whose center is at `x` lands in.
    pub fn drop_index(&self, x: f32) -> usize {
        let last = self.tabs.len().saturating_sub(1);
        if self.tab_width <= 0.0 {
            return 0;
        }
        (((x - self.rect.x) / self.tab_width).floor().max(0.0) as usize).min(last)
    }

    /// Left edge a dragged tab follows the mouse with, kept inside the tabs.
    fn drag_left(&self, left: f32) -> f32 {
        let max = self.slot_x(self.tabs.len().saturating_sub(1));
        left.clamp(self.rect.x, max)
    }

    /// Tab `from` dragged with its left edge at `left`: it follows the mouse while the
    /// others move to the slots they take once it is dropped. Returns the slot it lands
    /// in; the dragged tab is last in `tabs`, so it is drawn over the others.
    pub fn dragged(&mut self, from: usize, left: f32) -> usize {
        if from >= self.tabs.len() {
            return from;
        }
        let left = self.drag_left(left);
        let to = self.drop_index(left + self.tab_width / 2.0);
        let mut order: Vec<TabSlot> = self.tabs.clone();
        let moving = order.remove(from);
        order.insert(to, moving);
        let mut tabs = Vec::with_capacity(order.len());
        let mut dragged = None;
        for (index, tab) in order.into_iter().enumerate() {
            if index == to {
                dragged = Some(self.slot(left, tab.title, tab.active));
            } else {
                tabs.push(self.slot(self.slot_x(index), tab.title, tab.active));
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
        let wide = TabBarLayout::new(2000.0, &titles(3), 0, METRICS);
        assert_eq!(wide.tab_width, TAB_MAX_WIDTH);
        let xs: Vec<f32> = wide.tabs.iter().map(|t| t.rect.x).collect();
        assert_eq!(xs, [0.0, 220.0, 440.0]);
        // The `+` button follows the last tab.
        assert_eq!(wide.new_tab.x, 660.0);
        assert_eq!(wide.new_tab.width, wide.rect.height);

        // Shrunk to fit: 4 tabs and the button in 430 px.
        let narrow = TabBarLayout::new(430.0, &titles(4), 0, METRICS);
        assert_eq!(narrow.tab_width, 100.0);
        assert!(narrow.new_tab.x + narrow.new_tab.width <= 430.0);
    }

    #[test]
    fn titles_get_the_room_left_by_the_padding_and_the_close_button() {
        let bar = TabBarLayout::new(2000.0, &titles(2), 1, METRICS);
        let tab = &bar.tabs[1];
        assert!(tab.active && !bar.tabs[0].active);
        assert_eq!(tab.title_x, tab.rect.x + 10.0);
        assert!(tab.title_x + tab.title_width <= tab.close.x);
        assert!(tab.close.x + tab.close.width <= tab.rect.x + tab.rect.width);
        assert_eq!(tab.title, "tab 1");
        // Shrunk tabs leave less room to their title, never a negative one.
        let narrow = TabBarLayout::new(130.0, &titles(4), 0, METRICS);
        assert!(narrow.tabs[0].title_width < tab.title_width);
        assert!(narrow.tabs.iter().all(|t| t.title_width >= 0.0));
    }

    #[test]
    fn hit_finds_the_tab_its_close_button_and_the_new_tab_button() {
        let bar = TabBarLayout::new(2000.0, &titles(2), 0, METRICS);
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
        let bar = TabBarLayout::new(2000.0, &titles(3), 0, METRICS);
        assert_eq!(bar.drop_index(-50.0), 0);
        assert_eq!(bar.drop_index(219.0), 0);
        assert_eq!(bar.drop_index(220.0), 1);
        assert_eq!(bar.drop_index(500.0), 2);
        assert_eq!(bar.drop_index(5000.0), 2);
    }

    #[test]
    fn dragged_tab_follows_the_mouse_and_the_others_shift() {
        let mut bar = TabBarLayout::new(2000.0, &titles(3), 0, METRICS);
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
        let mut bar = TabBarLayout::new(2000.0, &titles(2), 0, METRICS);
        assert_eq!(bar.dragged(1, -100.0), 0);
        assert_eq!(bar.tabs.last().unwrap().rect.x, 0.0);
        let mut bar = TabBarLayout::new(2000.0, &titles(2), 0, METRICS);
        assert_eq!(bar.dragged(0, 1500.0), 1);
        assert_eq!(bar.tabs.last().unwrap().rect.x, 220.0);
    }
}
