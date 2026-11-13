//! Rows shown in the window over the timeline: which row of which block each view row
//! displays.
//!
//! The timeline is a column of cards, oldest at the top, the current block at the
//! bottom. A card holds its block's header rows, then its output rows, or one summary
//! row when the block is folded. A block never takes more than [`cap`] rows: a taller
//! one shows its header and the last output rows that fit, shifted up by its own scroll.
//!
//! Heights are counted in half-lines: a row takes two, the gap between two cards one.
//! Positions are measured from the bottom of the timeline: the view shows the half-lines
//! from `view_top - budget` to `view_top`, and only whole rows. At rest (`scroll` 0),
//! `view_top` is the timeline height when it fits in the view (the cards start at the
//! top, blank rows below), the view height otherwise (anchored at the bottom), or the
//! top of the pinned block when a screen clear pinned one and the rest fits. Scrolling
//! adds to it.
//!
//! On the alternate screen the view is the live screen, full window, with no card.

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::{Term, TermMode};

use crate::timeline::{BlockId, Phase, Timeline, content_rows};

/// Where a view row takes its cells from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowSource {
    /// The frozen header of a block (finished, or the current one once it runs).
    Header(BlockId),
    /// The frozen output of a finished block.
    Output(BlockId),
    /// The session's live grid: the current block's prompt, or its output once it runs.
    Live,
    /// The summary row of a folded block, which stands for its hidden output rows.
    Summary(BlockId),
}

/// One view row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewRow {
    pub source: RowSource,
    /// Block the row belongs to; `None` on the alternate screen, where there is no card.
    pub block: Option<BlockId>,
    /// Line shown in the source's grid (live history lines are negative). Meaningless on
    /// a summary row.
    pub line: Line,
    /// On a summary row, the output rows the fold hides.
    pub hidden: usize,
    /// Offset of the row from the view top, in half-lines.
    pub top: u32,
    /// A gap of half a line separates the row from the row above: it starts a card
    /// below a row of another card.
    pub gap_before: bool,
}

/// Half-lines taken by a row.
const ROW_UNITS: usize = 2;
/// Half-lines taken by the gap between two cards.
const GAP_UNITS: usize = 1;
/// View lines kept out of a block's cap: room for the gap above its card, so a capped
/// block always fits whole in the view.
const CAP_MARGIN: usize = 1;

/// Most rows a block shows: the view lines minus the room for a gap, at least two (a
/// header row and an output row).
pub fn cap(lines: usize) -> usize {
    lines.saturating_sub(CAP_MARGIN).max(2)
}

/// Consecutive rows of one source.
#[derive(Debug, Clone, Copy)]
struct Part {
    source: RowSource,
    /// Line of the first row.
    first: Line,
    rows: usize,
}

impl Part {
    fn line(&self, index: usize) -> Line {
        Line(self.first.0 + index as i32)
    }
}

/// A block as the view sees it, finished or current.
#[derive(Debug, Clone, Copy)]
struct Shape {
    id: BlockId,
    header: Option<Part>,
    /// Output rows; for the current block, the live grid (its prompt before it runs).
    body: Option<Part>,
    folded: bool,
    /// Body rows hidden below.
    scroll: usize,
}

/// What a block shows under its cap.
#[derive(Debug, Clone, Copy)]
struct Shown {
    /// First header row shown, and how many.
    header: (usize, usize),
    body: Body,
}

#[derive(Debug, Clone, Copy)]
enum Body {
    None,
    /// The summary row of a folded block, with the rows it hides.
    Summary(usize),
    /// First body row shown, and how many.
    Rows(usize, usize),
}

impl Shape {
    fn header_rows(&self) -> usize {
        self.header.map_or(0, |part| part.rows)
    }

    fn body_rows(&self) -> usize {
        self.body.map_or(0, |part| part.rows)
    }

    /// Folded only with a header to show above the summary and output to hide.
    fn is_folded(&self) -> bool {
        self.folded && self.header.is_some() && self.body_rows() > 0
    }

    /// Body rows shown under `cap`, and the most the body can scroll.
    fn body_window(&self, cap: usize) -> (usize, usize) {
        let header = self.header_shown(cap);
        let shown = self.body_rows().min(cap - header);
        (shown, self.body_rows() - shown)
    }

    /// Header rows shown: all of them, or the last ones leaving one row below.
    fn header_shown(&self, cap: usize) -> usize {
        let below = usize::from(self.body_rows() > 0);
        self.header_rows().min(cap - below)
    }

    fn shown(&self, cap: usize) -> Shown {
        let header = self.header_shown(cap);
        let body = if self.is_folded() {
            Body::Summary(self.body_rows())
        } else if self.body_rows() == 0 {
            Body::None
        } else {
            let (shown, max_scroll) = self.body_window(cap);
            let scroll = self.scroll.min(max_scroll);
            Body::Rows(self.body_rows() - shown - scroll, shown)
        };
        Shown {
            header: (self.header_rows() - header, header),
            body,
        }
    }

    fn rows(&self, cap: usize) -> usize {
        let shown = self.shown(cap);
        shown.header.1
            + match shown.body {
                Body::None => 0,
                Body::Summary(_) => 1,
                Body::Rows(_, count) => count,
            }
    }

    fn units(&self, cap: usize) -> usize {
        self.rows(cap) * ROW_UNITS
    }

    /// The rows shown, top to bottom, as `(source, line, hidden)`.
    fn row_sources(&self, cap: usize) -> Vec<(RowSource, Line, usize)> {
        let shown = self.shown(cap);
        let mut rows = Vec::new();
        if let Some(header) = self.header {
            let (first, count) = shown.header;
            rows.extend((first..first + count).map(|i| (header.source, header.line(i), 0)));
        }
        match (shown.body, self.body) {
            (Body::Summary(hidden), _) => rows.push((RowSource::Summary(self.id), Line(0), hidden)),
            (Body::Rows(first, count), Some(body)) => {
                rows.extend((first..first + count).map(|i| (body.source, body.line(i), 0)))
            }
            _ => {}
        }
        rows
    }
}

/// Rows of the live grid that belong to the current block: its history, then the
/// screen down to the cursor row or the last row holding text, at least one.
pub fn live_rows<L>(live: &Term<L>) -> usize {
    let grid = live.grid();
    let cursor = grid.history_size() + grid.cursor.point.line.0.max(0) as usize + 1;
    content_rows(grid).max(cursor)
}

/// The blocks, oldest first, the current one last.
fn shapes<L>(timeline: &Timeline, live: &Term<L>) -> Vec<Shape> {
    let mut shapes: Vec<Shape> = timeline
        .blocks()
        .iter()
        .map(|block| Shape {
            id: block.id,
            header: Some(Part {
                source: RowSource::Header(block.id),
                first: Line(0),
                rows: block.header.screen_lines(),
            }),
            body: (!block.output_is_empty()).then(|| Part {
                source: RowSource::Output(block.id),
                first: Line(0),
                rows: block.output.screen_lines(),
            }),
            folded: block.folded,
            scroll: block.scroll,
        })
        .collect();
    let current = timeline.current();
    let header = match current.phase {
        Phase::Prompt => None,
        Phase::Running => current.header.as_ref().map(|header| Part {
            source: RowSource::Header(current.id),
            first: Line(0),
            rows: header.screen_lines(),
        }),
    };
    shapes.push(Shape {
        id: current.id,
        header,
        body: Some(Part {
            source: RowSource::Live,
            first: Line(-(live.grid().history_size() as i32)),
            rows: live_rows(live),
        }),
        folded: current.folded,
        scroll: current.scroll,
    });
    shapes
}

/// Heights of the timeline for a view of `lines` lines.
struct Geometry {
    cap: usize,
    /// View height, in half-lines.
    budget: usize,
    /// Height of the timeline, in half-lines.
    total: usize,
    /// Top of the view at rest, in half-lines from the timeline bottom.
    rest_top: usize,
}

impl Geometry {
    fn new(shapes: &[Shape], pinned: Option<BlockId>, lines: usize) -> Self {
        let cap = cap(lines);
        let budget = lines * ROW_UNITS;
        let total = tail_units(shapes, shapes.first().map(|shape| shape.id), cap);
        let anchor = pinned
            .filter(|id| shapes.iter().any(|shape| shape.id == *id))
            .map_or(total, |id| tail_units(shapes, Some(id), cap));
        Geometry {
            cap,
            budget,
            total,
            rest_top: anchor.min(budget),
        }
    }

    fn max_scroll(&self) -> usize {
        self.total - self.rest_top
    }
}

/// Height in half-lines from the top of block `from` to the bottom of the timeline,
/// the gaps included; 0 without `from`.
fn tail_units(shapes: &[Shape], from: Option<BlockId>, cap: usize) -> usize {
    let Some(from) = from else {
        return 0;
    };
    let tail = shapes.iter().filter(|shape| shape.id >= from);
    let count = tail.clone().count();
    tail.map(|shape| shape.units(cap)).sum::<usize>() + count.saturating_sub(1) * GAP_UNITS
}

fn alternate<L>(live: &Term<L>) -> bool {
    live.mode().contains(TermMode::ALT_SCREEN)
}

/// Rows of the view, top to bottom, for a view of `lines` lines over `timeline`, whose
/// current block lives in `live`. Blank space is left out: below the cards when they
/// do not fill the view, and in the gaps.
pub fn rows<L>(timeline: &Timeline, live: &Term<L>, lines: usize) -> Vec<ViewRow> {
    if alternate(live) {
        return (0..live.screen_lines())
            .map(|index| ViewRow {
                source: RowSource::Live,
                block: None,
                line: Line(index as i32),
                hidden: 0,
                top: (ROW_UNITS * index) as u32,
                gap_before: false,
            })
            .collect();
    }
    let shapes = shapes(timeline, live);
    let geometry = Geometry::new(&shapes, timeline.pinned(), lines);
    let view_top = geometry.rest_top + timeline.scroll.min(geometry.max_scroll());
    let view_bottom = view_top as isize - geometry.budget as isize;

    // Newest block first, each block's rows top to bottom.
    let mut cards: Vec<Vec<ViewRow>> = Vec::new();
    let mut bottom = 0;
    for shape in shapes.iter().rev() {
        if bottom >= view_top {
            break;
        }
        let upper = bottom + shape.units(geometry.cap);
        if upper as isize > view_bottom {
            let mut card = Vec::new();
            for (index, (source, line, hidden)) in
                shape.row_sources(geometry.cap).into_iter().enumerate()
            {
                let row_top = upper - ROW_UNITS * index;
                let row_bottom = row_top - ROW_UNITS;
                if row_top <= view_top && row_bottom as isize >= view_bottom {
                    card.push(ViewRow {
                        source,
                        block: Some(shape.id),
                        line,
                        hidden,
                        top: (view_top - row_top) as u32,
                        gap_before: false,
                    });
                }
            }
            if !card.is_empty() {
                cards.push(card);
            }
        }
        bottom = upper + GAP_UNITS;
    }
    let mut rows: Vec<ViewRow> = Vec::new();
    for mut card in cards.into_iter().rev() {
        card[0].gap_before = !rows.is_empty();
        rows.append(&mut card);
    }
    rows
}

/// What the view looked like at the last [`follow`], to keep it still while output
/// arrives.
#[derive(Debug, Clone, Copy, Default)]
pub struct Anchor {
    /// The block that was current, and the height from its top to the timeline bottom.
    tail: Option<(BlockId, usize)>,
    /// Top of the view at rest.
    rest_top: usize,
    /// The current block, its phase and its body rows.
    current: Option<(BlockId, Phase, usize)>,
}

/// Keeps the view still while the timeline grows, before the rows are computed for a
/// new frame: when scrolled up, the timeline scroll grows by the half-lines added below
/// the view since the last call; when the current block is scrolled up, its scroll
/// grows by the rows its output gained. Both scrolls are clamped to their range.
pub fn follow<L>(timeline: &mut Timeline, live: &Term<L>, lines: usize, anchor: &mut Anchor) {
    if alternate(live) {
        return;
    }
    let shapes = shapes(timeline, live);
    let current = *shapes.last().expect("the current block is always there");
    let geometry = Geometry::new(&shapes, timeline.pinned(), lines);
    let phase = timeline.current().phase;

    // The current block first: its scroll does not change its height.
    let body_rows = current.body_rows();
    let block = timeline.current_mut();
    if let Some((id, was_phase, rows)) = anchor.current
        && block.scroll > 0
        && id == block.id
        && was_phase == phase
    {
        block.scroll = block
            .scroll
            .saturating_add_signed(body_rows as isize - rows as isize);
    }
    block.scroll = block.scroll.min(current.body_window(geometry.cap).1);

    if let Some((id, units)) = anchor.tail
        && timeline.scroll > 0
    {
        let grown = tail_units(&shapes, Some(id), geometry.cap) as isize - units as isize;
        let rest_moved = geometry.rest_top as isize - anchor.rest_top as isize;
        timeline.scroll = timeline.scroll.saturating_add_signed(grown - rest_moved);
    }
    timeline.scroll = timeline.scroll.min(geometry.max_scroll());

    *anchor = Anchor {
        tail: Some((current.id, current.units(geometry.cap))),
        rest_top: geometry.rest_top,
        current: Some((current.id, phase, body_rows)),
    };
}

/// The shape of block `id`, if it is still in the timeline.
fn shape<L>(timeline: &Timeline, live: &Term<L>, id: BlockId) -> Option<Shape> {
    shapes(timeline, live)
        .into_iter()
        .find(|shape| shape.id == id)
}

/// Most body rows block `id` can hide below: 0 when its output fits under its cap, or
/// when it is folded.
pub fn max_block_scroll<L>(
    timeline: &Timeline,
    live: &Term<L>,
    id: BlockId,
    lines: usize,
) -> usize {
    shape(timeline, live, id)
        .filter(|shape| !shape.is_folded())
        .map_or(0, |shape| shape.body_window(cap(lines)).1)
}

/// Whether block `id` shows only part of its output, so its own scroll can move.
pub fn can_scroll<L>(timeline: &Timeline, live: &Term<L>, id: BlockId, lines: usize) -> bool {
    max_block_scroll(timeline, live, id, lines) > 0
}

/// Scrolls the output of block `id` by `delta` rows (positive shows older rows),
/// stopping at its ends, and reveals the whole block: a capped block anchored at the
/// bottom has its header cut by the view top, and scrolling its output must not leave
/// the command out of sight. Scrolling down once the output is at its bottom end moves
/// the timeline instead, back to the blocks below. Returns the rows of output actually
/// scrolled.
pub fn scroll_block<L>(
    timeline: &mut Timeline,
    live: &Term<L>,
    id: BlockId,
    lines: usize,
    delta: isize,
) -> isize {
    let scrolled = scroll_output(timeline, live, id, lines, delta);
    if delta < 0 && scrolled == 0 {
        scroll_timeline(timeline, live, lines, delta * ROW_UNITS as isize);
    } else {
        reveal(timeline, live, lines, id);
    }
    scrolled
}

fn scroll_output<L>(
    timeline: &mut Timeline,
    live: &Term<L>,
    id: BlockId,
    lines: usize,
    delta: isize,
) -> isize {
    let max = max_block_scroll(timeline, live, id, lines);
    let scroll = if id == timeline.current().id {
        &mut timeline.current_mut().scroll
    } else if let Some(block) = timeline.block_mut(id) {
        &mut block.scroll
    } else {
        return 0;
    };
    let before = (*scroll).min(max);
    *scroll = before.saturating_add_signed(delta).min(max);
    *scroll as isize - before as isize
}

/// What a wheel step or a page key scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollTarget {
    /// The output of a block that shows only part of it.
    Block(BlockId),
    /// The whole timeline.
    Timeline,
}

/// What `Shift+PageUp/PageDown` scrolls: the selected block when it can scroll
/// (`scrollable`), otherwise the timeline.
pub fn page_target(
    selected: Option<BlockId>,
    scrollable: impl Fn(BlockId) -> bool,
) -> ScrollTarget {
    match selected {
        Some(id) if scrollable(id) => ScrollTarget::Block(id),
        _ => ScrollTarget::Timeline,
    }
}

/// Most half-lines the timeline can scroll up from rest.
pub fn max_timeline_scroll<L>(timeline: &Timeline, live: &Term<L>, lines: usize) -> usize {
    if alternate(live) {
        return 0;
    }
    let shapes = shapes(timeline, live);
    Geometry::new(&shapes, timeline.pinned(), lines).max_scroll()
}

/// Scrolls the timeline by `delta` half-lines (positive shows older blocks), stopping at
/// its ends. Returns the half-lines actually scrolled.
pub fn scroll_timeline<L>(
    timeline: &mut Timeline,
    live: &Term<L>,
    lines: usize,
    delta: isize,
) -> isize {
    let max = max_timeline_scroll(timeline, live, lines);
    let before = timeline.scroll.min(max);
    timeline.scroll = before.saturating_add_signed(delta).min(max);
    timeline.scroll as isize - before as isize
}

/// Rows of the view for a frame or an input event: the view follows the timeline
/// ([`follow`]), and the frozen blocks it shows are reflowed for the live columns
/// first, never every block at once.
pub fn prepare<L>(
    timeline: &mut Timeline,
    live: &Term<L>,
    lines: usize,
    anchor: &mut Anchor,
) -> Vec<ViewRow> {
    let columns = live.columns();
    loop {
        follow(timeline, live, lines, anchor);
        let rows = rows(timeline, live, lines);
        let mut reflowed = false;
        let mut previous = None;
        for id in rows.iter().filter_map(|row| row.block) {
            if previous != Some(id) {
                reflowed |= timeline.reflow(id, columns);
                previous = Some(id);
            }
        }
        if !reflowed {
            return rows;
        }
    }
}

/// Scrolls the timeline the least so that block `id` is in the view, its top first.
pub fn reveal<L>(timeline: &mut Timeline, live: &Term<L>, lines: usize, id: BlockId) {
    if alternate(live) {
        return;
    }
    let shapes = shapes(timeline, live);
    let Some(shape) = shapes.iter().find(|shape| shape.id == id) else {
        return;
    };
    let geometry = Geometry::new(&shapes, timeline.pinned(), lines);
    let top = tail_units(&shapes, Some(id), geometry.cap);
    let bottom = top - shape.units(geometry.cap);
    let view_top = geometry.rest_top + timeline.scroll.min(geometry.max_scroll());
    let new_top = if top > view_top {
        top
    } else if bottom + geometry.budget < view_top {
        // A block always fits under its cap: its bottom on the view bottom.
        bottom + geometry.budget
    } else {
        return;
    };
    timeline.scroll = new_top
        .saturating_sub(geometry.rest_top)
        .min(geometry.max_scroll());
}

/// Frozen terminal shown by `source`: a block's header or output. `None` for the live
/// grid and summary rows, or a block no longer in the timeline.
pub fn frozen_term(timeline: &Timeline, source: RowSource) -> Option<&Term<VoidListener>> {
    match source {
        RowSource::Header(id) => match timeline.block(id) {
            Some(block) => Some(&block.header),
            None => {
                let current = timeline.current();
                current.header.as_ref().filter(|_| current.id == id)
            }
        },
        RowSource::Output(id) => timeline.block(id).map(|block| &block.output),
        RowSource::Live | RowSource::Summary(_) => None,
    }
}

/// Grids shown by the view, top to bottom: each block's header, then its output when
/// it is not empty, the live grid last. Only the live grid on the alternate screen.
pub fn sources<L>(timeline: &Timeline, live: &Term<L>) -> Vec<RowSource> {
    if alternate(live) {
        return vec![RowSource::Live];
    }
    shapes(timeline, live)
        .into_iter()
        .flat_map(|shape| shape.header.into_iter().chain(shape.body))
        .map(|part| part.source)
        .collect()
}

/// Source and point of the cell at `column` on view row `index`, clamped to the last
/// row; `None` for an empty view.
pub fn point(rows: &[ViewRow], index: usize, column: usize) -> Option<(RowSource, Point)> {
    let row = rows.get(index).or(rows.last())?;
    Some((row.source, Point::new(row.line, Column(column))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::testing::{Session, end, start, texts};
    use alacritty_terminal::event::VoidListener;

    /// Text of each view row: `[s] n hidden` for a summary row.
    fn view(session: &Session, rows: &[ViewRow]) -> Vec<String> {
        rows.iter()
            .map(|row| {
                let text = |term: &Term<VoidListener>| {
                    let top = -(term.grid().history_size() as i32);
                    texts(term)[(row.line.0 - top) as usize].clone()
                };
                let timeline = &session.timeline;
                match row.source {
                    RowSource::Live => text(&session.term),
                    RowSource::Header(id) => match timeline.block(id) {
                        Some(block) => text(&block.header),
                        None => text(timeline.current().header.as_ref().expect("header")),
                    },
                    RowSource::Output(id) => text(&timeline.block(id).expect("block").output),
                    RowSource::Summary(_) => format!("[s] {}", row.hidden),
                }
            })
            .collect()
    }

    fn tops(rows: &[ViewRow]) -> Vec<(u32, bool)> {
        rows.iter().map(|row| (row.top, row.gap_before)).collect()
    }

    fn numbered(name: &str, count: usize) -> Vec<String> {
        (1..=count).map(|row| format!("{name}{row}")).collect()
    }

    fn run(session: &mut Session, name: &str, output: usize) {
        let output = numbered(name, output);
        let output: Vec<&str> = output.iter().map(String::as_str).collect();
        session.command(name, &output, 0);
    }

    /// A session with a 5-line window and the prompt `$ ` after the commands.
    fn session(commands: &[(&str, usize)]) -> Session {
        let mut session = Session::new(1000);
        for (name, output) in commands {
            run(&mut session, name, *output);
        }
        session.feed("$ ");
        session
    }

    fn rows_of(session: &Session, lines: usize) -> Vec<ViewRow> {
        rows(&session.timeline, &session.term, lines)
    }

    /// Rows of the view scrolled to the top of the timeline.
    fn top_rows(session: &mut Session, lines: usize) -> Vec<ViewRow> {
        let max = max_timeline_scroll(&session.timeline, &session.term, lines);
        scroll_timeline(&mut session.timeline, &session.term, lines, max as isize);
        rows_of(session, lines)
    }

    #[test]
    fn cards_start_at_the_top_when_they_fit() {
        let session = session(&[("a", 1)]);
        let rows = rows_of(&session, 8);
        assert_eq!(view(&session, &rows), ["$ a", "a1", "$"]);
        assert_eq!(tops(&rows), [(0, false), (2, false), (5, true)]);
        assert_eq!(rows[0].source, RowSource::Header(BlockId(0)));
        assert_eq!(rows[1].source, RowSource::Output(BlockId(0)));
        assert_eq!(rows[2].source, RowSource::Live);
        assert_eq!(rows[2].block, Some(BlockId(1)));
    }

    #[test]
    fn a_tall_timeline_is_anchored_at_the_bottom_with_gaps() {
        let session = session(&[("a", 1), ("b", 1), ("c", 1)]);
        // 8 lines: 16 half-lines. From the bottom: "$" (2), gap, "$ c" "c1" (4), gap,
        // "$ b" "b1" (4), gap: 13; "a1" takes 2 more, "$ a" does not fit.
        let rows = rows_of(&session, 8);
        assert_eq!(view(&session, &rows), ["a1", "$ b", "b1", "$ c", "c1", "$"]);
        assert_eq!(
            tops(&rows),
            [
                (1, false),
                (4, true),
                (6, false),
                (9, true),
                (11, false),
                (14, true)
            ]
        );
    }

    #[test]
    fn a_tall_block_shows_its_header_and_the_last_output_rows() {
        let mut session = session(&[("a", 10)]);
        // 5 lines: cap 4, the header then the last three output rows. At rest the view
        // is anchored at the bottom and the header is cut: scrolled to the top here.
        let rows = top_rows(&mut session, 5);
        assert_eq!(view(&session, &rows), ["$ a", "a8", "a9", "a10"]);
        assert_eq!(cap(5), 4);
        assert!(can_scroll(&session.timeline, &session.term, BlockId(0), 5));
        assert_eq!(
            max_block_scroll(&session.timeline, &session.term, BlockId(0), 5),
            7
        );
    }

    #[test]
    fn a_header_taller_than_the_cap_keeps_its_last_rows() {
        let mut session = Session::new(1000);
        session.feed(&format!("p1\r\np2\r\np3\r\np4\r\n$ a\r\n{}", start("a")));
        session.feed("a1\r\na2\r\n");
        session.feed(&end(0));
        session.feed("$ ");
        let rows = top_rows(&mut session, 5);
        // Header of five rows under a cap of 4: its last three, then one output row.
        assert_eq!(view(&session, &rows), ["p3", "p4", "$ a", "a2"]);
    }

    #[test]
    fn block_scroll_shifts_the_output_and_stops_at_its_ends() {
        let mut session = session(&[("a", 10)]);
        let id = BlockId(0);
        let (timeline, live) = (&mut session.timeline, &session.term);
        assert_eq!(scroll_block(timeline, live, id, 5, 2), 2);
        assert_eq!(scroll_block(timeline, live, id, 5, 10), 5);
        assert_eq!(timeline.block(id).unwrap().scroll, 7);
        let rows = rows_of(&session, 5);
        assert_eq!(view(&session, &rows), ["$ a", "a1", "a2", "a3"]);
        let (timeline, live) = (&mut session.timeline, &session.term);
        assert_eq!(scroll_block(timeline, live, id, 5, -20), -7);
        assert_eq!(scroll_block(timeline, live, id, 5, -1), 0);
        // A short block cannot scroll.
        let mut session = self::session(&[("a", 2)]);
        let (timeline, live) = (&mut session.timeline, &session.term);
        assert!(!can_scroll(timeline, live, BlockId(0), 5));
        assert_eq!(scroll_block(timeline, live, BlockId(0), 5, 1), 0);
    }

    #[test]
    fn scrolling_a_capped_block_reveals_its_header() {
        let mut session = session(&[("a", 10)]);
        let id = BlockId(0);
        // At rest the prompt card below pushes the capped block up: its header is cut.
        let rows = rows_of(&session, 5);
        assert!(!rows.iter().any(|row| row.source == RowSource::Header(id)));
        // At the top end of its output, a wheel step up still brings the command back.
        let (timeline, live) = (&mut session.timeline, &session.term);
        scroll_block(timeline, live, id, 5, 7);
        assert_eq!(scroll_block(timeline, live, id, 5, 1), 0);
        timeline.scroll = 0;
        scroll_block(timeline, live, id, 5, 1);
        let rows = rows_of(&session, 5);
        assert_eq!(view(&session, &rows), ["$ a", "a1", "a2", "a3"]);
        // Scrolling down as well: the header stays in sight.
        let (timeline, live) = (&mut session.timeline, &session.term);
        scroll_block(timeline, live, id, 5, -3);
        let rows = rows_of(&session, 5);
        assert_eq!(view(&session, &rows), ["$ a", "a4", "a5", "a6"]);
        // At the bottom end of its output, scrolling down goes back to the prompt.
        let (timeline, live) = (&mut session.timeline, &session.term);
        scroll_block(timeline, live, id, 5, -4);
        assert_eq!(timeline.block(id).unwrap().scroll, 0);
        assert!(timeline.scroll > 0);
        scroll_block(timeline, live, id, 5, -10);
        assert_eq!(timeline.scroll, 0);
        let rows = rows_of(&session, 5);
        assert_eq!(view(&session, &rows).last().map(String::as_str), Some("$"));
    }

    #[test]
    fn folded_block_shows_its_header_and_a_summary() {
        let mut session = session(&[("a", 10), ("b", 1)]);
        session.timeline.toggle_fold(BlockId(0));
        let rows = rows_of(&session, 8);
        assert_eq!(view(&session, &rows), ["$ a", "[s] 10", "$ b", "b1", "$"]);
        assert_eq!(rows[1].source, RowSource::Summary(BlockId(0)));
        assert!(!can_scroll(&session.timeline, &session.term, BlockId(0), 8));
    }

    #[test]
    fn timeline_scroll_moves_the_view_and_stops_at_the_top() {
        let mut session = session(&[("a", 1), ("b", 1), ("c", 1)]);
        // Total: "$ a" "a1" "$ b" "b1" "$ c" "c1" "$" = 14 + 3 gaps = 17 half-lines.
        let max = max_timeline_scroll(&session.timeline, &session.term, 8);
        assert_eq!(max, 1);
        let (timeline, live) = (&mut session.timeline, &session.term);
        assert_eq!(scroll_timeline(timeline, live, 8, 5), 1);
        let rows = rows_of(&session, 8);
        assert_eq!(
            view(&session, &rows),
            ["$ a", "a1", "$ b", "b1", "$ c", "c1"]
        );
        assert_eq!(rows[0].top, 0);
        let (timeline, live) = (&mut session.timeline, &session.term);
        assert_eq!(scroll_timeline(timeline, live, 8, -5), -1);
    }

    #[test]
    fn the_view_stays_still_while_output_arrives_when_scrolled_up() {
        let mut session = Session::new(1000);
        for name in ["a", "b", "c", "d"] {
            run(&mut session, name, 1);
        }
        session.feed(&format!("$ e\r\n{}", start("e")));
        let mut anchor = Anchor::default();
        follow(&mut session.timeline, &session.term, 8, &mut anchor);
        scroll_timeline(&mut session.timeline, &session.term, 8, 6);
        let before = view(&session, &rows_of(&session, 8));

        session.feed("e1\r\ne2\r\n");
        follow(&mut session.timeline, &session.term, 8, &mut anchor);
        assert_eq!(view(&session, &rows_of(&session, 8)), before);

        // The block ends and a new prompt comes: still in place.
        session.feed(&end(0));
        session.feed("$ ");
        follow(&mut session.timeline, &session.term, 8, &mut anchor);
        assert_eq!(view(&session, &rows_of(&session, 8)), before);
    }

    #[test]
    fn the_view_follows_the_bottom_at_rest() {
        let mut session = Session::new(1000);
        run(&mut session, "a", 3);
        session.feed(&format!("$ b\r\n{}", start("b")));
        let mut anchor = Anchor::default();
        follow(&mut session.timeline, &session.term, 5, &mut anchor);
        session.feed("b1\r\nb2\r\nb3\r\n");
        follow(&mut session.timeline, &session.term, 5, &mut anchor);
        assert_eq!(session.timeline.scroll, 0);
        let rows = rows_of(&session, 5);
        assert_eq!(view(&session, &rows).last().unwrap(), "");
        assert_eq!(view(&session, &rows)[rows.len() - 2], "b3");
    }

    #[test]
    fn the_current_block_follows_its_output_unless_scrolled() {
        let mut session = Session::new(1000);
        session.feed(&format!("$ a\r\n{}", start("a")));
        session.feed(&numbered("a", 6).join("\r\n"));
        let mut anchor = Anchor::default();
        follow(&mut session.timeline, &session.term, 5, &mut anchor);
        // Cap 4: header and the last three live rows.
        assert_eq!(
            view(&session, &rows_of(&session, 5)),
            ["$ a", "a4", "a5", "a6"]
        );

        let id = session.timeline.current().id;
        scroll_block(&mut session.timeline, &session.term, id, 5, 2);
        assert_eq!(
            view(&session, &rows_of(&session, 5)),
            ["$ a", "a2", "a3", "a4"]
        );
        session.feed("\r\na7\r\na8");
        follow(&mut session.timeline, &session.term, 5, &mut anchor);
        assert_eq!(session.timeline.current().scroll, 4);
        assert_eq!(
            view(&session, &rows_of(&session, 5)),
            ["$ a", "a2", "a3", "a4"]
        );

        // Back at the bottom, it follows again.
        scroll_block(&mut session.timeline, &session.term, id, 5, -10);
        session.feed("\r\na9");
        follow(&mut session.timeline, &session.term, 5, &mut anchor);
        assert_eq!(
            view(&session, &rows_of(&session, 5)),
            ["$ a", "a7", "a8", "a9"]
        );
    }

    #[test]
    fn a_pinned_block_starts_the_view_with_blank_rows_below() {
        let mut session = session(&[("a", 2), ("b", 2)]);
        // Ctrl+L at the prompt: the shell clears the screen and redraws its prompt.
        session.feed("\x1b[H\x1b[2J$ ");
        assert_eq!(
            session.timeline.pinned(),
            Some(session.timeline.current().id)
        );
        let rows = rows_of(&session, 5);
        assert_eq!(view(&session, &rows), ["$"]);
        assert_eq!(tops(&rows), [(0, false)]);
        // The blocks above are still there, by scrolling up.
        let max = max_timeline_scroll(&session.timeline, &session.term, 5);
        assert!(max > 0);
        scroll_timeline(&mut session.timeline, &session.term, 5, max as isize);
        assert_eq!(view(&session, &rows_of(&session, 5))[0], "$ a");
    }

    #[test]
    fn a_pinned_block_taller_than_the_view_is_anchored_at_the_bottom() {
        let mut session = Session::new(1000);
        run(&mut session, "a", 1);
        session.feed(&format!("$ b\r\n{}", start("b")));
        session.feed("\x1b[H\x1b[2J");
        for line in numbered("b", 3) {
            session.feed(&format!("{line}\r\n"));
        }
        session.feed(&end(0));
        session.feed("$ ");
        assert_eq!(session.timeline.pinned(), Some(BlockId(1)));
        // "$ b" and its three rows, then "$": 8 + 1 + 2 = 11 half-lines out of 10.
        let rows = rows_of(&session, 5);
        assert_eq!(view(&session, &rows), ["b1", "b2", "b3", "$"]);
    }

    #[test]
    fn alternate_screen_is_the_live_screen_without_cards() {
        let mut session = session(&[("a", 1)]);
        session.feed("\x1b[?1049h\x1b[Hfull");
        let rows = rows_of(&session, 5);
        assert_eq!(rows.len(), session.term.screen_lines());
        assert!(
            rows.iter()
                .all(|row| row.block.is_none() && !row.gap_before)
        );
        assert!(rows.iter().all(|row| row.source == RowSource::Live));
        assert_eq!(tops(&rows)[..2], [(0, false), (2, false)]);
        assert_eq!(view(&session, &rows)[0], "full");
        assert_eq!(max_timeline_scroll(&session.timeline, &session.term, 5), 0);
    }

    #[test]
    fn without_signals_the_live_grid_is_one_scrollable_card() {
        let mut session = Session::new(1000);
        session.feed(&numbered("l", 8).join("\r\n"));
        let rows = rows_of(&session, 5);
        assert_eq!(view(&session, &rows), ["l5", "l6", "l7", "l8"]);
        assert!(rows.iter().all(|row| row.block == Some(BlockId(0))));
        let id = session.timeline.current().id;
        assert!(can_scroll(&session.timeline, &session.term, id, 5));
        scroll_block(&mut session.timeline, &session.term, id, 5, 4);
        assert_eq!(
            view(&session, &rows_of(&session, 5)),
            ["l1", "l2", "l3", "l4"]
        );
    }

    #[test]
    fn point_maps_a_view_row_to_its_source() {
        let session = session(&[("a", 1)]);
        let rows = rows_of(&session, 8);
        assert_eq!(
            point(&rows, 1, 3),
            Some((
                RowSource::Output(BlockId(0)),
                Point::new(Line(0), Column(3))
            ))
        );
        // Clamped to the last row.
        assert_eq!(
            point(&rows, 9, 0),
            Some((RowSource::Live, Point::new(Line(0), Column(0))))
        );
        assert_eq!(point(&[], 0, 0), None);
    }

    #[test]
    fn prepare_reflows_only_the_blocks_it_shows() {
        let mut session = session(&[("a", 1), ("b", 1), ("c", 1), ("d", 1)]);
        session
            .term
            .resize(alacritty_terminal::term::test::TermSize::new(10, 5));
        let mut anchor = Anchor::default();
        let rows = prepare(&mut session.timeline, &session.term, 5, &mut anchor);
        let columns = |id: u64| {
            let block = session.timeline.block(BlockId(id)).unwrap();
            (block.header.columns(), block.output.columns())
        };
        // Shown: "d1", then the prompt, with "$ d" above and "c1" before it.
        assert_eq!(view(&session, &rows), ["c1", "$ d", "d1", "$"]);
        assert_eq!(columns(0), (20, 20));
        assert_eq!(columns(2), (10, 10));
        assert_eq!(columns(3), (10, 10));
    }

    #[test]
    fn reveal_scrolls_the_least_to_show_a_block() {
        let mut session = session(&[("a", 1), ("b", 1), ("c", 1), ("d", 1)]);
        let (timeline, live) = (&mut session.timeline, &session.term);
        reveal(timeline, live, 5, BlockId(0));
        let rows = rows_of(&session, 5);
        assert_eq!(rows[0].source, RowSource::Header(BlockId(0)));
        assert_eq!(rows[0].line, Line(0));

        // Block 1 is already in view: nothing moves.
        let scroll = session.timeline.scroll;
        reveal(&mut session.timeline, &session.term, 5, BlockId(1));
        assert_eq!(session.timeline.scroll, scroll);

        // Down to block 3, shown whole at the bottom of the view.
        reveal(&mut session.timeline, &session.term, 5, BlockId(3));
        let rows = rows_of(&session, 5);
        assert_eq!(view(&session, &rows)[rows.len() - 2..], ["$ d", "d1"]);
        // An unknown block changes nothing.
        let scroll = session.timeline.scroll;
        reveal(&mut session.timeline, &session.term, 5, BlockId(42));
        assert_eq!(session.timeline.scroll, scroll);
    }

    #[test]
    fn frozen_terms_of_headers_and_outputs() {
        let mut session = session(&[("a", 1)]);
        session.feed(&format!("b\r\n{}", start("b")));
        let timeline = &mut session.timeline;
        let current = timeline.current().id;
        assert!(frozen_term(timeline, RowSource::Header(BlockId(0))).is_some());
        assert!(frozen_term(timeline, RowSource::Output(BlockId(0))).is_some());
        assert!(frozen_term(timeline, RowSource::Header(current)).is_some());
        assert!(frozen_term(timeline, RowSource::Output(current)).is_none());
        assert!(frozen_term(timeline, RowSource::Live).is_none());
        assert!(frozen_term(timeline, RowSource::Summary(BlockId(0))).is_none());
        assert!(frozen_term(timeline, RowSource::Header(BlockId(42))).is_none());
    }

    #[test]
    fn sources_list_the_shown_grids_top_to_bottom() {
        let mut session = session(&[("a", 1), ("b", 0)]);
        let (a, b) = (BlockId(0), BlockId(1));
        assert_eq!(
            sources(&session.timeline, &session.term),
            vec![
                RowSource::Header(a),
                RowSource::Output(a),
                // An empty output shows no row.
                RowSource::Header(b),
                RowSource::Live,
            ]
        );
        // A running command: its frozen header, then the live grid.
        session.feed(&format!("c\r\n{}", start("c")));
        let c = session.timeline.current().id;
        assert_eq!(
            sources(&session.timeline, &session.term)[3..],
            [RowSource::Header(c), RowSource::Live]
        );
        // The alternate screen shows the live grid only.
        session.feed("\x1b[?1049h");
        assert_eq!(
            sources(&session.timeline, &session.term),
            vec![RowSource::Live]
        );
    }

    #[test]
    fn page_keys_scroll_the_selected_block_when_it_can_scroll() {
        let scrollable = |id: BlockId| id == BlockId(2);
        assert_eq!(
            page_target(Some(BlockId(2)), scrollable),
            ScrollTarget::Block(BlockId(2))
        );
        assert_eq!(
            page_target(Some(BlockId(1)), scrollable),
            ScrollTarget::Timeline
        );
        assert_eq!(page_target(None, scrollable), ScrollTarget::Timeline);
    }
}
