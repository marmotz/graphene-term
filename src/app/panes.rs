//! Panes and tabs of a session: focus, split, creation and closing.

use super::*;

impl Session {
    /// Gives the keyboard focus to the pane `id`.
    pub(super) fn focus_pane(&mut self, id: PaneId) {
        let old = self.workspace.active_pane_id();
        if self.workspace.focus(id) {
            self.focus_changed(old);
        }
    }

    /// After the active pane may have changed from `old`: reports the focus change to
    /// both panes when they asked for it and the window has the focus, lays the tabs
    /// out again (a focus change unzooms) and refreshes the title and the pointer.
    pub(super) fn focus_changed(&mut self, old: Option<PaneId>) {
        let new = self.workspace.active_pane_id();
        if old != new && self.focused {
            for (id, focused) in [(old, false), (new, true)] {
                if let Some(pane) = id.and_then(|id| self.workspace.pane(id))
                    && let Some(bytes) = mouse::focus_report(focused, pane.mode())
                {
                    pane.terminal.write(bytes);
                }
            }
        }
        self.relayout();
        self.refresh_title();
        self.update_pointer();
        // The new active pane is now watched.
        self.clear_watched();
    }

    /// What a new pane is spawned from: the directory its shell starts in (the active
    /// pane's, else the home directory) and a first layout, the active pane's (the new
    /// pane is laid out for real by `relayout`). `None` without active pane.
    pub(super) fn new_pane_start(&self) -> Option<(Option<PathBuf>, GridLayout)> {
        let active = self.workspace.active_pane()?;
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let cwd = workspace::start_dir(active.terminal.current_dir(), home);
        Some((cwd, active.layout))
    }

    /// A closure spawning the pane of a given id, starting in `cwd` with `layout`.
    pub(super) fn pane_spawner(
        &self,
        cwd: Option<PathBuf>,
        layout: GridLayout,
    ) -> impl FnOnce(PaneId) -> Option<Pane> + use<> {
        let (proxy, config) = (self.proxy.clone(), self.term_config.clone());
        move |id| match Terminal::spawn(
            EventProxy::new(proxy, id),
            layout.window_size(),
            config,
            cwd,
            None,
        ) {
            Ok(terminal) => Some(Pane::new(id, terminal, layout)),
            Err(error) => {
                log::error!("cannot start a shell for the new pane: {error:#}");
                None
            }
        }
    }

    /// Splits the active pane along `axis`; the new pane takes the focus.
    pub(super) fn split(&mut self, axis: Axis) {
        let Some((cwd, layout)) = self.new_pane_start() else {
            return;
        };
        let old = self.workspace.active_pane_id();
        let spawn = self.pane_spawner(cwd, layout);
        if self.workspace.split(axis, self.geometry(), spawn).is_some() {
            self.focus_changed(old);
        }
    }

    /// Opens a tab after the last one and shows it.
    pub(super) fn new_tab(&mut self) {
        self.leave_settings();
        let Some((cwd, layout)) = self.new_pane_start() else {
            return;
        };
        let old = self.workspace.active_pane_id();
        let id = self.workspace.allocate_id();
        if let Some(pane) = self.pane_spawner(cwd, layout)(id) {
            self.workspace.new_tab(id, pane);
            self.focus_changed(old);
        }
    }

    /// Shows the tab chosen by `select` on the workspace, when it changes the shown tab;
    /// the settings tab is left for it in any case.
    pub(super) fn change_tab(&mut self, select: impl FnOnce(&mut Workspace<Pane>) -> bool) {
        self.leave_settings();
        let (old, index) = (
            self.workspace.active_pane_id(),
            self.workspace.active_index(),
        );
        if select(&mut self.workspace) && self.workspace.active_index() != index {
            self.focus_changed(old);
        }
    }

    /// Asks to close `target`: at once when none of its panes runs a program other
    /// than its shell, otherwise once the user confirms in a dialog.
    /// Writes the window size and maximized state to `window.json`, whatever the startup
    /// mode, so that switching to `remember` later restores the last window. A failure
    /// is only logged. The position is not saved.
    pub(super) fn save_window_state(&self) {
        let Some(file) = window_state::path() else {
            return;
        };
        let state = self.window_state();
        if let Err(error) = state.save(&file) {
            log::warn!("cannot write {}: {error}", file.display());
        }
    }

    /// Size and maximized state of the window; the size is the last one seen while not
    /// maximized.
    fn window_state(&self) -> WindowState {
        WindowState {
            width: self.restore_size.width,
            height: self.restore_size.height,
            maximized: self.window.is_maximized(),
        }
    }

    /// What the session saves of this window, `closed_at` being the Unix seconds of a
    /// clean exit. The placement is `None` where the platform does not give the
    /// position (Wayland).
    pub(super) fn window_snapshot(&self, closed_at: Option<u64>) -> WindowSnapshot {
        let (tabs, active_tab) = self.workspace.snapshot();
        let placement = self.window.outer_position().ok().map(|position| Placement {
            x: position.x,
            y: position.y,
            monitor: self
                .window
                .current_monitor()
                .and_then(|monitor| monitor.name()),
        });
        WindowSnapshot {
            version: SESSION_VERSION,
            kind: if self.quick {
                WindowKind::Quick
            } else {
                WindowKind::Normal
            },
            window: self.window_state(),
            placement,
            tabs,
            active_tab,
            closed_at,
        }
    }

    pub(super) fn request_close(&mut self, target: Target, event_loop: &ActiveEventLoop) {
        let programs: Vec<_> = target
            .panes(&self.workspace)
            .into_iter()
            .map(|id| {
                self.workspace
                    .pane(id)
                    .and_then(|pane| pane.terminal.foreground_program())
            })
            .collect();
        match ConfirmClose::request(target.clone(), programs) {
            Some(confirm) => {
                self.overlay = Some(Overlay::Confirm(confirm));
                self.window.request_redraw();
                self.update_pointer();
            }
            None => self.close(&target, event_loop),
        }
    }

    /// Closes `target` without asking: the window, or each of its panes still open;
    /// the window ends with the last pane (see [`Session::end_window`]).
    pub(super) fn close(&mut self, target: &Target, event_loop: &ActiveEventLoop) {
        if *target == Target::Window {
            self.end_window(event_loop);
            return;
        }
        for id in target.panes(&self.workspace) {
            if event_loop.exiting() || self.quick_closed {
                return;
            }
            self.close_pane(id, event_loop);
        }
    }

    /// Closes the pane `id`; the window ends with the last one (see
    /// [`Session::end_window`]).
    pub(super) fn close_pane(&mut self, id: PaneId, event_loop: &ActiveEventLoop) {
        // The closed pane's selection, and a divider of the tree it leaves, are dropped.
        // A dragged tab may be the one closing: the drag is dropped. A dragged pane is
        // put back, whichever pane closes: the tab indices it holds may shift.
        if matches!(self.drag, Some(Drag::Pane { .. })) {
            self.cancel_pane_drag();
        }
        let shown_tab = self.workspace.tab_of(id) == Some(self.workspace.active_index());
        match self.drag {
            Some(Drag::Selection { pane, .. } | Drag::BroadcastClick { pane, .. })
                if pane == id =>
            {
                self.drag = None
            }
            Some(Drag::Divider { .. }) if shown_tab => self.drag = None,
            Some(Drag::Tab { .. }) => self.drag = None,
            _ => {}
        }
        let old = self.workspace.active_pane_id();
        if self.workspace.close_pane(id).is_err() {
            self.end_window(event_loop);
            return;
        }
        // The closed pane's unseen notices leave the window's total.
        self.sync_badge();
        // A dialog about panes that are all gone has nothing left to close.
        if let Some(Overlay::Confirm(confirm)) = &self.overlay
            && confirm.target != Target::Window
            && confirm.target.panes(&self.workspace).is_empty()
        {
            self.overlay = None;
        }
        // The new active pane gets the focus-in; the closed one is gone.
        self.focus_changed(old);
    }
}
