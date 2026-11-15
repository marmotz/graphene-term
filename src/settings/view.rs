//! egui drawing of the settings window: the sections sidebar, the file problems at the
//! top and the selected section's fields.

use std::collections::BTreeMap;

use egui::{CentralPanel, ComboBox, Grid, Panel, RichText, ScrollArea, Slider, TextEdit, Ui};
use winit::keyboard::{Key, ModifiersState, NamedKey, PhysicalKey};

use super::model::{self, Change, Input, InputError, SettingsModel, Widget};
use crate::i18n::{self, Lang, Msg, SettingsField as Field, SettingsSection};
use crate::input::Action;
use crate::keybindings::Shortcut;

/// Width of the sections sidebar, in points.
const SIDEBAR_WIDTH: f32 = 180.0;
/// Width of the field widgets, in points.
const FIELD_WIDTH: f32 = 260.0;

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
            Ok(changes) => changes,
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

    /// One row per action: its shortcuts, each removable, and the "Add" button that
    /// captures a new one.
    fn shortcuts(
        &mut self,
        ui: &mut Ui,
        lang: Lang,
        model: &SettingsModel,
        changes: &mut Vec<Change>,
    ) {
        let label = |msg: Msg| i18n::text(lang, &msg).into_owned();
        Grid::new("settings_shortcuts")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .striped(true)
            .show(ui, |ui| {
                for (action, shortcuts) in model.shortcut_rows() {
                    ui.label(label(action.label()));
                    ui.vertical(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            if shortcuts.is_empty() {
                                ui.weak(label(Msg::NoShortcut));
                            }
                            for shortcut in shortcuts {
                                if ui
                                    .button(format!("{} \u{00d7}", shortcut.label()))
                                    .on_hover_text(label(Msg::RemoveShortcut))
                                    .clicked()
                                {
                                    self.conflict = None;
                                    changes.extend(model.remove_shortcut(action, shortcut));
                                }
                            }
                            if self.capturing == Some(action) {
                                ui.strong(label(Msg::PressShortcut));
                                if ui.button(label(Msg::Cancel)).clicked() {
                                    self.capturing = None;
                                }
                            } else if ui.button(label(Msg::AddShortcut)).clicked() {
                                self.capturing = Some(action);
                                self.conflict = None;
                            }
                        });
                        if let Some(conflict) = self.conflict.filter(|c| c.action == action) {
                            self.conflict_row(ui, lang, model, conflict, changes);
                        }
                    });
                    ui.end_row();
                }
            });
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

/// A combo box of the field's choices; returns the picked value when it changed.
fn choice(ui: &mut Ui, lang: Lang, model: &SettingsModel, field: Field) -> Option<String> {
    let current = model.text(field);
    let choices = model.choices(field);
    let label_of = |choice: &model::Choice| match &choice.label {
        Some(msg) => i18n::text(lang, msg).into_owned(),
        None => choice.value.clone(),
    };
    let selected = choices
        .iter()
        .find(|choice| choice.value == current)
        .map(label_of)
        .unwrap_or_default();
    let mut picked = None;
    ComboBox::from_id_salt(("settings_choice", field))
        .width(FIELD_WIDTH)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for choice in &choices {
                if ui
                    .selectable_label(choice.value == current, label_of(choice))
                    .clicked()
                    && choice.value != current
                {
                    picked = Some(choice.value.clone());
                }
            }
        });
    picked
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
