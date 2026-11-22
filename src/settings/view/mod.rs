//! egui drawing of the settings window: the sections sidebar, the file problems at the
//! top and the selected section's fields.

mod shortcuts;
mod theme_editor;

use std::collections::BTreeMap;
use std::time::Instant;

use alacritty_terminal::vte::ansi::Rgb;
use egui::{
    Align, Button, CentralPanel, Color32, ComboBox, FontId, Frame, Grid, Label, Layout, Margin,
    Panel, Popup, Pos2, Rect, RichText, ScrollArea, Shape, Slider, TextEdit, TextStyle, Ui,
    UiBuilder, Vec2,
};
use winit::keyboard::{Key, ModifiersState, NamedKey, PhysicalKey};

use super::model::{self, Change, Input, InputError, SettingsModel, ThemeOp, Widget};
use crate::i18n::{self, Lang, Msg, SettingsField as Field, SettingsSection};
use crate::input::Action;
use crate::keybindings::Shortcut;
use crate::theme::{ColorGroup, ColorKey, NameError, format_color, parse_color};

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
/// Width of the `#rrggbb` field of a theme color, in monospace digits: the 7
/// characters and room for the cursor and the frame.
const HEX_CHARS: f32 = 9.0;
/// Indent of the theme colors under their group heading, in points.
const COLOR_INDENT: f32 = 12.0;
/// Inner margin of the theme editor's boxes, in points.
const BOX_MARGIN: i8 = 10;

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
    /// What the shortcuts list is narrowed to.
    filter: ShortcutFilter,
    /// Whether the shortcuts filter field takes the focus on the next frame.
    focus_filter: bool,
    /// The color picker open in the theme editor, until it closes and its color is
    /// written.
    picking: Option<Picking>,
    /// The name of a copy or a rename of the edited theme, being typed.
    naming: Option<Naming>,
    /// Whether the deletion of the edited theme waits for its confirmation.
    confirm_delete: bool,
    /// The `#rrggbb` code of a theme color being typed, until its field loses focus.
    hex: Option<HexInput>,
    /// The recent clicks on the version label, oldest first.
    version_clicks: Vec<Instant>,
}

/// What one frame asks to write or change.
#[derive(Debug, Default, PartialEq)]
pub struct Edits {
    /// Changes to the configuration files.
    pub changes: Vec<Change>,
    /// Changes to the edited theme's file, each with the configuration changes it
    /// makes ([`SettingsModel::theme_op`]).
    pub theme_ops: Vec<(Vec<Change>, ThemeOp)>,
    /// The theme to show in the theme editor instead of the edited one.
    pub edit_theme: Option<String>,
    /// Whether to open the user themes folder.
    pub open_themes_dir: bool,
}

impl Edits {
    /// Whether there is nothing to write nor change.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
            && self.theme_ops.is_empty()
            && self.edit_theme.is_none()
            && !self.open_themes_dir
    }

    /// Checks `op` on the edited theme and adds it.
    fn theme_op(&mut self, model: &SettingsModel, op: ThemeOp) -> Result<(), NameError> {
        self.theme_ops.push(model.theme_op(op)?);
        Ok(())
    }
}

/// A color picker open in the theme editor.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Picking {
    key: ColorKey,
    /// The color shown by the picker.
    color: Rgb,
    /// The color of the file when the picker opened.
    saved: Option<Rgb>,
}

/// The `#rrggbb` code of a theme color being typed, and whether it was refused.
#[derive(Debug, Clone, PartialEq)]
struct HexInput {
    key: ColorKey,
    text: String,
    invalid: bool,
}

/// Parses a typed color code, `#rrggbb` or `rrggbb`, around spaces.
fn parse_hex(text: &str) -> Option<Rgb> {
    let text = text.trim();
    if text.starts_with('#') {
        parse_color(text)
    } else {
        parse_color(&format!("#{text}"))
    }
}

/// What a typed theme name is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameAction {
    Duplicate,
    Rename,
}

/// A theme name being typed, and why the last one was refused.
#[derive(Debug, Clone, PartialEq)]
struct Naming {
    action: NameAction,
    text: String,
    error: Option<NameError>,
}

/// What the shortcuts list is narrowed to.
#[derive(Debug, Clone, Default, PartialEq)]
enum ShortcutFilter {
    #[default]
    None,
    /// Actions whose label contains this text.
    Text(String),
    /// The actions bound to `shortcut` when it was captured, frozen.
    // Built by the search by shortcut, which is not there yet.
    #[allow(dead_code)]
    Shortcut {
        shortcut: Shortcut,
        actions: Vec<Action>,
    },
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
        self.hex = None;
    }

    /// Whether the next key press is captured as a shortcut, so it must not reach egui.
    pub fn capturing(&self) -> bool {
        self.capturing.is_some()
    }

    /// Takes an `Escape` press egui leaves alone: dismisses a pending shortcut conflict,
    /// theme name or theme deletion and returns `true`, or returns `false` when nothing
    /// is pending, so the window closes.
    pub fn escape(&mut self) -> bool {
        self.conflict.take().is_some()
            || self.naming.take().is_some()
            || std::mem::take(&mut self.confirm_delete)
    }

    /// Draws one frame; returns what to write.
    pub fn show(&mut self, ui: &mut Ui, lang: Lang, model: &SettingsModel) -> Edits {
        let label = |msg: Msg| i18n::text(lang, &msg).into_owned();
        let mut edits = Edits::default();
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
                        && let Some(op) = self.select_section(section)
                    {
                        let _ = edits.theme_op(model, op);
                    }
                }
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.add_space(8.0);
                    if version_label(ui).clicked() {
                        self.version_click(model, Instant::now(), &mut edits);
                    }
                });
            });
        CentralPanel::default().show(ui, |ui| {
            ScrollArea::vertical().show(ui, |ui| {
                file_problems(ui, lang, model, self.section);
                ui.heading(label(Msg::Section(self.section)));
                ui.separator();
                ui.add_enabled_ui(model.locked(self.section).is_none(), |ui| {
                    self.section_body(ui, lang, model, &mut edits);
                });
            });
        });
        edits
    }

    /// Shows `section` and drops what was being edited in the previous one; returns
    /// the color of the open picker to write. Entering the shortcuts section gives the
    /// focus to its filter field.
    fn select_section(&mut self, section: SettingsSection) -> Option<ThemeOp> {
        let op = self.finish_picking();
        self.section = section;
        self.capturing = None;
        self.conflict = None;
        self.naming = None;
        self.confirm_delete = false;
        self.hex = None;
        self.filter = ShortcutFilter::None;
        self.focus_filter = section == SettingsSection::KeyboardShortcuts;
        op
    }

    /// Narrows the shortcuts list to `filter` and cancels the shortcut being edited.
    fn set_filter(&mut self, filter: ShortcutFilter) {
        self.filter = filter;
        self.capturing = None;
        self.conflict = None;
    }

    /// A click on the version label at `now`: the 7th within 2 seconds unlocks the
    /// hidden theme, makes it the active one and starts the count again.
    fn version_click(&mut self, model: &SettingsModel, now: Instant, edits: &mut Edits) {
        self.version_clicks.push(now);
        if crate::easter_eggs::rapid_clicks(&mut self.version_clicks, now) {
            edits.changes.extend(model.unlock_hidden_theme());
            self.version_clicks.clear();
        }
    }

    fn section_body(&mut self, ui: &mut Ui, lang: Lang, model: &SettingsModel, edits: &mut Edits) {
        let changes = &mut edits.changes;
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
        if self.section == SettingsSection::Theme {
            self.theme_editor(ui, lang, model, edits);
        }
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

/// A combo box of the themes; returns the one picked when it is not the edited theme.
fn theme_selector(ui: &mut Ui, model: &SettingsModel) -> Option<String> {
    let edited = model.edited_theme();
    let mut picked = None;
    ComboBox::from_id_salt("theme_editor_theme")
        .width(FIELD_WIDTH)
        .selected_text(edited)
        .show_ui(ui, |ui| {
            for name in model.theme_choices() {
                if ui.selectable_label(name == edited, name).clicked() && name != edited {
                    picked = Some(name.clone());
                }
            }
        });
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

/// The problems of the files: why `section` is locked, a failed write, unknown keys and
/// errors tied to no section.
fn file_problems(ui: &mut Ui, lang: Lang, model: &SettingsModel, section: SettingsSection) {
    let label = |msg: Msg| i18n::text(lang, &msg).into_owned();
    let mut shown = false;
    if let Some(error) = model.locked(section) {
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

/// The `Graphene <version>` text shown at the bottom of the sections sidebar.
fn version_text() -> String {
    format!("Graphene {}", crate::VERSION)
}

/// Draws the build version, faint and clickable without looking like it: no text
/// selection, so the pointer keeps its arrow.
fn version_label(ui: &mut Ui) -> egui::Response {
    ui.add(
        Label::new(RichText::new(version_text()).weak())
            .selectable(false)
            .sense(egui::Sense::click()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use winit::keyboard::KeyCode;

    use super::model::{LoadErrors, Value};
    use crate::appearance::Appearance;
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
            None,
            Appearance::Dark,
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

    /// The actions of the rows kept by a text filter, in French.
    fn filter_actions(model: &SettingsModel, text: &str) -> Vec<Action> {
        shortcuts::filtered(
            model.shortcut_rows(),
            &ShortcutFilter::Text(text.into()),
            Lang::Fr,
        )
        .into_iter()
        .map(|(action, _)| action)
        .collect()
    }

    #[test]
    fn the_text_filter_keeps_the_actions_whose_name_contains_it() {
        let model = model("");
        // In the middle of the name.
        assert_eq!(filter_actions(&model, "glet suiv"), vec![Action::NextTab]);
        // Whatever the case.
        let tabs = filter_actions(&model, "ONGLET");
        for action in [Action::NewTab, Action::NextTab, Action::PreviousTab] {
            assert!(tabs.contains(&action), "{action:?}");
        }
        assert!(!tabs.contains(&Action::Copy));
        // Whatever the accents, typed or in the name.
        assert_eq!(filter_actions(&model, "onglét"), tabs);
        assert!(filter_actions(&model, "precedent").contains(&Action::PreviousTab));
        // Nothing matches.
        assert_eq!(filter_actions(&model, "zzz"), vec![]);
        // An empty text or no filter keeps every row.
        let all = model.shortcut_rows();
        assert_eq!(filter_actions(&model, "").len(), all.len());
        assert_eq!(
            shortcuts::filtered(all.clone(), &ShortcutFilter::None, Lang::Fr),
            all
        );
    }

    #[test]
    fn changing_the_filter_cancels_the_shortcut_being_edited() {
        let mut view = View {
            capturing: Some(Action::NextTab),
            conflict: Some(Conflict {
                action: Action::NextTab,
                shortcut: Shortcut::parse("Ctrl+Shift+T").unwrap(),
                other: Action::NewTab,
            }),
            ..View::default()
        };
        view.set_filter(ShortcutFilter::Text("tab".into()));
        assert_eq!(view.filter, ShortcutFilter::Text("tab".into()));
        assert!(!view.capturing());
        assert_eq!(view.conflict, None);
    }

    #[test]
    fn changing_section_clears_the_filter_and_focuses_it_in_shortcuts() {
        let mut view = View {
            filter: ShortcutFilter::Text("tab".into()),
            ..View::default()
        };
        view.select_section(SettingsSection::KeyboardShortcuts);
        assert_eq!(view.filter, ShortcutFilter::None);
        assert!(view.focus_filter);
        view.filter = ShortcutFilter::Text("tab".into());
        view.select_section(SettingsSection::Theme);
        assert_eq!(view.filter, ShortcutFilter::None);
        assert!(!view.focus_filter);
    }

    /// Draws one frame of `view` with `events`; returns what it writes.
    fn frame(
        ctx: &egui::Context,
        view: &mut View,
        model: &SettingsModel,
        events: Vec<egui::Event>,
    ) -> Edits {
        let mut edits = Edits::default();
        let input = egui::RawInput {
            events,
            ..egui::RawInput::default()
        };
        let mut output = ctx.run_ui(input, |ui| edits = view.show(ui, Lang::Fr, model));
        output.textures_delta.clear();
        edits
    }

    fn press(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn the_filter_field_takes_the_typed_text_and_escape_clears_it() {
        let model = model("");
        let mut view = View::default();
        view.select_section(SettingsSection::KeyboardShortcuts);
        let ctx = egui::Context::default();
        assert_eq!(frame(&ctx, &mut view, &model, vec![]), Edits::default());
        assert!(!view.focus_filter);
        frame(
            &ctx,
            &mut view,
            &model,
            vec![egui::Event::Text("zzz".into())],
        );
        assert_eq!(view.filter, ShortcutFilter::Text("zzz".into()));
        // No action matches: the list is replaced by a hint, nothing is written.
        assert_eq!(frame(&ctx, &mut view, &model, vec![]), Edits::default());
        // Escape clears the filter and keeps the focus.
        frame(&ctx, &mut view, &model, vec![press(egui::Key::Escape)]);
        assert_eq!(view.filter, ShortcutFilter::None);
        frame(
            &ctx,
            &mut view,
            &model,
            vec![egui::Event::Text("tab".into())],
        );
        assert_eq!(view.filter, ShortcutFilter::Text("tab".into()));
        // Without filter, Escape only takes the focus away.
        view.filter = ShortcutFilter::None;
        frame(&ctx, &mut view, &model, vec![press(egui::Key::Escape)]);
        frame(
            &ctx,
            &mut view,
            &model,
            vec![egui::Event::Text("tab".into())],
        );
        assert_eq!(view.filter, ShortcutFilter::None);
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
    fn a_locked_shortcuts_file_cancels_the_capture() {
        let file = std::path::PathBuf::from("/tmp/keybindings.toml");
        let (_, errors, _) = Config::parse_keybindings("[keybindings\n", &Config::default(), &file);
        let model = SettingsModel::new(
            &Config::default(),
            &LoadErrors {
                config: errors,
                theme: Vec::new(),
            },
            None,
            Appearance::Dark,
        );
        assert!(model.locked(SettingsSection::KeyboardShortcuts).is_some());
        let mut view = View {
            capturing: Some(Action::DetachPane),
            ..View::default()
        };
        let (key, physical) = letter("d", KeyCode::KeyD);
        assert_eq!(view.captured_key(&model, &key, physical, CTRL_ALT), vec![]);
        assert!(!view.capturing());
    }

    #[test]
    fn a_locked_config_file_keeps_the_capture() {
        let model = model("[font\n");
        assert!(model.locked(SettingsSection::Font).is_some());
        assert!(model.locked(SettingsSection::KeyboardShortcuts).is_none());
        let mut view = View {
            capturing: Some(Action::DetachPane),
            ..View::default()
        };
        let (key, physical) = letter("d", KeyCode::KeyD);
        assert_eq!(view.captured_key(&model, &key, physical, CTRL_ALT).len(), 1);
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

    const RED: Rgb = Rgb { r: 255, g: 0, b: 0 };
    const GREEN: Rgb = Rgb { r: 0, g: 255, b: 0 };
    const BLUE: Rgb = Rgb { r: 0, g: 0, b: 255 };

    #[test]
    fn a_color_is_written_once_when_its_picker_closes() {
        let mut view = View::default();
        let key = ColorKey::NormalRed;
        // Dragged while open: nothing written, the picker's color is shown.
        assert_eq!(view.picker_frame(key, Some(RED), RED, true), None);
        assert_eq!(view.picker_frame(key, Some(RED), GREEN, true), None);
        assert_eq!(view.picker_frame(key, Some(RED), BLUE, true), None);
        assert_eq!(view.shown_color(key, Some(RED)), Some(BLUE));
        // Closed (a click outside or Escape): written once.
        assert_eq!(
            view.picker_frame(key, Some(RED), BLUE, false),
            Some(ThemeOp::SetColor(key, Some(BLUE)))
        );
        assert_eq!(view.picker_frame(key, Some(RED), BLUE, false), None);
        assert_eq!(view.shown_color(key, Some(RED)), Some(RED));
    }

    fn typed(view: &mut View, key: ColorKey, text: &str) {
        view.hex = Some(HexInput {
            key,
            text: text.to_owned(),
            invalid: false,
        });
    }

    #[test]
    fn a_typed_or_pasted_code_is_written_when_its_field_loses_focus() {
        let mut view = View::default();
        let key = ColorKey::NormalRed;
        for text in ["#00ff00", "00FF00", "  #00ff00 "] {
            typed(&mut view, key, text);
            assert_eq!(
                view.commit_hex(key, Some(RED)),
                Some(ThemeOp::SetColor(key, Some(GREEN)))
            );
            assert_eq!(view.hex, None);
        }
    }

    #[test]
    fn a_typed_code_equal_to_the_file_writes_nothing() {
        let mut view = View::default();
        typed(&mut view, ColorKey::Background, "#ff0000");
        assert_eq!(view.commit_hex(ColorKey::Background, Some(RED)), None);
        assert_eq!(view.hex, None);
    }

    #[test]
    fn an_invalid_code_stays_shown_with_its_error() {
        let mut view = View::default();
        let key = ColorKey::Cursor;
        for text in ["", "#ff00", "red", "#gg0000", "#ff0000ff"] {
            typed(&mut view, key, text);
            assert_eq!(view.commit_hex(key, Some(RED)), None);
            let hex = view.hex.clone().unwrap();
            assert_eq!(hex.text, text);
            assert!(hex.invalid);
        }
        // Another key's field losing focus does not take it.
        assert_eq!(view.commit_hex(ColorKey::Foreground, Some(RED)), None);
        assert!(view.hex.is_some());
        // A reload drops it.
        view.reset_inputs();
        assert_eq!(view.hex, None);
    }

    #[test]
    fn a_picker_closed_on_the_saved_color_writes_nothing() {
        let mut view = View::default();
        let key = ColorKey::Background;
        view.picker_frame(key, Some(RED), GREEN, true);
        view.picker_frame(key, Some(RED), RED, true);
        assert_eq!(view.picker_frame(key, Some(RED), RED, false), None);
    }

    #[test]
    fn opening_another_picker_writes_the_first_one() {
        let mut view = View::default();
        view.picker_frame(ColorKey::Cursor, Some(RED), GREEN, true);
        assert_eq!(
            view.picker_frame(ColorKey::PaneBorder, Some(BLUE), BLUE, true),
            Some(ThemeOp::SetColor(ColorKey::Cursor, Some(GREEN)))
        );
        // A row whose picker is closed leaves the open one alone.
        assert_eq!(
            view.picker_frame(ColorKey::Cursor, Some(RED), RED, false),
            None
        );
        assert_eq!(
            view.shown_color(ColorKey::PaneBorder, Some(BLUE)),
            Some(BLUE)
        );
    }

    #[test]
    fn the_picked_color_is_previewed_while_the_picker_is_open() {
        let mut view = View::default();
        assert_eq!(view.picked(), None);
        view.picker_frame(ColorKey::Background, Some(RED), RED, true);
        assert_eq!(view.picked(), Some((ColorKey::Background, RED)));
        view.picker_frame(ColorKey::Background, Some(RED), GREEN, true);
        assert_eq!(view.picked(), Some((ColorKey::Background, GREEN)));
        // Closing writes the color and ends the preview.
        assert_eq!(
            view.picker_frame(ColorKey::Background, Some(RED), GREEN, false),
            Some(ThemeOp::SetColor(ColorKey::Background, Some(GREEN)))
        );
        assert_eq!(view.picked(), None);
    }

    #[test]
    fn closing_the_window_writes_the_open_picker() {
        let mut view = View::default();
        view.picker_frame(ColorKey::BannerText, Some(RED), GREEN, true);
        assert_eq!(
            view.finish_picking(),
            Some(ThemeOp::SetColor(ColorKey::BannerText, Some(GREEN)))
        );
        assert_eq!(view.finish_picking(), None);
    }

    fn theme_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("graphene-view-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("themes")).unwrap();
        std::fs::write(
            dir.join("themes/mine.toml"),
            "[colors]\nforeground = \"#ffffff\"\n",
        )
        .unwrap();
        dir
    }

    fn model_in(dir: &std::path::Path, text: &str) -> SettingsModel {
        let file = dir.join("config.toml");
        let (config, errors) = Config::parse(text, &Config::default(), &file);
        SettingsModel::new(
            &config,
            &LoadErrors {
                config: errors,
                theme: Vec::new(),
            },
            Some(dir.to_owned()),
            Appearance::Dark,
        )
    }

    fn theme_name(name: &str) -> Change {
        Change::Set(
            vec!["theme".into(), "name".into()],
            Value::Text(name.into()),
        )
    }

    #[test]
    fn a_built_in_theme_can_only_be_duplicated() {
        let model = model("");
        assert!(model.edited_is_built_in());
        let mut view = View::default();
        view.start_naming(&model, NameAction::Rename);
        assert_eq!(view.naming, None);
        view.request_delete(&model);
        assert!(!view.confirm_delete);
        // Even forced, no deletion comes out.
        view.confirm_delete = true;
        let mut edits = Edits::default();
        view.confirm_delete(&model, &mut edits);
        assert_eq!(edits, Edits::default());
        view.start_naming(&model, NameAction::Duplicate);
        assert_eq!(
            view.naming,
            Some(Naming {
                action: NameAction::Duplicate,
                text: format!("{}-copy", model.edited_theme()),
                error: None,
            })
        );
    }

    #[test]
    fn a_refused_name_stays_shown_with_its_error() {
        let model = model("");
        let mut view = View::default();
        view.start_naming(&model, NameAction::Duplicate);
        let mut edits = Edits::default();
        view.naming.as_mut().unwrap().text = model.edited_theme().to_owned();
        view.confirm_name(&model, &mut edits);
        assert_eq!(edits, Edits::default());
        assert_eq!(view.naming.as_ref().unwrap().error, Some(NameError::Taken));
        view.naming.as_mut().unwrap().text = "  new-theme ".into();
        view.confirm_name(&model, &mut edits);
        assert_eq!(view.naming, None);
        assert_eq!(
            edits.theme_ops,
            vec![(
                vec![theme_name("new-theme")],
                ThemeOp::Duplicate("new-theme".into())
            )]
        );
    }

    #[test]
    fn a_user_theme_can_be_renamed_and_deleted_after_confirmation() {
        let dir = theme_dir("user");
        let model = model_in(&dir, "[theme]\nname = \"mine\"\n");
        assert!(!model.edited_is_built_in());
        let mut view = View::default();
        view.start_naming(&model, NameAction::Rename);
        assert_eq!(view.naming.as_ref().unwrap().text, "mine");
        view.naming.as_mut().unwrap().text = "ours".into();
        let mut edits = Edits::default();
        view.confirm_name(&model, &mut edits);
        assert_eq!(
            edits.theme_ops,
            vec![(vec![theme_name("ours")], ThemeOp::Rename("ours".into()))]
        );

        let mut edits = Edits::default();
        view.request_delete(&model);
        assert!(view.confirm_delete);
        // Escape dismisses the confirmation.
        assert!(view.escape());
        view.confirm_delete(&model, &mut edits);
        assert_eq!(edits, Edits::default());
        view.request_delete(&model);
        view.confirm_delete(&model, &mut edits);
        assert_eq!(
            edits.theme_ops,
            vec![(vec![theme_name("graphene-night")], ThemeOp::Delete)]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn switching_the_edited_theme_writes_the_open_picker_first() {
        let model = model("");
        let mut view = View::default();
        view.picker_frame(ColorKey::Cursor, Some(RED), GREEN, true);
        view.start_naming(&model, NameAction::Duplicate);
        let mut edits = Edits::default();
        view.switch_theme(&model, "gruvbox-dark".into(), &mut edits);
        assert_eq!(
            edits.theme_ops,
            vec![(vec![], ThemeOp::SetColor(ColorKey::Cursor, Some(GREEN)))]
        );
        assert_eq!(edits.edit_theme.as_deref(), Some("gruvbox-dark"));
        assert_eq!(view.naming, None);
        assert_eq!(view.picking, None);
    }

    #[test]
    fn the_theme_section_draws_the_editor_without_writing() {
        let dir = theme_dir("draw");
        for text in ["", "[theme]\nname = \"mine\"\n"] {
            let model = model_in(&dir, text);
            let mut view = View {
                section: SettingsSection::Theme,
                ..View::default()
            };
            let ctx = egui::Context::default();
            for _ in 0..2 {
                let mut edits = None;
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    edits = Some(view.show(ui, Lang::Fr, &model));
                });
                output.textures_delta.clear();
                assert_eq!(edits, Some(Edits::default()));
            }
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_version_label_shows_the_build_version() {
        assert_eq!(version_text(), format!("Graphene {}", crate::VERSION));
        let ctx = egui::Context::default();
        let mut clicked = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            clicked = Some(version_label(ui).clicked());
        });
        output.textures_delta.clear();
        assert_eq!(clicked, Some(false));
    }

    /// The edits of clicks on the version at `start + offset` milliseconds.
    fn version_clicks(view: &mut View, model: &SettingsModel, offsets: &[u64]) -> Edits {
        let start = Instant::now();
        let mut edits = Edits::default();
        for offset in offsets {
            let now = start + std::time::Duration::from_millis(*offset);
            view.version_click(model, now, &mut edits);
        }
        edits
    }

    fn set(path: &[&str], value: Value) -> Change {
        Change::Set(path.iter().map(|s| (*s).to_owned()).collect(), value)
    }

    const HIDDEN: &str = crate::easter_eggs::HIDDEN_THEME;

    #[test]
    fn six_clicks_on_the_version_unlock_nothing() {
        let model = model("");
        let mut view = View::default();
        let edits = version_clicks(&mut view, &model, &[0, 100, 200, 300, 400, 500]);
        assert_eq!(edits, Edits::default());
    }

    #[test]
    fn seven_spread_clicks_on_the_version_unlock_nothing() {
        let model = model("");
        let mut view = View::default();
        let offsets = [0, 400, 800, 1200, 1600, 2000, 2400];
        assert_eq!(
            version_clicks(&mut view, &model, &offsets),
            Edits::default()
        );
    }

    #[test]
    fn seven_quick_clicks_unlock_and_activate_the_hidden_theme() {
        let model = model("[theme]\nname = \"gruvbox-dark\"\n");
        let mut view = View::default();
        let edits = version_clicks(&mut view, &model, &[0, 100, 200, 300, 400, 500, 600]);
        assert_eq!(
            edits.changes,
            vec![
                set(&["theme", "unlocked"], Value::List(vec![HIDDEN.into()])),
                set(&["theme", "name"], Value::Text(HIDDEN.into())),
            ]
        );
        assert!(view.version_clicks.is_empty());
    }

    #[test]
    fn unlocking_replaces_the_current_appearance_theme_when_following_it() {
        // The test model is under the dark appearance.
        let model = model("[theme]\nlight = \"graphene-day\"\ndark = \"graphene-night\"\n");
        let mut view = View::default();
        let edits = version_clicks(&mut view, &model, &[0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(
            edits.changes,
            vec![
                set(&["theme", "unlocked"], Value::List(vec![HIDDEN.into()])),
                set(&["theme", "dark"], Value::Text(HIDDEN.into())),
            ]
        );
    }

    #[test]
    fn an_unlocked_hidden_theme_is_only_made_active_again() {
        let model = model(&format!("[theme]\nunlocked = [\"{HIDDEN}\"]\n"));
        let mut view = View::default();
        let edits = version_clicks(&mut view, &model, &[0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(
            edits.changes,
            vec![set(&["theme", "name"], Value::Text(HIDDEN.into()))]
        );
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
