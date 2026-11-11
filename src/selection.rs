//! Pure helpers for mouse selection: click counting, selection kind and copied text cleanup.

use std::time::{Duration, Instant};

use alacritty_terminal::index::Point;
use alacritty_terminal::selection::SelectionType;
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
}
