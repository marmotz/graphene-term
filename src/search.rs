//! Text search in the history of a pane: the query, its regex, the matches found in
//! the grids of the timeline, and the current match.
//!
//! Pure: it reads the timeline and the live grid, never changes them. The grids are
//! searched top to bottom: each finished block's header then output, the current
//! block's header, then the live grid unless it holds the command line being edited.
//! Matches are kept oldest first; they are numbered from the bottom, 1 being the most
//! recent.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::ops::RangeInclusive;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Line, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::search::{RegexIter, RegexSearch};

use crate::timeline::{BlockId, Phase, Status, Timeline, content_rows};
use crate::view::RowSource;

/// Most matches collected: past it the collection stops, keeping the most recent ones.
pub const CAP: usize = 10_000;

/// Which part of the blocks is searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Zone {
    /// Headers, outputs and the live grid.
    #[default]
    Both,
    /// The command text of the headers, without the prompt.
    Command,
    /// The outputs, and the live grid while a command runs.
    Output,
}

/// Which blocks are searched, by how their command ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusFilter {
    /// Every block, the current one included.
    #[default]
    All,
    /// Only the finished blocks whose command exited with 0.
    Success,
    /// Only the finished blocks whose command exited with a non-zero code.
    Failure,
}

impl StatusFilter {
    fn accepts(self, status: Status) -> bool {
        match self {
            StatusFilter::All => true,
            StatusFilter::Success => status == Status::Success,
            StatusFilter::Failure => matches!(status, Status::Failure(_)),
        }
    }
}

/// What to search for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    pub text: String,
    /// Upper and lower case differ. Off: the case is ignored, whatever the text holds.
    pub case_sensitive: bool,
    /// The text is a regular expression. Off: it is matched literally.
    pub regex: bool,
    /// Only matches standing between non-word characters (or line ends).
    pub whole_word: bool,
    pub zone: Zone,
    pub status: StatusFilter,
}

/// The query text is not a valid regular expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidRegex;

/// The regex searching for `query`: `None` for an empty text, which finds nothing.
pub fn regex(query: &Query) -> Result<Option<RegexSearch>, InvalidRegex> {
    if query.text.is_empty() {
        return Ok(None);
    }
    let pattern = if query.regex {
        query.text.clone()
    } else {
        escape(&query.text)
    };
    // An inline flag wins over the "smart case" of `RegexSearch::new`, which ignores
    // the case unless the pattern holds an upper case letter.
    let case = if query.case_sensitive {
        "(?-i)"
    } else {
        "(?i)"
    };
    RegexSearch::new(&format!("{case}{pattern}"))
        .map(Some)
        .map_err(|_| InvalidRegex)
}

/// `text` with the regex metacharacters escaped, so that it matches literally.
pub fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if r"\.+*?()|[]{}^$#&-~".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// Which frozen grid of a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Part {
    Header,
    Output,
}

/// A match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// The block the match is in; the current block for the live grid.
    pub block: BlockId,
    /// [`RowSource::Header`], [`RowSource::Output`] or [`RowSource::Live`].
    pub source: RowSource,
    /// First and last cells, lines counted from the top of the grid's history, so that
    /// a match in the live grid stays on its text while the screen scrolls. Frozen
    /// grids have no history: their lines are the grid lines. See
    /// [`Match::grid_range`].
    pub range: RangeInclusive<Point>,
}

impl Match {
    /// [`Match::range`] in the lines of `term`, the grid it was found in: history lines
    /// are negative.
    pub fn grid_range<L>(&self, term: &Term<L>) -> RangeInclusive<Point> {
        let history = term.grid().history_size() as i32;
        let shift = |point: Point| Point::new(Line(point.line.0 - history), point.column);
        shift(*self.range.start())..=shift(*self.range.end())
    }

    /// Order of the matches in the timeline, top to bottom.
    fn key(&self) -> (BlockId, u8, Point) {
        let part = match self.source {
            RowSource::Header(_) | RowSource::Summary(_) => 0,
            RowSource::Output(_) => 1,
            RowSource::Live => 2,
        };
        (self.block, part, *self.range.start())
    }
}

/// The matches of a search.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Matches {
    /// Oldest first.
    pub list: Vec<Match>,
    /// The collection stopped at [`CAP`]: older matches were left out.
    pub capped: bool,
}

/// The matches found in a frozen grid, for the columns it had.
struct Entry {
    columns: usize,
    matches: Vec<Match>,
}

/// Searches a timeline for a query. A finished block never changes but at a reflow,
/// so the matches of each frozen grid are kept until the query or its columns change:
/// only the live grid is searched again when output arrives.
pub struct Searcher {
    query: Query,
    regex: Option<RegexSearch>,
    /// The query is not a valid regex.
    invalid: bool,
    cache: HashMap<(BlockId, Part), Entry>,
}

impl Default for Searcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Searcher {
    /// A searcher with an empty query, which finds nothing.
    pub fn new() -> Self {
        Searcher {
            query: Query::default(),
            regex: None,
            invalid: false,
            cache: HashMap::new(),
        }
    }

    pub fn query(&self) -> &Query {
        &self.query
    }

    /// Searches for `query` from now on. When it is not a valid regex, the searcher
    /// keeps it but finds nothing.
    pub fn set_query(&mut self, query: Query) -> Result<(), InvalidRegex> {
        if query != self.query {
            self.cache.clear();
            let regex = regex(&query);
            self.query = query;
            self.invalid = regex.is_err();
            self.regex = regex.ok().flatten();
        }
        if self.invalid {
            Err(InvalidRegex)
        } else {
            Ok(())
        }
    }

    /// The matches of the query in `timeline` and `live`, the session's terminal.
    /// Nothing on the alternate screen, whose screen is not part of the history.
    pub fn search<L>(&mut self, timeline: &Timeline, live: &Term<L>) -> Matches {
        self.cache.retain(|(id, _), _| known_block(timeline, *id));
        if self.regex.is_none() || alternate(live) {
            return Matches::default();
        }
        let query = self.query.clone();
        let current = timeline.current();
        let status_on = query.status != StatusFilter::All;
        // Grids bottom to top, so that the cap keeps the most recent matches.
        let mut collector = Collector::default();

        let live_searched = !(current.phase == Phase::Prompt && current.editing)
            && !status_on
            && match query.zone {
                Zone::Both => true,
                Zone::Command => false,
                Zone::Output => current.phase == Phase::Running,
            };
        if live_searched {
            let regex = self.regex.as_mut().expect("regex checked above");
            let matches = find(live, regex, query.whole_word, None)
                .into_iter()
                .map(|range| live_match(live, current.id, range))
                .collect();
            if collector.push(matches) {
                return collector.finish();
            }
        }

        if let Some(header) = current.header.as_ref()
            && !status_on
            && query.zone != Zone::Output
        {
            let matches = self.cached(current.id, Part::Header, header, &current.command);
            if collector.push(matches) {
                return collector.finish();
            }
        }

        for block in timeline.blocks().iter().rev() {
            if !query.status.accepts(block.status) {
                continue;
            }
            if query.zone != Zone::Command {
                let matches = self.cached(block.id, Part::Output, &block.output, "");
                if collector.push(matches) {
                    return collector.finish();
                }
            }
            if query.zone != Zone::Output {
                let matches = self.cached(block.id, Part::Header, &block.header, &block.command);
                if collector.push(matches) {
                    return collector.finish();
                }
            }
        }
        collector.finish()
    }

    /// The matches of the frozen grid `term`, part `part` of block `id`, from the cache
    /// when it was searched with the same columns. `command` is the block's command,
    /// to find the command part of a header.
    fn cached<L>(&mut self, id: BlockId, part: Part, term: &Term<L>, command: &str) -> Vec<Match> {
        let columns = term.columns();
        if let Some(entry) = self.cache.get(&(id, part))
            && entry.columns == columns
        {
            return entry.matches.clone();
        }
        let Some(regex) = self.regex.as_mut() else {
            return Vec::new();
        };
        let from = match (part, self.query.zone) {
            (Part::Header, Zone::Command) => Some(command_start(term, command)),
            _ => None,
        };
        let source = match part {
            Part::Header => RowSource::Header(id),
            Part::Output => RowSource::Output(id),
        };
        let matches: Vec<Match> = find(term, regex, self.query.whole_word, from)
            .into_iter()
            .map(|range| Match {
                block: id,
                source,
                range,
            })
            .collect();
        self.cache.insert(
            (id, part),
            Entry {
                columns,
                matches: matches.clone(),
            },
        );
        matches
    }
}

/// Whether the search filters by zone and status make sense: the shell integration
/// split the history into blocks.
pub fn filters_available(timeline: &Timeline) -> bool {
    !timeline.blocks().is_empty() || timeline.current().header.is_some()
}

/// Index of the match after `index` going to the oldest, wrapping to the most recent.
pub fn older(index: usize, len: usize) -> usize {
    if index == 0 {
        len.saturating_sub(1)
    } else {
        index - 1
    }
}

/// Index of the match after `index` going to the most recent, wrapping to the oldest.
pub fn newer(index: usize, len: usize) -> usize {
    if index + 1 >= len { 0 } else { index + 1 }
}

/// Number shown for the match at `index` among `len`: 1 for the most recent.
pub fn number(index: usize, len: usize) -> usize {
    len - index
}

/// The match current once the query is typed: the most recent one starting above the
/// bottom of the view (`above_bottom`), the most recent one when none does.
pub fn initial(matches: &[Match], above_bottom: impl Fn(&Match) -> bool) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    matches
        .iter()
        .rposition(above_bottom)
        .or(Some(matches.len() - 1))
}

/// The index in `matches`, a new search, of `old`, the current match of the previous
/// one: the same source and start, or the next one going to the oldest when it is
/// gone (the most recent when none is older).
pub fn retain(old: &Match, matches: &[Match]) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    let key = old.key();
    let mut older = None;
    for (index, candidate) in matches.iter().enumerate() {
        match candidate.key().cmp(&key) {
            Ordering::Equal => return Some(index),
            Ordering::Less => older = Some(index),
            Ordering::Greater => break,
        }
    }
    older.or(Some(matches.len() - 1))
}

fn known_block(timeline: &Timeline, id: BlockId) -> bool {
    timeline.current().id == id || timeline.block(id).is_some()
}

fn alternate<L>(term: &Term<L>) -> bool {
    term.mode()
        .contains(alacritty_terminal::term::TermMode::ALT_SCREEN)
}

/// Matches gathered grid by grid, bottom to top, up to [`CAP`].
#[derive(Default)]
struct Collector {
    groups: Vec<Vec<Match>>,
    total: usize,
    capped: bool,
}

impl Collector {
    /// Adds the matches of the grid above the last one, the most recent ones when they
    /// overflow the cap. Returns whether they did: the collection stops.
    fn push(&mut self, mut matches: Vec<Match>) -> bool {
        let room = CAP - self.total;
        if matches.len() > room {
            self.capped = true;
            matches.drain(..matches.len() - room);
        }
        self.total += matches.len();
        self.groups.push(matches);
        self.capped
    }

    /// Every match, oldest first.
    fn finish(self) -> Matches {
        Matches {
            list: self.groups.into_iter().rev().flatten().collect(),
            capped: self.capped,
        }
    }
}

fn live_match<L>(term: &Term<L>, block: BlockId, range: RangeInclusive<Point>) -> Match {
    let history = term.grid().history_size() as i32;
    let shift = |point: Point| Point::new(Line(point.line.0 + history), point.column);
    Match {
        block,
        source: RowSource::Live,
        range: shift(*range.start())..=shift(*range.end()),
    }
}

/// The matches of `regex` in the content rows of `term`, top to bottom, in grid lines.
/// `whole_word` keeps those standing between non-word characters; `from` keeps those
/// starting at it or after.
fn find<L>(
    term: &Term<L>,
    regex: &mut RegexSearch,
    whole_word: bool,
    from: Option<Point>,
) -> Vec<RangeInclusive<Point>> {
    let grid = term.grid();
    let rows = content_rows(grid);
    if rows == 0 {
        return Vec::new();
    }
    let history = grid.history_size() as i32;
    let top = Point::new(Line(-history), Column(0));
    let start = from.map_or(top, |from| from.max(top));
    let end = Point::new(Line(rows as i32 - history - 1), Column(grid.columns() - 1));
    if start > end {
        return Vec::new();
    }
    RegexIter::new(start, end, Direction::Right, term, regex)
        .filter(|range| !whole_word || is_whole_word(term, range))
        .collect()
}

/// Whether the characters just before and after `range` (across wrapped lines) are not
/// word characters.
fn is_whole_word<L>(term: &Term<L>, range: &RangeInclusive<Point>) -> bool {
    let before = char_before(term, *range.start());
    let after = char_after(term, *range.end());
    !before.is_some_and(is_word) && !after.is_some_and(is_word)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The character before `point` on its line, or at the end of the line above when it
/// wraps into this one.
fn char_before<L>(term: &Term<L>, point: Point) -> Option<char> {
    let grid = term.grid();
    let last = Column(grid.columns() - 1);
    let mut point = if point.column > Column(0) {
        Point::new(point.line, point.column - 1)
    } else {
        let above = Line(point.line.0 - 1);
        if above < grid.topmost_line() || !grid[above][last].flags.contains(Flags::WRAPLINE) {
            return None;
        }
        Point::new(above, last)
    };
    let flags = grid[point].flags;
    if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
        if point.column == Column(0) {
            return None;
        }
        point.column -= 1;
    }
    Some(grid[point].c)
}

/// The character after `point` on its line, or at the start of the line below when
/// this one wraps into it.
fn char_after<L>(term: &Term<L>, point: Point) -> Option<char> {
    let grid = term.grid();
    let last = Column(grid.columns() - 1);
    let mut point = point;
    if grid[point].flags.contains(Flags::WIDE_CHAR) && point.column < last {
        point.column += 1;
    }
    if point.column < last {
        let next = Point::new(point.line, point.column + 1);
        if grid[next].flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
            return wrapped_next(term, next);
        }
        return Some(grid[next].c);
    }
    wrapped_next(term, point)
}

/// The first character of the line below `point`, on the last column, when its line
/// wraps.
fn wrapped_next<L>(term: &Term<L>, point: Point) -> Option<char> {
    let grid = term.grid();
    let last = Column(grid.columns() - 1);
    let below = Line(point.line.0 + 1);
    if below > grid.bottommost_line() || !grid[point.line][last].flags.contains(Flags::WRAPLINE) {
        return None;
    }
    Some(grid[below][Column(0)].c)
}

/// Start of the command part of a header: where the first line of `command` last
/// appears in it. The top of the header when the command is empty or not found.
fn command_start<L>(term: &Term<L>, command: &str) -> Point {
    let grid = term.grid();
    let top = Point::new(grid.topmost_line(), Column(0));
    let first = command.lines().next().unwrap_or("").trim_end();
    if first.trim().is_empty() {
        return top;
    }
    let Ok(mut regex) = RegexSearch::new(&format!("(?-i){}", escape(first))) else {
        return top;
    };
    let rows = content_rows(grid);
    if rows == 0 {
        return top;
    }
    let history = grid.history_size() as i32;
    let end = Point::new(Line(rows as i32 - history - 1), Column(grid.columns() - 1));
    term.regex_search_left(&mut regex, end, top)
        .map_or(top, |range| *range.start())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::testing::*;

    fn query(text: &str) -> Query {
        Query {
            text: text.to_owned(),
            ..Query::default()
        }
    }

    fn search(session: &Session, query: Query) -> Matches {
        let mut searcher = Searcher::new();
        searcher.set_query(query).expect("valid query");
        searcher.search(&session.timeline, &session.term)
    }

    /// Source and start (line, column) of each match.
    fn starts(matches: &Matches) -> Vec<(RowSource, i32, usize)> {
        matches
            .list
            .iter()
            .map(|m| (m.source, m.range.start().line.0, m.range.start().column.0))
            .collect()
    }

    /// Finished blocks `ls` (ok), `false` (fails), then a prompt being edited.
    fn blocks() -> Session {
        let mut session = Session::new(100);
        session.command("ls", &["foo", "bar"], 0);
        session.command("false", &["foo failed"], 1);
        session.feed(&format!("{}$ foo", prompt()));
        session
    }

    #[test]
    fn escape_makes_the_metacharacters_literal() {
        assert_eq!(escape("a.b*(c)"), r"a\.b\*\(c\)");
        let mut session = Session::new(100);
        session.feed("a.b axb\r\n");
        let matches = search(&session, query("a.b"));
        assert_eq!(starts(&matches), [(RowSource::Live, 0, 0)]);

        let regex = Query {
            regex: true,
            ..query("a.b")
        };
        assert_eq!(search(&session, regex).list.len(), 2);
    }

    #[test]
    fn case_follows_the_toggle_not_the_text() {
        let mut session = Session::new(100);
        session.feed("Foo foo\r\n");
        // Insensitive even with an upper case letter in the text.
        assert_eq!(search(&session, query("Foo")).list.len(), 2);
        let sensitive = |text: &str| Query {
            case_sensitive: true,
            ..query(text)
        };
        assert_eq!(
            starts(&search(&session, sensitive("foo"))),
            [(RowSource::Live, 0, 4)]
        );
        assert_eq!(
            starts(&search(&session, sensitive("Foo"))),
            [(RowSource::Live, 0, 0)]
        );
    }

    #[test]
    fn invalid_regex_is_an_error_and_finds_nothing() {
        let mut session = Session::new(100);
        session.feed("a(b\r\n");
        let mut searcher = Searcher::new();
        let invalid = Query {
            regex: true,
            ..query("a(b")
        };
        assert_eq!(searcher.set_query(invalid.clone()), Err(InvalidRegex));
        assert_eq!(searcher.set_query(invalid), Err(InvalidRegex));
        assert!(
            searcher
                .search(&session.timeline, &session.term)
                .list
                .is_empty()
        );
        // The same text, literal, is valid.
        assert_eq!(searcher.set_query(query("a(b")), Ok(()));
        assert_eq!(
            searcher.search(&session.timeline, &session.term).list.len(),
            1
        );
    }

    #[test]
    fn empty_text_finds_nothing() {
        let mut session = Session::new(100);
        session.feed("abc\r\n");
        assert_eq!(search(&session, query("")), Matches::default());
    }

    #[test]
    fn whole_word_handles_accents_and_wrapped_lines() {
        let mut session = Session::new(100);
        // "déjà" is a word; "jà" inside it is not. The 20 columns wrap "xxxx...cat"
        // so that "cat" starts a wrapped line glued to the x before it.
        session.feed("déjà jà\r\n");
        session.feed(&format!("{}cat dog_cat cat\r\n", "x".repeat(20)));
        let whole = |text: &str| Query {
            whole_word: true,
            ..query(text)
        };
        assert_eq!(
            starts(&search(&session, whole("jà"))),
            [(RowSource::Live, 0, 5)]
        );
        assert_eq!(
            starts(&search(&session, whole("déjà"))),
            [(RowSource::Live, 0, 0)]
        );
        // The first "cat" follows the x of the wrapped line, the second an underscore.
        assert_eq!(
            starts(&search(&session, whole("cat"))),
            [(RowSource::Live, 2, 12)]
        );
        assert_eq!(search(&session, query("cat")).list.len(), 3);
    }

    #[test]
    fn grids_are_searched_top_to_bottom_without_the_edited_prompt() {
        let session = blocks();
        let matches = search(&session, query("foo"));
        assert_eq!(
            starts(&matches),
            [
                (RowSource::Output(BlockId(0)), 0, 0),
                (RowSource::Output(BlockId(1)), 0, 0),
            ]
        );
        assert_eq!(matches.list[1].block, BlockId(1));
    }

    #[test]
    fn without_shell_integration_the_live_grid_holds_everything() {
        let mut session = Session::new(100);
        for line in 0..8 {
            session.feed(&format!("$ echo {line}\r\nout {line}\r\n"));
        }
        assert!(!filters_available(&session.timeline));
        let matches = search(&session, query("out"));
        assert_eq!(matches.list.len(), 8);
        // Lines from the top of the history: the first output is on line 1.
        assert_eq!(
            *matches.list[0].range.start(),
            Point::new(Line(1), Column(0))
        );
        let history = session.term.grid().history_size() as i32;
        assert_eq!(
            *matches.list[0].grid_range(&session.term).start(),
            Point::new(Line(1 - history), Column(0))
        );
    }

    #[test]
    fn running_output_is_searched() {
        let mut session = Session::new(100);
        session.command("ls", &["foo"], 0);
        session.feed(&format!("$ make\r\n{}foo building", start("make")));
        assert!(filters_available(&session.timeline));
        let matches = search(&session, query("foo"));
        assert_eq!(
            starts(&matches),
            [
                (RowSource::Output(BlockId(0)), 0, 0),
                (RowSource::Live, 0, 0),
            ]
        );
        assert_eq!(matches.list[1].block, BlockId(1));
        let output = Query {
            zone: Zone::Output,
            ..query("foo")
        };
        assert_eq!(search(&session, output).list.len(), 2);
    }

    #[test]
    fn zone_filters_headers_and_outputs() {
        let mut session = Session::new(100);
        session.command("cat ls", &["ls"], 0);
        let zone = |zone| Query {
            zone,
            ..query("ls")
        };
        assert_eq!(
            starts(&search(&session, zone(Zone::Command))),
            [(RowSource::Header(BlockId(0)), 0, 6)]
        );
        assert_eq!(
            starts(&search(&session, zone(Zone::Output))),
            [(RowSource::Output(BlockId(0)), 0, 0)]
        );
        assert_eq!(search(&session, zone(Zone::Both)).list.len(), 2);
    }

    #[test]
    fn command_part_leaves_the_prompt_out() {
        let mut session = Session::new(100);
        // The prompt holds "ls" too.
        session.command("ls", &[], 0);
        session.feed(&format!("ls@host$ ls\r\n{}{}", start("ls"), end(0)));
        let command = Query {
            zone: Zone::Command,
            ..query("ls")
        };
        assert_eq!(
            starts(&search(&session, command)),
            [
                (RowSource::Header(BlockId(0)), 0, 2),
                (RowSource::Header(BlockId(1)), 0, 9),
            ]
        );
    }

    #[test]
    fn command_part_falls_back_to_the_whole_header() {
        let mut session = Session::new(100);
        // No command sent by the hook.
        session.command("", &[], 0);
        // A command not found in the header.
        session.feed(&format!("$ gs\r\n{}{}", start("git status"), end(0)));
        let command = |text: &str| Query {
            zone: Zone::Command,
            ..query(text)
        };
        assert_eq!(
            starts(&search(&session, command("$"))),
            [
                (RowSource::Header(BlockId(0)), 0, 0),
                (RowSource::Header(BlockId(1)), 0, 0),
            ]
        );
    }

    #[test]
    fn command_part_covers_a_multiline_command() {
        let mut session = Session::new(100);
        let command = "echo a \\\nb";
        session.feed(&format!(
            "$ echo a \\\r\n> b\r\n{}{}",
            start(command),
            end(0)
        ));
        let query = |text: &str| Query {
            zone: Zone::Command,
            ..query(text)
        };
        // The second line, after its own prompt, is part of the command.
        assert_eq!(
            starts(&search(&session, query("b"))),
            [(RowSource::Header(BlockId(0)), 1, 2)]
        );
        assert!(search(&session, query("$")).list.is_empty());
        assert_eq!(search(&session, query(">")).list.len(), 1);
    }

    #[test]
    fn status_filter_keeps_finished_blocks_by_status() {
        let mut session = blocks();
        session.feed(&format!(" bar\r\n{}foo", start("foo bar")));
        let status = |status| Query {
            status,
            ..query("foo")
        };
        assert_eq!(
            starts(&search(&session, status(StatusFilter::Success))),
            [(RowSource::Output(BlockId(0)), 0, 0)]
        );
        assert_eq!(
            starts(&search(&session, status(StatusFilter::Failure))),
            [(RowSource::Output(BlockId(1)), 0, 0)]
        );
        // The running block is in with every status.
        let all = search(&session, status(StatusFilter::All));
        assert_eq!(all.list.len(), 4);
        assert_eq!(all.list[2].source, RowSource::Header(BlockId(2)));
        assert_eq!(all.list[3].source, RowSource::Live);
    }

    #[test]
    fn cancelled_and_empty_blocks_are_left_out_by_a_status_filter() {
        let mut session = Session::new(100);
        session.feed(&format!("{}$ foo\r\n{}{}$ ", prompt(), cancel(), prompt()));
        assert_eq!(session.timeline.blocks()[0].status, Status::Cancelled);
        assert_eq!(search(&session, query("foo")).list.len(), 1);
        let success = Query {
            status: StatusFilter::Success,
            ..query("foo")
        };
        assert!(search(&session, success).list.is_empty());
    }

    #[test]
    fn collection_stops_at_the_cap_keeping_the_most_recent() {
        let mut session = Session::sized(200, 5, CAP);
        session.command("old", &["x"], 0);
        let line = "x ".repeat(100);
        session.feed(&format!("$ new\r\n{}", start("new")));
        for _ in 0..CAP / 100 + 1 {
            session.feed(&format!("{line}\r\n"));
        }
        let matches = search(&session, query("x"));
        assert!(matches.capped);
        assert_eq!(matches.list.len(), CAP);
        assert!(matches.list.iter().all(|m| m.source == RowSource::Live));
        // The oldest live line (line 0) is the one left out.
        assert_eq!(matches.list[0].range.start().line, Line(1));
    }

    #[test]
    fn under_the_cap_nothing_is_left_out() {
        let session = blocks();
        assert!(!search(&session, query("foo")).capped);
    }

    #[test]
    fn navigation_numbers_from_the_bottom_and_wraps() {
        assert_eq!(number(2, 3), 1);
        assert_eq!(number(0, 3), 3);
        assert_eq!(older(2, 3), 1);
        assert_eq!(older(0, 3), 2);
        assert_eq!(newer(1, 3), 2);
        assert_eq!(newer(2, 3), 0);
        assert_eq!(older(0, 1), 0);
        assert_eq!(newer(0, 1), 0);
    }

    #[test]
    fn initial_match_is_the_most_recent_above_the_view_bottom() {
        let session = blocks();
        let matches = search(&session, query("foo")).list;
        let first_block = |m: &Match| m.block == BlockId(0);
        assert_eq!(initial(&matches, first_block), Some(0));
        assert_eq!(initial(&matches, |_| false), Some(1));
        assert_eq!(initial(&[], |_| true), None);
    }

    #[test]
    fn current_match_is_kept_when_output_arrives() {
        let mut session = Session::new(100);
        session.feed(&format!("$ tail\r\n{}foo 1\r\nfoo 2\r\n", start("tail")));
        let mut searcher = Searcher::new();
        searcher.set_query(query("foo")).unwrap();
        let before = searcher.search(&session.timeline, &session.term).list;
        let current = before[0].clone();

        // Enough output to scroll the first match into the history.
        session.feed("foo 3\r\nfoo 4\r\nfoo 5\r\nfoo 6\r\n");
        let after = searcher.search(&session.timeline, &session.term).list;
        assert_eq!(after.len(), 6);
        assert_eq!(retain(&current, &after), Some(0));
        assert_eq!(after[0], current);

        // Gone: the next one going to the oldest, the most recent when none is older.
        let gone = Match {
            range: Point::new(Line(1), Column(3))..=Point::new(Line(1), Column(5)),
            ..current.clone()
        };
        assert_eq!(retain(&gone, &after), Some(1));
        let oldest = Match {
            block: BlockId(0),
            source: RowSource::Header(BlockId(0)),
            ..current
        };
        assert_eq!(retain(&oldest, &after), Some(5));
        assert_eq!(retain(&gone, &[]), None);
    }

    #[test]
    fn frozen_matches_come_from_the_cache_until_the_columns_change() {
        let mut session = Session::new(100);
        session.command("echo", &["0123456789abcdefghij0123456789"], 0);
        let mut searcher = Searcher::new();
        searcher.set_query(query("a")).unwrap();
        let wide = searcher.search(&session.timeline, &session.term).list;
        assert_eq!(
            starts(&Matches {
                list: wide,
                capped: false
            }),
            [(RowSource::Output(BlockId(0)), 0, 10)]
        );

        session.timeline.reflow(BlockId(0), 10);
        let narrow = searcher.search(&session.timeline, &session.term).list;
        assert_eq!(narrow[0].range.start(), &Point::new(Line(1), Column(0)));
    }

    #[test]
    fn evicted_blocks_leave_the_cache() {
        let mut session = Session::new(6);
        let mut searcher = Searcher::new();
        searcher.set_query(query("x")).unwrap();
        session.command("a", &["x"], 0);
        searcher.search(&session.timeline, &session.term);
        assert!(searcher.cache.contains_key(&(BlockId(0), Part::Output)));
        for name in ["b", "c", "d"] {
            session.command(name, &["x"], 0);
        }
        searcher.search(&session.timeline, &session.term);
        assert!(!searcher.cache.keys().any(|(id, _)| *id == BlockId(0)));
    }

    #[test]
    fn alternate_screen_finds_nothing() {
        let mut session = Session::new(100);
        session.command("ls", &["foo"], 0);
        session.feed("\x1b[?1049hfoo");
        assert!(search(&session, query("foo")).list.is_empty());
    }
}
