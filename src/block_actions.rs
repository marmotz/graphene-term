//! Block actions: which block an action targets, block navigation and the copied texts
//! of a block. Pure: works on the [`Timeline`], no window or clipboard access. The
//! navigable blocks are the finished ones, oldest first, then the current one.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;

use crate::selection::clean_copied_text;
use crate::timeline::{BlockId, Phase, Timeline};

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

/// `selected` when it is still in the timeline: a finished block or the current one.
fn selectable(timeline: &Timeline, selected: Option<BlockId>) -> Option<BlockId> {
    selected.filter(|id| timeline.contains(*id))
}

fn last_finished(timeline: &Timeline) -> Option<BlockId> {
    timeline.blocks().back().map(|block| block.id)
}

/// Block an action applies to: the selected block (the current one included),
/// otherwise the last finished block.
pub fn target(timeline: &Timeline, selected: Option<BlockId>) -> Option<BlockId> {
    selectable(timeline, selected).or_else(|| last_finished(timeline))
}

/// Block selected by a navigation through the finished blocks then the current one:
/// the neighbour of the selected block in `direction`, the selected block itself at
/// either end. Without a selected block, `Previous` gives the last finished block
/// (the current one when there is none) and `Next` the current block.
pub fn neighbour(
    timeline: &Timeline,
    selected: Option<BlockId>,
    direction: Direction,
) -> Option<BlockId> {
    let current = timeline.current().id;
    let Some(selected) = selectable(timeline, selected) else {
        return Some(match direction {
            Direction::Previous => last_finished(timeline).unwrap_or(current),
            Direction::Next => current,
        });
    };
    let blocks = timeline.blocks();
    // Finished blocks are ordered oldest first, ids increasing; the current block has
    // the greatest id.
    let found = match direction {
        Direction::Previous => blocks
            .iter()
            .rev()
            .find(|block| block.id < selected)
            .map(|block| block.id),
        Direction::Next if selected == current => None,
        Direction::Next => Some(
            blocks
                .iter()
                .find(|block| block.id > selected)
                .map_or(current, |block| block.id),
        ),
    };
    Some(found.unwrap_or(selected))
}

/// Cleaned text of block `id`: its command, then its output. `None` when both are
/// empty.
pub fn block_text(timeline: &Timeline, id: BlockId) -> Option<String> {
    let parts: Vec<String> = [Part::Command, Part::Output]
        .into_iter()
        .filter_map(|part| text(timeline, id, part))
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

/// Cleaned text of one part of block `id`, `None` when that part is empty. The command
/// is the one its `start` signal carried, or the last header row (where the command was
/// typed) when the signal carried none. The current block has a command once it runs,
/// none at the prompt, and never an output: it is frozen only when the command ends.
pub fn text(timeline: &Timeline, id: BlockId, part: Part) -> Option<String> {
    let current = timeline.current();
    let (command, header, output) = if id == current.id {
        if current.phase == Phase::Prompt {
            return None;
        }
        (&current.command, current.header.as_ref()?, None)
    } else {
        let block = timeline.block(id)?;
        (&block.command, &block.header, Some(&block.output))
    };
    let text = match part {
        Part::Command if !command.is_empty() => command.clone(),
        Part::Command => {
            let last = header.bottommost_line();
            rows_text(header, last, last)
        }
        Part::Output => {
            let output = output?;
            rows_text(output, output.topmost_line(), output.bottommost_line())
        }
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
    fn selected_current_block_is_the_target() {
        let mut session = Session::new(1000);
        session.command("ls", &["out"], 0);
        session.feed(&format!("$ make\r\n{}", start("make")));
        // Without a selection, the running block is not the target.
        assert_eq!(target(&session.timeline, None), Some(BlockId(0)));
        let current = session.timeline.current().id;
        assert_eq!(target(&session.timeline, Some(current)), Some(current));
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
    fn first_navigation_selects_the_last_finished_or_the_current_block() {
        let session = three_blocks();
        let timeline = &session.timeline;
        assert_eq!(
            neighbour(timeline, None, Direction::Previous),
            Some(BlockId(2))
        );
        assert_eq!(
            neighbour(timeline, None, Direction::Next),
            Some(timeline.current().id)
        );
    }

    #[test]
    fn without_finished_blocks_navigation_selects_the_current_block() {
        let mut session = Session::new(1000);
        session.feed("plain shell\r\n");
        let timeline = &session.timeline;
        let current = timeline.current().id;
        for direction in [Direction::Previous, Direction::Next] {
            assert_eq!(neighbour(timeline, None, direction), Some(current));
            assert_eq!(neighbour(timeline, Some(current), direction), Some(current));
        }
    }

    #[test]
    fn navigation_moves_to_the_neighbours_and_stops_at_the_ends() {
        let timeline = &three_blocks().timeline;
        let at = |id, direction| neighbour(timeline, Some(BlockId(id)), direction);
        assert_eq!(at(1, Direction::Previous), Some(BlockId(0)));
        assert_eq!(at(1, Direction::Next), Some(BlockId(2)));
        assert_eq!(at(0, Direction::Previous), Some(BlockId(0)));
        // After the last finished block comes the current one, the fresh prompt.
        let current = timeline.current().id;
        assert_eq!(at(2, Direction::Next), Some(current));
        assert_eq!(at(current.0, Direction::Next), Some(current));
        assert_eq!(at(current.0, Direction::Previous), Some(BlockId(2)));
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
        // Unknown blocks have no text.
        assert_eq!(block_text(timeline, BlockId(42)), None);

        // Blank output rows only.
        let mut session = Session::new(1000);
        session.command("blank", &["   ", ""], 0);
        assert_eq!(text(&session.timeline, BlockId(0), Part::Output), None);
    }

    #[test]
    fn running_block_copies_its_command_but_not_its_output() {
        let mut session = Session::new(1000);
        session.feed(&format!("$ make\r\n{}building\r\n", start("make")));
        let timeline = &session.timeline;
        let current = timeline.current().id;
        assert_eq!(
            text(timeline, current, Part::Command).as_deref(),
            Some("make")
        );
        assert_eq!(text(timeline, current, Part::Output), None);
        assert_eq!(block_text(timeline, current).as_deref(), Some("make"));
    }

    #[test]
    fn running_block_command_falls_back_to_the_last_header_row() {
        let mut session = Session::new(1000);
        session.feed(&format!("~/dev\r\n$ make  \r\n{}", start("")));
        let timeline = &session.timeline;
        assert_eq!(
            text(timeline, timeline.current().id, Part::Command).as_deref(),
            Some("$ make")
        );
    }

    #[test]
    fn prompt_has_no_text() {
        let timeline = &three_blocks().timeline;
        let current = timeline.current().id;
        for part in [Part::Command, Part::Output] {
            assert_eq!(text(timeline, current, part), None);
        }
        assert_eq!(block_text(timeline, current), None);
    }
}
