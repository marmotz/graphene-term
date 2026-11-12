//! Snapshot of the visible terminal content, decoupled from the GPU renderer.
//!
//! Building a [`Frame`] only requires the terminal lock for a short time; the
//! renderer then works on this plain data.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Point};
use alacritty_terminal::selection::SelectionRange;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};

use crate::banner::BannerLayout;
use crate::blocks::{RowRole, ShellState, Status};
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

/// Command block marks of one row, drawn in the gutter and across the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowDecoration {
    /// Status bar of the block the row belongs to.
    pub bar: Option<Status>,
    /// The row is the first one of its block: a line is drawn at its top.
    pub separator: bool,
}

/// Colors of the block marks, resolved from the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockColors {
    pub success: Rgb,
    pub failure: Rgb,
    pub running: Rgb,
    /// `Unknown` status and separators.
    pub dim: Rgb,
}

impl BlockColors {
    fn resolve(palette: &Palette, overrides: &Colors) -> Self {
        let named = |color| palette.resolve(Color::Named(color), overrides);
        Self {
            success: named(NamedColor::Green),
            failure: named(NamedColor::Red),
            running: named(NamedColor::Yellow),
            dim: named(NamedColor::DimForeground),
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
    pub block_colors: BlockColors,
    pub cursor: Option<FrameCursor>,
    pub background: Rgb,
    pub banner: Option<FrameBanner>,
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
        let mut rows = Vec::with_capacity(lines);
        for view_row in view {
            let grid_row = &grid[view_row.line];
            let row = (0..columns).map(|column| {
                let point = Point::new(view_row.line, Column(column));
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
        let cursor_line = view.iter().position(|row| row.line == cursor_point.line);
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

        Self {
            rows,
            decorations: decorations(term, view, shell),
            block_colors: BlockColors::resolve(palette, overrides),
            cursor,
            background,
            banner: None,
        }
    }
}

/// Block marks of the rows of `view`. Rows below the cursor are not drawn by the shell
/// yet, so they get none even though the index counts them in the last block.
fn decorations<T>(term: &Term<T>, view: &[ViewRow], shell: &ShellState) -> Vec<RowDecoration> {
    let cursor_line = term.grid().cursor.point.line;
    // Block of the row above the first view row, to know whether that row starts its
    // block.
    let mut previous = view
        .first()
        .filter(|row| row.role.block().is_some() && row.line > term.grid().topmost_line())
        .and_then(|row| crate::blocks::index(term, row.line - 1..row.line)[0].block());
    view.iter()
        .map(|row| {
            let block = row.role.block().filter(|_| row.line <= cursor_line);
            let separator = matches!(row.role, RowRole::Prompt(_) | RowRole::Input(_))
                && block.is_some()
                && previous != block;
            previous = row.role.block();
            RowDecoration {
                bar: block.map(|id| shell.block(id).map_or(Status::Unknown, |b| b.status)),
                separator,
            }
        })
        .collect()
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
    }

    #[test]
    fn block_rows_are_decorated_up_to_the_cursor() {
        let mut term = Term::new(Config::default(), &TermSize::new(10, 6), VoidListener);
        let frame = capture_blocks(
            &mut term,
            "\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07out\r\n\x1b]133;D;0\x07\
             \x1b]133;A\x07$ \x1b]133;B\x07false\r\n\x1b]133;C\x07\x1b]133;D;1\x07\
             \x1b]133;A\x07$ \x1b]133;B\x07",
        );
        let row = |bar, separator| RowDecoration { bar, separator };
        assert_eq!(
            frame.decorations,
            vec![
                row(Some(Status::Success), true),
                row(Some(Status::Success), false),
                row(Some(Status::Failure(1)), true),
                // The last prompt: no command ran yet. Below the cursor: nothing.
                row(Some(Status::Unknown), true),
                row(None, false),
                row(None, false),
            ]
        );
        let p = Palette::default();
        assert_eq!(frame.block_colors.success, p.indexed[2]);
        assert_eq!(frame.block_colors.failure, p.indexed[1]);
        assert_eq!(frame.block_colors.running, p.indexed[3]);
        assert_eq!(frame.block_colors.dim, p.named(NamedColor::DimForeground));
    }

    #[test]
    fn block_continuing_above_the_view_gets_no_separator() {
        let mut term = Term::new(Config::default(), &TermSize::new(10, 3), VoidListener);
        let frame = capture_blocks(
            &mut term,
            "\x1b]133;A\x07$ \x1b]133;B\x07seq\r\n\x1b]133;C\x07\
             1\r\n2\r\n3\r\n4\r\n5\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ ",
        );
        // Rows "4", "5" are the output of the first block, cut at the top of the view.
        assert_eq!(row_text(&frame, 0), "4         ");
        assert!(!frame.decorations[0].separator);
        assert_eq!(frame.decorations[0].bar, Some(Status::Success));
        assert!(frame.decorations[2].separator);
    }

    #[test]
    fn alternate_screen_is_not_decorated() {
        let mut term = Term::new(Config::default(), &TermSize::new(10, 3), VoidListener);
        let frame = capture_blocks(
            &mut term,
            "\x1b]133;A\x07$ \x1b]133;B\x07vim\r\n\x1b]133;C\x07\x1b[?1049hx",
        );
        assert_eq!(frame.decorations, vec![RowDecoration::default(); 3]);
    }
}
