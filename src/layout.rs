//! Geometry: conversion between pixel sizes and terminal grid dimensions.

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::index::{Line, Side};
use alacritty_terminal::term::{MIN_COLUMNS, MIN_SCREEN_LINES};

use crate::timeline::BlockId;
use crate::view::{RowSource, ViewRow};

/// Size of a single character cell, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellSize {
    pub width: f32,
    pub height: f32,
}

/// A rectangle of the window, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// The whole surface of the given physical size, from the window corner.
    pub fn from_size(width: u32, height: u32) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
        }
    }

    /// Whether the physical pixel (`x`, `y`) lies in the rectangle (right and bottom
    /// edges excluded).
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// Placement of the terminal grid inside a pane rectangle of the window.
///
/// The rectangle holds, from the left: the padding, the margin (status stripe of the
/// command block cards), the card padding, the grid, the card padding again, then the
/// right reserve (room for the fold chevron on the cards' top-right corner). All of it
/// is always reserved, so a block never reflows the grid.
///
/// Vertically, rows are placed by the view in half-lines ([`ViewRow::top`]): a gap of
/// half a line separates two cards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLayout {
    /// Rectangle of the window the grid is laid out in.
    pub rect: Rect,
    pub cell: CellSize,
    /// Inner padding around the content, in physical pixels.
    pub padding: f32,
    /// Width of the margin left of the cards, in physical pixels: half a cell.
    pub margin: f32,
    /// Horizontal padding inside a card on each side of the grid: half a cell.
    pub card_pad: f32,
    /// Radius of the fold chevron disc, centered on a card's top-right corner.
    pub chevron_radius: f32,
    /// Room right of the cards: the padding, or the chevron radius when larger.
    pub right_reserve: f32,
    pub columns: usize,
    pub lines: usize,
}

/// What lies under a pixel of the window. `row` is a view row index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// Left of the grid (margin, left card padding or left padding).
    Margin { row: usize },
    /// Inside the fold chevron disc of the card starting on view row `row`.
    Chevron { row: usize },
    /// In the gap between two cards.
    Gap,
    /// Below the last view row, right of the margin: the blank space under the cards, or
    /// the bottom padding.
    Blank,
    /// A grid cell, with the half of the cell the pixel falls in, and what the view row
    /// shows: its source, the line in that source and its block (`None` on the
    /// alternate screen).
    Cell {
        column: usize,
        row: usize,
        side: Side,
        source: RowSource,
        line: Line,
        block: Option<BlockId>,
    },
}

/// Where a pixel row falls in the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowAt {
    Row(usize),
    Gap(usize),
}

impl GridLayout {
    /// Computes how many cells fit in the rectangle `rect` of the window.
    pub fn new(rect: Rect, cell: CellSize, padding: f32) -> Self {
        let margin = (cell.width / 2.0).round();
        let card_pad = (cell.width / 2.0).round();
        let chevron_radius = (cell.height * 0.45).round();
        let right_reserve = padding.max(chevron_radius);
        let horizontal = rect.width - padding - margin - 2.0 * card_pad - right_reserve;
        Self {
            rect,
            cell,
            padding,
            margin,
            card_pad,
            chevron_radius,
            right_reserve,
            columns: ((horizontal.max(0.0) / cell.width).floor() as usize).max(MIN_COLUMNS),
            lines: (((rect.height - 2.0 * padding).max(0.0) / cell.height).floor() as usize)
                .max(MIN_SCREEN_LINES),
        }
    }

    /// Left pixel edge of the content: the rectangle's left edge plus the padding.
    pub fn content_left(&self) -> f32 {
        self.rect.x + self.padding
    }

    /// Top pixel edge of the content: the rectangle's top edge plus the padding.
    pub fn content_top(&self) -> f32 {
        self.rect.y + self.padding
    }

    /// Whether the physical pixel (`x`, `y`) lies in the layout's rectangle.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.rect.contains(x, y)
    }

    /// Left pixel edge of the grid.
    pub fn grid_left(&self) -> f32 {
        self.content_left() + self.margin + self.card_pad
    }

    /// Right pixel edge of the grid.
    pub fn grid_right(&self) -> f32 {
        self.grid_left() + self.columns as f32 * self.cell.width
    }

    /// Left pixel edge of grid column `column`.
    pub fn cell_x(&self, column: usize) -> f32 {
        self.grid_left() + column as f32 * self.cell.width
    }

    /// Top pixel edge of a row placed `units` half-lines below the view top.
    pub fn row_top(&self, units: u32) -> f32 {
        self.content_top() + units as f32 * self.cell.height / 2.0
    }

    /// View row under the pixel row `y`, clamped to the view, or the gap above a row.
    fn row_at(&self, y: f32, view: &[ViewRow]) -> RowAt {
        let units = (y - self.content_top()) / (self.cell.height / 2.0);
        for (index, row) in view.iter().enumerate() {
            let top = row.top as f32;
            if units < top {
                return if index > 0 && row.gap_before && units >= top - 1.0 {
                    RowAt::Gap(index)
                } else {
                    RowAt::Row(index)
                };
            }
            if units < top + 2.0 {
                return RowAt::Row(index);
            }
        }
        RowAt::Row(view.len().saturating_sub(1))
    }

    /// Whether a card starts on view row `row`: a block row right below a row of another
    /// block, or the first view row.
    fn starts_card(view: &[ViewRow], row: usize) -> bool {
        let Some(block) = view[row].block else {
            return false;
        };
        row == 0 || view[row - 1].block != Some(block)
    }

    /// What lies under the physical pixel (`x`, `y`) of the view `view`: a fold chevron,
    /// a gap between cards, the blank space below the last row, the margin left of the
    /// grid, or a grid cell. Above the first row, the row is clamped to it; right of the
    /// grid, the column is clamped to the last one.
    pub fn hit(&self, x: f32, y: f32, view: &[ViewRow]) -> Hit {
        let cx = self.grid_right() + self.card_pad;
        let chevron = (0..view.len()).find(|&row| {
            let cy = self.row_top(view[row].top);
            Self::starts_card(view, row)
                && (x - cx).powi(2) + (y - cy).powi(2) <= self.chevron_radius.powi(2)
        });
        if let Some(row) = chevron {
            return Hit::Chevron { row };
        }
        if let RowAt::Gap(_) = self.row_at(y, view) {
            return Hit::Gap;
        }
        match view.last() {
            Some(last) if y < self.row_top(last.top + 2) => {}
            // The margin below the cards still belongs to the last row's block.
            Some(_) if x < self.grid_left() => {
                return Hit::Margin {
                    row: view.len() - 1,
                };
            }
            _ => return Hit::Blank,
        }
        let (column, row, side) = self.point_at(x, y, view);
        if x < self.grid_left() {
            return Hit::Margin { row };
        }
        let shown = view[row];
        Hit::Cell {
            column,
            row,
            side,
            source: shown.source,
            line: shown.line,
            block: shown.block,
        }
    }

    /// View cell (column, view row) under the physical pixel (`x`, `y`), clamped to the
    /// grid, with the half of the cell the pixel falls in. A pixel in a gap maps to the
    /// row below it.
    pub fn point_at(&self, x: f32, y: f32, view: &[ViewRow]) -> (usize, usize, Side) {
        let column_pos = (x - self.grid_left()) / self.cell.width;
        let column = (column_pos.max(0.0) as usize).min(self.columns - 1);
        let row = match self.row_at(y, view) {
            RowAt::Row(row) | RowAt::Gap(row) => row,
        };
        let side = if column_pos - (column as f32) < 0.5 {
            Side::Left
        } else {
            Side::Right
        };
        (column, row, side)
    }

    /// Number of lines between the pixel row `y` and the rows of `view`: negative above,
    /// positive below, 0 inside.
    pub fn vertical_overflow(&self, y: f32, view: &[ViewRow]) -> i32 {
        let (Some(first), Some(last)) = (view.first(), view.last()) else {
            return 0;
        };
        let top = self.row_top(first.top);
        let bottom = self.row_top(last.top + 2);
        if y < top {
            -(((top - y) / self.cell.height).ceil() as i32)
        } else if y >= bottom {
            ((y - bottom) / self.cell.height).floor() as i32 + 1
        } else {
            0
        }
    }

    /// Size information expected by the PTY.
    pub fn window_size(&self) -> WindowSize {
        WindowSize {
            num_lines: self.lines as u16,
            num_cols: self.columns as u16,
            cell_width: self.cell.width.round() as u16,
            cell_height: self.cell.height.round() as u16,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::BlockId;
    use crate::view::RowSource;
    use alacritty_terminal::index::Line;

    const CELL: CellSize = CellSize {
        width: 10.0,
        height: 20.0,
    };

    /// 80 columns and 20 lines with 8 px of padding: 5 px of margin, 5 px of card
    /// padding on each side and 8 px of right reserve (the padding, larger than the
    /// 7 px chevron radius).
    const WIDTH: u32 = 832;
    const HEIGHT: u32 = 416;

    fn layout() -> GridLayout {
        GridLayout::new(Rect::from_size(WIDTH, HEIGHT), CELL, 8.0)
    }

    fn row(top: u32, gap_before: bool, block: Option<u64>) -> ViewRow {
        ViewRow {
            source: RowSource::Live,
            block: block.map(BlockId),
            line: Line(0),
            hidden: 0,
            top,
            gap_before,
        }
    }

    /// Rows of a view without gaps.
    fn plain(rows: u32) -> Vec<ViewRow> {
        (0..rows).map(|i| row(2 * i, false, None)).collect()
    }

    /// Block 0's output on rows 0-1, then block 1 (prompt and output) after a gap, with
    /// one leftover half-line at the top: tops 1, 3, 6, 8.
    fn gapped() -> Vec<ViewRow> {
        vec![
            row(1, false, Some(0)),
            row(3, false, Some(0)),
            row(6, true, Some(1)),
            row(8, false, Some(1)),
        ]
    }

    #[test]
    fn computes_grid_from_pixels() {
        let l = layout();
        assert_eq!((l.columns, l.lines), (80, 20));
        assert_eq!(
            (l.margin, l.card_pad, l.chevron_radius, l.right_reserve),
            (5.0, 5.0, 9.0, 9.0)
        );
    }

    #[test]
    fn margin_card_padding_and_right_reserve_are_always_reserved() {
        // One pixel less loses a column.
        assert_eq!(
            GridLayout::new(Rect::from_size(WIDTH - 1, HEIGHT), CELL, 8.0).columns,
            79
        );
        // Without padding, the chevron radius is reserved on the right: 9 + 5 + 10.
        let l = GridLayout::new(Rect::from_size(824, HEIGHT), CELL, 0.0);
        assert_eq!((l.right_reserve, l.columns), (9.0, 80));
        assert_eq!(
            GridLayout::new(Rect::from_size(823, HEIGHT), CELL, 0.0).columns,
            79
        );
    }

    #[test]
    fn partial_cells_are_dropped() {
        let l = GridLayout::new(Rect::from_size(840, 435), CELL, 8.0);
        assert_eq!((l.columns, l.lines), (80, 20));
    }

    #[test]
    fn never_smaller_than_minimum() {
        let l = GridLayout::new(Rect::from_size(0, 0), CELL, 8.0);
        assert_eq!((l.columns, l.lines), (MIN_COLUMNS, MIN_SCREEN_LINES));
    }

    #[test]
    fn grid_left_includes_padding_margin_and_card_padding() {
        let l = layout();
        assert_eq!(l.grid_left(), 18.0);
        assert_eq!(l.grid_right(), 818.0);
        assert_eq!(l.cell_x(3), 48.0);
    }

    #[test]
    fn row_top_counts_half_lines() {
        let l = layout();
        assert_eq!(l.row_top(0), 8.0);
        assert_eq!(l.row_top(1), 18.0);
        assert_eq!(l.row_top(4), 48.0);
    }

    #[test]
    fn point_at_maps_pixels_to_cells() {
        let l = layout();
        let view = plain(20);
        assert_eq!(l.point_at(18.0, 8.0, &view), (0, 0, Side::Left));
        assert_eq!(l.point_at(48.0, 48.0, &view), (3, 2, Side::Left));
        assert_eq!(l.point_at(57.9, 67.9, &view), (3, 2, Side::Right));
    }

    #[test]
    fn point_at_splits_cells_in_halves() {
        let l = layout();
        let view = plain(20);
        assert_eq!(l.point_at(22.9, 10.0, &view).2, Side::Left);
        assert_eq!(l.point_at(23.0, 10.0, &view).2, Side::Right);
    }

    #[test]
    fn point_at_clamps_to_grid() {
        let l = layout();
        let view = plain(20);
        // The margin and the left padding clamp to the first column.
        assert_eq!(l.point_at(0.0, 0.0, &view), (0, 0, Side::Left));
        assert_eq!(l.point_at(12.0, 8.0, &view), (0, 0, Side::Left));
        assert_eq!(l.point_at(-50.0, -50.0, &view), (0, 0, Side::Left));
        assert_eq!(l.point_at(10_000.0, 10_000.0, &view), (79, 19, Side::Right));
        assert_eq!(l.point_at(815.0, 410.0, &view), (79, 19, Side::Right));
    }

    #[test]
    fn point_at_follows_the_view_rows() {
        let l = layout();
        let view = gapped();
        // The leftover half-line at the top clamps to the first row.
        assert_eq!(l.point_at(18.0, 10.0, &view).1, 0);
        assert_eq!(l.point_at(18.0, 18.0, &view).1, 0);
        assert_eq!(l.point_at(18.0, 37.9, &view).1, 0);
        assert_eq!(l.point_at(18.0, 38.0, &view).1, 1);
        // The gap (pixels 58..68) maps to the row below it.
        assert_eq!(l.point_at(18.0, 60.0, &view).1, 2);
        assert_eq!(l.point_at(18.0, 68.0, &view).1, 2);
        assert_eq!(l.point_at(18.0, 88.0, &view).1, 3);
        assert_eq!(l.point_at(18.0, 10_000.0, &view).1, 3);
    }

    /// The cell hit on view row `row` of `view`.
    fn cell(column: usize, row: usize, side: Side, view: &[ViewRow]) -> Hit {
        Hit::Cell {
            column,
            row,
            side,
            source: view[row].source,
            line: view[row].line,
            block: view[row].block,
        }
    }

    #[test]
    fn hit_tells_the_margin_from_the_grid() {
        let l = layout();
        let view = plain(20);
        assert_eq!(l.hit(0.0, 8.0, &view), Hit::Margin { row: 0 });
        assert_eq!(l.hit(8.0, 48.0, &view), Hit::Margin { row: 2 });
        assert_eq!(l.hit(17.9, 407.9, &view), Hit::Margin { row: 19 });
        assert_eq!(l.hit(18.0, 8.0, &view), cell(0, 0, Side::Left, &view));
        assert_eq!(l.hit(57.9, 67.9, &view), cell(3, 2, Side::Right, &view));
    }

    #[test]
    fn hit_cell_carries_the_row_source_line_and_block() {
        let l = layout();
        let mut view = gapped();
        view[1].source = RowSource::Output(BlockId(0));
        view[1].line = Line(7);
        assert_eq!(
            l.hit(100.0, 40.0, &view),
            Hit::Cell {
                column: 8,
                row: 1,
                side: Side::Left,
                source: RowSource::Output(BlockId(0)),
                line: Line(7),
                block: Some(BlockId(0)),
            }
        );
    }

    #[test]
    fn hit_below_the_last_row_is_blank() {
        let l = layout();
        // The gapped view ends at pixel row 108: blank below, except the margin, which
        // maps to the last row as a margin click selects its block.
        let view = gapped();
        assert_eq!(l.hit(100.0, 107.9, &view), cell(8, 3, Side::Left, &view));
        assert_eq!(l.hit(100.0, 108.0, &view), Hit::Blank);
        assert_eq!(
            l.hit(0.0, 300.0, &view),
            Hit::Margin {
                row: view.len() - 1
            }
        );
        // The bottom padding of a full view.
        assert_eq!(l.hit(100.0, 408.0, &plain(20)), Hit::Blank);
        // An empty view is blank everywhere.
        assert_eq!(l.hit(100.0, 8.0, &[]), Hit::Blank);
    }

    #[test]
    fn hit_on_a_view_with_gaps() {
        let l = layout();
        let view = gapped();
        // Gap between the cards: pixels 58..68.
        assert_eq!(l.hit(100.0, 58.0, &view), Hit::Gap);
        assert_eq!(l.hit(100.0, 67.9, &view), Hit::Gap);
        assert_eq!(l.hit(0.0, 60.0, &view), Hit::Gap);
        assert_eq!(l.hit(5.0, 70.0, &view), Hit::Margin { row: 2 });
        assert_eq!(l.hit(100.0, 68.0, &view), cell(8, 2, Side::Left, &view));
        assert_eq!(l.hit(100.0, 57.9, &view), cell(8, 1, Side::Left, &view));
    }

    #[test]
    fn hit_on_the_chevron_disc() {
        let l = layout();
        let view = gapped();
        // Card corners: x = 818 + 5 = 823; block 0 at y = 18 (first view row), block 1
        // at y = 68. Radius 9.
        assert_eq!(l.hit(823.0, 68.0, &view), Hit::Chevron { row: 2 });
        assert_eq!(l.hit(818.0, 64.0, &view), Hit::Chevron { row: 2 });
        assert_eq!(l.hit(829.0, 70.0, &view), Hit::Chevron { row: 2 });
        assert_eq!(l.hit(823.0, 18.0, &view), Hit::Chevron { row: 0 });
        // Outside the disc.
        assert!(matches!(
            l.hit(833.0, 68.0, &view),
            Hit::Cell { row: 2, .. }
        ));
        assert!(matches!(
            l.hit(815.0, 76.0, &view),
            Hit::Cell { row: 2, .. }
        ));
        // No chevron on rows outside blocks, nor inside a card.
        assert!(matches!(
            l.hit(823.0, 8.0, &plain(20)),
            Hit::Cell { row: 0, .. }
        ));
        assert!(matches!(
            l.hit(823.0, 38.0, &view),
            Hit::Cell { row: 1, .. }
        ));
    }

    #[test]
    fn vertical_overflow_counts_lines_outside_the_view() {
        let l = layout();
        let view = plain(20);
        // View spans pixel rows 8..408.
        assert_eq!(l.vertical_overflow(8.0, &view), 0);
        assert_eq!(l.vertical_overflow(407.9, &view), 0);
        assert_eq!(l.vertical_overflow(7.9, &view), -1);
        assert_eq!(l.vertical_overflow(-12.0, &view), -1);
        assert_eq!(l.vertical_overflow(-12.1, &view), -2);
        assert_eq!(l.vertical_overflow(408.0, &view), 1);
        assert_eq!(l.vertical_overflow(427.9, &view), 1);
        assert_eq!(l.vertical_overflow(428.0, &view), 2);
        // With a leftover half-line at the top: the view spans 18..108.
        let view = gapped();
        assert_eq!(l.vertical_overflow(17.9, &view), -1);
        assert_eq!(l.vertical_overflow(18.0, &view), 0);
        assert_eq!(l.vertical_overflow(107.9, &view), 0);
        assert_eq!(l.vertical_overflow(108.0, &view), 1);
    }

    /// The layout of `layout()` moved to a rect at (300, 200) of a larger window.
    fn offset() -> GridLayout {
        GridLayout::new(
            Rect {
                x: 300.0,
                y: 200.0,
                width: WIDTH as f32,
                height: HEIGHT as f32,
            },
            CELL,
            8.0,
        )
    }

    #[test]
    fn full_window_rect_keeps_the_window_origin() {
        let l = layout();
        assert_eq!(l.rect, Rect::from_size(WIDTH, HEIGHT));
        assert_eq!((l.content_left(), l.content_top()), (8.0, 8.0));
    }

    #[test]
    fn offset_rect_moves_the_grid_origin() {
        let l = offset();
        // Same cell count: only the origin moves.
        assert_eq!((l.columns, l.lines), (80, 20));
        assert_eq!((l.content_left(), l.content_top()), (308.0, 208.0));
        assert_eq!(l.grid_left(), 318.0);
        assert_eq!(l.grid_right(), 1118.0);
        assert_eq!(l.cell_x(3), 348.0);
        assert_eq!(l.row_top(4), 248.0);
    }

    #[test]
    fn offset_rect_moves_point_at_hit_and_overflow() {
        let l = offset();
        let view = plain(20);
        assert_eq!(l.point_at(318.0, 208.0, &view), (0, 0, Side::Left));
        assert_eq!(l.point_at(357.9, 267.9, &view), (3, 2, Side::Right));
        // Left of the rect clamps to the first cell.
        assert_eq!(l.point_at(0.0, 0.0, &view), (0, 0, Side::Left));
        assert_eq!(l.hit(300.0, 248.0, &view), Hit::Margin { row: 2 });
        assert_eq!(l.hit(318.0, 208.0, &view), cell(0, 0, Side::Left, &view));
        assert_eq!(l.vertical_overflow(207.9, &view), -1);
        assert_eq!(l.vertical_overflow(208.0, &view), 0);
        assert_eq!(l.vertical_overflow(607.9, &view), 0);
        assert_eq!(l.vertical_overflow(608.0, &view), 1);
    }

    #[test]
    fn point_at_is_the_inverse_of_the_cell_origin() {
        let view = plain(20);
        for l in [layout(), offset()] {
            for column in [0, 1, 40, 79] {
                for row in [0, 7, 19] {
                    let (x, y) = (l.cell_x(column), l.row_top(view[row].top));
                    assert_eq!(l.point_at(x, y, &view), (column, row, Side::Left));
                    let (x, y) = (x + CELL.width - 0.1, y + CELL.height - 0.1);
                    assert_eq!(l.point_at(x, y, &view), (column, row, Side::Right));
                }
            }
        }
    }

    #[test]
    fn contains_excludes_the_right_and_bottom_edges() {
        let l = offset();
        assert!(l.contains(300.0, 200.0));
        assert!(l.contains(1131.9, 615.9));
        assert!(!l.contains(299.9, 300.0));
        assert!(!l.contains(400.0, 199.9));
        assert!(!l.contains(1132.0, 300.0));
        assert!(!l.contains(400.0, 616.0));
    }

    #[test]
    fn window_size_matches_grid() {
        let ws = layout().window_size();
        assert_eq!(
            (ws.num_cols, ws.num_lines, ws.cell_width, ws.cell_height),
            (80, 20, 10, 20)
        );
    }
}
