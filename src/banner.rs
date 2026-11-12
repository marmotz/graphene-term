//! One-line banner drawn at the top of the window, over the grid.
//!
//! The banner content is plain data; [`BannerLayout`] places it in pixels so both the
//! renderer and the mouse handling use the same geometry.

use crate::config::ConfigError;
use crate::layout::CellSize;

/// Content of the banner: a message followed by clickable actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    pub text: String,
    /// Labels of the click targets, drawn after the text, left to right.
    pub actions: Vec<String>,
}

impl Banner {
    /// Banner reporting configuration errors: the first one, plus "(+N more)" when there are
    /// several. `None` when there is no error.
    pub fn config_errors(errors: &[ConfigError]) -> Option<Self> {
        let first = errors.first()?;
        let text = match errors.len() - 1 {
            0 => first.to_string(),
            more => format!("{first} (+{more} more)"),
        };
        Some(Self {
            text,
            actions: Vec::new(),
        })
    }
}

/// Axis-aligned rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl PixelRect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// A piece of banner text and the pixel position of its top-left corner.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedText {
    pub x: f32,
    pub y: f32,
    pub text: String,
}

/// What a click on the banner hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerHit {
    /// The action at this index in [`Banner::actions`].
    Action(usize),
    /// The banner outside any action.
    Body,
}

/// Pixel geometry of a [`Banner`].
#[derive(Debug, Clone, PartialEq)]
pub struct BannerLayout {
    /// Background, spanning the window width.
    pub rect: PixelRect,
    /// Message then action labels.
    pub texts: Vec<PlacedText>,
    /// Click target of each action, in [`Banner::actions`] order.
    pub actions: Vec<PixelRect>,
}

impl BannerLayout {
    /// Places `banner` at the top of a window `width` pixels wide. The text uses the grid
    /// font, so a character is one cell wide.
    pub fn new(banner: &Banner, cell: CellSize, width: f32) -> Self {
        let margin = (cell.height / 4.0).round();
        let height = cell.height + 2.0 * margin;
        let gap = 2.0 * cell.width;
        let text_width = |text: &str| text.chars().count() as f32 * cell.width;

        let mut x = 2.0 * cell.width;
        let mut texts = vec![PlacedText {
            x,
            y: margin,
            text: banner.text.clone(),
        }];
        x += text_width(&banner.text);
        let mut actions = Vec::with_capacity(banner.actions.len());
        for label in &banner.actions {
            x += gap;
            texts.push(PlacedText {
                x,
                y: margin,
                text: label.clone(),
            });
            // The target extends half a gap around the label, over the full banner height.
            actions.push(PixelRect {
                x: x - gap / 2.0,
                y: 0.0,
                width: text_width(label) + gap,
                height,
            });
            x += text_width(label);
        }

        Self {
            rect: PixelRect {
                x: 0.0,
                y: 0.0,
                width,
                height,
            },
            texts,
            actions,
        }
    }

    /// What the pixel (`x`, `y`) hits, `None` outside the banner.
    pub fn hit(&self, x: f32, y: f32) -> Option<BannerHit> {
        if !self.rect.contains(x, y) {
            return None;
        }
        Some(
            self.actions
                .iter()
                .position(|target| target.contains(x, y))
                .map_or(BannerHit::Body, BannerHit::Action),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn error(line: usize, message: &str) -> ConfigError {
        ConfigError {
            file: PathBuf::from("/home/me/.config/graphene/config.toml"),
            line: Some(line),
            message: message.to_owned(),
        }
    }

    const CELL: CellSize = CellSize {
        width: 10.0,
        height: 20.0,
    };

    #[test]
    fn no_error_gives_no_banner() {
        assert_eq!(Banner::config_errors(&[]), None);
    }

    #[test]
    fn one_error_is_shown_as_is() {
        let banner = Banner::config_errors(&[error(12, "unknown key `font.sise`")]).unwrap();
        assert_eq!(banner.text, "config.toml:12: unknown key `font.sise`");
        assert!(banner.actions.is_empty());
    }

    #[test]
    fn several_errors_show_the_first_and_a_count() {
        let banner = Banner::config_errors(&[
            error(2, "unknown key `a`"),
            error(5, "unknown key `b`"),
            error(9, "unknown key `c`"),
        ])
        .unwrap();
        assert_eq!(banner.text, "config.toml:2: unknown key `a` (+2 more)");
    }

    #[test]
    fn layout_spans_the_window_top() {
        let banner = Banner {
            text: "hello".into(),
            actions: Vec::new(),
        };
        let layout = BannerLayout::new(&banner, CELL, 800.0);
        assert_eq!(
            layout.rect,
            PixelRect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 30.0,
            }
        );
        assert_eq!(
            layout.texts,
            vec![PlacedText {
                x: 20.0,
                y: 5.0,
                text: "hello".into(),
            }]
        );
        assert!(layout.actions.is_empty());
    }

    #[test]
    fn actions_follow_the_text() {
        let banner = Banner {
            text: "abc".into(),
            actions: vec!["open".into(), "x".into()],
        };
        let layout = BannerLayout::new(&banner, CELL, 800.0);
        // Text from 20 to 50, then a 20 px gap before each action.
        let xs: Vec<f32> = layout.texts.iter().map(|t| t.x).collect();
        assert_eq!(xs, vec![20.0, 70.0, 130.0]);
        assert_eq!(layout.actions[0].x, 60.0);
        assert_eq!(layout.actions[0].width, 60.0);
        assert_eq!(layout.actions[1].x, 120.0);
        assert_eq!(layout.actions[1].width, 30.0);
    }

    #[test]
    fn hit_testing() {
        let banner = Banner {
            text: "abc".into(),
            actions: vec!["open".into(), "x".into()],
        };
        let layout = BannerLayout::new(&banner, CELL, 800.0);
        assert_eq!(layout.hit(30.0, 10.0), Some(BannerHit::Body));
        assert_eq!(layout.hit(75.0, 10.0), Some(BannerHit::Action(0)));
        assert_eq!(layout.hit(135.0, 29.0), Some(BannerHit::Action(1)));
        assert_eq!(layout.hit(500.0, 0.0), Some(BannerHit::Body));
        assert_eq!(layout.hit(30.0, 30.0), None);
    }
}
