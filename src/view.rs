//! Rows shown in the window: which grid line each viewport row displays, and its role
//! in the command blocks.
//!
//! The view follows alacritty's `display_offset`: its bottom row is the bottom grid line
//! of alacritty's viewport. Folding is a row filter over it: the output rows of folded
//! blocks are skipped while the rows are collected upward from that line, and scroll
//! deltas in visible rows are converted to grid lines ([`grid_delta`]). Without folds,
//! the view is exactly alacritty's viewport.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::{Term, TermMode};

use crate::blocks::{self, BlockId, RowRole, ShellState};

/// One viewport row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewRow {
    /// Grid line shown by the row (history lines are negative); `None` for a blank row
    /// padding the view below the content, when folds hide more rows than the history
    /// can replace.
    pub line: Option<Line>,
    pub role: RowRole,
    /// Output rows of a folded block hidden right below this row.
    pub hidden: usize,
}

impl ViewRow {
    fn shown(line: Line, role: RowRole) -> Self {
        Self {
            line: Some(line),
            role,
            hidden: 0,
        }
    }
}

/// Bottom grid line of alacritty's viewport scrolled `display_offset` lines into the
/// history.
fn bottom_line<L>(term: &Term<L>, display_offset: usize) -> Line {
    Line(term.screen_lines() as i32 - 1 - display_offset as i32)
}

/// Whether rows get block roles: not on the alternate screen, and only once a block is
/// known (shell integration active).
fn decorated<L>(term: &Term<L>, shell: &ShellState) -> bool {
    !term.mode().contains(TermMode::ALT_SCREEN) && shell.blocks().next().is_some()
}

/// Which rows the folds hide: the output rows of folded blocks above the cursor. The
/// cursor row stays visible, so does anything the shell draws next.
struct Folds<'a> {
    shell: &'a ShellState,
    cursor: Line,
}

impl<'a> Folds<'a> {
    /// `None` when no row is hidden: no fold, alternate screen, or no block.
    fn new<L>(term: &Term<L>, shell: &'a ShellState) -> Option<Self> {
        (decorated(term, shell) && shell.any_folded()).then(|| Self {
            shell,
            cursor: term.grid().cursor.point.line,
        })
    }

    fn hides(&self, line: Line, role: RowRole) -> bool {
        matches!(role, RowRole::Output(id) if line < self.cursor && self.shell.is_folded(id))
    }
}

/// Visits the rows from `from` up to the top of the history with their role, bottom to
/// top, until `visit` returns `false`. An untagged row waits for the next tagged row
/// above it to know its block.
fn walk_up<L>(term: &Term<L>, from: Line, mut visit: impl FnMut(Line, RowRole) -> bool) {
    let top = term.grid().topmost_line();
    let mut pending = Vec::new();
    let mut line = from;
    while line >= top {
        match blocks::head_role(term, line) {
            None => pending.push(line),
            Some(role) => {
                let output = role.block().map_or(RowRole::None, RowRole::Output);
                for row in pending.drain(..) {
                    if !visit(row, output) {
                        return;
                    }
                }
                if !visit(line, role) {
                    return;
                }
            }
        }
        line -= 1;
    }
    // Rows before the first block left in the grid.
    for row in pending {
        if !visit(row, RowRole::None) {
            return;
        }
    }
}

/// Role of the rows below a row of `block` until the next tagged row, as in
/// [`blocks::index`].
fn role_below<L>(term: &Term<L>, line: Line, block: &mut Option<BlockId>) -> RowRole {
    match blocks::head_role(term, line) {
        Some(role) => {
            *block = role.block();
            role
        }
        None => block.map_or(RowRole::None, RowRole::Output),
    }
}

/// Rows of the view, top to bottom, for the view scrolled `display_offset` lines into
/// the history. Every row has [`RowRole::None`] on the alternate screen, or when no
/// block is known (no shell integration): no row index is built then.
pub fn rows<L>(term: &Term<L>, shell: &ShellState, display_offset: usize) -> Vec<ViewRow> {
    let screen_lines = term.screen_lines();
    let bottom = bottom_line(term, display_offset);
    let lines = bottom - (screen_lines as i32 - 1)..bottom + 1;
    if let Some(folds) = Folds::new(term, shell) {
        return folded_rows(term, &folds, bottom, screen_lines);
    }
    let roles = if decorated(term, shell) {
        blocks::index(term, lines.clone())
    } else {
        vec![RowRole::None; screen_lines]
    };
    (lines.start.0..lines.end.0)
        .zip(roles)
        .map(|(line, role)| ViewRow::shown(Line(line), role))
        .collect()
}

/// Rows of the view with folds: collected upward from `bottom`, skipping hidden rows; at
/// the top of the history, completed downward from `bottom`, then padded with blank
/// rows. The rows hidden below `bottom` are still counted on the row above them.
fn folded_rows<L>(
    term: &Term<L>,
    folds: &Folds<'_>,
    bottom: Line,
    screen_lines: usize,
) -> Vec<ViewRow> {
    let mut up: Vec<ViewRow> = Vec::with_capacity(screen_lines);
    let mut hidden = 0;
    let mut bottom_role = RowRole::None;
    walk_up(term, bottom, |line, role| {
        if line == bottom {
            bottom_role = role;
        }
        if folds.hides(line, role) {
            hidden += 1;
            return true;
        }
        up.push(ViewRow {
            hidden: std::mem::take(&mut hidden),
            ..ViewRow::shown(line, role)
        });
        up.len() < screen_lines
    });
    let mut rows: Vec<ViewRow> = up.into_iter().rev().collect();

    // Below `bottom`: the rest of a hidden run, then the rows completing a view that
    // reached the top of the history.
    let mut block = bottom_role.block();
    let mut line = bottom + 1;
    while line <= term.grid().bottommost_line() {
        let role = role_below(term, line, &mut block);
        if folds.hides(line, role) {
            if let Some(last) = rows.last_mut() {
                last.hidden += 1;
            }
        } else if rows.len() < screen_lines {
            rows.push(ViewRow::shown(line, role));
        } else {
            break;
        }
        line += 1;
    }

    rows.resize(
        screen_lines.max(rows.len()),
        ViewRow {
            line: None,
            role: RowRole::None,
            hidden: 0,
        },
    );
    rows
}

/// `Scroll::Delta` lines (positive into the history) for a scroll of `view_delta`
/// visible rows from the view scrolled `display_offset` lines into the history. The
/// hidden rows crossed on the way are added. Without folds, `view_delta` itself.
pub fn grid_delta<L>(
    term: &Term<L>,
    shell: &ShellState,
    display_offset: usize,
    view_delta: i32,
) -> i32 {
    let Some(folds) = Folds::new(term, shell).filter(|_| view_delta != 0) else {
        return view_delta;
    };
    let bottom = bottom_line(term, display_offset);
    if view_delta > 0 {
        // The new bottom row is the `view_delta`-th visible row above the current one.
        let mut seen = -1;
        let mut last = bottom;
        walk_up(term, bottom, |line, role| {
            if folds.hides(line, role) {
                return true;
            }
            seen += 1;
            last = line;
            seen < view_delta
        });
        // At the top of the history, the rest is clamped by alacritty.
        bottom.0 - last.0 + (view_delta - seen.max(0))
    } else {
        // The new bottom row is the `-view_delta`-th visible row below the current one.
        let mut block = blocks::index(term, bottom..bottom + 1)[0].block();
        let mut seen = 0;
        let mut line = bottom + 1;
        while line <= term.grid().bottommost_line() {
            let role = role_below(term, line, &mut block);
            if !folds.hides(line, role) {
                seen += 1;
                if seen == -view_delta {
                    return bottom.0 - line.0;
                }
            }
            line += 1;
        }
        -(display_offset as i32)
    }
}

/// `Scroll::Delta` lines (positive into the history) that bring grid `line` into the
/// view scrolled `display_offset` lines into the history: 0 when it is shown (or
/// hidden by a fold), otherwise the smallest scroll putting it on the top or bottom row.
pub fn reveal<L>(term: &Term<L>, shell: &ShellState, display_offset: usize, line: Line) -> i32 {
    let bottom = bottom_line(term, display_offset);
    if line > bottom {
        return bottom.0 - line.0;
    }
    let view = rows(term, shell, display_offset);
    let Some(top) = view.iter().find_map(|row| row.line) else {
        return 0;
    };
    if line >= top {
        return 0;
    }
    // Up: by the visible rows from `line` to the top row.
    let shown = match Folds::new(term, shell) {
        Some(folds) => blocks::index(term, line..top)
            .into_iter()
            .zip(line.0..top.0)
            .filter(|(role, row)| !folds.hides(Line(*row), *role))
            .count(),
        None => (top.0 - line.0) as usize,
    };
    grid_delta(term, shell, display_offset, shown as i32)
}

/// Grid point of the cell at `column` on view row `line_index`, clamped to the last row
/// showing a grid line.
pub fn point(rows: &[ViewRow], line_index: usize, column: usize) -> Point {
    let index = line_index.min(rows.len() - 1);
    let line = rows[..=index]
        .iter()
        .rev()
        .chain(&rows[index + 1..])
        .find_map(|row| row.line)
        .unwrap_or(Line(0));
    Point::new(line, Column(column))
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

    const A: &str = "\x1b]133;A\x07";
    const B: &str = "\x1b]133;B\x07";
    const C: &str = "\x1b]133;C\x07";

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

    /// One full command with `output_rows` numbered output rows, ended with status 0.
    fn command(term: &mut Term<VoidListener>, shell: &mut ShellState, name: &str, rows: usize) {
        let output: String = (1..=rows).map(|row| format!("{name}{row}\r\n")).collect();
        feed(
            term,
            shell,
            &format!("{A}$ {B}{name}\r\n{C}{output}\x1b]133;D;0\x07"),
        );
    }

    /// Text of the first cells of each row, `~` for a padding row.
    fn texts(term: &Term<VoidListener>, rows: &[ViewRow]) -> Vec<String> {
        rows.iter()
            .map(|row| match row.line {
                Some(line) => term.grid()[line][..]
                    .iter()
                    .map(|cell| cell.c)
                    .collect::<String>()
                    .trim_end()
                    .to_owned(),
                None => "~".to_owned(),
            })
            .collect()
    }

    fn lines(rows: &[ViewRow]) -> Vec<i32> {
        rows.iter()
            .map(|row| row.line.expect("grid line").0)
            .collect()
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
            &format!("{A}$ {B}ls\r\n{C}out\r\n\x1b]133;D;0\x07"),
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
        feed(&mut term, &mut shell, &format!("{A}$ {B}vim\r\n"));
        feed(&mut term, &mut shell, &format!("{C}\x1b[?1049h"));
        assert!(
            rows(&term, &shell, 0)
                .iter()
                .all(|row| row.role == RowRole::None)
        );
    }

    /// Three blocks `a` (3 rows), `b` (2 rows) and `c` (3 rows) in a 4-row terminal,
    /// then a prompt: grid lines -8 to 3.
    fn three_blocks() -> (Term<VoidListener>, ShellState) {
        let mut term = new_term(4);
        let mut shell = ShellState::new();
        command(&mut term, &mut shell, "a", 3);
        command(&mut term, &mut shell, "b", 2);
        command(&mut term, &mut shell, "c", 3);
        feed(&mut term, &mut shell, &format!("{A}$ "));
        (term, shell)
    }

    #[test]
    fn three_blocks_layout() {
        let (term, shell) = three_blocks();
        let all = blocks::index(&term, Line(-8)..Line(4));
        assert_eq!(all[0], RowRole::Prompt(BlockId(0)));
        assert_eq!(all[4], RowRole::Prompt(BlockId(1)));
        assert_eq!(all[7], RowRole::Prompt(BlockId(2)));
        assert_eq!(all[11], RowRole::Prompt(BlockId(3)));
        assert_eq!(term.grid().cursor.point.line, Line(3));
        assert!(!shell.any_folded());
    }

    #[test]
    fn no_fold_keeps_the_plain_viewport() {
        let (term, shell) = three_blocks();
        assert_eq!(lines(&rows(&term, &shell, 0)), [0, 1, 2, 3]);
        assert_eq!(lines(&rows(&term, &shell, 5)), [-5, -4, -3, -2]);
    }

    #[test]
    fn one_fold_skips_its_output_and_counts_it() {
        let (term, mut shell) = three_blocks();
        shell.set_folded(BlockId(2), true);
        let rows = rows(&term, &shell, 0);
        assert_eq!(texts(&term, &rows), ["b1", "b2", "$ c", "$"]);
        assert_eq!(rows[2].hidden, 3);
        assert!(
            rows.iter()
                .enumerate()
                .all(|(i, r)| i == 2 || r.hidden == 0)
        );
    }

    #[test]
    fn several_folds() {
        let (term, mut shell) = three_blocks();
        shell.set_folded(BlockId(1), true);
        shell.set_folded(BlockId(2), true);
        let rows = rows(&term, &shell, 0);
        assert_eq!(texts(&term, &rows), ["a3", "$ b", "$ c", "$"]);
        assert_eq!(
            rows.iter().map(|r| r.hidden).collect::<Vec<_>>(),
            [0, 2, 3, 0]
        );
    }

    #[test]
    fn top_of_history_completes_downward_then_pads() {
        let (term, mut shell) = three_blocks();
        for id in 0..3 {
            shell.set_folded(BlockId(id), true);
        }
        // All blocks folded: four visible rows in the whole grid.
        let rows = rows(&term, &shell, 0);
        assert_eq!(texts(&term, &rows), ["$ a", "$ b", "$ c", "$"]);

        // Scrolled to the top of the history: the bottom grid line is hidden, the rows
        // below it complete the view.
        let rows = super::rows(&term, &shell, 8);
        assert_eq!(texts(&term, &rows), ["$ a", "$ b", "$ c", "$"]);
        assert_eq!(rows[0].hidden, 3);
        assert_eq!(rows[1].hidden, 2);
    }

    #[test]
    fn padding_rows_below_the_content() {
        let mut term = new_term(4);
        let mut shell = ShellState::new();
        command(&mut term, &mut shell, "a", 6);
        feed(&mut term, &mut shell, &format!("{A}$ "));
        shell.set_folded(BlockId(0), true);
        let rows = rows(&term, &shell, 0);
        assert_eq!(texts(&term, &rows), ["$ a", "$", "~", "~"]);
        assert_eq!(rows[0].hidden, 6);
        assert_eq!(rows[2].role, RowRole::None);
    }

    #[test]
    fn rows_hidden_below_the_bottom_line_are_counted() {
        let (term, mut shell) = three_blocks();
        shell.set_folded(BlockId(0), true);
        // Bottom grid line -5 is "a3", hidden: the view ends on "$ a", whose count
        // includes the hidden rows below the bottom line.
        let rows = rows(&term, &shell, 5);
        assert_eq!(texts(&term, &rows), ["$ a", "$ b", "b1", "b2"]);
        assert_eq!(rows[0].hidden, 3);
    }

    #[test]
    fn the_cursor_row_of_a_folded_block_stays_visible() {
        let mut term = new_term(4);
        let mut shell = ShellState::new();
        command(&mut term, &mut shell, "a", 2);
        feed(&mut term, &mut shell, "tail");
        shell.set_folded(BlockId(0), true);
        let rows = rows(&term, &shell, 0);
        // Grid lines 0 to 3: nothing above or below to fill the view.
        assert_eq!(texts(&term, &rows), ["$ a", "tail", "~", "~"]);
        assert_eq!(rows[0].hidden, 2);
    }

    #[test]
    fn grid_delta_without_folds_is_the_view_delta() {
        let (term, shell) = three_blocks();
        assert_eq!(grid_delta(&term, &shell, 0, 3), 3);
        assert_eq!(grid_delta(&term, &shell, 5, -2), -2);
    }

    #[test]
    fn grid_delta_across_a_fold() {
        let (term, mut shell) = three_blocks();
        shell.set_folded(BlockId(1), true);
        // View rows from the bottom: "$" (3), "c3".."c1", "$ c" (-1), "$ b" (-4, its two
        // output rows hidden), "a3" (-5).
        assert_eq!(grid_delta(&term, &shell, 0, 4), 4);
        assert_eq!(grid_delta(&term, &shell, 0, 5), 7);
        assert_eq!(grid_delta(&term, &shell, 0, 6), 8);
        // Back down from bottom line -5: "$ b", then "$ c".
        assert_eq!(grid_delta(&term, &shell, 8, -1), -1);
        assert_eq!(grid_delta(&term, &shell, 8, -2), -4);
        // Past the bottom of the grid: to the bottom.
        assert_eq!(grid_delta(&term, &shell, 8, -20), -8);
        // Past the top: the rest is left to alacritty's clamp.
        assert_eq!(grid_delta(&term, &shell, 0, 20), 22);
    }

    #[test]
    fn reveal_scrolls_the_least_to_show_a_line() {
        let (term, shell) = three_blocks();
        // Visible: no scroll.
        assert_eq!(reveal(&term, &shell, 0, Line(0)), 0);
        assert_eq!(reveal(&term, &shell, 0, Line(3)), 0);
        // Above the view: up, the line on the top row.
        assert_eq!(reveal(&term, &shell, 0, Line(-4)), 4);
        assert_eq!(reveal(&term, &shell, 2, Line(-4)), 2);
        // Below the view: down, the line on the bottom row.
        assert_eq!(reveal(&term, &shell, 5, Line(0)), -2);
    }

    #[test]
    fn reveal_counts_visible_rows_across_a_fold() {
        let (term, mut shell) = three_blocks();
        shell.set_folded(BlockId(1), true);
        // View: "c1", "c2", "c3", "$". Block 1's prompt (-4) is two visible rows above
        // the top row ("$ c", then "$ b").
        assert_eq!(lines(&rows(&term, &shell, 0)), [0, 1, 2, 3]);
        let delta = reveal(&term, &shell, 0, Line(-4));
        assert_eq!(delta, grid_delta(&term, &shell, 0, 2));
        assert_eq!(texts(&term, &rows(&term, &shell, delta as usize))[0], "$ b");
    }

    #[test]
    fn point_maps_view_rows_to_grid_lines() {
        let mut rows: Vec<ViewRow> = [Line(-2), Line(-1), Line(0)]
            .map(|line| ViewRow::shown(line, RowRole::None))
            .to_vec();
        assert_eq!(point(&rows, 0, 3), Point::new(Line(-2), Column(3)));
        assert_eq!(point(&rows, 2, 0), Point::new(Line(0), Column(0)));
        // Clamped to the last row.
        assert_eq!(point(&rows, 9, 1), Point::new(Line(0), Column(1)));
        // A padding row maps to the last row showing a grid line.
        rows.push(ViewRow {
            line: None,
            role: RowRole::None,
            hidden: 0,
        });
        assert_eq!(point(&rows, 3, 2), Point::new(Line(0), Column(2)));
    }
}
