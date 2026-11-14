//! Snapshot of the visible terminal content, decoupled from the GPU renderer.
//!
//! Building a [`Frame`] only requires the terminal and timeline locks for a short time;
//! the renderer then works on this plain data. Each view row takes its cells from its
//! source: a frozen block's grid, or the session's live grid.

use std::ops::Range;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Dimensions, Row};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::selection::SelectionRange;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};

use crate::banner::BannerLayout;
use crate::i18n::Lang;
use crate::palette::Palette;
use crate::timeline::{BlockId, Phase, Status, Timeline};
use crate::view::{self, RowSource, ViewRow};

/// A single cell ready to be drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameCell {
    pub c: char,
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    /// Second half of a double-width character: nothing to draw.
    pub spacer: bool,
}

/// Cursor drawn on top of the grid (block cursors are baked into the cells).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameCursor {
    pub column: usize,
    pub line: usize,
    pub shape: CursorShape,
    pub color: Rgb,
}

/// Banner drawn at the top of the window, over the grid.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameBanner {
    pub layout: BannerLayout,
    pub background: Rgb,
    pub text: Rgb,
}

/// Fold state shown by a block's chevron.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fold {
    Folded,
    Unfolded,
}

/// Command block mark of one row, drawn after its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowDecoration {
    /// On the summary row of a folded block, the output rows it hides.
    pub summary: Option<usize>,
}

/// Card drawn behind the consecutive view rows of one block.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    /// View rows covered.
    pub rows: Range<usize>,
    /// The block's first row is not shown (above the view, or cut by the block's cap):
    /// the card's top corners stay off screen.
    pub clipped_top: bool,
    /// Status shown by the stripe and the chevron. `None` for the current block at its
    /// prompt, and for the only block of a shell that sends no signal: a neutral card.
    pub status: Option<Status>,
    /// Fold chevron on the card's top-right corner: a finished block with output whose
    /// first row is in the card.
    pub chevron: Option<Fold>,
    /// The block is the selected one.
    pub selected: bool,
    /// Scroll indicator of a block showing only part of its output.
    pub scroll: Option<ScrollBar>,
}

/// Scroll indicator drawn on a card's right edge, beside its output rows.
#[derive(Debug, Clone, PartialEq)]
pub struct ScrollBar {
    /// View rows of the block's output in the card: the track.
    pub rows: Range<usize>,
    /// Where the shown output rows start in the whole output, as a share of it (0 to 1).
    pub start: f32,
    /// Share of the whole output shown.
    pub length: f32,
}

/// Column where the hidden-rows summary of a row starts: two columns after its last
/// non-blank cell, 0 on a blank row. `blank` tells, cell by cell, whether it is blank.
pub fn summary_column(blank: impl DoubleEndedIterator<Item = bool> + ExactSizeIterator) -> usize {
    let mut blank = blank;
    let len = blank.len();
    blank
        .rposition(|is_blank| !is_blank)
        .map_or(0, |last| (last + 2).min(len))
}

/// Colors of the block marks, resolved from the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockColors {
    pub success: Rgb,
    pub failure: Rgb,
    pub running: Rgb,
    /// `Unknown` status, neutral chevrons and fold summaries.
    pub dim: Rgb,
    /// Card background: the terminal background blended 6 % toward the foreground.
    pub card: Rgb,
    /// Background of the selected block's card: blended 14 %.
    pub card_selected: Rgb,
}

/// Share of the foreground in a card background.
const CARD_BLEND: f32 = 0.06;
/// Share of the foreground in the selected card background.
const CARD_SELECTED_BLEND: f32 = 0.14;

/// `from` moved toward `to` by `amount` (0 to 1), per sRGB channel.
pub fn blend(from: Rgb, to: Rgb, amount: f32) -> Rgb {
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * amount).round() as u8;
    Rgb {
        r: mix(from.r, to.r),
        g: mix(from.g, to.g),
        b: mix(from.b, to.b),
    }
}

impl BlockColors {
    fn resolve(palette: &Palette, overrides: &Colors) -> Self {
        let named = |color| palette.resolve(Color::Named(color), overrides);
        let background = named(NamedColor::Background);
        let foreground = named(NamedColor::Foreground);
        Self {
            success: named(NamedColor::Green),
            failure: named(NamedColor::Red),
            running: named(NamedColor::Yellow),
            dim: named(NamedColor::DimForeground),
            card: blend(background, foreground, CARD_BLEND),
            card_selected: blend(background, foreground, CARD_SELECTED_BLEND),
        }
    }

    pub fn status(&self, status: Status) -> Rgb {
        match status {
            Status::Success => self.success,
            Status::Failure(_) => self.failure,
            Status::Running => self.running,
            Status::Unknown => self.dim,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Visible rows, top to bottom.
    pub rows: Vec<Vec<FrameCell>>,
    /// Block marks of each row of `rows`.
    pub decorations: Vec<RowDecoration>,
    /// Block cards, top to bottom.
    pub cards: Vec<Card>,
    /// Offset of each row of `rows` from the view top, in half-lines (see
    /// [`ViewRow::top`]).
    pub row_tops: Vec<u32>,
    pub block_colors: BlockColors,
    pub cursor: Option<FrameCursor>,
    pub background: Rgb,
    pub banner: Option<FrameBanner>,
    /// Language of the fold summaries.
    pub lang: Lang,
}

impl Frame {
    /// Captures the rows shown by `view` (see [`crate::view::rows`]): frozen rows from
    /// their block's grid in `timeline`, live rows and the cursor from `live`, the
    /// session's terminal. The card of `selected_block` is `selected`.
    pub fn capture<T: EventListener>(
        live: &Term<T>,
        timeline: &Timeline,
        view: &[ViewRow],
        palette: &Palette,
        focused: bool,
        selected_block: Option<BlockId>,
    ) -> Self {
        let selected_block = selected_block.filter(|id| timeline.block(*id).is_some());
        let content = live.renderable_content();
        let overrides = content.colors;
        let columns = live.columns();
        let live_selection = content.selection;

        let background = palette.resolve(Color::Named(NamedColor::Background), overrides);
        let foreground = palette.resolve(Color::Named(NamedColor::Foreground), overrides);
        let blank = FrameCell::blank(foreground, background);
        let style = RowStyle {
            columns,
            palette,
            overrides,
            blank,
        };
        // Selection of each frozen source met so far.
        let mut selections: Vec<(RowSource, Option<SelectionRange>)> = Vec::new();
        let mut rows = Vec::with_capacity(view.len());
        for view_row in view {
            let row = match view_row.source {
                RowSource::Live => {
                    style.row(&live.grid()[view_row.line], view_row.line, live_selection)
                }
                source => match view::frozen_term(timeline, source) {
                    Some(term) => {
                        let selection = match selections.iter().find(|(s, _)| *s == source) {
                            Some((_, selection)) => *selection,
                            None => {
                                let selection =
                                    term.selection.as_ref().and_then(|s| s.to_range(term));
                                selections.push((source, selection));
                                selection
                            }
                        };
                        style.row(&term.grid()[view_row.line], view_row.line, selection)
                    }
                    // A summary row: its text is drawn by the renderer.
                    None => vec![blank; columns],
                },
            };
            rows.push(row);
        }

        let cursor_color = palette.resolve(Color::Named(NamedColor::Cursor), overrides);
        let cursor_point = content.cursor.point;
        let cursor_line = view
            .iter()
            .position(|row| row.source == RowSource::Live && row.line == cursor_point.line);
        let cursor_column = cursor_point.column.0;
        let mut shape = content.cursor.shape;
        if !focused && shape != CursorShape::Hidden {
            shape = CursorShape::HollowBlock;
        }

        let cursor = match cursor_line {
            _ if shape == CursorShape::Hidden || cursor_column >= columns => None,
            None => None,
            Some(line) if shape == CursorShape::Block => {
                // Bake the block cursor into the cell colors.
                let cell = &mut rows[line][cursor_column];
                cell.bg = cursor_color;
                cell.fg = palette.cursor_text;
                if cursor_column + 1 < columns && rows[line][cursor_column + 1].spacer {
                    rows[line][cursor_column + 1].bg = cursor_color;
                }
                None
            }
            Some(line) => Some(FrameCursor {
                column: cursor_column,
                line,
                shape,
                color: cursor_color,
            }),
        };

        let (decorations, mut cards) = marks(live, timeline, view);
        if let Some(id) = selected_block {
            for card in &mut cards {
                card.selected = view[card.rows.start].block == Some(id);
            }
        }
        Self {
            rows,
            decorations,
            cards,
            row_tops: view.iter().map(|row| row.top).collect(),
            block_colors: BlockColors::resolve(palette, overrides),
            cursor,
            background,
            banner: None,
            lang: Lang::default(),
        }
    }
}

impl FrameCell {
    fn blank(fg: Rgb, bg: Rgb) -> Self {
        Self {
            c: ' ',
            fg,
            bg,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            spacer: false,
        }
    }
}

/// What turns a grid row into frame cells.
struct RowStyle<'a> {
    /// Columns of the live grid: a frozen row frozen with fewer is padded with blanks.
    columns: usize,
    palette: &'a Palette,
    overrides: &'a Colors,
    blank: FrameCell,
}

impl RowStyle<'_> {
    /// Cells of `row`, grid line `line` of its source, with the cells of `selection`
    /// highlighted.
    fn row(
        &self,
        row: &Row<Cell>,
        line: Line,
        selection: Option<SelectionRange>,
    ) -> Vec<FrameCell> {
        (0..self.columns)
            .map(|column| {
                if column >= row.len() {
                    return self.blank;
                }
                let point = Point::new(line, Column(column));
                let cell = &row[Column(column)];
                let flags = cell.flags;
                let (mut fg, mut bg) =
                    self.palette
                        .cell_colors(cell.fg, cell.bg, flags, self.overrides);
                if selection.is_some_and(|range| is_selected(&range, point, flags)) {
                    bg = self.palette.selection_background;
                    fg = self.palette.selection_foreground.unwrap_or(fg);
                }
                let spacer =
                    flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
                FrameCell {
                    c: if spacer || cell.c == '\t' {
                        ' '
                    } else {
                        cell.c
                    },
                    fg,
                    bg,
                    bold: flags.contains(Flags::BOLD),
                    italic: flags.contains(Flags::ITALIC),
                    underline: flags.intersects(Flags::ALL_UNDERLINES),
                    strikeout: flags.contains(Flags::STRIKEOUT),
                    spacer,
                }
            })
            .collect()
    }
}

/// Block marks of the rows of `view` and the cards behind them, one card per block,
/// from the blocks of `timeline` whose current one lives in `live`. The alternate
/// screen's rows belong to no block: no card.
pub fn marks<T>(
    live: &Term<T>,
    timeline: &Timeline,
    view: &[ViewRow],
) -> (Vec<RowDecoration>, Vec<Card>) {
    let decorations = view
        .iter()
        .map(|row| RowDecoration {
            summary: matches!(row.source, RowSource::Summary(_)).then_some(row.hidden),
        })
        .collect();
    let live_top = Line(-(live.grid().history_size() as i32));
    let mut cards: Vec<Card> = Vec::new();
    for (index, row) in view.iter().enumerate() {
        let Some(id) = row.block else {
            continue;
        };
        match cards.last_mut() {
            Some(card) if card.rows.end == index && view[card.rows.start].block == Some(id) => {
                card.rows.end = index + 1;
            }
            _ => {
                // The block's first row is above the view, or cut by its cap.
                let clipped_top = !match row.source {
                    RowSource::Header(_) => row.line == Line(0),
                    RowSource::Live => {
                        timeline.current().phase == Phase::Prompt && row.line == live_top
                    }
                    RowSource::Output(_) | RowSource::Summary(_) => false,
                };
                cards.push(Card {
                    rows: index..index + 1,
                    clipped_top,
                    status: card_status(timeline, id),
                    chevron: chevron(timeline, id).filter(|_| !clipped_top),
                    selected: false,
                    scroll: None,
                });
            }
        }
    }
    for card in &mut cards {
        card.scroll = scroll_bar(live, timeline, view, card.rows.clone());
    }
    (decorations, cards)
}

/// Scroll indicator of the card over the view rows `rows`: its block's output rows in
/// the card, when the block shows only part of its output.
fn scroll_bar<T>(
    live: &Term<T>,
    timeline: &Timeline,
    view: &[ViewRow],
    rows: Range<usize>,
) -> Option<ScrollBar> {
    let id = view[rows.start].block?;
    let max = view::max_block_scroll(timeline, live, id, live.screen_lines());
    if max == 0 {
        return None;
    }
    let output = |row: &ViewRow| match row.source {
        RowSource::Output(of) => of == id,
        RowSource::Live => timeline.current().id == id,
        RowSource::Header(_) | RowSource::Summary(_) => false,
    };
    let first = rows.clone().find(|&row| output(&view[row]))?;
    let last = rows.clone().rev().find(|&row| output(&view[row]))?;
    let (total, scroll) = match timeline.block(id) {
        Some(block) => (block.output.screen_lines(), block.scroll),
        None => (view::live_rows(live), timeline.current().scroll),
    };
    let shown = total - max;
    let start = total - shown - scroll.min(max);
    Some(ScrollBar {
        rows: first..last + 1,
        start: start as f32 / total as f32,
        length: shown as f32 / total as f32,
    })
}

/// Whether the card starting on view row `row` of `view` has a fold chevron.
pub fn has_chevron<T>(live: &Term<T>, timeline: &Timeline, view: &[ViewRow], row: usize) -> bool {
    let (_, cards) = marks(live, timeline, view);
    cards
        .iter()
        .any(|card| card.rows.start == row && card.chevron.is_some())
}

/// Status shown by the card of block `id`: none for the current block at its prompt
/// (a neutral card, also the only card of a shell that sends no signal).
fn card_status(timeline: &Timeline, id: BlockId) -> Option<Status> {
    match timeline.block(id) {
        Some(block) => Some(block.status),
        None => (timeline.current().phase == Phase::Running).then_some(Status::Running),
    }
}

/// Fold chevron of block `id`: a finished block with output.
fn chevron(timeline: &Timeline, id: BlockId) -> Option<Fold> {
    let block = timeline
        .block(id)
        .filter(|block| !block.output_is_empty())?;
    Some(if block.folded {
        Fold::Folded
    } else {
        Fold::Unfolded
    })
}

/// Whether the cell at `point` is highlighted by `range`. Both halves of a wide
/// character are highlighted as soon as either half is selected.
fn is_selected(range: &SelectionRange, point: Point, flags: Flags) -> bool {
    range.contains(point)
        || (flags.contains(Flags::WIDE_CHAR)
            && range.contains(Point::new(point.line, point.column + 1)))
        || (flags.contains(Flags::WIDE_CHAR_SPACER)
            && point.column.0 > 0
            && range.contains(Point::new(point.line, point.column - 1)))
}

/// How far the colors of an inactive pane move toward its background, when its tab
/// shows several panes.
pub const INACTIVE_PANE_DIM: f32 = 0.3;

impl Frame {
    /// This frame with every color moved `factor` (0 to 1) toward the background: cell
    /// text and backgrounds, block marks and the cursor. The banner is left as is, and
    /// a factor of 0 changes nothing.
    pub fn dimmed(mut self, factor: f32) -> Self {
        let background = self.background;
        let dim = |color: Rgb| blend(color, background, factor);
        for cell in self.rows.iter_mut().flatten() {
            cell.fg = dim(cell.fg);
            cell.bg = dim(cell.bg);
        }
        let colors = &mut self.block_colors;
        for color in [
            &mut colors.success,
            &mut colors.failure,
            &mut colors.running,
            &mut colors.dim,
            &mut colors.card,
            &mut colors.card_selected,
        ] {
            *color = dim(*color);
        }
        if let Some(cursor) = &mut self.cursor {
            cursor.color = dim(cursor.color);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::index::Side;
    use alacritty_terminal::selection::{Selection, SelectionType};

    use crate::timeline::testing::{Session, end, start};

    /// A 10x3 session fed with `bytes`, without any signal.
    fn session_with(bytes: &str) -> Session {
        let mut session = Session::sized(10, 3, 100);
        session.feed(bytes);
        session
    }

    fn view_rows(session: &Session) -> Vec<ViewRow> {
        view::rows(
            &session.timeline,
            &session.term,
            session.term.screen_lines(),
        )
    }

    fn capture_selected(session: &Session, selected_block: Option<BlockId>) -> Frame {
        let rows = view_rows(session);
        Frame::capture(
            &session.term,
            &session.timeline,
            &rows,
            &Palette::default(),
            true,
            selected_block,
        )
    }

    fn capture(session: &Session, palette: &Palette, focused: bool) -> Frame {
        let rows = view_rows(session);
        Frame::capture(
            &session.term,
            &session.timeline,
            &rows,
            palette,
            focused,
            None,
        )
    }

    fn row_text(frame: &Frame, line: usize) -> String {
        frame.rows[line]
            .iter()
            .filter(|c| !c.spacer)
            .map(|c| c.c)
            .collect()
    }

    fn texts(frame: &Frame) -> Vec<String> {
        (0..frame.rows.len())
            .map(|line| row_text(frame, line).trim_end().to_owned())
            .collect()
    }

    #[test]
    fn dimmed_by_zero_is_the_identity() {
        let session = session_with("\x1b[31;44mR\x1b[0m plain");
        let frame = capture(&session, &Palette::default(), true);
        assert_eq!(frame.clone().dimmed(0.0), frame);
    }

    #[test]
    fn dimmed_moves_colors_toward_the_background() {
        let session = session_with("\x1b[31;44mR\x1b[0m plain");
        let mut frame = capture(&session, &Palette::default(), true);
        frame.cursor = Some(FrameCursor {
            column: 0,
            line: 0,
            shape: CursorShape::Beam,
            color: Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
        });
        let background = frame.background;
        let toward = |color: Rgb| blend(color, background, 0.3);
        let dimmed = frame.clone().dimmed(0.3);

        let (cell, before) = (dimmed.rows[0][0], frame.rows[0][0]);
        assert_eq!((cell.fg, cell.bg), (toward(before.fg), toward(before.bg)));
        assert_ne!(cell.fg, before.fg);
        // Default-background cells keep the background, so they still draw no rect.
        assert_eq!(dimmed.rows[0][5].bg, background);
        assert_eq!(dimmed.background, background);
        assert_eq!(dimmed.block_colors.card, toward(frame.block_colors.card));
        assert_eq!(
            dimmed.block_colors.failure,
            toward(frame.block_colors.failure)
        );
        assert_eq!(
            dimmed.cursor.unwrap().color,
            toward(Rgb {
                r: 255,
                g: 255,
                b: 255
            })
        );
        // Text and layout are untouched.
        assert_eq!(texts(&dimmed), texts(&frame));
        assert_eq!(dimmed.row_tops, frame.row_tops);
    }

    #[test]
    fn captures_text_and_dimensions() {
        let frame = capture(&session_with("hello\r\nworld"), &Palette::default(), true);
        assert_eq!((frame.rows[0].len(), frame.rows.len()), (10, 2));
        assert_eq!(row_text(&frame, 0), "hello     ");
        assert_eq!(row_text(&frame, 1), "world     ");
    }

    #[test]
    fn applies_sgr_attributes() {
        let p = Palette::default();
        let frame = capture(&session_with("\x1b[1;31mR\x1b[0m\x1b[4mU"), &p, true);
        let r = frame.rows[0][0];
        assert!(r.bold);
        assert_eq!(r.fg, p.indexed[1]);
        assert!(frame.rows[0][1].underline);
        assert!(!frame.rows[0][1].bold);
    }

    #[test]
    fn truecolor_background() {
        let frame = capture(
            &session_with("\x1b[48;2;1;2;3mX"),
            &Palette::default(),
            true,
        );
        assert_eq!(frame.rows[0][0].bg, Rgb { r: 1, g: 2, b: 3 });
    }

    #[test]
    fn block_cursor_is_baked_into_cell() {
        let p = Palette::default();
        let frame = capture(&session_with("ab"), &p, true);
        assert_eq!(frame.cursor, None);
        assert_eq!(frame.rows[0][2].bg, p.cursor);
        assert_eq!(frame.rows[0][2].fg, p.cursor_text);
    }

    #[test]
    fn unfocused_cursor_is_hollow() {
        let frame = capture(&session_with("ab"), &Palette::default(), false);
        let cursor = frame.cursor.expect("cursor visible");
        assert_eq!((cursor.column, cursor.line), (2, 0));
        assert_eq!(cursor.shape, CursorShape::HollowBlock);
    }

    #[test]
    fn hidden_cursor_is_not_drawn() {
        let p = Palette::default();
        let frame = capture(&session_with("\x1b[?25l"), &p, true);
        assert_eq!(frame.cursor, None);
        assert_eq!(frame.rows[0][0].bg, p.background);
    }

    fn select<L>(term: &mut Term<L>, start: (i32, usize), end: (i32, usize)) {
        let start = Point::new(Line(start.0), Column(start.1));
        let end = Point::new(Line(end.0), Column(end.1));
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        term.selection = Some(selection);
    }

    #[test]
    fn selected_cells_use_the_selection_background() {
        let p = Palette::default();
        assert_eq!(p.selection_foreground, None);
        let mut session = session_with("\x1b[31mhe\x1b[0mllo\r\n");
        select(&mut session.term, (0, 1), (0, 2));
        let frame = capture(&session, &p, true);
        // Each cell keeps its own foreground.
        assert_eq!(frame.rows[0][1].fg, p.indexed[1]);
        assert_eq!(frame.rows[0][2].fg, p.foreground);
        for column in [1, 2] {
            assert_eq!(frame.rows[0][column].bg, p.selection_background);
        }
        for column in [0, 3] {
            assert_eq!(frame.rows[0][column].bg, p.background);
        }
    }

    #[test]
    fn selection_foreground_overrides_the_cell_foreground() {
        let mut p = Palette::default();
        let white = Rgb {
            r: 255,
            g: 255,
            b: 255,
        };
        p.selection_foreground = Some(white);
        let mut session = session_with("\x1b[31mhello\r\n");
        select(&mut session.term, (0, 1), (0, 2));
        let frame = capture(&session, &p, true);
        assert_eq!(frame.rows[0][1].fg, white);
        assert_eq!(frame.rows[0][0].fg, p.indexed[1]);
    }

    #[test]
    fn selected_wide_char_is_fully_highlighted() {
        let p = Palette::default();
        let mut session = session_with("x日\r\n");
        select(&mut session.term, (0, 1), (0, 1));
        let frame = capture(&session, &p, true);
        assert!(frame.rows[0][2].spacer);
        for column in [1, 2] {
            assert_eq!(frame.rows[0][column].bg, p.selection_background);
        }
        assert_eq!(frame.rows[0][0].bg, p.background);

        // Selecting only the spacer highlights the whole character too.
        select(&mut session.term, (0, 2), (0, 2));
        let frame = capture(&session, &p, true);
        for column in [1, 2] {
            assert_eq!(frame.rows[0][column].bg, p.selection_background);
        }
    }

    #[test]
    fn block_cursor_wins_over_selection() {
        let p = Palette::default();
        let mut session = session_with("ab");
        select(&mut session.term, (0, 0), (0, 4));
        let frame = capture(&session, &p, true);
        assert_eq!(frame.rows[0][2].bg, p.cursor);
        assert_eq!(frame.rows[0][2].fg, p.cursor_text);
        assert_eq!(frame.rows[0][1].bg, p.selection_background);
    }

    #[test]
    fn wide_chars_mark_spacer() {
        let frame = capture(&session_with("日x"), &Palette::default(), true);
        assert_eq!(frame.rows[0][0].c, '日');
        assert!(frame.rows[0][1].spacer);
        assert_eq!(frame.rows[0][2].c, 'x');
    }

    #[test]
    fn scrolled_block_shows_older_rows_and_hides_the_cursor() {
        let mut session = session_with("1\r\n2\r\n3\r\n4\r\n5");
        let id = session.timeline.current().id;
        view::scroll_block(&mut session.timeline, &session.term, id, 3, 2);
        let frame = capture(&session, &Palette::default(), false);
        assert_eq!(texts(&frame), ["2", "3"]);
        assert_eq!(frame.cursor, None);
    }

    #[test]
    fn selection_follows_scrolled_rows() {
        let p = Palette::default();
        let mut session = session_with("1\r\n2\r\n3\r\n4\r\n5");
        // Grid line -1 holds "2", history line of the live grid.
        select(&mut session.term, (-1, 0), (-1, 0));
        let id = session.timeline.current().id;
        view::scroll_block(&mut session.timeline, &session.term, id, 3, 2);
        let frame = capture(&session, &p, true);
        assert_eq!(texts(&frame), ["2", "3"]);
        assert_eq!(frame.rows[0][0].bg, p.selection_background);
        assert_eq!(frame.rows[1][0].bg, p.background);
    }

    fn card(rows: Range<usize>, status: Option<Status>, chevron: Option<Fold>) -> Card {
        Card {
            rows,
            clipped_top: false,
            status,
            chevron,
            selected: false,
            scroll: None,
        }
    }

    #[test]
    fn without_signals_the_session_is_one_neutral_card() {
        let frame = capture(&session_with("hello\r\nworld"), &Palette::default(), true);
        assert_eq!(frame.decorations, vec![RowDecoration::default(); 2]);
        assert_eq!(frame.cards, [card(0..2, None, None)]);
    }

    /// Two finished commands ("ls" printing "out", "false" printing nothing), then a
    /// prompt, in a 10x8 session.
    fn two_blocks() -> Session {
        let mut session = Session::sized(10, 8, 1000);
        session.command("ls", &["out"], 0);
        session.command("false", &[], 1);
        session.feed("$ ");
        session
    }

    #[test]
    fn rows_come_from_frozen_and_live_sources() {
        let session = two_blocks();
        let rows = view_rows(&session);
        let sources: Vec<RowSource> = rows.iter().map(|row| row.source).collect();
        assert_eq!(
            sources,
            [
                RowSource::Header(BlockId(0)),
                RowSource::Output(BlockId(0)),
                RowSource::Header(BlockId(1)),
                RowSource::Live,
            ]
        );
        let frame = capture(&session, &Palette::default(), true);
        assert_eq!(texts(&frame), ["$ ls", "out", "$ false", "$"]);
        // The cursor is on the live row.
        assert_eq!(frame.rows[3][2].bg, Palette::default().cursor);
    }

    #[test]
    fn cards_of_finished_failed_and_active_prompt_blocks() {
        let frame = capture(&two_blocks(), &Palette::default(), true);
        assert_eq!(
            frame.cards,
            vec![
                // A finished block with output: an unfolded chevron.
                card(0..2, Some(Status::Success), Some(Fold::Unfolded)),
                // No output: no chevron.
                card(2..3, Some(Status::Failure(1)), None),
                // The active prompt: a neutral card.
                card(3..4, None, None),
            ]
        );
        assert!(frame.decorations.iter().all(|d| d.summary.is_none()));
        let p = Palette::default();
        assert_eq!(frame.block_colors.success, p.indexed[2]);
        assert_eq!(frame.block_colors.failure, p.indexed[1]);
        assert_eq!(frame.block_colors.running, p.indexed[3]);
        assert_eq!(frame.block_colors.dim, p.named(NamedColor::DimForeground));
        assert_eq!(
            frame.block_colors.card,
            blend(p.background, p.foreground, 0.06)
        );
        assert_eq!(
            frame.block_colors.card_selected,
            blend(p.background, p.foreground, 0.14)
        );
    }

    #[test]
    fn blend_moves_each_channel_toward_the_target() {
        let black = Rgb { r: 0, g: 0, b: 0 };
        let white = Rgb {
            r: 255,
            g: 255,
            b: 255,
        };
        assert_eq!(blend(black, white, 0.0), black);
        assert_eq!(blend(black, white, 1.0), white);
        assert_eq!(
            blend(black, white, 0.06),
            Rgb {
                r: 15,
                g: 15,
                b: 15
            }
        );
        assert_eq!(
            blend(white, black, 0.14),
            Rgb {
                r: 219,
                g: 219,
                b: 219
            }
        );
    }

    #[test]
    fn card_of_a_block_continuing_above_the_view_is_clipped() {
        let mut session = Session::sized(10, 4, 100);
        session.command("seq", &["1", "2", "3"], 0);
        session.feed("$ ");
        let frame = capture(&session, &Palette::default(), true);
        // The block's header and first rows are above the view: no chevron. Capped at
        // 3 rows, it shows its last 2 output rows of 3: a scroll indicator.
        assert_eq!(texts(&frame), ["2", "3", "$"]);
        assert_eq!(
            frame.cards,
            vec![
                Card {
                    clipped_top: true,
                    scroll: Some(ScrollBar {
                        rows: 0..2,
                        start: 1.0 / 3.0,
                        length: 2.0 / 3.0,
                    }),
                    ..card(0..2, Some(Status::Success), None)
                },
                card(2..3, None, None),
            ]
        );
    }

    /// A finished `seq` printing 1 to 6 then a prompt, in a 10x6 session: capped at 5
    /// rows, the block shows its header and its last 4 output rows of 6; with the
    /// prompt's card below, its header is above the view.
    fn tall_block() -> Session {
        let mut session = Session::sized(10, 6, 100);
        session.command("seq", &["1", "2", "3", "4", "5", "6"], 0);
        session.feed("$ ");
        session
    }

    #[test]
    fn scroll_indicator_beside_the_output_of_a_capped_block() {
        let mut session = tall_block();
        let frame = capture(&session, &Palette::default(), true);
        assert_eq!(texts(&frame), ["3", "4", "5", "6", "$"]);
        let bar = ScrollBar {
            rows: 0..4,
            start: 2.0 / 6.0,
            length: 4.0 / 6.0,
        };
        assert_eq!(frame.cards[0].scroll, Some(bar.clone()));
        // The prompt's card does not scroll.
        assert_eq!(frame.cards[1].scroll, None);

        // Scrolled to its top, the whole block is revealed, its command back at the
        // top, and the thumb moves up beside the output rows.
        let lines = session.term.screen_lines();
        view::scroll_block(&mut session.timeline, &session.term, BlockId(0), lines, 5);
        let frame = capture(&session, &Palette::default(), true);
        assert_eq!(texts(&frame), ["$ seq", "1", "2", "3", "4"]);
        assert_eq!(
            frame.cards[0].scroll,
            Some(ScrollBar {
                rows: 1..5,
                start: 0.0,
                ..bar
            })
        );
    }

    #[test]
    fn no_scroll_indicator_on_a_folded_block() {
        let mut session = tall_block();
        session.timeline.toggle_fold(BlockId(0));
        let frame = capture(&session, &Palette::default(), true);
        assert!(frame.cards.iter().all(|card| card.scroll.is_none()));
    }

    #[test]
    fn scroll_indicator_on_the_live_output_of_a_tall_current_block() {
        // No signal: one block holding the whole session, capped at 2 rows of 3 lines.
        let frame = capture(
            &session_with("1\r\n2\r\n3\r\n4\r\n5"),
            &Palette::default(),
            true,
        );
        assert_eq!(
            frame.cards[0].scroll,
            Some(ScrollBar {
                rows: 0..2,
                start: 3.0 / 5.0,
                length: 2.0 / 5.0,
            })
        );
    }

    #[test]
    fn block_selection_marks_the_card() {
        let session = two_blocks();
        let frame = capture_selected(&session, Some(BlockId(0)));
        let selected: Vec<_> = frame.cards.iter().map(|card| card.selected).collect();
        assert_eq!(selected, [true, false, false]);
        let p = Palette::default();
        assert!(
            frame
                .rows
                .iter()
                .flatten()
                .all(|cell| cell.bg != p.selection_background)
        );
    }

    #[test]
    fn selected_block_no_longer_in_the_timeline_marks_no_card() {
        let session = two_blocks();
        let frame = capture_selected(&session, Some(BlockId(7)));
        assert!(frame.cards.iter().all(|card| !card.selected));
        // The current block is not a finished one.
        let frame = capture_selected(&session, Some(session.timeline.current().id));
        assert!(frame.cards.iter().all(|card| !card.selected));
    }

    #[test]
    fn selection_in_a_frozen_block_highlights_its_cells() {
        let mut session = two_blocks();
        let output = &mut session.timeline.block_mut(BlockId(0)).unwrap().output;
        select(output, (0, 0), (0, 1));
        let p = Palette::default();
        let frame = capture(&session, &p, true);
        assert_eq!(frame.rows[1][0].bg, p.selection_background);
        assert_eq!(frame.rows[1][1].bg, p.selection_background);
        assert_eq!(frame.rows[1][2].bg, p.background);
        // Same lines in other sources are not highlighted.
        assert_eq!(frame.rows[0][0].bg, p.background);
        assert_eq!(frame.rows[2][0].bg, p.background);
    }

    #[test]
    fn folded_block_shows_a_summary_row_and_a_folded_chevron() {
        let mut session = Session::sized(10, 8, 1000);
        session.command("seq", &["1", "2", "3"], 0);
        session.feed("$ ");
        session.timeline.toggle_fold(BlockId(0));
        let frame = capture(&session, &Palette::default(), true);
        assert_eq!(texts(&frame), ["$ seq", "", "$"]);
        assert_eq!(
            frame.cards,
            [
                card(0..2, Some(Status::Success), Some(Fold::Folded)),
                card(2..3, None, None)
            ]
        );
        let summaries: Vec<_> = frame.decorations.iter().map(|d| d.summary).collect();
        assert_eq!(summaries, [None, Some(3), None]);
    }

    #[test]
    fn has_chevron_only_on_the_first_row_of_a_card_with_one() {
        let session = two_blocks();
        let rows = view_rows(&session);
        let (term, timeline) = (&session.term, &session.timeline);
        assert!(has_chevron(term, timeline, &rows, 0));
        assert!(!has_chevron(term, timeline, &rows, 1));
        assert!(!has_chevron(term, timeline, &rows, 2));
        assert!(!has_chevron(term, timeline, &rows, 3));
    }

    #[test]
    fn running_block_has_a_running_card_and_no_chevron() {
        let mut session = Session::sized(10, 6, 100);
        session.feed(&format!("$ yes\r\n{}y\r\ny\r\n", start("yes")));
        let frame = capture(&session, &Palette::default(), true);
        assert_eq!(texts(&frame), ["$ yes", "y", "y", ""]);
        assert_eq!(frame.cards, [card(0..4, Some(Status::Running), None)]);
        session.feed(&end(0));
        let frame = capture(&session, &Palette::default(), true);
        assert_eq!(frame.cards[0].status, Some(Status::Success));
    }

    #[test]
    fn summary_column_follows_the_last_non_blank_cell() {
        let blank = |text: &str| text.chars().map(|c| c == ' ').collect::<Vec<_>>();
        assert_eq!(summary_column(blank("$ ls      ").into_iter()), 5);
        assert_eq!(summary_column(blank("          ").into_iter()), 0);
        // Clamped to the row width.
        assert_eq!(summary_column(blank("         x").into_iter()), 10);
    }

    #[test]
    fn alternate_screen_is_not_decorated() {
        let mut session = Session::sized(10, 3, 100);
        session.feed(&format!("$ vim\r\n{}\x1b[?1049hx", start("vim")));
        let frame = capture(&session, &Palette::default(), true);
        assert_eq!(frame.rows.len(), 3);
        assert_eq!(row_text(&frame, 0), "x         ");
        assert_eq!(frame.decorations, vec![RowDecoration::default(); 3]);
        assert!(frame.cards.is_empty());
    }

    #[test]
    fn frozen_rows_narrower_than_the_live_grid_are_padded() {
        let mut session = Session::sized(10, 8, 1000);
        session.command("ls", &["out"], 0);
        session
            .term
            .resize(alacritty_terminal::term::test::TermSize::new(14, 8));
        let frame = capture(&session, &Palette::default(), true);
        assert_eq!(frame.rows[0].len(), 14);
        assert_eq!(row_text(&frame, 1), "out           ");
    }
}
