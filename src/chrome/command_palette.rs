//! Command palette (pure, no GPU access): its entries (every action, then the open tabs
//! and panes), the fuzzy search over them, the keys it reacts to, where its box and rows
//! sit, and the file of recently run actions.
//!
//! Entries are ordered by match score, then by how recently their action was run, then
//! in their default order; with an empty query, the recent actions come first.

use std::cmp::Reverse;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use winit::keyboard::{Key, NamedKey};

use super::tab_bar::UiMetrics;
use crate::i18n::{self, Lang, Msg};
use crate::input::Action;
use crate::keybindings::Keybindings;
use crate::layout::Rect;
use crate::terminal::PaneId;
use crate::workspace::{TitleSource, Workspace};

/// Name of the recent actions file inside the state directory.
pub const RECENT_FILE: &str = "palette-recent";
/// Most actions the recent file keeps.
pub const MAX_RECENT: usize = 50;
/// Most rows shown at once; the others are reached by scrolling.
pub const VISIBLE_ROWS: usize = 12;

/// Widest box, in logical pixels.
const MAX_WIDTH: f32 = 600.0;
/// Space kept between the box and the window edges, in logical pixels.
const MARGIN: f32 = 16.0;
/// Top of the box, as a fraction of the window height.
const TOP: f32 = 0.15;
/// Space between the box edges and its content, in logical pixels.
const PADDING: f32 = 8.0;
/// Space above and below the text of a row, in logical pixels.
const ROW_PADDING: f32 = 4.0;
/// Space left and right of the text of a row, in logical pixels.
const TEXT_PADDING: f32 = 12.0;
/// Space between an entry label and its shortcut, in logical pixels.
const SHORTCUT_GAP: f32 = 16.0;

/// What an entry does when it is run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Action(Action),
    /// Shows the tab of this index.
    Tab(usize),
    /// Shows the tab of this pane and focuses it.
    Pane(PaneId),
}

/// A line of the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub command: Command,
    pub label: String,
    pub shortcut: Option<String>,
}

/// The entries of the palette in their default order: every [`Action::ALL`], then each
/// tab of `workspace`, followed by its panes when it holds several. Shortcuts are read
/// from `keybindings`.
pub fn entries<P: TitleSource>(
    workspace: &Workspace<P>,
    lang: Lang,
    keybindings: &Keybindings,
) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Action::ALL
        .into_iter()
        .map(|action| Entry {
            command: Command::Action(action),
            label: i18n::text(lang, &action.label()).into_owned(),
            shortcut: keybindings.label(action),
        })
        .collect();
    for (index, tab) in workspace.tabs().iter().enumerate() {
        let title = workspace.tab_title(index).unwrap_or_default();
        let shortcut = u8::try_from(index + 1)
            .ok()
            .and_then(|number| keybindings.label(Action::GoToTab(number)));
        entries.push(Entry {
            command: Command::Tab(index),
            label: i18n::text(
                lang,
                &Msg::TabEntry {
                    tab: index + 1,
                    title,
                },
            )
            .into_owned(),
            shortcut,
        });
        if tab.is_single() {
            continue;
        }
        for (number, id) in tab.tree.leaves().into_iter().enumerate() {
            let title = workspace.pane_title(id).unwrap_or_default();
            entries.push(Entry {
                command: Command::Pane(id),
                label: i18n::text(
                    lang,
                    &Msg::PaneEntry {
                        tab: index + 1,
                        pane: number + 1,
                        title,
                    },
                )
                .into_owned(),
                shortcut: None,
            });
        }
    }
    entries
}

/// An entry matching the query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Index of the entry in the palette's entries.
    pub entry: usize,
    /// Characters of the label matching the query, by char index, ascending.
    pub indices: Vec<usize>,
}

/// What a key or a click asks of the open palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Close the palette and run this command.
    Run(Command),
    /// Close the palette.
    Close,
}

/// The open palette: its query, the entries matching it, the selected one and the
/// first shown row.
pub struct CommandPalette {
    entries: Vec<Entry>,
    /// Ids of the recently run actions, most recent first.
    recent: Vec<String>,
    query: String,
    matches: Vec<Match>,
    /// Index of the selected match.
    selected: usize,
    /// Index of the first shown match.
    scroll: usize,
    matcher: Matcher,
}

impl CommandPalette {
    /// A palette over `entries`, the actions of `recent` (ids, most recent first)
    /// listed first while the query is empty.
    pub fn new(entries: Vec<Entry>, recent: Vec<String>) -> Self {
        let mut palette = Self {
            entries,
            recent,
            query: String::new(),
            matches: Vec::new(),
            selected: 0,
            scroll: 0,
            matcher: Matcher::new(Config::DEFAULT),
        };
        palette.refilter();
        palette
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// The entries matching the query, best first.
    pub fn matches(&self) -> &[Match] {
        &self.matches
    }

    pub fn entry(&self, found: &Match) -> &Entry {
        &self.entries[found.entry]
    }

    pub fn recent(&self) -> &[String] {
        &self.recent
    }

    /// Index of the selected match.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Index of the first shown match.
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// Handles a key: `text` (the characters it types) edits the query, `Backspace`
    /// erases, `Up` / `Down` move the selection, `Enter` runs the selected entry and
    /// `Escape` closes. `None` while the palette stays open.
    pub fn key(&mut self, key: &Key, text: Option<&str>) -> Option<Outcome> {
        match key {
            Key::Named(NamedKey::Escape) => return Some(Outcome::Close),
            Key::Named(NamedKey::Enter) => return self.choose(self.selected),
            Key::Named(NamedKey::ArrowUp) => self.move_selection(-1),
            Key::Named(NamedKey::ArrowDown) => self.move_selection(1),
            Key::Named(NamedKey::Backspace) => {
                if self.query.pop().is_some() {
                    self.refilter();
                }
            }
            _ => {
                let typed: String = text
                    .unwrap_or_default()
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect();
                if !typed.is_empty() {
                    self.query.push_str(&typed);
                    self.refilter();
                }
            }
        }
        None
    }

    /// Runs the match `index`; `None` when there is no such match.
    pub fn choose(&self, index: usize) -> Option<Outcome> {
        let found = self.matches.get(index)?;
        Some(Outcome::Run(self.entries[found.entry].command))
    }

    /// Scrolls the rows by `rows`, keeping the selection on a shown row.
    pub fn scroll_by(&mut self, rows: isize) {
        let last = self.matches.len().saturating_sub(VISIBLE_ROWS);
        self.scroll = self.scroll.saturating_add_signed(rows).min(last);
        let shown_last = (self.scroll + VISIBLE_ROWS).min(self.matches.len());
        self.selected = self
            .selected
            .clamp(self.scroll, shown_last.saturating_sub(1).max(self.scroll));
    }

    /// Moves the selection by `delta` rows, wrapping around, and scrolls it into view.
    fn move_selection(&mut self, delta: isize) {
        let count = self.matches.len();
        if count == 0 {
            return;
        }
        let count = count as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(count) as usize;
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + VISIBLE_ROWS {
            self.scroll = self.selected + 1 - VISIBLE_ROWS;
        }
    }

    /// Rank of `entry` in the recent actions, `None` when it is not one.
    fn recent_rank(&self, entry: &Entry) -> Option<usize> {
        let Command::Action(action) = entry.command else {
            return None;
        };
        let id = action.id();
        self.recent.iter().position(|recent| *recent == id)
    }

    /// Matches the query against every entry and selects the best one.
    fn refilter(&mut self) {
        let pattern = Pattern::new(
            &self.query,
            CaseMatching::Smart,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut scored = Vec::new();
        let mut chars = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            // Built from the chars so the indices are char indices.
            let haystack = if entry.label.is_ascii() {
                Utf32Str::Ascii(entry.label.as_bytes())
            } else {
                chars.clear();
                chars.extend(entry.label.chars());
                Utf32Str::Unicode(&chars)
            };
            let mut indices = Vec::new();
            let Some(score) = pattern.indices(haystack, &mut self.matcher, &mut indices) else {
                continue;
            };
            indices.sort_unstable();
            indices.dedup();
            let rank = self.recent_rank(entry).unwrap_or(usize::MAX);
            let found = Match {
                entry: index,
                indices: indices.into_iter().map(|i| i as usize).collect(),
            };
            scored.push(((Reverse(score), rank, index), found));
        }
        scored.sort_by_key(|(key, _)| *key);
        self.matches = scored.into_iter().map(|(_, found)| found).collect();
        self.selected = 0;
        self.scroll = 0;
    }
}

/// `$XDG_STATE_HOME/graphene/palette-recent`, else
/// `$HOME/.local/state/graphene/palette-recent`.
pub fn recent_path() -> Option<PathBuf> {
    recent_path_from(std::env::var_os("XDG_STATE_HOME"), std::env::var_os("HOME"))
}

fn recent_path_from(xdg_state_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    // The XDG spec ignores an empty or relative `XDG_STATE_HOME`.
    let xdg = xdg_state_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let base = xdg.or_else(|| {
        home.filter(|home| !home.is_empty())
            .map(|home| PathBuf::from(home).join(".local").join("state"))
    })?;
    Some(base.join("graphene").join(RECENT_FILE))
}

/// The action ids of the recent file, most recent first, at most [`MAX_RECENT`]. A
/// missing or unreadable file gives an empty list; only the latter is logged.
pub fn load_recent(file: &Path) -> Vec<String> {
    match std::fs::read_to_string(file) {
        Ok(text) => text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(MAX_RECENT)
            .map(str::to_owned)
            .collect(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            log::warn!("cannot read {}: {error}", file.display());
            Vec::new()
        }
    }
}

/// Writes `recent` to the recent file, one id per line, at most [`MAX_RECENT`].
pub fn save_recent(file: &Path, recent: &[String]) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut text = String::new();
    for id in recent.iter().take(MAX_RECENT) {
        text.push_str(id);
        text.push('\n');
    }
    std::fs::write(file, text)
}

/// `recent` with `action` moved, or added, to the front, at most [`MAX_RECENT`] long.
pub fn remember(recent: &[String], action: Action) -> Vec<String> {
    let id = action.id();
    std::iter::once(id.clone())
        .chain(recent.iter().filter(|recent| **recent != id).cloned())
        .take(MAX_RECENT)
        .collect()
}

/// A piece of an entry label, drawn highlighted when it matches the query.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub text: String,
    pub x: f32,
    /// Width the piece is truncated to: the room left for the label.
    pub max_width: f32,
    pub highlighted: bool,
}

/// A shown row of the palette.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub rect: Rect,
    /// Index of the match it shows.
    pub index: usize,
    pub selected: bool,
    /// Top of its text.
    pub text_y: f32,
    pub runs: Vec<Run>,
    /// The shortcut and the left edge of its text.
    pub shortcut: Option<(String, f32)>,
}

/// The palette placed in a window, in physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct PaletteLayout {
    pub rect: Rect,
    /// The search field.
    pub input: Rect,
    /// Text of the search field: the query, or a hint while it is empty.
    pub input_text: String,
    pub placeholder: bool,
    /// Left edge and top of the search field text.
    pub text_x: f32,
    pub input_text_y: f32,
    /// Width the search field text is truncated to.
    pub text_width: f32,
    /// The text cursor, after the query.
    pub caret: Rect,
    pub rows: Vec<Row>,
}

/// What the mouse is over in the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteHit {
    /// The row of the match of this index.
    Row(usize),
    Box,
}

impl PaletteLayout {
    /// Lays `palette` out in a `window`: centered, at most [`MAX_WIDTH`] wide, its top
    /// at [`TOP`] of the height, at most [`VISIBLE_ROWS`] rows; texts are measured by
    /// `measure` in pixels.
    pub fn new(
        palette: &CommandPalette,
        lang: Lang,
        window: Rect,
        metrics: UiMetrics,
        mut measure: impl FnMut(&str) -> f32,
    ) -> Self {
        let scale = metrics.scale;
        let line = metrics.line_height;
        let (padding, row_padding, text_padding) =
            (PADDING * scale, ROW_PADDING * scale, TEXT_PADDING * scale);
        let width = (MAX_WIDTH * scale)
            .min(window.width - 2.0 * MARGIN * scale)
            .max(0.0)
            .round();
        let row_height = (line + 2.0 * row_padding).round();
        let x = (window.x + (window.width - width) / 2.0).round();
        let y = (window.y + window.height * TOP).round();
        let inner_width = (width - 2.0 * padding).max(0.0);

        let input = Rect {
            x: x + padding,
            y: y + padding,
            width: inner_width,
            height: row_height,
        };
        let text_x = input.x + text_padding;
        let text_width = (input.width - 2.0 * text_padding).max(0.0);
        let input_text_y = input.y + row_padding;
        let placeholder = palette.query().is_empty();
        let input_text = if placeholder {
            i18n::text(lang, &Msg::PalettePlaceholder).into_owned()
        } else {
            palette.query().to_owned()
        };
        let caret_x = if placeholder {
            text_x
        } else {
            text_x + measure(palette.query()).min(text_width)
        };
        let caret = Rect {
            x: caret_x.round(),
            y: input_text_y,
            width: scale.round().max(1.0),
            height: line,
        };

        let shown = palette
            .matches()
            .len()
            .saturating_sub(palette.scroll())
            .min(VISIBLE_ROWS);
        let rows_top = input.y + input.height + padding;
        let rows = (0..shown)
            .map(|offset| {
                let index = palette.scroll() + offset;
                let rect = Rect {
                    x: input.x,
                    y: rows_top + offset as f32 * row_height,
                    width: inner_width,
                    height: row_height,
                };
                Self::row(
                    palette,
                    index,
                    rect,
                    (row_padding, text_padding, SHORTCUT_GAP * scale),
                    &mut measure,
                )
            })
            .collect();
        let height = if shown == 0 {
            input.height + 2.0 * padding
        } else {
            input.height + 3.0 * padding + shown as f32 * row_height
        };

        Self {
            rect: Rect {
                x,
                y,
                width,
                height,
            },
            input,
            input_text,
            placeholder,
            text_x,
            input_text_y,
            text_width,
            caret,
            rows,
        }
    }

    /// The row of match `index` in `rect`: its label split in matching and other
    /// pieces, its shortcut at the right edge.
    fn row(
        palette: &CommandPalette,
        index: usize,
        rect: Rect,
        (row_padding, text_padding, gap): (f32, f32, f32),
        measure: &mut impl FnMut(&str) -> f32,
    ) -> Row {
        let found = &palette.matches()[index];
        let entry = palette.entry(found);
        let text_x = rect.x + text_padding;
        let right = rect.x + rect.width - text_padding;
        let shortcut = entry.shortcut.as_ref().map(|shortcut| {
            let x = (right - measure(shortcut)).round();
            (shortcut.clone(), x)
        });
        let label_right = shortcut.as_ref().map_or(right, |(_, x)| x - gap);
        // Pieces of consecutive characters that match, or not: (first char, text).
        let mut pieces: Vec<(usize, String, bool)> = Vec::new();
        for (i, c) in entry.label.chars().enumerate() {
            let matching = found.indices.binary_search(&i).is_ok();
            match pieces.last_mut() {
                Some((_, text, highlighted)) if *highlighted == matching => text.push(c),
                _ => pieces.push((i, c.to_string(), matching)),
            }
        }
        let runs = pieces
            .into_iter()
            .filter_map(|(start, text, highlighted)| {
                let prefix: String = entry.label.chars().take(start).collect();
                let x = (text_x + measure(&prefix)).round();
                let max_width = label_right - x;
                (max_width > 0.0).then_some(Run {
                    text,
                    x,
                    max_width,
                    highlighted,
                })
            })
            .collect();
        Row {
            rect,
            index,
            selected: index == palette.selected(),
            text_y: rect.y + row_padding,
            runs,
            shortcut,
        }
    }

    /// What is under the pixel (`x`, `y`): a row, the rest of the box, or `None`
    /// outside of it.
    pub fn hit(&self, x: f32, y: f32) -> Option<PaletteHit> {
        if !self.rect.contains(x, y) {
            return None;
        }
        let row = self.rows.iter().find(|row| row.rect.contains(x, y));
        Some(row.map_or(PaletteHit::Box, |row| PaletteHit::Row(row.index)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::CellSize;
    use crate::workspace::DIVIDER_THICKNESS;
    use crate::workspace::tree::{Axis, Geometry};

    fn entry(label: &str, action: Action) -> Entry {
        Entry {
            command: Command::Action(action),
            label: label.to_owned(),
            shortcut: None,
        }
    }

    fn labels(palette: &CommandPalette) -> Vec<&str> {
        palette
            .matches()
            .iter()
            .map(|found| palette.entry(found).label.as_str())
            .collect()
    }

    fn typed(palette: &mut CommandPalette, text: &str) {
        for c in text.chars() {
            let s = c.to_string();
            assert_eq!(
                palette.key(&Key::Character(s.as_str().into()), Some(&s)),
                None
            );
        }
    }

    fn named(palette: &mut CommandPalette, key: NamedKey) -> Option<Outcome> {
        palette.key(&Key::Named(key), None)
    }

    #[test]
    fn fuzzy_query_keeps_the_matching_entries_best_first() {
        let mut palette = CommandPalette::new(
            vec![
                entry("Scroll to top", Action::ScrollToTop),
                entry("Paste", Action::Paste),
                entry("Split right", Action::SplitRight),
            ],
            Vec::new(),
        );
        typed(&mut palette, "sp");
        // A contiguous match at the start of a word wins; "Paste" has no `s` before
        // a `p`.
        assert_eq!(labels(&palette), ["Split right", "Scroll to top"]);
        assert_eq!(palette.matches()[0].indices, [0, 1]);
    }

    #[test]
    fn match_indices_are_char_indices() {
        let mut palette =
            CommandPalette::new(vec![entry("Égaliser à droite", Action::Copy)], Vec::new());
        typed(&mut palette, "ad");
        let found = &palette.matches()[0];
        let chars: Vec<char> = palette.entry(found).label.chars().collect();
        let matched: String = found.indices.iter().map(|&i| chars[i]).collect();
        // Byte offsets would point inside `É` and `à`; `a` also matches `à`.
        assert_eq!(matched, "àd");
    }

    #[test]
    fn query_case_is_smart() {
        let entries = vec![
            entry("copy output", Action::CopyOutput),
            entry("Copy", Action::Copy),
        ];
        let mut palette = CommandPalette::new(entries.clone(), Vec::new());
        typed(&mut palette, "copy");
        assert_eq!(palette.matches().len(), 2);
        let mut palette = CommandPalette::new(entries, Vec::new());
        typed(&mut palette, "Copy");
        assert_eq!(labels(&palette), ["Copy"]);
    }

    #[test]
    fn recent_actions_come_first_on_an_empty_query() {
        let palette = CommandPalette::new(
            vec![
                entry("New tab", Action::NewTab),
                entry("Split right", Action::SplitRight),
                entry("Copy", Action::Copy),
                entry("Paste", Action::Paste),
            ],
            vec![
                "paste".to_owned(),
                "split_right".to_owned(),
                "gone".to_owned(),
            ],
        );
        assert_eq!(
            labels(&palette),
            ["Paste", "Split right", "New tab", "Copy"]
        );
    }

    #[test]
    fn recent_rank_breaks_equal_scores() {
        let entries = vec![
            entry("Next tab", Action::NextTab),
            entry("Next pane", Action::NextPane),
        ];
        let mut palette = CommandPalette::new(entries.clone(), Vec::new());
        typed(&mut palette, "next");
        assert_eq!(labels(&palette), ["Next tab", "Next pane"]);
        let mut palette = CommandPalette::new(entries, vec!["next_pane".to_owned()]);
        typed(&mut palette, "next");
        assert_eq!(labels(&palette), ["Next pane", "Next tab"]);
    }

    #[test]
    fn keys_edit_the_query_move_and_run() {
        let mut palette = CommandPalette::new(
            vec![
                entry("New tab", Action::NewTab),
                entry("Next tab", Action::NextTab),
                entry("Copy", Action::Copy),
            ],
            Vec::new(),
        );
        typed(&mut palette, "tx");
        assert!(palette.matches().is_empty());
        // Enter without match does nothing.
        assert_eq!(named(&mut palette, NamedKey::Enter), None);
        assert_eq!(named(&mut palette, NamedKey::Backspace), None);
        assert_eq!(palette.query(), "t");
        assert_eq!(labels(&palette), ["New tab", "Next tab"]);
        // Control characters typed with Ctrl are not text.
        assert_eq!(palette.key(&Key::Character("p".into()), Some("\x10")), None);
        assert_eq!(palette.query(), "t");

        named(&mut palette, NamedKey::ArrowDown);
        assert_eq!(palette.selected(), 1);
        named(&mut palette, NamedKey::ArrowDown);
        assert_eq!(palette.selected(), 0, "wraps to the first");
        named(&mut palette, NamedKey::ArrowUp);
        assert_eq!(palette.selected(), 1, "wraps to the last");
        assert_eq!(
            named(&mut palette, NamedKey::Enter),
            Some(Outcome::Run(Command::Action(Action::NextTab)))
        );
        assert_eq!(named(&mut palette, NamedKey::Escape), Some(Outcome::Close));
        // Editing the query selects the best match again.
        typed(&mut palette, "a");
        assert_eq!(palette.selected(), 0);
        assert_eq!(
            palette.choose(1),
            Some(Outcome::Run(Command::Action(Action::NextTab)))
        );
        assert_eq!(palette.choose(5), None);
    }

    #[test]
    fn selection_scrolls_past_the_visible_rows() {
        let entries = Action::ALL
            .into_iter()
            .map(|action| entry(&action.id(), action))
            .collect();
        let mut palette = CommandPalette::new(entries, Vec::new());
        for _ in 0..VISIBLE_ROWS {
            named(&mut palette, NamedKey::ArrowDown);
        }
        assert_eq!((palette.selected(), palette.scroll()), (VISIBLE_ROWS, 1));
        named(&mut palette, NamedKey::ArrowUp);
        named(&mut palette, NamedKey::ArrowUp);
        assert_eq!(palette.scroll(), 1);
        // Up from the first wraps to the last, at the bottom of the shown rows.
        let mut palette = CommandPalette::new(palette.entries.clone(), Vec::new());
        named(&mut palette, NamedKey::ArrowUp);
        let last = Action::ALL.len() - 1;
        assert_eq!(
            (palette.selected(), palette.scroll()),
            (last, last + 1 - VISIBLE_ROWS)
        );
        // Scrolling keeps the selection on a shown row.
        palette.scroll_by(-100);
        assert_eq!(palette.scroll(), 0);
        assert_eq!(palette.selected(), VISIBLE_ROWS - 1);
        palette.scroll_by(3);
        assert_eq!(palette.scroll(), 3);
    }

    #[derive(Default)]
    struct Fake(Option<String>);

    impl TitleSource for Fake {
        fn osc_title(&self) -> Option<&str> {
            self.0.as_deref()
        }
        fn current_dir(&self) -> Option<PathBuf> {
            None
        }
        fn shell_name(&self) -> Option<String> {
            None
        }
    }

    fn geometry() -> Geometry {
        Geometry {
            area: Rect::from_size(800, 600),
            cell: CellSize {
                width: 10.0,
                height: 20.0,
            },
            divider: DIVIDER_THICKNESS,
        }
    }

    #[test]
    fn entries_list_actions_then_tabs_and_their_panes() {
        let mut ws = Workspace::new();
        let first = ws.allocate_id();
        ws.new_tab(first, Fake(Some("~/dev".into())));
        let second = ws.allocate_id();
        ws.new_tab(second, Fake(Some("htop".into())));
        let third = ws
            .split(Axis::Horizontal, geometry(), |_| {
                Some(Fake(Some("vim".into())))
            })
            .expect("split fits");

        let entries = entries(&ws, Lang::En, &Keybindings::default());
        let actions = Action::ALL.len();
        assert_eq!(entries.len(), actions + 4);
        assert_eq!(entries[0].label, "New tab");
        assert_eq!(entries[0].shortcut.as_deref(), Some("Ctrl+Shift+T"));
        let rest: Vec<(&str, Command, Option<&str>)> = entries[actions..]
            .iter()
            .map(|e| (e.label.as_str(), e.command, e.shortcut.as_deref()))
            .collect();
        assert_eq!(
            rest,
            [
                ("Tab 1: ~/dev", Command::Tab(0), Some("Alt+1")),
                // The tab title follows its active pane, the one split off.
                ("Tab 2: vim", Command::Tab(1), Some("Alt+2")),
                ("Tab 2 > pane 1: htop", Command::Pane(second), None),
                ("Tab 2 > pane 2: vim", Command::Pane(third), None),
            ]
        );
        // Pane entries are found by their program.
        let mut palette = CommandPalette::new(entries, Vec::new());
        typed(&mut palette, "htop");
        assert_eq!(palette.choose(0), Some(Outcome::Run(Command::Pane(second))));
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("graphene-palette-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("temp dir");
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn recent_path_prefers_xdg_state_home() {
        let path = recent_path_from(Some("/state".into()), Some("/home/u".into()));
        assert_eq!(path, Some(PathBuf::from("/state/graphene/palette-recent")));
        let path = recent_path_from(Some("relative".into()), Some("/home/u".into()));
        assert_eq!(
            path,
            Some(PathBuf::from(
                "/home/u/.local/state/graphene/palette-recent"
            ))
        );
        assert_eq!(recent_path_from(None, None), None);
    }

    #[test]
    fn recent_file_round_trips_and_is_capped() {
        let dir = TempDir::new("round-trip");
        let file = dir.0.join("graphene").join(RECENT_FILE);
        assert!(load_recent(&file).is_empty(), "missing file");

        let mut recent = Vec::new();
        for action in Action::ALL.into_iter().chain(Action::ALL) {
            recent = remember(&recent, action);
        }
        // Each action once, the last run first.
        assert_eq!(recent.len(), Action::ALL.len());
        assert_eq!(recent[0], Action::FontSizeReset.id());
        let many: Vec<String> = (0..80).map(|i| format!("action_{i}")).collect();
        save_recent(&file, &many).expect("saved");
        let loaded = load_recent(&file);
        assert_eq!(loaded.len(), MAX_RECENT);
        assert_eq!(loaded, many[..MAX_RECENT]);
        assert_eq!(remember(&loaded, Action::Copy).len(), MAX_RECENT);

        save_recent(&file, &recent).expect("saved");
        assert_eq!(load_recent(&file), recent);
    }

    #[test]
    fn unreadable_recent_file_is_an_empty_list() {
        let dir = TempDir::new("unreadable");
        // A directory cannot be read as a file.
        assert!(load_recent(&dir.0).is_empty());
        let file = dir.0.join(RECENT_FILE);
        std::fs::write(&file, [0xff, 0xfe, b'\n']).expect("written");
        assert!(load_recent(&file).is_empty());
    }

    const METRICS: UiMetrics = UiMetrics {
        line_height: 20.0,
        scale: 1.0,
    };

    fn measure(text: &str) -> f32 {
        text.chars().count() as f32 * 10.0
    }

    #[test]
    fn layout_is_capped_in_width_and_placed_at_the_top() {
        let palette = CommandPalette::new(vec![entry("Copy", Action::Copy)], Vec::new());
        let wide = PaletteLayout::new(
            &palette,
            Lang::En,
            Rect::from_size(2000, 1000),
            METRICS,
            measure,
        );
        assert_eq!(wide.rect.width, MAX_WIDTH);
        assert_eq!(wide.rect.x, 700.0);
        assert_eq!(wide.rect.y, 150.0);
        assert!(wide.placeholder);
        assert_eq!(wide.caret.x, wide.text_x);
        let narrow = PaletteLayout::new(
            &palette,
            Lang::En,
            Rect::from_size(300, 1000),
            METRICS,
            measure,
        );
        assert_eq!(narrow.rect.width, 300.0 - 2.0 * MARGIN);
        assert_eq!(narrow.rect.x, MARGIN);
    }

    #[test]
    fn layout_shows_at_most_the_visible_rows_from_the_scroll() {
        let entries: Vec<Entry> = Action::ALL
            .into_iter()
            .map(|action| entry(&action.id(), action))
            .collect();
        let mut palette = CommandPalette::new(entries, Vec::new());
        palette.scroll_by(2);
        let layout = PaletteLayout::new(
            &palette,
            Lang::En,
            Rect::from_size(1000, 1000),
            METRICS,
            measure,
        );
        assert_eq!(layout.rows.len(), VISIBLE_ROWS);
        assert_eq!(layout.rows[0].index, 2);
        assert!(layout.rows[0].selected, "the selection follows the scroll");
        assert!(layout.rows.windows(2).all(|w| w[0].rect.y < w[1].rect.y));
        let last = layout.rows.last().unwrap().rect;
        assert!(last.y + last.height <= layout.rect.y + layout.rect.height);

        let row = layout.rows[3].rect;
        assert_eq!(
            layout.hit(row.x + 1.0, row.y + 1.0),
            Some(PaletteHit::Row(5))
        );
        assert_eq!(
            layout.hit(layout.input.x + 1.0, layout.input.y + 1.0),
            Some(PaletteHit::Box)
        );
        assert_eq!(layout.hit(0.0, 0.0), None);
    }

    #[test]
    fn layout_splits_labels_into_highlighted_runs_and_places_shortcuts() {
        let mut palette = CommandPalette::new(
            vec![Entry {
                shortcut: Some("Ctrl+Shift+E".to_owned()),
                ..entry("Split right", Action::SplitRight)
            }],
            Vec::new(),
        );
        typed(&mut palette, "sr");
        let layout = PaletteLayout::new(
            &palette,
            Lang::En,
            Rect::from_size(1000, 1000),
            METRICS,
            measure,
        );
        assert!(!layout.placeholder);
        assert_eq!(layout.input_text, "sr");
        assert_eq!(layout.caret.x, layout.text_x + 20.0);
        let row = &layout.rows[0];
        let runs: Vec<(&str, bool)> = row
            .runs
            .iter()
            .map(|run| (run.text.as_str(), run.highlighted))
            .collect();
        assert_eq!(
            runs,
            [("S", true), ("plit ", false), ("r", true), ("ight", false)]
        );
        let text_x = row.rect.x + TEXT_PADDING;
        assert_eq!(row.runs[0].x, text_x);
        assert_eq!(row.runs[2].x, text_x + 60.0);
        let (shortcut, x) = row.shortcut.clone().expect("shortcut");
        assert_eq!(shortcut, "Ctrl+Shift+E");
        assert_eq!(x, row.rect.x + row.rect.width - TEXT_PADDING - 120.0);
        assert!(row.runs.iter().all(|run| run.x + run.max_width <= x));
    }
}
