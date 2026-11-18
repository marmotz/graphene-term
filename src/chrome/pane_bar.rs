//! Pane title bar (pure, no GPU access): the strip at the top of every pane showing its
//! title and a close button, laid out from the pane rectangle the split tree gives.
//!
//! The bar is one interface text line high plus the tab padding, the same vertical
//! rhythm as a horizontal tab. The rest of the pane rectangle, under the bar, is the
//! content the terminal grid is laid out in. Titles are truncated by the interface
//! text.

use super::tab_bar::UiMetrics;
use crate::layout::Rect;
use crate::mouse::Button;
use crate::terminal::PaneId;

/// Height of a pane bar, in pixels: a line of interface text and its padding.
pub fn height(metrics: UiMetrics) -> f32 {
    metrics.bar_height()
}

/// `pane` cut into its title bar, at the top, and the content under it. A pane lower
/// than the bar is all bar.
pub fn split(pane: Rect, metrics: UiMetrics) -> (Rect, Rect) {
    let bar_height = height(metrics).min(pane.height).max(0.0);
    let bar = Rect {
        height: bar_height,
        ..pane
    };
    let content = Rect {
        y: pane.y + bar_height,
        height: pane.height - bar_height,
        ..pane
    };
    (bar, content)
}

/// Distance the pointer travels from a press on a title before the pane is dragged,
/// in logical pixels: a plain click never starts a drag.
pub const PANE_DRAG_THRESHOLD: f32 = 4.0;

/// Whether the pointer at `position` has moved far enough from the `press` on a title
/// to drag the pane, both in physical pixels at `scale` physical pixels per logical one.
pub fn drag_started(press: (f32, f32), position: (f32, f32), scale: f32) -> bool {
    let (dx, dy) = (position.0 - press.0, position.1 - press.1);
    dx.hypot(dy) >= PANE_DRAG_THRESHOLD * scale
}

/// What the mouse is over in a pane bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneBarHit {
    Title,
    Close,
}

/// The title bar of one pane, in pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneBarLayout {
    pub pane: PaneId,
    pub rect: Rect,
    pub title: String,
    /// Left edge of the title, in pixels.
    pub title_x: f32,
    /// Top of the title line, in pixels.
    pub title_y: f32,
    /// Width the title is truncated to.
    pub title_width: f32,
    /// Square close button, at the right end of the bar.
    pub close: Rect,
    /// Whether the pane is the active pane of its tab.
    pub active: bool,
}

impl PaneBarLayout {
    /// The bar of the pane `pane` laid out in `rect`, titled `title`.
    pub fn new(pane: PaneId, rect: Rect, title: String, active: bool, metrics: UiMetrics) -> Self {
        let (bar, _) = split(rect, metrics);
        let padding = metrics.padding();
        let side = metrics.line_height.min(bar.width).min(bar.height).max(0.0);
        let close = Rect {
            x: bar.x + bar.width - padding / 2.0 - side,
            y: bar.y + ((bar.height - side) / 2.0).round(),
            width: side,
            height: side,
        };
        let title_x = bar.x + padding;
        Self {
            pane,
            rect: bar,
            title,
            title_x,
            title_y: bar.y + ((bar.height - metrics.line_height) / 2.0).round(),
            title_width: (close.x - padding / 2.0 - title_x).max(0.0),
            close,
            active,
        }
    }

    /// What is under the pixel `(x, y)`: the close button before the title.
    pub fn hit(&self, x: f32, y: f32) -> Option<PaneBarHit> {
        if !self.rect.contains(x, y) {
            None
        } else if self.close.contains(x, y) {
            Some(PaneBarHit::Close)
        } else {
            Some(PaneBarHit::Title)
        }
    }
}

/// The bar under the pixel `(x, y)` among the panes `panes` (pane and rectangle, as a
/// split layout gives them): its pane and what the pixel is over.
pub fn hit_at(
    panes: &[(PaneId, Rect)],
    x: f32,
    y: f32,
    metrics: UiMetrics,
) -> Option<(PaneId, PaneBarHit)> {
    panes.iter().find_map(|&(pane, rect)| {
        PaneBarLayout::new(pane, rect, String::new(), false, metrics)
            .hit(x, y)
            .map(|hit| (pane, hit))
    })
}

/// What a press on a pane bar does. The bar is Graphene's: a press on it is never
/// reported to the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneBarAction {
    /// Close the pane, with confirmation while a program runs in it.
    Close,
    /// Open the pane menu, without block actions.
    Menu,
    /// Focus the pane, then toggle the zoom of its tab when `zoom`.
    Focus { zoom: bool },
}

/// What a press of `button` on `hit` does, the press being click `clicks` of its series
/// on the bar (see [`crate::selection::ClickTracker`]): a double click on the title
/// toggles the zoom.
pub fn press_action(button: Button, hit: PaneBarHit, clicks: u8) -> Option<PaneBarAction> {
    match (button, hit) {
        (Button::Left, PaneBarHit::Close) => Some(PaneBarAction::Close),
        (Button::Left, PaneBarHit::Title) => Some(PaneBarAction::Focus { zoom: clicks == 2 }),
        (Button::Right, _) => Some(PaneBarAction::Menu),
        (Button::Middle, _) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_starts_past_the_threshold_only() {
        assert!(!drag_started((10.0, 10.0), (10.0, 10.0), 1.0));
        assert!(!drag_started((10.0, 10.0), (12.0, 13.0), 1.0));
        assert!(drag_started((10.0, 10.0), (14.0, 10.0), 1.0));
        assert!(drag_started((10.0, 10.0), (10.0, 6.0), 1.0));
        // At twice the scale, the threshold is twice as many physical pixels.
        assert!(!drag_started((10.0, 10.0), (17.0, 10.0), 2.0));
        assert!(drag_started((10.0, 10.0), (18.0, 10.0), 2.0));
    }

    const METRICS: UiMetrics = UiMetrics {
        line_height: 18.0,
        scale: 1.0,
    };

    fn pane() -> Rect {
        Rect {
            x: 100.0,
            y: 50.0,
            width: 400.0,
            height: 300.0,
        }
    }

    #[test]
    fn bar_is_as_high_as_a_horizontal_tab() {
        assert_eq!(height(METRICS), METRICS.bar_height());
        assert_eq!(height(METRICS), 30.0);
        let hidpi = UiMetrics {
            line_height: 36.0,
            scale: 2.0,
        };
        assert_eq!(height(hidpi), 60.0);
    }

    #[test]
    fn split_puts_the_bar_on_top_and_the_content_under_it() {
        let (bar, content) = split(pane(), METRICS);
        assert_eq!(
            bar,
            Rect {
                x: 100.0,
                y: 50.0,
                width: 400.0,
                height: 30.0
            }
        );
        assert_eq!(
            content,
            Rect {
                x: 100.0,
                y: 80.0,
                width: 400.0,
                height: 270.0
            }
        );
    }

    #[test]
    fn a_pane_lower_than_the_bar_is_all_bar() {
        let low = Rect {
            height: 10.0,
            ..pane()
        };
        let (bar, content) = split(low, METRICS);
        assert_eq!(bar, low);
        assert_eq!(content.height, 0.0);
        assert_eq!(content.y, low.y + low.height);
    }

    #[test]
    fn title_and_close_button_sit_in_the_bar() {
        let bar = PaneBarLayout::new(PaneId(3), pane(), "vim".to_owned(), true, METRICS);
        assert_eq!(bar.pane, PaneId(3));
        assert!(bar.active);
        assert_eq!(bar.rect.height, 30.0);
        // Square button, 18 px, half a padding from the right end, centered vertically.
        assert_eq!(
            bar.close,
            Rect {
                x: 100.0 + 400.0 - 5.0 - 18.0,
                y: 56.0,
                width: 18.0,
                height: 18.0
            }
        );
        assert_eq!(bar.title_x, 110.0);
        assert_eq!(bar.title_y, 56.0);
        // The title stops half a padding before the button.
        assert_eq!(bar.title_width, bar.close.x - 5.0 - 110.0);
    }

    #[test]
    fn narrow_pane_truncates_the_title_to_nothing() {
        let narrow = Rect {
            width: 20.0,
            ..pane()
        };
        let bar = PaneBarLayout::new(PaneId(1), narrow, "title".to_owned(), false, METRICS);
        assert_eq!(bar.title_width, 0.0);
        assert!(bar.close.width <= narrow.width);
    }

    #[test]
    fn hit_finds_the_close_button_before_the_title() {
        let bar = PaneBarLayout::new(PaneId(1), pane(), "t".to_owned(), false, METRICS);
        let (cx, cy) = (bar.close.x + 1.0, bar.close.y + 1.0);
        assert_eq!(bar.hit(cx, cy), Some(PaneBarHit::Close));
        assert_eq!(bar.hit(bar.title_x, cy), Some(PaneBarHit::Title));
        // The padding on both sides of the button is still the title.
        assert_eq!(
            bar.hit(bar.rect.x + 1.0, bar.rect.y + 1.0),
            Some(PaneBarHit::Title)
        );
        assert_eq!(
            bar.hit(bar.rect.x + bar.rect.width - 1.0, bar.rect.y + 1.0),
            Some(PaneBarHit::Title)
        );
        // Under the bar is the content, outside of it nothing.
        assert_eq!(bar.hit(cx, bar.rect.y + bar.rect.height + 1.0), None);
        assert_eq!(bar.hit(bar.rect.x - 1.0, cy), None);
    }

    #[test]
    fn hit_at_finds_the_bar_of_the_pane_under_the_pixel() {
        let right = Rect { x: 500.0, ..pane() };
        let panes = [(PaneId(1), pane()), (PaneId(2), right)];
        assert_eq!(
            hit_at(&panes, 120.0, 60.0, METRICS),
            Some((PaneId(1), PaneBarHit::Title))
        );
        let close = PaneBarLayout::new(PaneId(2), right, String::new(), false, METRICS).close;
        assert_eq!(
            hit_at(&panes, close.x + 1.0, close.y + 1.0, METRICS),
            Some((PaneId(2), PaneBarHit::Close))
        );
        // The content under the bars and the space outside the panes have no bar.
        assert_eq!(hit_at(&panes, 120.0, 200.0, METRICS), None);
        assert_eq!(hit_at(&panes, 50.0, 60.0, METRICS), None);
        assert_eq!(hit_at(&[], 120.0, 60.0, METRICS), None);
    }

    #[test]
    fn left_press_closes_on_the_button_and_focuses_on_the_title() {
        assert_eq!(
            press_action(Button::Left, PaneBarHit::Close, 1),
            Some(PaneBarAction::Close)
        );
        assert_eq!(
            press_action(Button::Left, PaneBarHit::Title, 1),
            Some(PaneBarAction::Focus { zoom: false })
        );
    }

    #[test]
    fn double_click_on_the_title_toggles_the_zoom_only_on_the_second_click() {
        assert_eq!(
            press_action(Button::Left, PaneBarHit::Title, 2),
            Some(PaneBarAction::Focus { zoom: true })
        );
        assert_eq!(
            press_action(Button::Left, PaneBarHit::Title, 3),
            Some(PaneBarAction::Focus { zoom: false })
        );
    }

    #[test]
    fn right_press_opens_the_menu_and_middle_press_does_nothing() {
        for hit in [PaneBarHit::Title, PaneBarHit::Close] {
            assert_eq!(
                press_action(Button::Right, hit, 1),
                Some(PaneBarAction::Menu)
            );
            assert_eq!(press_action(Button::Middle, hit, 1), None);
        }
    }
}
