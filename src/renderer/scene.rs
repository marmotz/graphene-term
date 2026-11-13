//! Conversion of a [`Frame`] into GPU-ready primitives (pure, no GPU access).

use alacritty_terminal::vte::ansi::{CursorShape, Rgb};

use super::glyphs::GlyphKey;
use super::rects::{RectInstance, Shape};
use crate::blocks::Status;
use crate::frame::{self, Card, Fold, Frame};
use crate::i18n::{self, Msg};
use crate::layout::GridLayout;

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

/// Top and bottom pixel edges of the rows of `card`, the top one unclipped.
fn card_edges(frame: &Frame, layout: &GridLayout, card: &Card) -> (f32, f32) {
    let top = layout.row_top(frame.row_tops[card.rows.start]);
    let bottom = layout.row_top(frame.row_tops[card.rows.end - 1] + 2);
    (top, bottom)
}

/// Card backgrounds and status stripes, drawn below the cell backgrounds.
fn cards_under(frame: &Frame, layout: &GridLayout, srgb_target: bool) -> Vec<RectInstance> {
    let cell = layout.cell;
    let radius = (cell.height / 4.0).round();
    let stripe = (cell.width / 6.0).round().max(2.0);
    let left = layout.grid_left() - layout.card_pad;
    let width = layout.grid_right() + layout.card_pad - left;
    let mut rects = Vec::new();
    for card in &frame.cards {
        let (mut top, bottom) = card_edges(frame, layout, card);
        // A card cut by the view top keeps its rounded corners above it.
        if card.clipped_top {
            top -= radius;
        }
        let color = if card.selected {
            frame.block_colors.card_selected
        } else {
            frame.block_colors.card
        };
        rects.push(
            RectInstance::new(
                [left, top],
                [width, bottom - top],
                gpu_color(color, srgb_target),
            )
            .with_radius(radius),
        );
        if let Some(status) = card.status {
            // Centered in the margin, with round ends.
            let x = layout.padding + ((layout.margin - stripe) / 2.0).max(0.0).round();
            rects.push(
                RectInstance::new(
                    [x, top],
                    [stripe, bottom - top],
                    gpu_color(frame.block_colors.status(status), srgb_target),
                )
                .with_radius(stripe / 2.0),
            );
        }
    }
    rects
}

/// Fold chevrons, drawn above the glyphs: a disc in the card color on the card's
/// top-right corner, then a triangle in the status color.
fn chevrons(frame: &Frame, layout: &GridLayout, srgb_target: bool) -> Vec<RectInstance> {
    let radius = layout.chevron_radius;
    let center_x = layout.grid_right() + layout.card_pad;
    let side = (radius * 1.2).round();
    let mut rects = Vec::new();
    for card in &frame.cards {
        let Some(fold) = card.chevron else {
            continue;
        };
        let (center_y, _) = card_edges(frame, layout, card);
        let disc = if card.selected {
            frame.block_colors.card_selected
        } else {
            frame.block_colors.card
        };
        rects.push(
            RectInstance::new(
                [center_x - radius, center_y - radius],
                [2.0 * radius, 2.0 * radius],
                gpu_color(disc, srgb_target),
            )
            .with_radius(radius),
        );
        let color = match card.status {
            None | Some(Status::Unknown) => frame.block_colors.dim,
            Some(status) => frame.block_colors.status(status),
        };
        let shape = match fold {
            Fold::Folded => Shape::TriangleRight,
            Fold::Unfolded => Shape::TriangleDown,
        };
        rects.push(
            RectInstance::new(
                [center_x - side / 2.0, center_y - side / 2.0],
                [side, side],
                gpu_color(color, srgb_target),
            )
            .with_shape(shape),
        );
    }
    rects
}

pub fn build(frame: &Frame, layout: &GridLayout, srgb_target: bool) -> Scene {
    let cell = layout.cell;
    let thickness = (cell.height / 16.0).round().max(1.0);
    let mut under = cards_under(frame, layout, srgb_target);
    let mut over = Vec::new();
    let mut glyphs = Vec::new();

    for (line, cells) in frame.rows.iter().enumerate() {
        let y = layout.row_top(frame.row_tops[line]);
        // Backgrounds: merge consecutive cells with the same non-default color.
        let mut column = 0;
        while column < cells.len() {
            let bg = cells[column].bg;
            let start = column;
            while column < cells.len() && cells[column].bg == bg {
                column += 1;
            }
            if bg != frame.background {
                let x = layout.cell_x(start);
                under.push(RectInstance::new(
                    [x, y],
                    [(column - start) as f32 * cell.width, cell.height],
                    gpu_color(bg, srgb_target),
                ));
            }
        }

        // Hidden-rows summary in dim text after the row's content, cut at the grid edge.
        let decoration = frame.decorations.get(line).copied().unwrap_or_default();
        if let Some(hidden) = decoration.summary {
            let label = i18n::text(frame.lang, &Msg::LinesHidden(hidden));
            let start = frame::summary_column(cells.iter().map(|c| c.c == ' ' && !c.spacer));
            let color = gpu_color(frame.block_colors.dim, srgb_target);
            for (column, c) in (start..cells.len()).zip(label.chars()) {
                if c != ' ' {
                    let x = layout.cell_x(column);
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
            let x = layout.cell_x(column);
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
            let x = layout.cell_x(column);
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

    over.extend(chevrons(frame, layout, srgb_target));

    if let Some(cursor) = frame.cursor {
        let (x, y) = (
            layout.cell_x(cursor.column),
            layout.row_top(frame.row_tops[cursor.line]),
        );
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
    use crate::frame::{
        BlockColors, Card, Fold, FrameBanner, FrameCell, FrameCursor, RowDecoration,
    };
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

    /// 10 x 20 px cells, no padding: the grid starts after the 5 px margin and the 5 px
    /// card padding.
    fn margin_layout() -> GridLayout {
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
            margin: 0.0,
            card_pad: 0.0,
            ..margin_layout()
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
    const CARD: Rgb = Rgb {
        r: 15,
        g: 15,
        b: 15,
    };
    const CARD_SELECTED: Rgb = Rgb {
        r: 36,
        g: 36,
        b: 36,
    };

    fn card(rows: std::ops::Range<usize>, status: Option<Status>) -> Card {
        Card {
            rows,
            clipped_top: false,
            status,
            chevron: None,
            selected: false,
        }
    }

    fn frame(rows: Vec<Vec<FrameCell>>) -> Frame {
        Frame {
            decorations: vec![RowDecoration::default(); rows.len()],
            cards: Vec::new(),
            row_tops: (0..rows.len() as u32).map(|row| 2 * row).collect(),
            block_colors: BlockColors {
                success: GREEN,
                failure: RED,
                running: YELLOW,
                dim: GREY,
                card: CARD,
                card_selected: CARD_SELECTED,
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
    fn rows_are_placed_at_their_view_offset() {
        let mut f = frame(vec![vec![cell('a')], vec![cell('b')], vec![cell('c')]]);
        // One leftover half-line at the top, a gap before the last row.
        f.row_tops = vec![1, 3, 6];
        f.rows[2][0].bg = RED;
        f.cursor = Some(FrameCursor {
            column: 0,
            line: 2,
            shape: CursorShape::Beam,
            color: RED,
        });
        let scene = build(&f, &layout(), false);
        assert_eq!(
            placed(&scene.glyphs),
            vec![([0.0, 10.0], 'a'), ([0.0, 30.0], 'b'), ([0.0, 60.0], 'c')]
        );
        assert_eq!(scene.under[0].position, [0.0, 60.0]);
        assert_eq!(scene.over[0].position, [0.0, 60.0]);
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

    /// (position, size, color, radius, shape) of a rect.
    type Drawn = ([f32; 2], [f32; 2], [f32; 4], f32, u32);

    fn shapes(rects: &[RectInstance]) -> Vec<Drawn> {
        rects
            .iter()
            .map(|r| (r.position, r.size, r.color, r.radius, r.shape))
            .collect()
    }

    #[test]
    fn no_card_without_blocks() {
        let scene = build(&frame(vec![vec![cell('a')]; 3]), &margin_layout(), false);
        assert!(scene.under.is_empty());
        assert!(scene.over.is_empty());
    }

    #[test]
    fn card_background_and_status_stripe_below_the_cells() {
        let mut f = frame(vec![vec![cell('a')]; 5]);
        // A gap before the second card: rows at half-lines 0, 2 | 5, 7, 9.
        f.row_tops = vec![0, 2, 5, 7, 9];
        f.rows[3][0].bg = RED;
        f.cards = vec![
            card(0..2, Some(Status::Success)),
            card(2..5, Some(Status::Failure(2))),
        ];
        let l = margin_layout();
        let scene = build(&f, &l, false);
        let rgb = |c: Rgb| gpu_color(c, false);
        // Cards from the margin edge (5 px) to the grid right edge plus the card
        // padding, radius a quarter of the cell height. Stripes 2 px wide (a sixth of
        // the cell width, at least 2), centered in the 5 px margin, round ends.
        let width = l.grid_right() + 5.0 - 5.0;
        assert_eq!(
            shapes(&scene.under),
            vec![
                ([5.0, 0.0], [width, 40.0], rgb(CARD), 5.0, 0),
                ([2.0, 0.0], [2.0, 40.0], rgb(GREEN), 1.0, 0),
                ([5.0, 50.0], [width, 60.0], rgb(CARD), 5.0, 0),
                ([2.0, 50.0], [2.0, 60.0], rgb(RED), 1.0, 0),
                // The cell background comes after the cards.
                ([10.0, 70.0], [10.0, 20.0], rgb(RED), 0.0, 0),
            ]
        );
        // The grid itself starts after the margin and the card padding.
        assert_eq!(placed(&scene.glyphs)[0], ([10.0, 0.0], 'a'));
    }

    #[test]
    fn neutral_card_has_no_stripe_and_a_selected_card_is_brighter() {
        let mut f = frame(vec![vec![cell('a')]; 2]);
        f.cards = vec![card(0..1, None), card(1..2, Some(Status::Running))];
        f.cards[1].selected = true;
        let scene = build(&f, &margin_layout(), false);
        let colors: Vec<_> = scene.under.iter().map(|r| r.color).collect();
        assert_eq!(
            colors,
            [
                gpu_color(CARD, false),
                gpu_color(CARD_SELECTED, false),
                gpu_color(YELLOW, false)
            ]
        );
    }

    #[test]
    fn card_clipped_at_the_top_starts_above_the_view() {
        let mut f = frame(vec![vec![cell('a')]; 2]);
        f.cards = vec![Card {
            clipped_top: true,
            ..card(0..2, Some(Status::Success))
        }];
        let scene = build(&f, &margin_layout(), false);
        // Background and stripe start one radius (5 px) above the first row.
        assert_eq!(scene.under[0].position[1], -5.0);
        assert_eq!(scene.under[0].size[1], 45.0);
        assert_eq!(scene.under[1].position[1], -5.0);
    }

    #[test]
    fn chevron_disc_and_triangle_on_the_card_corner_above_the_glyphs() {
        let mut f = frame(vec![vec![cell('a')]; 4]);
        f.row_tops = vec![0, 2, 5, 7];
        f.cards = vec![
            Card {
                chevron: Some(Fold::Folded),
                ..card(0..2, Some(Status::Failure(1)))
            },
            Card {
                chevron: Some(Fold::Unfolded),
                selected: true,
                ..card(2..4, Some(Status::Unknown))
            },
        ];
        let l = margin_layout();
        let scene = build(&f, &l, false);
        let rgb = |c: Rgb| gpu_color(c, false);
        // Radius round(20 * 0.45) = 9, centered on the card's top-right corner; the
        // triangle is an 11 px box (round(9 * 1.2)) in its middle.
        let cx = l.grid_right() + 5.0;
        assert_eq!(
            shapes(&scene.over),
            vec![
                ([cx - 9.0, -9.0], [18.0, 18.0], rgb(CARD), 9.0, 0),
                (
                    [cx - 5.5, -5.5],
                    [11.0, 11.0],
                    rgb(RED),
                    0.0,
                    Shape::TriangleRight as u32
                ),
                ([cx - 9.0, 41.0], [18.0, 18.0], rgb(CARD_SELECTED), 9.0, 0),
                // `Unknown`: the dim color.
                (
                    [cx - 5.5, 44.5],
                    [11.0, 11.0],
                    rgb(GREY),
                    0.0,
                    Shape::TriangleDown as u32
                ),
            ]
        );
        // No glyph is drawn for the chevrons.
        assert_eq!(scene.glyphs.len(), 4);
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
    fn summary_after_the_row_content() {
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
        frame.decorations[0].summary = Some(12);
        let scene = build(&frame, &margin_layout(), false);
        let grey = gpu_color(GREY, false);
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
