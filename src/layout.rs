//! Geometry: conversion between pixel sizes and terminal grid dimensions.

use alacritty_terminal::event::WindowSize;
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
    fn window_size_matches_grid() {
        let ws = GridLayout::new(816, 416, CELL, 8.0).window_size();
        assert_eq!((ws.num_cols, ws.num_lines, ws.cell_width, ws.cell_height), (80, 20, 10, 20));
    }
}
