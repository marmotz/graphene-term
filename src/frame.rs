//! Snapshot of the visible terminal content, decoupled from the GPU renderer.
//!
//! Building a [`Frame`] only requires the terminal lock for a short time; the
//! renderer then works on this plain data.

use std::ops::Range;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::selection::SelectionRange;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};

use crate::banner::BannerLayout;
use crate::blocks::{self, BlockId, Phase, RowRole, ShellState, Status};
use crate::i18n::Lang;
use crate::palette::Palette;
use crate::view::ViewRow;

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
    /// Output rows hidden below this row, the last command row of a folded block.
    pub summary: Option<usize>,
}

/// Card drawn behind the consecutive view rows of one block, up to the cursor row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    /// View rows covered.
    pub rows: Range<usize>,
    /// The block continues above the view: the card's top corners stay off screen.
    pub clipped_top: bool,
    /// Status shown by the stripe and the chevron. `None` for the active prompt (the
    /// current block while its prompt is drawn or its command typed): a neutral card.
    pub status: Option<Status>,
    /// Fold chevron on the card's top-right corner: a finished block with output whose
    /// first command row is in the card.
    pub chevron: Option<Fold>,
    /// The block is the selected one.
    pub selected: bool,
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

#[derive(Debug, Clone)]
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
    /// Captures the grid lines shown by `view` (see [`crate::view::rows`]), with the
    /// block marks of `shell`.
    pub fn capture<T: EventListener>(
        term: &Term<T>,
        view: &[ViewRow],
        shell: &ShellState,
        palette: &Palette,
        focused: bool,
    ) -> Self {
        let content = term.renderable_content();
        let overrides = content.colors;
        let columns = term.columns();
        let lines = view.len();
        let selection = content.selection;
        let grid = term.grid();

        let background = palette.resolve(Color::Named(NamedColor::Background), overrides);
        let foreground = palette.resolve(Color::Named(NamedColor::Foreground), overrides);
        let mut rows = Vec::with_capacity(lines);
        for view_row in view {
            let Some(line) = view_row.line else {
                // Padding row below the content.
                rows.push(vec![FrameCell::blank(foreground, background); columns]);
                continue;
            };
            let grid_row = &grid[line];
            let row = (0..columns).map(|column| {
                let point = Point::new(line, Column(column));
                let cell = &grid_row[Column(column)];
                let flags = cell.flags;
                let (mut fg, mut bg) = palette.cell_colors(cell.fg, cell.bg, flags, overrides);
                if selection.is_some_and(|range| is_selected(&range, point, flags)) {
                    bg = palette.selection_background;
                    fg = palette.selection_foreground.unwrap_or(fg);
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
            });
            rows.push(row.collect::<Vec<_>>());
        }

        let cursor_color = palette.resolve(Color::Named(NamedColor::Cursor), overrides);
        let cursor_point = content.cursor.point;
        let cursor_line = view
            .iter()
            .position(|row| row.line == Some(cursor_point.line));
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

        let (decorations, cards) = marks(term, view, shell);
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

/// Block marks of the rows of `view` and the cards behind them. Rows below the cursor
/// are not drawn by the shell yet, so they get none even though the index counts them
/// in the last block.
pub fn marks<T>(
    term: &Term<T>,
    view: &[ViewRow],
    shell: &ShellState,
) -> (Vec<RowDecoration>, Vec<Card>) {
    let cursor_line = term.grid().cursor.point.line;
    // Row above the first view row, to know whether that row starts its block or its
    // command.
    let mut previous = view
        .first()
        .filter(|row| row.role.block().is_some())
        .and_then(|row| row.line)
        .filter(|line| *line > term.grid().topmost_line())
        .map(|line| (Some(line - 1), blocks::index(term, line - 1..line)[0]));
    let above_view = previous.and_then(|(_, role)| role.block());
    let mut decorations = Vec::with_capacity(view.len());
    let mut cards: Vec<Card> = Vec::new();
    // Block of the card the previous view row belongs to.
    let mut card_block = None;
    for (index, row) in view.iter().enumerate() {
        let Some(line) = row.line else {
            previous = None;
            card_block = None;
            decorations.push(RowDecoration::default());
            continue;
        };
        let block = row.role.block().filter(|_| line <= cursor_line);
        let chevron = block
            .filter(|id| shell.is_finished(*id))
            .filter(|id| first_command_row(term, line, *id, previous))
            .filter(|id| blocks::has_output(term, *id, line))
            .map(|id| {
                if shell.is_folded(id) {
                    Fold::Folded
                } else {
                    Fold::Unfolded
                }
            });
        previous = Some((Some(line), row.role));
        decorations.push(RowDecoration {
            summary: (block.is_some() && row.hidden > 0).then_some(row.hidden),
        });
        match (block, cards.last_mut()) {
            (Some(id), Some(card)) if card_block == Some(id) => {
                card.rows.end = index + 1;
                card.chevron = card.chevron.or(chevron);
            }
            (Some(id), _) => cards.push(Card {
                rows: index..index + 1,
                clipped_top: index == 0 && above_view == Some(id),
                status: card_status(shell, id),
                chevron,
                selected: false,
            }),
            (None, _) => {}
        }
        card_block = block;
    }
    (decorations, cards)
}

/// Status shown by the card of block `id`: none for the active prompt.
fn card_status(shell: &ShellState, id: BlockId) -> Option<Status> {
    let active_prompt =
        shell.current() == Some(id) && matches!(shell.phase, Phase::Prompt | Phase::Input);
    (!active_prompt).then(|| shell.block(id).map_or(Status::Unknown, |b| b.status))
}

/// Whether `line` holds the first command cell of block `id`: it has one, and the row
/// above it (`previous`, a prompt or command row of the same block when there is one)
/// has none.
fn first_command_row<T>(
    term: &Term<T>,
    line: Line,
    id: BlockId,
    previous: Option<(Option<Line>, RowRole)>,
) -> bool {
    if !matches!(blocks::head_role(term, line), Some(role) if role.block() == Some(id))
        || !blocks::has_input(term, line, id)
    {
        return false;
    }
    match previous {
        Some((Some(above), RowRole::Prompt(block) | RowRole::Input(block))) if block == id => {
            !blocks::has_input(term, above, id)
        }
        _ => true,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::grid::Scroll;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::Processor;

    use crate::osc133::Scanner;
    use crate::pty;
    use crate::view;

    fn term_with(bytes: &[u8]) -> Term<VoidListener> {
        let mut term = Term::new(Config::default(), &TermSize::new(10, 3), VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, bytes);
        term
    }

    /// Captures the viewport of `term`, with no shell integration.
    fn capture(term: &Term<VoidListener>, palette: &Palette, focused: bool) -> Frame {
        let shell = ShellState::new();
        let rows = view::rows(term, &shell, term.grid().display_offset());
        Frame::capture(term, &rows, &shell, palette, focused)
    }

    /// Feeds `bytes` through the shell integration path, then captures the viewport.
    fn capture_blocks(term: &mut Term<VoidListener>, bytes: &str) -> Frame {
        let mut shell = ShellState::new();
        pty::process(
            &mut Scanner::new(),
            &mut Processor::new(),
            &mut shell,
            term,
            bytes.as_bytes(),
        );
        let rows = view::rows(term, &shell, term.grid().display_offset());
        Frame::capture(term, &rows, &shell, &Palette::default(), true)
    }

    fn row_text(frame: &Frame, line: usize) -> String {
        frame.rows[line]
            .iter()
            .filter(|c| !c.spacer)
            .map(|c| c.c)
            .collect()
    }

    #[test]
    fn captures_text_and_dimensions() {
        let frame = capture(&term_with(b"hello\r\nworld"), &Palette::default(), true);
        assert_eq!((frame.rows[0].len(), frame.rows.len()), (10, 3));
        assert_eq!(row_text(&frame, 0), "hello     ");
        assert_eq!(row_text(&frame, 1), "world     ");
    }

    #[test]
    fn applies_sgr_attributes() {
        let p = Palette::default();
        let frame = capture(&term_with(b"\x1b[1;31mR\x1b[0m\x1b[4mU"), &p, true);
        let r = frame.rows[0][0];
        assert!(r.bold);
        assert_eq!(r.fg, p.indexed[1]);
        assert!(frame.rows[0][1].underline);
        assert!(!frame.rows[0][1].bold);
    }

    #[test]
    fn truecolor_background() {
        let frame = capture(&term_with(b"\x1b[48;2;1;2;3mX"), &Palette::default(), true);
        assert_eq!(frame.rows[0][0].bg, Rgb { r: 1, g: 2, b: 3 });
    }

    #[test]
    fn block_cursor_is_baked_into_cell() {
        let p = Palette::default();
        let frame = capture(&term_with(b"ab"), &p, true);
        assert_eq!(frame.cursor, None);
        assert_eq!(frame.rows[0][2].bg, p.cursor);
        assert_eq!(frame.rows[0][2].fg, p.cursor_text);
    }

    #[test]
    fn unfocused_cursor_is_hollow() {
        let frame = capture(&term_with(b"ab"), &Palette::default(), false);
        let cursor = frame.cursor.expect("cursor visible");
        assert_eq!((cursor.column, cursor.line), (2, 0));
        assert_eq!(cursor.shape, CursorShape::HollowBlock);
    }

    #[test]
    fn hidden_cursor_is_not_drawn() {
        let p = Palette::default();
        let frame = capture(&term_with(b"\x1b[?25l"), &p, true);
        assert_eq!(frame.cursor, None);
        assert_eq!(frame.rows[0][0].bg, p.background);
    }

    fn select(term: &mut Term<VoidListener>, start: (i32, usize), end: (i32, usize)) {
        use alacritty_terminal::index::{Column, Line, Point, Side};
        use alacritty_terminal::selection::{Selection, SelectionType};
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
        let mut term = term_with(b"\x1b[31mhe\x1b[0mllo\r\n");
        select(&mut term, (0, 1), (0, 2));
        let frame = capture(&term, &p, true);
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
        let white = alacritty_terminal::vte::ansi::Rgb {
            r: 255,
            g: 255,
            b: 255,
        };
        p.selection_foreground = Some(white);
        let mut term = term_with(b"\x1b[31mhello\r\n");
        select(&mut term, (0, 1), (0, 2));
        let frame = capture(&term, &p, true);
        assert_eq!(frame.rows[0][1].fg, white);
        assert_eq!(frame.rows[0][0].fg, p.indexed[1]);
    }

    #[test]
    fn selected_wide_char_is_fully_highlighted() {
        let p = Palette::default();
        let mut term = term_with("x日\r\n".as_bytes());
        select(&mut term, (0, 1), (0, 1));
        let frame = capture(&term, &p, true);
        assert!(frame.rows[0][2].spacer);
        for column in [1, 2] {
            assert_eq!(frame.rows[0][column].bg, p.selection_background);
        }
        assert_eq!(frame.rows[0][0].bg, p.background);

        // Selecting only the spacer highlights the whole character too.
        select(&mut term, (0, 2), (0, 2));
        let frame = capture(&term, &p, true);
        for column in [1, 2] {
            assert_eq!(frame.rows[0][column].bg, p.selection_background);
        }
    }

    #[test]
    fn block_cursor_wins_over_selection() {
        let p = Palette::default();
        let mut term = term_with(b"ab");
        select(&mut term, (0, 0), (0, 4));
        let frame = capture(&term, &p, true);
        assert_eq!(frame.rows[0][2].bg, p.cursor);
        assert_eq!(frame.rows[0][2].fg, p.cursor_text);
        assert_eq!(frame.rows[0][1].bg, p.selection_background);
    }

    #[test]
    fn wide_chars_mark_spacer() {
        let frame = capture(&term_with("日x".as_bytes()), &Palette::default(), true);
        assert_eq!(frame.rows[0][0].c, '日');
        assert!(frame.rows[0][1].spacer);
        assert_eq!(frame.rows[0][2].c, 'x');
    }

    #[test]
    fn scrolled_view_shows_history_rows_and_hides_the_cursor() {
        let mut term = term_with(b"1\r\n2\r\n3\r\n4\r\n5");
        term.scroll_display(Scroll::Delta(2));
        let frame = capture(&term, &Palette::default(), false);
        let texts: Vec<String> = (0..3).map(|line| row_text(&frame, line)).collect();
        assert_eq!(texts, ["1         ", "2         ", "3         "]);
        assert_eq!(frame.cursor, None);
    }

    #[test]
    fn selection_follows_scrolled_rows() {
        let p = Palette::default();
        let mut term = term_with(b"1\r\n2\r\n3\r\n4\r\n5");
        select(&mut term, (-1, 0), (-1, 0));
        term.scroll_display(Scroll::Delta(2));
        let frame = capture(&term, &p, true);
        // Grid line -1 ("2") is the second visible row.
        assert_eq!(frame.rows[1][0].bg, p.selection_background);
        assert_eq!(frame.rows[0][0].bg, p.background);
    }

    #[test]
    fn rows_without_blocks_have_no_decoration() {
        let frame = capture(&term_with(b"hello\r\nworld"), &Palette::default(), true);
        assert_eq!(frame.decorations, vec![RowDecoration::default(); 3]);
        assert!(frame.cards.is_empty());
    }

    fn card(rows: Range<usize>, status: Option<Status>, chevron: Option<Fold>) -> Card {
        Card {
            rows,
            clipped_top: false,
            status,
            chevron,
            selected: false,
        }
    }

    #[test]
    fn cards_of_finished_failed_and_active_prompt_blocks_up_to_the_cursor() {
        let mut term = Term::new(Config::default(), &TermSize::new(10, 6), VoidListener);
        let frame = capture_blocks(
            &mut term,
            "\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07out\r\n\x1b]133;D;0\x07\
             \x1b]133;A\x07$ \x1b]133;B\x07false\r\n\x1b]133;C\x07\x1b]133;D;1\x07\
             \x1b]133;A\x07$ \x1b]133;B\x07",
        );
        assert_eq!(
            frame.cards,
            vec![
                // A finished block with output: an unfolded chevron.
                card(0..2, Some(Status::Success), Some(Fold::Unfolded)),
                // No output: no chevron.
                card(2..3, Some(Status::Failure(1)), None),
                // The active prompt: a neutral card. The blank row below the cursor
                // gets none.
                card(3..4, None, None),
            ]
        );
        assert_eq!(frame.rows.len(), 5);
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
        let mut term = Term::new(Config::default(), &TermSize::new(10, 3), VoidListener);
        let frame = capture_blocks(
            &mut term,
            "\x1b]133;A\x07$ \x1b]133;B\x07seq\r\n\x1b]133;C\x07\
             1\r\n2\r\n3\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ ",
        );
        // Row "3" is the output of the first block, cut at the top of the view (three
        // rows: not taller than the window, so not folded); the gap above the prompt
        // leaves room for two rows only. The command row is out of view: no chevron.
        assert_eq!(frame.rows.len(), 2);
        assert_eq!(row_text(&frame, 0), "3         ");
        assert_eq!(
            frame.cards,
            vec![
                Card {
                    clipped_top: true,
                    ..card(0..1, Some(Status::Success), None)
                },
                card(1..2, None, None),
            ]
        );
    }

    /// Feeds `bytes` to `term` and `shell` through the shell integration path.
    fn feed_blocks(term: &mut Term<VoidListener>, shell: &mut ShellState, bytes: &str) {
        pty::process(
            &mut Scanner::new(),
            &mut Processor::new(),
            shell,
            term,
            bytes.as_bytes(),
        );
    }

    fn capture_shell(term: &Term<VoidListener>, shell: &ShellState) -> Frame {
        let rows = view::rows(term, shell, term.grid().display_offset());
        Frame::capture(term, &rows, shell, &Palette::default(), true)
    }

    #[test]
    fn chevron_on_the_first_command_row_and_summary_on_the_last() {
        let mut term = Term::new(Config::default(), &TermSize::new(10, 8), VoidListener);
        let mut shell = ShellState::new();
        // Two-row prompt, two-row command, three output rows, then a new prompt.
        feed_blocks(
            &mut term,
            &mut shell,
            "\x1b]133;A\x07~/dev\r\n$ \x1b]133;B\x07echo \\\r\n  x\r\n\x1b]133;C\x07\
             1\r\n2\r\n3\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ ",
        );
        let frame = capture_shell(&term, &shell);
        // The first block's card covers its prompt, command and output rows.
        assert_eq!(
            frame.cards[0],
            card(0..6, Some(Status::Success), Some(Fold::Unfolded))
        );
        assert!(frame.decorations.iter().all(|d| d.summary.is_none()));

        shell.set_folded(BlockId(0), true);
        let frame = capture_shell(&term, &shell);
        assert_eq!(row_text(&frame, 3), "$         ");
        assert_eq!(
            frame.cards[..2],
            [
                card(0..3, Some(Status::Success), Some(Fold::Folded)),
                card(3..4, None, None)
            ]
        );
        let summaries: Vec<_> = frame.decorations.iter().map(|d| d.summary).collect();
        assert_eq!(summaries[..4], [None, None, Some(3), None]);
    }

    #[test]
    fn running_block_has_no_chevron() {
        let mut term = Term::new(Config::default(), &TermSize::new(10, 4), VoidListener);
        let mut shell = ShellState::new();
        feed_blocks(
            &mut term,
            &mut shell,
            "\x1b]133;A\x07$ \x1b]133;B\x07yes\r\n\x1b]133;C\x07y\r\ny\r\n",
        );
        let frame = capture_shell(&term, &shell);
        // Up to the cursor row, below the last output row.
        assert_eq!(frame.cards, [card(0..4, Some(Status::Running), None)]);
    }

    #[test]
    fn auto_folded_block_shows_its_summary() {
        let mut term = Term::new(Config::default(), &TermSize::new(20, 3), VoidListener);
        let mut shell = ShellState::new();
        feed_blocks(
            &mut term,
            &mut shell,
            "\x1b]133;A\x07$ \x1b]133;B\x07seq 4\r\n\x1b]133;C\x07\
             1\r\n2\r\n3\r\n4\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ ",
        );
        assert!(shell.is_folded(BlockId(0)));
        let frame = capture_shell(&term, &shell);
        // Two visible rows; a blank row would not fit after the gap.
        assert_eq!(frame.rows.len(), 2);
        assert_eq!(row_text(&frame, 0), "$ seq 4             ");
        assert_eq!(row_text(&frame, 1), "$                   ");
        assert_eq!(frame.decorations[0].summary, Some(4));
        assert_eq!(
            frame.cards,
            [
                card(0..1, Some(Status::Success), Some(Fold::Folded)),
                card(1..2, None, None)
            ]
        );
        assert_eq!(frame.row_tops, [1, 4]);
        // The cursor is on the second visible row.
        assert_eq!(frame.rows[1][2].bg, Palette::default().cursor);
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
        let mut term = Term::new(Config::default(), &TermSize::new(10, 3), VoidListener);
        let frame = capture_blocks(
            &mut term,
            "\x1b]133;A\x07$ \x1b]133;B\x07vim\r\n\x1b]133;C\x07\x1b[?1049hx",
        );
        assert_eq!(frame.decorations, vec![RowDecoration::default(); 3]);
        assert!(frame.cards.is_empty());
    }
}
