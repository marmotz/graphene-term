//! Dragging a pane to another place of the workspace.

use super::*;

impl Session {
    /// Auto-scroll while dragging a selection past the top or bottom edge of its pane:
    /// once per `AUTO_SCROLL_INTERVAL`, scrolls the timeline and moves the selection end
    /// onto the edge row. Returns when the next step is due, `None` when auto-scroll is
    /// idle.
    /// Follows the pointer with the grabbed pane: starts the drag past the threshold,
    /// then looks the drop target up.
    pub(super) fn pane_drag_moved(&mut self) {
        let Some(Drag::Pane { press, started, .. }) = self.drag else {
            return;
        };
        if !started {
            let scale = self.window.scale_factor() as f32;
            let press = (press.x as f32, press.y as f32);
            if !pane_bar::drag_started(press, self.mouse_pixel(), scale) {
                return;
            }
            self.start_pane_drag();
        }
        self.update_pane_drop(Instant::now());
        self.window.request_redraw();
    }

    /// Starts dragging the grabbed pane: its tab is unzoomed, and the tab bar shown
    /// even for a single tab, the panes being laid out again.
    pub(super) fn start_pane_drag(&mut self) {
        let zoomed = self.workspace.active_tab().is_some_and(|tab| tab.zoomed);
        if zoomed {
            self.workspace.toggle_zoom();
        }
        if let Some(Drag::Pane {
            started,
            was_zoomed,
            ..
        }) = &mut self.drag
        {
            *started = true;
            *was_zoomed = zoomed;
        }
        self.relayout();
        self.update_pointer();
    }

    /// Looks up the drop target under the pointer, and the tab it hovers, at `now`.
    pub(super) fn update_pane_drop(&mut self, now: Instant) {
        let Some(Drag::Pane {
            pane,
            started: true,
            ..
        }) = self.drag
        else {
            return;
        };
        let (x, y) = self.mouse_pixel();
        let layout = self.shown_layout();
        let bar = self.tab_bar();
        let geometry = self.geometry();
        let found = drop::target(&layout, bar.as_ref(), pane, x, y, |target, side| {
            self.workspace.can_move_pane(pane, target, side, geometry)
        });
        let over = match found {
            Some(DropTarget::Tab(index)) => Some(index),
            _ => None,
        };
        // Dropping a pane on its own tab changes nothing: no target there.
        let own_tab = self.workspace.tab_of(pane);
        let found = found
            .filter(|found| !matches!(found, DropTarget::Tab(index) if Some(*index) == own_tab));
        let shown = self.workspace.active_index();
        if let Some(Drag::Pane {
            target, tab_hover, ..
        }) = &mut self.drag
        {
            *target = found;
            *tab_hover = drop::tab_hover(*tab_hover, over, shown, now);
        }
    }

    /// Shows the tab the dragged pane hovers once it has hovered it long enough, the
    /// drag going on. Returns when that is due, `None` without hovered tab.
    pub(super) fn pane_drag_tick(&mut self, now: Instant) -> Option<Instant> {
        let Some(Drag::Pane { tab_hover, .. }) = self.drag else {
            return None;
        };
        let deadline = drop::tab_hover_deadline(tab_hover)?;
        if now < deadline {
            return Some(deadline);
        }
        let (index, _) = tab_hover?;
        if let Some(Drag::Pane { tab_hover, .. }) = &mut self.drag {
            *tab_hover = None;
        }
        self.change_tab(|workspace| workspace.select(index));
        self.update_pane_drop(now);
        self.window.request_redraw();
        None
    }

    /// Ends a pane drag on release: the pane is moved onto its target; without target
    /// or when the move is refused, it is put back as on cancel. A press released
    /// before the threshold was a click: nothing happens.
    pub(super) fn drop_pane(&mut self) {
        let geometry = self.geometry();
        let Some(Drag::Pane {
            pane,
            started,
            was_zoomed,
            target,
            ..
        }) = self.drag.take()
        else {
            return;
        };
        if !started {
            return;
        }
        let old = self.workspace.active_pane_id();
        let moved = match target {
            Some(DropTarget::Edge { pane: other, side }) => {
                self.workspace.move_pane(pane, other, side, geometry)
            }
            Some(DropTarget::Center { pane: other }) => self.workspace.swap_panes(pane, other),
            Some(DropTarget::Tab(index)) => self.workspace.move_pane_to_tab(pane, index, geometry),
            Some(DropTarget::NewTab) => self.workspace.move_pane_to_new_tab(pane),
            None => false,
        };
        if moved {
            // Also hides the tab bar shown for the drag of a single tab's pane.
            self.focus_changed(old);
        } else {
            self.restore_dragged_pane(pane, was_zoomed);
        }
    }

    /// Cancels the pane drag in progress, if any: the pane gets the focus again, its
    /// tab shown and zoomed back when it was.
    pub(super) fn cancel_pane_drag(&mut self) {
        let Some(Drag::Pane {
            pane,
            started,
            was_zoomed,
            ..
        }) = self.drag
        else {
            return;
        };
        self.drag = None;
        if started {
            self.restore_dragged_pane(pane, was_zoomed);
        }
    }

    /// Puts the workspace back as before the drag of `pane`.
    pub(super) fn restore_dragged_pane(&mut self, pane: PaneId, was_zoomed: bool) {
        let old = self.workspace.active_pane_id();
        self.workspace.focus(pane);
        if was_zoomed && !self.workspace.active_tab().is_some_and(|tab| tab.zoomed) {
            self.workspace.toggle_zoom();
        }
        self.focus_changed(old);
    }
}
