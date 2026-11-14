//! Image anchoring with kitty's Unicode placeholders.
//!
//! An image on screen is a rectangle of cells holding [`PLACEHOLDER`]: the image id in
//! the foreground color (low 24 bits, most significant byte in a third diacritic), the
//! placement id in the underline color, and the cell's row and column in the image as
//! diacritics. `Term` then scrolls, erases, keeps in history and clears these cells like
//! text, and the renderer draws the image over them.

use std::collections::BTreeSet;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::vte::ansi::{Color, Handler as _, Rgb};

use super::diacritics::DIACRITICS;

/// Character of the cells covered by an image.
pub const PLACEHOLDER: char = '\u{10EEEE}';

/// Largest row or column a diacritic can encode, plus one: an image is clipped to that
/// many rows and columns.
pub const MAX_CELLS: u32 = DIACRITICS.len() as u32;

/// Number encoded by the diacritic `c`, `None` when `c` is not one.
pub fn diacritic_value(c: char) -> Option<u32> {
    DIACRITICS.binary_search(&c).ok().map(|index| index as u32)
}

/// One cell of an image, as decoded from its placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageCell {
    pub image: u32,
    /// 0 when the cell names no placement.
    pub placement: u32,
    /// Row of the image shown by the cell.
    pub row: u32,
    /// Column of the image shown by the cell.
    pub col: u32,
}

/// What one placeholder cell says by itself, before the missing row and column are
/// inferred from its left neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Raw {
    image_low: u32,
    placement: u32,
    row: Option<u32>,
    col: Option<u32>,
    image_high: Option<u32>,
}

/// 24-bit number carried by a color: an RGB color, or a 256-color index.
fn color_number(color: Color) -> Option<u32> {
    match color {
        Color::Spec(Rgb { r, g, b }) => Some(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)),
        Color::Indexed(index) => Some(u32::from(index)),
        Color::Named(_) => None,
    }
}

fn color_of(number: u32) -> Color {
    Color::Spec(Rgb {
        r: (number >> 16) as u8,
        g: (number >> 8) as u8,
        b: number as u8,
    })
}

fn raw(cell: &Cell) -> Option<Raw> {
    if cell.c != PLACEHOLDER {
        return None;
    }
    let image_low = color_number(cell.fg)?;
    let placement = cell.underline_color().and_then(color_number).unwrap_or(0);
    let mut numbers = cell
        .zerowidth()
        .unwrap_or_default()
        .iter()
        .map(|&c| diacritic_value(c));
    let mut next = || numbers.next().flatten();
    let row = next();
    let col = row.and(next());
    let image_high = col.and(next());
    Some(Raw {
        image_low,
        placement,
        row,
        col,
        image_high,
    })
}

/// Image cells of a row of cells, `None` for the cells that are not placeholders. A
/// missing row, column or id high byte is inferred from the left neighbour when it
/// shows the same image and placement (column + 1), as the protocol allows; else they
/// are 0.
pub fn row_cells<'a>(cells: impl IntoIterator<Item = &'a Cell>) -> Vec<Option<ImageCell>> {
    let mut out: Vec<Option<ImageCell>> = Vec::new();
    for cell in cells {
        let decoded = raw(cell).map(|raw| {
            let left = out.last().copied().flatten().filter(|left| {
                left.image & 0x00ff_ffff == raw.image_low
                    && left.placement == raw.placement
                    && raw.row.is_none_or(|row| row == left.row)
                    && raw.image_high.is_none_or(|high| high == left.image >> 24)
            });
            let row = raw.row.or(left.map(|left| left.row)).unwrap_or(0);
            let col = raw.col.or(left.map(|left| left.col + 1)).unwrap_or(0);
            let high = raw
                .image_high
                .or(left.map(|left| left.image >> 24))
                .unwrap_or(0);
            ImageCell {
                image: high << 24 | raw.image_low,
                placement: raw.placement,
                row,
                col,
            }
        });
        out.push(decoded);
    }
    out
}

/// Image and placement of a placeholder cell, without inference from its neighbours.
fn cell_ids(cell: &Cell) -> Option<(u32, u32)> {
    raw(cell).map(|raw| {
        (
            raw.image_high.unwrap_or(0) << 24 | raw.image_low,
            raw.placement,
        )
    })
}

/// Where [`place`] leaves the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorMove {
    /// Where it was (kitty `C=1`), on the same text when the image scrolled the screen.
    Keep,
    /// After the image's last column, on its last row (kitty).
    After,
    /// On the line below the image, at its first column (sixel).
    #[cfg_attr(not(test), allow(dead_code, reason = "used by the sixel handler"))]
    Below,
}

/// Writes placement `placement` of image `image`, `cols` x `rows` cells, at the cursor
/// of `term`: the rows go down from the cursor line (scrolling at the bottom of the
/// scroll region like printed text), and each is clipped at the right edge, never
/// wrapped. The cursor template is restored, and the cursor left as `cursor` says.
pub fn place<L: EventListener>(
    term: &mut Term<L>,
    image: u32,
    placement: u32,
    cols: u32,
    rows: u32,
    cursor: CursorMove,
) {
    let cols = cols.min(MAX_CELLS);
    let rows = rows.min(MAX_CELLS);
    if cols == 0 || rows == 0 {
        return;
    }
    let saved = term.grid().cursor.clone();
    let start = saved.point.column;
    let fit = (cols as usize).min(term.columns().saturating_sub(start.0)) as u32;

    let mut template = Cell {
        fg: color_of(image & 0x00ff_ffff),
        ..Cell::default()
    };
    if placement != 0 {
        template.set_underline_color(Some(color_of(placement)));
    }
    term.grid_mut().cursor.template = template;

    let high = DIACRITICS[(image >> 24) as usize];
    let mut scrolled = 0;
    for row in 0..rows {
        if row > 0 {
            scrolled += linefeed(term);
        }
        go_to_column(term, start);
        for col in 0..fit {
            term.input(PLACEHOLDER);
            term.input(DIACRITICS[row as usize]);
            term.input(DIACRITICS[col as usize]);
            term.input(high);
        }
    }

    term.grid_mut().cursor.template = saved.template.clone();
    match cursor {
        CursorMove::Keep => {
            let line = saved.point.line - scrolled as i32;
            let grid = term.grid_mut();
            grid.cursor.point.line = line.max(Line(0));
            grid.cursor.point.column = start;
            grid.cursor.input_needs_wrap = saved.input_needs_wrap;
        }
        CursorMove::After => {
            let column = start.0 + cols as usize;
            let last = term.columns() - 1;
            let grid = term.grid_mut();
            grid.cursor.point.column = Column(column.min(last));
            grid.cursor.input_needs_wrap = column > last;
        }
        CursorMove::Below => {
            linefeed(term);
            go_to_column(term, start);
        }
    }
}

/// Moves the cursor one line down, scrolling at the bottom of the scroll region.
/// Returns 1 when the screen scrolled, 0 when the cursor moved.
fn linefeed<L: EventListener>(term: &mut Term<L>) -> u32 {
    let before = term.grid().cursor.point.line;
    term.linefeed();
    u32::from(term.grid().cursor.point.line == before)
}

fn go_to_column<L: EventListener>(term: &mut Term<L>, column: Column) {
    let grid = term.grid_mut();
    grid.cursor.point.column = column;
    grid.cursor.input_needs_wrap = false;
}

/// Blanks the placeholder cells of `image` (of its placement `placement` only when
/// given) on the screen and in the history of `term`. Returns how many were blanked.
pub fn erase<L>(term: &mut Term<L>, image: u32, placement: Option<u32>) -> usize {
    let grid = term.grid_mut();
    let top = grid.topmost_line();
    let bottom = grid.bottommost_line();
    let columns = grid.columns();
    let mut erased = 0;
    let mut line = top;
    while line <= bottom {
        let row = &mut grid[line];
        for column in 0..columns {
            let cell = &mut row[Column(column)];
            let matches = cell_ids(cell).is_some_and(|(id, place)| {
                id == image && placement.is_none_or(|placement| placement == place)
            });
            if matches {
                *cell = Cell::default();
                erased += 1;
            }
        }
        line += 1;
    }
    erased
}

/// Image and placement ids of the placeholder cells of the screen (not the history)
/// at the points `select` accepts.
pub fn on_screen<L>(term: &Term<L>, mut select: impl FnMut(Point) -> bool) -> BTreeSet<(u32, u32)> {
    let grid = term.grid();
    let mut ids = BTreeSet::new();
    for line in 0..grid.screen_lines() as i32 {
        for (column, cell) in row_cells(&grid[Line(line)]).into_iter().enumerate() {
            if let Some(cell) = cell
                && select(Point::new(Line(line), Column(column)))
            {
                ids.insert((cell.image, cell.placement));
            }
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::index::Point;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    use super::*;

    fn term(columns: usize, lines: usize) -> Term<VoidListener> {
        let config = Config {
            scrolling_history: 100,
            ..Config::default()
        };
        Term::new(config, &TermSize::new(columns, lines), VoidListener)
    }

    fn feed(term: &mut Term<VoidListener>, text: &str) {
        let mut processor: Processor<StdSyncHandler> = Processor::new();
        processor.advance(term, text.as_bytes());
    }

    fn line_cells(term: &Term<VoidListener>, line: i32) -> Vec<Option<ImageCell>> {
        row_cells(&term.grid()[Line(line)])
    }

    fn cell(image: u32, placement: u32, row: u32, col: u32) -> Option<ImageCell> {
        Some(ImageCell {
            image,
            placement,
            row,
            col,
        })
    }

    #[test]
    fn diacritics_encode_their_index() {
        assert_eq!(diacritic_value('\u{0305}'), Some(0));
        assert_eq!(diacritic_value('\u{030D}'), Some(1));
        assert_eq!(diacritic_value('\u{1D244}'), Some(296));
        assert_eq!(diacritic_value('a'), None);
        assert!(DIACRITICS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn place_writes_ids_and_diacritics_and_restores_the_template() {
        let mut term = term(10, 5);
        feed(&mut term, "\x1b[1;31mab");
        let image = 0x0201_0203;
        place(&mut term, image, 7, 3, 2, CursorMove::After);

        let first = &term.grid()[Line(0)][Column(2)];
        assert_eq!(first.c, PLACEHOLDER);
        assert_eq!(first.fg, Color::Spec(Rgb { r: 1, g: 2, b: 3 }));
        assert_eq!(
            first.underline_color(),
            Some(Color::Spec(Rgb { r: 0, g: 0, b: 7 }))
        );
        assert_eq!(
            first.zerowidth().unwrap(),
            &[DIACRITICS[0], DIACRITICS[0], DIACRITICS[2]]
        );
        assert_eq!(
            line_cells(&term, 1)[2..6],
            [
                cell(image, 7, 1, 0),
                cell(image, 7, 1, 1),
                cell(image, 7, 1, 2),
                None
            ]
        );
        assert_eq!(line_cells(&term, 0)[..2], [None, None]);

        // The text attributes in use before the image are back.
        feed(&mut term, "c");
        let after = &term.grid()[Line(1)][Column(5)];
        assert_eq!(after.c, 'c');
        assert_eq!(
            after.fg,
            Color::Named(alacritty_terminal::vte::ansi::NamedColor::Red)
        );
        assert!(after.underline_color().is_none());
    }

    #[test]
    fn place_without_placement_sets_no_underline_color() {
        let mut term = term(10, 5);
        place(&mut term, 5, 0, 1, 1, CursorMove::After);
        let first = &term.grid()[Line(0)][Column(0)];
        assert!(first.underline_color().is_none());
        assert_eq!(line_cells(&term, 0)[0], cell(5, 0, 0, 0));
    }

    #[test]
    fn place_clips_at_the_right_edge() {
        let mut term = term(6, 4);
        feed(&mut term, "abcd");
        place(&mut term, 1, 1, 5, 2, CursorMove::After);
        for line in 0..2 {
            let cells = line_cells(&term, line);
            assert_eq!(cells[4], cell(1, 1, line as u32, 0));
            assert_eq!(cells[5], cell(1, 1, line as u32, 1));
        }
        // Nothing wrapped onto the next lines.
        assert!(line_cells(&term, 2).iter().all(Option::is_none));
        assert_eq!(term.grid()[Line(1)][Column(0)].c, ' ');
    }

    #[test]
    fn place_scrolls_at_the_bottom_and_the_image_survives_in_history() {
        let mut term = term(8, 3);
        feed(&mut term, "top\r\n\r\n");
        place(&mut term, 9, 2, 2, 3, CursorMove::After);
        // Two rows did not fit: the screen scrolled twice, `top` went to history.
        assert_eq!(term.grid().history_size(), 2);
        assert_eq!(term.grid()[Line(-2)][Column(0)].c, 't');
        assert_eq!(line_cells(&term, 0)[0], cell(9, 2, 0, 0));
        assert_eq!(line_cells(&term, 2)[1], cell(9, 2, 2, 1));

        // More output scrolls the image into the history, cells intact.
        feed(&mut term, "\r\n\r\n\r\n");
        assert_eq!(line_cells(&term, -3)[0], cell(9, 2, 0, 0));
        assert_eq!(line_cells(&term, -1)[1], cell(9, 2, 2, 1));
    }

    #[test]
    fn place_leaves_the_cursor_as_asked() {
        let mut after = term(10, 5);
        feed(&mut after, "ab");
        place(&mut after, 1, 1, 3, 2, CursorMove::After);
        assert_eq!(after.grid().cursor.point, Point::new(Line(1), Column(5)));

        let mut below = term(10, 5);
        feed(&mut below, "ab");
        place(&mut below, 1, 1, 3, 2, CursorMove::Below);
        assert_eq!(below.grid().cursor.point, Point::new(Line(2), Column(2)));

        let mut keep = term(10, 5);
        feed(&mut keep, "ab");
        place(&mut keep, 1, 1, 3, 2, CursorMove::Keep);
        assert_eq!(keep.grid().cursor.point, Point::new(Line(0), Column(2)));
    }

    #[test]
    fn place_after_at_the_right_edge_wraps_the_next_text() {
        let mut term = term(5, 4);
        feed(&mut term, "ab");
        place(&mut term, 1, 1, 3, 1, CursorMove::After);
        assert!(term.grid().cursor.input_needs_wrap);
        feed(&mut term, "x");
        assert_eq!(term.grid()[Line(1)][Column(0)].c, 'x');
    }

    #[test]
    fn place_keep_follows_the_text_when_the_screen_scrolls() {
        let mut term = term(8, 3);
        feed(&mut term, "\r\n\r\nab");
        place(&mut term, 1, 1, 1, 2, CursorMove::Keep);
        assert_eq!(term.grid().cursor.point, Point::new(Line(1), Column(2)));
    }

    #[test]
    fn erase_blanks_one_placement_or_the_whole_image_in_screen_and_history() {
        let mut term = term(8, 3);
        place(&mut term, 4, 1, 2, 1, CursorMove::Below);
        place(&mut term, 4, 2, 2, 1, CursorMove::Below);
        place(&mut term, 5, 1, 2, 1, CursorMove::Below);
        feed(&mut term, "\r\n\r\n");
        assert!(term.grid().history_size() > 0);

        assert_eq!(erase(&mut term, 4, Some(1)), 2);
        let all: Vec<_> = (-(term.grid().history_size() as i32)..3)
            .flat_map(|line| line_cells(&term, line))
            .flatten()
            .collect();
        assert_eq!(all.len(), 4);
        assert!(
            all.iter()
                .all(|cell| !(cell.image == 4 && cell.placement == 1))
        );

        assert_eq!(erase(&mut term, 4, None), 2);
        assert_eq!(erase(&mut term, 5, None), 2);
        assert_eq!(erase(&mut term, 5, None), 0);
    }

    #[test]
    fn text_over_an_image_and_erase_in_line_remove_its_cells() {
        let mut term = term(8, 3);
        place(&mut term, 3, 1, 4, 1, CursorMove::Keep);
        feed(&mut term, "x\x1b[2C\x1b[K");
        assert_eq!(
            line_cells(&term, 0)[..4],
            [None, cell(3, 1, 0, 1), cell(3, 1, 0, 2), None]
        );
    }

    #[test]
    fn missing_column_and_high_byte_are_inferred_from_the_left_neighbour() {
        let mut term = term(8, 2);
        // Row and column on the first cell only, high byte 1; the next cells say nothing.
        let row = DIACRITICS[2];
        let col = DIACRITICS[4];
        let high = DIACRITICS[1];
        let text = format!(
            "\x1b[38;2;0;0;9m\x1b[58;2;0;0;3m{PLACEHOLDER}{row}{col}{high}{PLACEHOLDER}{PLACEHOLDER}{row}"
        );
        feed(&mut term, &text);
        let image = 1 << 24 | 9;
        assert_eq!(
            line_cells(&term, 0)[..4],
            [
                cell(image, 3, 2, 4),
                cell(image, 3, 2, 5),
                cell(image, 3, 2, 6),
                None
            ]
        );
    }

    #[test]
    fn a_lone_placeholder_without_diacritics_is_row_and_column_zero() {
        let mut term = term(8, 2);
        feed(&mut term, &format!("\x1b[38;5;42m{PLACEHOLDER}"));
        assert_eq!(line_cells(&term, 0)[0], cell(42, 0, 0, 0));
        // Without an id color, it is not an image cell.
        let mut plain = super::tests::term(8, 2);
        feed(&mut plain, &PLACEHOLDER.to_string());
        assert_eq!(line_cells(&plain, 0)[0], None);
    }

    #[test]
    fn on_screen_lists_the_selected_cells_of_the_screen_only() {
        let mut term = term(8, 3);
        place(&mut term, 4, 1, 2, 2, CursorMove::After);
        place(&mut term, 0x0100_0005, 2, 1, 1, CursorMove::After);
        assert_eq!(
            on_screen(&term, |_| true),
            BTreeSet::from([(4, 1), (0x0100_0005, 2)])
        );
        assert_eq!(
            on_screen(&term, |point| point == Point::new(Line(1), Column(1))),
            BTreeSet::from([(4, 1)])
        );
        assert_eq!(
            on_screen(&term, |point| point.column == Column(2)),
            BTreeSet::from([(0x0100_0005, 2)])
        );

        // Scrolled into the history: no longer on the screen.
        feed(&mut term, "\r\n\r\n\r\n");
        assert!(on_screen(&term, |_| true).is_empty());
    }
}
