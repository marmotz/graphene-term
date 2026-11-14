//! Tooltip showing the target of the hovered OSC 8 hyperlink: one line of interface
//! text in a box near the mouse, kept inside the window, the URI truncated to the
//! window width.

use super::tab_bar::UiMetrics;
use crate::layout::Rect;

/// Horizontal space between the text and the box edges, in logical pixels.
const PADDING: f32 = 6.0;
/// Vertical space between the text and the box edges, in logical pixels.
const VERTICAL_PADDING: f32 = 3.0;
/// Distance from the mouse to the box, below the pointer (or above it when the box does
/// not fit below), in logical pixels: clears the pointer's arrow.
const POINTER_GAP: f32 = 18.0;
/// Space kept between the box and the window edges, in logical pixels.
const MARGIN: f32 = 4.0;

/// The tooltip laid out in the window.
#[derive(Debug, Clone, PartialEq)]
pub struct TooltipLayout {
    pub rect: Rect,
    /// The URI, truncated to `text_width` when drawn.
    pub text: String,
    /// Top-left corner of the text.
    pub text_x: f32,
    pub text_y: f32,
    /// Width the text is truncated to.
    pub text_width: f32,
}

impl TooltipLayout {
    /// Lays the tooltip of `uri` out near the `mouse` position (in physical pixels):
    /// below and to the right of the pointer, moved left to stay inside the `window`,
    /// and above the pointer when it does not fit below. The box is as wide as the text
    /// measured by `measure`, at most the window width minus the margins (the text is
    /// then truncated). `None` when the window has no room for it.
    pub fn new(
        uri: &str,
        mouse: (f32, f32),
        window: Rect,
        metrics: UiMetrics,
        measure: impl FnOnce(&str) -> f32,
    ) -> Option<Self> {
        let scale = metrics.scale;
        let (padding, vertical) = (PADDING * scale, VERTICAL_PADDING * scale);
        let (gap, margin) = (POINTER_GAP * scale, MARGIN * scale);
        let widest = window.width - 2.0 * margin;
        let height = (metrics.line_height + 2.0 * vertical).round();
        if uri.is_empty() || widest <= 2.0 * padding || height > window.height - 2.0 * margin {
            return None;
        }
        let width = (measure(uri) + 2.0 * padding).min(widest).round();

        let (left, right) = (window.x + margin, window.x + window.width - margin);
        let x = mouse.0.min(right - width).max(left).round();
        let (top, bottom) = (window.y + margin, window.y + window.height - margin);
        let below = mouse.1 + gap;
        let y = if below + height <= bottom {
            below.max(top)
        } else {
            // Above the pointer, as close as the window allows.
            (mouse.1 - gap - height).min(bottom - height).max(top)
        }
        .round();

        Some(Self {
            rect: Rect {
                x,
                y,
                width,
                height,
            },
            text: uri.to_owned(),
            text_x: x + padding,
            text_y: y + vertical,
            text_width: (width - 2.0 * padding).max(0.0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    };

    /// Scale 1, lines 20 px high: the box is 26 px high.
    const METRICS: UiMetrics = UiMetrics {
        line_height: 20.0,
        scale: 1.0,
    };

    /// Every character 10 px wide.
    fn width(text: &str) -> f32 {
        text.chars().count() as f32 * 10.0
    }

    fn layout(uri: &str, mouse: (f32, f32)) -> TooltipLayout {
        TooltipLayout::new(uri, mouse, WINDOW, METRICS, width).unwrap()
    }

    #[test]
    fn box_is_below_and_right_of_the_pointer_and_fits_the_text() {
        let tooltip = layout("https://a.b", (100.0, 50.0));
        // 11 characters, plus the 6 px padding each side.
        assert_eq!(
            tooltip.rect,
            Rect {
                x: 100.0,
                y: 68.0,
                width: 122.0,
                height: 26.0,
            }
        );
        assert_eq!((tooltip.text_x, tooltip.text_y), (106.0, 71.0));
        assert_eq!(tooltip.text_width, 110.0);
        assert_eq!(tooltip.text, "https://a.b");
    }

    #[test]
    fn box_moves_left_to_stay_inside_the_window() {
        let tooltip = layout("https://a.b", (790.0, 50.0));
        assert_eq!(tooltip.rect.x, 800.0 - 4.0 - 122.0);
        assert_eq!(tooltip.text_x, tooltip.rect.x + 6.0);
    }

    #[test]
    fn box_goes_above_the_pointer_at_the_bottom_of_the_window() {
        let tooltip = layout("https://a.b", (100.0, 580.0));
        assert_eq!(tooltip.rect.y, 580.0 - 18.0 - 26.0);
        assert!(tooltip.rect.y + tooltip.rect.height <= 600.0);
    }

    #[test]
    fn box_stays_inside_a_window_too_short_for_above_or_below() {
        let short = Rect {
            height: 40.0,
            ..WINDOW
        };
        let tooltip = TooltipLayout::new("x", (10.0, 20.0), short, METRICS, width).unwrap();
        assert_eq!(tooltip.rect.y, 4.0);
        assert!(tooltip.rect.y + tooltip.rect.height <= 40.0 - 4.0);
    }

    #[test]
    fn long_uri_is_truncated_to_the_window_width() {
        let uri = format!("https://example.com/{}", "a".repeat(200));
        let tooltip = layout(&uri, (300.0, 50.0));
        assert_eq!(tooltip.rect.x, 4.0);
        assert_eq!(tooltip.rect.width, 800.0 - 8.0);
        assert_eq!(tooltip.text_width, 800.0 - 8.0 - 12.0);
        // The whole URI is kept: the text is cut with an ellipsis when drawn.
        assert_eq!(tooltip.text, uri);
    }

    #[test]
    fn layout_follows_the_scale_factor() {
        let metrics = UiMetrics {
            line_height: 40.0,
            scale: 2.0,
        };
        let tooltip = TooltipLayout::new("ab", (100.0, 50.0), WINDOW, metrics, width).unwrap();
        assert_eq!(tooltip.rect.y, 50.0 + 36.0);
        assert_eq!(tooltip.rect.height, 40.0 + 12.0);
        assert_eq!(tooltip.rect.width, 20.0 + 24.0);
    }

    #[test]
    fn no_tooltip_without_room_or_text() {
        let tiny = Rect {
            width: 10.0,
            ..WINDOW
        };
        assert!(TooltipLayout::new("x", (0.0, 0.0), tiny, METRICS, width).is_none());
        let flat = Rect {
            height: 20.0,
            ..WINDOW
        };
        assert!(TooltipLayout::new("x", (0.0, 0.0), flat, METRICS, width).is_none());
        assert!(TooltipLayout::new("", (0.0, 0.0), WINDOW, METRICS, width).is_none());
    }

    #[test]
    fn box_inside_a_window_offset_from_the_origin() {
        let window = Rect {
            x: 100.0,
            y: 50.0,
            width: 300.0,
            height: 200.0,
        };
        let tooltip = TooltipLayout::new("ab", (0.0, 0.0), window, METRICS, width).unwrap();
        assert_eq!((tooltip.rect.x, tooltip.rect.y), (104.0, 54.0));
    }
}
