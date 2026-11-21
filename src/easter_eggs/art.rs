//! ASCII drawing of graphene (pure, no GPU access): its text, and the box centered in
//! the window it is shown in, opened from the `graphene` secret of the command palette.

use crate::chrome::dialog::{MARGIN, PADDING};
use crate::chrome::tab_bar::UiMetrics;
use crate::layout::Rect;

/// A honeycomb lattice and its name, in pure ASCII, every line of the same length.
pub const GRAPHENE_ART: &[&str] = &[
    r"   __    __    __   ",
    r"  /  \__/  \__/  \  ",
    r"  \__/  \__/  \__/  ",
    r"  /  \__/  \__/  \  ",
    r"  \__/  \__/  \__/  ",
    r"     \__/  \__/     ",
    r"                    ",
    r"      graphene      ",
];

/// A line of the drawing, placed.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtLine {
    pub text: &'static str,
    /// Top of the line.
    pub y: f32,
}

/// The drawing's box placed in a window, in physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtLayout {
    pub rect: Rect,
    /// Left edge of every line.
    pub text_x: f32,
    /// Width the lines are truncated to.
    pub text_width: f32,
    /// The lines that fit the box, from the top.
    pub lines: Vec<ArtLine>,
}

impl ArtLayout {
    /// Lays the drawing out in the center of `window`, with the dialog's padding, its
    /// lines measured by `measure` in pixels; the box never exceeds the window minus
    /// the dialog's margins.
    pub fn new(window: Rect, metrics: UiMetrics, mut measure: impl FnMut(&str) -> f32) -> Self {
        let scale = metrics.scale;
        let line = metrics.line_height;
        let padding = PADDING * scale;
        let content = GRAPHENE_ART
            .iter()
            .map(|text| measure(text))
            .fold(0.0, f32::max);
        let widest = (window.width - 2.0 * MARGIN * scale).max(0.0);
        let highest = (window.height - 2.0 * MARGIN * scale).max(0.0);
        let width = (content + 2.0 * padding).min(widest).round();
        let height = (GRAPHENE_ART.len() as f32 * line + 2.0 * padding)
            .min(highest)
            .round();
        let rect = Rect {
            x: (window.x + (window.width - width) / 2.0).round(),
            y: (window.y + (window.height - height) / 2.0).round(),
            width,
            height,
        };
        let lines = GRAPHENE_ART
            .iter()
            .enumerate()
            .map(|(index, &text)| ArtLine {
                text,
                y: (rect.y + padding + index as f32 * line).round(),
            })
            .filter(|art| art.y + line <= rect.y + rect.height)
            .collect();
        Self {
            rect,
            text_x: rect.x + padding,
            text_width: (rect.width - 2.0 * padding).max(0.0),
            lines,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character 10 px wide.
    fn width(text: &str) -> f32 {
        text.chars().count() as f32 * 10.0
    }

    const METRICS: UiMetrics = UiMetrics {
        line_height: 20.0,
        scale: 1.0,
    };

    fn inside(inner: Rect, outer: Rect) -> bool {
        inner.x >= outer.x
            && inner.y >= outer.y
            && inner.x + inner.width <= outer.x + outer.width
            && inner.y + inner.height <= outer.y + outer.height
    }

    #[test]
    fn art_lines_are_ascii_and_of_the_same_length() {
        let length = GRAPHENE_ART[0].len();
        assert!(GRAPHENE_ART.len() >= 6);
        for line in GRAPHENE_ART {
            assert!(line.is_ascii(), "{line:?}");
            assert_eq!(line.len(), length, "{line:?}");
        }
        assert!(GRAPHENE_ART.iter().any(|line| line.contains(r"\__/")));
        assert!(GRAPHENE_ART.iter().any(|line| line.trim() == "graphene"));
    }

    #[test]
    fn layout_is_centered_and_holds_every_line() {
        let window = Rect::from_size(1000, 800);
        let layout = ArtLayout::new(window, METRICS, width);
        let rect = layout.rect;
        assert!(inside(rect, window));
        assert_eq!(
            rect.x - window.x,
            window.x + window.width - rect.x - rect.width
        );
        assert_eq!(
            rect.y - window.y,
            window.y + window.height - rect.y - rect.height
        );
        assert_eq!(layout.lines.len(), GRAPHENE_ART.len());
        assert!(layout.text_width >= width(GRAPHENE_ART[0]));
        assert!(
            layout
                .lines
                .windows(2)
                .all(|pair| pair[1].y - pair[0].y == 20.0)
        );
        let last = layout.lines.last().unwrap();
        assert!(last.y + METRICS.line_height <= rect.y + rect.height);
    }

    #[test]
    fn layout_stays_in_a_small_window() {
        let window = Rect::from_size(120, 100);
        let layout = ArtLayout::new(window, METRICS, width);
        assert!(inside(layout.rect, window));
        assert!(layout.lines.len() < GRAPHENE_ART.len());
        for line in &layout.lines {
            assert!(line.y + METRICS.line_height <= layout.rect.y + layout.rect.height);
        }
    }
}
