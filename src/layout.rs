//! Geometry: conversion between pixel sizes and terminal grid dimensions.

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::index::Side;
use alacritty_terminal::term::{MIN_COLUMNS, MIN_SCREEN_LINES};

/// Size of a single character cell, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellSize {
    pub width: f32,
    pub height: f32,
}

/// Placement of the terminal grid inside the window.
///
/// The window holds, from the left: the padding, the gutter (command block marks,
/// always reserved so a block never reflows the grid), then the grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLayout {
    pub cell: CellSize,
    /// Inner padding around the gutter and the grid, in physical pixels.
    pub padding: f32,
    /// Width of the gutter left of the grid, in physical pixels: one cell.
    pub gutter: f32,
    pub columns: usize,
    pub lines: usize,
}

/// What lies under a pixel of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// Left of the grid (gutter or left padding), on viewport row `line`.
    Gutter { line: usize },
    /// A grid cell, with the half of the cell the pixel falls in.
    Cell {
        column: usize,
        line: usize,
        side: Side,
    },
}

impl GridLayout {
    /// Computes how many cells fit in a surface of the given physical size.
    pub fn new(width: u32, height: u32, cell: CellSize, padding: f32) -> Self {
        let fit = |available: f32, size: f32| {
            ((available - 2.0 * padding).max(0.0) / size).floor() as usize
        };
        let gutter = cell.width;
        Self {
            cell,
            padding,
            gutter,
            columns: fit(width as f32 - gutter, cell.width).max(MIN_COLUMNS),
            lines: fit(height as f32, cell.height).max(MIN_SCREEN_LINES),
        }
    }

    /// Top-left pixel position of the cell at (`column`, `line`).
    pub fn cell_origin(&self, column: usize, line: usize) -> (f32, f32) {
        (
            self.grid_left() + column as f32 * self.cell.width,
            self.padding + line as f32 * self.cell.height,
        )
    }

    /// Left pixel edge of the grid, right edge of the gutter.
    pub fn grid_left(&self) -> f32 {
        self.padding + self.gutter
    }

    /// What lies under the physical pixel (`x`, `y`): the gutter left of the grid, or a
    /// grid cell. The row is clamped to the grid, the column too right of it.
    pub fn hit(&self, x: f32, y: f32) -> Hit {
        let (column, line, side) = self.point_at(x, y);
        if x < self.grid_left() {
            Hit::Gutter { line }
        } else {
            Hit::Cell { column, line, side }
        }
    }

    /// Viewport cell under the physical pixel (`x`, `y`), clamped to the grid, with the half
    /// of the cell the pixel falls in. The inverse of [`Self::cell_origin`].
    pub fn point_at(&self, x: f32, y: f32) -> (usize, usize, Side) {
        let column_pos = (x - self.grid_left()) / self.cell.width;
        let line_pos = (y - self.padding) / self.cell.height;
        let column = (column_pos.max(0.0) as usize).min(self.columns - 1);
        let line = (line_pos.max(0.0) as usize).min(self.lines - 1);
        let side = if column_pos - (column as f32) < 0.5 {
            Side::Left
        } else {
            Side::Right
        };
        (column, line, side)
    }

    /// Number of lines between the pixel row `y` and the grid: negative above, positive
    /// below, 0 inside.
    pub fn vertical_overflow(&self, y: f32) -> i32 {
        let top = self.padding;
        let bottom = top + self.lines as f32 * self.cell.height;
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

    const CELL: CellSize = CellSize {
        width: 10.0,
        height: 20.0,
    };

    /// 80 columns and 20 lines with 8 px of padding, plus the one-cell gutter.
    const WIDTH: u32 = 826;
    const HEIGHT: u32 = 416;

    fn layout() -> GridLayout {
        GridLayout::new(WIDTH, HEIGHT, CELL, 8.0)
    }

    #[test]
    fn computes_grid_from_pixels() {
        let l = layout();
        assert_eq!((l.columns, l.lines), (80, 20));
    }

    #[test]
    fn gutter_is_one_cell_and_always_reserved() {
        let l = layout();
        assert_eq!(l.gutter, 10.0);
        // Without the gutter, the same width would fit one more column.
        assert_eq!(GridLayout::new(WIDTH - 10, HEIGHT, CELL, 8.0).columns, 79);
    }

    #[test]
    fn partial_cells_are_dropped() {
        let l = GridLayout::new(835, 435, CELL, 8.0);
        assert_eq!((l.columns, l.lines), (80, 20));
    }

    #[test]
    fn never_smaller_than_minimum() {
        let l = GridLayout::new(0, 0, CELL, 8.0);
        assert_eq!((l.columns, l.lines), (MIN_COLUMNS, MIN_SCREEN_LINES));
    }

    #[test]
    fn cell_origin_includes_padding_and_gutter() {
        let l = layout();
        assert_eq!(l.grid_left(), 18.0);
        assert_eq!(l.cell_origin(0, 0), (18.0, 8.0));
        assert_eq!(l.cell_origin(3, 2), (48.0, 48.0));
    }

    #[test]
    fn point_at_maps_pixels_to_cells() {
        let l = layout();
        assert_eq!(l.point_at(18.0, 8.0), (0, 0, Side::Left));
        assert_eq!(l.point_at(48.0, 48.0), (3, 2, Side::Left));
        assert_eq!(l.point_at(57.9, 67.9), (3, 2, Side::Right));
    }

    #[test]
    fn point_at_splits_cells_in_halves() {
        let l = layout();
        assert_eq!(l.point_at(22.9, 10.0).2, Side::Left);
        assert_eq!(l.point_at(23.0, 10.0).2, Side::Right);
    }

    #[test]
    fn point_at_clamps_to_grid() {
        let l = layout();
        // The gutter and the left padding clamp to the first column.
        assert_eq!(l.point_at(0.0, 0.0), (0, 0, Side::Left));
        assert_eq!(l.point_at(12.0, 8.0), (0, 0, Side::Left));
        assert_eq!(l.point_at(-50.0, -50.0), (0, 0, Side::Left));
        assert_eq!(l.point_at(10_000.0, 10_000.0), (79, 19, Side::Right));
        assert_eq!(l.point_at(820.0, 410.0), (79, 19, Side::Right));
    }

    #[test]
    fn hit_tells_the_gutter_from_the_grid() {
        let l = layout();
        assert_eq!(l.hit(0.0, 8.0), Hit::Gutter { line: 0 });
        assert_eq!(l.hit(8.0, 48.0), Hit::Gutter { line: 2 });
        assert_eq!(l.hit(17.9, 10_000.0), Hit::Gutter { line: 19 });
        assert_eq!(
            l.hit(18.0, 8.0),
            Hit::Cell {
                column: 0,
                line: 0,
                side: Side::Left
            }
        );
        assert_eq!(
            l.hit(57.9, 67.9),
            Hit::Cell {
                column: 3,
                line: 2,
                side: Side::Right
            }
        );
    }

    #[test]
    fn vertical_overflow_counts_lines_outside_grid() {
        let l = layout();
        // Grid spans pixel rows 8..408.
        assert_eq!(l.vertical_overflow(8.0), 0);
        assert_eq!(l.vertical_overflow(407.9), 0);
        assert_eq!(l.vertical_overflow(7.9), -1);
        assert_eq!(l.vertical_overflow(-12.0), -1);
        assert_eq!(l.vertical_overflow(-12.1), -2);
        assert_eq!(l.vertical_overflow(408.0), 1);
        assert_eq!(l.vertical_overflow(427.9), 1);
        assert_eq!(l.vertical_overflow(428.0), 2);
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
