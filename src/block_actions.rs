//! Block actions: which block an action targets, block navigation and the copied texts
//! of a block. Pure: works on the [`Timeline`], no window or clipboard access. Only
//! finished blocks are targets: the current one is still being written.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;

use crate::selection::clean_copied_text;
use crate::timeline::{BlockId, Timeline};

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

/// `selected` when it is a finished block still in the timeline.
fn finished(timeline: &Timeline, selected: Option<BlockId>) -> Option<BlockId> {
    selected.filter(|id| timeline.block(*id).is_some())
}

fn last_finished(timeline: &Timeline) -> Option<BlockId> {
    timeline.blocks().back().map(|block| block.id)
}

/// Block an action applies to: the selected block, otherwise the last finished block.
pub fn target(timeline: &Timeline, selected: Option<BlockId>) -> Option<BlockId> {
    finished(timeline, selected).or_else(|| last_finished(timeline))
}

/// Block selected by a navigation: the neighbour of the selected block in `direction`,
/// the selected block itself at either end. Without a selected block, the last finished
/// block.
pub fn neighbour(
    timeline: &Timeline,
    selected: Option<BlockId>,
    direction: Direction,
) -> Option<BlockId> {
    let Some(current) = finished(timeline, selected) else {
        return last_finished(timeline);
    };
    let blocks = timeline.blocks();
    let found = match direction {
        // Finished blocks are ordered oldest first, ids increasing.
        Direction::Previous => blocks.iter().rev().find(|block| block.id < current),
        Direction::Next => blocks.iter().find(|block| block.id > current),
    };
    Some(found.map_or(current, |block| block.id))
}

/// Cleaned text of finished block `id`: its command, then its output. `None` when both
/// are empty.
pub fn block_text(timeline: &Timeline, id: BlockId) -> Option<String> {
    let parts: Vec<String> = [Part::Command, Part::Output]
        .into_iter()
        .filter_map(|part| text(timeline, id, part))
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

/// Cleaned text of one part of finished block `id`, `None` when that part is empty. The
/// command is the one its `start` signal carried, or the last header row (where the
/// command was typed) when the signal carried none.
pub fn text(timeline: &Timeline, id: BlockId, part: Part) -> Option<String> {
    let block = timeline.block(id)?;
    let text = match part {
        Part::Command if !block.command.is_empty() => block.command.clone(),
        Part::Command => {
            let last = block.header.bottommost_line();
            rows_text(&block.header, last, last)
        }
        Part::Output => rows_text(
            &block.output,
            block.output.topmost_line(),
            block.output.bottommost_line(),
        ),
    };
    Some(clean_copied_text(&text)).filter(|text| !text.is_empty())
}

/// Text of the rows `first` to `last` of a frozen terminal.
fn rows_text<L>(term: &Term<L>, first: Line, last: Line) -> String {
    term.bounds_to_string(
        Point::new(first, Column(0)),
        Point::new(last, term.last_column()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::testing::{Session, end, start};

    /// Three finished blocks ("one", "two" printing two rows, "three" printing nothing),
    /// then a fresh prompt.
    fn three_blocks() -> Session {
        let mut session = Session::sized(20, 10, 1000);
        session.command("one", &["1"], 0);
        session.command("two", &["2a", "2b"], 0);
        session.command("three", &[], 0);
        session.feed("$ ");
        session
    }

    #[test]
    fn target_is_the_last_finished_block_without_selection() {
        let session = three_blocks();
        assert_eq!(target(&session.timeline, None), Some(BlockId(2)));
    }

    #[test]
    fn running_block_is_not_a_target() {
        let mut session = Session::new(1000);
        session.command("ls", &["out"], 0);
        session.feed(&format!("$ make\r\n{}", start("make")));
        assert_eq!(target(&session.timeline, None), Some(BlockId(0)));
        let current = session.timeline.current().id;
        assert_eq!(target(&session.timeline, Some(current)), Some(BlockId(0)));
    }

    #[test]
    fn target_is_the_selected_block() {
        let session = three_blocks();
        assert_eq!(
            target(&session.timeline, Some(BlockId(1))),
            Some(BlockId(1))
        );
        // A block no longer in the timeline is not selected any more.
        assert_eq!(
            target(&session.timeline, Some(BlockId(42))),
            Some(BlockId(2))
        );
    }

    #[test]
    fn no_target_without_blocks() {
        let mut session = Session::new(1000);
        session.feed("plain shell\r\n");
        assert_eq!(target(&session.timeline, None), None);
    }

    #[test]
    fn first_navigation_selects_the_last_finished_block() {
        let session = three_blocks();
        for direction in [Direction::Previous, Direction::Next] {
            assert_eq!(
                neighbour(&session.timeline, None, direction),
                Some(BlockId(2))
            );
        }
    }

    #[test]
    fn navigation_moves_to_the_neighbours_and_stops_at_the_ends() {
        let timeline = &three_blocks().timeline;
        let at = |id, direction| neighbour(timeline, Some(BlockId(id)), direction);
        assert_eq!(at(1, Direction::Previous), Some(BlockId(0)));
        assert_eq!(at(1, Direction::Next), Some(BlockId(2)));
        assert_eq!(at(0, Direction::Previous), Some(BlockId(0)));
        // The fresh prompt is not finished: Next stays on the last finished block.
        assert_eq!(at(2, Direction::Next), Some(BlockId(2)));
    }

    #[test]
    fn navigation_skips_evicted_blocks() {
        // A scrollback of 4 rows keeps the last two blocks only.
        let mut session = Session::new(4);
        session.command("a", &["1"], 0);
        session.command("b", &["1"], 0);
        session.command("c", &["1"], 0);
        assert_eq!(session.timeline.blocks()[0].id, BlockId(1));
        assert_eq!(
            neighbour(&session.timeline, Some(BlockId(2)), Direction::Previous),
            Some(BlockId(1))
        );
        assert_eq!(
            neighbour(&session.timeline, Some(BlockId(1)), Direction::Previous),
            Some(BlockId(1))
        );
    }

    #[test]
    fn copied_command_output_and_whole_block() {
        let timeline = &three_blocks().timeline;
        assert_eq!(
            text(timeline, BlockId(1), Part::Command).as_deref(),
            Some("two")
        );
        assert_eq!(
            text(timeline, BlockId(1), Part::Output).as_deref(),
            Some("2a\n2b")
        );
        assert_eq!(
            block_text(timeline, BlockId(1)).as_deref(),
            Some("two\n2a\n2b")
        );
        assert_eq!(block_text(timeline, BlockId(2)).as_deref(), Some("three"));
    }

    #[test]
    fn command_falls_back_to_the_last_header_row() {
        let mut session = Session::new(1000);
        session.feed(&format!("~/dev\r\n$ make  \r\n{}", start("")));
        session.feed(&format!("ok\r\n{}", end(0)));
        assert_eq!(
            text(&session.timeline, BlockId(0), Part::Command).as_deref(),
            Some("$ make")
        );
    }

    #[test]
    fn empty_parts_have_no_text() {
        let timeline = &three_blocks().timeline;
        // Block 2 printed nothing.
        assert_eq!(text(timeline, BlockId(2), Part::Output), None);
        // The current block and unknown blocks are no targets.
        assert_eq!(text(timeline, BlockId(3), Part::Command), None);
        assert_eq!(block_text(timeline, BlockId(42)), None);

        // Blank output rows only.
        let mut session = Session::new(1000);
        session.command("blank", &["   ", ""], 0);
        assert_eq!(text(&session.timeline, BlockId(0), Part::Output), None);
    }
}
