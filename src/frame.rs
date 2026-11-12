//! Snapshot of the visible terminal content, decoupled from the GPU renderer.
//!
//! Building a [`Frame`] only requires the terminal lock for a short time; the
//! renderer then works on this plain data.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::index::Point;
use alacritty_terminal::selection::SelectionRange;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{CursorShape, Rgb};

use crate::banner::BannerLayout;
use crate::palette::Palette;

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

#[derive(Debug, Clone)]
pub struct Frame {
    /// Visible rows, top to bottom.
    pub rows: Vec<Vec<FrameCell>>,
    pub cursor: Option<FrameCursor>,
    pub background: Rgb,
    pub banner: Option<FrameBanner>,
}

impl Frame {
    /// Captures the visible part of the terminal.
    pub fn capture<T: EventListener>(term: &Term<T>, palette: &Palette, focused: bool) -> Self {
        use alacritty_terminal::grid::Dimensions;

        let content = term.renderable_content();
        let overrides = content.colors;
        let columns = term.columns();
        let lines = term.screen_lines();
        let display_offset = content.display_offset as i32;
        let selection = content.selection;

        let background = palette.resolve(
            alacritty_terminal::vte::ansi::Color::Named(
                alacritty_terminal::vte::ansi::NamedColor::Background,
            ),
            overrides,
        );
        let blank = FrameCell {
            c: ' ',
            fg: palette.foreground,
            bg: background,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            spacer: false,
        };
        let mut rows = vec![vec![blank; columns]; lines];

        for indexed in content.display_iter {
            let line = indexed.point.line.0 + display_offset;
            let column = indexed.point.column.0;
            if line < 0 || line as usize >= lines || column >= columns {
                continue;
            }
            let cell = indexed.cell;
            let flags = cell.flags;
            let (mut fg, mut bg) = palette.cell_colors(cell.fg, cell.bg, flags, overrides);
            if selection.is_some_and(|range| is_selected(&range, indexed.point, flags)) {
                bg = palette.selection_background;
                fg = palette.selection_foreground.unwrap_or(fg);
            }
            let spacer =
                flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
            rows[line as usize][column] = FrameCell {
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
            };
        }

        let cursor_color = palette.resolve(
            alacritty_terminal::vte::ansi::Color::Named(
                alacritty_terminal::vte::ansi::NamedColor::Cursor,
            ),
            overrides,
        );
        let cursor_line = content.cursor.point.line.0 + display_offset;
        let cursor_column = content.cursor.point.column.0;
        let mut shape = content.cursor.shape;
        if !focused && shape != CursorShape::Hidden {
            shape = CursorShape::HollowBlock;
        }

        let cursor = if shape == CursorShape::Hidden
            || cursor_line < 0
            || cursor_line as usize >= lines
            || cursor_column >= columns
        {
            None
        } else if shape == CursorShape::Block {
            // Bake the block cursor into the cell colors.
            let line = cursor_line as usize;
            let cell = &mut rows[line][cursor_column];
            cell.bg = cursor_color;
            cell.fg = palette.cursor_text;
            if cursor_column + 1 < columns && rows[line][cursor_column + 1].spacer {
                rows[line][cursor_column + 1].bg = cursor_color;
            }
            None
        } else {
            Some(FrameCursor {
                column: cursor_column,
                line: cursor_line as usize,
                shape,
                color: cursor_color,
            })
        };

        Self {
            rows,
            cursor,
            background,
            banner: None,
        }
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
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::Processor;

    fn term_with(bytes: &[u8]) -> Term<VoidListener> {
        let mut term = Term::new(Config::default(), &TermSize::new(10, 3), VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, bytes);
        term
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
        let frame = Frame::capture(&term_with(b"hello\r\nworld"), &Palette::default(), true);
        assert_eq!((frame.rows[0].len(), frame.rows.len()), (10, 3));
        assert_eq!(row_text(&frame, 0), "hello     ");
        assert_eq!(row_text(&frame, 1), "world     ");
    }

    #[test]
    fn applies_sgr_attributes() {
        let p = Palette::default();
        let frame = Frame::capture(&term_with(b"\x1b[1;31mR\x1b[0m\x1b[4mU"), &p, true);
        let r = frame.rows[0][0];
        assert!(r.bold);
        assert_eq!(r.fg, p.indexed[1]);
        assert!(frame.rows[0][1].underline);
        assert!(!frame.rows[0][1].bold);
    }

    #[test]
    fn truecolor_background() {
        let frame = Frame::capture(&term_with(b"\x1b[48;2;1;2;3mX"), &Palette::default(), true);
        assert_eq!(frame.rows[0][0].bg, Rgb { r: 1, g: 2, b: 3 });
    }

    #[test]
    fn block_cursor_is_baked_into_cell() {
        let p = Palette::default();
        let frame = Frame::capture(&term_with(b"ab"), &p, true);
        assert_eq!(frame.cursor, None);
        assert_eq!(frame.rows[0][2].bg, p.cursor);
        assert_eq!(frame.rows[0][2].fg, p.cursor_text);
    }

    #[test]
    fn unfocused_cursor_is_hollow() {
        let frame = Frame::capture(&term_with(b"ab"), &Palette::default(), false);
        let cursor = frame.cursor.expect("cursor visible");
        assert_eq!((cursor.column, cursor.line), (2, 0));
        assert_eq!(cursor.shape, CursorShape::HollowBlock);
    }

    #[test]
    fn hidden_cursor_is_not_drawn() {
        let p = Palette::default();
        let frame = Frame::capture(&term_with(b"\x1b[?25l"), &p, true);
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
        let frame = Frame::capture(&term, &p, true);
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
        let frame = Frame::capture(&term, &p, true);
        assert_eq!(frame.rows[0][1].fg, white);
        assert_eq!(frame.rows[0][0].fg, p.indexed[1]);
    }

    #[test]
    fn selected_wide_char_is_fully_highlighted() {
        let p = Palette::default();
        let mut term = term_with("x日\r\n".as_bytes());
        select(&mut term, (0, 1), (0, 1));
        let frame = Frame::capture(&term, &p, true);
        assert!(frame.rows[0][2].spacer);
        for column in [1, 2] {
            assert_eq!(frame.rows[0][column].bg, p.selection_background);
        }
        assert_eq!(frame.rows[0][0].bg, p.background);

        // Selecting only the spacer highlights the whole character too.
        select(&mut term, (0, 2), (0, 2));
        let frame = Frame::capture(&term, &p, true);
        for column in [1, 2] {
            assert_eq!(frame.rows[0][column].bg, p.selection_background);
        }
    }

    #[test]
    fn block_cursor_wins_over_selection() {
        let p = Palette::default();
        let mut term = term_with(b"ab");
        select(&mut term, (0, 0), (0, 4));
        let frame = Frame::capture(&term, &p, true);
        assert_eq!(frame.rows[0][2].bg, p.cursor);
        assert_eq!(frame.rows[0][2].fg, p.cursor_text);
        assert_eq!(frame.rows[0][1].bg, p.selection_background);
    }

    #[test]
    fn wide_chars_mark_spacer() {
        let frame = Frame::capture(&term_with("日x".as_bytes()), &Palette::default(), true);
        assert_eq!(frame.rows[0][0].c, '日');
        assert!(frame.rows[0][1].spacer);
        assert_eq!(frame.rows[0][2].c, 'x');
    }
}
