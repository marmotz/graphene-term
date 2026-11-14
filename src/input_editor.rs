//! Rich input editor: editor shortcuts at the prompt, the Graphene private key sequences
//! the shell integration binds, and buffer distances read from the grid (pure).
//!
//! The shell keeps ownership of the command line: Graphene only translates editor
//! shortcuts, clicks and selections into private sequences the integration scripts bind
//! to the shell's line editor. Distances are read from the grid, every edit is done by
//! the shell.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Term, TermMode};
use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::selection;
use crate::timeline::{Phase, Timeline};
use crate::timeline_selection::{End, TimelineSelection};
use crate::view::RowSource;

/// Whether the rich input editor is active: the shell's line editor reads a command
/// line with Graphene's key bindings (`prompt` signalled, no command running), the
/// terminal is not on the alternate screen, and the mouse is not reserved for an
/// application.
pub fn rich_mode(phase: Phase, editing: bool, mode: TermMode, mods: ModifiersState) -> bool {
    phase == Phase::Prompt
        && editing
        && !mode.contains(TermMode::ALT_SCREEN)
        && !selection::mouse_reserved(mode, mods)
}

/// Editor command a key press runs at the prompt: `None` outside the rich mode, or when
/// the key is not an editor shortcut, so the key follows the normal path.
pub fn prompt_command(key: &Key, mods: ModifiersState, rich: bool) -> Option<EditorCommand> {
    if rich {
        editor_command(key, mods)
    } else {
        None
    }
}

/// Bytes an editor command writes to the PTY: the motion of a move or an extension,
/// the move to the buffer end for `SelectAll`.
pub fn command_bytes(command: EditorCommand) -> &'static [u8] {
    match command {
        EditorCommand::Move(motion) | EditorCommand::Extend(motion) => motion_bytes(motion),
        EditorCommand::SelectAll => Sequence::BufferEnd.bytes(),
        EditorCommand::InsertNewline => Sequence::InsertNewline.bytes(),
        EditorCommand::DeleteWord(direction) => delete_word_bytes(direction),
    }
}

/// An `Extend` or a `SelectAll` written to the shell, waiting for the cursor move it
/// causes: the live cursor when the sequence was written, its line counted from the top
/// of the live history so the screen scrolling is not taken for a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingExtend {
    line: i32,
    column: Column,
}

impl PendingExtend {
    /// Waits for the cursor of the live grid `term` to leave where it stands now.
    pub fn at<L>(term: &Term<L>) -> Self {
        let (line, column) = absolute_cursor(term);
        PendingExtend { line, column }
    }

    /// The cursor point of `term` once it moved, `None` while it has not.
    pub fn moved<L>(self, term: &Term<L>) -> Option<Point> {
        (absolute_cursor(term) != (self.line, self.column)).then(|| term.grid().cursor.point)
    }
}

fn absolute_cursor<L>(term: &Term<L>) -> (i32, Column) {
    let grid = term.grid();
    let cursor = grid.cursor.point;
    (grid.history_size() as i32 + cursor.line.0, cursor.column)
}

/// Selection end at the live grid point `point`, before its cell: a keyboard selection
/// covers the characters between the two cursor positions.
pub fn cursor_end(point: Point) -> End {
    End {
        source: RowSource::Live,
        point,
        side: Side::Left,
    }
}

/// Selection an `Extend` grows: `selection` while it covers the content of the live
/// grid, otherwise an empty one at the cursor of `term`. Successive extensions keep
/// the first one's anchor.
pub fn extend_selection<L>(
    selection: Option<TimelineSelection>,
    timeline: &Timeline,
    term: &Term<L>,
) -> TimelineSelection {
    match selection {
        Some(selection) if selection.covers_live(timeline) && !selection.is_stale(timeline) => {
            selection
        }
        _ => {
            let cursor = cursor_end(term.grid().cursor.point);
            TimelineSelection::new(SelectionType::Simple, cursor, timeline, term)
        }
    }
}

/// Selection of `SelectAll`: anchored where the prompt starts (the cursor without a
/// prompt), up to the cursor of `term` until the move to the buffer end extends it.
pub fn select_all<L>(timeline: &Timeline, term: &Term<L>) -> TimelineSelection {
    let cursor = term.grid().cursor.point;
    let anchor = timeline.current().prompt_start(term).unwrap_or(cursor);
    let mut selection =
        TimelineSelection::new(SelectionType::Simple, cursor_end(anchor), timeline, term);
    selection.update(cursor_end(cursor), term);
    selection
}

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
/// `\n` and its trailing blank cells are ignored, except on the line of `keep` (the
/// shell cursor) up to its column: the blanks typed before the cursor are in the
/// buffer. Cells for which `skip` returns true (the right prompt) are not counted nor
/// treated as line content.
pub fn buffer_distance<T: EventListener>(
    term: &Term<T>,
    from: Point,
    to: Point,
    skip: impl Fn(Point, &Cell) -> bool,
    keep: Option<Point>,
) -> isize {
    distance(term, from, to, &skip, keep)
}

/// Moves writing which brings the shell cursor, at the grid point `cursor`, to the
/// clicked point `click`: `CharLeft` or `CharRight` repeated over their buffer distance.
///
/// A click before `floor` (where the prompt starts) counts from `floor`. The blanks
/// before the cursor on its line are typed text, so they count. A click past the end of
/// the text gives too many moves, clamped by the shell at the buffer ends.
pub fn click_moves<T: EventListener>(
    term: &Term<T>,
    cursor: Point,
    click: Point,
    floor: Option<Point>,
) -> Vec<u8> {
    let click = floor.map_or(click, |floor| click.max(floor));
    char_moves(buffer_distance(
        term,
        cursor,
        click,
        skip_none,
        Some(cursor),
    ))
}

/// Grid points bounding a selection typing replaces: its first selected cell and the
/// point past its last one. `None` unless it lies on the live grid alone, from where
/// the prompt starts on (the cursor stands in the input), so a selection over the
/// output keeps the normal path. Block selections are never replaced.
pub fn input_selection<L>(
    selection: &TimelineSelection,
    timeline: &Timeline,
    term: &Term<L>,
) -> Option<(Point, Point)> {
    let floor = timeline.current().prompt_start(term)?;
    let (ty, upper, lower) = selection.live_ends(timeline, term)?;
    let (start, end) = match ty {
        SelectionType::Block => return None,
        // The ends of a keyboard or a dragged selection, as cell sides: a lower end
        // left of the first cell of a line takes the line break before it.
        SelectionType::Simple => (past(upper.point, upper.side), past(lower.point, lower.side)),
        // Words and lines grow from their ends: alacritty's range, inclusive.
        SelectionType::Semantic | SelectionType::Lines => {
            let mut grown = Selection::new(ty, upper.point, upper.side);
            grown.update(lower.point, lower.side);
            let range = grown.to_range(term)?;
            (range.start, past(range.end, Side::Right))
        }
    };
    (start >= floor && start < end).then_some((start, end))
}

/// The point before the cell side `side` of `point`: the cell itself for its left
/// side, the next column for its right side (past the last column at a line end).
fn past(point: Point, side: Side) -> Point {
    match side {
        Side::Left => point,
        Side::Right => Point::new(point.line, point.column + 1),
    }
}

/// Text a key types in place of an input selection: nothing for `Backspace` and
/// `Delete`, the typed characters for a printable key (`Shift` allowed). `None` for any
/// other key or with `Ctrl`, `Alt` or `Super`, which keep the normal path.
pub fn replacing_text(key: &Key, text: Option<&str>, mods: ModifiersState) -> Option<Vec<u8>> {
    if mods.intersects(ModifiersState::CONTROL | ModifiersState::ALT | ModifiersState::SUPER) {
        return None;
    }
    match key {
        Key::Named(NamedKey::Backspace | NamedKey::Delete) => Some(Vec::new()),
        Key::Named(NamedKey::Space) => Some(b" ".to_vec()),
        _ => text
            .filter(|text| !text.is_empty() && !text.chars().any(char::is_control))
            .map(|text| text.as_bytes().to_vec()),
    }
}

/// What an editor command types in place of an input selection: nothing for a word
/// deletion (the selection is deleted instead of the word), the line break sequence for
/// `InsertNewline`. `None` for the moves and extensions, which keep their own path.
pub fn command_replacing_text(command: EditorCommand) -> Option<&'static [u8]> {
    match command {
        EditorCommand::DeleteWord(_) => Some(b""),
        EditorCommand::InsertNewline => Some(Sequence::InsertNewline.bytes()),
        EditorCommand::Move(_) | EditorCommand::Extend(_) | EditorCommand::SelectAll => None,
    }
}

/// Bytes replacing the input selection `(start, end)` by `typed`, the shell cursor at
/// the grid point `cursor`, in one write: the moves to the selection end, one `0x7f`
/// (`Backspace`) per selected character, then the typed text.
///
/// A selection starting on the prompt text counts its characters too: the extra
/// backspaces are absorbed by the shell at the buffer start.
pub fn replacement_bytes<T: EventListener>(
    term: &Term<T>,
    cursor: Point,
    (start, end): (Point, Point),
    typed: &[u8],
) -> Vec<u8> {
    let keep = Some(cursor);
    let moves = buffer_distance(term, cursor, end, skip_none, keep);
    let selected = buffer_distance(term, start, end, skip_none, keep);
    let mut bytes = char_moves(moves);
    bytes.extend(std::iter::repeat_n(0x7f, selected.unsigned_abs()));
    bytes.extend_from_slice(typed);
    bytes
}

/// Body of [`buffer_distance`], the skip predicate borrowed for the recursion.
fn distance<T: EventListener>(
    term: &Term<T>,
    from: Point,
    to: Point,
    skip: &impl Fn(Point, &Cell) -> bool,
    keep: Option<Point>,
) -> isize {
    if to < from {
        return -distance(term, to, from, skip, keep);
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
                .max(
                    keep.filter(|keep| keep.line == line)
                        .map_or(0, |keep| keep.column.0),
                )
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
        buffer_distance(term, from, to, skip_none, None)
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
            buffer_distance(&term, point(0, 2), point(1, 2), skip_right_prompt, None),
            5
        );
        assert_eq!(
            buffer_distance(&term, point(0, 2), point(0, 9), skip_right_prompt, None),
            2
        );
    }

    #[test]
    fn rich_mode_needs_an_editing_prompt_on_the_main_screen() {
        let empty = TermMode::empty();
        assert!(rich_mode(Phase::Prompt, true, empty, NONE));
        // No `prompt` signal: no integration, or the prompt not drawn yet.
        assert!(!rich_mode(Phase::Prompt, false, empty, NONE));
        assert!(!rich_mode(Phase::Running, true, empty, NONE));
        assert!(!rich_mode(Phase::Running, false, empty, NONE));
        assert!(!rich_mode(Phase::Prompt, true, TermMode::ALT_SCREEN, NONE));
        // Mouse reserved for an application, unless `Shift` forces Graphene's.
        assert!(!rich_mode(
            Phase::Prompt,
            true,
            TermMode::MOUSE_REPORT_CLICK,
            NONE
        ));
        assert!(rich_mode(
            Phase::Prompt,
            true,
            TermMode::MOUSE_REPORT_CLICK,
            SHIFT
        ));
    }

    #[test]
    fn prompt_commands_only_in_rich_mode() {
        let home = named(NamedKey::Home);
        assert_eq!(
            prompt_command(&home, SHIFT, true),
            Some(EditorCommand::Extend(Motion::Line(Backward)))
        );
        assert_eq!(prompt_command(&home, SHIFT, false), None);
        assert_eq!(prompt_command(&home, NONE, false), None);
        assert_eq!(prompt_command(&character("a"), CTRL, false), None);
    }

    #[test]
    fn ctrl_a_still_sends_0x01_outside_rich_mode() {
        let key = character("a");
        assert_eq!(prompt_command(&key, CTRL, false), None);
        // In rich mode it selects the input instead.
        assert_eq!(
            prompt_command(&key, CTRL, true),
            Some(EditorCommand::SelectAll)
        );
        assert_eq!(
            crate::input::key_to_bytes(&key, Some("a"), CTRL, TermMode::empty()).as_deref(),
            Some(&[0x01][..])
        );
    }

    #[test]
    fn command_bytes_write_the_private_sequences() {
        use EditorCommand::*;
        assert_eq!(
            command_bytes(Move(Motion::Word(Backward))),
            Sequence::WordLeft.bytes()
        );
        assert_eq!(
            command_bytes(Move(Motion::Buffer(Forward))),
            Sequence::BufferEnd.bytes()
        );
        assert_eq!(
            command_bytes(InsertNewline),
            Sequence::InsertNewline.bytes()
        );
        assert_eq!(
            command_bytes(DeleteWord(Forward)),
            Sequence::DeleteWordRight.bytes()
        );
        // An extension moves like the plain move; `SelectAll` goes to the buffer end.
        assert_eq!(
            command_bytes(Extend(Motion::Char(Forward))),
            Sequence::CharRight.bytes()
        );
        assert_eq!(
            command_bytes(Extend(Motion::Line(Backward))),
            Sequence::LineStart.bytes()
        );
        assert_eq!(command_bytes(SelectAll), Sequence::BufferEnd.bytes());
    }

    #[test]
    fn click_after_the_cursor_moves_right() {
        // Cursor after "$ echo hello" at column 12, back at column 7 ("hello" start).
        let term = term_with(20, 3, "$ echo hello\x1b[8G");
        let cursor = term.grid().cursor.point;
        assert_eq!(cursor, point(0, 7));
        assert_eq!(
            click_moves(&term, cursor, point(0, 10), Some(point(0, 0))),
            repeat(Forward, 3)
        );
    }

    #[test]
    fn click_before_the_cursor_moves_left() {
        let term = term_with(20, 3, "$ echo hello");
        let cursor = term.grid().cursor.point;
        assert_eq!(
            click_moves(&term, cursor, point(0, 4), Some(point(0, 0))),
            repeat(Backward, 8)
        );
        assert!(click_moves(&term, cursor, cursor, Some(point(0, 0))).is_empty());
    }

    #[test]
    fn click_on_the_prompt_is_clamped_to_its_start() {
        // Output above the prompt, which starts on line 1.
        let term = term_with(20, 4, "motd\r\n$ ls");
        let cursor = term.grid().cursor.point;
        let floor = Some(point(1, 0));
        // On the motd: counted from the prompt start.
        assert_eq!(
            click_moves(&term, cursor, point(0, 2), floor),
            repeat(Backward, 4)
        );
        // On the prompt itself: moves back over it, the shell stops at the buffer start.
        assert_eq!(
            click_moves(&term, cursor, point(1, 1), floor),
            repeat(Backward, 3)
        );
    }

    #[test]
    fn blanks_typed_before_the_cursor_count() {
        // "ls  " typed: the cursor stands after two blanks.
        let term = term_with(20, 3, "$ ls  ");
        let cursor = term.grid().cursor.point;
        assert_eq!(cursor, point(0, 6));
        assert_eq!(
            click_moves(&term, cursor, point(0, 2), None),
            repeat(Backward, 4)
        );
    }

    #[test]
    fn click_on_a_line_below_counts_the_line_break() {
        // Multi-line input "ab" then "cd", cursor at the end of "ab".
        let term = term_with(10, 3, "$ ab\r\ncd\x1b[1;5H");
        let cursor = term.grid().cursor.point;
        assert_eq!(cursor, point(0, 4));
        assert_eq!(
            click_moves(&term, cursor, point(1, 1), None),
            repeat(Forward, 2)
        );
        // Past the end of a line: its end, not the next line.
        let term = term_with(10, 3, "$ ab\r\ncd\x1b[1;3H");
        let cursor = term.grid().cursor.point;
        assert_eq!(
            click_moves(&term, cursor, point(0, 8), None),
            repeat(Forward, 2)
        );
    }

    // Keyboard selection: a session whose prompt `$ ` was signalled, `typed` after it.

    fn editing(typed: &str) -> crate::timeline::testing::Session {
        let mut session = crate::timeline::testing::Session::new(100);
        session.feed(&format!(
            "motd\r\n{}$ {typed}",
            crate::timeline::testing::prompt()
        ));
        session
    }

    /// Text of the selection over the session's grids.
    fn selected(session: &crate::timeline::testing::Session) -> Option<String> {
        crate::timeline_selection::text(&session.term, &session.timeline)
    }

    /// Applies an extension the way the app does once the cursor moved.
    fn finish(
        session: &mut crate::timeline::testing::Session,
        pending: PendingExtend,
        selection: &mut TimelineSelection,
    ) -> bool {
        let Some(cursor) = pending.moved(&session.term) else {
            return false;
        };
        selection.update(cursor_end(cursor), &session.term);
        crate::timeline_selection::apply(Some(selection), &mut session.term, &mut session.timeline);
        true
    }

    #[test]
    fn extension_waits_for_the_cursor_to_move() {
        let mut session = editing("echo hi");
        let pending = PendingExtend::at(&session.term);
        assert_eq!(pending.moved(&session.term), None);
        // Output that leaves the cursor in place is no move.
        session.feed("\x1b[s\x1b[u");
        assert_eq!(pending.moved(&session.term), None);
        // The shell moves it two characters left.
        session.feed("\x08\x08");
        assert_eq!(pending.moved(&session.term), Some(point(1, 7)));
    }

    #[test]
    fn screen_scroll_alone_is_no_move() {
        let mut session = editing("ls");
        let pending = PendingExtend::at(&session.term);
        // A line feed at the bottom, the cursor back on the same text: same absolute
        // place, the view line changed.
        session.feed("\r\n\r\n\r\n\r\n\x1b[4A\x1b[4C");
        assert!(session.term.grid().history_size() > 0);
        assert_eq!(pending.moved(&session.term), None);
    }

    #[test]
    fn extension_starts_at_the_cursor_and_follows_it() {
        let mut session = editing("echo hello");
        let mut selection = extend_selection(None, &session.timeline, &session.term);
        assert!(selection.is_empty(&session.timeline, &session.term));
        let pending = PendingExtend::at(&session.term);
        // Not applied before the cursor moved.
        assert!(!finish(&mut session, pending, &mut selection));
        session.feed("\x08\x08\x08");
        assert!(finish(&mut session, pending, &mut selection));
        assert_eq!(selected(&session).as_deref(), Some("llo"));
    }

    #[test]
    fn successive_extensions_keep_the_anchor() {
        let mut session = editing("echo hello");
        let selection = extend_selection(None, &session.timeline, &session.term);
        let pending = PendingExtend::at(&session.term);
        session.feed("\x08");
        let mut selection = Some(selection);
        assert!(finish(&mut session, pending, selection.as_mut().unwrap()));
        assert_eq!(selected(&session).as_deref(), Some("o"));

        // The next `Shift+Left` grows the same selection.
        let mut selection = extend_selection(selection, &session.timeline, &session.term);
        let pending = PendingExtend::at(&session.term);
        session.feed("\x08\x08");
        assert!(finish(&mut session, pending, &mut selection));
        assert_eq!(selected(&session).as_deref(), Some("llo"));

        // Back right onto the anchor: nothing selected any more.
        let mut selection = extend_selection(Some(selection), &session.timeline, &session.term);
        let pending = PendingExtend::at(&session.term);
        session.feed("\x1b[3C");
        assert!(finish(&mut session, pending, &mut selection));
        assert!(selection.is_empty(&session.timeline, &session.term));
    }

    #[test]
    fn extension_past_the_anchor_flips_to_its_other_side() {
        // Cursor on "w" of "echo hello world", anchor there.
        let mut session = editing("echo hello world\x1b[5D");
        let mut selection = extend_selection(None, &session.timeline, &session.term);
        let pending = PendingExtend::at(&session.term);
        session.feed("\x1b[3D");
        assert!(finish(&mut session, pending, &mut selection));
        // "lo " selected, copied without its trailing blank.
        assert_eq!(selected(&session).as_deref(), Some("lo"));

        let mut selection = extend_selection(Some(selection), &session.timeline, &session.term);
        let pending = PendingExtend::at(&session.term);
        session.feed("\x1b[6C");
        assert!(finish(&mut session, pending, &mut selection));
        assert_eq!(selected(&session).as_deref(), Some("wor"));
    }

    #[test]
    fn extension_restarts_once_the_command_ran() {
        let mut session = editing("ls");
        let selection = extend_selection(None, &session.timeline, &session.term);
        session.feed(&format!(
            "\r\n{}{}{}$ ",
            crate::timeline::testing::start("ls"),
            crate::timeline::testing::end(0),
            crate::timeline::testing::prompt()
        ));
        assert!(selection.is_stale(&session.timeline));
        let fresh = extend_selection(Some(selection), &session.timeline, &session.term);
        assert!(!fresh.is_stale(&session.timeline));
        assert!(fresh.is_empty(&session.timeline, &session.term));
    }

    #[test]
    fn select_all_is_anchored_at_the_prompt_start() {
        let mut session = editing("echo a\x1b[3D");
        let mut selection = select_all(&session.timeline, &session.term);
        crate::timeline_selection::apply(
            Some(&selection),
            &mut session.term,
            &mut session.timeline,
        );
        // Up to the cursor before the shell moved it.
        assert_eq!(selected(&session).as_deref(), Some("$ ech"));
        let pending = PendingExtend::at(&session.term);
        session.feed("\x1b[3C");
        assert!(finish(&mut session, pending, &mut selection));
        assert_eq!(selected(&session).as_deref(), Some("$ echo a"));
    }

    #[test]
    fn select_all_with_the_cursor_at_the_end_is_complete_at_once() {
        let mut session = editing("pwd");
        let selection = select_all(&session.timeline, &session.term);
        crate::timeline_selection::apply(
            Some(&selection),
            &mut session.term,
            &mut session.timeline,
        );
        assert_eq!(selected(&session).as_deref(), Some("$ pwd"));
    }

    #[test]
    fn keyboard_selection_keeps_its_snapshot_text() {
        let mut session = editing("echo hello");
        let mut selection = extend_selection(None, &session.timeline, &session.term);
        let pending = PendingExtend::at(&session.term);
        session.feed("\x08\x08");
        assert!(finish(&mut session, pending, &mut selection));
        let mut snapshot = crate::selection::SelectionSnapshot::capture(&session.term).unwrap();
        // The shell redraws the line unchanged: the selection survives.
        session.feed("\r\x1b[K$ echo hello\x1b[2D");
        assert!(snapshot.validate(&mut session.term));
        assert_eq!(selected(&session).as_deref(), Some("lo"));
    }

    // Selection replacement.

    fn live(line: i32, column: usize, side: Side) -> End {
        End {
            source: RowSource::Live,
            point: point(line, column),
            side,
        }
    }

    /// A selection of kind `ty` from `from` to `to`, set on the grids.
    fn select_in(
        session: &mut crate::timeline::testing::Session,
        ty: SelectionType,
        from: End,
        to: End,
    ) -> TimelineSelection {
        let mut selection = TimelineSelection::new(ty, from, &session.timeline, &session.term);
        selection.update(to, &session.term);
        crate::timeline_selection::apply(
            Some(&selection),
            &mut session.term,
            &mut session.timeline,
        );
        selection
    }

    /// Bytes typing `typed` over `selection`, `None` when it is not in the input.
    fn replace(
        session: &crate::timeline::testing::Session,
        selection: &TimelineSelection,
        typed: &[u8],
    ) -> Option<Vec<u8>> {
        let bounds = input_selection(selection, &session.timeline, &session.term)?;
        let cursor = session.term.grid().cursor.point;
        Some(replacement_bytes(&session.term, cursor, bounds, typed))
    }

    fn expected(moves: isize, erased: usize, typed: &[u8]) -> Vec<u8> {
        let mut bytes = char_moves(moves);
        bytes.extend(vec![0x7f; erased]);
        bytes.extend_from_slice(typed);
        bytes
    }

    #[test]
    fn typing_over_a_selection_before_the_cursor() {
        // "$ echo hello", cursor at the end, "hello" selected.
        let mut session = editing("echo hello");
        let selection = select_in(
            &mut session,
            SelectionType::Simple,
            live(1, 7, Side::Left),
            live(1, 12, Side::Left),
        );
        assert_eq!(selected(&session).as_deref(), Some("hello"));
        assert_eq!(
            replace(&session, &selection, b"X"),
            Some(expected(0, 5, b"X"))
        );
    }

    #[test]
    fn typing_over_a_selection_after_the_cursor() {
        // Cursor on "e" of "echo", "hello" selected: move right to its end first.
        let mut session = editing("echo hello\x1b[10D");
        assert_eq!(session.term.grid().cursor.point, point(1, 2));
        let selection = select_in(
            &mut session,
            SelectionType::Simple,
            live(1, 7, Side::Left),
            live(1, 12, Side::Left),
        );
        assert_eq!(
            replace(&session, &selection, b"world"),
            Some(expected(10, 5, b"world"))
        );
    }

    #[test]
    fn typing_over_a_selection_holding_the_cursor() {
        // Cursor on the second "l" of "hello", "hello" selected.
        let mut session = editing("echo hello\x1b[3D");
        let selection = select_in(
            &mut session,
            SelectionType::Simple,
            live(1, 12, Side::Left),
            live(1, 7, Side::Left),
        );
        assert_eq!(
            replace(&session, &selection, b"a"),
            Some(expected(3, 5, b"a"))
        );
    }

    #[test]
    fn backspace_over_a_selection_erases_it_only() {
        let mut session = editing("echo hello\x1b[3D");
        let selection = select_in(
            &mut session,
            SelectionType::Simple,
            live(1, 2, Side::Left),
            live(1, 6, Side::Left),
        );
        // Back left to the end of "echo", then four backspaces.
        assert_eq!(
            replace(&session, &selection, b""),
            Some(expected(-3, 4, b""))
        );
    }

    #[test]
    fn multi_line_selection_counts_its_line_breaks() {
        // Input "ab" then "cd" on the next line, cursor at the end of "cd".
        let mut session = editing("ab\r\ncd");
        assert_eq!(session.term.grid().cursor.point, point(2, 2));
        let selection = select_in(
            &mut session,
            SelectionType::Simple,
            live(1, 3, Side::Left),
            live(2, 1, Side::Left),
        );
        // "b", the line break, "c".
        assert_eq!(
            replace(&session, &selection, b""),
            Some(expected(-1, 3, b""))
        );

        // A keyboard selection ending left of the next line's first cell holds the line
        // break.
        let selection = select_in(
            &mut session,
            SelectionType::Simple,
            live(1, 4, Side::Left),
            live(2, 0, Side::Left),
        );
        assert_eq!(
            replace(&session, &selection, b""),
            Some(expected(-2, 1, b""))
        );
    }

    #[test]
    fn dragged_and_double_clicked_selections_cover_their_cells() {
        let mut session = editing("echo hello");
        // Dragged from the right side of the blank to the right side of the last "l".
        let selection = select_in(
            &mut session,
            SelectionType::Simple,
            live(1, 6, Side::Right),
            live(1, 10, Side::Right),
        );
        assert_eq!(selected(&session).as_deref(), Some("hell"));
        assert_eq!(
            replace(&session, &selection, b""),
            Some(expected(-1, 4, b""))
        );

        // A double click on "hello" grows to the word.
        let selection = select_in(
            &mut session,
            SelectionType::Semantic,
            live(1, 8, Side::Left),
            live(1, 8, Side::Right),
        );
        assert_eq!(selected(&session).as_deref(), Some("hello"));
        assert_eq!(
            replace(&session, &selection, b""),
            Some(expected(0, 5, b""))
        );
    }

    #[test]
    fn selection_from_the_prompt_text_counts_it_too() {
        // "$ e" selected from the prompt start: three backspaces, the two extra ones
        // absorbed by the shell at the buffer start.
        let mut session = editing("echo");
        let selection = select_in(
            &mut session,
            SelectionType::Simple,
            live(1, 0, Side::Left),
            live(1, 3, Side::Left),
        );
        assert_eq!(
            replace(&session, &selection, b""),
            Some(expected(-3, 3, b""))
        );
    }

    #[test]
    fn selection_outside_the_input_keeps_the_normal_path() {
        let mut session = editing("echo hello");
        // From the output above the prompt into the input.
        let selection = select_in(
            &mut session,
            SelectionType::Simple,
            live(0, 0, Side::Left),
            live(1, 4, Side::Left),
        );
        assert_eq!(replace(&session, &selection, b"x"), None);
        // A block selection in the input.
        let selection = select_in(
            &mut session,
            SelectionType::Block,
            live(1, 2, Side::Left),
            live(1, 6, Side::Left),
        );
        assert_eq!(replace(&session, &selection, b"x"), None);
        // An empty selection.
        let selection = select_in(
            &mut session,
            SelectionType::Simple,
            live(1, 4, Side::Left),
            live(1, 4, Side::Left),
        );
        assert_eq!(replace(&session, &selection, b"x"), None);
    }

    #[test]
    fn selection_of_a_finished_command_is_not_replaced() {
        let mut session = editing("ls");
        let selection = select_in(
            &mut session,
            SelectionType::Simple,
            live(1, 2, Side::Left),
            live(1, 4, Side::Left),
        );
        session.feed(&crate::timeline::testing::start("ls"));
        assert_eq!(replace(&session, &selection, b"x"), None);
    }

    #[test]
    fn keys_replacing_a_selection() {
        assert_eq!(
            replacing_text(&named(NamedKey::Backspace), None, NONE),
            Some(Vec::new())
        );
        assert_eq!(
            replacing_text(&named(NamedKey::Delete), None, NONE),
            Some(Vec::new())
        );
        assert_eq!(
            replacing_text(&named(NamedKey::Space), Some(" "), NONE),
            Some(b" ".to_vec())
        );
        assert_eq!(
            replacing_text(&character("a"), Some("a"), NONE),
            Some(b"a".to_vec())
        );
        assert_eq!(
            replacing_text(&character("A"), Some("A"), SHIFT),
            Some(b"A".to_vec())
        );
        assert_eq!(
            replacing_text(&character("é"), Some("é"), NONE),
            Some("é".as_bytes().to_vec())
        );
        // Shortcuts, control keys and keys without text keep the normal path.
        assert_eq!(replacing_text(&character("c"), Some("c"), CTRL), None);
        assert_eq!(replacing_text(&character("b"), Some("b"), ALT), None);
        assert_eq!(
            replacing_text(&named(NamedKey::Backspace), None, CTRL),
            None
        );
        assert_eq!(
            replacing_text(&named(NamedKey::Enter), Some("\r"), NONE),
            None
        );
        assert_eq!(
            replacing_text(&named(NamedKey::Tab), Some("\t"), NONE),
            None
        );
        assert_eq!(
            replacing_text(&named(NamedKey::ArrowLeft), None, NONE),
            None
        );
        assert_eq!(
            replacing_text(&named(NamedKey::Escape), Some("\x1b"), NONE),
            None
        );
    }

    #[test]
    fn commands_replacing_a_selection() {
        assert_eq!(
            command_replacing_text(EditorCommand::DeleteWord(Backward)),
            Some(&b""[..])
        );
        assert_eq!(
            command_replacing_text(EditorCommand::DeleteWord(Forward)),
            Some(&b""[..])
        );
        assert_eq!(
            command_replacing_text(EditorCommand::InsertNewline),
            Some(Sequence::InsertNewline.bytes())
        );
        assert_eq!(
            command_replacing_text(EditorCommand::Move(Motion::Char(Backward))),
            None
        );
        assert_eq!(
            command_replacing_text(EditorCommand::Extend(Motion::Char(Forward))),
            None
        );
        assert_eq!(command_replacing_text(EditorCommand::SelectAll), None);
    }
}
