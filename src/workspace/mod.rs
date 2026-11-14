//! Tabs and panes: how the window is divided between the shells it runs.
//!
//! The workspace is generic over the pane type so its bookkeeping (which tab holds
//! which pane, which is active, zoom) is tested without spawning shells.
#![allow(
    dead_code,
    reason = "tab and split operations are wired by the shortcut and tab bar tasks"
)]

pub mod tree;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::layout::Rect;
use crate::terminal::PaneId;
use tree::{Axis, Geometry, Layout, Node};

/// Thickness of the divider between two panes, in physical pixels.
pub const DIVIDER_THICKNESS: f32 = 1.0;

/// Edge of the window the tab bar sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabBarPosition {
    #[default]
    Top,
}

impl TabBarPosition {
    /// `window` without a tab bar `thickness` pixels thick on this edge.
    pub fn content_area(self, window: Rect, thickness: f32) -> Rect {
        let thickness = thickness.clamp(0.0, window.height);
        match self {
            TabBarPosition::Top => Rect {
                y: window.y + thickness,
                height: window.height - thickness,
                ..window
            },
        }
    }
}

/// A tab: its split tree, the pane with the keyboard focus, and whether that pane is
/// zoomed to the whole tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    pub tree: Node,
    pub active: PaneId,
    pub zoomed: bool,
}

impl Tab {
    fn new(pane: PaneId) -> Self {
        Self {
            tree: Node::Leaf(pane),
            active: pane,
            zoomed: false,
        }
    }

    /// Rectangles of the panes shown in `geometry.area`: only the active pane, over the
    /// whole area and without dividers, while zoomed.
    pub fn layout(&self, geometry: Geometry) -> Layout {
        if self.zoomed {
            Layout {
                panes: vec![(self.active, geometry.area)],
                dividers: Vec::new(),
                divider: geometry.divider,
            }
        } else {
            self.tree.layout(geometry)
        }
    }

    /// Whether the pane `id` is shown when this tab is the active one.
    pub fn shows(&self, id: PaneId) -> bool {
        if self.zoomed {
            id == self.active
        } else {
            self.tree.contains(id)
        }
    }

    /// Whether the tab holds a single pane.
    fn is_single(&self) -> bool {
        matches!(self.tree, Node::Leaf(_))
    }
}

/// The last pane of the last tab was closed: the window has nothing left to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceEmpty;

/// What a tab title falls back on, read from its active pane.
pub trait TitleSource {
    /// Last title set by the shell or a program (OSC 0/2).
    fn osc_title(&self) -> Option<&str>;
    /// Current directory of the shell.
    fn current_dir(&self) -> Option<PathBuf>;
    /// Name of the shell program.
    fn shell_name(&self) -> Option<String>;
}

/// The tabs of the window and the panes they hold.
pub struct Workspace<P> {
    tabs: Vec<Tab>,
    /// Index of the shown tab in `tabs`.
    active: usize,
    panes: HashMap<PaneId, P>,
    /// Next pane id to hand out.
    next_id: u64,
}

impl<P> Default for Workspace<P> {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            panes: HashMap::new(),
            next_id: 0,
        }
    }
}

impl<P> Workspace<P> {
    /// A workspace without tabs; [`Workspace::new_tab`] opens the first one.
    pub fn new() -> Self {
        Self::default()
    }

    /// A pane id never handed out before, for a pane about to be created.
    pub fn allocate_id(&mut self) -> PaneId {
        let id = PaneId(self.next_id);
        self.next_id += 1;
        id
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Index of the shown tab.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// The shown tab, `None` only before the first tab is opened.
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    /// The pane with the keyboard focus in the shown tab.
    pub fn active_pane_id(&self) -> Option<PaneId> {
        self.active_tab().map(|tab| tab.active)
    }

    pub fn active_pane(&self) -> Option<&P> {
        self.pane(self.active_pane_id()?)
    }

    pub fn active_pane_mut(&mut self) -> Option<&mut P> {
        let id = self.active_pane_id()?;
        self.pane_mut(id)
    }

    pub fn pane(&self, id: PaneId) -> Option<&P> {
        self.panes.get(&id)
    }

    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut P> {
        self.panes.get_mut(&id)
    }

    /// Every pane, in no particular order.
    pub fn panes_mut(&mut self) -> impl Iterator<Item = (PaneId, &mut P)> {
        self.panes.iter_mut().map(|(id, pane)| (*id, pane))
    }

    /// Index of the tab holding the pane `id`.
    pub fn tab_of(&self, id: PaneId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.tree.contains(id))
    }

    /// Whether the pane `id` is on screen: in the shown tab, and not hidden by a zoom.
    pub fn is_visible(&self, id: PaneId) -> bool {
        self.active_tab().is_some_and(|tab| tab.shows(id))
    }

    /// Rectangles of the panes of tab `index` shown in `geometry.area`.
    pub fn layout(&self, index: usize, geometry: Geometry) -> Layout {
        self.tabs
            .get(index)
            .map(|tab| tab.layout(geometry))
            .unwrap_or_default()
    }

    /// Opens a tab holding the pane `pane`, with the id `id` from
    /// [`Workspace::allocate_id`], after the last one, and shows it.
    pub fn new_tab(&mut self, id: PaneId, pane: P) {
        self.panes.insert(id, pane);
        self.tabs.push(Tab::new(id));
        self.active = self.tabs.len() - 1;
    }

    /// Closes the pane `id`: its sibling takes its place in the tree and the focus
    /// moves to the previous pane when it had it. The last pane of a tab closes the
    /// tab; the last tab gives [`WorkspaceEmpty`]. Unknown ids are ignored.
    pub fn close_pane(&mut self, id: PaneId) -> Result<(), WorkspaceEmpty> {
        let Some(index) = self.tab_of(id) else {
            return Ok(());
        };
        self.panes.remove(&id);
        let tab = &mut self.tabs[index];
        if tab.is_single() {
            self.remove_tab(index);
            return if self.tabs.is_empty() {
                Err(WorkspaceEmpty)
            } else {
                Ok(())
            };
        }
        tab.zoomed = false;
        if tab.active == id
            && let Some(previous) = tab.tree.previous(id)
        {
            tab.active = previous;
        }
        tab.tree.remove(id);
        Ok(())
    }

    /// Removes tab `index`; the shown tab stays shown, or the next one (the previous
    /// one for the last tab) replaces it.
    fn remove_tab(&mut self, index: usize) {
        self.tabs.remove(index);
        if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
    }

    /// Shows tab `index`. `false` when there is no such tab.
    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        self.active = index;
        true
    }

    /// Shows the tab after the shown one, wrapping to the first.
    pub fn next(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }

    /// Shows the tab before the shown one, wrapping to the last.
    pub fn previous(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
        }
    }

    /// Moves tab `from` to index `to`; the shown tab stays shown. `false` when either
    /// index is out of range.
    pub fn move_tab(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tabs.len() || to >= self.tabs.len() {
            return false;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        if self.active == from {
            self.active = to;
        } else if from < self.active && self.active <= to {
            self.active -= 1;
        } else if to <= self.active && self.active < from {
            self.active += 1;
        }
        true
    }

    /// Moves the pane `id` out of its tree into a new tab right after its tab, and
    /// shows it. `false` when it is the only pane of its tab, or unknown.
    pub fn detach(&mut self, id: PaneId) -> bool {
        let Some(index) = self.tab_of(id) else {
            return false;
        };
        let tab = &mut self.tabs[index];
        if tab.is_single() {
            return false;
        }
        tab.zoomed = false;
        if tab.active == id
            && let Some(previous) = tab.tree.previous(id)
        {
            tab.active = previous;
        }
        tab.tree.remove(id);
        self.tabs.insert(index + 1, Tab::new(id));
        self.active = index + 1;
        true
    }

    /// Gives the keyboard focus to the pane `id`, showing its tab. A focus change
    /// inside a zoomed tab unzooms it first. `false` when the pane is unknown.
    pub fn focus(&mut self, id: PaneId) -> bool {
        let Some(index) = self.tab_of(id) else {
            return false;
        };
        self.active = index;
        let tab = &mut self.tabs[index];
        if tab.active != id {
            tab.zoomed = false;
            tab.active = id;
        }
        true
    }

    /// Splits the active pane of the shown tab along `axis` (see [`Node::split`]):
    /// the new pane, built by `spawn` from its id, comes second and takes the focus;
    /// the tab is unzoomed. `None`, with nothing changed, when the split is refused or
    /// `spawn` fails.
    pub fn split(
        &mut self,
        axis: Axis,
        geometry: Geometry,
        spawn: impl FnOnce(PaneId) -> Option<P>,
    ) -> Option<PaneId> {
        let target = self.active_pane_id()?;
        let id = self.allocate_id();
        let tab = self.tabs.get_mut(self.active)?;
        if !tab.tree.split(target, axis, id, geometry) {
            return None;
        }
        let Some(pane) = spawn(id) else {
            tab.tree.remove(id);
            return None;
        };
        tab.zoomed = false;
        tab.active = id;
        self.panes.insert(id, pane);
        Some(id)
    }

    /// Zooms the active pane of the shown tab to the whole tab, or back. `false` when
    /// the tab has a single pane, which has nothing to zoom over.
    pub fn toggle_zoom(&mut self) -> bool {
        match self.tabs.get_mut(self.active) {
            Some(tab) if !tab.is_single() => {
                tab.zoomed = !tab.zoomed;
                true
            }
            _ => false,
        }
    }
}

impl<P: TitleSource> Workspace<P> {
    /// Title of tab `index`, from its active pane (see [`title_for`]).
    pub fn tab_title(&self, index: usize) -> Option<String> {
        let pane = self.pane(self.tabs.get(index)?.active)?;
        let home = std::env::var_os("HOME").map(PathBuf::from);
        title_for(
            pane.osc_title(),
            pane.current_dir().as_deref(),
            home.as_deref(),
            pane.shell_name().as_deref(),
        )
    }
}

/// A tab title: the title set by the shell or a program, else the current directory
/// with `home` shown as `~`, else the shell name.
pub fn title_for(
    osc: Option<&str>,
    cwd: Option<&Path>,
    home: Option<&Path>,
    shell: Option<&str>,
) -> Option<String> {
    if let Some(title) = osc.filter(|title| !title.is_empty()) {
        return Some(title.to_owned());
    }
    if let Some(cwd) = cwd {
        return Some(display_dir(cwd, home));
    }
    shell.filter(|name| !name.is_empty()).map(str::to_owned)
}

/// `dir` with the `home` prefix replaced by `~`.
fn display_dir(dir: &Path, home: Option<&Path>) -> String {
    let relative = home
        .filter(|home| !home.as_os_str().is_empty())
        .and_then(|home| dir.strip_prefix(home).ok());
    match relative {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::CellSize;

    #[derive(Debug, Default)]
    struct Fake {
        title: Option<String>,
        cwd: Option<PathBuf>,
        shell: Option<String>,
    }

    impl TitleSource for Fake {
        fn osc_title(&self) -> Option<&str> {
            self.title.as_deref()
        }
        fn current_dir(&self) -> Option<PathBuf> {
            self.cwd.clone()
        }
        fn shell_name(&self) -> Option<String> {
            self.shell.clone()
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

    /// A workspace with one tab per entry, each holding that many panes split side by
    /// side; the first tab is shown.
    fn workspace(panes_per_tab: &[usize]) -> Workspace<Fake> {
        let mut ws = Workspace::new();
        for &count in panes_per_tab {
            let id = ws.allocate_id();
            ws.new_tab(id, Fake::default());
            for _ in 1..count {
                ws.split(Axis::Horizontal, geometry(), |_| Some(Fake::default()))
                    .expect("split fits");
            }
        }
        ws.select(0);
        ws
    }

    #[test]
    fn allocated_ids_are_unique() {
        let mut ws = Workspace::<Fake>::new();
        let a = ws.allocate_id();
        let b = ws.allocate_id();
        assert_ne!(a, b);
    }

    #[test]
    fn new_tab_goes_last_and_is_shown() {
        let mut ws = workspace(&[1, 1]);
        let id = ws.allocate_id();
        ws.new_tab(id, Fake::default());
        assert_eq!(ws.tabs().len(), 3);
        assert_eq!(ws.active_index(), 2);
        assert_eq!(ws.active_pane_id(), Some(id));
    }

    #[test]
    fn closing_a_pane_keeps_the_tab_and_moves_the_focus() {
        let mut ws = workspace(&[2]);
        let panes = ws.tabs()[0].tree.leaves();
        assert_eq!(ws.active_pane_id(), Some(panes[1]));
        assert_eq!(ws.close_pane(panes[1]), Ok(()));
        assert_eq!(ws.tabs().len(), 1);
        assert_eq!(ws.tabs()[0].tree, Node::Leaf(panes[0]));
        assert_eq!(ws.active_pane_id(), Some(panes[0]));
        assert!(ws.pane(panes[1]).is_none());
    }

    #[test]
    fn closing_the_last_pane_closes_the_tab() {
        let mut ws = workspace(&[1, 1, 1]);
        ws.select(1);
        let middle = ws.active_pane_id().unwrap();
        assert_eq!(ws.close_pane(middle), Ok(()));
        assert_eq!(ws.tabs().len(), 2);
        // The next tab takes the closed tab's place.
        assert_eq!(ws.active_index(), 1);
    }

    #[test]
    fn closing_the_last_tab_keeps_a_valid_active_index() {
        let mut ws = workspace(&[1, 1]);
        ws.select(1);
        let last = ws.active_pane_id().unwrap();
        assert_eq!(ws.close_pane(last), Ok(()));
        assert_eq!(ws.active_index(), 0);
    }

    #[test]
    fn closing_a_tab_before_the_shown_one_keeps_it_shown() {
        let mut ws = workspace(&[1, 1, 1]);
        ws.select(2);
        let shown = ws.active_pane_id();
        let first = ws.tabs()[0].active;
        assert_eq!(ws.close_pane(first), Ok(()));
        assert_eq!(ws.active_pane_id(), shown);
        assert_eq!(ws.active_index(), 1);
    }

    #[test]
    fn closing_the_last_pane_of_the_last_tab_empties_the_workspace() {
        let mut ws = workspace(&[1]);
        let only = ws.active_pane_id().unwrap();
        assert_eq!(ws.close_pane(only), Err(WorkspaceEmpty));
        assert!(ws.tabs().is_empty());
    }

    #[test]
    fn closing_an_unknown_pane_does_nothing() {
        let mut ws = workspace(&[1]);
        assert_eq!(ws.close_pane(PaneId(99)), Ok(()));
        assert_eq!(ws.tabs().len(), 1);
    }

    #[test]
    fn select_next_and_previous_wrap() {
        let mut ws = workspace(&[1, 1, 1]);
        assert!(!ws.select(3));
        ws.previous();
        assert_eq!(ws.active_index(), 2);
        ws.next();
        assert_eq!(ws.active_index(), 0);
        ws.next();
        assert_eq!(ws.active_index(), 1);
    }

    #[test]
    fn move_tab_keeps_the_active_tab_active() {
        let mut ws = workspace(&[1, 1, 1, 1]);
        ws.select(1);
        let shown = ws.active_pane_id();
        // The shown tab itself moves.
        assert!(ws.move_tab(1, 3));
        assert_eq!(ws.active_index(), 3);
        assert_eq!(ws.active_pane_id(), shown);
        // A tab moves from before to after the shown one.
        assert!(ws.move_tab(0, 3));
        assert_eq!(ws.active_pane_id(), shown);
        // A tab moves from after to before the shown one.
        assert!(ws.move_tab(3, 0));
        assert_eq!(ws.active_pane_id(), shown);
        assert!(!ws.move_tab(0, 4));
    }

    #[test]
    fn detach_of_the_only_pane_is_refused() {
        let mut ws = workspace(&[1]);
        let only = ws.active_pane_id().unwrap();
        assert!(!ws.detach(only));
        assert_eq!(ws.tabs().len(), 1);
    }

    #[test]
    fn detach_opens_a_tab_after_its_own() {
        let mut ws = workspace(&[2, 1]);
        let panes = ws.tabs()[0].tree.leaves();
        assert!(ws.detach(panes[1]));
        assert_eq!(ws.tabs().len(), 3);
        assert_eq!(ws.tabs()[0].tree, Node::Leaf(panes[0]));
        assert_eq!(ws.tabs()[0].active, panes[0]);
        assert_eq!(ws.tabs()[1].tree, Node::Leaf(panes[1]));
        assert_eq!(ws.active_index(), 1);
    }

    #[test]
    fn zoom_needs_several_panes() {
        let mut ws = workspace(&[1]);
        assert!(!ws.toggle_zoom());
        assert!(!ws.active_tab().unwrap().zoomed);
    }

    #[test]
    fn zoomed_layout_shows_only_the_active_pane() {
        let mut ws = workspace(&[2]);
        let active = ws.active_pane_id().unwrap();
        let other = ws.tabs()[0].tree.leaves()[0];
        assert!(ws.toggle_zoom());
        let layout = ws.layout(0, geometry());
        assert_eq!(layout.panes, vec![(active, geometry().area)]);
        assert!(layout.dividers.is_empty());
        assert!(ws.is_visible(active));
        assert!(!ws.is_visible(other));
    }

    #[test]
    fn focus_change_clears_the_zoom() {
        let mut ws = workspace(&[2]);
        let panes = ws.tabs()[0].tree.leaves();
        assert!(ws.toggle_zoom());
        // Focusing the zoomed pane again keeps the zoom.
        assert!(ws.focus(panes[1]));
        assert!(ws.active_tab().unwrap().zoomed);
        assert!(ws.focus(panes[0]));
        assert!(!ws.active_tab().unwrap().zoomed);
        assert_eq!(ws.active_pane_id(), Some(panes[0]));
    }

    #[test]
    fn focus_shows_the_tab_of_the_pane() {
        let mut ws = workspace(&[1, 2]);
        let target = ws.tabs()[1].tree.leaves()[0];
        assert!(ws.focus(target));
        assert_eq!(ws.active_index(), 1);
        assert_eq!(ws.active_pane_id(), Some(target));
        assert!(!ws.focus(PaneId(99)));
    }

    #[test]
    fn split_clears_the_zoom_and_focuses_the_new_pane() {
        let mut ws = workspace(&[2]);
        assert!(ws.toggle_zoom());
        let new = ws
            .split(Axis::Vertical, geometry(), |_| Some(Fake::default()))
            .unwrap();
        let tab = ws.active_tab().unwrap();
        assert!(!tab.zoomed);
        assert_eq!(tab.active, new);
        assert!(ws.pane(new).is_some());
    }

    #[test]
    fn failed_spawn_leaves_the_tree_unchanged() {
        let mut ws = workspace(&[1]);
        let before = ws.active_tab().unwrap().clone();
        assert_eq!(ws.split(Axis::Horizontal, geometry(), |_| None), None);
        assert_eq!(ws.active_tab().unwrap(), &before);
    }

    #[test]
    fn refused_split_does_not_spawn() {
        let mut ws = workspace(&[1]);
        let small = Geometry {
            area: Rect::from_size(150, 600),
            ..geometry()
        };
        let mut spawned = false;
        let result = ws.split(Axis::Horizontal, small, |_| {
            spawned = true;
            Some(Fake::default())
        });
        assert_eq!(result, None);
        assert!(!spawned);
    }

    #[test]
    fn closing_a_pane_clears_the_zoom() {
        let mut ws = workspace(&[3]);
        assert!(ws.toggle_zoom());
        let first = ws.tabs()[0].tree.leaves()[0];
        assert_eq!(ws.close_pane(first), Ok(()));
        assert!(!ws.active_tab().unwrap().zoomed);
    }

    #[test]
    fn title_prefers_the_osc_title() {
        let home = Path::new("/home/me");
        let title = title_for(
            Some("vim"),
            Some(Path::new("/home/me/dev")),
            Some(home),
            Some("zsh"),
        );
        assert_eq!(title.as_deref(), Some("vim"));
    }

    #[test]
    fn title_falls_back_on_the_directory_with_home_as_tilde() {
        let home = Some(Path::new("/home/me"));
        let at = |dir: &str| title_for(None, Some(Path::new(dir)), home, Some("zsh"));
        assert_eq!(at("/home/me/dev/x").as_deref(), Some("~/dev/x"));
        assert_eq!(at("/home/me").as_deref(), Some("~"));
        assert_eq!(at("/home/meow").as_deref(), Some("/home/meow"));
        assert_eq!(at("/tmp").as_deref(), Some("/tmp"));
        // An empty OSC title counts as none.
        let title = title_for(Some(""), Some(Path::new("/tmp")), home, None);
        assert_eq!(title.as_deref(), Some("/tmp"));
    }

    #[test]
    fn title_falls_back_on_the_shell_name_last() {
        assert_eq!(
            title_for(None, None, None, Some("fish")).as_deref(),
            Some("fish")
        );
        assert_eq!(title_for(None, None, None, None), None);
    }

    #[test]
    fn tab_title_reads_the_active_pane() {
        let mut ws = workspace(&[2]);
        let active = ws.active_pane_id().unwrap();
        ws.pane_mut(active).unwrap().title = Some("htop".to_owned());
        assert_eq!(ws.tab_title(0).as_deref(), Some("htop"));
        assert_eq!(ws.tab_title(5), None);
    }

    #[test]
    fn tab_bar_area_is_taken_from_the_top() {
        let window = Rect::from_size(800, 600);
        let area = TabBarPosition::Top.content_area(window, 30.0);
        assert_eq!(
            area,
            Rect {
                x: 0.0,
                y: 30.0,
                width: 800.0,
                height: 570.0
            }
        );
        assert_eq!(TabBarPosition::Top.content_area(window, 0.0), window);
    }
}
