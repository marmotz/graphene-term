//! Conversion of a [`Frame`] into GPU-ready primitives (pure, no GPU access).

use std::sync::Arc;

use alacritty_terminal::vte::ansi::{CursorShape, Rgb};

use super::glyphs::GlyphKey;
use super::rects::RectInstance;
use crate::fold_animation;
use crate::frame::{self, Card, Frame, FrameImage, ScrollBar};
use crate::graphics::{ImageCell, Pixels};
use crate::i18n::{self, Msg};
use crate::layout::GridLayout;
use crate::timeline::Status;

fn plain(c: char) -> GlyphKey {
    GlyphKey {
        c,
        bold: false,
        italic: false,
        ui: None,
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

/// A cell of a [`TextRun`]: its character and linear RGBA color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunCell {
    pub c: char,
    pub color: [f32; 4],
}

/// Consecutive cells of a grid row shaped together, so that the font can form its
/// ligatures: one column each, no space, of one style.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    /// Origin of the first cell, in pixels.
    pub origin: [f32; 2],
    /// Width of a cell, in pixels: the cell `i` starts `i * advance` after `origin`.
    pub advance: f32,
    pub cells: Vec<RunCell>,
    pub bold: bool,
    pub italic: bool,
    /// Cell of the terminal cursor, around which the ligatures are broken.
    pub cursor: Option<usize>,
}

/// A part of an image to draw: a screen rectangle and the texture coordinates of the
/// part of the image it shows.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageQuad {
    /// Top-left corner, in pixels.
    pub position: [f32; 2],
    pub size: [f32; 2],
    /// Texture coordinates (0 to 1) of the top-left and bottom-right corners.
    pub uv: [f32; 4],
    pub pixels: Arc<Pixels>,
}

#[derive(Debug, Clone)]
pub struct Scene {
    /// Clear color (linear or sRGB depending on the target).
    pub clear: [f32; 4],
    /// Rectangles drawn below the glyphs.
    pub under: Vec<RectInstance>,
    /// Rectangles drawn above the glyphs.
    pub over: Vec<RectInstance>,
    /// Images, drawn over `under` and below the glyphs.
    pub images: Vec<ImageQuad>,
    /// Glyphs of the grid cells drawn one by one.
    pub glyphs: Vec<GlyphInstance>,
    /// Runs of grid cells shaped together, with ligatures only.
    pub runs: Vec<TextRun>,
    /// Rectangles covering the whole window, drawn after every pane and the interface
    /// (banner background).
    pub overlay_rects: Vec<RectInstance>,
    /// Glyphs drawn after `overlay_rects` (banner text).
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

/// `color` (already converted by [`gpu_color`]) with `opacity`, premultiplied as the
/// surface expects: `[r * a, g * a, b * a, a]`. Only the clear color carries the
/// window's background opacity; every rect stays opaque.
pub fn premultiplied(color: [f32; 4], opacity: f32) -> [f32; 4] {
    let [r, g, b, a] = color;
    let a = a * opacity;
    [r * a, g * a, b * a, a]
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
    let width = layout.card_right() - left;
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
            let x = layout.content_left() + ((layout.margin - stripe) / 2.0).max(0.0).round();
            rects.push(
                RectInstance::new(
                    [x, top],
                    [stripe, bottom - top],
                    gpu_color(frame.block_colors.status(status), srgb_target),
                )
                .with_radius(stripe / 2.0),
            );
        }
        if let Some(bar) = &card.scroll {
            rects.push(scroll_thumb(frame, layout, bar, stripe, srgb_target));
        }
    }
    rects
}

/// Scroll indicator of a card: a thin bar with round ends in the card's right padding,
/// in the dim color, beside the output rows (the track), placed and sized after the
/// share of the output shown. Never shorter than it is wide.
fn scroll_thumb(
    frame: &Frame,
    layout: &GridLayout,
    bar: &ScrollBar,
    width: f32,
    srgb_target: bool,
) -> RectInstance {
    let track_top = layout.row_top(frame.row_tops[bar.rows.start]);
    let track_bottom = layout.row_top(frame.row_tops[bar.rows.end - 1] + 2);
    let track = track_bottom - track_top;
    let height = (bar.length * track).round().clamp(width, track);
    let top = (track_top + (bar.start * track).round()).min(track_bottom - height);
    let x = layout.grid_right() + ((layout.card_pad_right - width) / 2.0).max(0.0).round();
    RectInstance::new(
        [x, top],
        [width, height],
        gpu_color(frame.block_colors.dim, srgb_target),
    )
    .with_radius(width / 2.0)
}

/// Fold chevrons, drawn above the glyphs: a stroke in the status color, centered in
/// the right card padding of the card's first row, bent into a V pointing down
/// (unfolded) or up (folded), and through a straight line while the fold changes.
fn chevrons(frame: &Frame, layout: &GridLayout, srgb_target: bool) -> Vec<RectInstance> {
    let width = layout.chevron_width;
    let stroke = (layout.cell.height / 10.0).max(1.5);
    let height = (width * 0.35).round() + stroke;
    let center_x = layout.grid_right() + layout.card_pad_right / 2.0;
    let mut rects = Vec::new();
    for card in &frame.cards {
        let Some(fold) = card.chevron else {
            continue;
        };
        let (top, _) = card_edges(frame, layout, card);
        let center_y = top + layout.cell.height / 2.0;
        let color = match card.status {
            None | Some(Status::Unknown | Status::Cancelled | Status::Empty) => {
                frame.block_colors.dim
            }
            Some(status) => frame.block_colors.status(status),
        };
        rects.push(
            RectInstance::new(
                [center_x - width / 2.0, center_y - height / 2.0],
                [width, height],
                gpu_color(color, srgb_target),
            )
            .chevron(stroke, fold_animation::bend(fold, card.fold_progress)),
        );
    }
    rects
}

/// Quads of the image cells of `cells`, frame row `line`: one per run of cells showing
/// consecutive columns of one row of one placement. The image is scaled to fit its
/// placement's box of cells, keeping its aspect ratio, and centered in it; each quad
/// shows the part of the image that falls in its cells.
fn image_quads(
    frame: &Frame,
    layout: &GridLayout,
    line: usize,
    cells: &[frame::FrameCell],
) -> Vec<ImageQuad> {
    let cell_size = layout.cell;
    let y = layout.row_top(frame.row_tops[line]);
    let mut quads = Vec::new();
    let mut column = 0;
    while column < cells.len() {
        let Some(first) = cells[column].image else {
            column += 1;
            continue;
        };
        let start = column;
        column += 1;
        let continues = |cell: Option<ImageCell>, offset: usize| {
            cell.is_some_and(|cell| {
                cell.image == first.image
                    && cell.placement == first.placement
                    && cell.row == first.row
                    && cell.col == first.col + offset as u32
            })
        };
        while column < cells.len() && continues(cells[column].image, column - start) {
            column += 1;
        }
        let Some(image) = frame
            .images
            .iter()
            .find(|image| image.image == first.image && image.placement == first.placement)
        else {
            continue;
        };
        let run = first.col..first.col + (column - start) as u32;
        if let Some(quad) = image_quad(image, cell_size.width, cell_size.height, first.row, run) {
            let x = layout.cell_x(start);
            quads.push(ImageQuad {
                position: [x + quad.0[0], y + quad.0[1]],
                ..quad.1
            });
        }
    }
    quads
}

/// The part of `image` shown by the cells `cols` of row `row` of its box: its offset
/// from the top-left corner of the first cell, and the quad (position left at 0).
/// `None` when the image does not reach these cells.
fn image_quad(
    image: &FrameImage,
    cell_width: f32,
    cell_height: f32,
    row: u32,
    cols: std::ops::Range<u32>,
) -> Option<([f32; 2], ImageQuad)> {
    let source = image.source;
    let (box_width, box_height) = (
        image.cols as f32 * cell_width,
        image.rows as f32 * cell_height,
    );
    let scale = (box_width / source.width as f32).min(box_height / source.height as f32);
    let (width, height) = (source.width as f32 * scale, source.height as f32 * scale);
    // The image, inside the box.
    let left = (box_width - width) / 2.0;
    let top = (box_height - height) / 2.0;
    // The cells, inside the box.
    let cells_left = cols.start as f32 * cell_width;
    let cells_top = row as f32 * cell_height;
    let cells_right = cols.end as f32 * cell_width;
    let cells_bottom = cells_top + cell_height;

    let x0 = left.max(cells_left);
    let y0 = top.max(cells_top);
    let x1 = (left + width).min(cells_right);
    let y1 = (top + height).min(cells_bottom);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (image_width, image_height) = (image.pixels.width as f32, image.pixels.height as f32);
    let u = |x: f32| (source.x as f32 + (x - left) / scale) / image_width;
    let v = |y: f32| (source.y as f32 + (y - top) / scale) / image_height;
    Some((
        [x0 - cells_left, y0 - cells_top],
        ImageQuad {
            position: [0.0, 0.0],
            size: [x1 - x0, y1 - y0],
            uv: [u(x0), v(y0), u(x1), v(y1)],
            pixels: image.pixels.clone(),
        },
    ))
}

/// The glyph of the grid cell `c` at `origin`.
fn cell_glyph(c: &frame::FrameCell, origin: [f32; 2], srgb_target: bool) -> GlyphInstance {
    GlyphInstance {
        origin,
        key: GlyphKey {
            c: c.c,
            bold: c.bold,
            italic: c.italic,
            ui: None,
        },
        color: gpu_color(c.fg, srgb_target),
    }
}

/// Whether the cell `column` of `cells` can be part of a [`TextRun`]: a drawn character
/// of one column, outside images.
fn runnable(cells: &[frame::FrameCell], column: usize) -> bool {
    let c = &cells[column];
    !c.spacer
        && c.c != ' '
        && c.image.is_none()
        && !cells.get(column + 1).is_some_and(|next| next.spacer)
}

/// Whether two cells have the same style, so that they belong to the same run. With
/// `colors` false, only the attributes are compared: the colors of the block cursor's
/// cell are baked by the cursor, which does not cut the run.
fn same_style(a: &frame::FrameCell, b: &frame::FrameCell, colors: bool) -> bool {
    (a.bold, a.italic, a.underline, a.strikeout) == (b.bold, b.italic, b.underline, b.strikeout)
        && (!colors || (a.fg, a.bg) == (b.fg, b.bg))
}

/// Glyphs of the row `cells` starting at `origin`, with ligatures: maximal runs of
/// runnable cells of one style go to `runs`, the other drawn cells (and the runs of a
/// single cell) to `glyphs`, one each. The column `cursor` holds the terminal cursor:
/// its colors are left out of the style comparison, and its run records it.
fn push_runs(
    layout: &GridLayout,
    srgb_target: bool,
    origin: [f32; 2],
    cells: &[frame::FrameCell],
    cursor: Option<usize>,
    glyphs: &mut Vec<GlyphInstance>,
    runs: &mut Vec<TextRun>,
) {
    let x = |column: usize| origin[0] + column as f32 * layout.cell.width;
    let mut column = 0;
    while column < cells.len() {
        let c = &cells[column];
        if !runnable(cells, column) {
            if !c.spacer && c.c != ' ' {
                glyphs.push(cell_glyph(c, [x(column), origin[1]], srgb_target));
            }
            column += 1;
            continue;
        }
        let start = column;
        // The first cell of the run outside the cursor, whose colors the others match.
        let mut reference = (cursor != Some(start)).then_some(c);
        column += 1;
        while column < cells.len() && runnable(cells, column) {
            let next = &cells[column];
            let fits = match reference {
                _ if cursor == Some(column) => same_style(c, next, false),
                Some(reference) => same_style(reference, next, true),
                None => same_style(c, next, false),
            };
            if !fits {
                break;
            }
            if cursor != Some(column) {
                reference.get_or_insert(next);
            }
            column += 1;
        }
        if column - start == 1 {
            glyphs.push(cell_glyph(c, [x(start), origin[1]], srgb_target));
            continue;
        }
        runs.push(TextRun {
            origin: [x(start), origin[1]],
            advance: layout.cell.width,
            cells: cells[start..column]
                .iter()
                .map(|cell| RunCell {
                    c: cell.c,
                    color: gpu_color(cell.fg, srgb_target),
                })
                .collect(),
            bold: c.bold,
            italic: c.italic,
            cursor: cursor
                .filter(|cursor| (start..column).contains(cursor))
                .map(|cursor| cursor - start),
        });
    }
}

/// The scene of `frame`. With `ligatures`, the grid cells are grouped into
/// [`TextRun`]s shaped together; otherwise each cell is drawn on its own.
pub fn build(frame: &Frame, layout: &GridLayout, srgb_target: bool, ligatures: bool) -> Scene {
    let cell = layout.cell;
    let thickness = (cell.height / 16.0).round().max(1.0);
    let mut under = cards_under(frame, layout, srgb_target);
    let mut over = Vec::new();
    let mut glyphs = Vec::new();
    let mut runs = Vec::new();
    let mut images = Vec::new();

    for (line, cells) in frame.rows.iter().enumerate() {
        let y = layout.row_top(frame.row_tops[line]);
        if !frame.images.is_empty() {
            images.extend(image_quads(frame, layout, line, cells));
        }
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

        if ligatures {
            push_runs(
                layout,
                srgb_target,
                [layout.cell_x(0), y],
                cells,
                frame
                    .ligature_break
                    .filter(|(cursor_line, _)| *cursor_line == line)
                    .map(|(_, column)| column),
                &mut glyphs,
                &mut runs,
            );
        } else {
            // Glyphs: one per drawn cell. A wide character is drawn once, from its first
            // cell; its spacer draws nothing.
            for (column, c) in cells.iter().enumerate() {
                if c.spacer || c.c == ' ' {
                    continue;
                }
                glyphs.push(cell_glyph(c, [layout.cell_x(column), y], srgb_target));
            }
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

    // The banner sits in its own strip above the tab bar and the panes, drawn after them:
    // its rect and its glyphs go in the overlay, one character per cell width from the
    // text position.
    let mut overlay_rects = Vec::new();
    let mut overlay = Vec::new();
    if let Some(banner) = &frame.banner {
        let rect = banner.layout.rect;
        overlay_rects.push(RectInstance::new(
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
                    ui: None,
                },
                color,
            }));
        }
    }

    Scene {
        clear: gpu_color(frame.background, srgb_target),
        under,
        over,
        images,
        glyphs,
        runs,
        overlay_rects,
        overlay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::banner::{Banner, BannerLayout};
    use crate::frame::{
        BlockColors, Card, Fold, FrameBanner, FrameCell, FrameCursor, RowDecoration, ScrollBar,
    };
    use crate::layout::{CellSize, Rect};
    use crate::renderer::rects::Shape;
    use crate::timeline::Status;

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
            image: None,
        }
    }

    /// 10 x 20 px cells, no padding and no centring: the grid starts after the 5 px
    /// margin and the 5 px card padding.
    fn margin_layout() -> GridLayout {
        GridLayout {
            origin: (0.0, 0.0),
            ..GridLayout::new(
                Rect::from_size(100, 100),
                CellSize {
                    width: 10.0,
                    height: 20.0,
                },
                0.0,
            )
        }
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
            fold_progress: 1.0,
            selected: false,
            scroll: None,
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
            ligature_break: None,
            background: BG,
            banner: None,
            lang: crate::i18n::Lang::En,
            images: Vec::new(),
        }
    }

    fn image_cell(row: u32, col: u32) -> FrameCell {
        FrameCell {
            image: Some(ImageCell {
                image: 7,
                placement: 1,
                row,
                col,
            }),
            ..cell(' ')
        }
    }

    /// Image 7 placement 1: a 4 x 4 px image in a box of 2 x 2 cells (20 x 40 px).
    fn with_image(mut frame: Frame) -> Frame {
        frame.images.push(FrameImage {
            image: 7,
            placement: 1,
            cols: 2,
            rows: 2,
            source: crate::graphics::SourceRect {
                x: 0,
                y: 0,
                width: 4,
                height: 4,
            },
            pixels: Pixels::new(4, 4, vec![0; 64]).unwrap(),
        });
        frame
    }

    fn quad_geometry(scene: &Scene) -> Vec<([f32; 2], [f32; 2], [f32; 4])> {
        scene
            .images
            .iter()
            .map(|quad| (quad.position, quad.size, quad.uv))
            .collect()
    }

    #[test]
    fn image_is_fit_in_its_box_and_centered_one_quad_per_row() {
        let frame = with_image(frame(vec![
            vec![cell('a'), image_cell(0, 0), image_cell(0, 1)],
            vec![cell('b'), image_cell(1, 0), image_cell(1, 1)],
        ]));
        let scene = build(&frame, &layout(), false, false);
        // Scaled 5 times to 20 x 20 px, 10 px from the box top.
        assert_eq!(
            quad_geometry(&scene),
            vec![
                ([10.0, 10.0], [20.0, 10.0], [0.0, 0.0, 1.0, 0.5]),
                ([10.0, 20.0], [20.0, 10.0], [0.0, 0.5, 1.0, 1.0]),
            ]
        );
        // Image cells draw no glyph and no background.
        let drawn: Vec<char> = scene.glyphs.iter().map(|glyph| glyph.key.c).collect();
        assert_eq!(drawn, vec!['a', 'b']);
        assert!(scene.under.is_empty());
    }

    #[test]
    fn a_cut_image_shows_the_part_of_its_cells() {
        // Only the second column of the first row, at the left edge.
        let frame = with_image(frame(vec![vec![image_cell(0, 1), cell('c')]]));
        let scene = build(&frame, &layout(), false, false);
        assert_eq!(
            quad_geometry(&scene),
            vec![([0.0, 10.0], [10.0, 10.0], [0.5, 0.0, 1.0, 0.5])]
        );
    }

    #[test]
    fn image_cells_without_their_image_draw_nothing() {
        let scene = build(
            &frame(vec![vec![image_cell(0, 0), image_cell(0, 1)]]),
            &layout(),
            false,
            false,
        );
        assert!(scene.images.is_empty());
        assert!(scene.glyphs.is_empty());
    }

    #[test]
    fn default_background_produces_no_rect() {
        let scene = build(
            &frame(vec![vec![cell('a'), cell('b')]]),
            &layout(),
            false,
            false,
        );
        assert!(scene.under.is_empty());
        assert_eq!(scene.clear, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn adjacent_backgrounds_are_merged() {
        let mut row = vec![cell('a'), cell('b'), cell('c')];
        row[1].bg = RED;
        row[2].bg = RED;
        let scene = build(&frame(vec![row]), &layout(), false, false);
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
        let scene = build(&f, &layout(), false, false);
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
        let scene = build(&frame(vec![vec![cell('x')], row]), &layout(), false, false);
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
                italic: true,
                ui: None,
            }
        );
        assert_eq!(b.color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn spaces_draw_no_glyph() {
        let mut row = vec![cell('a'), cell(' '), cell(' '), cell('b')];
        row[1].fg = RED;
        row[2].bg = RED;
        let scene = build(&frame(vec![row]), &layout(), false, false);
        assert_eq!(
            placed(&scene.glyphs),
            vec![([0.0, 0.0], 'a'), ([30.0, 0.0], 'b')]
        );
    }

    #[test]
    fn wide_char_is_drawn_once_and_keeps_the_next_cell_aligned() {
        let mut row = vec![cell('日'), cell(' '), cell('c')];
        row[1].spacer = true;
        let scene = build(&frame(vec![row]), &layout(), false, false);
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
        let scene = build(&f, &layout(), false, false);
        assert_eq!(scene.over.len(), 2);
        assert_eq!(scene.over[1].position, [10.0, 0.0]);
    }

    #[test]
    fn no_banner_draws_no_overlay() {
        let scene = build(&frame(vec![vec![cell('a')]]), &layout(), false, false);
        assert!(scene.over.is_empty());
        assert!(scene.overlay_rects.is_empty());
        assert!(scene.overlay.is_empty());
    }

    #[test]
    fn banner_goes_in_the_overlay_above_the_cursor() {
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
        let scene = build(&f, &layout(), false, false);
        // The cursor stays in `over`, clipped to its pane; the banner is drawn later.
        assert_eq!(scene.over.len(), 1);
        assert_eq!(scene.overlay_rects.len(), 1);
        let last = scene.overlay_rects[0];
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
        let scene = build(&f, &layout(), false, false);
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
        let scene = build(
            &frame(vec![vec![cell('a')]; 3]),
            &margin_layout(),
            false,
            false,
        );
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
        let scene = build(&f, &l, false, false);
        let rgb = |c: Rgb| gpu_color(c, false);
        // Cards from the margin edge (5 px) to the grid right edge plus the 22 px right
        // card padding, radius a quarter of the cell height. Stripes 2 px wide (a sixth
        // of the cell width, at least 2), centered in the 5 px margin, round ends.
        let width = l.grid_right() + 22.0 - 5.0;
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
    fn scroll_indicator_beside_the_output_rows_in_the_dim_color() {
        let mut f = frame(vec![vec![cell('a')]; 4]);
        f.row_tops = vec![0, 2, 4, 6];
        f.cards = vec![Card {
            // Output on rows 1 to 3 (pixels 20..80), the middle quarter shown.
            scroll: Some(ScrollBar {
                rows: 1..4,
                start: 0.5,
                length: 0.25,
            }),
            ..card(0..4, Some(Status::Success))
        }];
        let l = margin_layout();
        let scene = build(&f, &l, false, false);
        // Card, stripe, then the 2 px thumb centered in the 22 px right card padding,
        // under the chevron: a quarter of the 60 px track, halfway down.
        assert_eq!(scene.under.len(), 3);
        assert_eq!(
            shapes(&scene.under[2..]),
            vec![(
                [l.grid_right() + 10.0, 50.0],
                [2.0, 15.0],
                gpu_color(GREY, false),
                1.0,
                0
            )]
        );
    }

    #[test]
    fn scroll_indicator_keeps_a_minimum_length_inside_its_track() {
        let mut f = frame(vec![vec![cell('a')]; 4]);
        f.row_tops = vec![0, 2, 4, 6];
        f.cards = vec![Card {
            scroll: Some(ScrollBar {
                rows: 1..4,
                start: 1.0,
                length: 0.001,
            }),
            ..card(0..4, None)
        }];
        let scene = build(&f, &margin_layout(), false, false);
        // No stripe on a neutral card: the thumb follows the card background.
        let thumb = &scene.under[1];
        assert_eq!((thumb.position[1], thumb.size[1]), (78.0, 2.0));
    }

    #[test]
    fn neutral_card_has_no_stripe_and_a_selected_card_is_brighter() {
        let mut f = frame(vec![vec![cell('a')]; 2]);
        f.cards = vec![card(0..1, None), card(1..2, Some(Status::Running))];
        f.cards[1].selected = true;
        let scene = build(&f, &margin_layout(), false, false);
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
        let scene = build(&f, &margin_layout(), false, false);
        // Background and stripe start one radius (5 px) above the first row.
        assert_eq!(scene.under[0].position[1], -5.0);
        assert_eq!(scene.under[0].size[1], 45.0);
        assert_eq!(scene.under[1].position[1], -5.0);
    }

    #[test]
    fn chevron_in_the_right_card_padding_of_the_first_row_above_the_glyphs() {
        let mut f = frame(vec![vec![cell('a')]; 4]);
        f.row_tops = vec![0, 2, 5, 7];
        f.cards = vec![
            Card {
                chevron: Some(Fold::Folded),
                ..card(0..2, Some(Status::Failure(1)))
            },
            Card {
                chevron: Some(Fold::Unfolded),
                fold_progress: 0.5,
                selected: true,
                ..card(2..4, Some(Status::Unknown))
            },
        ];
        let l = margin_layout();
        let scene = build(&f, &l, false, false);
        let rgb = |c: Rgb| gpu_color(c, false);
        // A 12 x 6 box (4 px of bend, a 2 px stroke) centered in the 22 px right card
        // padding and on the first row (10 px below the card top at 0, then at 50).
        let x = l.grid_right() + 11.0 - 6.0;
        assert_eq!(
            shapes(&scene.over),
            vec![
                ([x, 7.0], [12.0, 6.0], rgb(RED), 1.0, Shape::Chevron as u32),
                // `Unknown`: the dim color.
                (
                    [x, 57.0],
                    [12.0, 6.0],
                    rgb(GREY),
                    1.0,
                    Shape::Chevron as u32
                ),
            ]
        );
        // Folded: a V pointing up. Halfway through unfolding: a straight line.
        assert_eq!(scene.over[0].bend, -1.0);
        assert_eq!(scene.over[1].bend, 0.0);
        // No glyph is drawn for the chevrons.
        assert_eq!(scene.glyphs.len(), 4);
    }

    #[test]
    fn opacity_premultiplies_the_clear_color_in_the_target_space() {
        let background = Rgb {
            r: 128,
            g: 255,
            b: 0,
        };
        let linear = premultiplied(gpu_color(background, false), 0.5);
        assert!((linear[0] - 128.0 / 255.0 * 0.5).abs() < 1e-6);
        assert_eq!(linear[1..], [0.5, 0.0, 0.5]);
        let srgb = premultiplied(gpu_color(background, true), 0.5);
        assert!((srgb[0] - 0.2158 * 0.5).abs() < 0.001);
        assert_eq!(srgb[1..], [0.5, 0.0, 0.5]);
        let opaque = gpu_color(background, true);
        assert_eq!(premultiplied(opaque, 1.0), opaque);
        assert_eq!(premultiplied(opaque, 0.0), [0.0; 4]);
    }

    #[test]
    fn backgrounds_cursor_and_banner_stay_opaque() {
        let banner = Banner {
            text: "oops".into(),
            actions: vec!["x".into()],
        };
        // Explicit background (also how the selection is drawn), cursor and banner.
        let mut row = vec![cell('a'), cell('b')];
        row[1].bg = RED;
        let mut f = frame(vec![row]);
        f.cursor = Some(FrameCursor {
            column: 0,
            line: 0,
            shape: CursorShape::Block,
            color: FG,
        });
        f.banner = Some(FrameBanner {
            layout: BannerLayout::new(&banner, layout().cell, 100.0),
            background: RED,
            text: FG,
        });
        let scene = build(&f, &layout(), true, false);
        let rects: Vec<_> = scene
            .under
            .iter()
            .chain(&scene.over)
            .chain(&scene.overlay_rects)
            .collect();
        assert!(rects.len() >= 3);
        assert!(rects.iter().all(|rect| rect.color[3] == 1.0));
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
        let scene = build(&frame, &margin_layout(), false, false);
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
            Rect::from_size(400, 100),
            CellSize {
                width: 10.0,
                height: 20.0,
            },
            0.0,
        );
        let scene = build(&frame, &layout, false, false);
        let grey = gpu_color(GREY, false);
        let summary: Vec<_> = scene.glyphs.iter().filter(|g| g.color == grey).collect();
        let text: String = summary.iter().map(|g| g.key.c).collect();
        // Spaces draw no glyph.
        assert_eq!(text, "1lignemasquée");
        // 36 columns leave 8 px over: the grid is centred 4 px further right.
        assert_eq!(summary[0].origin, [4.0 + 10.0 + 2.0 * 10.0, 0.0]);
    }

    /// (origin, text, bold, italic) of each run of `scene`.
    fn runs(scene: &Scene) -> Vec<([f32; 2], String, bool, bool)> {
        scene
            .runs
            .iter()
            .map(|run| {
                let text = run.cells.iter().map(|cell| cell.c).collect();
                (run.origin, text, run.bold, run.italic)
            })
            .collect()
    }

    fn row(text: &str) -> Vec<FrameCell> {
        text.chars().map(cell).collect()
    }

    #[test]
    fn without_ligatures_every_cell_is_drawn_on_its_own() {
        let scene = build(&frame(vec![row("a->b")]), &layout(), false, false);
        assert!(scene.runs.is_empty());
        assert_eq!(scene.glyphs.len(), 4);
    }

    #[test]
    fn ligatures_group_the_cells_into_runs_cut_at_spaces() {
        let scene = build(
            &frame(vec![row("a->b x  ==="), row("c")]),
            &layout(),
            false,
            true,
        );
        assert_eq!(
            runs(&scene),
            vec![
                ([0.0, 0.0], "a->b".into(), false, false),
                ([80.0, 0.0], "===".into(), false, false),
            ]
        );
        // Runs of one cell stay single glyphs.
        assert_eq!(
            placed(&scene.glyphs),
            vec![([50.0, 0.0], 'x'), ([0.0, 20.0], 'c')]
        );
        let run = &scene.runs[0];
        assert_eq!(run.advance, 10.0);
        assert_eq!(run.cursor, None);
        assert!(run.cells.iter().all(|cell| cell.color == [1.0; 4]));
    }

    #[test]
    fn runs_start_at_the_grid_origin() {
        let scene = build(&frame(vec![row(" ->")]), &margin_layout(), false, true);
        assert_eq!(runs(&scene), vec![([20.0, 0.0], "->".into(), false, false)]);
    }

    #[test]
    fn a_style_change_cuts_the_run() {
        let styled = |change: fn(&mut FrameCell)| {
            let mut cells = row("->>");
            change(&mut cells[2]);
            let scene = build(&frame(vec![cells]), &layout(), false, true);
            (runs(&scene), placed(&scene.glyphs))
        };
        let changes: [fn(&mut FrameCell); 6] = [
            |c| c.fg = RED,
            |c| c.bg = RED,
            |c| c.bold = true,
            |c| c.italic = true,
            |c| c.underline = true,
            |c| c.strikeout = true,
        ];
        for change in changes {
            assert_eq!(
                styled(change),
                (
                    vec![([0.0, 0.0], "->".into(), false, false)],
                    vec![([20.0, 0.0], '>')]
                )
            );
        }
    }

    #[test]
    fn runs_keep_their_style_and_the_color_of_each_cell() {
        let mut cells = row("=>");
        for cell in &mut cells {
            cell.bold = true;
            cell.italic = true;
            cell.fg = RED;
        }
        let scene = build(&frame(vec![cells]), &layout(), false, true);
        assert_eq!(runs(&scene), vec![([0.0, 0.0], "=>".into(), true, true)]);
        assert!(
            scene.runs[0]
                .cells
                .iter()
                .all(|cell| cell.color == [1.0, 0.0, 0.0, 1.0])
        );
    }

    #[test]
    fn wide_characters_and_image_cells_stay_out_of_runs() {
        let mut cells = row("ab日 cd");
        cells[3].spacer = true;
        cells.push(FrameCell {
            c: 'x',
            ..image_cell(0, 0)
        });
        cells.extend(row("ef"));
        let scene = build(&frame(vec![cells]), &layout(), false, true);
        assert_eq!(
            runs(&scene),
            vec![
                ([0.0, 0.0], "ab".into(), false, false),
                ([40.0, 0.0], "cd".into(), false, false),
                ([70.0, 0.0], "ef".into(), false, false),
            ]
        );
        assert_eq!(
            placed(&scene.glyphs),
            vec![([20.0, 0.0], '日'), ([60.0, 0.0], 'x')]
        );
    }

    /// The row `text` with the block cursor baked into the cell `column`.
    fn with_block_cursor(text: &str, column: usize) -> Frame {
        let mut cells = row(text);
        cells[column].bg = RED;
        cells[column].fg = GREY;
        let mut frame = frame(vec![cells]);
        frame.ligature_break = Some((0, column));
        frame
    }

    #[test]
    fn the_block_cursor_does_not_cut_the_run() {
        for column in 0..3 {
            let scene = build(&with_block_cursor("-->", column), &layout(), false, true);
            assert_eq!(runs(&scene), vec![([0.0, 0.0], "-->".into(), false, false)]);
            let run = &scene.runs[0];
            assert_eq!(run.cursor, Some(column));
            // The cursor cell keeps its own color.
            assert_eq!(run.cells[column].color, gpu_color(GREY, false));
        }
    }

    #[test]
    fn the_cursor_keeps_the_other_style_cuts() {
        // The cells around the cursor still differ in color.
        let mut frame = with_block_cursor("->>", 1);
        frame.rows[0][2].fg = RED;
        let scene = build(&frame, &layout(), false, true);
        assert_eq!(runs(&scene), vec![([0.0, 0.0], "->".into(), false, false)]);
        assert_eq!(scene.runs[0].cursor, Some(1));

        // A bold cursor cell cuts the run.
        let mut frame = with_block_cursor("->>", 1);
        frame.rows[0][1].bold = true;
        let scene = build(&frame, &layout(), false, true);
        assert!(scene.runs.is_empty());
    }

    #[test]
    fn only_the_run_holding_the_cursor_records_it() {
        let mut frame = frame(vec![row("-> =="), row("=>")]);
        frame.ligature_break = Some((0, 3));
        let scene = build(&frame, &layout(), false, true);
        let cursors: Vec<_> = scene.runs.iter().map(|run| run.cursor).collect();
        assert_eq!(cursors, [None, Some(0), None]);
    }
}
