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

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Term;

use super::tab_bar::UiMetrics;
use crate::block_actions::{self, Direction as BlockDirection, Part};
use crate::config::{MAX_FONT_SIZE, MIN_FONT_SIZE};
use crate::i18n::{self, Lang, Msg};
use crate::input::Action;
use crate::keybindings::Keybindings;
use crate::layout::Rect;
use crate::terminal::PaneId;
use crate::timeline::{BlockId, Timeline};
use crate::view::{self, ScrollTarget};
use crate::workspace::tree::{Axis, Geometry};
use crate::workspace::{RESIZE_STEP_CELLS, TitleSource, Workspace};

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
/// Width of the accent bar on the left edge of the selected row, in logical pixels: the
/// width of the active pane border.
const MARKER_WIDTH: f32 = 2.0;

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

/// What the active pane allows, read when the palette opens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaneContext {
    /// A text selection or a selected block to copy.
    pub copy: bool,
    pub copy_command: bool,
    pub copy_output: bool,
    pub previous_block: bool,
    pub next_block: bool,
    pub toggle_fold: bool,
    pub toggle_fold_all: bool,
    pub scroll_page_up: bool,
    pub scroll_page_down: bool,
    pub scroll_to_top: bool,
    pub scroll_to_bottom: bool,
}

impl PaneContext {
    /// What a pane showing `timeline` over `live` allows, `selected` being its selected
    /// block and `has_selection` whether it holds a text selection.
    pub fn read<L>(
        timeline: &Timeline,
        live: &Term<L>,
        selected: Option<BlockId>,
        has_selection: bool,
    ) -> Self {
        let lines = live.screen_lines();
        let target = block_actions::target(timeline, selected);
        let part = |part| target.and_then(|id| block_actions::text(timeline, id, part));
        // A navigation at either end selects the selected block again.
        let moves = |direction| {
            block_actions::neighbour(timeline, selected, direction)
                .is_some_and(|id| Some(id) != selected)
        };
        let max_timeline = view::max_timeline_scroll(timeline, live, lines);
        let timeline_scroll = timeline.scroll.min(max_timeline);
        let (page_up, page_down) =
            match view::page_target(selected, |id| view::can_scroll(timeline, live, id, lines)) {
                ScrollTarget::Block(id) => {
                    let max = view::max_block_scroll(timeline, live, id, lines);
                    let scroll = if id == timeline.current().id {
                        timeline.current().scroll
                    } else {
                        timeline.block(id).map_or(0, |block| block.scroll)
                    };
                    // Down past the bottom of its output moves the timeline.
                    (
                        scroll.min(max) < max,
                        scroll.min(max) > 0 || timeline_scroll > 0,
                    )
                }
                ScrollTarget::Timeline => (timeline_scroll < max_timeline, timeline_scroll > 0),
            };
        Self {
            copy: has_selection
                || selected.is_some_and(|id| block_actions::block_text(timeline, id).is_some()),
            copy_command: part(Part::Command).is_some(),
            copy_output: part(Part::Output).is_some(),
            previous_block: moves(BlockDirection::Previous),
            next_block: moves(BlockDirection::Next),
            toggle_fold: target.is_some(),
            toggle_fold_all: !timeline.blocks().is_empty(),
            scroll_page_up: page_up,
            scroll_page_down: page_down,
            scroll_to_top: timeline_scroll < max_timeline,
            scroll_to_bottom: timeline_scroll > 0 || timeline.current().scroll > 0,
        }
    }
}

/// What the window allows besides its workspace, read when the palette opens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Context {
    /// The active pane's context; `None` without a pane.
    pub pane: Option<PaneContext>,
    /// The clipboard holds text to paste.
    pub paste: bool,
    pub font_size: f32,
    pub default_font_size: f32,
}

/// Whether `action` would do something now, in `workspace` laid out in `geometry`.
pub fn available<P>(
    action: Action,
    workspace: &Workspace<P>,
    geometry: Geometry,
    context: &Context,
) -> bool {
    let several_panes = workspace.active_tab().is_some_and(|tab| !tab.is_single());
    let pane = context.pane.unwrap_or_default();
    match action {
        Action::NewTab
        | Action::CloseTab
        | Action::OpenSettings
        | Action::CommandPalette
        | Action::Quit => true,
        Action::GoToTab(number) => usize::from(number) <= workspace.tabs().len(),
        Action::SplitRight => workspace.can_split(Axis::Horizontal, geometry),
        Action::SplitDown => workspace.can_split(Axis::Vertical, geometry),
        Action::ClosePane => workspace.active_pane_id().is_some(),
        Action::NextTab | Action::PreviousTab => workspace.tabs().len() > 1,
        Action::NextPane => workspace.next_pane().is_some(),
        Action::PreviousPane => workspace.previous_pane().is_some(),
        Action::FocusPane(direction) => workspace.neighbour(direction, geometry).is_some(),
        Action::ResizePane(direction) => {
            workspace.can_resize_active(direction, RESIZE_STEP_CELLS, geometry)
        }
        Action::ToggleZoom | Action::EqualisePanes | Action::DetachPane => several_panes,
        Action::FontSizeIncrease => context.font_size < MAX_FONT_SIZE,
        Action::FontSizeDecrease => context.font_size > MIN_FONT_SIZE,
        Action::FontSizeReset => {
            context.font_size
                != context
                    .default_font_size
                    .clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
        }
        Action::Paste => context.paste && context.pane.is_some(),
        Action::Copy => pane.copy,
        Action::CopyCommand => pane.copy_command,
        Action::CopyOutput => pane.copy_output,
        Action::PreviousBlock => pane.previous_block,
        Action::NextBlock => pane.next_block,
        Action::ToggleFold => pane.toggle_fold,
        Action::ToggleFoldAll => pane.toggle_fold_all,
        Action::ScrollPageUp => pane.scroll_page_up,
        Action::ScrollPageDown => pane.scroll_page_down,
        Action::ScrollToTop => pane.scroll_to_top,
        Action::ScrollToBottom => pane.scroll_to_bottom,
    }
}

/// The entries of the palette in their default order: every [`Action::ALL`] that is
/// [`available`], then each tab of `workspace`, followed by its panes when it holds
/// several. Shortcuts are read from `keybindings`.
pub fn entries<P: TitleSource>(
    workspace: &Workspace<P>,
    geometry: Geometry,
    context: &Context,
    lang: Lang,
    keybindings: &Keybindings,
) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Action::ALL
        .into_iter()
        .filter(|&action| available(action, workspace, geometry, context))
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

/// `$XDG_STATE_HOME/graphene/palette-recent`, else the platform's application state folder.
pub fn recent_path() -> Option<PathBuf> {
    recent_path_from(std::env::var_os("XDG_STATE_HOME"), std::env::var_os("HOME"))
}

fn recent_path_from(xdg_state_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    // The XDG spec ignores an empty or relative `XDG_STATE_HOME`.
    let xdg = xdg_state_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let base = xdg.or_else(|| home.filter(|home| !home.is_empty()).map(default_state_base))?;
    Some(base.join("graphene").join(RECENT_FILE))
}

fn default_state_base(home: OsString) -> PathBuf {
    #[cfg(target_os = "macos")]
    let base = PathBuf::from(home)
        .join("Library")
        .join("Application Support");
    #[cfg(not(target_os = "macos"))]
    let base = PathBuf::from(home).join(".local").join("state");
    base
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
    /// The accent bar on the left edge of the selected row, when it is shown.
    pub marker: Option<Rect>,
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
        let rows: Vec<Row> = (0..shown)
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
        let marker = rows.iter().find(|row| row.selected).map(|row| Rect {
            x: row.rect.x,
            y: row.rect.y,
            width: (MARKER_WIDTH * scale).round().max(1.0),
            height: row.rect.height,
        });
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
            marker,
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
    use crate::i18n::Direction;
    use crate::layout::CellSize;
    use crate::timeline::testing::Session;
    use crate::workspace::DIVIDER_THICKNESS;

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

        let entries = entries(
            &ws,
            geometry(),
            &context(),
            Lang::En,
            &Keybindings::default(),
        );
        let actions = entries
            .iter()
            .filter(|e| matches!(e.command, Command::Action(_)))
            .count();
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

    /// A window with a pane allowing nothing, an empty clipboard and the default
    /// font size.
    fn context() -> Context {
        Context {
            pane: Some(PaneContext::default()),
            paste: false,
            font_size: 12.0,
            default_font_size: 12.0,
        }
    }

    fn actions<P: TitleSource>(workspace: &Workspace<P>, context: &Context) -> Vec<Action> {
        entries(
            workspace,
            geometry(),
            context,
            Lang::En,
            &Keybindings::default(),
        )
        .into_iter()
        .filter_map(|e| match e.command {
            Command::Action(action) => Some(action),
            _ => None,
        })
        .collect()
    }

    fn single_pane() -> Workspace<Fake> {
        let mut ws = Workspace::new();
        let id = ws.allocate_id();
        ws.new_tab(id, Fake(None));
        ws
    }

    #[test]
    fn a_lone_pane_with_nothing_to_act_on_offers_only_what_it_can_do() {
        assert_eq!(
            actions(&single_pane(), &context()),
            [
                Action::NewTab,
                Action::CloseTab,
                Action::ClosePane,
                Action::SplitRight,
                Action::SplitDown,
                Action::FontSizeIncrease,
                Action::FontSizeDecrease,
                Action::OpenSettings,
                Action::Quit,
            ]
        );
    }

    #[test]
    fn tab_and_pane_actions_follow_the_workspace() {
        let mut ws = single_pane();
        let id = ws.allocate_id();
        ws.new_tab(id, Fake(None));
        ws.split(Axis::Horizontal, geometry(), |_| Some(Fake(None)))
            .expect("split fits");
        let shown = actions(&ws, &context());
        for action in [
            Action::NextTab,
            Action::PreviousTab,
            Action::NextPane,
            Action::PreviousPane,
            Action::FocusPane(Direction::Left),
            Action::ResizePane(Direction::Left),
            Action::ToggleZoom,
            Action::EqualisePanes,
            Action::DetachPane,
        ] {
            assert!(shown.contains(&action), "{action:?} is missing");
        }
        // The active pane is the right one of a side by side split.
        for action in [
            Action::FocusPane(Direction::Right),
            Action::FocusPane(Direction::Up),
            Action::ResizePane(Direction::Down),
        ] {
            assert!(!shown.contains(&action), "{action:?} is shown");
        }
        // The right edge is the window boundary, so resizing right uses the divider
        // on the pane's opposite side.
        assert!(shown.contains(&Action::ResizePane(Direction::Right)));
        // A zoomed pane cannot be resized.
        assert!(ws.toggle_zoom());
        assert!(!actions(&ws, &context()).contains(&Action::ResizePane(Direction::Left)));
    }

    #[test]
    fn split_is_left_out_when_the_pane_is_too_small() {
        let ws = single_pane();
        let narrow = Geometry {
            area: Rect::from_size(40, 600),
            ..geometry()
        };
        assert!(!available(Action::SplitRight, &ws, narrow, &context()));
        assert!(available(Action::SplitDown, &ws, narrow, &context()));
    }

    #[test]
    fn font_size_actions_stop_at_their_limits() {
        let ws = single_pane();
        let at = |font_size| Context {
            font_size,
            ..context()
        };
        let shown = actions(&ws, &at(MAX_FONT_SIZE));
        assert!(!shown.contains(&Action::FontSizeIncrease));
        assert!(shown.contains(&Action::FontSizeDecrease));
        assert!(shown.contains(&Action::FontSizeReset));
        let shown = actions(&ws, &at(MIN_FONT_SIZE));
        assert!(shown.contains(&Action::FontSizeIncrease));
        assert!(!shown.contains(&Action::FontSizeDecrease));
    }

    #[test]
    fn pane_and_clipboard_actions_follow_the_context() {
        let ws = single_pane();
        let everything = PaneContext {
            copy: true,
            copy_command: true,
            copy_output: true,
            previous_block: true,
            next_block: true,
            toggle_fold: true,
            toggle_fold_all: true,
            scroll_page_up: true,
            scroll_page_down: true,
            scroll_to_top: true,
            scroll_to_bottom: true,
        };
        let full = Context {
            pane: Some(everything),
            paste: true,
            ..context()
        };
        let shown = actions(&ws, &full);
        for action in Action::ALL {
            let expected = !matches!(
                action,
                Action::NextTab
                    | Action::PreviousTab
                    | Action::NextPane
                    | Action::PreviousPane
                    | Action::FocusPane(_)
                    | Action::ResizePane(_)
                    | Action::ToggleZoom
                    | Action::EqualisePanes
                    | Action::DetachPane
                    | Action::FontSizeReset
            );
            assert_eq!(shown.contains(&action), expected, "{action:?}");
        }
        // Without a pane, there is nothing to paste into.
        let no_pane = Context { pane: None, ..full };
        assert!(!available(Action::Paste, &ws, geometry(), &no_pane));
    }

    #[test]
    fn pane_context_of_an_empty_session_allows_nothing() {
        let session = Session::new(1000);
        let context = PaneContext::read(&session.timeline, &session.term, None, false);
        assert_eq!(context, PaneContext::default());
        let context = PaneContext::read(&session.timeline, &session.term, None, true);
        assert_eq!(
            context,
            PaneContext {
                copy: true,
                ..PaneContext::default()
            }
        );
    }

    #[test]
    fn pane_context_follows_the_blocks_and_the_selected_one() {
        let mut session = Session::sized(20, 10, 1000);
        session.command("one", &["1"], 0);
        session.command("two", &["2"], 0);
        session.feed("$ ");
        let (timeline, term) = (&session.timeline, &session.term);
        let first = timeline.blocks()[0].id;
        let last = timeline.blocks()[1].id;

        let none = PaneContext::read(timeline, term, None, false);
        assert!(!none.copy);
        assert!(none.copy_command && none.copy_output);
        assert!(none.toggle_fold && none.toggle_fold_all);
        // Without a selected block, a navigation selects the last one.
        assert!(none.previous_block && none.next_block);

        let on_last = PaneContext::read(timeline, term, Some(last), false);
        assert!(on_last.copy);
        assert!(on_last.previous_block && !on_last.next_block);
        let on_first = PaneContext::read(timeline, term, Some(first), false);
        assert!(!on_first.previous_block && on_first.next_block);
    }

    #[test]
    fn pane_context_scrolls_only_where_there_is_room() {
        let mut session = Session::sized(20, 5, 1000);
        for name in ["a", "b", "c", "d", "e", "f"] {
            session.command(name, &["1", "2", "3"], 0);
        }
        session.feed("$ ");
        let at_rest = PaneContext::read(&session.timeline, &session.term, None, false);
        assert!(at_rest.scroll_page_up && at_rest.scroll_to_top);
        assert!(!at_rest.scroll_page_down && !at_rest.scroll_to_bottom);

        session.timeline.scroll = 2;
        let scrolled = PaneContext::read(&session.timeline, &session.term, None, false);
        assert!(scrolled.scroll_page_up && scrolled.scroll_to_top);
        assert!(scrolled.scroll_page_down && scrolled.scroll_to_bottom);

        session.timeline.scroll = usize::MAX;
        let at_top = PaneContext::read(&session.timeline, &session.term, None, false);
        assert!(!at_top.scroll_page_up && !at_top.scroll_to_top);
        assert!(at_top.scroll_page_down && at_top.scroll_to_bottom);
    }

    #[test]
    fn pane_context_pages_through_a_selected_block_cut_by_its_cap() {
        let mut session = Session::sized(20, 5, 1000);
        let output: Vec<String> = (0..20).map(|n| n.to_string()).collect();
        let output: Vec<&str> = output.iter().map(String::as_str).collect();
        session.command("long", &output, 0);
        session.feed("$ ");
        let id = session.timeline.blocks()[0].id;
        assert!(view::can_scroll(&session.timeline, &session.term, id, 5));

        let at_bottom = PaneContext::read(&session.timeline, &session.term, Some(id), false);
        assert!(at_bottom.scroll_page_up);
        assert!(!at_bottom.scroll_page_down);

        session
            .timeline
            .block_mut(id)
            .expect("finished block")
            .scroll = usize::MAX;
        let at_top = PaneContext::read(&session.timeline, &session.term, Some(id), false);
        assert!(!at_top.scroll_page_up);
        assert!(at_top.scroll_page_down);
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
        #[cfg(target_os = "macos")]
        let fallback = PathBuf::from("/home/u/Library/Application Support/graphene/palette-recent");
        #[cfg(not(target_os = "macos"))]
        let fallback = PathBuf::from("/home/u/.local/state/graphene/palette-recent");
        let path = recent_path_from(Some("relative".into()), Some("/home/u".into()));
        assert_eq!(path, Some(fallback));
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
        assert_eq!(recent[0], Action::ALL[Action::ALL.len() - 1].id());
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

    #[test]
    fn marker_follows_the_selected_row_and_scales() {
        let entries: Vec<Entry> = Action::ALL
            .into_iter()
            .map(|action| entry(&action.id(), action))
            .collect();
        let mut palette = CommandPalette::new(entries, Vec::new());
        let lay = |palette: &CommandPalette, scale: f32| {
            PaletteLayout::new(
                palette,
                Lang::En,
                Rect::from_size(1000, 1000),
                UiMetrics { scale, ..METRICS },
                measure,
            )
        };

        let layout = lay(&palette, 1.0);
        let row = layout.rows[0].rect;
        assert_eq!(
            layout.marker,
            Some(Rect {
                x: row.x,
                y: row.y,
                width: MARKER_WIDTH,
                height: row.height,
            })
        );
        assert!(
            layout.rows[0].runs[0].x >= row.x + MARKER_WIDTH,
            "the marker does not cover the label"
        );

        assert_eq!(named(&mut palette, NamedKey::ArrowDown), None);
        let layout = lay(&palette, 1.0);
        let marker = layout.marker.expect("marker");
        assert_eq!(marker.y, layout.rows[1].rect.y, "it follows the selection");

        palette.scroll_by(2);
        let layout = lay(&palette, 1.0);
        let selected = layout.rows.iter().find(|row| row.selected).expect("row");
        assert_eq!(layout.marker.expect("marker").y, selected.rect.y);

        assert_eq!(lay(&palette, 2.0).marker.expect("marker").width, 4.0);
        assert_eq!(lay(&palette, 1.5).marker.expect("marker").width, 3.0);
        assert_eq!(
            lay(&palette, 0.25).marker.expect("marker").width,
            1.0,
            "at least one pixel"
        );
    }

    #[test]
    fn marker_is_absent_without_a_match() {
        let mut palette = CommandPalette::new(vec![entry("Copy", Action::Copy)], Vec::new());
        typed(&mut palette, "zzz");
        let layout = PaletteLayout::new(
            &palette,
            Lang::En,
            Rect::from_size(1000, 1000),
            METRICS,
            measure,
        );
        assert!(layout.rows.is_empty());
        assert_eq!(layout.marker, None);
    }
}
