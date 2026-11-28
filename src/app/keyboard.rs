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
        // winit sends no key during a composition; one that comes anyway belongs to the
        // input method, not to the terminal.
        if self.ime.is_composing() {
            return;
        }
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
        // `-E`: once the program ended, any key but a modifier alone closes the pane,
        // shortcuts included.
        if let Some(id) = self.workspace.active_pane_id()
            && self
                .workspace
                .active_pane()
                .is_some_and(|pane| pane.hold == Hold::Waiting)
            && closes_hold(&event.logical_key)
        {
            self.close_pane(id, event_loop);
            return;
        }
        if self.search_key(event, event_loop) {
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

    /// What has the keyboard now, which a composition of the input method belongs to.
    pub(super) fn ime_target(&self) -> ImeTarget {
        match &self.overlay {
            Some(Overlay::Palette(_)) => return ImeTarget::Palette,
            Some(_) => return ImeTarget::Other,
            None => {}
        }
        if self.settings_shown() {
            return ImeTarget::Other;
        }
        match self.workspace.active_pane_id() {
            Some(id) if self.workspace.pane(id).is_some_and(Pane::search_focused) => {
                ImeTarget::Search(id)
            }
            Some(id) => ImeTarget::Pane(id),
            None => ImeTarget::Other,
        }
    }

    /// The text being composed by the input method for what has the keyboard now.
    pub(super) fn preedit(&self) -> Option<&ime::Preedit> {
        self.ime.preedit()
    }

    /// Handles an event of the input method: the composition is shown, the committed
    /// text delivered.
    pub(super) fn ime_event(&mut self, event: Ime) {
        let target = self.ime_target();
        let effect = self.ime.event(event, target);
        if let Some(text) = effect.commit {
            self.reset_blink();
            self.commit_text(&text);
        }
        if effect.redraw {
            self.window.request_redraw();
        }
    }

    /// Gives the input method the cell where its composition starts in the active pane,
    /// for its candidate window, when it changed since it was given last. Nothing is
    /// given while the input method is off, the settings tab has it, or something else
    /// than a pane has the keyboard.
    pub(super) fn place_ime_area(&mut self) {
        if !self.ime_allowed || self.settings_shown() {
            return;
        }
        let ImeTarget::Pane(id) = self.ime_target() else {
            return;
        };
        let area = self
            .frames
            .get(id)
            .zip(self.workspace.pane(id))
            .and_then(|(frame, pane)| ime::CursorArea::of_cursor(frame, &pane.layout));
        if let Some(area) = area
            && area.replace(&mut self.ime_area)
        {
            self.window.set_ime_cursor_area(
                PhysicalPosition::new(area.x, area.y),
                PhysicalSize::new(area.width, area.height),
            );
        }
    }

    /// Drops the composition of the input method, if any, and redraws without it.
    pub(super) fn clear_composition(&mut self) {
        if self.ime.clear() {
            self.window.request_redraw();
        }
    }

    /// Drops the composition once what it was typed into lost the keyboard: another
    /// pane, an overlay opened or closed, the search bar.
    pub(super) fn follow_ime_target(&mut self) {
        let target = self.ime_target();
        if self.ime.follow(&target) {
            self.window.request_redraw();
        }
    }

    /// Allows the input method in the terminal or not (`[keyboard] ime`). The settings
    /// tab, while shown, keeps it until it is left.
    pub(super) fn set_ime_allowed(&mut self, allowed: bool) {
        self.ime_allowed = allowed;
        self.ime_area = None;
        if !self.settings_shown() {
            self.window.set_ime_allowed(allowed);
        }
        if !allowed {
            self.clear_composition();
        }
    }

    /// Delivers `text`, committed by the input method, as typed text: to the search
    /// field of the active pane when it has the keyboard, otherwise to every input target
    /// (see [`ime::commit_to_panes`]). It runs no shortcut. The overlays take no text
    /// from the input method.
    pub(super) fn commit_text(&mut self, text: &str) {
        if self.overlay.is_some() || self.pane_dragging() {
            return;
        }
        if let ImeTarget::Search(id) = self.ime_target() {
            if self.edit_search(id, |field| field.insert(text)) {
                let revision = self.workspace.revision(id);
                self.with_pane(id, |pane| pane.search_edited(revision));
            }
            return;
        }
        ime::commit_to_panes(&mut self.workspace, text, self.modifiers);
        self.window.request_redraw();
    }

    /// Handles a key while the search bar of the active pane has the keyboard: `Enter`
    /// and `Shift+Enter` go to the older and the more recent match, `Escape` closes the
    /// bar, the search and paste shortcuts act on the field, the other shortcuts run as
    /// usual and the other keys edit the field. Nothing reaches the shell. Returns
    /// whether the bar took the key.
    fn search_key(&mut self, event: &KeyEvent, event_loop: &ActiveEventLoop) -> bool {
        let Some(id) = self.workspace.active_pane_id() else {
            return false;
        };
        if !self.workspace.pane(id).is_some_and(Pane::search_focused) {
            return false;
        }
        let modifiers = self.modifiers;
        let action = self
            .keybindings
            .action(&event.logical_key, event.physical_key, modifiers);
        let changed = match pane_search::bar_key(&event.logical_key, modifiers, action) {
            BarKey::Close => {
                self.with_pane(id, Pane::close_search);
                false
            }
            BarKey::Older => {
                self.with_pane(id, |pane| pane.search_step(true));
                false
            }
            BarKey::Newer => {
                self.with_pane(id, |pane| pane.search_step(false));
                false
            }
            BarKey::SelectAll => {
                self.with_pane(id, |pane| {
                    pane.open_search();
                });
                false
            }
            BarKey::Paste => {
                let text = self.clipboard.as_mut().and_then(|c| c.get_text().ok());
                text.is_some_and(|text| self.edit_search(id, |field| field.insert(&text)))
            }
            BarKey::Shortcut(action) => {
                self.run_action(action, event_loop);
                false
            }
            BarKey::Edit => {
                let ctrl = modifiers.control_key();
                self.edit_search(id, |field| {
                    field.key(&event.logical_key, event.text.as_deref(), ctrl)
                })
            }
        };
        if changed {
            let revision = self.workspace.revision(id);
            self.with_pane(id, |pane| pane.search_edited(revision));
        }
        true
    }

    /// Edits the search field of the pane `id` with `edit`, which tells whether the
    /// text changed, then redraws.
    fn edit_search(&mut self, id: PaneId, edit: impl FnOnce(&mut SearchField) -> bool) -> bool {
        let Some(pane) = self.workspace.pane_mut(id) else {
            return false;
        };
        let Some(search) = pane.search.as_mut() else {
            return false;
        };
        let changed = edit(&mut search.field);
        self.window.request_redraw();
        changed
    }

    /// Runs `f` on the pane `id`, then redraws.
    pub(super) fn with_pane(&mut self, id: PaneId, f: impl FnOnce(&mut Pane)) {
        if let Some(pane) = self.workspace.pane_mut(id) {
            f(pane);
            self.window.request_redraw();
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

/// What has the keyboard, which a composition of the input method belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ImeTarget {
    /// The shell of a pane.
    Pane(PaneId),
    /// The search bar of a pane.
    Search(PaneId),
    /// The command palette.
    Palette,
    /// Something that takes no text: a dialog, a menu, the settings tab, no pane.
    Other,
}

/// Whether `key` closes a pane waiting after its program (`-E`): every key but a
/// modifier alone, so a combination is not cut at its first key.
pub(super) fn closes_hold(key: &Key) -> bool {
    !matches!(
        key,
        Key::Named(
            NamedKey::Shift
                | NamedKey::Control
                | NamedKey::Alt
                | NamedKey::AltGraph
                | NamedKey::Super
                | NamedKey::Meta
                | NamedKey::Hyper
                | NamedKey::CapsLock
                | NamedKey::NumLock
                | NamedKey::Fn
                | NamedKey::FnLock
        )
    )
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
        Action::OpenSearch => {
            pane.open_search();
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_key_but_a_modifier_alone_closes_a_waiting_pane() {
        for key in [
            Key::Character("a".into()),
            Key::Character("C".into()),
            Key::Named(NamedKey::Enter),
            Key::Named(NamedKey::Escape),
            Key::Named(NamedKey::Space),
            Key::Named(NamedKey::F5),
        ] {
            assert!(closes_hold(&key), "{key:?}");
        }
        for key in [
            NamedKey::Shift,
            NamedKey::Control,
            NamedKey::Alt,
            NamedKey::Super,
            NamedKey::CapsLock,
        ] {
            assert!(!closes_hold(&Key::Named(key)), "{key:?}");
        }
    }
}
