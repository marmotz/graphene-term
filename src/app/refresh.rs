//! Keeping a session up to date: configuration, font size, animations, cursor blink and title.

use super::*;

impl Session {
    /// Lays out every tab, hidden ones included, so each shell keeps a correct size;
    /// only panes whose cell count changed resize their shell. Each grid fills the pane
    /// under its title bar. The hidden panes of a zoomed tab keep their last size.
    pub(super) fn relayout(&mut self) {
        let geometry = self.geometry();
        let metrics = self.ui_metrics(UiFont::Chrome);
        let padding = self.padding * self.window.scale_factor() as f32;
        for index in 0..self.workspace.tabs().len() {
            for (id, rect) in self.workspace.layout(index, geometry).panes {
                let (_, content) = pane_bar::split(rect, metrics);
                if let Some(pane) = self.workspace.pane_mut(id) {
                    pane.resize(GridLayout::new(content, geometry.cell, padding));
                }
            }
        }
        self.window.request_redraw();
    }

    /// Shows `palette` until the next reload: the live preview of the theme being
    /// edited in the settings tab.
    pub(super) fn preview_palette(&mut self, palette: &Palette) {
        palette.clone_into(&mut self.palette);
        self.window.request_redraw();
    }

    /// Applies a reloaded configuration, `old` being the one applied so far. The window
    /// size and startup mode only affect the windows created afterwards.
    pub(super) fn apply_config(
        &mut self,
        old: &Config,
        new: &Config,
        palette: &Palette,
        banner: Option<Banner>,
    ) {
        let term_config = new.term_config();
        for (_, pane) in self.workspace.panes_mut() {
            // Lock order: `term`, then `timeline`.
            let mut term = pane.terminal.term.lock();
            term.set_options(term_config.clone());
            pane::lock(&pane.terminal.timeline).set_scrollback(new.scrollback_lines);
        }
        self.term_config = term_config;

        let font_size = reloaded_font_size(&old.font, &new.font, self.font_size);
        if font_changed(&old.font, &new.font) {
            self.font_size = font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
            let scale = self.window.scale_factor() as f32;
            self.renderer
                .set_font(&font_options(&new.font), self.font_size * scale);
        } else if font_size != self.font_size {
            self.apply_font_size(font_size);
        }
        self.padding = new.window.padding;
        self.renderer
            .set_background_opacity(new.window.background_opacity);
        #[cfg(target_os = "macos")]
        self.window
            .set_transparent(window_transparent(new.window.background_opacity, true));
        self.palette = palette.clone();
        // A menu still pointing at a theme previews it again over the new colors.
        self.menu_theme = None;
        self.theme = new.theme.clone();
        self.keybindings = Keybindings::new(&new.keybindings);
        self.lang = new.language.resolve(Lang::detect);
        self.notifications = new.notifications.clone();
        self.tab_position = new.tab_position.into();
        self.workspace.set_progress_shown(new.window.progress);
        self.config_banner = banner;
        // A changed cursor style shows at once, visible.
        self.reset_blink();
        self.relayout();
    }

    pub(super) fn apply_font_size(&mut self, font_size: f32) {
        self.font_size = font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        let scale = self.window.scale_factor() as f32;
        self.renderer.set_font_size(self.font_size * scale);
        self.renderer.set_ui_scale(scale);
        self.relayout();
    }

    /// Applies what the panes asked for since the last call: a redraw when one of the
    /// visible ones changed, their text for the primary selection.
    /// Shows the due frames of the animated images of the shown panes, and requests a
    /// redraw when one changed. Returns when the next frame is due, `None` when no
    /// shown pane plays an animation.
    pub(super) fn animate(&mut self, now: Instant) -> Option<Instant> {
        let shown = self.workspace.active_tab().cloned();
        let mut advanced = Vec::new();
        let mut deadline = None;
        for (id, pane) in self.workspace.panes() {
            if !shown.as_ref().is_some_and(|tab| tab.shows(id)) {
                continue;
            }
            let mut images = graphics::lock(&pane.terminal.images);
            if images.advance(now) {
                advanced.push(id);
            }
            deadline = earliest(deadline, images.next_frame_deadline(now));
        }
        if !advanced.is_empty() {
            for id in advanced {
                self.workspace.touch(id);
            }
            self.window.request_redraw();
        }
        deadline
    }

    /// Requests a redraw while a fold chevron of a shown pane is animated. Returns when
    /// its next frame is due, `None` when no chevron is animated.
    pub(super) fn fold_animate(&mut self, now: Instant) -> Option<Instant> {
        let shown = self.workspace.active_tab().cloned();
        let deadline = self
            .workspace
            .panes()
            .filter(|(id, _)| shown.as_ref().is_some_and(|tab| tab.shows(*id)))
            .filter_map(|(_, pane)| pane.fold_animations.borrow().deadline(now))
            .min();
        if deadline.is_some() {
            self.window.request_redraw();
        }
        deadline
    }

    /// Drops the progress of the panes that reported none for too long, and requests a
    /// redraw for them, then for every frame while an indeterminate progress is on
    /// screen. Returns the earliest of the next expiry and the next animation frame,
    /// `None` without any progress.
    pub(super) fn progress_tick(&mut self, now: Instant) -> Option<Instant> {
        let mut expired = Vec::new();
        let mut deadline = None;
        let mut animated = false;
        for (id, pane) in self.workspace.panes_mut() {
            if pane.expire_progress(now) {
                expired.push(id);
            }
        }
        for (id, pane) in self.workspace.panes() {
            let Some(progress) = pane.progress() else {
                continue;
            };
            deadline = earliest(deadline, Some(progress.expires_at()));
            animated |= progress.state == ProgressState::Indeterminate
                && self.workspace.progress_on_screen(id);
        }
        let redraw = expired
            .iter()
            .any(|&id| self.workspace.progress_on_screen(id));
        for id in expired {
            self.workspace.touch(id);
        }
        if animated {
            deadline = earliest(deadline, Some(now + progress::INDETERMINATE_FRAME));
        }
        if redraw || animated {
            self.window.request_redraw();
        }
        deadline
    }

    /// Hands the progress of the active tab to the taskbar, after each round of events:
    /// a report, an expiry, a change of tab or of active pane, or the `[window]
    /// progress` setting turned off. Only a change reaches the desktop.
    pub(super) fn sync_taskbar(&mut self) {
        let progress = self.workspace.tab_progress(self.workspace.active_index());
        // Colors first: a backend drawing its own bar needs them for the first progress.
        self.taskbar.set_colors(self.progress_colors());
        self.taskbar.set_progress(progress);
    }

    /// `progress` as drawn in the frame at `now`.
    pub(super) fn progress_view(&self, progress: Progress, now: Instant) -> ProgressView {
        ProgressView::new(progress, now.saturating_duration_since(self.progress_epoch))
    }

    /// At the next local midnight, computes the special day again and updates the window
    /// title. Returns the next midnight, `None` if the local time is unknown.
    pub(super) fn special_day_tick(&mut self, now: Instant) -> Option<Instant> {
        let midnight = self.midnight?;
        if now < midnight {
            return Some(midnight);
        }
        self.today = special_days::today();
        self.refresh_title();
        self.midnight = special_days::until_midnight().map(|left| now + left);
        self.midnight
    }

    /// Requests a redraw while the honeycomb is shown, and once more when it ends to
    /// clear it. Returns when its next frame is due, `None` when it is not shown.
    pub(super) fn honeycomb_tick(&mut self, now: Instant) -> Option<Instant> {
        let deadline = self.honeycomb?.deadline(now);
        if deadline.is_none() {
            self.honeycomb = None;
        }
        self.window.request_redraw();
        deadline
    }

    /// Updates the blink phase of the active pane's cursor, and requests a redraw when
    /// it toggled. Returns when the next toggle is due, `None` while the cursor does not
    /// blink: its style does not ask for it, or the window does not have the focus.
    pub(super) fn blink(&mut self, now: Instant) -> Option<Instant> {
        let blinking = self.focused
            && self
                .workspace
                .active_pane()
                .is_some_and(|pane| pane.terminal.term.lock().cursor_style().blinking);
        let (changed, deadline) = self.cursor_blink.update(now, blinking);
        if changed {
            self.window.request_redraw();
        }
        deadline
    }

    /// Shows the cursor for a full blink interval: typing or pasting never hides it.
    pub(super) fn reset_blink(&mut self) {
        if self.cursor_blink.reset(Instant::now()) {
            self.window.request_redraw();
        }
    }

    pub(super) fn flush_panes(&mut self) {
        let shown = self.workspace.active_tab().cloned();
        let mut redraw = false;
        let mut changed = Vec::new();
        let mut primary = Vec::new();
        for (id, pane) in self.workspace.panes() {
            if pane.take_redraw() {
                changed.push(id);
                redraw |= shown.as_ref().is_some_and(|tab| tab.shows(id));
            }
            if pane.has_primary() {
                primary.push(id);
            }
        }
        for id in changed {
            self.workspace.touch(id);
        }
        let primary = primary
            .into_iter()
            .filter_map(|id| self.workspace.pane_mut(id)?.take_primary())
            .last();
        if redraw {
            self.window.request_redraw();
        }
        if let Some(text) = primary {
            self.set_primary(text);
        }
    }

    /// Sets the window title to the shown tab's title, when it changed.
    pub(super) fn refresh_title(&mut self) {
        let title = self
            .workspace
            .tab_title(self.workspace.active_index())
            .unwrap_or_else(|| DEFAULT_TITLE.to_owned());
        let title = special_days::titled(self.today.as_ref(), &title);
        if title != self.window_title {
            self.window.set_title(&title);
            self.window_title = title;
        }
    }
}
