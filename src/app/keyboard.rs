//! Key presses and the actions they run.

use super::*;

impl Session {
    pub(super) fn run_action(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        match action {
            Action::NewTab => self.new_tab(),
            Action::CloseTab => {
                if let Some(target) = Target::tab(&self.workspace, self.workspace.active_index()) {
                    self.request_close(target, event_loop);
                }
            }
            Action::Quit => self.request_close(Target::Window, event_loop),
            Action::CommandPalette => self.open_palette(),
            Action::OpenSettings => self.settings_requested = true,
            Action::NextTab => self.change_tab(|workspace| {
                workspace.next();
                true
            }),
            Action::PreviousTab => self.change_tab(|workspace| {
                workspace.previous();
                true
            }),
            Action::GoToTab(number) => {
                let index = usize::from(number).saturating_sub(1);
                self.change_tab(|workspace| workspace.select(index));
            }
            Action::SplitRight => self.split(Axis::Horizontal),
            Action::SplitDown => self.split(Axis::Vertical),
            Action::ClosePane => {
                if let Some(id) = self.workspace.active_pane_id() {
                    self.request_close(Target::Pane(id), event_loop);
                }
            }
            Action::FocusPane(direction) => {
                if let Some(id) = self.workspace.neighbour(direction, self.geometry()) {
                    self.focus_pane(id);
                }
            }
            Action::NextPane => {
                if let Some(id) = self.workspace.next_pane() {
                    self.focus_pane(id);
                }
            }
            Action::PreviousPane => {
                if let Some(id) = self.workspace.previous_pane() {
                    self.focus_pane(id);
                }
            }
            Action::ResizePane(direction) => {
                let geometry = self.geometry();
                if self
                    .workspace
                    .resize_active(direction, RESIZE_STEP_CELLS, geometry)
                {
                    self.relayout();
                }
            }
            Action::ToggleZoom => {
                if self.workspace.toggle_zoom() {
                    self.relayout();
                    self.update_pointer();
                }
            }
            Action::ToggleBroadcastAll => {
                if self.workspace.toggle_broadcast_all() {
                    self.window.request_redraw();
                }
            }
            Action::EqualisePanes => {
                if self.workspace.equalise() {
                    self.relayout();
                    self.update_pointer();
                }
            }
            Action::DetachPane => {
                if let Some(id) = self.workspace.active_pane_id()
                    && self.workspace.detach(id)
                {
                    self.relayout();
                    self.refresh_title();
                    self.update_pointer();
                }
            }
            Action::RenameTab => self.open_rename_tab(self.workspace.active_index()),
            Action::Copy => self.copy(),
            Action::Paste => self.paste(),
            Action::FontSizeIncrease => self.step_active_pane_font(1.0),
            Action::FontSizeDecrease => self.step_active_pane_font(-1.0),
            Action::FontSizeReset => {
                if let Some(id) = self.workspace.active_pane_id() {
                    self.set_pane_font_offset(id, 0.0);
                }
            }
            Action::FontSizeResetAll => self.reset_all_pane_fonts(),
            Action::CopyCommand => self.copy_block_part(Part::Command),
            Action::CopyOutput => self.copy_block_part(Part::Output),
            _ => {
                if let Some(pane) = self.workspace.active_pane_mut() {
                    run_pane_action(pane, action);
                }
            }
        }
    }

    pub(super) fn key_press(&mut self, event: &KeyEvent, event_loop: &ActiveEventLoop) {
        self.reset_blink();
        // While an overlay is open, keys go to it only.
        match &mut self.overlay {
            Some(Overlay::Confirm(_)) => {
                if let Some(answer) = dialog::answer_for(&event.logical_key) {
                    self.answer_dialog(answer, event_loop);
                }
                return;
            }
            Some(Overlay::Palette(palette)) => {
                match palette.key(&event.logical_key, event.text.as_deref()) {
                    Some(outcome) => self.palette_outcome(outcome, event_loop),
                    None => self.window.request_redraw(),
                }
                return;
            }
            Some(Overlay::RenameTab(rename)) => {
                match rename.key(&event.logical_key, event.text.as_deref()) {
                    Some(outcome) => self.rename_tab_outcome(outcome),
                    None => self.window.request_redraw(),
                }
                return;
            }
            Some(Overlay::Menu(menu)) => {
                if let Some(outcome) = menu.key(&event.logical_key) {
                    self.menu_outcome(outcome, event_loop);
                } else {
                    self.window.request_redraw();
                }
                return;
            }
            Some(Overlay::Art) => {
                if matches!(
                    event.logical_key,
                    Key::Named(NamedKey::Escape | NamedKey::Enter)
                ) {
                    self.close_art();
                }
                return;
            }
            None => {}
        }
        // While a pane is dragged, `Escape` cancels the drag and no other key acts, so
        // nothing changes the workspace under it.
        if self.pane_dragging() {
            if event.logical_key == Key::Named(NamedKey::Escape) {
                self.cancel_pane_drag();
            }
            return;
        }
        let modifiers = self.modifiers;
        self.watch_konami(&event.logical_key, modifiers);
        let action = self
            .keybindings
            .action(&event.logical_key, event.physical_key, modifiers);
        // With several panes, the pane focus wins over the editor's `Ctrl+Shift+Arrow`.
        if let Some(action @ Action::FocusPane(_)) = action
            && self
                .workspace
                .active_tab()
                .is_some_and(|tab| !tab.is_single())
        {
            self.run_action(action, event_loop);
            return;
        }
        // The active pane decides whether the key is a shortcut (run once, never
        // broadcast); otherwise every target types it in its own state.
        let active = self.workspace.active_pane_id();
        for id in self.workspace.input_targets() {
            let delivery = self.deliver_key(id, event, modifiers, action.is_some());
            if delivery == Delivery::Shortcut
                && Some(id) == active
                && let Some(action) = action
            {
                self.run_action(action, event_loop);
                return;
            }
        }
    }

    /// Feeds the Konami detector with a key typed at the prompt of the active pane,
    /// and starts the honeycomb over that pane when the code is complete. Any other
    /// key (with `Ctrl`, `Alt` or `Super`, or outside the prompt) starts the code over.
    /// The key itself is handled as usual.
    fn watch_konami(&mut self, key: &Key, modifiers: ModifiersState) {
        let plain = !(modifiers.control_key() || modifiers.alt_key() || modifiers.super_key());
        let at_prompt = plain
            && self
                .workspace
                .active_pane()
                .is_some_and(|pane| pane.rich_mode(modifiers));
        if !at_prompt {
            self.konami.reset();
            return;
        }
        if self.konami.feed(key) {
            self.start_honeycomb(Instant::now());
        }
    }

    /// Shows the honeycomb over the active pane from `now`, from the start when it is
    /// already shown.
    pub(super) fn start_honeycomb(&mut self, now: Instant) {
        let active = self.workspace.active_pane_id();
        let area = self
            .shown_layout()
            .panes
            .into_iter()
            .find(|&(id, _)| Some(id) == active)
            .map(|(_, rect)| rect);
        if let Some(area) = area {
            self.honeycomb = Some(Honeycomb::new(now, area));
            self.window.request_redraw();
        }
    }

    /// Makes the pane `id` handle a key in its own state and returns what it was.
    /// A shortcut is not run here.
    pub(super) fn deliver_key(
        &mut self,
        id: PaneId,
        event: &KeyEvent,
        modifiers: ModifiersState,
        is_shortcut: bool,
    ) -> Delivery {
        let Some(pane) = self.workspace.pane_mut(id) else {
            return Delivery::Shortcut;
        };
        let rich = pane.rich_mode(modifiers);
        let delivery = input::resolve_key(
            &event.logical_key,
            event.text.as_deref(),
            modifiers,
            rich,
            pane.mode(),
            is_shortcut,
        );
        match &delivery {
            Delivery::Editor(command) => {
                let replaced = input_editor::command_replacing_text(*command)
                    .is_some_and(|typed| pane.replace_selection(typed));
                if !replaced {
                    pane.run_editor_command(*command);
                }
            }
            Delivery::Shortcut => {}
            Delivery::Input { replacing, bytes } => {
                let replaced = replacing
                    .as_ref()
                    .is_some_and(|typed| pane.replace_selection(typed));
                if !replaced && let Some(bytes) = bytes {
                    pane.type_bytes(bytes.clone());
                }
            }
        }
        delivery
    }
}

/// Runs an action that only concerns `pane`; the other actions are ignored.
pub(super) fn run_pane_action(pane: &mut Pane, action: Action) {
    match action {
        Action::ScrollPageUp => pane.scroll_page(1),
        Action::ScrollPageDown => pane.scroll_page(-1),
        Action::ScrollToTop => pane.scroll_to_top(),
        Action::ScrollToBottom => pane.scroll_to_bottom(),
        Action::PreviousBlock => pane.navigate_block(Direction::Previous),
        Action::NextBlock => pane.navigate_block(Direction::Next),
        Action::ToggleFold => pane.toggle_fold(),
        Action::ToggleFoldAll => pane.change_folds(|timeline, _| timeline.toggle_fold_all()),
        Action::Copy
        | Action::Paste
        | Action::FontSizeIncrease
        | Action::FontSizeDecrease
        | Action::FontSizeReset
        | Action::FontSizeResetAll
        | Action::CopyCommand
        | Action::CopyOutput
        | Action::SplitRight
        | Action::SplitDown
        | Action::ClosePane
        | Action::FocusPane(_)
        | Action::ResizePane(_)
        | Action::NextPane
        | Action::PreviousPane
        | Action::ToggleZoom
        | Action::ToggleBroadcastAll
        | Action::EqualisePanes
        | Action::DetachPane
        | Action::RenameTab
        | Action::NewTab
        | Action::CloseTab
        | Action::NextTab
        | Action::PreviousTab
        | Action::GoToTab(_)
        | Action::CommandPalette
        | Action::OpenSettings
        | Action::Quit => {}
    }
}
