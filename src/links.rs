//! Links of the terminal grid: OSC 8 hyperlinks set by the application, and URLs
//! detected in the text. Also the pure decisions around them: whether a link is
//! Graphene's under the pointer, whether a click opens it, whether it may be opened.
//! No window, process or system access.

use std::cell::RefCell;
use std::ops::RangeInclusive;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::{Flags, Hyperlink};
use alacritty_terminal::term::search::RegexSearch;
use winit::keyboard::ModifiersState;

/// URLs detected in plain text: the four opened schemes, then every character that is
/// not a control, a blank, or a delimiter commonly put around a URL.
const URL_PATTERN: &str =
    "(https?://|file:|mailto:)[^\u{0000}-\u{001F}\u{007F}-\u{009F}<>\"\\s{|}\\\\^⟨⟩`]+";

/// Characters trimmed from the end of a detected URL: sentence punctuation and quotes.
const TRAILING_PUNCTUATION: &[char] = &['.', ',', ';', ':', '!', '?', '\'', '"'];

thread_local! {
    /// The URL search, built once per thread: building its automata is not free.
    static URL_SEARCH: RefCell<Option<RegexSearch>> = const { RefCell::new(None) };
}

/// A link found in a grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Cells of the link in the grid it was found in, wrapped lines included.
    pub range: RangeInclusive<Point>,
    pub uri: String,
    /// Set by the application with OSC 8, rather than detected in the text.
    pub explicit: bool,
}

/// The link covering `point` of `term`: the OSC 8 hyperlink of its cell, extended over
/// the adjacent cells with the same hyperlink; otherwise a URL detected on its logical
/// line (wrapped lines joined).
pub fn link_at<L>(term: &Term<L>, point: Point) -> Option<Link> {
    if !in_grid(term, point) {
        return None;
    }
    match hyperlink(term, point) {
        Some(link) => Some(explicit(term, point, &link)),
        None => detected(term, point),
    }
}

/// Whether `point` is a cell of `term`.
fn in_grid<L>(term: &Term<L>, point: Point) -> bool {
    point.line >= term.topmost_line()
        && point.line <= term.bottommost_line()
        && point.column <= term.last_column()
}

/// OSC 8 hyperlink of the cell at `point`. The spacer of a wide character carries the
/// hyperlink of the character.
fn hyperlink<L>(term: &Term<L>, point: Point) -> Option<Hyperlink> {
    let cell = &term.grid()[point];
    if cell.flags.contains(Flags::WIDE_CHAR_SPACER) && point.column.0 > 0 {
        return term.grid()[Point::new(point.line, point.column - 1)].hyperlink();
    }
    cell.hyperlink()
}

/// The cell before `point` on its logical line.
fn previous<L>(term: &Term<L>, point: Point) -> Option<Point> {
    if point.column.0 > 0 {
        return Some(Point::new(point.line, point.column - 1));
    }
    let above = point.line - 1i32;
    let wrapped = point.line > term.topmost_line()
        && term.grid()[above][term.last_column()]
            .flags
            .contains(Flags::WRAPLINE);
    wrapped.then(|| Point::new(above, term.last_column()))
}

/// The cell after `point` on its logical line.
fn next<L>(term: &Term<L>, point: Point) -> Option<Point> {
    if point.column < term.last_column() {
        return Some(Point::new(point.line, point.column + 1));
    }
    let wrapped = point.line < term.bottommost_line()
        && term.grid()[point.line][point.column]
            .flags
            .contains(Flags::WRAPLINE);
    wrapped.then(|| Point::new(point.line + 1i32, Column(0)))
}

/// The OSC 8 link `link` covering `point`, over every adjacent cell with the same id
/// and URI.
fn explicit<L>(term: &Term<L>, point: Point, link: &Hyperlink) -> Link {
    let same = |p: Point| {
        hyperlink(term, p).is_some_and(|other| other.id() == link.id() && other.uri() == link.uri())
    };
    let mut start = point;
    while let Some(p) = previous(term, start).filter(|&p| same(p)) {
        start = p;
    }
    let mut end = point;
    while let Some(p) = next(term, end).filter(|&p| same(p)) {
        end = p;
    }
    Link {
        range: start..=end,
        uri: link.uri().to_owned(),
        explicit: true,
    }
}

/// The URL detected on the logical line of `point` that covers it, trimmed.
fn detected<L>(term: &Term<L>, point: Point) -> Option<Link> {
    let start = term.line_search_left(point);
    let end = term.line_search_right(point);
    URL_SEARCH.with(|search| {
        let mut search = search.borrow_mut();
        let search = match search.as_mut() {
            Some(search) => search,
            None => match RegexSearch::new(URL_PATTERN) {
                Ok(built) => search.insert(built),
                Err(error) => {
                    log::error!("invalid URL pattern: {error}");
                    return None;
                }
            },
        };
        let mut from = start;
        loop {
            let found = term.regex_search_right(search, from, end)?;
            if *found.start() > point {
                return None;
            }
            if *found.end() >= point {
                return trimmed(term, found).filter(|link| link.range.contains(&point));
            }
            if *found.end() >= end {
                return None;
            }
            from = next(term, *found.end())?;
        }
    })
}

/// The detected URL over `range`, without its trailing punctuation and unbalanced
/// closing brackets. `None` when nothing is left after the scheme.
fn trimmed<L>(term: &Term<L>, range: RangeInclusive<Point>) -> Option<Link> {
    // The characters of the range with their cell, wide character spacers skipped.
    let mut chars = Vec::new();
    let mut point = *range.start();
    loop {
        let cell = &term.grid()[point];
        if !cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            chars.push((point, cell.c));
        }
        if point >= *range.end() {
            break;
        }
        point = next(term, point)?;
    }
    while let Some(&(_, last)) = chars.last() {
        let unbalanced = |open: char, close: char| {
            last == close
                && chars.iter().filter(|(_, c)| *c == close).count()
                    > chars.iter().filter(|(_, c)| *c == open).count()
        };
        if TRAILING_PUNCTUATION.contains(&last) || unbalanced('(', ')') || unbalanced('[', ']') {
            chars.pop();
        } else {
            break;
        }
    }
    let uri: String = chars.iter().map(|(_, c)| c).collect();
    let scheme = uri.find(':')? + 1;
    let after = uri[scheme..].trim_start_matches('/');
    if after.is_empty() {
        return None;
    }
    let first = chars.first()?.0;
    let last = chars.last()?.0;
    // A wide character ends on its spacer.
    let last = match next(term, last) {
        Some(spacer) if term.grid()[spacer].flags.contains(Flags::WIDE_CHAR_SPACER) => spacer,
        _ => last,
    };
    Some(Link {
        range: first..=last,
        uri,
        explicit: false,
    })
}

/// Whether a link under the pointer is Graphene's (hovered, underlined, clickable):
/// always, except where the application captures the mouse (`captured`), where `Ctrl`
/// must be held.
pub fn active(captured: bool, mods: ModifiersState) -> bool {
    !captured || mods.control_key()
}

/// A left press recorded for a possible link click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Press<C> {
    /// Cell pressed, in whatever coordinates the caller compares.
    pub cell: C,
    /// Rank of the press in a click series: 1, 2 or 3.
    pub clicks: u8,
    pub mods: ModifiersState,
    /// The application captured the mouse at the pressed cell.
    pub captured: bool,
}

/// Whether the release at `release` of the left press `press` on a link opens it.
/// `same_link` tells whether the link under the release is still the pressed one.
///
/// A simple click without movement opens; a drag, a double or triple-click, and `Shift`
/// keep selecting. Where the application captures the mouse, only a `Ctrl` + click
/// (consumed by Graphene, never reported) opens.
pub fn click_opens<C: PartialEq>(press: &Press<C>, release: &C, same_link: bool) -> bool {
    if press.cell != *release || !same_link {
        return false;
    }
    if press.captured {
        return press.mods.control_key();
    }
    press.clicks == 1 && !press.mods.shift_key()
}

/// Whether `link` may be opened, `hostname` being the local host name: detected URLs
/// only with the four detected schemes; `file` URIs only on this machine (empty host,
/// `localhost` or the local host name).
pub fn may_open(link: &Link, hostname: Option<&str>) -> bool {
    let lower = link.uri.to_ascii_lowercase();
    if !link.explicit
        && !["http://", "https://", "file:", "mailto:"]
            .iter()
            .any(|scheme| lower.starts_with(scheme))
    {
        return false;
    }
    let Some(rest) = lower.strip_prefix("file://") else {
        return true;
    };
    let host = &rest[..rest.find('/').unwrap_or(rest.len())];
    host.is_empty()
        || host == "localhost"
        || hostname.is_some_and(|name| name.eq_ignore_ascii_case(host))
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::index::Line;

    use super::*;
    use crate::timeline::testing::Session;

    fn term(columns: usize, bytes: &str) -> Session {
        let mut session = Session::sized(columns, 4, 100);
        session.feed(bytes);
        session
    }

    fn p(line: i32, column: usize) -> Point {
        Point::new(Line(line), Column(column))
    }

    fn osc8(id: &str, uri: &str, text: &str) -> String {
        let params = if id.is_empty() {
            String::new()
        } else {
            format!("id={id}")
        };
        format!("\x1b]8;{params};{uri}\x1b\\{text}\x1b]8;;\x1b\\")
    }

    #[test]
    fn osc8_range_covers_the_adjacent_cells_of_the_link() {
        let s = term(20, &format!("ab {} cd", osc8("", "https://x.y/z", "link")));
        let link = link_at(&s.term, p(0, 4)).expect("link");
        assert_eq!(
            link,
            Link {
                range: p(0, 3)..=p(0, 6),
                uri: "https://x.y/z".into(),
                explicit: true,
            }
        );
        assert_eq!(link_at(&s.term, p(0, 2)), None);
        assert_eq!(link_at(&s.term, p(0, 7)), None);
    }

    #[test]
    fn osc8_with_any_scheme_is_kept_as_given() {
        let s = term(20, &osc8("", "ssh://host", "x"));
        assert_eq!(link_at(&s.term, p(0, 0)).expect("link").uri, "ssh://host");
    }

    #[test]
    fn osc8_range_follows_wrapped_lines() {
        let s = term(5, &format!("abc{}", osc8("a", "file:///tmp", "0123456")));
        let link = link_at(&s.term, p(1, 2)).expect("link");
        assert_eq!(link.range, p(0, 3)..=p(1, 4));
        assert!(link.explicit);
    }

    #[test]
    fn two_osc8_links_side_by_side_stay_apart() {
        let s = term(
            20,
            &format!(
                "{}{}",
                osc8("1", "https://a", "aa"),
                osc8("2", "https://b", "bb")
            ),
        );
        assert_eq!(
            link_at(&s.term, p(0, 1)).expect("a").range,
            p(0, 0)..=p(0, 1)
        );
        assert_eq!(
            link_at(&s.term, p(0, 2)).expect("b").range,
            p(0, 2)..=p(0, 3)
        );
    }

    #[test]
    fn osc8_on_a_wide_character_includes_its_spacer() {
        let s = term(20, &osc8("", "https://w", "日本"));
        let link = link_at(&s.term, p(0, 1)).expect("link");
        assert_eq!(link.range, p(0, 0)..=p(0, 3));
    }

    #[test]
    fn each_scheme_is_detected() {
        for url in [
            "http://example.com/a",
            "https://example.com/a?b=1&c=2#d",
            "file:///home/user/f.txt",
            "mailto:someone@example.com",
        ] {
            let s = term(60, &format!("see {url} now"));
            let link = link_at(&s.term, p(0, 6)).unwrap_or_else(|| panic!("{url}"));
            assert_eq!(link.uri, url);
            assert_eq!(link.range, p(0, 4)..=p(0, 3 + url.len()));
            assert!(!link.explicit);
        }
    }

    #[test]
    fn other_schemes_are_not_detected() {
        let s = term(40, "ftp://example.com ssh://host");
        assert_eq!(link_at(&s.term, p(0, 2)), None);
        assert_eq!(link_at(&s.term, p(0, 20)), None);
    }

    #[test]
    fn trailing_punctuation_is_trimmed() {
        for (text, url) in [
            ("go to https://a.io/x.", "https://a.io/x"),
            ("(https://a.io/x)", "https://a.io/x"),
            ("'https://a.io/x',", "https://a.io/x"),
            ("https://a.io/x?", "https://a.io/x"),
            (
                "https://en.wikipedia.org/wiki/Rust_(language)",
                "https://en.wikipedia.org/wiki/Rust_(language)",
            ),
            ("[https://a.io/[1]]", "https://a.io/[1]"),
        ] {
            let s = term(80, text);
            let column = text.find("https").expect("url") + 2;
            assert_eq!(
                link_at(&s.term, p(0, column)).map(|link| link.uri),
                Some(url.to_owned()),
                "{text}"
            );
        }
    }

    #[test]
    fn a_trimmed_character_is_not_part_of_the_link() {
        let s = term(40, "https://a.io/x.");
        assert!(link_at(&s.term, p(0, 13)).is_some());
        assert_eq!(link_at(&s.term, p(0, 14)), None);
    }

    #[test]
    fn a_bare_scheme_is_no_link() {
        let s = term(40, "https://. mailto:");
        assert_eq!(link_at(&s.term, p(0, 2)), None);
        assert_eq!(link_at(&s.term, p(0, 11)), None);
    }

    #[test]
    fn detected_url_follows_wrapped_lines() {
        let s = term(10, "x https://a.io/long/path y");
        let link = link_at(&s.term, p(1, 3)).expect("link");
        assert_eq!(link.uri, "https://a.io/long/path");
        assert_eq!(link.range, p(0, 2)..=p(2, 3));
    }

    #[test]
    fn the_url_under_the_point_is_chosen_among_several() {
        let s = term(60, "https://a.io and https://b.io");
        assert_eq!(link_at(&s.term, p(0, 20)).expect("b").uri, "https://b.io");
        assert_eq!(link_at(&s.term, p(0, 2)).expect("a").uri, "https://a.io");
        assert_eq!(link_at(&s.term, p(0, 14)), None);
    }

    #[test]
    fn plain_text_has_no_link() {
        let s = term(20, "hello world");
        for column in 0..20 {
            assert_eq!(link_at(&s.term, p(0, column)), None);
        }
    }

    #[test]
    fn a_point_outside_the_grid_has_no_link() {
        let s = term(20, "https://a.io");
        assert_eq!(link_at(&s.term, p(-1, 0)), None);
        assert_eq!(link_at(&s.term, p(4, 0)), None);
        assert_eq!(link_at(&s.term, p(0, 20)), None);
    }

    #[test]
    fn links_in_the_history_are_found() {
        let mut s = term(20, "https://a.io\r\n");
        s.feed("1\r\n2\r\n3\r\n4");
        let link = link_at(&s.term, p(-1, 2)).expect("link");
        assert_eq!(link.range, p(-1, 0)..=p(-1, 11));
    }

    const NONE: ModifiersState = ModifiersState::empty();

    fn press(clicks: u8, mods: ModifiersState, captured: bool) -> Press<u8> {
        Press {
            cell: 1,
            clicks,
            mods,
            captured,
        }
    }

    #[test]
    fn click_decision_table() {
        let ctrl = ModifiersState::CONTROL;
        let shift = ModifiersState::SHIFT;
        // (press, release cell, same link, opens)
        let table = [
            (press(1, NONE, false), 1, true, true),
            (press(1, ctrl, false), 1, true, true),
            // Drag.
            (press(1, NONE, false), 2, true, false),
            // Double and triple-click select.
            (press(2, NONE, false), 1, true, false),
            (press(3, NONE, false), 1, true, false),
            // Shift extends the selection.
            (press(1, shift, false), 1, true, false),
            // The link changed under the pointer.
            (press(1, NONE, false), 1, false, false),
            // Mouse captured by the application: Ctrl + click only.
            (press(1, ctrl, true), 1, true, true),
            (press(1, NONE, true), 1, true, false),
            (press(1, ctrl, true), 2, true, false),
            (press(1, ctrl, true), 1, false, false),
        ];
        for (press, release, same, opens) in table {
            assert_eq!(
                click_opens(&press, &release, same),
                opens,
                "{press:?} -> {release}"
            );
        }
    }

    #[test]
    fn a_link_is_active_unless_captured_without_ctrl() {
        assert!(active(false, NONE));
        assert!(active(false, ModifiersState::CONTROL));
        assert!(!active(true, NONE));
        assert!(!active(true, ModifiersState::SHIFT));
        assert!(active(true, ModifiersState::CONTROL));
    }

    fn link(uri: &str, explicit: bool) -> Link {
        Link {
            range: p(0, 0)..=p(0, 0),
            uri: uri.to_owned(),
            explicit,
        }
    }

    #[test]
    fn file_links_open_on_this_machine_only() {
        for uri in [
            "file:///etc",
            "file://localhost/etc",
            "file://Box/etc",
            "file:/etc",
        ] {
            assert!(may_open(&link(uri, true), Some("box")), "{uri}");
        }
        assert!(!may_open(&link("file://other/etc", true), Some("box")));
        assert!(!may_open(&link("file://box/etc", true), None));
        assert!(!may_open(&link("file://other/etc", false), Some("box")));
    }

    #[test]
    fn explicit_links_open_with_any_scheme_detected_ones_with_four() {
        assert!(may_open(&link("ssh://host", true), None));
        assert!(!may_open(&link("ssh://host", false), None));
        for uri in ["http://a", "HTTPS://a", "mailto:a@b", "file:///a"] {
            assert!(may_open(&link(uri, false), None), "{uri}");
        }
    }
}
