//! Pane title bar (pure, no GPU access): the strip at the top of every pane showing its
//! title and a close button, laid out from the pane rectangle the split tree gives.
//!
//! The bar is one interface text line high plus the tab padding, the same vertical
//! rhythm as a horizontal tab. The rest of the pane rectangle, under the bar, is the
//! content the terminal grid is laid out in. Titles are truncated by the interface
//! text.

use super::tab_bar::UiMetrics;
use crate::layout::Rect;
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

/// What the mouse is over in a pane bar.
#[allow(dead_code)] // Mouse routing on the bar is tracked by issue #136.
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
    #[allow(dead_code)] // Mouse routing on the bar is tracked by issue #136.
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
