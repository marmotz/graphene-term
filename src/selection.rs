//! Mouse selection logic: click counting, selection kind, capture rule, selection updates
//! on the `Term` and copied text cleanup. No window or clipboard access.

use std::time::{Duration, Instant};

use alacritty_terminal::index::{Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::{Term, TermMode};
use winit::keyboard::ModifiersState;

/// Characters ending a word on double-click.
///
/// Alacritty's default list without `:`, so that a URL (`https://...`) or `host:port`
/// stays one word.
pub const SEMANTIC_ESCAPE_CHARS: &str = ",│`|\"' ()[]{}<>\t";

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

/// Left press on `point`: with `Shift` held, extends the existing selection; otherwise
/// starts a new one whose kind depends on the click count. A single click starts an empty
/// selection, which renders nothing.
pub fn start_selection<T>(
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

/// Left release: an empty selection (click without drag) is cleared and `None` returned;
/// otherwise returns the cleaned selected text.
pub fn finish_selection<T>(term: &mut Term<T>) -> Option<String> {
    if term.selection.as_ref().is_none_or(Selection::is_empty) {
        term.selection = None;
        return None;
    }
    selected_text(term)
}

/// Cleaned text of the current selection, `None` without selection.
pub fn selected_text<T>(term: &Term<T>) -> Option<String> {
    term.selection_to_string().map(|text| clean_copied_text(&text))
}

/// Cleans a selection before it reaches the clipboard: trailing whitespace removed from
/// every line, common indentation of non-blank lines removed, no trailing newline.
pub fn clean_copied_text(text: &str) -> String {
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
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::index::{Column, Line, Side};
    use alacritty_terminal::selection::Selection;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::term::{self, Config, Term};
    use alacritty_terminal::vte::ansi::Processor;

    fn point(line: i32, column: usize) -> Point {
        Point::new(Line(line), Column(column))
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
    fn escape_chars_are_alacritty_defaults_without_colon() {
        assert_eq!(
            SEMANTIC_ESCAPE_CHARS,
            term::SEMANTIC_ESCAPE_CHARS.replace(':', "")
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
        let mut term = Term::new(Config::default(), &TermSize::new(columns, lines), VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, content);
        term
    }

    #[test]
    fn clicks_are_reserved_only_in_mouse_mode_without_shift() {
        let none = ModifiersState::empty();
        assert!(!mouse_reserved(TermMode::empty(), none));
        assert!(mouse_reserved(TermMode::MOUSE_REPORT_CLICK, none));
        assert!(mouse_reserved(TermMode::MOUSE_DRAG, ModifiersState::CONTROL));
        assert!(mouse_reserved(TermMode::MOUSE_MOTION, none));
        assert!(!mouse_reserved(TermMode::MOUSE_REPORT_CLICK, ModifiersState::SHIFT));
        assert!(!mouse_reserved(TermMode::BRACKETED_PASTE, none));
    }

    #[test]
    fn single_click_without_drag_clears_the_selection() {
        let mut term = term_with(20, 2, b"hello world");
        start_selection(&mut term, point(0, 3), Side::Left, 1, ModifiersState::empty());
        assert!(term.selection.is_some());
        assert_eq!(finish_selection(&mut term), None);
        assert!(term.selection.is_none());
    }

    #[test]
    fn drag_selects_cells() {
        let mut term = term_with(20, 2, b"hello world");
        start_selection(&mut term, point(0, 0), Side::Left, 1, ModifiersState::empty());
        term.selection.as_mut().unwrap().update(point(0, 4), Side::Right);
        assert_eq!(finish_selection(&mut term).as_deref(), Some("hello"));
        assert!(term.selection.is_some());
    }

    #[test]
    fn double_click_selects_a_word() {
        let mut term = term_with(20, 2, b"hello world");
        start_selection(&mut term, point(0, 8), Side::Left, 2, ModifiersState::empty());
        assert_eq!(finish_selection(&mut term).as_deref(), Some("world"));
    }

    #[test]
    fn shift_click_extends_the_existing_selection() {
        let mut term = term_with(20, 2, b"hello world");
        start_selection(&mut term, point(0, 0), Side::Left, 1, ModifiersState::empty());
        term.selection.as_mut().unwrap().update(point(0, 1), Side::Right);
        start_selection(&mut term, point(0, 7), Side::Right, 1, ModifiersState::SHIFT);
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
        start_selection(&mut term, point(0, 0), Side::Left, 1, ModifiersState::empty());
        term.selection.as_mut().unwrap().update(point(1, 4), Side::Right);
        assert_eq!(finish_selection(&mut term).as_deref(), Some("abcdefghijklmno"));
    }

    #[test]
    fn line_selection_is_copied_without_trailing_newline() {
        let mut term = term_with(10, 3, b"  abcdefghijklmno\r\nnext");
        start_selection(&mut term, point(1, 2), Side::Left, 3, ModifiersState::empty());
        assert_eq!(term.selection_to_string().as_deref(), Some("  abcdefghijklmno\n"));
        assert_eq!(finish_selection(&mut term).as_deref(), Some("abcdefghijklmno"));
    }

    #[test]
    fn no_selection_has_no_text() {
        let term = term_with(10, 2, b"abc");
        assert_eq!(selected_text(&term), None);
    }
}
