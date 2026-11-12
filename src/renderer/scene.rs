//! Conversion of a [`Frame`] into GPU-ready primitives (pure, no GPU access).

use alacritty_terminal::vte::ansi::{CursorShape, Rgb};

use super::glyphs::GlyphKey;
use super::rects::RectInstance;
use crate::frame::Frame;
use crate::layout::GridLayout;

/// A glyph to draw: the character of one cell (or of the banner), anchored at the
/// top-left corner of its cell. Spaces never produce one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphInstance {
    /// Cell origin, in pixels. The glyph placement offsets are added by the renderer.
    pub origin: [f32; 2],
    pub key: GlyphKey,
    /// Linear RGBA color (ignored by color glyphs such as emoji).
    pub color: [f32; 4],
}

#[derive(Debug, Clone)]
pub struct Scene {
    /// Clear color (linear or sRGB depending on the target).
    pub clear: [f32; 4],
    /// Rectangles drawn below the glyphs.
    pub under: Vec<RectInstance>,
    /// Rectangles drawn above the glyphs.
    pub over: Vec<RectInstance>,
    /// Glyphs of the grid cells.
    pub glyphs: Vec<GlyphInstance>,
    /// Glyphs drawn after the `over` rectangles (banner text).
    pub overlay: Vec<GlyphInstance>,
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
    let mut glyphs = Vec::new();

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

        // Glyphs: one per drawn cell. A wide character is drawn once, from its first
        // cell; its spacer draws nothing.
        for (column, c) in cells.iter().enumerate() {
            if c.spacer || c.c == ' ' {
                continue;
            }
            let (x, y) = layout.cell_origin(column, line);
            glyphs.push(GlyphInstance {
                origin: [x, y],
                key: GlyphKey {
                    c: c.c,
                    bold: c.bold,
                    italic: c.italic,
                },
                color: gpu_color(c.fg, srgb_target),
            });
        }
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

    // The banner covers the grid: its rect comes last and its glyphs go in the overlay,
    // one character per cell width from the text position.
    let mut overlay = Vec::new();
    if let Some(banner) = &frame.banner {
        let rect = banner.layout.rect;
        over.push(RectInstance {
            position: [rect.x, rect.y],
            size: [rect.width, rect.height],
            color: gpu_color(banner.background, srgb_target),
        });
        let color = gpu_color(banner.text, srgb_target);
        for placed in &banner.layout.texts {
            let characters = placed.text.chars().enumerate().filter(|(_, c)| *c != ' ');
            overlay.extend(characters.map(|(index, c)| GlyphInstance {
                origin: [placed.x + index as f32 * cell.width, placed.y],
                key: GlyphKey {
                    c,
                    bold: false,
                    italic: false,
                },
                color,
            }));
        }
    }

    Scene {
        clear: gpu_color(frame.background, srgb_target),
        under,
        over,
        glyphs,
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

    /// (origin, character) of each glyph of `glyphs`.
    fn placed(glyphs: &[GlyphInstance]) -> Vec<([f32; 2], char)> {
        glyphs.iter().map(|g| (g.origin, g.key.c)).collect()
    }

    #[test]
    fn glyphs_are_anchored_at_their_cell_origin_with_style() {
        let mut row = vec![cell('a'), cell('b')];
        row[1].fg = RED;
        row[1].bold = true;
        row[1].italic = true;
        let scene = build(&frame(vec![vec![cell('x')], row]), &layout(), false);
        assert_eq!(
            placed(&scene.glyphs),
            vec![([0.0, 0.0], 'x'), ([0.0, 20.0], 'a'), ([10.0, 20.0], 'b')]
        );
        let b = scene.glyphs[2];
        assert_eq!(
            b.key,
            GlyphKey {
                c: 'b',
                bold: true,
                italic: true
            }
        );
        assert_eq!(b.color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn spaces_draw_no_glyph() {
        let mut row = vec![cell('a'), cell(' '), cell(' '), cell('b')];
        row[1].fg = RED;
        row[2].bg = RED;
        let scene = build(&frame(vec![row]), &layout(), false);
        assert_eq!(
            placed(&scene.glyphs),
            vec![([0.0, 0.0], 'a'), ([30.0, 0.0], 'b')]
        );
    }

    #[test]
    fn wide_char_is_drawn_once_and_keeps_the_next_cell_aligned() {
        let mut row = vec![cell('日'), cell(' '), cell('c')];
        row[1].spacer = true;
        let scene = build(&frame(vec![row]), &layout(), false);
        assert_eq!(
            placed(&scene.glyphs),
            vec![([0.0, 0.0], '日'), ([20.0, 0.0], 'c')]
        );
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
        // Overlay glyphs follow the banner text, one cell width apart, spaces skipped.
        let texts = &f.banner.as_ref().unwrap().layout.texts;
        let (oops, x) = (&texts[0], &texts[1]);
        let expected: Vec<([f32; 2], char)> = "oops"
            .chars()
            .enumerate()
            .map(|(i, c)| ([oops.x + i as f32 * 10.0, oops.y], c))
            .chain([([x.x, x.y], 'x')])
            .collect();
        assert_eq!(placed(&scene.overlay), expected);
        assert!(scene.overlay.iter().all(|g| g.color == [1.0; 4]));
        // The grid glyph stays in the grid list, drawn before the `over` rects.
        assert_eq!(placed(&scene.glyphs), vec![([0.0, 0.0], 'a')]);
    }

    #[test]
    fn banner_spaces_draw_no_glyph() {
        let banner = Banner {
            text: "a b".into(),
            actions: vec![],
        };
        let mut f = frame(vec![vec![cell(' ')]]);
        f.banner = Some(FrameBanner {
            layout: BannerLayout::new(&banner, layout().cell, 100.0),
            background: RED,
            text: FG,
        });
        let scene = build(&f, &layout(), false);
        let text = &f.banner.as_ref().unwrap().layout.texts[0];
        assert_eq!(
            placed(&scene.overlay),
            vec![([text.x, text.y], 'a'), ([text.x + 20.0, text.y], 'b')]
        );
        assert!(scene.glyphs.is_empty());
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
