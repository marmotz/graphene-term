//! Tabs and panes: how the window is divided between the shells it runs.
//!
//! The workspace is generic over the pane type so its bookkeeping (which tab holds
//! which pane, which is active, zoom) is tested without spawning shells.

pub mod drop;
pub mod tree;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use crate::config::TabPosition;
use crate::i18n::Direction;
use crate::layout::Rect;
use crate::progress::Progress;
use crate::terminal::PaneId;
use tree::{Axis, Branch, Geometry, Layout, Node};

/// Thickness of the divider between two panes, in physical pixels.
pub const DIVIDER_THICKNESS: f32 = 1.0;

/// Cells a pane grows by on each resize key press.
pub const RESIZE_STEP_CELLS: usize = 2;

/// Edge of the window the tab bar sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabBarPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

impl From<TabPosition> for TabBarPosition {
    fn from(position: TabPosition) -> Self {
        match position {
            TabPosition::Top => Self::Top,
            TabPosition::Bottom => Self::Bottom,
            TabPosition::Left => Self::Left,
            TabPosition::Right => Self::Right,
        }
    }
}

impl TabBarPosition {
    /// Whether the bar runs along a side of the window, its tabs stacked.
    pub fn is_vertical(self) -> bool {
        matches!(self, TabBarPosition::Left | TabBarPosition::Right)
    }

    /// `window` without a tab bar `thickness` pixels thick on this edge.
    pub fn content_area(self, window: Rect, thickness: f32) -> Rect {
        let across = if self.is_vertical() {
            window.width
        } else {
            window.height
        };
        let thickness = thickness.clamp(0.0, across);
        match self {
            TabBarPosition::Top => Rect {
                y: window.y + thickness,
                height: window.height - thickness,
                ..window
            },
            TabBarPosition::Bottom => Rect {
                height: window.height - thickness,
                ..window
            },
            TabBarPosition::Left => Rect {
                x: window.x + thickness,
                width: window.width - thickness,
                ..window
            },
            TabBarPosition::Right => Rect {
                width: window.width - thickness,
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
    /// Panes of this tab that typed input is sent to together; empty when broadcast
    /// is off. Only ever holds panes of this tab.
    pub broadcast: BTreeSet<PaneId>,
}

impl Tab {
    fn new(pane: PaneId) -> Self {
        Self {
            tree: Node::Leaf(pane),
            active: pane,
            zoomed: false,
            broadcast: BTreeSet::new(),
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
    pub fn is_single(&self) -> bool {
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

/// What a progress indicator reads from a pane.
pub trait ProgressSource {
    /// The progress reported by the pane's programs, `None` without one.
    fn progress(&self) -> Option<Progress>;
}

/// The tabs of the window and the panes they hold.
pub struct Workspace<P> {
    tabs: Vec<Tab>,
    /// Index of the shown tab in `tabs`.
    active: usize,
    panes: HashMap<PaneId, P>,
    /// Revision of each pane changed at least once: see [`Workspace::revision`].
    revisions: HashMap<PaneId, u64>,
    /// Last revision handed out.
    last_revision: u64,
    /// Next pane id to hand out.
    next_id: u64,
    /// Whether the progress of the panes is shown (`window.progress`).
    progress_shown: bool,
}

impl<P> Default for Workspace<P> {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            panes: HashMap::new(),
            revisions: HashMap::new(),
            last_revision: 0,
            next_id: 0,
            progress_shown: true,
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

    /// The pane `id`, to change it: its revision changes.
    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut P> {
        if self.panes.contains_key(&id) {
            self.touch(id);
        }
        self.panes.get_mut(&id)
    }

    /// Every pane, in no particular order.
    pub fn panes(&self) -> impl Iterator<Item = (PaneId, &P)> {
        self.panes.iter().map(|(id, pane)| (*id, pane))
    }

    /// Every pane, in no particular order, to change them: their revisions change.
    pub fn panes_mut(&mut self) -> impl Iterator<Item = (PaneId, &mut P)> {
        let ids: Vec<_> = self.panes.keys().copied().collect();
        for id in ids {
            self.touch(id);
        }
        self.panes.iter_mut().map(|(id, pane)| (*id, pane))
    }

    /// Changes the revision of the pane `id`: what it shows changed in a way the
    /// workspace cannot see (behind a lock or a cell).
    pub fn touch(&mut self, id: PaneId) {
        self.last_revision += 1;
        self.revisions.insert(id, self.last_revision);
    }

    /// Revision of the pane `id`: it changes whenever the pane may have changed, so a
    /// frame captured at a revision is still the pane's while the revision is the same.
    pub fn revision(&self, id: PaneId) -> u64 {
        self.revisions.get(&id).copied().unwrap_or(0)
    }

    /// Index of the tab holding the pane `id`.
    pub fn tab_of(&self, id: PaneId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.tree.contains(id))
    }

    /// Shows or hides the progress of every pane (`window.progress`): hidden, every
    /// progress read returns `None`, while the panes keep following theirs.
    pub fn set_progress_shown(&mut self, shown: bool) {
        self.progress_shown = shown;
    }

    /// Whether a progress of the pane `id` is drawn: on the pane when it is on screen,
    /// on its tab when the tab bar is shown and the pane is the active one of its tab.
    pub fn progress_on_screen(&self, id: PaneId, tab_bar_shown: bool) -> bool {
        self.progress_shown
            && (self.is_visible(id)
                || tab_bar_shown && self.tabs.iter().any(|tab| tab.active == id))
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
        self.revisions.remove(&id);
        let tab = &mut self.tabs[index];
        tab.broadcast.remove(&id);
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
        if self.tabs[index].is_single() {
            return false;
        }
        self.take_out(index, id);
        self.tabs.insert(index + 1, Tab::new(id));
        self.active = index + 1;
        true
    }

    /// Moves the pane `id` next to the pane `target`, on its `side` (see
    /// [`Node::insert`]). The moved pane becomes the active pane of the target tab,
    /// which is shown; both tabs are unzoomed, and a source tab left empty is closed.
    /// `false`, with nothing changed, when either pane is unknown, they are the same
    /// pane, or the target is too small to split.
    pub fn move_pane(
        &mut self,
        id: PaneId,
        target: PaneId,
        side: Direction,
        geometry: Geometry,
    ) -> bool {
        let Some((source, mut index, tree)) = self.moved_tree(id, target, side, geometry) else {
            return false;
        };
        let broadcast = std::mem::take(&mut self.tabs[index].broadcast);
        self.tabs[index] = Tab {
            tree,
            active: id,
            zoomed: false,
            broadcast,
        };
        if source != index {
            if self.tabs[source].is_single() {
                self.remove_tab(source);
                if source < index {
                    index -= 1;
                }
            } else {
                self.take_out(source, id);
            }
        }
        self.active = index;
        true
    }

    /// Whether [`Self::move_pane`] would accept moving `id` next to `target` on `side`,
    /// without changing anything.
    pub fn can_move_pane(
        &self,
        id: PaneId,
        target: PaneId,
        side: Direction,
        geometry: Geometry,
    ) -> bool {
        self.moved_tree(id, target, side, geometry).is_some()
    }

    /// The tree of the tab of `target` once `id` is inserted next to it on `side` (and
    /// removed from it, when both share that tab), with the indices of the source and
    /// target tabs. `None` when the move is refused.
    fn moved_tree(
        &self,
        id: PaneId,
        target: PaneId,
        side: Direction,
        geometry: Geometry,
    ) -> Option<(usize, usize, Node)> {
        if id == target {
            return None;
        }
        let (source, index) = (self.tab_of(id)?, self.tab_of(target)?);
        let mut tree = self.tabs[index].tree.clone();
        if source == index && !tree.remove(id) {
            return None;
        }
        tree.insert(target, side, id, geometry)
            .then_some((source, index, tree))
    }

    /// Exchanges the places of the panes `a` (the one moved) and `b`, in one tab or
    /// across two. `a` gets the focus in the tab of `b`, which is shown; in the tab of
    /// `a`, `b` stays active when `a` was. `false` when either pane is unknown or they
    /// are the same pane.
    pub fn swap_panes(&mut self, a: PaneId, b: PaneId) -> bool {
        if a == b {
            return false;
        }
        let (Some(source), Some(index)) = (self.tab_of(a), self.tab_of(b)) else {
            return false;
        };
        if source == index {
            let tab = &mut self.tabs[index];
            tab.tree.swap(a, b);
        } else {
            let tab = &mut self.tabs[source];
            tab.tree.replace(a, b);
            if tab.active == a {
                tab.active = b;
            }
            tab.broadcast.remove(&a);
            tab.broadcast.remove(&b);
            let target = &mut self.tabs[index];
            target.tree.replace(b, a);
            target.broadcast.remove(&a);
            target.broadcast.remove(&b);
        }
        self.tabs[index].active = a;
        self.active = index;
        true
    }

    /// Moves the pane `id` into tab `index`, next to its active pane on its longer
    /// side: `Right` when wider than tall, else `Down` (see [`Self::move_pane`]).
    /// `false`, with nothing changed, for the pane's own tab, an unknown pane or tab,
    /// or a refused move.
    pub fn move_pane_to_tab(&mut self, id: PaneId, index: usize, geometry: Geometry) -> bool {
        let Some(tab) = self.tabs.get(index) else {
            return false;
        };
        if self.tab_of(id).is_none_or(|source| source == index) {
            return false;
        }
        let target = tab.active;
        let Some(rect) = tab.tree.layout(geometry).rect_of(target) else {
            return false;
        };
        let side = if rect.width > rect.height {
            Direction::Right
        } else {
            Direction::Down
        };
        self.move_pane(id, target, side, geometry)
    }

    /// Moves the pane `id` into a new last tab, and shows it. For the only pane of its
    /// tab, the tab itself moves last. `false` when the pane is unknown.
    pub fn move_pane_to_new_tab(&mut self, id: PaneId) -> bool {
        let Some(source) = self.tab_of(id) else {
            return false;
        };
        let last = self.tabs.len() - 1;
        if self.tabs[source].is_single() {
            self.tabs[source].broadcast.clear();
            self.move_tab(source, last);
            self.active = last;
            return true;
        }
        self.take_out(source, id);
        self.tabs.push(Tab::new(id));
        self.active = self.tabs.len() - 1;
        true
    }

    /// Removes the pane `id` from the tree of tab `index`, which holds other panes,
    /// and unzooms it; the focus moves to the previous pane when `id` had it.
    fn take_out(&mut self, index: usize, id: PaneId) {
        let tab = &mut self.tabs[index];
        tab.zoomed = false;
        tab.broadcast.remove(&id);
        if tab.active == id
            && let Some(previous) = tab.tree.previous(id)
        {
            tab.active = previous;
        }
        tab.tree.remove(id);
    }

    /// Adds the pane `id` to the broadcast set of its tab, or removes it. `false` for
    /// an unknown pane.
    pub fn toggle_broadcast(&mut self, id: PaneId) -> bool {
        let Some(index) = self.tab_of(id) else {
            return false;
        };
        let set = &mut self.tabs[index].broadcast;
        if !set.remove(&id) {
            set.insert(id);
        }
        true
    }

    /// Ctrl+click on the pane `id`: like [`Self::toggle_broadcast`], but a pane added to
    /// the set brings the active pane of its tab along, so that the first click starts a
    /// broadcast between two panes instead of selecting a lone one. `false` for an
    /// unknown pane.
    pub fn toggle_broadcast_click(&mut self, id: PaneId) -> bool {
        let Some(index) = self.tab_of(id) else {
            return false;
        };
        let tab = &mut self.tabs[index];
        if !tab.broadcast.contains(&id) && tab.active != id {
            tab.broadcast.insert(tab.active);
        }
        self.toggle_broadcast(id)
    }

    /// On the shown tab: an empty broadcast set becomes every pane, a non-empty one is
    /// emptied. `false` without a shown tab.
    pub fn toggle_broadcast_all(&mut self) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return false;
        };
        if tab.broadcast.is_empty() {
            tab.broadcast = tab.tree.leaves().into_iter().collect();
        } else {
            tab.broadcast.clear();
        }
        true
    }

    /// Whether the pane `id` is in the broadcast set of its tab.
    pub fn is_broadcast(&self, id: PaneId) -> bool {
        self.tab_of(id)
            .is_some_and(|index| self.tabs[index].broadcast.contains(&id))
    }

    /// Number of panes in the broadcast set of tab `index`.
    pub fn broadcast_count(&self, index: usize) -> usize {
        self.tabs.get(index).map_or(0, |tab| tab.broadcast.len())
    }

    /// The panes typed input goes to: the active pane alone when it is not in the
    /// broadcast set of the shown tab, else the whole set with the active pane first.
    /// A zoom does not change it.
    pub fn input_targets(&self) -> Vec<PaneId> {
        let Some(tab) = self.active_tab() else {
            return Vec::new();
        };
        if !tab.broadcast.contains(&tab.active) {
            return vec![tab.active];
        }
        let mut targets = vec![tab.active];
        targets.extend(tab.broadcast.iter().copied().filter(|&id| id != tab.active));
        targets
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

    /// The pane next to the active one of the shown tab in `direction`, as the panes
    /// are laid out without zoom.
    pub fn neighbour(&self, direction: Direction, geometry: Geometry) -> Option<PaneId> {
        let tab = self.active_tab()?;
        tree::neighbour(tab.active, direction, &tab.tree.layout(geometry))
    }

    /// The pane after the active one of the shown tab, wrapping; `None` with a single
    /// pane.
    pub fn next_pane(&self) -> Option<PaneId> {
        let tab = self.active_tab()?;
        tab.tree.next(tab.active).filter(|&id| id != tab.active)
    }

    /// The pane before the active one of the shown tab, wrapping; `None` with a single
    /// pane.
    pub fn previous_pane(&self) -> Option<PaneId> {
        let tab = self.active_tab()?;
        tab.tree.previous(tab.active).filter(|&id| id != tab.active)
    }

    /// Grows the active pane of the shown tab by `step` cells toward `direction` (see
    /// [`Node::resize`]). `false` when nothing moved, or while zoomed.
    pub fn resize_active(&mut self, direction: Direction, step: usize, geometry: Geometry) -> bool {
        match self.tabs.get_mut(self.active) {
            Some(tab) if !tab.zoomed => tab.tree.resize(tab.active, direction, step, geometry),
            _ => false,
        }
    }

    /// Whether [`Self::split`] along `axis` would fit the active pane of the shown tab.
    pub fn can_split(&self, axis: Axis, geometry: Geometry) -> bool {
        let Some(tab) = self.active_tab() else {
            return false;
        };
        let mut tree = tab.tree.clone();
        let probe = PaneId(self.next_id);
        tree.split(tab.active, axis, probe, geometry)
    }

    /// Whether [`Self::resize_active`] toward `direction` would move a divider.
    pub fn can_resize_active(&self, direction: Direction, step: usize, geometry: Geometry) -> bool {
        match self.active_tab() {
            Some(tab) if !tab.zoomed => tab
                .tree
                .clone()
                .resize(tab.active, direction, step, geometry),
            _ => false,
        }
    }

    /// Moves the divider of the split at `node_path` in the shown tab to the pixel
    /// `position` along its axis (see [`Node::set_divider`]). `false` when nothing
    /// moved.
    pub fn set_divider(&mut self, node_path: &[Branch], position: f32, geometry: Geometry) -> bool {
        self.tabs
            .get_mut(self.active)
            .is_some_and(|tab| tab.tree.set_divider(node_path, position, geometry))
    }

    /// Gives the panes of the shown tab equal shares (see [`Node::equalise`]) and
    /// unzooms it. `false` with a single pane.
    pub fn equalise(&mut self) -> bool {
        match self.tabs.get_mut(self.active) {
            Some(tab) if !tab.is_single() => {
                tab.tree.equalise();
                tab.zoomed = false;
                true
            }
            _ => false,
        }
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

impl<P: ProgressSource> Workspace<P> {
    /// Progress of the pane `id`, `None` without one or when progress is hidden.
    pub fn pane_progress(&self, id: PaneId) -> Option<Progress> {
        self.progress_shown
            .then(|| self.pane(id)?.progress())
            .flatten()
    }

    /// Progress shown on the tab `index`: the one of its active pane.
    pub fn tab_progress(&self, index: usize) -> Option<Progress> {
        self.pane_progress(self.tabs.get(index)?.active)
    }
}

impl<P: TitleSource> Workspace<P> {
    /// Title of tab `index`, from its active pane (see [`title_for`]).
    pub fn tab_title(&self, index: usize) -> Option<String> {
        self.pane_title(self.tabs.get(index)?.active)
    }

    /// Title of the pane `id` (see [`title_for`]).
    pub fn pane_title(&self, id: PaneId) -> Option<String> {
        let pane = self.pane(id)?;
        let home = std::env::var_os("HOME").map(PathBuf::from);
        title_for(
            pane.osc_title(),
            pane.current_dir().as_deref(),
            home.as_deref(),
            pane.shell_name().as_deref(),
        )
    }
}

/// Directory a new pane's shell starts in: the one of the active pane's shell, else
/// `home`.
pub fn start_dir(active: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    active.or(home)
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

    #[test]
    fn mutable_access_changes_the_pane_revision() {
        let mut workspace = Workspace::new();
        let first = workspace.allocate_id();
        workspace.new_tab(first, ());
        let second = workspace.allocate_id();
        workspace.new_tab(second, ());
        let (a, b) = (workspace.revision(first), workspace.revision(second));

        workspace.pane(first);
        workspace.panes().count();
        assert_eq!(
            (workspace.revision(first), workspace.revision(second)),
            (a, b)
        );

        workspace.pane_mut(first);
        let a2 = workspace.revision(first);
        assert_ne!(a2, a);
        assert_eq!(workspace.revision(second), b);

        workspace.touch(second);
        let b2 = workspace.revision(second);
        assert_ne!(b2, b);

        workspace.panes_mut().count();
        assert_ne!(workspace.revision(first), a2);
        assert_ne!(workspace.revision(second), b2);
        // Unknown panes are not given a revision.
        workspace.pane_mut(PaneId(99));
        assert_eq!(workspace.revision(PaneId(99)), 0);
    }

    #[derive(Debug, Default)]
    struct Fake {
        title: Option<String>,
        cwd: Option<PathBuf>,
        shell: Option<String>,
        progress: Option<Progress>,
    }

    impl ProgressSource for Fake {
        fn progress(&self) -> Option<Progress> {
            self.progress
        }
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
            title_bar: 0.0,
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
    fn neighbour_follows_the_layout_of_the_shown_tab() {
        let ws = workspace(&[3]);
        let panes = ws.tabs()[0].tree.leaves();
        // The last pane split off is active, on the right.
        assert_eq!(ws.active_pane_id(), Some(panes[2]));
        assert_eq!(ws.neighbour(Direction::Left, geometry()), Some(panes[1]));
        assert_eq!(ws.neighbour(Direction::Right, geometry()), None);
        assert_eq!(ws.neighbour(Direction::Up, geometry()), None);
    }

    #[test]
    fn neighbour_ignores_the_zoom() {
        let mut ws = workspace(&[2]);
        let panes = ws.tabs()[0].tree.leaves();
        assert!(ws.toggle_zoom());
        assert_eq!(ws.neighbour(Direction::Left, geometry()), Some(panes[0]));
        // Focusing it unzooms the tab.
        assert!(ws.focus(panes[0]));
        assert!(!ws.tabs()[0].zoomed);
    }

    #[test]
    fn next_and_previous_pane_wrap_and_skip_a_single_pane() {
        let mut ws = workspace(&[3, 1]);
        let panes = ws.tabs()[0].tree.leaves();
        assert_eq!(ws.next_pane(), Some(panes[0]));
        assert_eq!(ws.previous_pane(), Some(panes[1]));
        ws.select(1);
        assert_eq!(ws.next_pane(), None);
        assert_eq!(ws.previous_pane(), None);
    }

    #[test]
    fn resize_active_moves_the_divider_but_not_while_zoomed() {
        let mut ws = workspace(&[2]);
        let before = ws.layout(0, geometry());
        assert!(ws.resize_active(Direction::Left, 2, geometry()));
        let after = ws.layout(0, geometry());
        let active = ws.active_pane_id().unwrap();
        let width = |layout: &Layout| layout.rect_of(active).unwrap().width;
        assert_eq!(width(&after), width(&before) + 20.0);
        // The right edge is outside the window, so the opposite divider moves right.
        assert!(ws.resize_active(Direction::Right, 2, geometry()));
        let after = ws.layout(0, geometry());
        assert_eq!(width(&after), width(&before));
        assert!(ws.toggle_zoom());
        assert!(!ws.resize_active(Direction::Left, 2, geometry()));
    }

    #[test]
    fn resize_active_moves_vertical_dividers_at_the_window_edges() {
        let mut ws = Workspace::<Fake>::new();
        let first = ws.allocate_id();
        ws.new_tab(first, Fake::default());
        let second = ws
            .split(Axis::Vertical, geometry(), |_| Some(Fake::default()))
            .expect("vertical split fits");
        let before = ws.layout(0, geometry());
        let height = |layout: &Layout, id| layout.rect_of(id).unwrap().height;

        // The active pane is at the bottom; its lower edge is the window edge.
        assert!(ws.resize_active(Direction::Down, 2, geometry()));
        let after_down = ws.layout(0, geometry());
        assert_eq!(height(&after_down, second), height(&before, second) - 40.0);

        // The top pane has no upper divider either, so the same divider moves up.
        assert!(ws.focus(first));
        assert!(ws.resize_active(Direction::Up, 2, geometry()));
        let after_up = ws.layout(0, geometry());
        assert_eq!(height(&after_up, first), height(&after_down, first) - 40.0);
    }

    #[test]
    fn can_resize_active_answers_like_resize_active_without_moving_anything() {
        let mut ws = workspace(&[2]);
        let before = ws.layout(0, geometry());
        assert!(ws.can_resize_active(Direction::Left, 2, geometry()));
        assert!(ws.can_resize_active(Direction::Right, 2, geometry()));
        assert!(!ws.can_resize_active(Direction::Up, 2, geometry()));
        assert_eq!(ws.layout(0, geometry()), before);
        assert!(ws.toggle_zoom());
        assert!(!ws.can_resize_active(Direction::Left, 2, geometry()));
    }

    #[test]
    fn can_split_is_refused_once_the_pane_is_too_small() {
        let mut ws = workspace(&[1]);
        let tiny = Geometry {
            area: Rect::from_size(40, 600),
            ..geometry()
        };
        assert!(!ws.can_split(Axis::Horizontal, tiny));
        assert!(ws.can_split(Axis::Vertical, tiny));
        assert!(ws.can_split(Axis::Horizontal, geometry()));
        // Probing allocates no id and leaves the tree alone.
        assert!(ws.tabs()[0].is_single());
        assert_eq!(ws.allocate_id(), PaneId(1));
        assert!(!Workspace::<Fake>::new().can_split(Axis::Horizontal, geometry()));
    }

    #[test]
    fn set_divider_moves_the_split_of_the_shown_tab() {
        let mut ws = workspace(&[2]);
        let path = ws.layout(0, geometry()).dividers[0].node_path.clone();
        assert!(ws.set_divider(&path, 200.0, geometry()));
        let first = ws.tabs()[0].tree.leaves()[0];
        assert_eq!(
            ws.layout(0, geometry()).rect_of(first).unwrap().width,
            200.0
        );
    }

    #[test]
    fn equalise_unzooms_and_refuses_a_single_pane() {
        let mut ws = workspace(&[2, 1]);
        let path = ws.layout(0, geometry()).dividers[0].node_path.clone();
        ws.set_divider(&path, 200.0, geometry());
        ws.toggle_zoom();
        assert!(ws.equalise());
        assert!(!ws.tabs()[0].zoomed);
        let layout = ws.layout(0, geometry());
        // Equal up to the rounding to whole cells.
        let (a, b) = (layout.panes[0].1.width, layout.panes[1].1.width);
        assert!((a - b).abs() < geometry().cell.width, "{a} vs {b}");
        ws.select(1);
        assert!(!ws.equalise());
    }

    #[test]
    fn start_dir_prefers_the_active_pane_then_home() {
        let cwd = PathBuf::from("/srv/project");
        let home = PathBuf::from("/home/user");
        assert_eq!(start_dir(Some(cwd.clone()), Some(home.clone())), Some(cwd));
        assert_eq!(start_dir(None, Some(home.clone())), Some(home));
        assert_eq!(start_dir(None, None), None);
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

    fn ids(tab: &Tab) -> Vec<u64> {
        tab.tree.leaves().into_iter().map(|id| id.0).collect()
    }

    fn snapshot(ws: &Workspace<Fake>) -> (Vec<Tab>, usize) {
        (ws.tabs().to_vec(), ws.active_index())
    }

    #[test]
    fn move_pane_within_a_tab_reorders_and_focuses_it() {
        let mut ws = workspace(&[3]);
        ws.toggle_zoom();
        assert!(ws.move_pane(PaneId(2), PaneId(0), Direction::Left, geometry()));
        let tab = &ws.tabs()[0];
        assert_eq!(ids(tab), vec![2, 0, 1]);
        assert_eq!(tab.active, PaneId(2));
        assert!(!tab.zoomed);
    }

    #[test]
    fn move_pane_across_tabs_shows_the_target_tab() {
        let mut ws = workspace(&[2, 2]);
        ws.select(1);
        ws.toggle_zoom();
        ws.select(0);
        ws.toggle_zoom();
        assert!(ws.move_pane(PaneId(1), PaneId(2), Direction::Down, geometry()));
        assert_eq!(ws.active_index(), 1);
        let (source, target) = (&ws.tabs()[0], &ws.tabs()[1]);
        assert_eq!(ids(source), vec![0]);
        assert_eq!(source.active, PaneId(0));
        assert!(!source.zoomed);
        assert_eq!(ids(target), vec![2, 1, 3]);
        assert_eq!(target.active, PaneId(1));
        assert!(!target.zoomed);
    }

    #[test]
    fn move_of_the_last_pane_of_a_tab_closes_it() {
        // Tabs: [0, 1], [2], [3]; the last tab is shown.
        let mut ws = workspace(&[2, 1, 1]);
        ws.select(2);
        assert!(ws.move_pane(PaneId(2), PaneId(3), Direction::Right, geometry()));
        assert_eq!(ws.tabs().len(), 2);
        assert_eq!(ws.active_index(), 1);
        assert_eq!(ids(&ws.tabs()[1]), vec![3, 2]);
        assert_eq!(ws.active_pane_id(), Some(PaneId(2)));
    }

    #[test]
    fn move_of_the_last_pane_to_an_earlier_tab_keeps_its_index() {
        let mut ws = workspace(&[1, 1, 1]);
        assert!(ws.move_pane(PaneId(2), PaneId(0), Direction::Up, geometry()));
        assert_eq!(ws.tabs().len(), 2);
        assert_eq!(ws.active_index(), 0);
        assert_eq!(ids(&ws.tabs()[0]), vec![2, 0]);
        assert_eq!(ids(&ws.tabs()[1]), vec![1]);
    }

    #[test]
    fn refused_moves_change_nothing() {
        let mut ws = workspace(&[2, 1]);
        let before = snapshot(&ws);
        let too_small = Geometry {
            area: Rect::from_size(150, 600),
            ..geometry()
        };
        assert!(!ws.move_pane(PaneId(1), PaneId(2), Direction::Left, too_small));
        assert!(!ws.move_pane(PaneId(0), PaneId(1), Direction::Left, too_small));
        assert!(!ws.move_pane(PaneId(1), PaneId(1), Direction::Left, geometry()));
        assert!(!ws.move_pane(PaneId(9), PaneId(1), Direction::Left, geometry()));
        assert!(!ws.move_pane(PaneId(1), PaneId(9), Direction::Left, geometry()));
        assert_eq!(snapshot(&ws), before);
    }

    #[test]
    fn can_move_pane_matches_move_pane_without_changing_anything() {
        let ws = workspace(&[2, 1]);
        let before = snapshot(&ws);
        let too_small = Geometry {
            area: Rect::from_size(150, 600),
            ..geometry()
        };
        assert!(ws.can_move_pane(PaneId(1), PaneId(0), Direction::Left, geometry()));
        assert!(ws.can_move_pane(PaneId(1), PaneId(2), Direction::Down, geometry()));
        assert!(!ws.can_move_pane(PaneId(1), PaneId(2), Direction::Left, too_small));
        assert!(!ws.can_move_pane(PaneId(1), PaneId(1), Direction::Left, geometry()));
        assert!(!ws.can_move_pane(PaneId(9), PaneId(1), Direction::Left, geometry()));
        assert_eq!(snapshot(&ws), before);
    }

    #[test]
    fn swap_within_a_tab_focuses_the_moved_pane() {
        let mut ws = workspace(&[3]);
        assert!(ws.swap_panes(PaneId(0), PaneId(2)));
        let tab = &ws.tabs()[0];
        assert_eq!(ids(tab), vec![2, 1, 0]);
        assert_eq!(tab.active, PaneId(0));
    }

    #[test]
    fn swap_across_tabs_fixes_both_active_panes() {
        // Tab 0 active pane is 1, tab 1 holds 2 and 3, 3 active.
        let mut ws = workspace(&[2, 2]);
        assert!(ws.swap_panes(PaneId(1), PaneId(2)));
        assert_eq!(ws.active_index(), 1);
        assert_eq!(ids(&ws.tabs()[0]), vec![0, 2]);
        assert_eq!(ws.tabs()[0].active, PaneId(2));
        assert_eq!(ids(&ws.tabs()[1]), vec![1, 3]);
        assert_eq!(ws.tabs()[1].active, PaneId(1));
    }

    #[test]
    fn swap_keeps_the_source_focus_on_another_pane() {
        let mut ws = workspace(&[2, 1]);
        assert!(ws.swap_panes(PaneId(0), PaneId(2)));
        assert_eq!(ids(&ws.tabs()[0]), vec![2, 1]);
        assert_eq!(ws.tabs()[0].active, PaneId(1));
        assert_eq!(ws.tabs()[1].active, PaneId(0));
    }

    #[test]
    fn refused_swaps_change_nothing() {
        let mut ws = workspace(&[2, 1]);
        let before = snapshot(&ws);
        assert!(!ws.swap_panes(PaneId(1), PaneId(1)));
        assert!(!ws.swap_panes(PaneId(1), PaneId(9)));
        assert!(!ws.swap_panes(PaneId(9), PaneId(1)));
        assert_eq!(snapshot(&ws), before);
    }

    #[test]
    fn move_to_tab_splits_its_active_pane_on_the_longer_side() {
        // 800x600: a single pane is wider than tall.
        let mut ws = workspace(&[2, 1]);
        assert!(ws.move_pane_to_tab(PaneId(0), 1, geometry()));
        let tab = &ws.tabs()[1];
        assert!(matches!(
            tab.tree,
            Node::Split {
                axis: Axis::Horizontal,
                ..
            }
        ));
        assert_eq!(ids(tab), vec![2, 0]);
        assert_eq!(ws.active_index(), 1);

        // The target tab now has two 400-wide panes, taller than wide.
        assert!(ws.move_pane_to_tab(PaneId(1), 1, geometry()));
        assert_eq!(ws.tabs().len(), 1);
        let tab = &ws.tabs()[0];
        assert_eq!(ids(tab), vec![2, 0, 1]);
        match &tab.tree {
            Node::Split { second, .. } => assert!(matches!(
                **second,
                Node::Split {
                    axis: Axis::Vertical,
                    ..
                }
            )),
            Node::Leaf(_) => panic!("split expected"),
        }
    }

    #[test]
    fn move_to_its_own_or_a_missing_tab_is_refused() {
        let mut ws = workspace(&[2, 1]);
        let before = snapshot(&ws);
        assert!(!ws.move_pane_to_tab(PaneId(0), 0, geometry()));
        assert!(!ws.move_pane_to_tab(PaneId(0), 5, geometry()));
        assert!(!ws.move_pane_to_tab(PaneId(9), 1, geometry()));
        assert_eq!(snapshot(&ws), before);
    }

    #[test]
    fn move_to_new_tab_opens_a_last_tab() {
        let mut ws = workspace(&[2, 1, 1]);
        ws.toggle_zoom();
        assert!(ws.move_pane_to_new_tab(PaneId(1)));
        assert_eq!(ws.tabs().len(), 4);
        assert_eq!(ws.active_index(), 3);
        assert_eq!(ids(&ws.tabs()[3]), vec![1]);
        assert_eq!(ids(&ws.tabs()[0]), vec![0]);
        assert_eq!(ws.tabs()[0].active, PaneId(0));
        assert!(!ws.tabs()[0].zoomed);
    }

    #[test]
    fn move_of_a_single_pane_to_new_tab_moves_its_tab_last() {
        let mut ws = workspace(&[1, 1, 1]);
        assert!(ws.move_pane_to_new_tab(PaneId(0)));
        assert_eq!(ws.tabs().len(), 3);
        assert_eq!(ws.active_index(), 2);
        let order: Vec<_> = ws.tabs().iter().map(ids).collect();
        assert_eq!(order, vec![vec![1], vec![2], vec![0]]);
        assert!(!ws.move_pane_to_new_tab(PaneId(9)));
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

    fn progress(value: u8) -> Progress {
        Progress {
            state: crate::progress::ProgressState::Normal,
            value,
            updated: std::time::Instant::now(),
        }
    }

    #[test]
    fn tab_progress_follows_the_active_pane() {
        let mut ws = workspace(&[2, 1]);
        let panes = ws.tabs()[0].tree.leaves();
        let reported = progress(10);
        ws.pane_mut(panes[0]).unwrap().progress = Some(reported);
        // The active pane, the last one split off, has none.
        assert_eq!(ws.tab_progress(0), None);
        assert_eq!(ws.pane_progress(panes[0]), Some(reported));
        assert!(ws.focus(panes[0]));
        assert_eq!(ws.tab_progress(0), Some(reported));
        assert_eq!(ws.tab_progress(1), None);
        assert_eq!(ws.tab_progress(5), None);
    }

    #[test]
    fn hidden_progress_reads_none() {
        let mut ws = workspace(&[1]);
        let id = ws.active_pane_id().unwrap();
        ws.pane_mut(id).unwrap().progress = Some(progress(30));
        ws.set_progress_shown(false);
        assert_eq!(ws.tab_progress(0), None);
        assert_eq!(ws.pane_progress(id), None);
        assert!(!ws.progress_on_screen(id, true));
        // Shown again, the pane's current progress is back.
        ws.set_progress_shown(true);
        assert_eq!(ws.tab_progress(0).map(|p| p.value), Some(30));
    }

    #[test]
    fn progress_is_on_screen_on_the_pane_or_its_tab() {
        let mut ws = workspace(&[2, 2]);
        let first = ws.tabs()[0].tree.leaves();
        let second = ws.tabs()[1].tree.leaves();
        // Both panes of the shown tab are on screen.
        assert!(ws.progress_on_screen(first[0], false));
        assert!(ws.progress_on_screen(first[1], false));
        // In a hidden tab, only the active pane shows on its tab, with the bar.
        assert!(!ws.progress_on_screen(second[1], false));
        assert!(ws.progress_on_screen(second[1], true));
        assert!(!ws.progress_on_screen(second[0], true));
        // A pane hidden by a zoom still shows on its tab when active.
        assert!(ws.focus(first[0]));
        assert!(ws.toggle_zoom());
        assert!(!ws.progress_on_screen(first[1], true));
    }

    #[test]
    fn tab_bar_area_is_taken_from_its_edge() {
        let window = Rect::from_size(800, 600);
        let area = |position: TabBarPosition| position.content_area(window, 30.0);
        let rect = |x, y, width, height| Rect {
            x,
            y,
            width,
            height,
        };
        assert_eq!(area(TabBarPosition::Top), rect(0.0, 30.0, 800.0, 570.0));
        assert_eq!(area(TabBarPosition::Bottom), rect(0.0, 0.0, 800.0, 570.0));
        assert_eq!(area(TabBarPosition::Left), rect(30.0, 0.0, 770.0, 600.0));
        assert_eq!(area(TabBarPosition::Right), rect(0.0, 0.0, 770.0, 600.0));
        for position in [
            TabBarPosition::Top,
            TabBarPosition::Bottom,
            TabBarPosition::Left,
            TabBarPosition::Right,
        ] {
            assert_eq!(position.content_area(window, 0.0), window);
        }
        // A bar thicker than the window leaves an empty area, never a negative one.
        let narrow = Rect::from_size(20, 600);
        assert_eq!(TabBarPosition::Left.content_area(narrow, 30.0).width, 0.0);
    }

    #[test]
    fn tab_bar_area_keeps_a_window_below_the_banner() {
        // The window below a 40 px banner strip.
        let window = Rect {
            x: 0.0,
            y: 40.0,
            width: 800.0,
            height: 560.0,
        };
        let area = |position: TabBarPosition| position.content_area(window, 30.0);
        let rect = |x, y, width, height| Rect {
            x,
            y,
            width,
            height,
        };
        assert_eq!(area(TabBarPosition::Top), rect(0.0, 70.0, 800.0, 530.0));
        assert_eq!(area(TabBarPosition::Bottom), rect(0.0, 40.0, 800.0, 530.0));
        assert_eq!(area(TabBarPosition::Left), rect(30.0, 40.0, 770.0, 560.0));
        assert_eq!(area(TabBarPosition::Right), rect(0.0, 40.0, 770.0, 560.0));
    }

    #[test]
    fn configured_position_maps_to_the_bar_edge() {
        assert_eq!(TabBarPosition::from(TabPosition::Top), TabBarPosition::Top);
        assert_eq!(
            TabBarPosition::from(TabPosition::Bottom),
            TabBarPosition::Bottom
        );
        assert_eq!(
            TabBarPosition::from(TabPosition::Left),
            TabBarPosition::Left
        );
        assert_eq!(
            TabBarPosition::from(TabPosition::Right),
            TabBarPosition::Right
        );
        assert!(TabBarPosition::Left.is_vertical() && TabBarPosition::Right.is_vertical());
        assert!(!TabBarPosition::Top.is_vertical() && !TabBarPosition::Bottom.is_vertical());
    }

    fn broadcast_ids(ws: &Workspace<Fake>, index: usize) -> Vec<u64> {
        ws.tabs()[index].broadcast.iter().map(|id| id.0).collect()
    }

    #[test]
    fn ctrl_click_on_another_pane_broadcasts_to_it_and_the_active_pane() {
        let mut ws = workspace(&[3]);
        let active = ws.active_pane_id().unwrap();
        let other = PaneId((active.0 + 1) % 3);
        assert!(ws.toggle_broadcast_click(other));
        assert!(ws.is_broadcast(active) && ws.is_broadcast(other));
        assert_eq!(ws.broadcast_count(0), 2);
        // Clicking it again removes only that pane.
        assert!(ws.toggle_broadcast_click(other));
        assert!(ws.is_broadcast(active) && !ws.is_broadcast(other));
    }

    #[test]
    fn ctrl_click_on_the_active_pane_does_not_add_anything_else() {
        let mut ws = workspace(&[3]);
        let active = ws.active_pane_id().unwrap();
        assert!(ws.toggle_broadcast_click(active));
        assert_eq!(ws.broadcast_count(0), 1);
        assert!(ws.is_broadcast(active));
    }

    #[test]
    fn ctrl_click_removing_a_pane_does_not_bring_the_active_one() {
        let mut ws = workspace(&[3]);
        let active = ws.active_pane_id().unwrap();
        let other = PaneId((active.0 + 1) % 3);
        ws.toggle_broadcast(other);
        assert!(ws.toggle_broadcast_click(other));
        assert_eq!(ws.broadcast_count(0), 0);
        assert!(!ws.toggle_broadcast_click(PaneId(99)));
    }

    #[test]
    fn a_new_tab_and_a_split_start_without_broadcast() {
        let mut ws = workspace(&[2]);
        assert_eq!(ws.broadcast_count(0), 0);
        ws.toggle_broadcast_all();
        let id = ws.split(Axis::Vertical, geometry(), |_| Some(Fake::default()));
        assert!(!ws.is_broadcast(id.unwrap()));
        assert_eq!(ws.broadcast_count(0), 2);
    }

    #[test]
    fn toggle_broadcast_adds_then_removes_a_pane() {
        let mut ws = workspace(&[2, 1]);
        assert!(ws.toggle_broadcast(PaneId(1)));
        assert!(ws.is_broadcast(PaneId(1)));
        assert!(!ws.is_broadcast(PaneId(0)));
        assert_eq!(ws.broadcast_count(0), 1);
        assert_eq!(ws.broadcast_count(1), 0);
        assert!(ws.toggle_broadcast(PaneId(1)));
        assert!(!ws.is_broadcast(PaneId(1)));
        assert!(!ws.toggle_broadcast(PaneId(99)));
        assert!(!ws.is_broadcast(PaneId(99)));
    }

    #[test]
    fn toggle_broadcast_all_fills_the_shown_tab_then_empties_it() {
        let mut ws = workspace(&[3, 2]);
        assert!(ws.toggle_broadcast_all());
        assert_eq!(broadcast_ids(&ws, 0), vec![0, 1, 2]);
        assert_eq!(ws.broadcast_count(1), 0);
        // A partial set is emptied, not completed.
        assert!(ws.toggle_broadcast_all());
        assert_eq!(ws.broadcast_count(0), 0);
        ws.toggle_broadcast(PaneId(0));
        assert!(ws.toggle_broadcast_all());
        assert_eq!(ws.broadcast_count(0), 0);
        assert!(!Workspace::<Fake>::new().toggle_broadcast_all());
    }

    #[test]
    fn input_targets_is_the_active_pane_alone_without_broadcast() {
        let ws = workspace(&[3]);
        assert_eq!(ws.input_targets(), vec![PaneId(2)]);
        assert!(Workspace::<Fake>::new().input_targets().is_empty());
    }

    #[test]
    fn input_targets_ignores_a_set_the_active_pane_is_not_in() {
        let mut ws = workspace(&[3]);
        ws.toggle_broadcast(PaneId(0));
        ws.toggle_broadcast(PaneId(1));
        assert_eq!(ws.input_targets(), vec![PaneId(2)]);
    }

    #[test]
    fn input_targets_puts_the_active_pane_first() {
        let mut ws = workspace(&[3]);
        ws.toggle_broadcast_all();
        assert_eq!(ws.input_targets(), vec![PaneId(2), PaneId(0), PaneId(1)]);
        ws.focus(PaneId(1));
        assert_eq!(ws.input_targets(), vec![PaneId(1), PaneId(0), PaneId(2)]);
    }

    #[test]
    fn input_targets_only_look_at_the_shown_tab_and_survive_a_zoom() {
        let mut ws = workspace(&[2, 2]);
        ws.toggle_broadcast_all();
        ws.toggle_zoom();
        assert_eq!(ws.input_targets().len(), 2);
        ws.select(1);
        assert_eq!(ws.input_targets(), vec![PaneId(3)]);
    }

    #[test]
    fn closing_a_pane_removes_it_from_the_set() {
        let mut ws = workspace(&[3]);
        ws.toggle_broadcast_all();
        assert_eq!(ws.close_pane(PaneId(1)), Ok(()));
        assert_eq!(broadcast_ids(&ws, 0), vec![0, 2]);
    }

    #[test]
    fn detaching_a_pane_removes_it_from_the_set() {
        let mut ws = workspace(&[3]);
        ws.toggle_broadcast_all();
        assert!(ws.detach(PaneId(1)));
        assert_eq!(broadcast_ids(&ws, 0), vec![0, 2]);
        assert_eq!(ws.broadcast_count(1), 0);
    }

    #[test]
    fn moving_a_pane_across_tabs_leaves_the_source_set_and_joins_without_broadcast() {
        let mut ws = workspace(&[2, 2]);
        ws.toggle_broadcast_all();
        ws.select(1);
        ws.toggle_broadcast(PaneId(2));
        assert!(ws.move_pane(PaneId(0), PaneId(2), Direction::Right, geometry()));
        assert_eq!(broadcast_ids(&ws, 0), vec![1]);
        assert_eq!(broadcast_ids(&ws, 1), vec![2]);
        assert!(!ws.is_broadcast(PaneId(0)));
    }

    #[test]
    fn moving_a_pane_within_its_tab_keeps_the_set() {
        let mut ws = workspace(&[3]);
        ws.toggle_broadcast(PaneId(0));
        ws.toggle_broadcast(PaneId(2));
        assert!(ws.move_pane(PaneId(2), PaneId(0), Direction::Left, geometry()));
        assert_eq!(broadcast_ids(&ws, 0), vec![0, 2]);
    }

    #[test]
    fn moving_the_last_pane_of_a_tab_does_not_carry_its_set_over() {
        let mut ws = workspace(&[2, 1]);
        ws.select(1);
        ws.toggle_broadcast(PaneId(2));
        assert!(ws.move_pane(PaneId(2), PaneId(0), Direction::Right, geometry()));
        assert_eq!(ws.tabs().len(), 1);
        assert_eq!(ws.broadcast_count(0), 0);
    }

    #[test]
    fn swapping_across_tabs_removes_both_panes_from_both_sets() {
        let mut ws = workspace(&[3, 3]);
        // Tabs: [0, 1, 2] and [3, 4, 5].
        ws.toggle_broadcast_all();
        ws.select(1);
        ws.toggle_broadcast_all();
        assert!(ws.swap_panes(PaneId(0), PaneId(3)));
        assert_eq!(broadcast_ids(&ws, 0), vec![1, 2]);
        assert_eq!(broadcast_ids(&ws, 1), vec![4, 5]);
    }

    #[test]
    fn swapping_within_a_tab_keeps_the_set() {
        let mut ws = workspace(&[3]);
        ws.toggle_broadcast_all();
        assert!(ws.swap_panes(PaneId(0), PaneId(1)));
        assert_eq!(broadcast_ids(&ws, 0), vec![0, 1, 2]);
    }

    #[test]
    fn moving_a_pane_to_a_new_tab_leaves_the_set_behind() {
        let mut ws = workspace(&[3]);
        ws.toggle_broadcast_all();
        assert!(ws.move_pane_to_new_tab(PaneId(1)));
        assert_eq!(broadcast_ids(&ws, 0), vec![0, 2]);
        assert_eq!(ws.broadcast_count(1), 0);
    }

    #[test]
    fn moving_a_single_pane_tab_clears_its_set() {
        let mut ws = workspace(&[1, 1]);
        ws.toggle_broadcast(PaneId(0));
        assert!(ws.move_pane_to_new_tab(PaneId(0)));
        assert_eq!(ws.broadcast_count(0), 0);
        assert_eq!(ws.broadcast_count(1), 0);
        assert!(!ws.is_broadcast(PaneId(0)));
    }
}
