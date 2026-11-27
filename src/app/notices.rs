//! Desktop notifications of a session: the notices of the panes, their unseen counts
//! and the window's badge.

use super::*;

impl Session {
    /// Raises `notice` from the pane `id` when the configuration wants it and the user
    /// is not looking at that pane: one more unseen notice for the pane, and its
    /// notification.
    pub(super) fn notify(&mut self, id: PaneId, notice: Notice) {
        if !notifications::wanted(&notice, &self.notifications)
            || notifications::watched(self.focused, self.workspace.active_pane_id(), id)
        {
            return;
        }
        let title = self.workspace.pane_title(id);
        let Some(pane) = self.workspace.pane_mut(id) else {
            return;
        };
        pane.add_unseen();
        let (title, body) = notifications::content(&notice, title.as_deref(), self.lang);
        self.notifier.send(NotificationCommand::Show {
            pane: id,
            title: title.into(),
            body: body.into(),
        });
        self.sync_badge();
        // The counts are drawn on the tab and pane bars.
        self.window.request_redraw();
    }

    /// Resets the unseen notices of the active pane when the window has the focus: the
    /// user is now looking at it.
    pub(super) fn clear_watched(&mut self) {
        if self.focused
            && let Some(id) = self.workspace.active_pane_id()
        {
            self.clear_unseen(id);
        }
    }

    /// Resets the unseen notices of the pane `id` and closes its notification.
    fn clear_unseen(&mut self, id: PaneId) {
        let Some(command) = self.workspace.pane_mut(id).and_then(Pane::clear_unseen) else {
            return;
        };
        self.notifier.send(command);
        self.sync_badge();
        self.window.request_redraw();
    }

    /// Sends the unseen notices of every pane to the badge of the window's icon, when
    /// their total changed.
    pub(super) fn sync_badge(&mut self) {
        let total = self
            .workspace
            .panes()
            .map(|(_, pane)| pane.unseen())
            .fold(0, u32::saturating_add);
        if let Some(command) = self.badge.sync(total) {
            self.notifier.send(command);
        }
    }

    /// The user clicked the notification of the pane `id`: shows that pane, if still
    /// open, and brings the window forward.
    pub(super) fn notification_clicked(&mut self, id: PaneId) {
        if self.workspace.pane(id).is_some() {
            self.focus_pane(id);
            self.clear_unseen(id);
        }
        self.window.focus_window();
    }
}
