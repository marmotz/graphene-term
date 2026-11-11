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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLayout {
    pub cell: CellSize,
    /// Inner padding around the grid, in physical pixels.
    pub padding: f32,
    pub columns: usize,
    pub lines: usize,
}

impl GridLayout {
    /// Computes how many cells fit in a surface of the given physical size.
    pub fn new(width: u32, height: u32, cell: CellSize, padding: f32) -> Self {
        let fit = |available: f32, size: f32| ((available - 2.0 * padding).max(0.0) / size).floor() as usize;
        Self {
            cell,
            padding,
            columns: fit(width as f32, cell.width).max(MIN_COLUMNS),
            lines: fit(height as f32, cell.height).max(MIN_SCREEN_LINES),
        }
    }

    /// Top-left pixel position of the cell at (`column`, `line`).
    pub fn cell_origin(&self, column: usize, line: usize) -> (f32, f32) {
        (
            self.padding + column as f32 * self.cell.width,
            self.padding + line as f32 * self.cell.height,
        )
    }

    /// Viewport cell under the physical pixel (`x`, `y`), clamped to the grid, with the half
    /// of the cell the pixel falls in. The inverse of [`Self::cell_origin`].
    pub fn point_at(&self, x: f32, y: f32) -> (usize, usize, Side) {
        let column_pos = (x - self.padding) / self.cell.width;
        let line_pos = (y - self.padding) / self.cell.height;
        let column = (column_pos.max(0.0) as usize).min(self.columns - 1);
        let line = (line_pos.max(0.0) as usize).min(self.lines - 1);
        let side = if column_pos - (column as f32) < 0.5 { Side::Left } else { Side::Right };
        (column, line, side)
    }

    /// Number of lines between the pixel row `y` and the grid: negative above, positive
    /// below, 0 inside.
    #[cfg_attr(not(test), allow(dead_code))]
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

    const CELL: CellSize = CellSize { width: 10.0, height: 20.0 };

    #[test]
    fn computes_grid_from_pixels() {
        let l = GridLayout::new(816, 416, CELL, 8.0);
        assert_eq!((l.columns, l.lines), (80, 20));
    }

    #[test]
    fn partial_cells_are_dropped() {
        let l = GridLayout::new(825, 435, CELL, 8.0);
        assert_eq!((l.columns, l.lines), (80, 20));
    }

    #[test]
    fn never_smaller_than_minimum() {
        let l = GridLayout::new(0, 0, CELL, 8.0);
        assert_eq!((l.columns, l.lines), (MIN_COLUMNS, MIN_SCREEN_LINES));
    }

    #[test]
    fn cell_origin_includes_padding() {
        let l = GridLayout::new(816, 416, CELL, 8.0);
        assert_eq!(l.cell_origin(0, 0), (8.0, 8.0));
        assert_eq!(l.cell_origin(3, 2), (38.0, 48.0));
    }

    #[test]
    fn point_at_maps_pixels_to_cells() {
        let l = GridLayout::new(816, 416, CELL, 8.0);
        assert_eq!(l.point_at(8.0, 8.0), (0, 0, Side::Left));
        assert_eq!(l.point_at(38.0, 48.0), (3, 2, Side::Left));
        assert_eq!(l.point_at(47.9, 67.9), (3, 2, Side::Right));
    }

    #[test]
    fn point_at_splits_cells_in_halves() {
        let l = GridLayout::new(816, 416, CELL, 8.0);
        assert_eq!(l.point_at(12.9, 10.0).2, Side::Left);
        assert_eq!(l.point_at(13.0, 10.0).2, Side::Right);
    }

    #[test]
    fn point_at_clamps_to_grid() {
        let l = GridLayout::new(816, 416, CELL, 8.0);
        assert_eq!(l.point_at(0.0, 0.0), (0, 0, Side::Left));
        assert_eq!(l.point_at(-50.0, -50.0), (0, 0, Side::Left));
        assert_eq!(l.point_at(10_000.0, 10_000.0), (79, 19, Side::Right));
        assert_eq!(l.point_at(810.0, 410.0), (79, 19, Side::Right));
    }

    #[test]
    fn vertical_overflow_counts_lines_outside_grid() {
        let l = GridLayout::new(816, 416, CELL, 8.0);
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
        let ws = GridLayout::new(816, 416, CELL, 8.0).window_size();
        assert_eq!(
            (ws.num_cols, ws.num_lines, ws.cell_width, ws.cell_height),
            (80, 20, 10, 20)
        );
    }
}
