//! Rows shown in the window: which grid line each viewport row displays, and its role
//! in the command blocks.
//!
//! The view follows alacritty's `display_offset`. Without folding (yet), it is exactly
//! alacritty's viewport, with the role of each row attached.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::{Term, TermMode};

use crate::blocks::{self, RowRole, ShellState};

/// One viewport row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewRow {
    /// Grid line shown by the row (history lines are negative).
    pub line: Line,
    pub role: RowRole,
}

/// Rows of the viewport, top to bottom, for the view scrolled `display_offset` lines
/// into the history. Every row has [`RowRole::None`] on the alternate screen, or when
/// no block is known (no shell integration): no row index is built then.
pub fn rows<L>(term: &Term<L>, shell: &ShellState, display_offset: usize) -> Vec<ViewRow> {
    let screen_lines = term.screen_lines() as i32;
    let top = -(display_offset as i32);
    let lines = Line(top)..Line(top + screen_lines);
    let decorated = !term.mode().contains(TermMode::ALT_SCREEN) && shell.blocks().next().is_some();
    let roles = if decorated {
        blocks::index(term, lines.clone())
    } else {
        vec![RowRole::None; screen_lines as usize]
    };
    (lines.start.0..lines.end.0)
        .zip(roles)
        .map(|(line, role)| ViewRow {
            line: Line(line),
            role,
        })
        .collect()
}

/// Grid point of the cell at `column` on viewport row `line_index`, clamped to the last
/// row.
pub fn point(rows: &[ViewRow], line_index: usize, column: usize) -> Point {
    let row = rows[line_index.min(rows.len() - 1)];
    Point::new(row.line, Column(column))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc133::Scanner;
    use crate::pty;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::grid::Scroll;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::Processor;

    use crate::blocks::BlockId;

    fn feed(term: &mut Term<VoidListener>, shell: &mut ShellState, bytes: &str) {
        pty::process(
            &mut Scanner::new(),
            &mut Processor::new(),
            shell,
            term,
            bytes.as_bytes(),
        );
    }

    fn new_term(lines: usize) -> Term<VoidListener> {
        Term::new(Config::default(), &TermSize::new(10, lines), VoidListener)
    }

    fn lines(rows: &[ViewRow]) -> Vec<i32> {
        rows.iter().map(|row| row.line.0).collect()
    }

    #[test]
    fn plain_viewport_without_blocks() {
        let mut term = new_term(3);
        let mut shell = ShellState::new();
        feed(&mut term, &mut shell, "1\r\n2\r\n3\r\n4\r\n5");
        let rows = rows(&term, &shell, 0);
        assert_eq!(lines(&rows), [0, 1, 2]);
        assert!(rows.iter().all(|row| row.role == RowRole::None));

        term.scroll_display(Scroll::Delta(2));
        let offset = term.grid().display_offset();
        assert_eq!(lines(&super::rows(&term, &shell, offset)), [-2, -1, 0]);
    }

    #[test]
    fn rows_carry_block_roles() {
        let mut term = new_term(4);
        let mut shell = ShellState::new();
        feed(
            &mut term,
            &mut shell,
            "\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07out\r\n\x1b]133;D;0\x07",
        );
        let id = BlockId(0);
        let roles: Vec<RowRole> = rows(&term, &shell, 0).iter().map(|r| r.role).collect();
        assert_eq!(
            roles,
            [
                RowRole::Prompt(id),
                RowRole::Output(id),
                RowRole::Output(id),
                RowRole::Output(id)
            ]
        );
    }

    #[test]
    fn alternate_screen_rows_have_no_role() {
        let mut term = new_term(3);
        let mut shell = ShellState::new();
        feed(
            &mut term,
            &mut shell,
            "\x1b]133;A\x07$ \x1b]133;B\x07vim\r\n",
        );
        feed(&mut term, &mut shell, "\x1b]133;C\x07\x1b[?1049h");
        assert!(
            rows(&term, &shell, 0)
                .iter()
                .all(|row| row.role == RowRole::None)
        );
    }

    #[test]
    fn point_maps_viewport_rows_to_grid_lines() {
        let rows = [Line(-2), Line(-1), Line(0)].map(|line| ViewRow {
            line,
            role: RowRole::None,
        });
        assert_eq!(point(&rows, 0, 3), Point::new(Line(-2), Column(3)));
        assert_eq!(point(&rows, 2, 0), Point::new(Line(0), Column(0)));
        // Clamped to the last row.
        assert_eq!(point(&rows, 9, 1), Point::new(Line(0), Column(1)));
    }
}
