//! Conversion of a [`Frame`] into GPU-ready primitives (pure, no GPU access).

use std::hash::{Hash, Hasher};

use alacritty_terminal::vte::ansi::{CursorShape, Rgb};

use super::rects::RectInstance;
use crate::frame::Frame;
use crate::layout::GridLayout;

/// A run of consecutive characters sharing the same style, anchored at a grid column.
///
/// Non-ASCII characters always get their own run: they may come from a fallback
/// font whose advance differs from the cell width, and isolating them keeps the
/// following text aligned on the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRun {
    pub column: usize,
    pub text: String,
    pub color: Rgb,
    pub bold: bool,
    pub italic: bool,
}

impl Hash for TextRun {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.column.hash(state);
        self.text.hash(state);
        (
            self.color.r,
            self.color.g,
            self.color.b,
            self.bold,
            self.italic,
        )
            .hash(state);
    }
}

/// Text drawn above everything else, at a pixel position (banner).
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayText {
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub color: Rgb,
}

#[derive(Debug, Clone)]
pub struct Scene {
    /// Clear color (linear or sRGB depending on the target).
    pub clear: [f32; 4],
    /// Rectangles drawn below the text.
    pub under: Vec<RectInstance>,
    /// Rectangles drawn above the text.
    pub over: Vec<RectInstance>,
    /// Styled text runs per visible row.
    pub rows: Vec<Vec<TextRun>>,
    /// Text drawn after the `over` rectangles.
    pub overlay: Vec<OverlayText>,
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Converts a color for a render target, linearizing it when the target is sRGB.
pub fn gpu_color(rgb: Rgb, srgb_target: bool) -> [f32; 4] {
    if srgb_target {
        [
            srgb_to_linear(rgb.r),
            srgb_to_linear(rgb.g),
            srgb_to_linear(rgb.b),
            1.0,
        ]
    } else {
        [
            rgb.r as f32 / 255.0,
            rgb.g as f32 / 255.0,
            rgb.b as f32 / 255.0,
            1.0,
        ]
    }
}

pub fn build(frame: &Frame, layout: &GridLayout, srgb_target: bool) -> Scene {
    let cell = layout.cell;
    let thickness = (cell.height / 16.0).round().max(1.0);
    let mut under = Vec::new();
    let mut over = Vec::new();
    let mut rows = Vec::with_capacity(frame.rows.len());

    for (line, cells) in frame.rows.iter().enumerate() {
        // Backgrounds: merge consecutive cells with the same non-default color.
        let mut column = 0;
        while column < cells.len() {
            let bg = cells[column].bg;
            let start = column;
            while column < cells.len() && cells[column].bg == bg {
                column += 1;
            }
            if bg != frame.background {
                let (x, y) = layout.cell_origin(start, line);
                under.push(RectInstance {
                    position: [x, y],
                    size: [(column - start) as f32 * cell.width, cell.height],
                    color: gpu_color(bg, srgb_target),
                });
            }
        }

        // Decorations.
        for (column, c) in cells.iter().enumerate() {
            let (x, y) = layout.cell_origin(column, line);
            let color = gpu_color(c.fg, srgb_target);
            if c.underline {
                over.push(RectInstance {
                    position: [x, y + cell.height - 2.0 * thickness],
                    size: [cell.width, thickness],
                    color,
                });
            }
            if c.strikeout {
                over.push(RectInstance {
                    position: [x, y + (cell.height - thickness) / 2.0],
                    size: [cell.width, thickness],
                    color,
                });
            }
        }

        // Text runs.
        let mut runs: Vec<TextRun> = Vec::new();
        let mut previous_ascii = false;
        for (column, c) in cells.iter().enumerate().filter(|(_, c)| !c.spacer) {
            let ascii = c.c.is_ascii();
            match runs.last_mut() {
                Some(run)
                    if ascii
                        && previous_ascii
                        && run.color == c.fg
                        && run.bold == c.bold
                        && run.italic == c.italic =>
                {
                    run.text.push(c.c);
                }
                _ => runs.push(TextRun {
                    column,
                    text: c.c.to_string(),
                    color: c.fg,
                    bold: c.bold,
                    italic: c.italic,
                }),
            }
            previous_ascii = ascii;
        }
        // Blank runs draw nothing.
        runs.retain(|run| !run.text.chars().all(|c| c == ' '));
        rows.push(runs);
    }

    if let Some(cursor) = frame.cursor {
        let (x, y) = layout.cell_origin(cursor.column, cursor.line);
        let color = gpu_color(cursor.color, srgb_target);
        let beam = (cell.width / 8.0).round().max(2.0);
        let rect = |position: [f32; 2], size: [f32; 2]| RectInstance {
            position,
            size,
            color,
        };
        match cursor.shape {
            CursorShape::Beam => over.push(rect([x, y], [beam, cell.height])),
            CursorShape::Underline => {
                over.push(rect([x, y + cell.height - beam], [cell.width, beam]))
            }
            CursorShape::HollowBlock | CursorShape::Block => {
                over.push(rect([x, y], [cell.width, thickness]));
                over.push(rect(
                    [x, y + cell.height - thickness],
                    [cell.width, thickness],
                ));
                over.push(rect([x, y], [thickness, cell.height]));
                over.push(rect(
                    [x + cell.width - thickness, y],
                    [thickness, cell.height],
                ));
            }
            CursorShape::Hidden => {}
        }
    }

    // The banner covers the grid: its rect comes last and its text goes in the overlay.
    let mut overlay = Vec::new();
    if let Some(banner) = &frame.banner {
        let rect = banner.layout.rect;
        over.push(RectInstance {
            position: [rect.x, rect.y],
            size: [rect.width, rect.height],
            color: gpu_color(banner.background, srgb_target),
        });
        overlay.extend(banner.layout.texts.iter().map(|placed| OverlayText {
            x: placed.x,
            y: placed.y,
            text: placed.text.clone(),
            color: banner.text,
        }));
    }

    Scene {
        clear: gpu_color(frame.background, srgb_target),
        under,
        over,
        rows,
        overlay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::banner::{Banner, BannerLayout};
    use crate::frame::{FrameBanner, FrameCell, FrameCursor};
    use crate::layout::CellSize;

    const BG: Rgb = Rgb { r: 0, g: 0, b: 0 };
    const FG: Rgb = Rgb {
        r: 255,
        g: 255,
        b: 255,
    };
    const RED: Rgb = Rgb { r: 255, g: 0, b: 0 };

    fn cell(c: char) -> FrameCell {
        FrameCell {
            c,
            fg: FG,
            bg: BG,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            spacer: false,
        }
    }

    fn layout() -> GridLayout {
        GridLayout::new(
            100,
            100,
            CellSize {
                width: 10.0,
                height: 20.0,
            },
            0.0,
        )
    }

    fn frame(rows: Vec<Vec<FrameCell>>) -> Frame {
        Frame {
            rows,
            cursor: None,
            background: BG,
            banner: None,
        }
    }

    #[test]
    fn default_background_produces_no_rect() {
        let scene = build(&frame(vec![vec![cell('a'), cell('b')]]), &layout(), false);
        assert!(scene.under.is_empty());
        assert_eq!(scene.clear, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn adjacent_backgrounds_are_merged() {
        let mut row = vec![cell('a'), cell('b'), cell('c')];
        row[1].bg = RED;
        row[2].bg = RED;
        let scene = build(&frame(vec![row]), &layout(), false);
        assert_eq!(scene.under.len(), 1);
        assert_eq!(scene.under[0].position, [10.0, 0.0]);
        assert_eq!(scene.under[0].size, [20.0, 20.0]);
    }

    fn runs(scene: &Scene) -> Vec<(usize, &str)> {
        scene.rows[0]
            .iter()
            .map(|r| (r.column, r.text.as_str()))
            .collect()
    }

    #[test]
    fn runs_group_by_style_and_isolate_non_ascii() {
        let mut row = vec![
            cell('a'),
            cell('b'),
            cell('日'),
            cell(' '),
            cell('c'),
            cell('d'),
            cell('e'),
        ];
        row[3].spacer = true;
        row[5].fg = RED;
        row[6].fg = RED;
        let scene = build(&frame(vec![row]), &layout(), false);
        assert_eq!(
            runs(&scene),
            vec![(0, "ab"), (2, "日"), (4, "c"), (5, "de")]
        );
    }

    #[test]
    fn blank_runs_are_dropped() {
        let mut row = vec![cell('a'), cell(' '), cell(' '), cell('b')];
        row[1].fg = RED;
        row[2].fg = RED;
        let scene = build(&frame(vec![row]), &layout(), false);
        assert_eq!(runs(&scene), vec![(0, "a"), (3, "b")]);
    }

    #[test]
    fn underline_and_beam_cursor_go_over_text() {
        let mut row = vec![cell('a'), cell('b')];
        row[0].underline = true;
        let mut f = frame(vec![row]);
        f.cursor = Some(FrameCursor {
            column: 1,
            line: 0,
            shape: CursorShape::Beam,
            color: RED,
        });
        let scene = build(&f, &layout(), false);
        assert_eq!(scene.over.len(), 2);
        assert_eq!(scene.over[1].position, [10.0, 0.0]);
    }

    #[test]
    fn no_banner_draws_no_overlay() {
        let scene = build(&frame(vec![vec![cell('a')]]), &layout(), false);
        assert!(scene.over.is_empty());
        assert!(scene.overlay.is_empty());
    }

    #[test]
    fn banner_is_drawn_last_over_the_cursor() {
        let banner = Banner {
            text: "oops".into(),
            actions: vec!["x".into()],
        };
        let mut f = frame(vec![vec![cell('a')]]);
        f.cursor = Some(FrameCursor {
            column: 0,
            line: 0,
            shape: CursorShape::Beam,
            color: RED,
        });
        f.banner = Some(FrameBanner {
            layout: BannerLayout::new(&banner, layout().cell, 100.0),
            background: RED,
            text: FG,
        });
        let scene = build(&f, &layout(), false);
        assert_eq!(scene.over.len(), 2);
        let last = scene.over[1];
        assert_eq!(last.position, [0.0, 0.0]);
        assert_eq!(last.size, [100.0, 30.0]);
        assert_eq!(last.color, [1.0, 0.0, 0.0, 1.0]);
        let texts: Vec<(&str, Rgb)> = scene
            .overlay
            .iter()
            .map(|t| (t.text.as_str(), t.color))
            .collect();
        assert_eq!(texts, vec![("oops", FG), ("x", FG)]);
    }

    #[test]
    fn srgb_target_linearizes() {
        let c = gpu_color(
            Rgb {
                r: 128,
                g: 255,
                b: 0,
            },
            true,
        );
        assert!((c[0] - 0.2158).abs() < 0.001);
        assert_eq!(c[1], 1.0);
        assert_eq!(c[2], 0.0);
    }
}
