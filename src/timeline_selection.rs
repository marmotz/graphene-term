//! Text selection over the timeline. A selection may start in one block and end in
//! another: it is kept as two ends in view sources, and split into one alacritty
//! [`Selection`] per grid it covers (whole grids in the middle), each set on its grid
//! so the frame highlights it and `selection_to_string` reads it.

use std::collections::HashMap;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::Term;

use crate::selection::clean_copied_text;
use crate::timeline::{BlockId, Phase, Timeline};
use crate::view::{self, RowSource};

/// One end of a selection: a cell side in a view source. A summary row stands for the
/// whole hidden output of its block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct End {
    pub source: RowSource,
    pub point: Point,
    pub side: Side,
}

/// A text selection over the timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineSelection {
    ty: SelectionType,
    /// Where the selection started. On the live grid, lines count from the top of its
    /// history, so the end stays on its text while new output scrolls the grid.
    start: End,
    /// Where the selection ends, same convention.
    end: End,
    /// Current block and its phase when the selection started: which content the live
    /// grid held.
    live: (BlockId, Phase),
}

impl TimelineSelection {
    /// An empty selection of kind `ty` at `at`. `live` is the session's terminal.
    pub fn new<L>(ty: SelectionType, at: End, timeline: &Timeline, live: &Term<L>) -> Self {
        let at = absolute(at, live);
        let current = timeline.current();
        TimelineSelection {
            ty,
            start: at,
            end: at,
            live: (current.id, current.phase),
        }
    }

    /// Moves the end of the selection to `at`.
    pub fn update<L>(&mut self, at: End, live: &Term<L>) {
        self.end = absolute(at, live);
    }

    /// Whether the selection covers no cell.
    pub fn is_empty<L>(&self, timeline: &Timeline, live: &Term<L>) -> bool {
        match self.parts(timeline, live).as_slice() {
            [] => true,
            [(_, part)] => part.is_empty(),
            _ => false,
        }
    }

    /// Whether the selection reaches the live grid. The live grid is the last source,
    /// so one end is on it.
    pub fn covers_live(&self, timeline: &Timeline) -> bool {
        [self.start, self.end]
            .iter()
            .any(|end| grid_of(timeline, end.source) == RowSource::Live)
    }

    /// Whether the live grid the selection covers was detached into a block since the
    /// selection started: its live ends no longer point at the same content.
    pub fn is_stale(&self, timeline: &Timeline) -> bool {
        let current = timeline.current();
        self.covers_live(timeline) && (current.id, current.phase) != self.live
    }

    /// The selection split per grid, top to bottom: the first grid from the upper end
    /// to its last cell, the grids in between whole, the last grid from its first cell
    /// to the lower end. A block selection keeps its columns in every grid. Empty when
    /// an end is on a grid no longer shown.
    pub fn parts<L>(&self, timeline: &Timeline, live: &Term<L>) -> Vec<(RowSource, Selection)> {
        let sources = view::sources(timeline, live);
        let rank = |end: &End| {
            let grid = grid_of(timeline, end.source);
            sources.iter().position(|source| *source == grid)
        };
        let (Some(start_rank), Some(end_rank)) = (rank(&self.start), rank(&self.end)) else {
            return Vec::new();
        };
        let bounds = |source| bounds(timeline, live, source);
        let summary = |end: &End| matches!(end.source, RowSource::Summary(_));

        // Same grid, no summary: one selection oriented as the user dragged it.
        if start_rank == end_rank && !summary(&self.start) && !summary(&self.end) {
            let grid = sources[start_rank];
            let Some(grid_bounds) = bounds(grid) else {
                return Vec::new();
            };
            let (start, end) = (
                relative(self.start, live, grid_bounds),
                relative(self.end, live, grid_bounds),
            );
            let mut part = Selection::new(self.ty, start.point, start.side);
            part.update(end.point, end.side);
            return vec![(grid, part)];
        }

        let start_first = (start_rank, self.start.point) <= (end_rank, self.end.point);
        let ((upper, upper_rank), (lower, lower_rank)) = if start_first {
            ((self.start, start_rank), (self.end, end_rank))
        } else {
            ((self.end, end_rank), (self.start, start_rank))
        };
        let (Some(upper_bounds), Some(lower_bounds)) =
            (bounds(sources[upper_rank]), bounds(sources[lower_rank]))
        else {
            return Vec::new();
        };
        let upper = resolve(upper, live, upper_bounds, true);
        let lower = resolve(lower, live, lower_bounds, false);

        let block = self.ty == SelectionType::Block;
        (upper_rank..=lower_rank)
            .filter_map(|rank| {
                let grid = sources[rank];
                let grid_bounds = bounds(grid)?;
                let (top, bottom, last) = grid_bounds;
                let first = if rank == upper_rank {
                    (upper.point, upper.side)
                } else if block {
                    (Point::new(top, upper.point.column), upper.side)
                } else {
                    (Point::new(top, Column(0)), Side::Left)
                };
                let final_ = if rank == lower_rank {
                    (lower.point, lower.side)
                } else if block {
                    (Point::new(bottom, lower.point.column), lower.side)
                } else {
                    (Point::new(bottom, last), Side::Right)
                };
                let mut part = Selection::new(self.ty, first.0, first.1);
                part.update(final_.0, final_.1);
                Some((grid, part))
            })
            .collect()
    }
}

/// First line, last line and last column of the grid `source`.
type Bounds = (Line, Line, Column);

fn bounds<L>(timeline: &Timeline, live: &Term<L>, source: RowSource) -> Option<Bounds> {
    match source {
        RowSource::Live => Some((
            live.topmost_line(),
            live.bottommost_line(),
            live.last_column(),
        )),
        source => view::frozen_term(timeline, source).map(|term| {
            (
                term.topmost_line(),
                term.bottommost_line(),
                term.last_column(),
            )
        }),
    }
}

/// Grid holding the cells of `source`: a summary row stands for its block's output,
/// the live grid for the current block.
fn grid_of(timeline: &Timeline, source: RowSource) -> RowSource {
    match source {
        RowSource::Summary(id) if timeline.block(id).is_some() => RowSource::Output(id),
        RowSource::Summary(_) => RowSource::Live,
        source => source,
    }
}

/// `at` with a live line counted from the top of the live history.
fn absolute<L>(mut at: End, live: &Term<L>) -> End {
    if at.source == RowSource::Live {
        at.point.line += live.grid().history_size() as i32;
    }
    at
}

/// `at` back in grid lines, clamped to the grid `bounds`.
fn relative<L>(mut at: End, live: &Term<L>, (top, bottom, last): Bounds) -> End {
    if at.source == RowSource::Live {
        at.point.line -= live.grid().history_size() as i32;
    }
    at.point.line = at.point.line.clamp(top, bottom);
    at.point.column = at.point.column.min(last);
    at
}

/// `at` as the upper or lower end of a selection: on a summary row, the first or the
/// last cell of the hidden output.
fn resolve<L>(at: End, live: &Term<L>, bounds: Bounds, upper: bool) -> End {
    if !matches!(at.source, RowSource::Summary(_)) {
        return relative(at, live, bounds);
    }
    let (top, bottom, last) = bounds;
    let (point, side) = if upper {
        (Point::new(top, Column(0)), Side::Left)
    } else {
        (Point::new(bottom, last), Side::Right)
    };
    End { point, side, ..at }
}

/// Sets the parts of `selection` on their grids, and clears every other grid's
/// selection. `term` is the session's terminal.
pub fn apply<L>(
    selection: Option<&TimelineSelection>,
    term: &mut Term<L>,
    timeline: &mut Timeline,
) {
    let mut parts: HashMap<RowSource, Selection> = selection
        .map(|selection| selection.parts(timeline, term))
        .unwrap_or_default()
        .into_iter()
        .collect();
    term.selection = parts.remove(&RowSource::Live);
    for (id, header, frozen) in timeline.frozen_mut() {
        frozen.selection = parts.remove(&frozen_source(id, header));
    }
}

/// Clears the selection of every grid. Returns whether one had a selection.
pub fn clear<L>(term: &mut Term<L>, timeline: &mut Timeline) -> bool {
    let mut cleared = term.selection.take().is_some();
    for (_, _, frozen) in timeline.frozen_mut() {
        cleared |= frozen.selection.take().is_some();
    }
    cleared
}

fn frozen_source(id: BlockId, header: bool) -> RowSource {
    if header {
        RowSource::Header(id)
    } else {
        RowSource::Output(id)
    }
}

/// Cleaned text selected in the grids, top to bottom, the parts joined by a line
/// break. `None` without selection.
pub fn text<L>(term: &Term<L>, timeline: &Timeline) -> Option<String> {
    let parts: Vec<String> = timeline
        .frozen()
        .filter_map(|(_, _, frozen)| frozen.selection_to_string())
        .chain(term.selection_to_string())
        .collect();
    (!parts.is_empty()).then(|| clean_copied_text(&parts.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::testing::Session;

    /// Two finished blocks ("one" printing two rows, "two" printing one), then the prompt
    /// `$ ` in the live grid. Columns are 20 wide.
    fn two_blocks() -> Session {
        let mut session = Session::sized(20, 10, 1000);
        session.command("one", &["1a", "1b"], 0);
        session.command("two", &["2a"], 0);
        session.feed("$ ");
        session
    }

    fn at(source: RowSource, line: i32, column: usize, side: Side) -> End {
        End {
            source,
            point: Point::new(Line(line), Column(column)),
            side,
        }
    }

    fn select(session: &mut Session, from: End, to: End) -> TimelineSelection {
        let mut selection = TimelineSelection::new(
            SelectionType::Simple,
            from,
            &session.timeline,
            &session.term,
        );
        selection.update(to, &session.term);
        apply(Some(&selection), &mut session.term, &mut session.timeline);
        selection
    }

    fn copied(session: &Session) -> Option<String> {
        text(&session.term, &session.timeline)
    }

    const ONE: BlockId = BlockId(0);
    const TWO: BlockId = BlockId(1);

    #[test]
    fn selection_in_one_grid_is_one_part() {
        let mut session = two_blocks();
        let output = RowSource::Output(ONE);
        let selection = select(
            &mut session,
            at(output, 0, 1, Side::Left),
            at(output, 1, 0, Side::Right),
        );
        let parts = selection.parts(&session.timeline, &session.term);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].0, output);
        assert_eq!(copied(&session).as_deref(), Some("a\n1"));
    }

    #[test]
    fn selection_over_two_grids_is_split() {
        let mut session = two_blocks();
        let selection = select(
            &mut session,
            at(RowSource::Header(ONE), 0, 2, Side::Left),
            at(RowSource::Output(ONE), 0, 1, Side::Right),
        );
        let sources: Vec<RowSource> = selection
            .parts(&session.timeline, &session.term)
            .into_iter()
            .map(|(source, _)| source)
            .collect();
        assert_eq!(sources, [RowSource::Header(ONE), RowSource::Output(ONE)]);
        assert_eq!(copied(&session).as_deref(), Some("one\n1a"));
    }

    #[test]
    fn selection_over_three_blocks_takes_the_middle_grids_whole() {
        let mut session = two_blocks();
        let selection = select(
            &mut session,
            at(RowSource::Output(ONE), 1, 0, Side::Left),
            at(RowSource::Live, 0, 0, Side::Right),
        );
        let sources: Vec<RowSource> = selection
            .parts(&session.timeline, &session.term)
            .into_iter()
            .map(|(source, _)| source)
            .collect();
        assert_eq!(
            sources,
            [
                RowSource::Output(ONE),
                RowSource::Header(TWO),
                RowSource::Output(TWO),
                RowSource::Live,
            ]
        );
        assert_eq!(copied(&session).as_deref(), Some("1b\n$ two\n2a\n$"));
        // Every part is set on its grid, for the highlight.
        assert!(session.term.selection.is_some());
        assert!(
            session
                .timeline
                .block(TWO)
                .unwrap()
                .header
                .selection
                .is_some()
        );
        assert!(
            session
                .timeline
                .block(ONE)
                .unwrap()
                .header
                .selection
                .is_none()
        );
    }

    #[test]
    fn upward_drag_selects_the_same_text() {
        let mut session = two_blocks();
        select(
            &mut session,
            at(RowSource::Output(TWO), 0, 1, Side::Right),
            at(RowSource::Output(ONE), 1, 0, Side::Left),
        );
        assert_eq!(copied(&session).as_deref(), Some("1b\n$ two\n2a"));
    }

    #[test]
    fn summary_end_takes_the_whole_hidden_output() {
        let mut session = two_blocks();
        let summary = RowSource::Summary(ONE);
        // From above: down to the summary selects the output to its end.
        select(
            &mut session,
            at(RowSource::Header(ONE), 0, 2, Side::Left),
            at(summary, 0, 0, Side::Left),
        );
        assert_eq!(copied(&session).as_deref(), Some("one\n1a\n1b"));
        // From below: up to the summary selects the output from its start.
        select(
            &mut session,
            at(RowSource::Header(TWO), 0, 4, Side::Right),
            at(summary, 0, 5, Side::Right),
        );
        assert_eq!(copied(&session).as_deref(), Some("1a\n1b\n$ two"));
    }

    #[test]
    fn block_selection_keeps_its_columns_in_every_grid() {
        let mut session = two_blocks();
        let mut selection = TimelineSelection::new(
            SelectionType::Block,
            at(RowSource::Header(ONE), 0, 1, Side::Left),
            &session.timeline,
            &session.term,
        );
        selection.update(at(RowSource::Output(ONE), 1, 3, Side::Right), &session.term);
        apply(Some(&selection), &mut session.term, &mut session.timeline);
        // Columns 1 to 3 of "$ one", "1a" and "1b".
        assert_eq!(copied(&session).as_deref(), Some(" on\na\nb"));
    }

    #[test]
    fn apply_clears_the_grids_left_by_the_selection() {
        let mut session = two_blocks();
        select(
            &mut session,
            at(RowSource::Header(ONE), 0, 0, Side::Left),
            at(RowSource::Live, 0, 1, Side::Right),
        );
        let output = RowSource::Output(TWO);
        select(
            &mut session,
            at(output, 0, 0, Side::Left),
            at(output, 0, 1, Side::Right),
        );
        assert!(session.term.selection.is_none());
        assert!(
            session
                .timeline
                .block(ONE)
                .unwrap()
                .output
                .selection
                .is_none()
        );
        assert_eq!(copied(&session).as_deref(), Some("2a"));
        assert!(clear(&mut session.term, &mut session.timeline));
        assert_eq!(copied(&session), None);
        assert!(!clear(&mut session.term, &mut session.timeline));
    }

    #[test]
    fn emptiness() {
        let mut session = two_blocks();
        let output = RowSource::Output(ONE);
        let click = at(output, 0, 1, Side::Left);
        let selection = TimelineSelection::new(
            SelectionType::Simple,
            click,
            &session.timeline,
            &session.term,
        );
        assert!(selection.is_empty(&session.timeline, &session.term));
        let selection = select(
            &mut session,
            click,
            at(RowSource::Header(TWO), 0, 0, Side::Left),
        );
        assert!(!selection.is_empty(&session.timeline, &session.term));
        // An end on an evicted block: nothing is selected.
        let gone = at(RowSource::Output(BlockId(42)), 0, 0, Side::Left);
        let selection = TimelineSelection::new(
            SelectionType::Simple,
            gone,
            &session.timeline,
            &session.term,
        );
        assert!(selection.parts(&session.timeline, &session.term).is_empty());
        assert!(selection.is_empty(&session.timeline, &session.term));
    }

    #[test]
    fn live_end_follows_its_text_when_output_scrolls() {
        let mut session = Session::sized(20, 3, 1000);
        session.feed("x\r\ny\r\nz");
        let live = RowSource::Live;
        let mut selection = TimelineSelection::new(
            SelectionType::Simple,
            at(live, 1, 0, Side::Left),
            &session.timeline,
            &session.term,
        );
        selection.update(at(live, 1, 0, Side::Right), &session.term);
        // Two new rows push "y" into the history.
        session.feed("\r\nw\r\nv");
        apply(Some(&selection), &mut session.term, &mut session.timeline);
        assert_eq!(copied(&session).as_deref(), Some("y"));
    }

    #[test]
    fn live_selection_goes_stale_when_its_grid_is_detached() {
        let mut session = two_blocks();
        let selection = select(
            &mut session,
            at(RowSource::Live, 0, 0, Side::Left),
            at(RowSource::Live, 0, 1, Side::Right),
        );
        let frozen = select(
            &mut session,
            at(RowSource::Output(ONE), 0, 0, Side::Left),
            at(RowSource::Output(ONE), 0, 1, Side::Right),
        );
        assert!(selection.covers_live(&session.timeline));
        assert!(!selection.is_stale(&session.timeline));
        session.feed(&crate::timeline::testing::start("ls"));
        assert!(selection.is_stale(&session.timeline));
        assert!(!frozen.covers_live(&session.timeline));
        assert!(!frozen.is_stale(&session.timeline));
    }
}
