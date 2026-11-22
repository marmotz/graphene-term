//! Counter badge (pure, no GPU access): the pill showing how many notifications of a
//! pane, or of the panes of a tab, are still unseen, laid out at the right end of a
//! title, just before the close button.

use super::tab_bar::UiMetrics;
use crate::layout::Rect;

/// Largest count shown as is: above, the badge reads `99+`.
pub const MAX_SHOWN: u32 = 99;

/// Width of a digit of the interface font, relative to its line height. The font is
/// proportional: an estimate that keeps the pill around its label.
const DIGIT_WIDTH: f32 = 0.5;

/// Space between the pill and the close button after it, and between the title and
/// the pill, relative to the padding of the bar.
const GAP: f32 = 0.5;

/// The text of a badge counting `count`, `None` at 0: nothing to show.
pub fn label(count: u32) -> Option<String> {
    match count {
        0 => None,
        1..=MAX_SHOWN => Some(count.to_string()),
        _ => Some(format!("{MAX_SHOWN}+")),
    }
}

/// A badge laid out in a bar.
#[derive(Debug, Clone, PartialEq)]
pub struct Pill {
    /// Rounded rectangle of the badge, one text line high.
    pub rect: Rect,
    pub label: String,
    /// Left edge of the label, centered in the pill.
    pub label_x: f32,
}

impl Pill {
    /// The badge counting `count`, ending `GAP` before `right` (the left edge of the
    /// close button), its line starting at `line_y`. `None` at 0.
    pub fn new(count: u32, right: f32, line_y: f32, metrics: UiMetrics) -> Option<Self> {
        let label = label(count)?;
        let line = metrics.line_height;
        let padding = metrics.padding();
        let text_width = label.chars().count() as f32 * line * DIGIT_WIDTH;
        let width = (text_width + padding).max(line).round();
        let x = (right - padding * GAP - width).round();
        Some(Self {
            rect: Rect {
                x,
                y: line_y,
                width,
                height: line,
            },
            label_x: (x + (width - text_width) / 2.0).round(),
            label,
        })
    }

    /// Width the pill takes from the title before it, gap included.
    pub fn room(&self, metrics: UiMetrics) -> f32 {
        self.rect.width + metrics.padding() * GAP
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const METRICS: UiMetrics = UiMetrics {
        line_height: 18.0,
        scale: 1.0,
    };

    #[test]
    fn label_counts_up_to_ninety_nine_then_reads_99_plus() {
        assert_eq!(label(0), None);
        assert_eq!(label(1).as_deref(), Some("1"));
        assert_eq!(label(99).as_deref(), Some("99"));
        assert_eq!(label(100).as_deref(), Some("99+"));
        assert_eq!(label(u32::MAX).as_deref(), Some("99+"));
    }

    #[test]
    fn no_pill_at_zero() {
        assert_eq!(Pill::new(0, 200.0, 6.0, METRICS), None);
    }

    #[test]
    fn pill_ends_before_the_close_button_and_holds_its_label() {
        let pill = Pill::new(3, 200.0, 6.0, METRICS).unwrap();
        // A single digit and the padding, one line high, 5 px before the button.
        assert_eq!(
            pill.rect,
            Rect {
                x: 176.0,
                y: 6.0,
                width: 19.0,
                height: 18.0,
            }
        );
        assert_eq!(pill.label, "3");
        assert!(pill.label_x > pill.rect.x);
        assert!(pill.label_x + 9.0 <= pill.rect.x + pill.rect.width);
        assert_eq!(pill.room(METRICS), 24.0);
    }

    #[test]
    fn longer_labels_widen_the_pill() {
        let one = Pill::new(7, 200.0, 0.0, METRICS).unwrap();
        let two = Pill::new(42, 200.0, 0.0, METRICS).unwrap();
        let many = Pill::new(1000, 200.0, 0.0, METRICS).unwrap();
        assert!(two.rect.width > one.rect.width);
        assert!(many.rect.width > two.rect.width);
        assert_eq!(many.label, "99+");
        // All end at the same place.
        for pill in [one, two, many] {
            assert_eq!(pill.rect.x + pill.rect.width, 195.0);
        }
    }

    #[test]
    fn pill_scales_with_the_window() {
        let hidpi = UiMetrics {
            line_height: 36.0,
            scale: 2.0,
        };
        let pill = Pill::new(5, 400.0, 0.0, hidpi).unwrap();
        assert_eq!(pill.rect.height, 36.0);
        assert_eq!(pill.rect.x + pill.rect.width, 390.0);
    }
}
