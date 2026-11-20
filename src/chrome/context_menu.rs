//! Context menu contents, keyboard state and geometry (pure, no GPU access).

use winit::keyboard::{Key, NamedKey};

use super::tab_bar::UiMetrics;
use crate::block_actions::Part;
use crate::i18n::{self, Lang, Msg};
use crate::input::Action;
use crate::keybindings::Keybindings;
use crate::layout::Rect;
use crate::timeline::BlockId;

const PADDING: f32 = 6.0;
const ROW_PADDING: f32 = 4.0;
const TEXT_PADDING: f32 = 12.0;
const SHORTCUT_GAP: f32 = 16.0;
const ARROW_GAP: f32 = 20.0;
const SEPARATOR_HEIGHT: f32 = 9.0;
const MAX_SUBMENU_ROWS: usize = 12;

/// A command selected from the context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Action(Action),
    CloseTab(usize),
    BlockCopy(BlockId, Part),
    BlockFold(BlockId),
    Theme(String),
}

/// A selectable line, a submenu or a section divider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Entry {
        label: String,
        shortcut: Option<String>,
        enabled: bool,
        command: Command,
    },
    Submenu {
        label: String,
        entries: Vec<Item>,
    },
    Separator,
}

/// State for the finished block under a pane-menu click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockState {
    pub id: BlockId,
    pub command_copyable: bool,
    pub output_copyable: bool,
    pub folded: bool,
}

/// Facts captured when a menu opens. Borrowed theme strings keep this context `Copy`.
#[derive(Debug, Clone, Copy)]
pub struct MenuContext<'a> {
    pub copyable: bool,
    pub last_pane: bool,
    pub single_pane_tab: bool,
    pub zoomed: bool,
    pub only_tab: bool,
    pub block: Option<BlockState>,
    pub themes: &'a [String],
    pub active_theme: &'a str,
}

fn entry(
    label: Msg,
    shortcut: Option<String>,
    enabled: bool,
    command: Command,
    lang: Lang,
) -> Item {
    Item::Entry {
        label: i18n::text(lang, &label).into_owned(),
        shortcut,
        enabled,
        command,
    }
}

fn action(
    action: Action,
    label: Msg,
    enabled: bool,
    keybindings: &Keybindings,
    lang: Lang,
) -> Item {
    entry(
        label,
        keybindings.label(action),
        enabled,
        Command::Action(action),
        lang,
    )
}

fn themes(context: &MenuContext<'_>, lang: Lang) -> Item {
    Item::Submenu {
        label: i18n::text(lang, &Msg::Themes).into_owned(),
        entries: context
            .themes
            .iter()
            .map(|theme| Item::Entry {
                label: theme.clone(),
                shortcut: None,
                enabled: true,
                command: Command::Theme(theme.clone()),
            })
            .collect(),
    }
}

fn common_tail(context: &MenuContext<'_>, lang: Lang) -> Vec<Item> {
    vec![
        Item::Separator,
        entry(
            Msg::OpenSettings,
            None,
            true,
            Command::Action(Action::OpenSettings),
            lang,
        ),
        themes(context, lang),
        entry(Msg::Quit, None, true, Command::Action(Action::Quit), lang),
    ]
}

/// Builds the menu for a pane, with block actions only for a finished block.
pub fn pane_menu(context: &MenuContext<'_>, keybindings: &Keybindings, lang: Lang) -> Vec<Item> {
    let mut items = Vec::new();
    if let Some(block) = context.block {
        items.extend([
            entry(
                Msg::CopyCommand,
                None,
                block.command_copyable,
                Command::BlockCopy(block.id, Part::Command),
                lang,
            ),
            entry(
                Msg::CopyOutput,
                None,
                block.output_copyable,
                Command::BlockCopy(block.id, Part::Output),
                lang,
            ),
        ]);
        items.push(entry(
            if block.folded {
                Msg::UnfoldBlock
            } else {
                Msg::FoldBlock
            },
            None,
            true,
            Command::BlockFold(block.id),
            lang,
        ));
        items.push(Item::Separator);
    }
    items.extend([
        action(Action::Copy, Msg::Copy, context.copyable, keybindings, lang),
        action(Action::Paste, Msg::Paste, true, keybindings, lang),
        Item::Separator,
        action(
            Action::SplitRight,
            Msg::SplitVertical,
            true,
            keybindings,
            lang,
        ),
        action(
            Action::SplitDown,
            Msg::SplitHorizontal,
            true,
            keybindings,
            lang,
        ),
        action(
            Action::ClosePane,
            Msg::ClosePane,
            !context.last_pane,
            keybindings,
            lang,
        ),
        action(
            Action::ToggleZoom,
            if context.zoomed {
                Msg::UnzoomPane
            } else {
                Msg::ZoomPane
            },
            !context.single_pane_tab,
            keybindings,
            lang,
        ),
        Item::Separator,
        action(Action::NewTab, Msg::NewTab, true, keybindings, lang),
        action(
            Action::CloseTab,
            Msg::CloseTab,
            !context.only_tab,
            keybindings,
            lang,
        ),
    ]);
    items.extend(common_tail(context, lang));
    items
}

/// Builds a menu for a tab-bar click. The close command retains the clicked index.
pub fn tab_menu(
    index: usize,
    context: &MenuContext<'_>,
    keybindings: &Keybindings,
    lang: Lang,
) -> Vec<Item> {
    let mut items = vec![
        action(Action::NewTab, Msg::NewTab, true, keybindings, lang),
        entry(
            Msg::CloseTab,
            keybindings.label(Action::CloseTab),
            !context.only_tab,
            Command::CloseTab(index),
            lang,
        ),
    ];
    items.extend(common_tail(context, lang));
    items
}

/// What a key or a selected menu row asks of the open menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Run(Command),
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuLevel {
    Main,
    Submenu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuHit {
    Entry { level: MenuLevel, index: usize },
    Separator { level: MenuLevel },
    Box,
    Outside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SubmenuState {
    parent: usize,
    selected: Option<usize>,
    scroll: usize,
}

/// An open context menu and its current keyboard / hover selection.
#[derive(Debug, Clone)]
pub struct ContextMenu {
    pub anchor: (f32, f32),
    pub items: Vec<Item>,
    pub hovered: Option<usize>,
    submenu: Option<SubmenuState>,
    active_theme: Option<String>,
    /// Whether the submenu selection was moved by the pointer or the arrows since the
    /// submenu opened, so its theme is previewed: opening it selects its first entry
    /// without previewing it.
    theme_preview: bool,
}

impl ContextMenu {
    pub fn new(anchor: (f32, f32), items: Vec<Item>) -> Self {
        Self {
            anchor,
            items,
            hovered: None,
            submenu: None,
            active_theme: None,
            theme_preview: false,
        }
    }

    pub fn with_active_theme(mut self, theme: impl Into<String>) -> Self {
        self.active_theme = Some(theme.into());
        self
    }

    pub fn from_context(anchor: (f32, f32), items: Vec<Item>, context: &MenuContext<'_>) -> Self {
        Self::new(anchor, items).with_active_theme(context.active_theme)
    }

    /// The theme of the submenu entry under the pointer or the arrows, previewed in the
    /// terminal until the menu closes.
    pub fn previewed_theme(&self) -> Option<&str> {
        if !self.theme_preview {
            return None;
        }
        let state = self.submenu?;
        match submenu_items(&self.items, state.parent).get(state.selected?)? {
            Item::Entry {
                command: Command::Theme(name),
                ..
            } => Some(name),
            _ => None,
        }
    }

    pub fn submenu_open(&self) -> bool {
        self.submenu.is_some()
    }

    pub fn submenu_scroll(&self) -> usize {
        self.submenu.map_or(0, |submenu| submenu.scroll)
    }

    /// Moves the open submenu by rows, bounded by the visible row count.
    pub fn scroll_submenu(&mut self, rows: isize, visible_rows: usize) {
        let Some(state) = &mut self.submenu else {
            return;
        };
        let count = submenu_items(&self.items, state.parent).len();
        state.scroll = state
            .scroll
            .saturating_add_signed(rows)
            .min(count.saturating_sub(visible_rows));
        if let Some(selected) = state.selected {
            if selected < state.scroll {
                state.selected = Some(state.scroll);
            } else if selected >= state.scroll + visible_rows {
                state.selected = Some(state.scroll + visible_rows.saturating_sub(1));
            }
        }
    }

    /// Handles navigation, submenu opening and selection.
    pub fn key(&mut self, key: &Key) -> Option<Outcome> {
        match key {
            Key::Named(NamedKey::Escape) => {
                if self.submenu.take().is_some() {
                    return None;
                }
                return Some(Outcome::Close);
            }
            Key::Named(NamedKey::ArrowLeft) if self.submenu.is_some() => {
                self.submenu = None;
                return None;
            }
            Key::Named(NamedKey::ArrowRight) => {
                if self.submenu.is_none() {
                    self.open_hovered_submenu();
                }
                return None;
            }
            Key::Named(NamedKey::ArrowUp) => {
                if self.submenu.is_some() {
                    self.move_submenu(-1);
                } else {
                    self.move_main(-1);
                }
            }
            Key::Named(NamedKey::ArrowDown) => {
                if self.submenu.is_some() {
                    self.move_submenu(1);
                } else {
                    self.move_main(1);
                }
            }
            Key::Named(NamedKey::Enter) => {
                if self.submenu.is_some() {
                    return self.choose_submenu();
                }
                if let Some(index) = self.hovered
                    && matches!(self.items.get(index), Some(Item::Submenu { .. }))
                {
                    self.open_submenu(index);
                    return None;
                }
                return self.choose_main();
            }
            _ => {}
        }
        None
    }

    /// Updates keyboard hover state from a layout hit.
    pub fn hover(&mut self, hit: MenuHit) {
        match hit {
            MenuHit::Entry {
                level: MenuLevel::Main,
                index,
            } => {
                self.hovered = Some(index);
                if matches!(self.items.get(index), Some(Item::Submenu { .. })) {
                    self.open_submenu(index);
                } else {
                    self.submenu = None;
                }
            }
            MenuHit::Entry {
                level: MenuLevel::Submenu,
                index,
            } => {
                if let Some(state) = &mut self.submenu {
                    state.selected = Some(index);
                    self.theme_preview = true;
                }
            }
            MenuHit::Separator {
                level: MenuLevel::Main,
            } => {
                self.hovered = None;
                self.submenu = None;
            }
            MenuHit::Outside => {
                self.hovered = None;
                self.submenu = None;
            }
            MenuHit::Box | MenuHit::Separator { .. } => {}
        }
    }

    /// Updates hover state from pointer coordinates and the current layout.
    pub fn hover_at(&mut self, layout: &MenuLayout, x: f32, y: f32) {
        self.hover(layout.hit(x, y));
    }

    /// Chooses the enabled row under a press; presses outside close without passing
    /// through, while disabled rows, separators and blank menu-box space do nothing.
    pub fn choose(&self, hit: MenuHit) -> Option<Outcome> {
        match hit {
            MenuHit::Entry {
                level: MenuLevel::Main,
                index,
            } => match self.items.get(index)? {
                Item::Entry {
                    enabled: true,
                    command,
                    ..
                } => Some(Outcome::Run(command.clone())),
                _ => None,
            },
            MenuHit::Entry {
                level: MenuLevel::Submenu,
                index,
            } => match self
                .submenu
                .and_then(|state| submenu_items(&self.items, state.parent).get(index))
            {
                Some(Item::Entry {
                    enabled: true,
                    command,
                    ..
                }) => Some(Outcome::Run(command.clone())),
                _ => None,
            },
            MenuHit::Outside => Some(Outcome::Close),
            MenuHit::Separator { .. } | MenuHit::Box => None,
        }
    }

    fn open_hovered_submenu(&mut self) {
        if let Some(index) = self.hovered
            && matches!(self.items.get(index), Some(Item::Submenu { .. }))
        {
            self.open_submenu(index);
        }
    }

    fn open_submenu(&mut self, parent: usize) {
        let entries = submenu_items(&self.items, parent);
        let selected = selectable(entries).first().copied();
        self.submenu = Some(SubmenuState {
            parent,
            selected,
            scroll: 0,
        });
        self.theme_preview = false;
    }

    fn move_main(&mut self, delta: isize) {
        let Some(index) = move_selectable(&self.items, self.hovered, delta) else {
            return;
        };
        self.hovered = Some(index);
        self.submenu = None;
    }

    fn move_submenu(&mut self, delta: isize) {
        let Some(state) = &mut self.submenu else {
            return;
        };
        let entries = submenu_items(&self.items, state.parent);
        state.selected = move_selectable(entries, state.selected, delta);
        self.theme_preview = true;
        if let Some(selected) = state.selected {
            if selected < state.scroll {
                state.scroll = selected;
            } else if selected >= state.scroll + MAX_SUBMENU_ROWS {
                state.scroll = selected + 1 - MAX_SUBMENU_ROWS;
            }
        }
    }

    fn choose_main(&self) -> Option<Outcome> {
        self.hovered
            .and_then(|index| self.items.get(index))
            .and_then(|item| match item {
                Item::Entry {
                    enabled: true,
                    command,
                    ..
                } => Some(Outcome::Run(command.clone())),
                _ => None,
            })
    }

    fn choose_submenu(&self) -> Option<Outcome> {
        let state = self.submenu?;
        let item = submenu_items(&self.items, state.parent).get(state.selected?)?;
        match item {
            Item::Entry {
                enabled: true,
                command,
                ..
            } => Some(Outcome::Run(command.clone())),
            _ => None,
        }
    }
}

fn submenu_items(items: &[Item], parent: usize) -> &[Item] {
    match items.get(parent) {
        Some(Item::Submenu { entries, .. }) => entries,
        _ => &[],
    }
}

fn selectable(items: &[Item]) -> Vec<usize> {
    items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            Item::Entry { enabled: true, .. } => Some(index),
            Item::Submenu { entries, .. }
                if entries
                    .iter()
                    .any(|entry| matches!(entry, Item::Entry { enabled: true, .. })) =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect()
}

fn move_selectable(items: &[Item], selected: Option<usize>, delta: isize) -> Option<usize> {
    let options = selectable(items);
    if options.is_empty() {
        return None;
    }
    let position = selected
        .and_then(|current| options.iter().position(|&index| index == current))
        .unwrap_or(if delta < 0 { 0 } else { options.len() - 1 });
    let next = (position as isize + delta).rem_euclid(options.len() as isize) as usize;
    Some(options[next])
}

/// A visible item row ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuRow {
    pub rect: Rect,
    pub index: usize,
    pub label: Option<String>,
    pub text_x: f32,
    pub text_y: f32,
    pub max_width: f32,
    pub shortcut: Option<(String, f32)>,
    pub separator: bool,
    pub enabled: bool,
    pub hovered: bool,
    pub active_theme: bool,
    pub submenu_arrow: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubmenuLayout {
    pub rect: Rect,
    pub rows: Vec<MenuRow>,
    pub first_index: usize,
    pub visible_rows: usize,
}

/// A context menu placed in the window, including an open submenu.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuLayout {
    pub rect: Rect,
    pub rows: Vec<MenuRow>,
    pub submenu: Option<SubmenuLayout>,
}

impl MenuLayout {
    /// Places the menu at its anchor, flipping and clamping it inside `window`.
    pub fn new(
        menu: &ContextMenu,
        window: Rect,
        metrics: UiMetrics,
        mut measure: impl FnMut(&str) -> f32,
    ) -> Self {
        let line = metrics.line_height.max(1.0);
        let scale = metrics.scale;
        let pad = PADDING * scale;
        let row_height = (line + 2.0 * ROW_PADDING * scale).round().max(1.0);
        let width = menu_width(&menu.items, &mut measure, scale)
            .min(window.width)
            .round();
        let height = (menu_height(&menu.items, row_height, scale) + 2.0 * pad)
            .min(window.height)
            .round();
        let mut x = menu.anchor.0;
        let mut y = menu.anchor.1;
        if x + width > window.x + window.width {
            x = menu.anchor.0 - width;
        }
        if y + height > window.y + window.height {
            y = menu.anchor.1 - height;
        }
        x = x.clamp(window.x, (window.x + window.width - width).max(window.x));
        y = y.clamp(window.y, (window.y + window.height - height).max(window.y));
        let rect = Rect {
            x,
            y,
            width,
            height,
        };
        let rows = layout_rows(
            &menu.items,
            rect,
            pad,
            row_height,
            scale,
            &mut measure,
            menu.hovered,
            None,
            menu.active_theme.as_deref(),
        );
        let submenu = menu.submenu.and_then(|state| {
            let item = menu.items.get(state.parent)?;
            let Item::Submenu { entries, .. } = item else {
                return None;
            };
            let parent_row = rows.iter().find(|row| row.index == state.parent)?;
            let sub_width = menu_width(entries, &mut measure, scale)
                .min(window.width)
                .round();
            let max_height = window
                .height
                .min(menu_height(entries, row_height, scale) + 2.0 * pad);
            let visible_rows = (((max_height - 2.0 * pad).max(0.0) / row_height).floor() as usize)
                .min(MAX_SUBMENU_ROWS);
            let first = state.scroll.min(entries.len().saturating_sub(visible_rows));
            let shown = entries
                .iter()
                .skip(first)
                .take(visible_rows)
                .cloned()
                .collect::<Vec<_>>();
            let height = (menu_height(&shown, row_height, scale) + 2.0 * pad).min(window.height);
            let mut sub_x = rect.x + rect.width;
            if sub_x + sub_width > window.x + window.width {
                sub_x = rect.x - sub_width;
            }
            sub_x = sub_x.clamp(
                window.x,
                (window.x + window.width - sub_width).max(window.x),
            );
            let sub_y = (parent_row.rect.y - pad)
                .clamp(window.y, (window.y + window.height - height).max(window.y));
            let sub_rect = Rect {
                x: sub_x,
                y: sub_y,
                width: sub_width,
                height,
            };
            let sub_rows = layout_rows(
                &shown,
                sub_rect,
                pad,
                row_height,
                scale,
                &mut measure,
                state.selected.map(|index| index.saturating_sub(first)),
                Some((first, state.selected)),
                menu.active_theme.as_deref(),
            );
            Some(SubmenuLayout {
                rect: sub_rect,
                rows: sub_rows,
                first_index: first,
                visible_rows,
            })
        });
        Self {
            rect,
            rows,
            submenu,
        }
    }

    pub fn hit(&self, x: f32, y: f32) -> MenuHit {
        if let Some(submenu) = &self.submenu
            && submenu.rect.contains(x, y)
        {
            if let Some(row) = submenu.rows.iter().find(|row| row.rect.contains(x, y)) {
                return if row.separator {
                    MenuHit::Separator {
                        level: MenuLevel::Submenu,
                    }
                } else {
                    MenuHit::Entry {
                        level: MenuLevel::Submenu,
                        index: row.index,
                    }
                };
            }
            return MenuHit::Box;
        }
        if self.rect.contains(x, y) {
            if let Some(row) = self.rows.iter().find(|row| row.rect.contains(x, y)) {
                return if row.separator {
                    MenuHit::Separator {
                        level: MenuLevel::Main,
                    }
                } else {
                    MenuHit::Entry {
                        level: MenuLevel::Main,
                        index: row.index,
                    }
                };
            }
            MenuHit::Box
        } else {
            MenuHit::Outside
        }
    }
}

fn menu_width(items: &[Item], measure: &mut impl FnMut(&str) -> f32, scale: f32) -> f32 {
    let content = items
        .iter()
        .map(|item| match item {
            Item::Entry {
                label, shortcut, ..
            } => {
                measure(label)
                    + shortcut
                        .as_ref()
                        .map_or(0.0, |s| SHORTCUT_GAP * scale + measure(s))
            }
            Item::Submenu { label, .. } => measure(label) + ARROW_GAP * scale,
            Item::Separator => 0.0,
        })
        .fold(0.0, f32::max);
    content + 2.0 * TEXT_PADDING * scale
}

fn menu_height(items: &[Item], row_height: f32, scale: f32) -> f32 {
    items
        .iter()
        .map(|item| match item {
            Item::Separator => SEPARATOR_HEIGHT * scale,
            _ => row_height,
        })
        .sum()
}

#[allow(clippy::too_many_arguments)]
fn layout_rows(
    items: &[Item],
    rect: Rect,
    pad: f32,
    row_height: f32,
    scale: f32,
    measure: &mut impl FnMut(&str) -> f32,
    selected: Option<usize>,
    submenu_indices: Option<(usize, Option<usize>)>,
    active_theme: Option<&str>,
) -> Vec<MenuRow> {
    let mut y = rect.y + pad;
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let actual_index = submenu_indices.map_or(index, |(first, _)| first + index);
            let hovered = submenu_indices.map_or(selected == Some(index), |(_, selected)| {
                selected == Some(actual_index)
            });
            let height = if matches!(item, Item::Separator) {
                SEPARATOR_HEIGHT * scale
            } else {
                row_height
            };
            let row_rect = Rect {
                x: rect.x + 1.0,
                y,
                width: (rect.width - 2.0).max(0.0),
                height,
            };
            let (label, shortcut, enabled, arrow) = match item {
                Item::Entry {
                    label,
                    shortcut,
                    enabled,
                    ..
                } => (Some(label.clone()), shortcut.clone(), *enabled, false),
                Item::Submenu { label, .. } => (Some(label.clone()), None, true, true),
                Item::Separator => (None, None, false, false),
            };
            let text_x = rect.x + TEXT_PADDING * scale;
            let text_y = y
                + (height - row_height.min(height)) / 2.0
                + ((row_height - (row_height - 2.0 * ROW_PADDING * scale).max(1.0)) / 2.0);
            let shortcut_position = shortcut.as_ref().map(|text| {
                (
                    text.clone(),
                    (rect.x + rect.width - TEXT_PADDING * scale - measure(text)).max(text_x),
                )
            });
            let max_width = shortcut_position.as_ref().map_or(
                rect.x + rect.width - TEXT_PADDING * scale - text_x,
                |(_, x)| (x - SHORTCUT_GAP * scale - text_x).max(0.0),
            );
            let row = MenuRow {
                rect: row_rect,
                index: actual_index,
                label,
                text_x,
                text_y,
                max_width,
                shortcut: shortcut_position,
                separator: matches!(item, Item::Separator),
                enabled,
                hovered,
                active_theme: match item {
                    Item::Entry {
                        command: Command::Theme(name),
                        ..
                    } => active_theme == Some(name.as_str()),
                    _ => false,
                },
                submenu_arrow: arrow,
            };
            y += height;
            row
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn context() -> MenuContext<'static> {
        MenuContext {
            copyable: true,
            last_pane: false,
            single_pane_tab: false,
            zoomed: false,
            only_tab: false,
            block: None,
            themes: Box::leak(vec!["Dark".into(), "Light".into()].into_boxed_slice()),
            active_theme: "Dark",
        }
    }

    fn entry_at<'a>(items: &'a [Item], label: &str) -> &'a Item {
        items
            .iter()
            .find(|item| match item {
                Item::Entry { label: found, .. } | Item::Submenu { label: found, .. } => {
                    found == label
                }
                Item::Separator => false,
            })
            .expect("menu item")
    }

    fn width(text: &str) -> f32 {
        text.chars().count() as f32 * 8.0
    }

    const METRICS: UiMetrics = UiMetrics {
        line_height: 20.0,
        scale: 1.0,
    };

    #[test]
    fn pane_menu_has_context_sensitive_block_zoom_and_close_entries() {
        let keys = Keybindings::default();
        let mut ctx = context();
        let normal = pane_menu(&ctx, &keys, Lang::En);
        assert!(!normal.iter().any(|item| matches!(
            item,
            Item::Entry {
                command: Command::BlockCopy(..),
                ..
            }
        )));
        assert!(matches!(
            entry_at(&normal, "Zoom the pane"),
            Item::Entry { enabled: true, .. }
        ));

        ctx.block = Some(BlockState {
            id: BlockId(9),
            command_copyable: false,
            output_copyable: true,
            folded: true,
        });
        ctx.last_pane = true;
        ctx.single_pane_tab = true;
        ctx.zoomed = true;
        ctx.only_tab = true;
        let items = pane_menu(&ctx, &keys, Lang::En);
        assert!(matches!(
            entry_at(&items, "Copy block command"),
            Item::Entry { enabled: false, .. }
        ));
        assert!(matches!(
            entry_at(&items, "Copy block output"),
            Item::Entry { enabled: true, .. }
        ));
        assert!(matches!(
            entry_at(&items, "Unfold block"),
            Item::Entry { enabled: true, .. }
        ));
        assert!(matches!(
            entry_at(&items, "Close pane"),
            Item::Entry { enabled: false, .. }
        ));
        assert!(matches!(
            entry_at(&items, "Unzoom the pane"),
            Item::Entry { enabled: false, .. }
        ));
        assert!(matches!(
            entry_at(&items, "Close the tab"),
            Item::Entry { enabled: false, .. }
        ));
        assert!(matches!(
            entry_at(&items, "Paste"),
            Item::Entry { enabled: true, .. }
        ));
    }

    #[test]
    fn tab_menu_keeps_clicked_index_and_respects_only_tab() {
        let mut ctx = context();
        ctx.only_tab = true;
        let items = tab_menu(4, &ctx, &Keybindings::default(), Lang::En);
        assert!(matches!(
            entry_at(&items, "Close the tab"),
            Item::Entry {
                enabled: false,
                command: Command::CloseTab(4),
                ..
            }
        ));
    }

    #[test]
    fn shortcut_labels_follow_rebinding() {
        let keys = Keybindings::new(&BTreeMap::from([("Ctrl+Shift+C".into(), "copy".into())]));
        let items = pane_menu(&context(), &keys, Lang::En);
        assert!(matches!(entry_at(&items, "Copy"), Item::Entry {
            shortcut: Some(shortcut),
            ..
        } if shortcut == "Ctrl+Shift+C"));
    }

    #[test]
    fn keyboard_navigation_skips_disabled_entries_and_separators() {
        let items = vec![
            Item::Entry {
                label: "disabled".into(),
                shortcut: None,
                enabled: false,
                command: Command::Action(Action::Copy),
            },
            Item::Separator,
            Item::Entry {
                label: "enabled".into(),
                shortcut: None,
                enabled: true,
                command: Command::Action(Action::Paste),
            },
        ];
        let mut menu = ContextMenu::new((0.0, 0.0), items);
        assert_eq!(menu.key(&Key::Named(NamedKey::ArrowDown)), None);
        assert_eq!(
            menu.key(&Key::Named(NamedKey::Enter)),
            Some(Outcome::Run(Command::Action(Action::Paste)))
        );
    }

    #[test]
    fn keyboard_opens_and_closes_submenus_without_entering_them_on_navigation() {
        let mut menu = ContextMenu::new(
            (0.0, 0.0),
            vec![
                Item::Entry {
                    label: "Copy".into(),
                    shortcut: None,
                    enabled: true,
                    command: Command::Action(Action::Copy),
                },
                Item::Submenu {
                    label: "Themes".into(),
                    entries: vec![Item::Entry {
                        label: "Night".into(),
                        shortcut: None,
                        enabled: true,
                        command: Command::Theme("Night".into()),
                    }],
                },
            ],
        );
        assert_eq!(menu.key(&Key::Named(NamedKey::ArrowDown)), None);
        assert_eq!(menu.hovered, Some(0));
        assert!(!menu.submenu_open());
        assert_eq!(menu.key(&Key::Named(NamedKey::ArrowDown)), None);
        assert_eq!(menu.hovered, Some(1));
        assert!(!menu.submenu_open());
        assert_eq!(menu.key(&Key::Named(NamedKey::ArrowRight)), None);
        assert!(menu.submenu_open());
        assert_eq!(
            menu.key(&Key::Named(NamedKey::Escape)),
            None,
            "first Escape closes only the submenu"
        );
        assert!(!menu.submenu_open());
        assert_eq!(
            menu.key(&Key::Named(NamedKey::Escape)),
            Some(Outcome::Close)
        );
    }

    #[test]
    fn menu_flips_and_clamps_at_window_edges_and_hit_tests() {
        let menu = ContextMenu::new(
            (98.0, 98.0),
            vec![Item::Entry {
                label: "Copy".into(),
                shortcut: None,
                enabled: true,
                command: Command::Action(Action::Copy),
            }],
        );
        let layout = MenuLayout::new(&menu, Rect::from_size(100, 100), METRICS, width);
        assert!(layout.rect.x + layout.rect.width <= 100.0);
        assert!(layout.rect.y + layout.rect.height <= 100.0);
        assert!(layout.rect.x < menu.anchor.0, "menu flips left");
        assert!(layout.rect.y < menu.anchor.1, "menu flips up");
        let row = &layout.rows[0];
        assert_eq!(
            layout.hit(row.rect.x + 1.0, row.rect.y + 1.0),
            MenuHit::Entry {
                level: MenuLevel::Main,
                index: 0
            }
        );
        assert_eq!(layout.hit(-1.0, -1.0), MenuHit::Outside);
    }

    #[test]
    fn submenu_opens_left_when_needed_and_scrolls() {
        let entries = (0..20)
            .map(|number| Item::Entry {
                label: format!("Theme {number}"),
                shortcut: None,
                enabled: true,
                command: Command::Theme(format!("Theme {number}")),
            })
            .collect();
        let mut menu = ContextMenu::new(
            (145.0, 20.0),
            vec![Item::Submenu {
                label: "Themes".into(),
                entries,
            }],
        );
        menu.hovered = Some(0);
        menu.open_hovered_submenu();
        let layout = MenuLayout::new(&menu, Rect::from_size(150, 100), METRICS, width);
        let submenu = layout.submenu.as_ref().expect("submenu");
        assert!(submenu.rect.x < layout.rect.x);
        assert!(submenu.rect.height <= 100.0);
        assert!(submenu.visible_rows < 20);
        let first_row = &submenu.rows[0];
        assert_eq!(
            layout.hit(first_row.rect.x + 1.0, first_row.rect.y + 1.0),
            MenuHit::Entry {
                level: MenuLevel::Submenu,
                index: 0
            }
        );
        menu.scroll_submenu(2, submenu.visible_rows);
        assert_eq!(menu.submenu_scroll(), 2);
        let scrolled = MenuLayout::new(&menu, Rect::from_size(150, 100), METRICS, width);
        assert_eq!(scrolled.submenu.unwrap().first_index, 2);
    }

    fn theme_menu() -> ContextMenu {
        let theme = |name: &str| Item::Entry {
            label: name.into(),
            shortcut: None,
            enabled: true,
            command: Command::Theme(name.into()),
        };
        ContextMenu::new(
            (0.0, 0.0),
            vec![
                entry(
                    Msg::Quit,
                    None,
                    true,
                    Command::Action(Action::Quit),
                    Lang::En,
                ),
                Item::Submenu {
                    label: "Themes".into(),
                    entries: vec![theme("Dark"), theme("Light")],
                },
            ],
        )
    }

    const THEMES_ROW: MenuHit = MenuHit::Entry {
        level: MenuLevel::Main,
        index: 1,
    };

    fn theme_row(index: usize) -> MenuHit {
        MenuHit::Entry {
            level: MenuLevel::Submenu,
            index,
        }
    }

    #[test]
    fn the_theme_under_the_pointer_is_previewed() {
        let mut menu = theme_menu();
        // Opening the submenu selects its first theme without previewing it.
        menu.hover(THEMES_ROW);
        assert!(menu.submenu_open());
        assert_eq!(menu.previewed_theme(), None);

        menu.hover(theme_row(1));
        assert_eq!(menu.previewed_theme(), Some("Light"));
        menu.hover(theme_row(0));
        assert_eq!(menu.previewed_theme(), Some("Dark"));
        // Blank space of the submenu keeps the last theme.
        menu.hover(MenuHit::Box);
        assert_eq!(menu.previewed_theme(), Some("Dark"));

        // Back on the main menu, or outside it: no preview.
        menu.hover(THEMES_ROW);
        assert_eq!(menu.previewed_theme(), None);
        menu.hover(theme_row(1));
        menu.hover(MenuHit::Entry {
            level: MenuLevel::Main,
            index: 0,
        });
        assert_eq!(menu.previewed_theme(), None);
        menu.hover(theme_row(1));
        menu.hover(MenuHit::Outside);
        assert_eq!(menu.previewed_theme(), None);
    }

    #[test]
    fn the_theme_reached_with_the_arrows_is_previewed() {
        let mut menu = theme_menu();
        menu.key(&Key::Named(NamedKey::ArrowDown));
        menu.key(&Key::Named(NamedKey::ArrowDown));
        menu.key(&Key::Named(NamedKey::ArrowRight));
        assert!(menu.submenu_open());
        assert_eq!(menu.previewed_theme(), None);
        menu.key(&Key::Named(NamedKey::ArrowDown));
        assert_eq!(menu.previewed_theme(), Some("Light"));
        // Escape closes the submenu and ends the preview.
        menu.key(&Key::Named(NamedKey::Escape));
        assert_eq!(menu.previewed_theme(), None);
    }

    #[test]
    fn active_theme_is_marked_and_hits_can_be_chosen() {
        let context = context();
        let menu = ContextMenu::from_context(
            (0.0, 0.0),
            vec![Item::Submenu {
                label: "Themes".into(),
                entries: vec![Item::Entry {
                    label: "Dark".into(),
                    shortcut: None,
                    enabled: true,
                    command: Command::Theme("Dark".into()),
                }],
            }],
            &context,
        );
        let mut menu = menu;
        menu.hover(MenuHit::Entry {
            level: MenuLevel::Main,
            index: 0,
        });
        assert!(menu.submenu_open());
        let layout = MenuLayout::new(&menu, Rect::from_size(400, 200), METRICS, width);
        let submenu = layout.submenu.as_ref().expect("submenu");
        assert!(submenu.rows[0].active_theme);
        assert_eq!(
            menu.choose(MenuHit::Entry {
                level: MenuLevel::Submenu,
                index: 0,
            }),
            Some(Outcome::Run(Command::Theme("Dark".into())))
        );
    }
}
