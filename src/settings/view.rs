//! egui drawing of the settings window: the sections sidebar, the file problems at the
//! top and the selected section's fields.

use std::collections::BTreeMap;

use egui::{
    Align, Button, CentralPanel, Color32, ComboBox, FontId, Frame, Grid, Label, Layout, Margin,
    Panel, Pos2, Rect, RichText, ScrollArea, Shape, Slider, TextEdit, TextStyle, Ui, UiBuilder,
    Vec2,
};
use winit::keyboard::{Key, ModifiersState, NamedKey, PhysicalKey};

use super::model::{self, Change, Input, InputError, SettingsModel, Widget};
use crate::i18n::{self, Lang, Msg, SettingsField as Field, SettingsSection};
use crate::input::Action;
use crate::keybindings::Shortcut;

/// Width of the sections sidebar, in points.
const SIDEBAR_WIDTH: f32 = 180.0;
/// Width of the field widgets, in points.
const FIELD_WIDTH: f32 = 260.0;
/// Space between the action label and its shortcuts, in points.
const SHORTCUTS_GAP: f32 = 16.0;
/// Vertical margin of a shortcut row, in points.
const ROW_MARGIN: i8 = 3;
/// Largest share of the row width taken by the action labels, which wrap beyond it.
const LABEL_SHARE: f32 = 0.45;
/// Strength of the highlight of the shortcut row under the pointer or being edited.
const HIGHLIGHT: f32 = 0.45;

/// State of the view kept between frames.
#[derive(Debug, Default)]
pub struct View {
    /// Section shown in the right panel.
    pub section: SettingsSection,
    /// Text of the text and number fields being edited, until written.
    editing: BTreeMap<Field, String>,
    /// Typed values that were not written, by field.
    input_errors: BTreeMap<Field, InputError>,
    /// Slider value while it is dragged.
    sliding: BTreeMap<Field, f64>,
    /// Action whose new shortcut is the next key press.
    capturing: Option<Action>,
    /// A captured shortcut bound to another action, waiting for "Replace" or "Cancel".
    conflict: Option<Conflict>,
    /// Shortcuts added in this window, oldest first, shown last in their row.
    added: Vec<Shortcut>,
}

/// A captured shortcut already bound to `other`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Conflict {
    action: Action,
    shortcut: Shortcut,
    other: Action,
}

impl View {
    /// Drops the typed values not written, so a reload shows the file's values.
    pub fn reset_inputs(&mut self) {
        self.editing.clear();
        self.input_errors.clear();
        self.sliding.clear();
    }

    /// Whether the next key press is captured as a shortcut, so it must not reach egui.
    pub fn capturing(&self) -> bool {
        self.capturing.is_some()
    }

    /// Takes an `Escape` press egui leaves alone: dismisses a pending shortcut conflict
    /// and returns `true`, or returns `false` when nothing is pending, so the window closes.
    pub fn escape(&mut self) -> bool {
        self.conflict.take().is_some()
    }

    /// Takes a key press while a shortcut is captured; returns the changes to write.
    /// `Escape` alone cancels, a modifier alone keeps waiting, a shortcut bound to
    /// another action waits for "Replace" or "Cancel".
    pub fn captured_key(
        &mut self,
        model: &SettingsModel,
        key: &Key,
        physical: PhysicalKey,
        mods: ModifiersState,
    ) -> Vec<Change> {
        let Some(action) = self.capturing else {
            return Vec::new();
        };
        if model.locked().is_some() || (mods.is_empty() && *key == Key::Named(NamedKey::Escape)) {
            self.capturing = None;
            return Vec::new();
        }
        let Some(shortcut) = Shortcut::from_event(key, physical, mods) else {
            return Vec::new();
        };
        self.capturing = None;
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

    /// Draws one frame; returns the changes to write.
    pub fn show(&mut self, ui: &mut Ui, lang: Lang, model: &SettingsModel) -> Vec<Change> {
        let label = |msg: Msg| i18n::text(lang, &msg).into_owned();
        let mut changes = Vec::new();
        Panel::left("settings_sections")
            .resizable(false)
            .exact_size(SIDEBAR_WIDTH)
            .show(ui, |ui| {
                ui.add_space(8.0);
                for section in SettingsSection::ALL {
                    let selected = self.section == section;
                    if ui
                        .selectable_label(selected, label(Msg::Section(section)))
                        .clicked()
                        && !selected
                    {
                        self.section = section;
                        self.capturing = None;
                        self.conflict = None;
                    }
                }
            });
        CentralPanel::default().show(ui, |ui| {
            ScrollArea::vertical().show(ui, |ui| {
                file_problems(ui, lang, model);
                ui.heading(label(Msg::Section(self.section)));
                ui.separator();
                ui.add_enabled_ui(model.locked().is_none(), |ui| {
                    self.section_body(ui, lang, model, &mut changes);
                });
            });
        });
        changes
    }

    fn section_body(
        &mut self,
        ui: &mut Ui,
        lang: Lang,
        model: &SettingsModel,
        changes: &mut Vec<Change>,
    ) {
        for message in model.section_errors(self.section) {
            error_label(ui, message);
        }
        if self.section == SettingsSection::Theme {
            for message in model.theme_errors() {
                error_label(ui, &message);
            }
        }
        if self.section == SettingsSection::KeyboardShortcuts {
            self.shortcuts(ui, lang, model, changes);
            return;
        }
        Grid::new(("settings_fields", self.section))
            .num_columns(2)
            .spacing([16.0, 10.0])
            .show(ui, |ui| {
                for &field in model::section_fields(self.section) {
                    if !model.visible(field) {
                        continue;
                    }
                    ui.label(i18n::text(lang, &Msg::Field(field)));
                    ui.vertical(|ui| {
                        self.field(ui, lang, model, field, changes);
                        if let Some(error) = self.input_errors.get(&field) {
                            error_label(ui, &i18n::text(lang, &error.msg()));
                        }
                        for message in model.field_errors(field) {
                            error_label(ui, message);
                        }
                    });
                    ui.end_row();
                }
            });
    }

    fn field(
        &mut self,
        ui: &mut Ui,
        lang: Lang,
        model: &SettingsModel,
        field: Field,
        changes: &mut Vec<Change>,
    ) {
        let input = match model::widget(field) {
            Widget::Choice => choice(ui, lang, model, field).map(Input::Choice),
            Widget::Toggle => {
                let mut on = model.on(field);
                ui.checkbox(&mut on, "")
                    .changed()
                    .then_some(Input::Toggle(on))
            }
            Widget::Text | Widget::Number(_) => self.text_field(ui, model, field),
            Widget::Slider { min, max, step } => {
                let mut value = self
                    .sliding
                    .get(&field)
                    .copied()
                    .unwrap_or_else(|| model.number(field));
                let response = ui.add(
                    Slider::new(&mut value, min..=max)
                        .step_by(step)
                        .fixed_decimals(2),
                );
                if response.dragged() {
                    self.sliding.insert(field, value);
                }
                // Written when the drag ends, and on each click or keyboard step.
                let done = response.drag_stopped() || (response.changed() && !response.dragged());
                done.then(|| {
                    self.sliding.remove(&field);
                    Input::Number(value)
                })
            }
        };
        let Some(input) = input else { return };
        match model.edit(field, input) {
            Ok(edit) => {
                self.input_errors.remove(&field);
                self.editing.remove(&field);
                changes.extend(edit);
            }
            Err(error) => {
                self.input_errors.insert(field, error);
            }
        }
    }

    /// One row per action: its label, its shortcuts in aligned slots, each removable,
    /// and the "Add" button that captures a new one on the right. The row under the
    /// pointer, or being edited, is highlighted.
    fn shortcuts(
        &mut self,
        ui: &mut Ui,
        lang: Lang,
        model: &SettingsModel,
        changes: &mut Vec<Change>,
    ) {
        let label = |msg: Msg| i18n::text(lang, &msg).into_owned();
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
            .map(|shortcut| width(chip_text(*shortcut), &button))
            .fold(width(label(Msg::NoShortcut), &body), f32::max)
            + 2.0 * ui.spacing().button_padding.x;
        let add_width = [label(Msg::AddShortcut), label(Msg::Cancel)]
            .into_iter()
            .map(|text| width(text, &button))
            .fold(0.0, f32::max)
            + 2.0 * ui.spacing().button_padding.x;
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
                            if shortcuts.is_empty() && self.capturing != Some(action) {
                                ui.add_sized(
                                    [slot_width, ui.spacing().interact_size.y],
                                    Label::new(RichText::new(label(Msg::NoShortcut)).weak()),
                                );
                            }
                            for shortcut in shortcuts {
                                let chip = Button::new(chip_text(shortcut))
                                    .min_size(Vec2::new(slot_width, 0.0));
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
                            if self.capturing == Some(action) {
                                if ui.button(label(Msg::Cancel)).clicked() {
                                    self.capturing = None;
                                }
                            } else if ui.button(label(Msg::AddShortcut)).clicked() {
                                self.capturing = Some(action);
                                self.conflict = None;
                            }
                        },
                    ));
                    if self.capturing == Some(action) {
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
            let edited =
                self.capturing == Some(action) || self.conflict.is_some_and(|c| c.action == action);
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
    fn added_last(&self, shortcuts: &mut [Shortcut]) {
        shortcuts.sort_by_key(|shortcut| {
            self.added
                .iter()
                .position(|added| added == shortcut)
                .map_or(0, |position| position + 1)
        });
    }

    /// Records `shortcut` as the last one added.
    fn mark_added(&mut self, shortcut: Shortcut) {
        self.added.retain(|added| *added != shortcut);
        self.added.push(shortcut);
    }

    /// "Already used by <action>" with "Replace" and "Cancel".
    fn conflict_row(
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

    /// A text field, whose text is kept while edited and returned when `Enter` is
    /// pressed or the field loses focus.
    fn text_field(&mut self, ui: &mut Ui, model: &SettingsModel, field: Field) -> Option<Input> {
        let mut text = self
            .editing
            .get(&field)
            .cloned()
            .unwrap_or_else(|| model.text(field));
        let response = ui.add(TextEdit::singleline(&mut text).desired_width(FIELD_WIDTH));
        if response.has_focus() || response.changed() {
            self.editing.insert(field, text.clone());
        }
        response.lost_focus().then_some(Input::Text(text))
    }
}

/// A column of `width` at `x` on the cursor's line, laid out with `layout` and as tall
/// as its contents; returns its bottom. The cursor does not move.
fn column(ui: &mut Ui, x: f32, width: f32, layout: Layout, add: impl FnOnce(&mut Ui)) -> f32 {
    let rect = Rect::from_min_size(Pos2::new(x, ui.cursor().top()), Vec2::new(width, 0.0));
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(layout));
    child.set_width(width);
    add(&mut child);
    child.min_rect().bottom()
}

/// Text of a shortcut's removal button.
fn chip_text(shortcut: Shortcut) -> String {
    format!("{} \u{00d7}", shortcut.label())
}

/// A combo box of the field's choices; returns the picked value when it changed.
/// While it is open, typing the start of a choice highlights it and scrolls to it, and
/// `Enter` picks it.
fn choice(ui: &mut Ui, lang: Lang, model: &SettingsModel, field: Field) -> Option<String> {
    let current = model.text(field);
    let choices = model.choices(field);
    let label_of = |choice: &model::Choice| match &choice.label {
        Some(msg) => i18n::text(lang, msg).into_owned(),
        None => choice.value.clone(),
    };
    let labels: Vec<String> = choices.iter().map(label_of).collect();
    let selected = choices
        .iter()
        .position(|choice| choice.value == current)
        .map(|index| labels[index].clone())
        .unwrap_or_default();
    let search_id = ui.make_persistent_id(("settings_choice_search", field));
    let mut picked = None;
    let popup = ComboBox::from_id_salt(("settings_choice", field))
        .width(FIELD_WIDTH)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            let mut search: Search = ui.data(|data| data.get_temp(search_id)).unwrap_or_default();
            let (typed, now, enter) = ui.input(|input| {
                let typed: String = input
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::Text(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                (typed, input.time, input.key_pressed(egui::Key::Enter))
            });
            let moved = search.type_text(&typed, now, &labels);
            for (index, choice) in choices.iter().enumerate() {
                let found = search.found == Some(index);
                let response =
                    ui.selectable_label(choice.value == current || found, &labels[index]);
                if found && moved {
                    response.scroll_to_me(Some(Align::Center));
                }
                if (response.clicked() || (found && enter)) && choice.value != current {
                    picked = Some(choice.value.clone());
                }
                if found && enter {
                    ui.close();
                }
            }
            ui.data_mut(|data| data.insert_temp(search_id, search));
        });
    if popup.inner.is_none() || picked.is_some() {
        ui.data_mut(|data| data.remove::<Search>(search_id));
    }
    picked
}

/// Seconds without typing after which the next character starts a new search.
const SEARCH_TIMEOUT: f64 = 1.0;

/// Text typed in an open combo box and the choice it found.
#[derive(Debug, Clone, Default, PartialEq)]
struct Search {
    text: String,
    /// When the last character was typed, in egui's input time.
    at: f64,
    /// Index of the first choice starting with `text`.
    found: Option<usize>,
}

impl Search {
    /// Adds `typed` at `now` to the search, after a pause starting over; returns
    /// whether a choice was found by it. A search finding nothing keeps the last
    /// found choice.
    fn type_text(&mut self, typed: &str, now: f64, labels: &[String]) -> bool {
        if typed.is_empty() {
            return false;
        }
        if now - self.at > SEARCH_TIMEOUT {
            self.text.clear();
        }
        self.text.push_str(typed);
        self.at = now;
        let text = self.text.to_lowercase();
        let found = labels
            .iter()
            .position(|label| label.to_lowercase().starts_with(&text));
        if found.is_some() {
            self.found = found;
        }
        found.is_some()
    }
}

/// The problems of the whole file: why it is locked, a failed write, unknown keys and
/// errors tied to no section.
fn file_problems(ui: &mut Ui, lang: Lang, model: &SettingsModel) {
    let label = |msg: Msg| i18n::text(lang, &msg).into_owned();
    let mut shown = false;
    if let Some(error) = model.locked() {
        error_label(ui, &error.to_string());
        ui.label(error.file.display().to_string());
        ui.label(label(Msg::FixFileByHand));
        shown = true;
    }
    if let Some(error) = model.write_error() {
        error_label(ui, &format!("{}: {error}", label(Msg::CannotSave)));
        shown = true;
    }
    let unknown = model.unknown_keys();
    if !unknown.is_empty() {
        ui.strong(label(Msg::UnknownKeys));
        for (key, line) in unknown {
            let text = match line {
                Some(line) => format!("{key} ({})", label(Msg::AtLine(line))),
                None => key.to_owned(),
            };
            warn_label(ui, &text);
        }
        shown = true;
    }
    for message in model.other_errors() {
        error_label(ui, &message);
        shown = true;
    }
    if shown {
        ui.separator();
    }
}

fn error_label(ui: &mut Ui, text: &str) {
    let color = ui.visuals().error_fg_color;
    ui.label(RichText::new(text).color(color));
}

fn warn_label(ui: &mut Ui, text: &str) {
    let color = ui.visuals().warn_fg_color;
    ui.label(RichText::new(text).color(color));
}

#[cfg(test)]
mod tests {
    use super::*;

    use winit::keyboard::KeyCode;

    use super::model::{LoadErrors, Value};
    use crate::config::Config;

    fn model(text: &str) -> SettingsModel {
        let file = std::path::PathBuf::from("/tmp/config.toml");
        let (config, errors) = Config::parse(text, &Config::default(), &file);
        SettingsModel::new(
            &config,
            &LoadErrors {
                config: errors,
                theme: Vec::new(),
            },
        )
    }

    fn letter(c: &str, code: KeyCode) -> (Key, PhysicalKey) {
        (Key::Character(c.into()), PhysicalKey::Code(code))
    }

    const CTRL_ALT: ModifiersState = ModifiersState::CONTROL.union(ModifiersState::ALT);

    #[test]
    fn a_captured_key_press_binds_the_shortcut() {
        let model = model("");
        let mut view = View {
            capturing: Some(Action::DetachPane),
            ..View::default()
        };
        // A modifier alone keeps waiting.
        let (control, control_left) = (
            Key::Named(NamedKey::Control),
            PhysicalKey::Code(KeyCode::ControlLeft),
        );
        assert_eq!(
            view.captured_key(&model, &control, control_left, CTRL_ALT),
            vec![]
        );
        assert!(view.capturing());
        let (key, physical) = letter("d", KeyCode::KeyD);
        assert_eq!(
            view.captured_key(&model, &key, physical, CTRL_ALT),
            vec![Change::Set(
                vec!["keybindings".into(), "Ctrl+Alt+D".into()],
                Value::Text("detach_pane".into())
            )]
        );
        assert!(!view.capturing());
        // Not capturing: the key is not taken.
        assert_eq!(view.captured_key(&model, &key, physical, CTRL_ALT), vec![]);
    }

    #[test]
    fn escape_alone_cancels_the_capture() {
        let model = model("");
        let mut view = View {
            capturing: Some(Action::DetachPane),
            ..View::default()
        };
        let escape = Key::Named(NamedKey::Escape);
        let physical = PhysicalKey::Code(KeyCode::Escape);
        assert_eq!(
            view.captured_key(&model, &escape, physical, ModifiersState::empty()),
            vec![]
        );
        assert!(!view.capturing());
        // With a modifier, Escape is a shortcut.
        view.capturing = Some(Action::DetachPane);
        let changes = view.captured_key(&model, &escape, physical, ModifiersState::SHIFT);
        assert_eq!(
            changes,
            vec![Change::Set(
                vec!["keybindings".into(), "Shift+Escape".into()],
                Value::Text("detach_pane".into())
            )]
        );
    }

    #[test]
    fn escape_dismisses_a_conflict_before_closing() {
        let mut view = View {
            conflict: Some(Conflict {
                action: Action::NextTab,
                shortcut: Shortcut::parse("Ctrl+Shift+T").unwrap(),
                other: Action::NewTab,
            }),
            ..View::default()
        };
        assert!(view.escape());
        assert_eq!(view.conflict, None);
        assert!(!view.escape());
    }

    #[test]
    fn a_shortcut_of_another_action_waits_for_replace() {
        let model = model("");
        let mut view = View {
            capturing: Some(Action::NextTab),
            ..View::default()
        };
        let (key, physical) = letter("T", KeyCode::KeyT);
        let mods = ModifiersState::CONTROL | ModifiersState::SHIFT;
        assert_eq!(view.captured_key(&model, &key, physical, mods), vec![]);
        assert!(!view.capturing());
        assert_eq!(
            view.conflict,
            Some(Conflict {
                action: Action::NextTab,
                shortcut: Shortcut::parse("Ctrl+Shift+T").unwrap(),
                other: Action::NewTab,
            })
        );
    }

    #[test]
    fn a_locked_file_cancels_the_capture() {
        let model = model("[font\n");
        assert!(model.locked().is_some());
        let mut view = View {
            capturing: Some(Action::DetachPane),
            ..View::default()
        };
        let (key, physical) = letter("d", KeyCode::KeyD);
        assert_eq!(view.captured_key(&model, &key, physical, CTRL_ALT), vec![]);
        assert!(!view.capturing());
    }

    #[test]
    fn added_shortcuts_come_last_in_the_order_they_were_added() {
        let model = model("");
        let mut view = View {
            capturing: Some(Action::Copy),
            ..View::default()
        };
        let (key, physical) = letter("a", KeyCode::KeyA);
        assert!(
            !view
                .captured_key(&model, &key, physical, CTRL_ALT)
                .is_empty()
        );
        let added = Shortcut::parse("Ctrl+Alt+A").unwrap();
        let (f7, copy, ctrl_shift_c) = (
            Shortcut::parse("F7").unwrap(),
            Shortcut::parse("Copy").unwrap(),
            Shortcut::parse("Ctrl+Shift+C").unwrap(),
        );
        // The file sorts its keys: "Ctrl+Alt+A" would come before "F7".
        let mut shortcuts = vec![ctrl_shift_c, copy, added, f7];
        view.added_last(&mut shortcuts);
        assert_eq!(shortcuts, [ctrl_shift_c, copy, f7, added]);
        // Added again, it moves after the newer ones.
        view.mark_added(f7);
        view.mark_added(added);
        let mut shortcuts = vec![added, f7, copy];
        view.added_last(&mut shortcuts);
        assert_eq!(shortcuts, [copy, f7, added]);
    }

    #[test]
    fn a_conflict_marks_nothing_added_before_replace() {
        let model = model("");
        let mut view = View {
            capturing: Some(Action::NextTab),
            ..View::default()
        };
        let (key, physical) = letter("T", KeyCode::KeyT);
        let mods = ModifiersState::CONTROL | ModifiersState::SHIFT;
        view.captured_key(&model, &key, physical, mods);
        assert!(view.added.is_empty(), "nothing written before Replace");
    }

    fn labels(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn typing_finds_the_first_choice_starting_with_the_text() {
        let labels = labels(&["DejaVu Sans Mono", "Fira Code", "Fira Mono", "Hack"]);
        let mut search = Search::default();
        assert!(!search.type_text("", 10.0, &labels));
        assert_eq!(search.found, None);
        assert!(search.type_text("f", 10.0, &labels));
        assert_eq!(search.found, Some(1));
        assert!(search.type_text("IRA M", 10.5, &labels));
        assert_eq!(search.found, Some(2));
        // Nothing starts with the text: the last choice found stays.
        assert!(!search.type_text("x", 10.8, &labels));
        assert_eq!(search.found, Some(2));
        assert_eq!(search.text, "fIRA Mx");
    }

    #[test]
    fn typing_after_a_pause_starts_a_new_search() {
        let labels = labels(&["Fira Code", "Hack"]);
        let mut search = Search::default();
        search.type_text("f", 1.0, &labels);
        assert!(search.type_text("h", 1.0 + SEARCH_TIMEOUT + 0.1, &labels));
        assert_eq!(search.text, "h");
        assert_eq!(search.found, Some(1));
    }

    #[test]
    fn reset_drops_the_values_not_written() {
        let mut view = View::default();
        view.editing.insert(Field::FontSize, "abc".into());
        view.input_errors
            .insert(Field::FontSize, InputError::NotANumber);
        view.sliding.insert(Field::BackgroundOpacity, 0.5);
        view.section = SettingsSection::Cursor;
        view.capturing = Some(Action::Copy);
        view.reset_inputs();
        assert!(view.editing.is_empty());
        assert!(view.input_errors.is_empty());
        assert!(view.sliding.is_empty());
        assert_eq!(view.section, SettingsSection::Cursor);
        // A reload after a write does not stop a capture.
        assert!(view.capturing());
    }
}
