//! Rows shown in the window: which grid line each viewport row displays, and its role
//! in the command blocks.
//!
//! The view follows alacritty's `display_offset`: its bottom row is the bottom grid line
//! of alacritty's viewport. Folding is a row filter over it: the output rows of folded
//! blocks are skipped while the rows are collected upward from that line, and scroll
//! deltas in visible rows are converted to grid lines ([`grid_delta`]).
//!
//! The view height is counted in half-lines: a row takes two, and the first row of a
//! block takes one more when a row of another block shows above it (the gap between two
//! cards). Rows are whole, so the view may hold fewer rows than the screen; the leftover
//! half-lines go to the top. Without blocks, or on the alternate screen, there is no gap
//! and the view is exactly alacritty's viewport.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::{Term, TermMode};

use crate::blocks::{self, BlockId, RowRole, ShellState};

// The view over the timeline, used once rendering switches to it (#77).
#[allow(dead_code)]
pub mod timeline;

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
    /// Offset of the row from the view top, in half-lines.
    pub top: u32,
    /// A gap of half a line separates the row from the row above: it starts a block
    /// below a row of another block.
    pub gap_before: bool,
}

impl ViewRow {
    fn shown(line: Line, role: RowRole) -> Self {
        Self {
            line: Some(line),
            role,
            hidden: 0,
            top: 0,
            gap_before: false,
        }
    }

    /// Blank row padding the view below the content.
    fn blank() -> Self {
        Self {
            line: None,
            role: RowRole::None,
            hidden: 0,
            top: 0,
            gap_before: false,
        }
    }
}

/// Half-lines taken by a row.
const ROW_UNITS: u32 = 2;

/// Whether a gap separates a row of role `lower` from the row of role `upper` above it:
/// `lower` starts a block.
fn gap_between(upper: RowRole, lower: RowRole) -> bool {
    lower.block().is_some() && upper.block() != lower.block()
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
/// the history, with their offset from the view top. Every row has [`RowRole::None`] on
/// the alternate screen, or when no block is known (no shell integration): no row index
/// is built then, and the view is alacritty's viewport.
pub fn rows<L>(term: &Term<L>, shell: &ShellState, display_offset: usize) -> Vec<ViewRow> {
    let screen_lines = term.screen_lines();
    let bottom = bottom_line(term, display_offset);
    if !decorated(term, shell) {
        let top = bottom.0 - (screen_lines as i32 - 1);
        return (0..screen_lines)
            .map(|index| ViewRow {
                top: ROW_UNITS * index as u32,
                ..ViewRow::shown(Line(top + index as i32), RowRole::None)
            })
            .collect();
    }
    let folds = Folds::new(term, shell);
    let budget = ROW_UNITS * screen_lines as u32;
    let (rows, reached_top) = block_rows(term, folds.as_ref(), bottom, budget);
    // Scrolled to the top of the history, rows cut at the top could never be reached:
    // the view starts at the top instead, unless that cuts the cursor row shown at the
    // bottom (a fresh screen, the rows below the cursor are blank).
    if !reached_top && display_offset == term.grid().history_size() {
        let (anchored, _) = block_rows(term, folds.as_ref(), term.grid().topmost_line(), budget);
        let cursor = term.grid().cursor.point.line;
        if cursor > bottom || anchored.iter().any(|row| row.line == Some(cursor)) {
            return anchored;
        }
    }
    rows
}

/// Rows of a view with blocks, within `budget` half-lines: collected upward from
/// `bottom`, skipping the rows hidden by `folds`, until the next row does not fit; at the
/// top of the history, completed downward from `bottom`, then padded with blank rows.
/// The rows hidden below `bottom` are still counted on the row above them. Also tells
/// whether the walk up reached the top of the history.
fn block_rows<L>(
    term: &Term<L>,
    folds: Option<&Folds<'_>>,
    bottom: Line,
    budget: u32,
) -> (Vec<ViewRow>, bool) {
    let hides = |line, role| folds.is_some_and(|folds| folds.hides(line, role));
    // Bottom to top.
    let mut up: Vec<ViewRow> = Vec::new();
    let mut used = 0;
    let mut hidden = 0;
    let mut bottom_role = RowRole::None;
    // The budget stopped the walk before the top of the history.
    let mut full = false;
    walk_up(term, bottom, |line, role| {
        if line == bottom {
            bottom_role = role;
        }
        if hides(line, role) {
            hidden += 1;
            return true;
        }
        let gap = up.last().is_some_and(|below| gap_between(role, below.role));
        let cost = ROW_UNITS + u32::from(gap);
        if used + cost > budget {
            full = true;
            return false;
        }
        used += cost;
        if let Some(below) = up.last_mut() {
            below.gap_before = gap;
        }
        up.push(ViewRow {
            hidden: std::mem::take(&mut hidden),
            ..ViewRow::shown(line, role)
        });
        full = used + ROW_UNITS > budget;
        !full
    });
    let mut rows: Vec<ViewRow> = up.into_iter().rev().collect();
    let reached_top = !full;

    // Below `bottom`: the rest of a hidden run, then the rows completing a view that
    // reached the top of the history.
    let mut block = bottom_role.block();
    let mut line = bottom + 1;
    while line <= term.grid().bottommost_line() {
        let role = role_below(term, line, &mut block);
        if hides(line, role) {
            if let Some(last) = rows.last_mut() {
                last.hidden += 1;
            }
        } else {
            let gap = rows
                .last()
                .is_some_and(|above| gap_between(above.role, role));
            let cost = ROW_UNITS + u32::from(gap);
            if full || used + cost > budget {
                full = true;
                break;
            }
            used += cost;
            rows.push(ViewRow {
                gap_before: gap,
                ..ViewRow::shown(line, role)
            });
        }
        line += 1;
    }

    // Blank rows only when the content ran out.
    while !full && used + ROW_UNITS <= budget {
        used += ROW_UNITS;
        rows.push(ViewRow::blank());
    }

    // The leftover half-lines go to the top.
    let mut top = budget - used;
    for row in &mut rows {
        top += u32::from(row.gap_before);
        row.top = top;
        top += ROW_UNITS;
    }
    (rows, reached_top)
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
/// hidden by a fold), otherwise the smallest scroll putting it on the top or bottom row
/// (the top row, or the second one when a gap would otherwise push it out).
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
    // The gaps of the new view may leave fewer rows above its bottom: one more row up
    // until `line` shows, or the top of the history is reached.
    let history = term.grid().history_size() as i32;
    let mut shown = shown as i32;
    loop {
        let delta = grid_delta(term, shell, display_offset, shown);
        let offset = (display_offset as i32 + delta).clamp(0, history);
        let top = rows(term, shell, offset as usize)
            .iter()
            .find_map(|row| row.line);
        if offset == history || top.is_none_or(|top| line >= top) {
            return delta;
        }
        shown += 1;
    }
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

    /// Offset from the view top of each row, in half-lines, and whether a gap is above.
    fn tops(rows: &[ViewRow]) -> Vec<(u32, bool)> {
        rows.iter().map(|row| (row.top, row.gap_before)).collect()
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
        // No gap: exactly the viewport.
        assert_eq!(tops(&rows), [(0, false), (2, false), (4, false)]);

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
        let rows = rows(&term, &shell, 0);
        assert!(rows.iter().all(|row| row.role == RowRole::None));
        assert_eq!(tops(&rows), [(0, false), (2, false), (4, false)]);
    }

    #[test]
    fn a_gap_starts_a_block_below_another_one() {
        let (a, b) = (RowRole::Output(BlockId(0)), RowRole::Prompt(BlockId(1)));
        assert!(gap_between(a, b));
        assert!(gap_between(RowRole::None, RowRole::Prompt(BlockId(0))));
        assert!(!gap_between(b, RowRole::Output(BlockId(1))));
        assert!(!gap_between(a, RowRole::None));
        assert!(!gap_between(RowRole::None, RowRole::None));
    }

    #[test]
    fn no_gap_inside_one_block() {
        let mut term = new_term(4);
        let mut shell = ShellState::new();
        command(&mut term, &mut shell, "a", 3);
        // Grid lines 0 to 3: "a1" to "a3", then the cursor row, all in block 0.
        let rows = rows(&term, &shell, 0);
        assert_eq!(texts(&term, &rows), ["a1", "a2", "a3", ""]);
        assert_eq!(
            tops(&rows),
            [(0, false), (2, false), (4, false), (6, false)]
        );
    }

    #[test]
    fn a_fresh_screen_starts_at_the_top() {
        let mut term = new_term(6);
        let mut shell = ShellState::new();
        command(&mut term, &mut shell, "a", 1);
        command(&mut term, &mut shell, "b", 0);
        feed(&mut term, &mut shell, &format!("{A}$ "));
        assert_eq!(term.grid().history_size(), 0);
        // Grid lines 0 to 5: "$ a", "a1", "$ b", "$", two blank rows below the cursor.
        // From the bottom, the gaps would cut "$ a", out of reach without a history:
        // the view starts at the top and cuts a blank row instead.
        let rows = rows(&term, &shell, 0);
        assert_eq!(texts(&term, &rows), ["$ a", "a1", "$ b", "$", ""]);
        assert_eq!(
            tops(&rows),
            [(0, false), (2, false), (5, true), (8, true), (10, false)]
        );
    }

    #[test]
    fn the_top_of_the_history_stays_reachable() {
        let (term, shell) = three_blocks();
        // Scrolled to the top (8 lines of history), from the bottom line "a3" the gap
        // would not cut anything: "$ a" is the top grid line.
        assert_eq!(lines(&rows(&term, &shell, 8)), [-8, -7, -6, -5]);
        // A view that would cut the top of the history but show the cursor row keeps
        // its bottom: the cursor row comes first.
        let mut term = new_term(3);
        let mut shell = ShellState::new();
        command(&mut term, &mut shell, "a", 1);
        feed(&mut term, &mut shell, &format!("{A}$ "));
        // Grid lines 0 to 2: "$ a", "a1", "$", no history.
        assert_eq!(texts(&term, &rows(&term, &shell, 0)), ["a1", "$"]);
    }

    #[test]
    fn several_card_boundaries_take_half_a_line_each() {
        let mut term = new_term(8);
        let mut shell = ShellState::new();
        for name in ["a", "b", "c"] {
            command(&mut term, &mut shell, name, 1);
        }
        feed(&mut term, &mut shell, &format!("{A}$ "));
        // Grid lines 0 to 7: "$ a", "a1", "$ b", "b1", "$ c", "c1", "$", the cursor row
        // below. From the bottom: 2 + 2 + 3 + 2 + 3 + 2 = 14 half-lines out of 16; "a1"
        // would take 3 more. The 2 left go to the top.
        let rows = rows(&term, &shell, 0);
        assert_eq!(texts(&term, &rows), ["$ b", "b1", "$ c", "c1", "$", ""]);
        assert_eq!(
            tops(&rows),
            [
                (2, false),
                (4, false),
                (7, true),
                (9, false),
                (12, true),
                (14, false)
            ]
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
    fn the_view_bottom_follows_the_viewport_and_gaps_push_rows_out() {
        let (term, shell) = three_blocks();
        // "$" (2), a gap and "c3" (3), "c2" (2): "c1" does not fit in the 8 half-lines,
        // the one left goes to the top.
        let rows = rows(&term, &shell, 0);
        assert_eq!(lines(&rows), [1, 2, 3]);
        assert_eq!(tops(&rows), [(1, false), (3, false), (6, true)]);
        // "b2", "b1", "$ b" (6), then "a3" would need a gap: 2 half-lines left at the
        // top.
        let rows = super::rows(&term, &shell, 5);
        assert_eq!(lines(&rows), [-4, -3, -2]);
        assert_eq!(tops(&rows), [(2, false), (4, false), (6, false)]);
        // Without a gap, as many rows as the screen.
        assert_eq!(lines(&super::rows(&term, &shell, 1)), [-1, 0, 1, 2]);
    }

    #[test]
    fn one_fold_skips_its_output_and_counts_it() {
        let (term, mut shell) = three_blocks();
        shell.set_folded(BlockId(2), true);
        let rows = rows(&term, &shell, 0);
        assert_eq!(texts(&term, &rows), ["b2", "$ c", "$"]);
        assert_eq!(rows.iter().map(|r| r.hidden).collect::<Vec<_>>(), [0, 3, 0]);
        // Folds and gaps together: the 8 half-lines are used up, nothing left.
        assert_eq!(tops(&rows), [(0, false), (3, true), (6, true)]);
    }

    #[test]
    fn several_folds() {
        let (term, mut shell) = three_blocks();
        shell.set_folded(BlockId(1), true);
        shell.set_folded(BlockId(2), true);
        let rows = rows(&term, &shell, 0);
        assert_eq!(texts(&term, &rows), ["$ b", "$ c", "$"]);
        assert_eq!(rows.iter().map(|r| r.hidden).collect::<Vec<_>>(), [2, 3, 0]);
        assert_eq!(tops(&rows), [(0, false), (3, true), (6, true)]);
    }

    #[test]
    fn top_of_history_completes_downward_then_pads() {
        let (term, mut shell) = three_blocks();
        for id in 0..3 {
            shell.set_folded(BlockId(id), true);
        }
        // All blocks folded: four visible rows in the whole grid, three fit with the
        // gaps.
        let rows = rows(&term, &shell, 0);
        assert_eq!(texts(&term, &rows), ["$ b", "$ c", "$"]);

        // Scrolled to the top of the history: the bottom grid line is hidden, the rows
        // below it complete the view, the hidden rows below "$ c" still counted.
        let rows = super::rows(&term, &shell, 8);
        assert_eq!(texts(&term, &rows), ["$ a", "$ b", "$ c"]);
        assert_eq!(rows.iter().map(|r| r.hidden).collect::<Vec<_>>(), [3, 2, 3]);
        assert_eq!(tops(&rows), [(0, false), (3, true), (6, true)]);
    }

    #[test]
    fn padding_rows_below_the_content() {
        let mut term = new_term(4);
        let mut shell = ShellState::new();
        command(&mut term, &mut shell, "a", 6);
        feed(&mut term, &mut shell, &format!("{A}$ "));
        shell.set_folded(BlockId(0), true);
        let rows = rows(&term, &shell, 0);
        assert_eq!(texts(&term, &rows), ["$ a", "$", "~"]);
        assert_eq!(rows[0].hidden, 6);
        assert_eq!(rows[2].role, RowRole::None);
        // The padding row takes no gap; the half-line left goes to the top.
        assert_eq!(tops(&rows), [(1, false), (4, true), (6, false)]);
    }

    #[test]
    fn rows_hidden_below_the_bottom_line_are_counted() {
        let (term, mut shell) = three_blocks();
        shell.set_folded(BlockId(2), true);
        // Bottom grid line 1 is "c2", hidden: the view ends on "$ c", whose count
        // includes "c3", hidden below the bottom line.
        let rows = rows(&term, &shell, 2);
        assert_eq!(texts(&term, &rows), ["b1", "b2", "$ c"]);
        assert_eq!(rows[2].hidden, 3);
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
        // Visible (the view shows lines 1 to 3): no scroll.
        assert_eq!(reveal(&term, &shell, 0, Line(1)), 0);
        assert_eq!(reveal(&term, &shell, 0, Line(3)), 0);
        // Above the view: up, the line on the top row.
        assert_eq!(reveal(&term, &shell, 0, Line(0)), 1);
        assert_eq!(reveal(&term, &shell, 0, Line(-4)), 5);
        assert_eq!(reveal(&term, &shell, 2, Line(-4)), 3);
        // Below the view: down, the line on the bottom row.
        assert_eq!(reveal(&term, &shell, 5, Line(0)), -2);
    }

    #[test]
    fn reveal_scrolls_one_more_row_when_a_gap_comes_in() {
        let (term, shell) = three_blocks();
        // Lines -1 to 2 shown without a gap. One row up, "b2" would come in with a gap
        // and push "$ c" out: one more row.
        assert_eq!(lines(&rows(&term, &shell, 1)), [-1, 0, 1, 2]);
        assert_eq!(lines(&rows(&term, &shell, 2)), [-1, 0, 1]);
        let delta = reveal(&term, &shell, 1, Line(-2));
        assert_eq!(delta, 2);
        assert_eq!(lines(&rows(&term, &shell, 3)), [-2, -1, 0]);
    }

    #[test]
    fn reveal_counts_visible_rows_across_a_fold() {
        let (term, mut shell) = three_blocks();
        shell.set_folded(BlockId(1), true);
        // View: "c2", "c3", "$". Block 1's prompt (-4) is three visible rows above the
        // top row ("c1", "$ c", then "$ b").
        assert_eq!(lines(&rows(&term, &shell, 0)), [1, 2, 3]);
        let delta = reveal(&term, &shell, 0, Line(-4));
        assert_eq!(delta, grid_delta(&term, &shell, 0, 3));
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
        rows.push(ViewRow::blank());
        assert_eq!(point(&rows, 3, 2), Point::new(Line(0), Column(2)));
    }
}
