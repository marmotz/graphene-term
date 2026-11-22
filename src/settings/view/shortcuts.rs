//! The shortcuts section: capture of a key press, conflicts and the list of bindings.

use nucleo_matcher::chars;

use super::*;

/// The rows of `rows` kept by `filter`, in their order.
pub(super) fn filtered(
    rows: Vec<(Action, Vec<Shortcut>)>,
    filter: &ShortcutFilter,
    lang: Lang,
) -> Vec<(Action, Vec<Shortcut>)> {
    match filter {
        ShortcutFilter::None => rows,
        ShortcutFilter::Text(text) => {
            let text = fold(text);
            rows.into_iter()
                .filter(|(action, _)| fold(&i18n::text(lang, &action.label())).contains(&text))
                .collect()
        }
        ShortcutFilter::Shortcut { actions, .. } => rows
            .into_iter()
            .filter(|(action, _)| actions.contains(action))
            .collect(),
    }
}

/// `text` without accents and in lower case, to compare names loosely.
fn fold(text: &str) -> String {
    text.chars()
        .map(|c| chars::to_lower_case(chars::normalize(c)))
        .collect()
}

impl View {
    /// Takes a key press while a shortcut is captured; returns the changes to write.
    /// `Escape` alone cancels, a modifier alone keeps waiting. A shortcut captured for
    /// a shortcut field becomes its value; one captured for an action is bound to it, or waits for "Replace" or "Cancel" when bound to
    /// another one; a shortcut searched for narrows the list to its actions and writes
    /// nothing.
    pub fn captured_key(
        &mut self,
        model: &SettingsModel,
        key: &Key,
        physical: PhysicalKey,
        mods: ModifiersState,
    ) -> Vec<Change> {
        let Some(capture) = self.capturing else {
            return Vec::new();
        };
        let section = match capture {
            Capture::Field(field) => field_section(field),
            Capture::Bind(_) | Capture::Search => SettingsSection::KeyboardShortcuts,
        };
        if model.locked(section).is_some()
            || (mods.is_empty() && *key == Key::Named(NamedKey::Escape))
        {
            self.capturing = None;
            return Vec::new();
        }
        let Some(shortcut) = Shortcut::from_event(key, physical, mods) else {
            return Vec::new();
        };
        self.capturing = None;
        let action = match capture {
            Capture::Bind(action) => action,
            Capture::Field(field) => {
                let mut changes = Vec::new();
                self.write_input(model, field, Input::Text(shortcut.label()), &mut changes);
                return changes;
            }
            Capture::Search => {
                let actions = model
                    .shortcut_rows()
                    .into_iter()
                    .filter(|(_, shortcuts)| shortcuts.contains(&shortcut))
                    .map(|(action, _)| action)
                    .collect();
                self.set_filter(ShortcutFilter::Shortcut { shortcut, actions });
                return Vec::new();
            }
        };
        match model.add_shortcut(action, shortcut) {
            Ok(changes) => {
                self.mark_added(shortcut);
                changes
            }
            Err(other) => {
                self.conflict = Some(Conflict {
                    action,
                    shortcut,
                    other,
                });
                Vec::new()
            }
        }
    }

    /// The field filtering the actions by name, then the button searching them by
    /// shortcut: it starts the capture, cancels it, or shows the shortcut found and
    /// clears the filter. `Escape` in the field clears an active filter and keeps the
    /// focus; without filter, the field only loses the focus.
    fn filter_bar(&mut self, ui: &mut Ui, lang: Lang) {
        let label = |msg: Msg| i18n::text(lang, &msg).into_owned();
        let mut text = match &self.filter {
            ShortcutFilter::Text(text) => text.clone(),
            _ => String::new(),
        };
        let response = ui
            .horizontal(|ui| {
                let response = ui.add(
                    TextEdit::singleline(&mut text)
                        .hint_text(label(Msg::FilterActions))
                        .desired_width(FIELD_WIDTH),
                );
                if self.capturing == Some(Capture::Search) {
                    if ui.button(label(Msg::Cancel)).clicked() {
                        self.capturing = None;
                    }
                } else if let ShortcutFilter::Shortcut { shortcut, .. } = self.filter {
                    if ui.add(chip(&shortcut.label())).clicked() {
                        self.set_filter(ShortcutFilter::None);
                    }
                } else if ui.button(label(Msg::SearchShortcut)).clicked() {
                    self.capturing = Some(Capture::Search);
                    self.conflict = None;
                }
                response
            })
            .inner;
        if self.capturing == Some(Capture::Search) {
            ui.label(RichText::new(label(Msg::PressShortcut)).weak());
        }
        if std::mem::take(&mut self.focus_filter) {
            response.request_focus();
        }
        if response.changed() {
            self.set_filter(if text.is_empty() {
                ShortcutFilter::None
            } else {
                ShortcutFilter::Text(text)
            });
        }
        if response.lost_focus()
            && ui.input(|input| input.key_pressed(egui::Key::Escape))
            && self.filter != ShortcutFilter::None
        {
            self.set_filter(ShortcutFilter::None);
            response.request_focus();
        }
    }

    /// The filter field, then one row per action kept by the filter: its label, its
    /// shortcuts in aligned slots, each removable, and the "Add" button that captures a
    /// new one on the right. The row under the pointer, or being edited, is
    /// highlighted.
    pub(super) fn shortcuts(
        &mut self,
        ui: &mut Ui,
        lang: Lang,
        model: &SettingsModel,
        changes: &mut Vec<Change>,
    ) {
        let label = |msg: Msg| i18n::text(lang, &msg).into_owned();
        self.filter_bar(ui, lang);
        ui.add_space(ui.spacing().item_spacing.y);
        let rows = model.shortcut_rows();
        let body = TextStyle::Body.resolve(ui.style());
        let button = TextStyle::Button.resolve(ui.style());
        let width = |text: String, font: &FontId| {
            ui.painter()
                .layout_no_wrap(text, font.clone(), Color32::PLACEHOLDER)
                .size()
                .x
        };
        let label_width = rows
            .iter()
            .map(|(action, _)| width(label(action.label()), &body))
            .fold(0.0, f32::max);
        let slot_width = rows
            .iter()
            .flat_map(|(_, shortcuts)| shortcuts)
            .map(|shortcut| chip_width(ui, &shortcut.label()))
            .fold(
                width(label(Msg::NoShortcut), &body) + 2.0 * ui.spacing().button_padding.x,
                f32::max,
            );
        let add_width = [label(Msg::AddShortcut), label(Msg::Cancel)]
            .into_iter()
            .map(|text| width(text, &button))
            .fold(0.0, f32::max)
            + 2.0 * ui.spacing().button_padding.x;
        let rows = filtered(rows, &self.filter, lang);
        if rows.is_empty() {
            let empty = match self.filter {
                ShortcutFilter::Shortcut { .. } => Msg::NoActionForShortcut,
                _ => Msg::NoActionMatches,
            };
            ui.label(RichText::new(label(empty)).weak());
            return;
        }
        let row_gap = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;
        for (index, (action, mut shortcuts)) in rows.into_iter().enumerate() {
            self.added_last(&mut shortcuts);
            let background = ui.painter().add(Shape::Noop);
            let row = Frame::NONE
                .inner_margin(Margin::symmetric(6, ROW_MARGIN))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = row_gap;
                    let total = ui.available_width();
                    let name_width = label_width.min(total * LABEL_SHARE);
                    let slots_width =
                        (total - name_width - add_width - 2.0 * SHORTCUTS_GAP).max(0.0);
                    let slot_width = slot_width.min(slots_width);
                    let text_offset = ui.spacing().button_padding.y;
                    let top = ui.cursor().top();
                    let left = ui.max_rect().left();
                    let mut bottom =
                        column(ui, left, name_width, Layout::top_down(Align::Min), |ui| {
                            ui.add_space(text_offset);
                            ui.add(Label::new(label(action.label())).wrap());
                        });
                    let slots = Layout::left_to_right(Align::Min).with_main_wrap(true);
                    bottom = bottom.max(column(
                        ui,
                        left + name_width + SHORTCUTS_GAP,
                        slots_width,
                        slots,
                        |ui| {
                            if shortcuts.is_empty() && self.capturing != Some(Capture::Bind(action))
                            {
                                ui.add_sized(
                                    [slot_width, ui.spacing().interact_size.y],
                                    Label::new(RichText::new(label(Msg::NoShortcut)).weak()),
                                );
                            }
                            for shortcut in shortcuts {
                                let chip =
                                    chip(&shortcut.label()).min_size(Vec2::new(slot_width, 0.0));
                                if ui
                                    .add(chip)
                                    .on_hover_text(label(Msg::RemoveShortcut))
                                    .clicked()
                                {
                                    self.conflict = None;
                                    changes.extend(model.remove_shortcut(action, shortcut));
                                }
                            }
                        },
                    ));
                    bottom = bottom.max(column(
                        ui,
                        left + total - add_width,
                        add_width,
                        Layout::top_down(Align::Max),
                        |ui| {
                            if self.capturing == Some(Capture::Bind(action)) {
                                if ui.button(label(Msg::Cancel)).clicked() {
                                    self.capturing = None;
                                }
                            } else if ui.button(label(Msg::AddShortcut)).clicked() {
                                self.capturing = Some(Capture::Bind(action));
                                self.conflict = None;
                            }
                        },
                    ));
                    if self.capturing == Some(Capture::Bind(action)) {
                        let hint_top = bottom + row_gap;
                        let hint_rect = Rect::from_min_size(
                            Pos2::new(left + name_width + SHORTCUTS_GAP, hint_top),
                            Vec2::new(slots_width, 0.0),
                        );
                        let mut hint_ui = ui.new_child(
                            UiBuilder::new()
                                .max_rect(hint_rect)
                                .layout(Layout::top_down(Align::Min)),
                        );
                        hint_ui.set_width(slots_width);
                        hint_ui.add(
                            Label::new(RichText::new(label(Msg::PressShortcut)).weak()).wrap(),
                        );
                        bottom = bottom.max(hint_ui.min_rect().bottom());
                    }
                    ui.advance_cursor_after_rect(Rect::from_min_max(
                        Pos2::new(left, top),
                        Pos2::new(left + total, bottom),
                    ));
                    if let Some(conflict) = self.conflict.filter(|c| c.action == action) {
                        self.conflict_row(ui, lang, model, conflict, changes);
                    }
                });
            let rect = Rect::from_x_y_ranges(ui.max_rect().x_range(), row.response.rect.y_range());
            let edited = self.capturing == Some(Capture::Bind(action))
                || self.conflict.is_some_and(|c| c.action == action);
            let visuals = ui.visuals();
            let fill = if edited || ui.rect_contains_pointer(rect) {
                visuals.selection.bg_fill.gamma_multiply(HIGHLIGHT)
            } else if index % 2 == 1 {
                visuals.faint_bg_color
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().set(
                background,
                Shape::rect_filled(rect, visuals.widgets.noninteractive.corner_radius, fill),
            );
        }
    }

    /// Moves the shortcuts added in this window to the end, in the order they were
    /// added.
    pub(super) fn added_last(&self, shortcuts: &mut [Shortcut]) {
        shortcuts.sort_by_key(|shortcut| {
            self.added
                .iter()
                .position(|added| added == shortcut)
                .map_or(0, |position| position + 1)
        });
    }

    /// Records `shortcut` as the last one added.
    pub(super) fn mark_added(&mut self, shortcut: Shortcut) {
        self.added.retain(|added| *added != shortcut);
        self.added.push(shortcut);
    }

    /// "Already used by <action>" with "Replace" and "Cancel".
    pub(super) fn conflict_row(
        &mut self,
        ui: &mut Ui,
        lang: Lang,
        model: &SettingsModel,
        conflict: Conflict,
        changes: &mut Vec<Change>,
    ) {
        let label = |msg: Msg| i18n::text(lang, &msg).into_owned();
        let other = label(conflict.other.label());
        warn_label(
            ui,
            &format!(
                "{}: {}",
                conflict.shortcut.label(),
                label(Msg::AlreadyUsedBy(other))
            ),
        );
        ui.horizontal(|ui| {
            if ui.button(label(Msg::Replace)).clicked() {
                self.conflict = None;
                self.mark_added(conflict.shortcut);
                changes.extend(model.replace_shortcut(conflict.action, conflict.shortcut));
            }
            if ui.button(label(Msg::Cancel)).clicked() {
                self.conflict = None;
            }
        });
    }
}
