//! Mouse selection logic: click counting, selection kind, capture rule, the live
//! selection snapshot, copied text cleanup and the images inside a selection. No window
//! or clipboard access.

use std::time::{Duration, Instant};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::{Term, TermMode};
use winit::keyboard::ModifiersState;

use crate::graphics::placeholder::{self, PLACEHOLDER};

/// Maximum delay between two clicks of the same series.
const MULTI_CLICK_DELAY: Duration = Duration::from_millis(400);

/// Counts consecutive clicks on the same cell (single, double, triple click).
#[derive(Debug, Default)]
pub struct ClickTracker {
    last: Option<(Point, Instant)>,
    count: u8,
}

impl ClickTracker {
    /// Registers a click on `point` at `now` and returns its rank in the series: 1, 2 or 3.
    /// A click on another cell, after the delay, or after a triple click starts over at 1.
    pub fn click(&mut self, point: Point, now: Instant) -> u8 {
        let continues = self.last.is_some_and(|(last_point, last_time)| {
            last_point == point && now.saturating_duration_since(last_time) <= MULTI_CLICK_DELAY
        });
        self.count = if continues && self.count < 3 {
            self.count + 1
        } else {
            1
        };
        self.last = Some((point, now));
        self.count
    }

    /// Rank of the last click in its series, 0 before the first click.
    pub fn count(&self) -> u8 {
        self.count
    }
}

/// Selection kind for a click series: 1 = character (block with `Ctrl`), 2 = word, 3 = line.
pub fn selection_type(clicks: u8, mods: ModifiersState) -> SelectionType {
    match clicks {
        2 => SelectionType::Semantic,
        3 => SelectionType::Lines,
        _ if mods.control_key() => SelectionType::Block,
        _ => SelectionType::Simple,
    }
}

/// Whether left and middle clicks are reserved for the application: it enabled mouse
/// reporting and `Shift` is not held to force a local selection.
pub fn mouse_reserved(mode: TermMode, mods: ModifiersState) -> bool {
    mode.intersects(TermMode::MOUSE_MODE) && !mods.shift_key()
}

/// Maximum number of lines scrolled per auto-scroll step.
pub const AUTO_SCROLL_MAX_LINES: i32 = 10;

/// Delay between two auto-scroll steps while dragging past the grid edges.
pub const AUTO_SCROLL_INTERVAL: Duration = Duration::from_millis(50);

/// Lines to scroll the display by during a selection drag, as a `Scroll::Delta` count
/// (positive scrolls up into history). `overflow` is `GridLayout::vertical_overflow` of the
/// cursor: 1 line just past the edge, plus 1 per cell height further, capped at
/// `AUTO_SCROLL_MAX_LINES`. The alternate screen has no history: always 0 there.
pub fn auto_scroll_delta(overflow: i32, mode: TermMode) -> i32 {
    if mode.contains(TermMode::ALT_SCREEN) {
        return 0;
    }
    -overflow.clamp(-AUTO_SCROLL_MAX_LINES, AUTO_SCROLL_MAX_LINES)
}

/// Cleaned text of the current selection, `None` without selection.
pub fn selected_text<T>(term: &Term<T>) -> Option<String> {
    term.selection_to_string()
        .map(|text| clean_copied_text(&text))
}

/// Shadow copy of a finished selection and its cleaned text.
///
/// `Term` clears its selection on erase sequences (`ED`, `EL`, `RIS`) even when the same
/// text is redrawn right after, and keeps it when a cell is overwritten by a different
/// character. The snapshot restores the first case and clears the second: the selection
/// lives as long as its text is unchanged.
#[derive(Debug, Clone)]
pub struct SelectionSnapshot {
    selection: Selection,
    text: String,
}

impl SelectionSnapshot {
    /// Snapshot of the current non-empty selection of `term`, `None` without one.
    pub fn capture<T>(term: &Term<T>) -> Option<Self> {
        let selection = term.selection.clone().filter(|s| !s.is_empty())?;
        let text = selected_text(term)?;
        Some(Self { selection, text })
    }

    /// Checks the selection of `term` after new PTY output. Returns `false`, with the
    /// selection of `term` cleared, when the selected text changed: the snapshot must then
    /// be dropped.
    pub fn validate<T>(&mut self, term: &mut Term<T>) -> bool {
        match term.selection.as_ref() {
            // Carries the rotation applied by `Term` when the output scrolls.
            Some(selection) => self.selection = selection.clone(),
            None => term.selection = Some(self.selection.clone()),
        }
        if selected_text(term).is_some_and(|text| text == self.text) {
            return true;
        }
        term.selection = None;
        false
    }
}

/// Distinct image placements, as `(image, placement)`, with a placeholder cell inside
/// the selection of `term`, in reading order. Empty without selection.
pub fn selected_images<T>(term: &Term<T>) -> Vec<(u32, u32)> {
    let Some(range) = term.selection.as_ref().and_then(|s| s.to_range(term)) else {
        return Vec::new();
    };
    let grid = term.grid();
    let first = range.start.line.0.max(-(grid.history_size() as i32));
    let last = range.end.line.0.min(grid.screen_lines() as i32 - 1);
    let mut images = Vec::new();
    for line in first..=last {
        let cells = placeholder::row_cells(&grid[Line(line)]);
        for (column, cell) in cells.into_iter().enumerate() {
            let Some(cell) = cell else { continue };
            let ids = (cell.image, cell.placement);
            if range.contains(Point::new(Line(line), Column(column))) && !images.contains(&ids) {
                images.push(ids);
            }
        }
    }
    images
}

/// `text` without the image placeholder characters and the diacritics that follow them:
/// the cells of an image copy as nothing.
fn strip_placeholders(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut in_placeholder = false;
    for c in text.chars() {
        if c == PLACEHOLDER {
            in_placeholder = true;
        } else if !(in_placeholder && placeholder::diacritic_value(c).is_some()) {
            in_placeholder = false;
            stripped.push(c);
        }
    }
    stripped
}

/// Cleans a selection before it reaches the clipboard: image placeholders removed,
/// trailing whitespace removed from every line, common indentation of non-blank lines
/// removed, no trailing newline.
pub fn clean_copied_text(text: &str) -> String {
    let text = strip_placeholders(text);
    let lines: Vec<&str> = text.split('\n').map(str::trim_end).collect();

    let indent_of = |line: &str| line.len() - line.trim_start().len();
    let common = lines
        .iter()
        .filter(|line| !line.is_empty())
        .map(|line| &line[..indent_of(line)])
        .reduce(|acc, indent| common_prefix(acc, indent))
        .unwrap_or("");

    let mut cleaned = lines
        .iter()
        .map(|line| line.strip_prefix(common).unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    cleaned.truncate(cleaned.trim_end_matches('\n').len());
    cleaned
}

/// Longest common prefix of two strings, compared character by character.
fn common_prefix<'a>(a: &'a str, b: &str) -> &'a str {
    let len = a
        .char_indices()
        .zip(b.chars())
        .find(|((_, ca), cb)| ca != cb)
        .map_or(a.len().min(b.len()), |((i, _), _)| i);
    &a[..len]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SEMANTIC_ESCAPE_CHARS;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::index::{Column, Line, Side};
    use alacritty_terminal::selection::Selection;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::term::{Config, Term};
    use alacritty_terminal::vte::ansi::Processor;

    /// Left press on `point` in one grid: with `Shift` held, extends the existing
    /// selection; otherwise starts a new one whose kind depends on the click count. A
    /// single click starts an empty selection, which renders nothing.
    fn start_selection<T>(
        term: &mut Term<T>,
        point: Point,
        side: Side,
        clicks: u8,
        mods: ModifiersState,
    ) {
        match term.selection.as_mut() {
            Some(selection) if mods.shift_key() => selection.update(point, side),
            _ => term.selection = Some(Selection::new(selection_type(clicks, mods), point, side)),
        }
    }

    /// Left release in one grid: an empty selection (click without drag) is cleared and
    /// `None` returned; otherwise returns the cleaned selected text.
    fn finish_selection<T>(term: &mut Term<T>) -> Option<String> {
        if term.selection.as_ref().is_none_or(Selection::is_empty) {
            term.selection = None;
            return None;
        }
        selected_text(term)
    }

    fn point(line: i32, column: usize) -> Point {
        Point::new(Line(line), Column(column))
    }

    #[test]
    fn auto_scroll_is_idle_inside_the_grid() {
        assert_eq!(auto_scroll_delta(0, TermMode::empty()), 0);
    }

    #[test]
    fn auto_scroll_goes_up_into_history_above_the_grid() {
        assert_eq!(auto_scroll_delta(-1, TermMode::empty()), 1);
        assert_eq!(auto_scroll_delta(-3, TermMode::empty()), 3);
    }

    #[test]
    fn auto_scroll_goes_down_below_the_grid() {
        assert_eq!(auto_scroll_delta(1, TermMode::empty()), -1);
        assert_eq!(auto_scroll_delta(4, TermMode::empty()), -4);
    }

    #[test]
    fn auto_scroll_is_capped() {
        assert_eq!(
            auto_scroll_delta(-50, TermMode::empty()),
            AUTO_SCROLL_MAX_LINES
        );
        assert_eq!(
            auto_scroll_delta(50, TermMode::empty()),
            -AUTO_SCROLL_MAX_LINES
        );
    }

    #[test]
    fn auto_scroll_is_disabled_on_alternate_screen() {
        assert_eq!(auto_scroll_delta(-3, TermMode::ALT_SCREEN), 0);
        assert_eq!(auto_scroll_delta(3, TermMode::ALT_SCREEN), 0);
    }

    #[test]
    fn counts_consecutive_clicks_on_same_cell() {
        let mut tracker = ClickTracker::default();
        let t0 = Instant::now();
        assert_eq!(tracker.click(point(0, 0), t0), 1);
        assert_eq!(
            tracker.click(point(0, 0), t0 + Duration::from_millis(200)),
            2
        );
        assert_eq!(
            tracker.click(point(0, 0), t0 + Duration::from_millis(400)),
            3
        );
        assert_eq!(tracker.count(), 3);
    }

    #[test]
    fn fourth_click_starts_over() {
        let mut tracker = ClickTracker::default();
        let t0 = Instant::now();
        for _ in 0..3 {
            tracker.click(point(0, 0), t0);
        }
        assert_eq!(tracker.click(point(0, 0), t0), 1);
        assert_eq!(tracker.click(point(0, 0), t0), 2);
    }

    #[test]
    fn slow_click_starts_over() {
        let mut tracker = ClickTracker::default();
        let t0 = Instant::now();
        tracker.click(point(0, 0), t0);
        assert_eq!(
            tracker.click(point(0, 0), t0 + Duration::from_millis(401)),
            1
        );
    }

    #[test]
    fn click_on_other_cell_starts_over() {
        let mut tracker = ClickTracker::default();
        let t0 = Instant::now();
        tracker.click(point(0, 0), t0);
        assert_eq!(tracker.click(point(0, 1), t0), 1);
        assert_eq!(tracker.click(point(1, 1), t0), 1);
    }

    #[test]
    fn selection_type_per_click_count() {
        let none = ModifiersState::empty();
        assert_eq!(selection_type(1, none), SelectionType::Simple);
        assert_eq!(
            selection_type(1, ModifiersState::CONTROL),
            SelectionType::Block
        );
        assert_eq!(selection_type(2, none), SelectionType::Semantic);
        assert_eq!(
            selection_type(2, ModifiersState::CONTROL),
            SelectionType::Semantic
        );
        assert_eq!(selection_type(3, none), SelectionType::Lines);
    }

    #[test]
    fn removes_trailing_spaces() {
        assert_eq!(clean_copied_text("foo   \nbar\t \n"), "foo\nbar");
    }

    #[test]
    fn removes_every_trailing_newline() {
        assert_eq!(clean_copied_text("ls -la\n\n\n"), "ls -la");
        assert_eq!(clean_copied_text("\n"), "");
    }

    #[test]
    fn dedents_common_indentation() {
        assert_eq!(
            clean_copied_text("    fn a() {\n        b();\n    }"),
            "fn a() {\n    b();\n}"
        );
    }

    #[test]
    fn dedent_ignores_blank_lines() {
        assert_eq!(clean_copied_text("    a\n\n  \n    b"), "a\n\n\nb");
    }

    #[test]
    fn dedent_compares_tabs_and_spaces_literally() {
        assert_eq!(clean_copied_text("\t  a\n\t b"), " a\nb");
        assert_eq!(clean_copied_text("\ta\n    b"), "\ta\n    b");
    }

    #[test]
    fn keeps_powerline_glyphs_and_combining_chars() {
        let text = " \u{e0b0} main \u{f418} cafe\u{301}  \n";
        assert_eq!(
            clean_copied_text(text),
            "\u{e0b0} main \u{f418} cafe\u{301}"
        );
    }

    #[test]
    fn url_is_selected_as_one_word() {
        let config = Config {
            semantic_escape_chars: SEMANTIC_ESCAPE_CHARS.to_owned(),
            ..Default::default()
        };
        let mut term = Term::new(config, &TermSize::new(60, 2), VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, b"see https://example.com:8080/a?b=c ok");

        term.selection = Some(Selection::new(
            SelectionType::Semantic,
            point(0, 12),
            Side::Left,
        ));
        assert_eq!(
            term.selection_to_string().as_deref(),
            Some("https://example.com:8080/a?b=c")
        );
    }

    fn term_with(columns: usize, lines: usize, content: &[u8]) -> Term<VoidListener> {
        let mut term = Term::new(
            Config::default(),
            &TermSize::new(columns, lines),
            VoidListener,
        );
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, content);
        term
    }

    #[test]
    fn clicks_are_reserved_only_in_mouse_mode_without_shift() {
        let none = ModifiersState::empty();
        assert!(!mouse_reserved(TermMode::empty(), none));
        assert!(mouse_reserved(TermMode::MOUSE_REPORT_CLICK, none));
        assert!(mouse_reserved(
            TermMode::MOUSE_DRAG,
            ModifiersState::CONTROL
        ));
        assert!(mouse_reserved(TermMode::MOUSE_MOTION, none));
        assert!(!mouse_reserved(
            TermMode::MOUSE_REPORT_CLICK,
            ModifiersState::SHIFT
        ));
        assert!(!mouse_reserved(TermMode::BRACKETED_PASTE, none));
    }

    #[test]
    fn single_click_without_drag_clears_the_selection() {
        let mut term = term_with(20, 2, b"hello world");
        start_selection(
            &mut term,
            point(0, 3),
            Side::Left,
            1,
            ModifiersState::empty(),
        );
        assert!(term.selection.is_some());
        assert_eq!(finish_selection(&mut term), None);
        assert!(term.selection.is_none());
    }

    #[test]
    fn drag_selects_cells() {
        let mut term = term_with(20, 2, b"hello world");
        start_selection(
            &mut term,
            point(0, 0),
            Side::Left,
            1,
            ModifiersState::empty(),
        );
        term.selection
            .as_mut()
            .unwrap()
            .update(point(0, 4), Side::Right);
        assert_eq!(finish_selection(&mut term).as_deref(), Some("hello"));
        assert!(term.selection.is_some());
    }

    #[test]
    fn double_click_selects_a_word() {
        let mut term = term_with(20, 2, b"hello world");
        start_selection(
            &mut term,
            point(0, 8),
            Side::Left,
            2,
            ModifiersState::empty(),
        );
        assert_eq!(finish_selection(&mut term).as_deref(), Some("world"));
    }

    #[test]
    fn shift_click_extends_the_existing_selection() {
        let mut term = term_with(20, 2, b"hello world");
        start_selection(
            &mut term,
            point(0, 0),
            Side::Left,
            1,
            ModifiersState::empty(),
        );
        term.selection
            .as_mut()
            .unwrap()
            .update(point(0, 1), Side::Right);
        start_selection(
            &mut term,
            point(0, 7),
            Side::Right,
            1,
            ModifiersState::SHIFT,
        );
        assert_eq!(selected_text(&term).as_deref(), Some("hello wo"));
    }

    #[test]
    fn shift_click_without_selection_starts_one() {
        let mut term = term_with(20, 2, b"hello world");
        start_selection(&mut term, point(0, 2), Side::Left, 1, ModifiersState::SHIFT);
        let selection = term.selection.as_ref().unwrap();
        assert_eq!(selection.ty, SelectionType::Simple);
    }

    #[test]
    fn wrapped_line_is_copied_as_one_line() {
        let mut term = term_with(10, 3, b"abcdefghijklmno");
        start_selection(
            &mut term,
            point(0, 0),
            Side::Left,
            1,
            ModifiersState::empty(),
        );
        term.selection
            .as_mut()
            .unwrap()
            .update(point(1, 4), Side::Right);
        assert_eq!(
            finish_selection(&mut term).as_deref(),
            Some("abcdefghijklmno")
        );
    }

    #[test]
    fn line_selection_is_copied_without_trailing_newline() {
        let mut term = term_with(10, 3, b"  abcdefghijklmno\r\nnext");
        start_selection(
            &mut term,
            point(1, 2),
            Side::Left,
            3,
            ModifiersState::empty(),
        );
        assert_eq!(
            term.selection_to_string().as_deref(),
            Some("  abcdefghijklmno\n")
        );
        assert_eq!(
            finish_selection(&mut term).as_deref(),
            Some("abcdefghijklmno")
        );
    }

    #[test]
    fn no_selection_has_no_text() {
        let term = term_with(10, 2, b"abc");
        assert_eq!(selected_text(&term), None);
    }

    /// `term` with "hello" selected on the first line and its snapshot.
    fn snapshot_of_hello(content: &[u8]) -> (Term<VoidListener>, SelectionSnapshot) {
        let mut term = term_with(20, 3, content);
        start_selection(
            &mut term,
            point(0, 0),
            Side::Left,
            1,
            ModifiersState::empty(),
        );
        term.selection
            .as_mut()
            .unwrap()
            .update(point(0, 4), Side::Right);
        let snapshot = SelectionSnapshot::capture(&term).unwrap();
        (term, snapshot)
    }

    fn feed(term: &mut Term<VoidListener>, bytes: &[u8]) {
        let mut parser: Processor = Processor::new();
        parser.advance(term, bytes);
    }

    #[test]
    fn empty_selection_has_no_snapshot() {
        let mut term = term_with(20, 2, b"hello");
        assert!(SelectionSnapshot::capture(&term).is_none());
        start_selection(
            &mut term,
            point(0, 0),
            Side::Left,
            1,
            ModifiersState::empty(),
        );
        assert!(SelectionSnapshot::capture(&term).is_none());
    }

    #[test]
    fn unchanged_output_keeps_the_selection() {
        let (mut term, mut snapshot) = snapshot_of_hello(b"hello world");
        assert!(snapshot.validate(&mut term));
        assert_eq!(selected_text(&term).as_deref(), Some("hello"));
    }

    #[test]
    fn same_text_redrawn_after_erase_keeps_the_selection() {
        let (mut term, mut snapshot) = snapshot_of_hello(b"hello world");
        feed(&mut term, b"\x1b[H\x1b[2K");
        assert!(term.selection.is_none(), "Term clears the selection on EL");
        feed(&mut term, b"hello world");
        assert!(snapshot.validate(&mut term));
        assert_eq!(selected_text(&term).as_deref(), Some("hello"));
    }

    #[test]
    fn different_text_after_erase_clears_the_selection() {
        let (mut term, mut snapshot) = snapshot_of_hello(b"hello world");
        feed(&mut term, b"\x1b[H\x1b[2Kbye world");
        assert!(!snapshot.validate(&mut term));
        assert!(term.selection.is_none());
    }

    #[test]
    fn overwritten_character_clears_the_selection() {
        let (mut term, mut snapshot) = snapshot_of_hello(b"hello world");
        feed(&mut term, b"\x1b[HJ");
        assert!(
            term.selection.is_some(),
            "Term keeps the selection on overwrite"
        );
        assert!(!snapshot.validate(&mut term));
        assert!(term.selection.is_none());
    }

    #[test]
    fn scrolled_output_keeps_the_selection_on_its_text() {
        let (mut term, mut snapshot) = snapshot_of_hello(b"hello world\r\nsecond");
        feed(&mut term, b"\r\nthird\r\nfourth\r\nfifth");
        assert!(snapshot.validate(&mut term));
        assert_eq!(selected_text(&term).as_deref(), Some("hello"));
        // The refreshed snapshot follows the rotated selection.
        assert!(snapshot.validate(&mut term));
    }

    /// `ab`, a 2x2 image (id 7, placement 3) after them, then `cd` below the image, at its
    /// first column, and a 1x1 image (id 9, placement 1) after `cd`.
    fn term_with_images() -> Term<VoidListener> {
        use crate::graphics::{CursorMove, place};
        let mut term = term_with(10, 6, b"ab");
        place(&mut term, 7, 3, 2, 2, CursorMove::Below);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, b"cd");
        place(&mut term, 9, 1, 1, 1, CursorMove::After);
        term
    }

    fn select<T>(term: &mut Term<T>, from: Point, to: Point) {
        let mut selection = Selection::new(SelectionType::Simple, from, Side::Left);
        selection.update(to, Side::Right);
        term.selection = Some(selection);
    }

    #[test]
    fn placeholders_and_their_diacritics_are_stripped() {
        let row = '\u{305}';
        let col = '\u{30d}';
        let text = format!("a{PLACEHOLDER}{row}{col}{PLACEHOLDER}{row}b\u{301}");
        // Combining marks outside a placeholder are kept.
        assert_eq!(strip_placeholders(&text), "ab\u{301}");
    }

    #[test]
    fn copied_text_never_holds_placeholders() {
        let mut term = term_with_images();
        select(&mut term, point(0, 0), point(2, 9));
        let text = selected_text(&term).unwrap();
        assert!(!text.contains(PLACEHOLDER), "{text:?}");
        assert_eq!(text, "ab\n\n  cd");
    }

    #[test]
    fn selected_images_lists_each_placement_once_in_reading_order() {
        let mut term = term_with_images();
        select(&mut term, point(0, 0), point(2, 9));
        assert_eq!(selected_images(&term), vec![(7, 3), (9, 1)]);
    }

    #[test]
    fn selected_images_counts_only_cells_inside_the_selection() {
        let mut term = term_with_images();
        // Second row of the first image only.
        select(&mut term, point(1, 2), point(1, 3));
        assert_eq!(selected_images(&term), vec![(7, 3)]);
        // Text before the first image.
        select(&mut term, point(0, 0), point(0, 1));
        assert!(selected_images(&term).is_empty());
        term.selection = None;
        assert!(selected_images(&term).is_empty());
    }
}
