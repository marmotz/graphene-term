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

/// A command selected from the context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Action(Action),
    CloseTab(usize),
    /// Opens the box renaming the tab of this index.
    RenameTab(usize),
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
    /// Rows a theme menu holds before "More...": as many as the window height allows.
    pub theme_rows: usize,
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
        entries: theme_pages(context.themes, context.theme_rows, lang),
    }
}

/// The entries of `themes`, `rows` per menu at most: when they do not all fit, the last
/// row is a "More..." submenu holding the next ones, and so on.
fn theme_pages(themes: &[String], rows: usize, lang: Lang) -> Vec<Item> {
    let entry = |theme: &String| Item::Entry {
        label: theme.clone(),
        shortcut: None,
        enabled: true,
        command: Command::Theme(theme.clone()),
    };
    let rows = rows.max(2);
    if themes.len() <= rows {
        return themes.iter().map(entry).collect();
    }
    let (page, rest) = themes.split_at(rows - 1);
    let mut items = page.iter().map(entry).collect::<Vec<_>>();
    items.push(Item::Submenu {
        label: i18n::text(lang, &Msg::MoreThemes).into_owned(),
        entries: theme_pages(rest, rows, lang),
    });
    items
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
        entry(
            Msg::RenameTab,
            keybindings.label(Action::RenameTab),
            true,
            Command::RenameTab(index),
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

/// Depth of a menu: 0 for the main menu, 1 for one of its submenus, 2 for a submenu of
/// that one, and so on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuLevel(pub usize);

impl MenuLevel {
    pub const MAIN: Self = Self(0);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuHit {
    Entry { level: MenuLevel, index: usize },
    Separator { level: MenuLevel },
    Box,
    Outside,
}

/// An open submenu: the row of the menu above that opened it, and its selected row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SubmenuState {
    parent: usize,
    selected: Option<usize>,
}

/// An open context menu and its current keyboard / hover selection.
#[derive(Debug, Clone)]
pub struct ContextMenu {
    pub anchor: (f32, f32),
    pub items: Vec<Item>,
    pub hovered: Option<usize>,
    /// The open submenus, outermost first.
    submenus: Vec<SubmenuState>,
    active_theme: Option<String>,
    /// The level whose selection the pointer or the arrows moved last, so its theme is
    /// previewed: opening a submenu selects its first entry without previewing it.
    preview_level: Option<usize>,
}

impl ContextMenu {
    pub fn new(anchor: (f32, f32), items: Vec<Item>) -> Self {
        Self {
            anchor,
            items,
            hovered: None,
            submenus: Vec::new(),
            active_theme: None,
            preview_level: None,
        }
    }

    pub fn with_active_theme(mut self, theme: impl Into<String>) -> Self {
        self.active_theme = Some(theme.into());
        self
    }

    pub fn from_context(anchor: (f32, f32), items: Vec<Item>, context: &MenuContext<'_>) -> Self {
        Self::new(anchor, items).with_active_theme(context.active_theme)
    }

    /// The theme of the entry under the pointer or the arrows, previewed in the terminal
    /// until the menu closes.
    pub fn previewed_theme(&self) -> Option<&str> {
        let level = self.preview_level.filter(|&level| level <= self.depth())?;
        match self.level_items(level).get(self.selected(level)?)? {
            Item::Entry {
                command: Command::Theme(name),
                ..
            } => Some(name),
            _ => None,
        }
    }

    pub fn submenu_open(&self) -> bool {
        !self.submenus.is_empty()
    }

    /// Number of open submenus.
    pub fn depth(&self) -> usize {
        self.submenus.len()
    }

    /// Handles navigation, submenu opening and selection, in the innermost open menu.
    pub fn key(&mut self, key: &Key) -> Option<Outcome> {
        match key {
            Key::Named(NamedKey::Escape) => {
                if self.close_submenu() {
                    return None;
                }
                return Some(Outcome::Close);
            }
            Key::Named(NamedKey::ArrowLeft) => {
                self.close_submenu();
            }
            Key::Named(NamedKey::ArrowRight) => self.open_selected_submenu(),
            Key::Named(NamedKey::ArrowUp) => self.move_selection(-1),
            Key::Named(NamedKey::ArrowDown) => self.move_selection(1),
            Key::Named(NamedKey::Enter) => {
                let level = self.depth();
                if let Some(index) = self.selected(level)
                    && matches!(
                        self.level_items(level).get(index),
                        Some(Item::Submenu { .. })
                    )
                {
                    self.open_submenu(index);
                    return None;
                }
                return self.choose_selected();
            }
            _ => {}
        }
        None
    }

    /// Updates the hover state from a layout hit: selects the row, closes the submenus
    /// below its menu other than the one it opened, and opens its submenu.
    pub fn hover(&mut self, hit: MenuHit) {
        match hit {
            MenuHit::Entry {
                level: MenuLevel(level),
                index,
            } if level <= self.depth() => {
                let keeps_child = self
                    .submenus
                    .get(level)
                    .is_some_and(|state| state.parent == index);
                if !keeps_child {
                    self.submenus.truncate(level);
                }
                self.set_selected(level, Some(index));
                self.preview_level = Some(level);
                if !keeps_child
                    && matches!(
                        self.level_items(level).get(index),
                        Some(Item::Submenu { .. })
                    )
                {
                    self.open_submenu(index);
                }
            }
            MenuHit::Separator {
                level: MenuLevel(level),
            } if level <= self.depth() => {
                self.submenus.truncate(level);
                self.set_selected(level, None);
                self.preview_level = Some(level);
            }
            MenuHit::Outside => {
                self.hovered = None;
                self.submenus.clear();
                self.preview_level = None;
            }
            MenuHit::Entry { .. } | MenuHit::Separator { .. } | MenuHit::Box => {}
        }
    }

    /// Updates hover state from pointer coordinates and the current layout.
    pub fn hover_at(&mut self, layout: &MenuLayout, x: f32, y: f32) {
        self.hover(layout.hit(x, y));
    }

    /// Chooses the enabled row under a press; presses outside close without passing
    /// through, while disabled rows, submenu rows, separators and blank menu-box space do
    /// nothing.
    pub fn choose(&self, hit: MenuHit) -> Option<Outcome> {
        match hit {
            MenuHit::Entry {
                level: MenuLevel(level),
                index,
            } if level <= self.depth() => run(self.level_items(level).get(index)?),
            MenuHit::Outside => Some(Outcome::Close),
            MenuHit::Entry { .. } | MenuHit::Separator { .. } | MenuHit::Box => None,
        }
    }

    /// The items of the menu at `level`.
    fn level_items(&self, level: usize) -> &[Item] {
        let mut items: &[Item] = &self.items;
        for state in self.submenus.iter().take(level) {
            items = submenu_items(items, state.parent);
        }
        items
    }

    fn selected(&self, level: usize) -> Option<usize> {
        match level {
            0 => self.hovered,
            _ => self.submenus.get(level - 1)?.selected,
        }
    }

    fn set_selected(&mut self, level: usize, index: Option<usize>) {
        match level {
            0 => self.hovered = index,
            _ => {
                if let Some(state) = self.submenus.get_mut(level - 1) {
                    state.selected = index;
                }
            }
        }
    }

    /// Closes the innermost submenu; returns whether one was open.
    fn close_submenu(&mut self) -> bool {
        let closed = self.submenus.pop().is_some();
        if closed {
            self.preview_level = Some(self.depth());
        }
        closed
    }

    fn open_selected_submenu(&mut self) {
        let level = self.depth();
        if let Some(index) = self.selected(level)
            && matches!(
                self.level_items(level).get(index),
                Some(Item::Submenu { .. })
            )
        {
            self.open_submenu(index);
        }
    }

    /// Opens the submenu of row `parent` of the innermost menu, its first entry selected.
    fn open_submenu(&mut self, parent: usize) {
        let entries = submenu_items(self.level_items(self.depth()), parent);
        let selected = selectable(entries).first().copied();
        self.submenus.push(SubmenuState { parent, selected });
    }

    fn move_selection(&mut self, delta: isize) {
        let level = self.depth();
        let Some(index) = move_selectable(self.level_items(level), self.selected(level), delta)
        else {
            return;
        };
        self.set_selected(level, Some(index));
        self.preview_level = Some(level);
    }

    fn choose_selected(&self) -> Option<Outcome> {
        let level = self.depth();
        run(self.level_items(level).get(self.selected(level)?)?)
    }
}

/// The command of an enabled entry.
fn run(item: &Item) -> Option<Outcome> {
    match item {
        Item::Entry {
            enabled: true,
            command,
            ..
        } => Some(Outcome::Run(command.clone())),
        _ => None,
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

/// An open submenu placed next to the row that opened it.
#[derive(Debug, Clone, PartialEq)]
pub struct SubmenuLayout {
    pub rect: Rect,
    pub rows: Vec<MenuRow>,
}

/// A context menu placed in the window, with its open submenus, outermost first.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuLayout {
    pub rect: Rect,
    pub rows: Vec<MenuRow>,
    pub submenus: Vec<SubmenuLayout>,
}

/// Height of a menu row, in pixels.
fn row_height(metrics: UiMetrics) -> f32 {
    (metrics.line_height.max(1.0) + 2.0 * ROW_PADDING * metrics.scale)
        .round()
        .max(1.0)
}

/// How many rows a menu `height` pixels tall holds: the size of a page of themes. At
/// least 2, a theme and "More...".
pub fn rows_fitting(height: f32, metrics: UiMetrics) -> usize {
    let pad = PADDING * metrics.scale;
    (((height - 2.0 * pad).max(0.0) / row_height(metrics)).floor() as usize).max(2)
}

impl MenuLayout {
    /// Places the menu at its anchor, flipping and clamping it inside `window`, then each
    /// open submenu beside the row that opened it.
    pub fn new(
        menu: &ContextMenu,
        window: Rect,
        metrics: UiMetrics,
        mut measure: impl FnMut(&str) -> f32,
    ) -> Self {
        let scale = metrics.scale;
        let pad = PADDING * scale;
        let row_height = row_height(metrics);
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
        let active_theme = menu.active_theme.as_deref();
        let rows = layout_rows(
            &menu.items,
            rect,
            pad,
            row_height,
            scale,
            &mut measure,
            menu.hovered,
            active_theme,
        );
        let mut submenus: Vec<SubmenuLayout> = Vec::new();
        for (level, state) in menu.submenus.iter().enumerate() {
            let entries = submenu_items(menu.level_items(level), state.parent);
            let (parent_rect, parent_rows) = match submenus.last() {
                Some(parent) => (parent.rect, &parent.rows),
                None => (rect, &rows),
            };
            let Some(parent_row) = parent_rows.iter().find(|row| row.index == state.parent) else {
                break;
            };
            let parent_y = parent_row.rect.y;
            let sub_width = menu_width(entries, &mut measure, scale)
                .min(window.width)
                .round();
            let sub_height =
                (menu_height(entries, row_height, scale) + 2.0 * pad).min(window.height);
            let mut sub_x = parent_rect.x + parent_rect.width;
            if sub_x + sub_width > window.x + window.width {
                sub_x = parent_rect.x - sub_width;
            }
            sub_x = sub_x.clamp(
                window.x,
                (window.x + window.width - sub_width).max(window.x),
            );
            let sub_y = (parent_y - pad).clamp(
                window.y,
                (window.y + window.height - sub_height).max(window.y),
            );
            let sub_rect = Rect {
                x: sub_x,
                y: sub_y,
                width: sub_width,
                height: sub_height,
            };
            let mut sub_rows = layout_rows(
                entries,
                sub_rect,
                pad,
                row_height,
                scale,
                &mut measure,
                state.selected,
                active_theme,
            );
            // Rows past a window smaller than the page they were built for are not drawn.
            let bottom = sub_rect.y + sub_rect.height - pad + 0.5;
            sub_rows.retain(|row| row.rect.y + row.rect.height <= bottom);
            submenus.push(SubmenuLayout {
                rect: sub_rect,
                rows: sub_rows,
            });
        }
        Self {
            rect,
            rows,
            submenus,
        }
    }

    /// What lies under `(x, y)`: the innermost menu wins where menus overlap.
    pub fn hit(&self, x: f32, y: f32) -> MenuHit {
        let menus = std::iter::once((self.rect, &self.rows))
            .chain(
                self.submenus
                    .iter()
                    .map(|submenu| (submenu.rect, &submenu.rows)),
            )
            .enumerate()
            .collect::<Vec<_>>();
        for (level, (rect, rows)) in menus.into_iter().rev() {
            if !rect.contains(x, y) {
                continue;
            }
            let level = MenuLevel(level);
            return match rows.iter().find(|row| row.rect.contains(x, y)) {
                Some(row) if row.separator => MenuHit::Separator { level },
                Some(row) => MenuHit::Entry {
                    level,
                    index: row.index,
                },
                None => MenuHit::Box,
            };
        }
        MenuHit::Outside
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
    active_theme: Option<&str>,
) -> Vec<MenuRow> {
    let mut y = rect.y + pad;
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let hovered = selected == Some(index);
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
                index,
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
            theme_rows: 30,
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
        assert!(matches!(
            entry_at(&items, "Rename the tab"),
            Item::Entry {
                enabled: true,
                shortcut: None,
                command: Command::RenameTab(4),
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
                level: MenuLevel::MAIN,
                index: 0
            }
        );
        assert_eq!(layout.hit(-1.0, -1.0), MenuHit::Outside);
    }

    fn themes_named(count: usize) -> Vec<String> {
        (0..count)
            .map(|number| format!("theme-{number:03}"))
            .collect()
    }

    /// The theme names of a page, then the page under its "More..." row.
    fn page(items: &[Item]) -> (Vec<&str>, Option<&[Item]>) {
        let mut names = Vec::new();
        let mut more = None;
        for item in items {
            match item {
                Item::Entry {
                    command: Command::Theme(name),
                    ..
                } => names.push(name.as_str()),
                Item::Submenu { label, entries } => {
                    assert_eq!(label, "More...");
                    more = Some(entries.as_slice());
                }
                _ => panic!("unexpected row {item:?}"),
            }
        }
        (names, more)
    }

    #[test]
    fn themes_that_fit_are_listed_without_more() {
        let themes = themes_named(5);
        let items = theme_pages(&themes, 5, Lang::En);
        let (names, more) = page(&items);
        assert_eq!(names.len(), 5);
        assert!(more.is_none());
    }

    #[test]
    fn themes_that_do_not_fit_continue_under_more() {
        let themes = themes_named(100);
        let items = theme_pages(&themes, 30, Lang::En);
        let mut all = Vec::new();
        let mut pages = Vec::new();
        let mut current = Some(items.as_slice());
        while let Some(items) = current {
            assert!(items.len() <= 30, "a page holds 30 rows at most");
            let (names, more) = page(items);
            pages.push(names.len());
            all.extend(names);
            current = more;
        }
        // 29 themes and "More..." per full page, the rest on the last one.
        assert_eq!(pages, [29, 29, 29, 13]);
        let expected = themes.iter().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(all, expected);
        assert_eq!(
            theme_pages(&themes, 30, Lang::Fr)
                .last()
                .and_then(|item| match item {
                    Item::Submenu { label, .. } => Some(label.as_str()),
                    _ => None,
                }),
            Some("Plus...")
        );
    }

    #[test]
    fn a_page_holds_as_many_rows_as_the_window_height() {
        // Rows of 20 + 2 * 4 = 28 px inside 6 px of padding.
        assert_eq!(rows_fitting(2.0 * 6.0 + 28.0 * 10.0, METRICS), 10);
        assert_eq!(rows_fitting(2.0 * 6.0 + 28.0 * 10.0 - 1.0, METRICS), 9);
        // Never fewer than a theme and "More...".
        assert_eq!(rows_fitting(10.0, METRICS), 2);
    }

    fn paged_menu(count: usize, rows: usize) -> ContextMenu {
        let themes = themes_named(count);
        ContextMenu::new(
            (10.0, 10.0),
            vec![Item::Submenu {
                label: "Themes".into(),
                entries: theme_pages(&themes, rows, Lang::En),
            }],
        )
    }

    fn rows_rect(rows: &[MenuRow], index: usize) -> Rect {
        rows.iter().find(|row| row.index == index).unwrap().rect
    }

    #[test]
    fn more_opens_the_next_themes_beside_it() {
        let window = Rect::from_size(800, 2 * 6 + 28 * 10);
        let mut menu = paged_menu(25, rows_fitting(window.height, METRICS));
        menu.hover(MenuHit::Entry {
            level: MenuLevel::MAIN,
            index: 0,
        });
        let layout = MenuLayout::new(&menu, window, METRICS, width);
        assert_eq!(layout.submenus.len(), 1);
        let first = &layout.submenus[0];
        assert_eq!(first.rows.len(), 10);
        assert!(first.rect.y + first.rect.height <= window.height);

        // Pointing at "More..." (the 10th row) opens the next page beside it.
        let more = rows_rect(&first.rows, 9);
        menu.hover_at(&layout, more.x + 1.0, more.y + 1.0);
        assert_eq!(menu.depth(), 2);
        assert_eq!(menu.previewed_theme(), None);
        let layout = MenuLayout::new(&menu, window, METRICS, width);
        assert_eq!(layout.submenus.len(), 2);
        let second = &layout.submenus[1];
        assert!(second.rect.x >= first.rect.x + first.rect.width);

        // Its rows hit level 2; the pointer moving onto one previews and chooses it.
        let row = rows_rect(&second.rows, 0);
        let hit = layout.hit(row.x + 1.0, row.y + 1.0);
        assert_eq!(
            hit,
            MenuHit::Entry {
                level: MenuLevel(2),
                index: 0
            }
        );
        menu.hover(hit);
        assert_eq!(menu.depth(), 2, "the page stays open under the pointer");
        assert_eq!(menu.previewed_theme(), Some("theme-009"));
        assert_eq!(
            menu.choose(hit),
            Some(Outcome::Run(Command::Theme("theme-009".into())))
        );

        // Back on a theme of the first page: the next page closes.
        menu.hover(MenuHit::Entry {
            level: MenuLevel(1),
            index: 2,
        });
        assert_eq!(menu.depth(), 1);
        assert_eq!(menu.previewed_theme(), Some("theme-002"));
    }

    #[test]
    fn the_arrows_reach_the_next_themes() {
        let mut menu = paged_menu(5, 3);
        menu.key(&Key::Named(NamedKey::ArrowDown));
        menu.key(&Key::Named(NamedKey::ArrowRight));
        assert_eq!(menu.depth(), 1);
        // theme-000, theme-001, More...
        menu.key(&Key::Named(NamedKey::ArrowUp));
        assert_eq!(menu.previewed_theme(), None);
        assert_eq!(menu.key(&Key::Named(NamedKey::Enter)), None);
        assert_eq!(menu.depth(), 2);
        menu.key(&Key::Named(NamedKey::ArrowDown));
        assert_eq!(menu.previewed_theme(), Some("theme-003"));
        assert_eq!(
            menu.key(&Key::Named(NamedKey::Enter)),
            Some(Outcome::Run(Command::Theme("theme-003".into())))
        );
        // Escape closes one menu at a time.
        assert_eq!(menu.key(&Key::Named(NamedKey::Escape)), None);
        assert_eq!(menu.depth(), 1);
        assert_eq!(menu.key(&Key::Named(NamedKey::ArrowLeft)), None);
        assert_eq!(menu.depth(), 0);
        assert_eq!(
            menu.key(&Key::Named(NamedKey::Escape)),
            Some(Outcome::Close)
        );
    }

    #[test]
    fn a_submenu_opens_left_when_needed() {
        let mut menu = paged_menu(3, 10);
        menu.anchor = (145.0, 20.0);
        menu.hover(MenuHit::Entry {
            level: MenuLevel::MAIN,
            index: 0,
        });
        let layout = MenuLayout::new(&menu, Rect::from_size(150, 200), METRICS, width);
        assert!(layout.submenus[0].rect.x < layout.rect.x);
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
        level: MenuLevel::MAIN,
        index: 1,
    };

    fn theme_row(index: usize) -> MenuHit {
        MenuHit::Entry {
            level: MenuLevel(1),
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
            level: MenuLevel::MAIN,
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
            level: MenuLevel::MAIN,
            index: 0,
        });
        assert!(menu.submenu_open());
        let layout = MenuLayout::new(&menu, Rect::from_size(400, 200), METRICS, width);
        let submenu = &layout.submenus[0];
        assert!(submenu.rows[0].active_theme);
        assert_eq!(
            menu.choose(MenuHit::Entry {
                level: MenuLevel(1),
                index: 0,
            }),
            Some(Outcome::Run(Command::Theme("Dark".into())))
        );
    }
}
