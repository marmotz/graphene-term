//! Block actions: which block an action targets, block navigation, block selection and
//! the copied command and output texts. Pure: works on a `Term` and its `ShellState`,
//! no window or clipboard access.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::Term;

use crate::blocks::{self, BlockBounds, BlockId, ShellState};
use crate::selection::clean_copied_text;

/// Direction of a block navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Previous,
    Next,
}

/// Part of a block copied to the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Command,
    Output,
}

/// Block holding the start of the selection, `None` without selection or outside blocks.
fn selected_block<L>(term: &Term<L>) -> Option<BlockId> {
    let start = term.selection.as_ref()?.to_range(term)?.start;
    blocks::index(term, start.line..start.line + 1)[0].block()
}

/// Finished blocks with rows left in the grid, top to bottom.
fn finished_blocks<L>(term: &Term<L>, shell: &ShellState) -> Vec<BlockId> {
    blocks::blocks_in_grid(term)
        .into_iter()
        .filter(|id| shell.is_finished(*id))
        .collect()
}

/// Block an action applies to: the one holding the start of the selection, otherwise
/// the last finished block.
pub fn target<L>(term: &Term<L>, shell: &ShellState) -> Option<BlockId> {
    selected_block(term).or_else(|| finished_blocks(term, shell).last().copied())
}

/// Block selected by a navigation: the finished neighbour of the selected block in
/// `direction`, the selected block itself at either end. Without a block selection,
/// the last finished block.
pub fn neighbour<L>(term: &Term<L>, shell: &ShellState, direction: Direction) -> Option<BlockId> {
    let finished = finished_blocks(term, shell);
    let Some(current) = selected_block(term) else {
        return finished.last().copied();
    };
    let found = match direction {
        // Block ids increase from top to bottom of the grid.
        Direction::Previous => finished.iter().rev().find(|id| **id < current),
        Direction::Next => finished.iter().find(|id| **id > current),
    };
    Some(found.copied().unwrap_or(current))
}

/// Selects block `id`: from the start of its command to the end of its last row.
/// Returns its bounds, `None` (selection untouched) when none of its rows is left.
pub fn select<L>(term: &mut Term<L>, id: BlockId) -> Option<BlockBounds> {
    let bounds = blocks::block_bounds(term, id)?;
    let last_line = if bounds.output.is_empty() {
        bounds.input_end.unwrap_or(bounds.input_start).line
    } else {
        bounds.output.end - 1
    };
    let mut selection = Selection::new(SelectionType::Simple, bounds.input_start, Side::Left);
    selection.update(Point::new(last_line, term.last_column()), Side::Right);
    term.selection = Some(selection);
    Some(bounds)
}

/// Cleaned text of one part of a block, `None` when that part is empty.
pub fn text<L>(term: &Term<L>, bounds: &BlockBounds, part: Part) -> Option<String> {
    let (start, end) = match part {
        Part::Command => (bounds.input_start, bounds.input_end?),
        Part::Output if bounds.output.is_empty() => return None,
        Part::Output => (
            Point::new(bounds.output.start, Column(0)),
            Point::new(bounds.output.end - 1, term.last_column()),
        ),
    };
    Some(clean_copied_text(&term.bounds_to_string(start, end))).filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc133::Scanner;
    use crate::pty;
    use crate::selection::selected_text;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::index::Line;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::Processor;

    const A: &str = "\x1b]133;A\x07";
    const B: &str = "\x1b]133;B\x07";
    const C: &str = "\x1b]133;C\x07";

    struct Harness {
        term: Term<VoidListener>,
        shell: ShellState,
        scanner: Scanner,
        processor: Processor,
    }

    impl Harness {
        fn new(lines: usize) -> Self {
            Self {
                term: Term::new(Config::default(), &TermSize::new(20, lines), VoidListener),
                shell: ShellState::new(),
                scanner: Scanner::new(),
                processor: Processor::new(),
            }
        }

        fn feed(&mut self, bytes: &str) -> &mut Self {
            pty::process(
                &mut self.scanner,
                &mut self.processor,
                &mut self.shell,
                &mut self.term,
                bytes.as_bytes(),
            );
            self
        }

        /// One full command: prompt, input, output, end with status 0.
        fn command(&mut self, input: &str, output: &str) -> &mut Self {
            self.feed(&format!("{A}$ {B}{input}\r\n{C}{output}\x1b]133;D;0\x07"))
        }

        /// Three finished blocks, then a fresh prompt (block 3, not finished).
        fn three_blocks() -> Self {
            let mut h = Self::new(20);
            h.command("one", "1\r\n")
                .command("two", "2a\r\n2b\r\n")
                .command("three", "")
                .feed(&format!("{A}$ "));
            h
        }

        fn select_at(&mut self, line: i32, column: usize) {
            let point = Point::new(Line(line), Column(column));
            let mut selection = Selection::new(SelectionType::Simple, point, Side::Left);
            selection.update(point, Side::Right);
            self.term.selection = Some(selection);
        }

        fn bounds(&self, id: u64) -> BlockBounds {
            blocks::block_bounds(&self.term, BlockId(id)).unwrap()
        }
    }

    #[test]
    fn target_is_the_last_finished_block_without_selection() {
        let h = Harness::three_blocks();
        assert_eq!(target(&h.term, &h.shell), Some(BlockId(2)));
    }

    #[test]
    fn target_is_the_last_block_once_its_command_ended() {
        let mut h = Harness::new(10);
        h.command("ls", "out\r\n");
        assert_eq!(target(&h.term, &h.shell), Some(BlockId(0)));
    }

    #[test]
    fn running_block_is_not_a_default_target() {
        let mut h = Harness::new(10);
        h.command("ls", "out\r\n")
            .feed(&format!("{A}$ {B}make\r\n{C}"));
        assert_eq!(target(&h.term, &h.shell), Some(BlockId(0)));
    }

    #[test]
    fn target_is_the_block_holding_the_selection_start() {
        let mut h = Harness::three_blocks();
        // Row 3 is the first output row of block 1 ("2a").
        h.select_at(3, 0);
        assert_eq!(target(&h.term, &h.shell), Some(BlockId(1)));
        // A selection on the fresh prompt targets it, finished or not.
        h.select_at(6, 0);
        assert_eq!(target(&h.term, &h.shell), Some(BlockId(3)));
    }

    #[test]
    fn no_target_without_blocks() {
        let mut h = Harness::new(5);
        h.feed("plain shell\r\n");
        assert_eq!(target(&h.term, &h.shell), None);
    }

    #[test]
    fn first_navigation_selects_the_last_finished_block() {
        let h = Harness::three_blocks();
        for direction in [Direction::Previous, Direction::Next] {
            assert_eq!(neighbour(&h.term, &h.shell, direction), Some(BlockId(2)));
        }
    }

    #[test]
    fn navigation_moves_to_the_neighbours_and_stops_at_the_ends() {
        let mut h = Harness::three_blocks();
        select(&mut h.term, BlockId(1)).unwrap();
        assert_eq!(
            neighbour(&h.term, &h.shell, Direction::Previous),
            Some(BlockId(0))
        );
        assert_eq!(
            neighbour(&h.term, &h.shell, Direction::Next),
            Some(BlockId(2))
        );

        select(&mut h.term, BlockId(0)).unwrap();
        assert_eq!(
            neighbour(&h.term, &h.shell, Direction::Previous),
            Some(BlockId(0))
        );
        // The fresh prompt is not finished: Next stays on the last finished block.
        select(&mut h.term, BlockId(2)).unwrap();
        assert_eq!(
            neighbour(&h.term, &h.shell, Direction::Next),
            Some(BlockId(2))
        );
    }

    #[test]
    fn navigation_skips_blocks_cleared_from_the_grid() {
        let mut h = Harness::new(10);
        // Clears the screen and the history.
        h.command("old", "x\r\n").feed("\x1b[H\x1b[2J\x1b[3J");
        h.command("new", "y\r\n").feed(&format!("{A}$ "));
        assert_eq!(
            neighbour(&h.term, &h.shell, Direction::Previous),
            Some(BlockId(1))
        );
        select(&mut h.term, BlockId(1)).unwrap();
        assert_eq!(
            neighbour(&h.term, &h.shell, Direction::Previous),
            Some(BlockId(1))
        );
    }

    #[test]
    fn block_selection_spans_the_command_and_the_output() {
        let mut h = Harness::three_blocks();
        let bounds = select(&mut h.term, BlockId(1)).unwrap();
        assert_eq!(bounds.first_row, Line(2));

        let range = h
            .term
            .selection
            .as_ref()
            .unwrap()
            .to_range(&h.term)
            .unwrap();
        assert_eq!(range.start, Point::new(Line(2), Column(2)));
        assert_eq!(range.end, Point::new(Line(4), Column(19)));
        assert_eq!(selected_text(&h.term).as_deref(), Some("two\n2a\n2b"));
    }

    #[test]
    fn block_selection_without_output_ends_on_the_command_row() {
        let mut h = Harness::three_blocks();
        select(&mut h.term, BlockId(2)).unwrap();
        let range = h
            .term
            .selection
            .as_ref()
            .unwrap()
            .to_range(&h.term)
            .unwrap();
        assert_eq!(range.start, Point::new(Line(5), Column(2)));
        assert_eq!(range.end, Point::new(Line(5), Column(19)));
        assert_eq!(selected_text(&h.term).as_deref(), Some("three"));
    }

    #[test]
    fn selecting_an_unknown_block_keeps_the_selection() {
        let mut h = Harness::three_blocks();
        h.select_at(0, 0);
        let before = h.term.selection.clone();
        assert_eq!(select(&mut h.term, BlockId(42)), None);
        assert_eq!(h.term.selection, before);
    }

    #[test]
    fn copied_command_and_output() {
        let h = Harness::three_blocks();
        let bounds = h.bounds(1);
        assert_eq!(
            text(&h.term, &bounds, Part::Command).as_deref(),
            Some("two")
        );
        assert_eq!(
            text(&h.term, &bounds, Part::Output).as_deref(),
            Some("2a\n2b")
        );
    }

    #[test]
    fn copied_multi_line_command() {
        let mut h = Harness::new(10);
        h.feed(&format!(
            "{A}$ {B}echo \\\r\n> x\r\n{C}x\r\n\x1b]133;D;0\x07"
        ));
        let bounds = h.bounds(0);
        assert_eq!(
            text(&h.term, &bounds, Part::Command).as_deref(),
            Some("echo \\\n> x")
        );
        assert_eq!(text(&h.term, &bounds, Part::Output).as_deref(), Some("x"));
    }

    #[test]
    fn empty_parts_have_no_text() {
        let h = Harness::three_blocks();
        // Block 2 printed nothing.
        assert_eq!(text(&h.term, &h.bounds(2), Part::Output), None);
        // Block 3 is a prompt with nothing typed.
        assert_eq!(text(&h.term, &h.bounds(3), Part::Command), None);
        assert_eq!(text(&h.term, &h.bounds(3), Part::Output), None);

        // Blank output rows only.
        let mut h = Harness::new(10);
        h.command("blank", "   \r\n\r\n").feed(&format!("{A}$ "));
        assert_eq!(text(&h.term, &h.bounds(0), Part::Output), None);
    }
}
