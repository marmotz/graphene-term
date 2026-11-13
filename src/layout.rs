//! Geometry: conversion between pixel sizes and terminal grid dimensions.

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::index::Side;
use alacritty_terminal::term::{MIN_COLUMNS, MIN_SCREEN_LINES};

use crate::view::ViewRow;

/// Size of a single character cell, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellSize {
    pub width: f32,
    pub height: f32,
}

/// Placement of the terminal grid inside the window.
///
/// The window holds, from the left: the padding, the margin (status stripe of the
/// command block cards), the card padding, the grid, the card padding again, then the
/// right reserve (room for the fold chevron on the cards' top-right corner). All of it
/// is always reserved, so a block never reflows the grid.
///
/// Vertically, rows are placed by the view in half-lines ([`ViewRow::top`]): a gap of
/// half a line separates two cards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLayout {
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
    /// A grid cell, with the half of the cell the pixel falls in.
    Cell {
        column: usize,
        row: usize,
        side: Side,
    },
}

/// Where a pixel row falls in the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowAt {
    Row(usize),
    Gap(usize),
}

impl GridLayout {
    /// Computes how many cells fit in a surface of the given physical size.
    pub fn new(width: u32, height: u32, cell: CellSize, padding: f32) -> Self {
        let margin = (cell.width / 2.0).round();
        let card_pad = (cell.width / 2.0).round();
        let chevron_radius = (cell.height * 0.35).round();
        let right_reserve = padding.max(chevron_radius);
        let horizontal = width as f32 - padding - margin - 2.0 * card_pad - right_reserve;
        Self {
            cell,
            padding,
            margin,
            card_pad,
            chevron_radius,
            right_reserve,
            columns: ((horizontal.max(0.0) / cell.width).floor() as usize).max(MIN_COLUMNS),
            lines: (((height as f32 - 2.0 * padding).max(0.0) / cell.height).floor() as usize)
                .max(MIN_SCREEN_LINES),
        }
    }

    /// Left pixel edge of the grid.
    pub fn grid_left(&self) -> f32 {
        self.padding + self.margin + self.card_pad
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
        self.padding + units as f32 * self.cell.height / 2.0
    }

    /// View row under the pixel row `y`, clamped to the view, or the gap above a row.
    fn row_at(&self, y: f32, view: &[ViewRow]) -> RowAt {
        let units = (y - self.padding) / (self.cell.height / 2.0);
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
        let Some(block) = view[row].role.block() else {
            return false;
        };
        row == 0 || view[row - 1].role.block() != Some(block)
    }

    /// What lies under the physical pixel (`x`, `y`) of the view `view`: a fold chevron,
    /// the margin left of the grid, a gap between cards, or a grid cell. The row is
    /// clamped to the view, the column too right of the grid.
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
        let (column, row, side) = self.point_at(x, y, view);
        if x < self.grid_left() {
            Hit::Margin { row }
        } else {
            Hit::Cell { column, row, side }
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
    use crate::blocks::{BlockId, RowRole};
    use alacritty_terminal::index::Line;

    const CELL: CellSize = CellSize {
        width: 10.0,
        height: 20.0,
    };

    /// 80 columns and 20 lines with 8 px of padding: 5 px of margin, 5 px of card
    /// padding on each side and 8 px of right reserve (the padding, larger than the
    /// 7 px chevron radius).
    const WIDTH: u32 = 831;
    const HEIGHT: u32 = 416;

    fn layout() -> GridLayout {
        GridLayout::new(WIDTH, HEIGHT, CELL, 8.0)
    }

    fn row(top: u32, gap_before: bool, role: RowRole) -> ViewRow {
        ViewRow {
            line: Some(Line(0)),
            role,
            hidden: 0,
            top,
            gap_before,
        }
    }

    /// Rows of a view without gaps.
    fn plain(rows: u32) -> Vec<ViewRow> {
        (0..rows)
            .map(|i| row(2 * i, false, RowRole::None))
            .collect()
    }

    /// Block 0's output on rows 0-1, then block 1 (prompt and output) after a gap, with
    /// one leftover half-line at the top: tops 1, 3, 6, 8.
    fn gapped() -> Vec<ViewRow> {
        vec![
            row(1, false, RowRole::Output(BlockId(0))),
            row(3, false, RowRole::Output(BlockId(0))),
            row(6, true, RowRole::Prompt(BlockId(1))),
            row(8, false, RowRole::Output(BlockId(1))),
        ]
    }

    #[test]
    fn computes_grid_from_pixels() {
        let l = layout();
        assert_eq!((l.columns, l.lines), (80, 20));
        assert_eq!(
            (l.margin, l.card_pad, l.chevron_radius, l.right_reserve),
            (5.0, 5.0, 7.0, 8.0)
        );
    }

    #[test]
    fn margin_card_padding_and_right_reserve_are_always_reserved() {
        // One pixel less loses a column.
        assert_eq!(GridLayout::new(WIDTH - 1, HEIGHT, CELL, 8.0).columns, 79);
        // Without padding, the chevron radius is reserved on the right: 7 + 5 + 10.
        let l = GridLayout::new(822, HEIGHT, CELL, 0.0);
        assert_eq!((l.right_reserve, l.columns), (7.0, 80));
        assert_eq!(GridLayout::new(821, HEIGHT, CELL, 0.0).columns, 79);
    }

    #[test]
    fn partial_cells_are_dropped() {
        let l = GridLayout::new(840, 435, CELL, 8.0);
        assert_eq!((l.columns, l.lines), (80, 20));
    }

    #[test]
    fn never_smaller_than_minimum() {
        let l = GridLayout::new(0, 0, CELL, 8.0);
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

    #[test]
    fn hit_tells_the_margin_from_the_grid() {
        let l = layout();
        let view = plain(20);
        assert_eq!(l.hit(0.0, 8.0, &view), Hit::Margin { row: 0 });
        assert_eq!(l.hit(8.0, 48.0, &view), Hit::Margin { row: 2 });
        assert_eq!(l.hit(17.9, 10_000.0, &view), Hit::Margin { row: 19 });
        assert_eq!(
            l.hit(18.0, 8.0, &view),
            Hit::Cell {
                column: 0,
                row: 0,
                side: Side::Left
            }
        );
        assert_eq!(
            l.hit(57.9, 67.9, &view),
            Hit::Cell {
                column: 3,
                row: 2,
                side: Side::Right
            }
        );
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
        assert_eq!(
            l.hit(100.0, 68.0, &view),
            Hit::Cell {
                column: 8,
                row: 2,
                side: Side::Left
            }
        );
        assert_eq!(
            l.hit(100.0, 57.9, &view),
            Hit::Cell {
                column: 8,
                row: 1,
                side: Side::Left
            }
        );
    }

    #[test]
    fn hit_on_the_chevron_disc() {
        let l = layout();
        let view = gapped();
        // Card corners: x = 818 + 5 = 823; block 0 at y = 18 (first view row), block 1
        // at y = 68. Radius 7.
        assert_eq!(l.hit(823.0, 68.0, &view), Hit::Chevron { row: 2 });
        assert_eq!(l.hit(818.0, 64.0, &view), Hit::Chevron { row: 2 });
        assert_eq!(l.hit(829.0, 70.0, &view), Hit::Chevron { row: 2 });
        assert_eq!(l.hit(823.0, 18.0, &view), Hit::Chevron { row: 0 });
        // Outside the disc.
        assert!(matches!(
            l.hit(831.0, 68.0, &view),
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

    #[test]
    fn window_size_matches_grid() {
        let ws = layout().window_size();
        assert_eq!(
            (ws.num_cols, ws.num_lines, ws.cell_width, ws.cell_height),
            (80, 20, 10, 20)
        );
    }
}
