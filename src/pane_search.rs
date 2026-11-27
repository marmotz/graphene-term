//! State of the search bar of a pane: what is typed and toggled, the matches it finds
//! and the current one, and whether the bar has the keyboard. Pure: the pane hands it
//! its locked timeline and terminal.

use alacritty_terminal::index::Line;
use alacritty_terminal::term::Term;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::chrome::search_bar::SearchBarView;
use crate::chrome::search_field::SearchField;
use crate::i18n::{self, Lang, Msg};
use crate::input::Action;
use crate::search::{self, Match, Matches, Query, Searcher, StatusFilter, Zone};
use crate::view::{RowSource, ViewRow};

/// What a key does while the search bar has the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKey {
    /// `Escape`: close the bar.
    Close,
    /// `Enter`: go to the older match.
    Older,
    /// `Shift+Enter`: go to the more recent match.
    Newer,
    /// The search shortcut: select the text of the field.
    SelectAll,
    /// The paste shortcut: paste into the field.
    Paste,
    /// Any other shortcut, run as usual.
    Shortcut(Action),
    /// Any other key edits the field; it never reaches the shell.
    Edit,
}

/// What `key`, held with `mods` and bound to `action`, does in a search bar that has
/// the keyboard.
pub fn bar_key(key: &Key, mods: ModifiersState, action: Option<Action>) -> BarKey {
    match key {
        Key::Named(NamedKey::Escape) => return BarKey::Close,
        Key::Named(NamedKey::Enter) if mods.shift_key() => return BarKey::Newer,
        Key::Named(NamedKey::Enter) => return BarKey::Older,
        _ => {}
    }
    match action {
        Some(Action::OpenSearch) => BarKey::SelectAll,
        Some(Action::Paste) => BarKey::Paste,
        Some(action) => BarKey::Shortcut(action),
        None => BarKey::Edit,
    }
}

/// What the counter of the bar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Counter {
    /// Nothing typed.
    Empty,
    /// The query is not a valid regex.
    Invalid,
    /// `current` (1 for the most recent, `None` without match) among `total`; `capped`
    /// when the search stopped at [`search::CAP`].
    Count {
        current: Option<usize>,
        total: usize,
        capped: bool,
    },
}

/// The open search bar of a pane.
pub struct SearchState {
    pub field: SearchField,
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
    pub zone: Zone,
    pub status: StatusFilter,
    /// The bar has the keyboard; otherwise keys go to the shell, the bar staying open.
    pub focused: bool,
    searcher: Searcher,
    matches: Matches,
    /// Index of the current match in `matches.list`.
    current: Option<usize>,
    invalid: bool,
    /// Revision of the pane at the last search, `None` before the first one.
    seen: Option<u64>,
    /// Changes whenever the matches or the current one change, so that a frame
    /// highlighting them is captured again.
    revision: u64,
}

impl Default for SearchState {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchState {
    /// A bar with an empty field, which has the keyboard.
    pub fn new() -> Self {
        Self {
            field: SearchField::default(),
            case_sensitive: false,
            regex: false,
            whole_word: false,
            zone: Zone::default(),
            status: StatusFilter::default(),
            focused: true,
            searcher: Searcher::new(),
            matches: Matches::default(),
            current: None,
            invalid: false,
            seen: None,
            revision: 0,
        }
    }

    /// The query of the field and the toggles.
    pub fn query(&self) -> Query {
        Query {
            text: self.field.text().to_owned(),
            case_sensitive: self.case_sensitive,
            regex: self.regex,
            whole_word: self.whole_word,
            zone: self.zone,
            status: self.status,
        }
    }

    /// The matches of the last search, oldest first.
    pub fn matches(&self) -> &Matches {
        &self.matches
    }

    /// The current match.
    pub fn current_match(&self) -> Option<&Match> {
        self.current.and_then(|index| self.matches.list.get(index))
    }

    /// Changes whenever the matches or the current one change.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn counter(&self) -> Counter {
        if self.field.text().is_empty() {
            Counter::Empty
        } else if self.invalid {
            Counter::Invalid
        } else {
            let total = self.matches.list.len();
            Counter::Count {
                current: self.current.map(|index| search::number(index, total)),
                total,
                capped: self.matches.capped,
            }
        }
    }

    /// Searches `timeline` over `live` for the query as it is now, the pane being at
    /// `revision`: the current match becomes the most recent one starting above the
    /// bottom of the view `rows`, or the most recent one.
    pub fn requery<L>(
        &mut self,
        timeline: &crate::timeline::Timeline,
        live: &Term<L>,
        rows: &[ViewRow],
        revision: u64,
    ) {
        self.invalid = self.searcher.set_query(self.query()).is_err();
        self.matches = self.searcher.search(timeline, live);
        let bottom = bottom_key(rows);
        self.current = search::initial(&self.matches.list, |m| {
            bottom.is_some_and(|bottom| match_key(m, live) <= bottom)
        });
        self.seen = Some(revision);
        self.revision += 1;
    }

    /// Searches again when the pane changed since the last search (`revision`): output
    /// arrived, a block finished or the columns changed. The current match stays on
    /// its text, or the next one going to the oldest when it is gone. Returns whether
    /// it searched.
    pub fn refresh<L>(
        &mut self,
        timeline: &crate::timeline::Timeline,
        live: &Term<L>,
        revision: u64,
    ) -> bool {
        if self.seen == Some(revision) {
            return false;
        }
        let old = self.current_match().cloned();
        self.invalid = self.searcher.set_query(self.query()).is_err();
        self.matches = self.searcher.search(timeline, live);
        self.current = match old {
            Some(old) => search::retain(&old, &self.matches.list),
            None => None,
        };
        self.seen = Some(revision);
        self.revision += 1;
        true
    }

    /// Makes the next match going to the oldest (`older`) or to the most recent the
    /// current one, wrapping at the ends, and returns it. Without current match, the
    /// most recent one.
    pub fn step(&mut self, older: bool) -> Option<&Match> {
        let len = self.matches.list.len();
        if len == 0 {
            return None;
        }
        self.current = Some(match self.current {
            Some(index) if older => search::older(index, len),
            Some(index) => search::newer(index, len),
            None => len - 1,
        });
        self.revision += 1;
        self.current_match()
    }

    /// What the bar shows, in `lang`; `filters` tells whether the history is split into
    /// blocks the zone and status selectors can filter.
    pub fn view(&self, filters: bool, lang: Lang) -> SearchBarView {
        let text = |msg: Msg| i18n::text(lang, &msg).into_owned();
        let counter = match self.counter() {
            Counter::Empty => String::new(),
            Counter::Invalid => text(Msg::InvalidRegex),
            Counter::Count {
                current,
                total,
                capped,
            } => {
                let current = current.map_or_else(|| "0".to_owned(), |n| n.to_string());
                let plus = if capped { "+" } else { "" };
                format!("{current}/{total}{plus}")
            }
        };
        SearchBarView {
            text: self.field.text().to_owned(),
            caret: self.field.caret(),
            selected: self.field.selected(),
            focused: self.focused,
            placeholder: text(Msg::SearchPlaceholder),
            counter,
            invalid: self.counter() == Counter::Invalid,
            case_sensitive: self.case_sensitive,
            regex: self.regex,
            whole_word: self.whole_word,
            zone: text(match self.zone {
                Zone::Both => Msg::SearchZoneBoth,
                Zone::Command => Msg::SearchZoneCommand,
                Zone::Output => Msg::SearchZoneOutput,
            }),
            status: text(match self.status {
                StatusFilter::All => Msg::SearchStatusAll,
                StatusFilter::Success => Msg::SearchStatusSuccess,
                StatusFilter::Failure => Msg::SearchStatusFailure,
            }),
            filters,
            zone_active: self.zone != Zone::Both,
            status_active: self.status != StatusFilter::All,
        }
    }

    /// Shows the next zone: both, commands, outputs, then both again.
    pub fn next_zone(&mut self) {
        self.zone = match self.zone {
            Zone::Both => Zone::Command,
            Zone::Command => Zone::Output,
            Zone::Output => Zone::Both,
        };
    }

    /// Shows the next status filter: all, successes, failures, then all again.
    pub fn next_status(&mut self) {
        self.status = match self.status {
            StatusFilter::All => StatusFilter::Success,
            StatusFilter::Success => StatusFilter::Failure,
            StatusFilter::Failure => StatusFilter::All,
        };
    }
}

/// The line of `live` or of a frozen grid where `m` starts, in the lines of the view
/// rows (live history lines are negative).
pub fn start_line<L>(m: &Match, live: &Term<L>) -> Line {
    match m.source {
        RowSource::Live => m.grid_range(live).start().line,
        _ => m.range.start().line,
    }
}

/// Order of a place in the timeline, top to bottom: block, part, line.
type Place = (crate::timeline::BlockId, u8, Line);

fn match_key<L>(m: &Match, live: &Term<L>) -> Place {
    (m.block, part(m.source), start_line(m, live))
}

/// The place of the last row of the view, `None` without row of a block.
fn bottom_key(rows: &[ViewRow]) -> Option<Place> {
    let row = rows.iter().rev().find(|row| row.block.is_some())?;
    let block = row.block?;
    Some(match row.source {
        // A folded block's summary stands for all its output.
        RowSource::Summary(_) => (block, part(RowSource::Output(block)), Line(i32::MAX)),
        source => (block, part(source), row.line),
    })
}

fn part(source: RowSource) -> u8 {
    match source {
        RowSource::Header(_) => 0,
        RowSource::Output(_) | RowSource::Summary(_) => 1,
        RowSource::Live => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::testing::*;
    use crate::view;
    use alacritty_terminal::grid::Dimensions;

    /// Finished blocks echoing `foo`, then a prompt.
    fn session() -> Session {
        let mut session = Session::new(100);
        session.command("a", &["foo 1"], 0);
        session.command("b", &["foo 2"], 1);
        session.command("c", &["foo 3"], 0);
        session.feed(&format!("{}$ ", prompt()));
        session
    }

    fn rows(session: &mut Session) -> Vec<ViewRow> {
        let mut anchor = view::Anchor::default();
        let lines = session.term.screen_lines();
        view::prepare(&mut session.timeline, &session.term, lines, &mut anchor)
    }

    fn typed(state: &mut SearchState, session: &mut Session, text: &str) {
        state.field.insert(text);
        let rows = rows(session);
        state.requery(&session.timeline, &session.term, &rows, 1);
    }

    #[test]
    fn keys_of_a_focused_bar_never_reach_the_shell() {
        let none = ModifiersState::empty();
        let shift = ModifiersState::SHIFT;
        let escape = Key::Named(NamedKey::Escape);
        let enter = Key::Named(NamedKey::Enter);
        let f = Key::Character("F".into());
        assert_eq!(bar_key(&escape, none, None), BarKey::Close);
        assert_eq!(bar_key(&enter, none, None), BarKey::Older);
        assert_eq!(bar_key(&enter, shift, None), BarKey::Newer);
        // Bound keys first ask the search and paste shortcuts, then run as usual.
        let cs = ModifiersState::CONTROL | ModifiersState::SHIFT;
        assert_eq!(bar_key(&f, cs, Some(Action::OpenSearch)), BarKey::SelectAll);
        let v = Key::Character("V".into());
        assert_eq!(bar_key(&v, cs, Some(Action::Paste)), BarKey::Paste);
        let t = Key::Character("T".into());
        assert_eq!(
            bar_key(&t, cs, Some(Action::NewTab)),
            BarKey::Shortcut(Action::NewTab)
        );
        // `Escape` and `Enter` stay the bar's even when bound.
        assert_eq!(bar_key(&escape, none, Some(Action::NewTab)), BarKey::Close);
        let a = Key::Character("a".into());
        assert_eq!(bar_key(&a, none, None), BarKey::Edit);
        assert_eq!(
            bar_key(
                &Key::Named(NamedKey::Backspace),
                ModifiersState::CONTROL,
                None
            ),
            BarKey::Edit
        );
    }

    #[test]
    fn a_new_bar_has_the_keyboard_and_finds_nothing() {
        let state = SearchState::new();
        assert!(state.focused);
        assert_eq!(state.counter(), Counter::Empty);
        assert_eq!(state.current_match(), None);
    }

    #[test]
    fn typing_makes_the_most_recent_match_in_the_view_current() {
        let mut session = session();
        let mut state = SearchState::new();
        typed(&mut state, &mut session, "foo");
        assert_eq!(
            state.counter(),
            Counter::Count {
                current: Some(1),
                total: 3,
                capped: false
            }
        );
        let current = state.current_match().expect("a current match");
        assert_eq!(current.block, session.timeline.blocks()[2].id);
    }

    #[test]
    fn the_initial_match_is_above_the_bottom_of_the_view() {
        let session = session();
        let mut state = SearchState::new();
        state.field.insert("foo");
        // The view scrolled up: its last row is the header of the second block.
        let second = session.timeline.blocks()[1].id;
        let rows = [ViewRow {
            source: RowSource::Header(second),
            block: Some(second),
            line: Line(0),
            hidden: 0,
            top: 0,
            gap_before: false,
        }];
        state.requery(&session.timeline, &session.term, &rows, 1);
        assert_eq!(
            state.current_match().map(|m| m.block),
            Some(session.timeline.blocks()[0].id)
        );
        // Without row of a block, the most recent match.
        state.requery(&session.timeline, &session.term, &[], 1);
        assert_eq!(state.current, Some(2));
    }

    #[test]
    fn steps_go_to_the_oldest_then_wrap() {
        let mut session = session();
        let mut state = SearchState::new();
        typed(&mut state, &mut session, "foo");
        let number = |state: &SearchState| match state.counter() {
            Counter::Count { current, .. } => current,
            other => panic!("{other:?}"),
        };
        let revision = state.revision();
        assert!(state.step(true).is_some());
        assert_eq!(number(&state), Some(2));
        assert!(state.revision() > revision);
        state.step(true);
        state.step(true);
        assert_eq!(number(&state), Some(1), "wrapped to the most recent");
        state.step(false);
        assert_eq!(number(&state), Some(3), "wrapped to the oldest");
    }

    #[test]
    fn an_invalid_regex_is_shown_and_finds_nothing() {
        let mut session = session();
        let mut state = SearchState::new();
        state.regex = true;
        typed(&mut state, &mut session, "foo(");
        assert_eq!(state.counter(), Counter::Invalid);
        assert!(state.matches().list.is_empty());
        assert!(state.step(true).is_none());
    }

    #[test]
    fn a_refresh_searches_once_per_revision_and_keeps_the_current_match() {
        let mut session = session();
        let mut state = SearchState::new();
        typed(&mut state, &mut session, "foo");
        state.step(true);
        let current = state.current_match().cloned();
        assert!(
            !state.refresh(&session.timeline, &session.term, 1),
            "same revision"
        );

        session.feed("x\r\n");
        session.command("d", &["foo 4"], 0);
        assert!(state.refresh(&session.timeline, &session.term, 2));
        assert_eq!(state.matches().list.len(), 4);
        assert_eq!(state.current_match().cloned(), current);
    }

    #[test]
    fn the_view_shows_the_counter_and_the_selector_labels() {
        let mut session = session();
        let mut state = SearchState::new();
        let view = state.view(true, Lang::En);
        assert_eq!(
            (view.counter.as_str(), view.placeholder.as_str()),
            ("", "Search")
        );
        assert_eq!((view.zone.as_str(), view.status.as_str()), ("All", "Any"));
        assert!(!view.zone_active && !view.status_active);

        typed(&mut state, &mut session, "foo");
        state.step(true);
        assert_eq!(state.view(true, Lang::En).counter, "2/3");
        state.next_zone();
        state.next_status();
        let view = state.view(false, Lang::Fr);
        assert_eq!(
            (view.zone.as_str(), view.status.as_str()),
            ("Commandes", "Succès")
        );
        assert!(view.zone_active && view.status_active && !view.filters);

        typed(&mut state, &mut session, "zzz");
        assert_eq!(state.view(true, Lang::En).counter, "0/0");
        state.regex = true;
        typed(&mut state, &mut session, "(");
        let view = state.view(true, Lang::Fr);
        assert_eq!(view.counter, "Regex invalide");
        assert!(view.invalid);
    }

    #[test]
    fn toggles_and_selectors_change_the_query() {
        let mut state = SearchState::new();
        state.field.insert("x");
        state.case_sensitive = true;
        state.next_zone();
        state.next_status();
        state.next_status();
        let query = state.query();
        assert_eq!(query.text, "x");
        assert!(query.case_sensitive);
        assert_eq!(query.zone, Zone::Command);
        assert_eq!(query.status, StatusFilter::Failure);
        state.next_zone();
        state.next_zone();
        state.next_status();
        assert_eq!((state.zone, state.status), (Zone::Both, StatusFilter::All));
    }
}
