//! Rich input editor: editor shortcuts at the prompt, the Graphene private key sequences
//! the shell integration binds, and buffer distances read from the grid (pure).
//!
//! The shell keeps ownership of the command line: Graphene only translates editor
//! shortcuts, clicks and selections into private sequences the integration scripts bind
//! to the shell's line editor. Distances are read from the grid, every edit is done by
//! the shell.

// Wired into the keyboard and mouse paths by the rich mode (issue #32).
#![cfg_attr(not(test), allow(dead_code))]

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::{Cell, Flags};
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// Direction of a motion or a word deletion: backward is left (or up), forward is right
/// (or down).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Backward,
    Forward,
}

/// Cursor motion in the shell's buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// One character left or right.
    Char(Direction),
    /// One word left or right.
    Word(Direction),
    /// Start or end of the current line.
    Line(Direction),
    /// Same column on the previous or next line of a multi-line input.
    LineVertical(Direction),
    /// Start or end of the whole buffer.
    Buffer(Direction),
}

/// Editor command bound to a shortcut at the prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorCommand {
    /// Move the cursor, clearing the selection.
    Move(Motion),
    /// Move the cursor, extending the selection.
    Extend(Motion),
    /// Select the whole input.
    SelectAll,
    /// Insert a new line in the buffer without running the command.
    InsertNewline,
    /// Erase the word before or after the cursor.
    DeleteWord(Direction),
}

/// Returns the editor command bound to a key combination at the prompt (v1 table).
///
/// Every other key returns `None` and follows the normal path: `Tab`, `Up` / `Down`,
/// `Enter` and `Backspace` reach the shell unchanged. `Ctrl+Shift+` arrows are not
/// bound: they belong to pane focus, at the prompt too.
pub fn editor_command(key: &Key, mods: ModifiersState) -> Option<EditorCommand> {
    use Direction::{Backward, Forward};
    use EditorCommand::{DeleteWord, Extend, InsertNewline, Move, SelectAll};

    const NONE: ModifiersState = ModifiersState::empty();
    const CTRL: ModifiersState = ModifiersState::CONTROL;
    const SHIFT: ModifiersState = ModifiersState::SHIFT;

    match key {
        Key::Named(named) => {
            let command = match (named, mods) {
                (NamedKey::ArrowLeft, CTRL) => Move(Motion::Word(Backward)),
                (NamedKey::ArrowRight, CTRL) => Move(Motion::Word(Forward)),
                (NamedKey::Backspace, CTRL) => DeleteWord(Backward),
                (NamedKey::Delete, CTRL) => DeleteWord(Forward),
                (NamedKey::Home, NONE) => Move(Motion::Line(Backward)),
                (NamedKey::End, NONE) => Move(Motion::Line(Forward)),
                (NamedKey::Home, CTRL) => Move(Motion::Buffer(Backward)),
                (NamedKey::End, CTRL) => Move(Motion::Buffer(Forward)),
                (NamedKey::ArrowLeft, SHIFT) => Extend(Motion::Char(Backward)),
                (NamedKey::ArrowRight, SHIFT) => Extend(Motion::Char(Forward)),
                (NamedKey::ArrowUp, SHIFT) => Extend(Motion::LineVertical(Backward)),
                (NamedKey::ArrowDown, SHIFT) => Extend(Motion::LineVertical(Forward)),
                (NamedKey::Home, SHIFT) => Extend(Motion::Line(Backward)),
                (NamedKey::End, SHIFT) => Extend(Motion::Line(Forward)),
                (NamedKey::Enter, SHIFT) => InsertNewline,
                _ => return None,
            };
            Some(command)
        }
        Key::Character(c) if mods == CTRL && c.eq_ignore_ascii_case("a") => Some(SelectAll),
        _ => None,
    }
}

/// Graphene private key sequence, bound by the zsh, bash and fish integration scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sequence {
    CharLeft,
    CharRight,
    WordLeft,
    WordRight,
    DeleteWordLeft,
    DeleteWordRight,
    LineStart,
    LineEnd,
    LineUp,
    LineDown,
    BufferStart,
    BufferEnd,
    InsertNewline,
}

/// Every private sequence with its bytes: `CSI <code> u`, code points taken from the
/// Unicode private use area so no real key collides. Mirrored in the integration
/// scripts under `shell-integration/`.
pub const SEQUENCES: [(Sequence, &[u8]); 13] = [
    (Sequence::CharLeft, b"\x1b[59136u"),
    (Sequence::CharRight, b"\x1b[59137u"),
    (Sequence::WordLeft, b"\x1b[59138u"),
    (Sequence::WordRight, b"\x1b[59139u"),
    (Sequence::DeleteWordLeft, b"\x1b[59140u"),
    (Sequence::DeleteWordRight, b"\x1b[59141u"),
    (Sequence::LineStart, b"\x1b[59142u"),
    (Sequence::LineEnd, b"\x1b[59143u"),
    (Sequence::LineUp, b"\x1b[59144u"),
    (Sequence::LineDown, b"\x1b[59145u"),
    (Sequence::BufferStart, b"\x1b[59146u"),
    (Sequence::BufferEnd, b"\x1b[59147u"),
    (Sequence::InsertNewline, b"\x1b[59148u"),
];

impl Sequence {
    /// Bytes written to the PTY for this sequence.
    pub fn bytes(self) -> &'static [u8] {
        SEQUENCES[self as usize].1
    }
}

/// Private sequence moving the shell cursor by `motion`.
pub fn motion_sequence(motion: Motion) -> Sequence {
    use Direction::{Backward, Forward};
    match motion {
        Motion::Char(Backward) => Sequence::CharLeft,
        Motion::Char(Forward) => Sequence::CharRight,
        Motion::Word(Backward) => Sequence::WordLeft,
        Motion::Word(Forward) => Sequence::WordRight,
        Motion::Line(Backward) => Sequence::LineStart,
        Motion::Line(Forward) => Sequence::LineEnd,
        Motion::LineVertical(Backward) => Sequence::LineUp,
        Motion::LineVertical(Forward) => Sequence::LineDown,
        Motion::Buffer(Backward) => Sequence::BufferStart,
        Motion::Buffer(Forward) => Sequence::BufferEnd,
    }
}

/// Bytes moving the shell cursor by `motion`.
pub fn motion_bytes(motion: Motion) -> &'static [u8] {
    motion_sequence(motion).bytes()
}

/// Bytes erasing the word before or after the shell cursor.
pub fn delete_word_bytes(direction: Direction) -> &'static [u8] {
    match direction {
        Direction::Backward => Sequence::DeleteWordLeft.bytes(),
        Direction::Forward => Sequence::DeleteWordRight.bytes(),
    }
}

/// `n` times `CharLeft` (backward) or `CharRight` (forward).
pub fn repeat(direction: Direction, n: usize) -> Vec<u8> {
    motion_bytes(Motion::Char(direction)).repeat(n)
}

/// Character moves covering a signed buffer distance: `CharRight` when positive,
/// `CharLeft` when negative, nothing when zero.
pub fn char_moves(distance: isize) -> Vec<u8> {
    let direction = if distance < 0 {
        Direction::Backward
    } else {
        Direction::Forward
    };
    repeat(direction, distance.unsigned_abs())
}

/// Number of buffer characters between two grid points, `from` included and `to`
/// excluded, negative when `to` is before `from`.
///
/// One per cell, wide-char spacers skipped; a line end without `WRAPLINE` counts as one
/// `\n` and its trailing blank cells are ignored. Cells for which `skip` returns true
/// (the right prompt) are not counted nor treated as line content.
pub fn buffer_distance<T: EventListener>(
    term: &Term<T>,
    from: Point,
    to: Point,
    skip: impl Fn(Point, &Cell) -> bool,
) -> isize {
    if to < from {
        return -buffer_distance(term, to, from, skip);
    }
    let grid = term.grid();
    let columns = grid.columns();
    let ignored = |point: Point, cell: &Cell| {
        cell.flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            || skip(point, cell)
    };

    let mut distance = 0;
    let mut line = from.line;
    while line <= to.line {
        let row = &grid[line];
        let wrapped = row[Column(columns - 1)].flags.contains(Flags::WRAPLINE);
        // First column past the line's content, trailing blanks of a hard break excluded.
        let content_end = if wrapped {
            columns
        } else {
            (0..columns)
                .rev()
                .find(|&column| {
                    let point = Point::new(line, Column(column));
                    let cell = &row[Column(column)];
                    !ignored(point, cell) && cell.c != ' '
                })
                .map_or(0, |column| column + 1)
        };
        let start = if line == from.line { from.column.0 } else { 0 };
        let end = if line == to.line {
            to.column.0.min(content_end)
        } else {
            content_end
        };
        distance += (start..end)
            .filter(|&column| {
                let point = Point::new(line, Column(column));
                !ignored(point, &row[Column(column)])
            })
            .count() as isize;
        if line < to.line && !wrapped {
            distance += 1;
        }
        line += 1;
    }
    distance
}

/// Skip predicate for [`buffer_distance`] that skips no cell.
pub fn skip_none(_: Point, _: &Cell) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::index::Line;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::Processor;
    use winit::keyboard::SmolStr;

    use Direction::{Backward, Forward};

    fn named(key: NamedKey) -> Key {
        Key::Named(key)
    }

    fn character(c: &str) -> Key {
        Key::Character(SmolStr::new(c))
    }

    const NONE: ModifiersState = ModifiersState::empty();
    const CTRL: ModifiersState = ModifiersState::CONTROL;
    const SHIFT: ModifiersState = ModifiersState::SHIFT;
    const ALT: ModifiersState = ModifiersState::ALT;

    #[test]
    fn each_v1_shortcut_maps_to_its_command() {
        use EditorCommand::*;
        let cases = [
            (
                named(NamedKey::ArrowLeft),
                CTRL,
                Move(Motion::Word(Backward)),
            ),
            (
                named(NamedKey::ArrowRight),
                CTRL,
                Move(Motion::Word(Forward)),
            ),
            (named(NamedKey::Backspace), CTRL, DeleteWord(Backward)),
            (named(NamedKey::Delete), CTRL, DeleteWord(Forward)),
            (named(NamedKey::Home), NONE, Move(Motion::Line(Backward))),
            (named(NamedKey::End), NONE, Move(Motion::Line(Forward))),
            (named(NamedKey::Home), CTRL, Move(Motion::Buffer(Backward))),
            (named(NamedKey::End), CTRL, Move(Motion::Buffer(Forward))),
            (
                named(NamedKey::ArrowLeft),
                SHIFT,
                Extend(Motion::Char(Backward)),
            ),
            (
                named(NamedKey::ArrowRight),
                SHIFT,
                Extend(Motion::Char(Forward)),
            ),
            (
                named(NamedKey::ArrowUp),
                SHIFT,
                Extend(Motion::LineVertical(Backward)),
            ),
            (
                named(NamedKey::ArrowDown),
                SHIFT,
                Extend(Motion::LineVertical(Forward)),
            ),
            (named(NamedKey::Home), SHIFT, Extend(Motion::Line(Backward))),
            (named(NamedKey::End), SHIFT, Extend(Motion::Line(Forward))),
            (character("a"), CTRL, SelectAll),
            (character("A"), CTRL, SelectAll),
            (named(NamedKey::Enter), SHIFT, InsertNewline),
        ];
        for (key, mods, command) in cases {
            assert_eq!(
                editor_command(&key, mods),
                Some(command),
                "{key:?} {mods:?}"
            );
        }
    }

    #[test]
    fn keys_outside_the_table_are_not_mapped() {
        let cases = [
            (named(NamedKey::Tab), NONE),
            (named(NamedKey::ArrowUp), NONE),
            (named(NamedKey::ArrowDown), NONE),
            (named(NamedKey::ArrowLeft), NONE),
            (named(NamedKey::ArrowRight), NONE),
            (named(NamedKey::Enter), NONE),
            (named(NamedKey::Backspace), NONE),
            (named(NamedKey::Delete), NONE),
            (character("a"), NONE),
            (character("b"), CTRL),
            (character("a"), CTRL | SHIFT),
            (character("a"), CTRL | ALT),
            // Pane focus owns Ctrl+Shift+arrows, at the prompt too.
            (named(NamedKey::ArrowLeft), CTRL | SHIFT),
            (named(NamedKey::ArrowRight), CTRL | SHIFT),
            (named(NamedKey::ArrowUp), CTRL | SHIFT),
            (named(NamedKey::ArrowDown), CTRL | SHIFT),
            (named(NamedKey::ArrowLeft), ALT),
            (named(NamedKey::Home), CTRL | SHIFT),
            (named(NamedKey::Enter), CTRL),
        ];
        for (key, mods) in cases {
            assert_eq!(editor_command(&key, mods), None, "{key:?} {mods:?}");
        }
    }

    #[test]
    fn sequence_table_uses_private_csi_u_codes_in_order() {
        for (index, (sequence, bytes)) in SEQUENCES.iter().enumerate() {
            assert_eq!(*sequence as usize, index);
            assert_eq!(sequence.bytes(), *bytes);
            let expected = format!("\x1b[{}u", 59136 + index);
            assert_eq!(*bytes, expected.as_bytes(), "{sequence:?}");
        }
        assert_eq!(Sequence::InsertNewline.bytes(), b"\x1b[59148u");
    }

    #[test]
    fn motions_map_to_their_sequences() {
        let cases = [
            (Motion::Char(Backward), Sequence::CharLeft),
            (Motion::Char(Forward), Sequence::CharRight),
            (Motion::Word(Backward), Sequence::WordLeft),
            (Motion::Word(Forward), Sequence::WordRight),
            (Motion::Line(Backward), Sequence::LineStart),
            (Motion::Line(Forward), Sequence::LineEnd),
            (Motion::LineVertical(Backward), Sequence::LineUp),
            (Motion::LineVertical(Forward), Sequence::LineDown),
            (Motion::Buffer(Backward), Sequence::BufferStart),
            (Motion::Buffer(Forward), Sequence::BufferEnd),
        ];
        for (motion, sequence) in cases {
            assert_eq!(motion_bytes(motion), sequence.bytes(), "{motion:?}");
        }
        assert_eq!(delete_word_bytes(Backward), b"\x1b[59140u");
        assert_eq!(delete_word_bytes(Forward), b"\x1b[59141u");
    }

    #[test]
    fn char_moves_repeat_the_char_sequences() {
        assert_eq!(repeat(Backward, 2), b"\x1b[59136u\x1b[59136u");
        assert_eq!(repeat(Forward, 1), b"\x1b[59137u");
        assert!(repeat(Forward, 0).is_empty());
        assert_eq!(char_moves(-3), repeat(Backward, 3));
        assert_eq!(char_moves(2), repeat(Forward, 2));
        assert!(char_moves(0).is_empty());
    }

    fn term_with(columns: usize, lines: usize, content: &str) -> Term<VoidListener> {
        let mut term = Term::new(
            Config::default(),
            &TermSize::new(columns, lines),
            VoidListener,
        );
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, content.as_bytes());
        term
    }

    fn point(line: i32, column: usize) -> Point {
        Point::new(Line(line), Column(column))
    }

    fn distance(term: &Term<VoidListener>, from: Point, to: Point) -> isize {
        buffer_distance(term, from, to, skip_none)
    }

    #[test]
    fn plain_line_counts_one_per_cell() {
        let term = term_with(20, 3, "$ echo hello");
        assert_eq!(distance(&term, point(0, 2), point(0, 12)), 10);
        assert_eq!(distance(&term, point(0, 7), point(0, 7)), 0);
        assert_eq!(distance(&term, point(0, 7), point(0, 9)), 2);
    }

    #[test]
    fn wrapped_line_has_no_newline() {
        // 10 columns: "$ abcdefgh" fills line 0, "ij" wraps on line 1.
        let term = term_with(10, 3, "$ abcdefghij");
        assert!(
            term.grid()[Line(0)][Column(9)]
                .flags
                .contains(Flags::WRAPLINE)
        );
        assert_eq!(distance(&term, point(0, 2), point(1, 2)), 10);
        assert_eq!(distance(&term, point(0, 8), point(1, 1)), 3);
    }

    #[test]
    fn wide_char_spacers_are_skipped() {
        // Each CJK char takes two cells: the second is a spacer.
        let term = term_with(20, 3, "$ 日本x");
        assert_eq!(distance(&term, point(0, 2), point(0, 7)), 3);
        assert_eq!(distance(&term, point(0, 2), point(0, 4)), 1);
    }

    #[test]
    fn leading_wide_char_spacer_at_a_wrap_is_skipped() {
        // 5 columns: "$ ab" then the wide char does not fit in the last column, which
        // becomes a leading spacer, and wraps to line 1.
        let term = term_with(5, 3, "$ ab日c");
        assert!(
            term.grid()[Line(0)][Column(4)]
                .flags
                .contains(Flags::LEADING_WIDE_CHAR_SPACER)
        );
        assert_eq!(distance(&term, point(0, 2), point(1, 3)), 4);
    }

    #[test]
    fn hard_break_counts_one_newline_and_ignores_trailing_blanks() {
        // A two-line buffer "ab  \ncd": the trailing spaces of line 0 are not counted.
        let term = term_with(10, 3, "$ ab  \r\ncd");
        assert_eq!(distance(&term, point(0, 2), point(1, 2)), 5);
        // From a trailing blank of line 0: only the newline and "c".
        assert_eq!(distance(&term, point(0, 6), point(1, 1)), 2);
        // A target in the trailing blanks stops at the end of the text.
        assert_eq!(distance(&term, point(0, 2), point(0, 8)), 2);
    }

    #[test]
    fn empty_line_in_the_middle_counts_one_newline() {
        let term = term_with(10, 4, "$ a\r\n\r\nb");
        assert_eq!(distance(&term, point(0, 2), point(2, 1)), 4);
    }

    #[test]
    fn distance_is_negative_when_the_target_is_before() {
        let term = term_with(10, 3, "$ ab\r\ncd");
        assert_eq!(distance(&term, point(1, 2), point(0, 2)), -5);
        assert_eq!(distance(&term, point(0, 4), point(0, 2)), -2);
    }

    #[test]
    fn skipped_cells_are_not_counted_nor_content() {
        // A right prompt "RP" at the end of line 0, then "cd" on line 1.
        let term = term_with(10, 3, "$ ab    RP\r\ncd");
        let skip_right_prompt =
            |point: Point, _: &Cell| point.line == Line(0) && point.column.0 >= 8;
        assert_eq!(
            buffer_distance(&term, point(0, 2), point(1, 2), skip_right_prompt),
            5
        );
        assert_eq!(
            buffer_distance(&term, point(0, 2), point(0, 9), skip_right_prompt),
            2
        );
    }
}
