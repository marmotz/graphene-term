//! Conversion of a [`Frame`] into GPU-ready primitives (pure, no GPU access).

use alacritty_terminal::vte::ansi::{CursorShape, Rgb};

use super::glyphs::GlyphKey;
use super::rects::RectInstance;
use crate::frame::{self, Fold, Frame};
use crate::i18n::{self, Msg};
use crate::layout::GridLayout;

/// Gutter chevron of a folded block.
pub const CHEVRON_FOLDED: char = '\u{25B8}';
/// Gutter chevron of an unfolded block.
pub const CHEVRON_UNFOLDED: char = '\u{25BE}';

fn plain(c: char) -> GlyphKey {
    GlyphKey {
        c,
        bold: false,
        italic: false,
    }
}

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
    let bar_width = (cell.width / 4.0).round().max(1.0);
    let grid_width = layout.columns as f32 * cell.width;

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
                under.push(RectInstance::new(
                    [x, y],
                    [(column - start) as f32 * cell.width, cell.height],
                    gpu_color(bg, srgb_target),
                ));
            }
        }

        // Command block marks, over the backgrounds and below the glyphs: the status bar
        // in the gutter, the separator at the top of the block's first row.
        let decoration = frame.decorations.get(line).copied().unwrap_or_default();
        let (grid_x, y) = layout.cell_origin(0, line);
        if let Some(status) = decoration.bar {
            under.push(RectInstance::new(
                [grid_x - layout.gutter, y],
                [bar_width, cell.height],
                gpu_color(frame.block_colors.status(status), srgb_target),
            ));
        }
        if decoration.separator {
            under.push(RectInstance::new(
                [grid_x, y],
                [grid_width, 1.0],
                gpu_color(frame.block_colors.dim, srgb_target),
            ));
        }
        // Fold chevron in the gutter, in the status color; hidden-rows summary in dim
        // text after the row's content, cut at the grid edge.
        if let Some(fold) = decoration.chevron {
            let status = decoration.bar.unwrap_or(crate::blocks::Status::Unknown);
            glyphs.push(GlyphInstance {
                origin: [grid_x - layout.gutter, y],
                key: plain(match fold {
                    Fold::Folded => CHEVRON_FOLDED,
                    Fold::Unfolded => CHEVRON_UNFOLDED,
                }),
                color: gpu_color(frame.block_colors.status(status), srgb_target),
            });
        }
        if let Some(hidden) = decoration.summary {
            let label = i18n::text(frame.lang, &Msg::LinesHidden(hidden));
            let start = frame::summary_column(cells.iter().map(|c| c.c == ' ' && !c.spacer));
            let color = gpu_color(frame.block_colors.dim, srgb_target);
            for (column, c) in (start..cells.len()).zip(label.chars()) {
                if c != ' ' {
                    let (x, y) = layout.cell_origin(column, line);
                    glyphs.push(GlyphInstance {
                        origin: [x, y],
                        key: plain(c),
                        color,
                    });
                }
            }
        }

        // Decorations.
        for (column, c) in cells.iter().enumerate() {
            let (x, y) = layout.cell_origin(column, line);
            let color = gpu_color(c.fg, srgb_target);
            if c.underline {
                over.push(RectInstance::new(
                    [x, y + cell.height - 2.0 * thickness],
                    [cell.width, thickness],
                    color,
                ));
            }
            if c.strikeout {
                over.push(RectInstance::new(
                    [x, y + (cell.height - thickness) / 2.0],
                    [cell.width, thickness],
                    color,
                ));
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
        let rect = |position: [f32; 2], size: [f32; 2]| RectInstance::new(position, size, color);
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
        over.push(RectInstance::new(
            [rect.x, rect.y],
            [rect.width, rect.height],
            gpu_color(banner.background, srgb_target),
        ));
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
    use crate::blocks::Status;
    use crate::frame::{BlockColors, Fold, FrameBanner, FrameCell, FrameCursor, RowDecoration};
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

    /// 10 x 20 px cells, no padding, the one-cell gutter left of the grid.
    fn gutter_layout() -> GridLayout {
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

    /// Same cells, with the grid at the origin.
    fn layout() -> GridLayout {
        GridLayout {
            gutter: 0.0,
            ..gutter_layout()
        }
    }

    const GREEN: Rgb = Rgb { r: 0, g: 255, b: 0 };
    const YELLOW: Rgb = Rgb {
        r: 255,
        g: 255,
        b: 0,
    };
    const GREY: Rgb = Rgb {
        r: 128,
        g: 128,
        b: 128,
    };

    fn frame(rows: Vec<Vec<FrameCell>>) -> Frame {
        Frame {
            decorations: vec![RowDecoration::default(); rows.len()],
            block_colors: BlockColors {
                success: GREEN,
                failure: RED,
                running: YELLOW,
                dim: GREY,
            },
            rows,
            cursor: None,
            background: BG,
            banner: None,
            lang: crate::i18n::Lang::En,
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
    fn block_rows_get_a_status_bar_in_the_gutter() {
        let mut f = frame(vec![vec![cell('a')]; 5]);
        let statuses = [
            None,
            Some(Status::Success),
            Some(Status::Failure(2)),
            Some(Status::Running),
            Some(Status::Unknown),
        ];
        for (decoration, bar) in f.decorations.iter_mut().zip(statuses) {
            decoration.bar = bar;
        }
        let l = gutter_layout();
        let scene = build(&f, &l, false);
        let rgb = |c: Rgb| gpu_color(c, false);
        // A quarter of a cell wide, full row height, at the gutter's left edge.
        let bars: Vec<_> = scene
            .under
            .iter()
            .map(|r| (r.position, r.size, r.color))
            .collect();
        assert_eq!(
            bars,
            vec![
                ([0.0, 20.0], [3.0, 20.0], rgb(GREEN)),
                ([0.0, 40.0], [3.0, 20.0], rgb(RED)),
                ([0.0, 60.0], [3.0, 20.0], rgb(YELLOW)),
                ([0.0, 80.0], [3.0, 20.0], rgb(GREY)),
            ]
        );
        // The grid itself starts after the gutter.
        assert_eq!(placed(&scene.glyphs)[0], ([10.0, 0.0], 'a'));
    }

    #[test]
    fn first_block_row_gets_a_separator_across_the_grid() {
        let mut f = frame(vec![vec![cell('a')]; 2]);
        f.decorations[1].separator = true;
        let l = gutter_layout();
        let scene = build(&f, &l, false);
        assert_eq!(scene.under.len(), 1);
        let separator = scene.under[0];
        assert_eq!(separator.position, [10.0, 20.0]);
        assert_eq!(separator.size, [l.columns as f32 * 10.0, 1.0]);
        assert_eq!(separator.color, gpu_color(GREY, false));
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

    fn placed_refs(glyphs: &[&GlyphInstance]) -> Vec<([f32; 2], char)> {
        glyphs.iter().map(|g| (g.origin, g.key.c)).collect()
    }

    #[test]
    fn chevron_in_the_gutter_and_summary_after_the_row_content() {
        let mut frame = frame(vec![
            vec![
                cell('$'),
                cell(' '),
                cell('l'),
                cell('s'),
                cell(' '),
                cell(' '),
            ],
            vec![cell('$'); 6],
        ]);
        frame.decorations[0] = RowDecoration {
            bar: Some(Status::Failure(2)),
            separator: true,
            chevron: Some(Fold::Folded),
            summary: Some(12),
        };
        frame.decorations[1].chevron = Some(Fold::Unfolded);
        let scene = build(&frame, &gutter_layout(), false);
        let red = gpu_color(RED, false);
        let grey = gpu_color(GREY, false);

        let chevron = scene
            .glyphs
            .iter()
            .find(|g| g.key.c == CHEVRON_FOLDED)
            .unwrap();
        assert_eq!(chevron.origin, [0.0, 0.0]);
        assert_eq!(chevron.color, red);
        // No status bar on row 1: the chevron takes the dim color.
        let unfolded = scene
            .glyphs
            .iter()
            .find(|g| g.key.c == CHEVRON_UNFOLDED)
            .unwrap();
        assert_eq!(unfolded.origin, [0.0, 20.0]);
        assert_eq!(unfolded.color, grey);

        // "12 lines hidden" from column 5 (two after "ls"), cut at the grid edge (6
        // columns): only "1" fits.
        let summary: Vec<_> = scene
            .glyphs
            .iter()
            .filter(|g| g.color == grey && g.origin[1] == 0.0)
            .collect();
        assert_eq!(placed_refs(&summary), [([10.0 + 5.0 * 10.0, 0.0], '1')]);
    }

    #[test]
    fn summary_is_translated() {
        let mut row = vec![cell(' '); 31];
        row[0] = cell('$');
        let mut frame = frame(vec![row]);
        frame.decorations[0].summary = Some(1);
        frame.lang = crate::i18n::Lang::Fr;
        let layout = GridLayout::new(
            400,
            100,
            CellSize {
                width: 10.0,
                height: 20.0,
            },
            0.0,
        );
        let scene = build(&frame, &layout, false);
        let grey = gpu_color(GREY, false);
        let summary: Vec<_> = scene.glyphs.iter().filter(|g| g.color == grey).collect();
        let text: String = summary.iter().map(|g| g.key.c).collect();
        // Spaces draw no glyph.
        assert_eq!(text, "1lignemasquée");
        assert_eq!(summary[0].origin, [10.0 + 2.0 * 10.0, 0.0]);
    }
}
