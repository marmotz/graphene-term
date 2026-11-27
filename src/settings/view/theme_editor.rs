//! The theme section: color pickers, hexadecimal codes, and the naming, duplication and deletion of user themes.

use super::*;

impl View {
    /// The key and color of the open color picker, previewed until it closes.
    pub fn picked(&self) -> Option<(ColorKey, Rgb)> {
        self.picking.map(|picking| (picking.key, picking.color))
    }

    /// Closes the open color picker; returns its color to write when it changed.
    /// Called when the picker is no longer drawn (the window closes, another section or
    /// theme is shown).
    pub fn finish_picking(&mut self) -> Option<ThemeOp> {
        let picking = self.picking.take()?;
        (Some(picking.color) != picking.saved)
            .then_some(ThemeOp::SetColor(picking.key, Some(picking.color)))
    }

    /// Takes one frame of the color picker of `key`, whose file color is `saved`, shown
    /// with `color`, open or not. The color is written once, when the picker closes,
    /// not while it is dragged.
    pub(super) fn picker_frame(
        &mut self,
        key: ColorKey,
        saved: Option<Rgb>,
        color: Rgb,
        open: bool,
    ) -> Option<ThemeOp> {
        let current = self.picking.map(|picking| picking.key);
        match (open, current) {
            (true, Some(picked)) if picked == key => {
                self.picking = Some(Picking { key, color, saved });
                None
            }
            (true, _) => {
                let done = self.finish_picking();
                self.picking = Some(Picking { key, color, saved });
                done
            }
            (false, Some(picked)) if picked == key => self.finish_picking(),
            (false, _) => None,
        }
    }

    /// Takes the typed code of `key` when its field loses focus: a valid code other than
    /// `saved` gives its color to write, an invalid one stays shown with its error.
    pub(super) fn commit_hex(&mut self, key: ColorKey, saved: Option<Rgb>) -> Option<ThemeOp> {
        let hex = self.hex.as_mut().filter(|hex| hex.key == key)?;
        match parse_hex(&hex.text) {
            Some(color) => {
                self.hex = None;
                (Some(color) != saved).then_some(ThemeOp::SetColor(key, Some(color)))
            }
            None => {
                hex.invalid = true;
                None
            }
        }
    }

    /// The color shown for `key`: the picker's while it is open, else `saved`.
    pub(super) fn shown_color(&self, key: ColorKey, saved: Option<Rgb>) -> Option<Rgb> {
        self.picking
            .filter(|picking| picking.key == key)
            .map(|picking| picking.color)
            .or(saved)
    }

    /// Shows the name field of a copy or a rename; a built-in theme cannot be renamed.
    pub(super) fn start_naming(&mut self, model: &SettingsModel, action: NameAction) {
        if action == NameAction::Rename && model.edited_is_built_in() {
            return;
        }
        let edited = model.edited_theme();
        let text = match action {
            NameAction::Duplicate => format!("{edited}-copy"),
            NameAction::Rename => edited.to_owned(),
        };
        self.confirm_delete = false;
        self.naming = Some(Naming {
            action,
            text,
            error: None,
        });
    }

    /// Takes the typed name: a valid one gives its operation, a refused one stays shown
    /// with its error.
    pub(super) fn confirm_name(&mut self, model: &SettingsModel, edits: &mut Edits) {
        let Some(naming) = &mut self.naming else {
            return;
        };
        let name = naming.text.trim().to_owned();
        let op = match naming.action {
            NameAction::Duplicate => ThemeOp::Duplicate(name),
            NameAction::Rename => ThemeOp::Rename(name),
        };
        match edits.theme_op(model, op) {
            Ok(()) => self.naming = None,
            Err(error) => naming.error = Some(error),
        }
    }

    /// Asks to confirm the deletion of the edited theme; a built-in theme cannot be
    /// deleted.
    pub(super) fn request_delete(&mut self, model: &SettingsModel) {
        if model.edited_is_built_in() {
            return;
        }
        self.naming = None;
        self.confirm_delete = true;
    }

    /// Deletes the edited theme once confirmed.
    pub(super) fn confirm_delete(&mut self, model: &SettingsModel, edits: &mut Edits) {
        if std::mem::take(&mut self.confirm_delete) && !model.edited_is_built_in() {
            // Deleting checks no name: it cannot be refused.
            let _ = edits.theme_op(model, ThemeOp::Delete);
        }
    }

    /// Shows `name` in the theme editor, writing the open picker's color first.
    pub(super) fn switch_theme(&mut self, model: &SettingsModel, name: String, edits: &mut Edits) {
        if let Some(op) = self.finish_picking() {
            let _ = edits.theme_op(model, op);
        }
        self.naming = None;
        self.confirm_delete = false;
        self.hex = None;
        edits.edit_theme = Some(name);
    }

    /// The theme editor, below the theme fields: what it is for, a box with the theme
    /// selector (independent of the active theme) and its actions, then the colors of
    /// every group in one grid, so their swatches line up. A built-in theme can only be
    /// duplicated.
    pub(super) fn theme_editor(
        &mut self,
        ui: &mut Ui,
        lang: Lang,
        model: &SettingsModel,
        edits: &mut Edits,
    ) {
        let label = |msg: Msg| i18n::text(lang, &msg);
        let editable = !model.edited_is_built_in();
        ui.add_space(24.0);
        ui.show_heading(label(Msg::ThemeEditor));
        ui.separator();
        ui.show_label(label(Msg::ThemeEditorIntro));
        ui.add_space(8.0);
        Frame::group(ui.style())
            .inner_margin(Margin::same(BOX_MARGIN))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.show_label(label(Msg::EditedTheme));
                    if let Some(name) = theme_selector(ui, model) {
                        self.switch_theme(model, name, edits);
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let mut duplicate = Button::new(label(Msg::Duplicate));
                    if !editable {
                        // The only action of a built-in theme: shown as the one to take.
                        duplicate = duplicate.fill(ui.visuals().selection.bg_fill);
                    }
                    if ui.add(duplicate).clicked() {
                        self.start_naming(model, NameAction::Duplicate);
                    }
                    if ui
                        .add_enabled(editable, Button::new(label(Msg::Rename)))
                        .clicked()
                    {
                        self.start_naming(model, NameAction::Rename);
                    }
                    if ui
                        .add_enabled(editable, Button::new(label(Msg::Delete)))
                        .clicked()
                    {
                        self.request_delete(model);
                    }
                    ui.separator();
                    if ui.show_button(label(Msg::OpenThemesFolder)).clicked() {
                        edits.open_themes_dir = true;
                    }
                });
                self.naming_row(ui, lang, model, edits);
                if self.confirm_delete {
                    ui.add_space(6.0);
                    warn_label(
                        ui,
                        label(Msg::ConfirmDeleteTheme(model.edited_theme().to_owned())),
                    );
                    ui.horizontal(|ui| {
                        if ui.show_button(label(Msg::Delete)).clicked() {
                            self.confirm_delete(model, edits);
                        }
                        if ui.show_button(label(Msg::Cancel)).clicked() {
                            self.confirm_delete = false;
                        }
                    });
                }
                if !editable {
                    ui.add_space(6.0);
                    warn_label(ui, label(Msg::BuiltInTheme));
                }
            });
        ui.add_space(12.0);
        ui.show_label(label(Msg::ThemeColorsHint).weak());
        ui.add_space(4.0);
        let group_size = ui
            .style()
            .text_styles
            .get(&TextStyle::Body)
            .map_or(14.0, |font| font.size * 1.15);
        ui.add_enabled_ui(editable, |ui| {
            Grid::new("theme_colors")
                .num_columns(2)
                .spacing([24.0, 6.0])
                .show(ui, |ui| {
                    for (index, group) in ColorGroup::ALL.into_iter().enumerate() {
                        ui.vertical(|ui| {
                            ui.add_space(if index == 0 { 4.0 } else { 14.0 });
                            ui.show_label(
                                label(Msg::ThemeColorGroup(group)).strong().size(group_size),
                            );
                        });
                        ui.show_label(UiText::default());
                        ui.end_row();
                        for key in group.keys() {
                            ui.horizontal(|ui| {
                                ui.add_space(COLOR_INDENT);
                                ui.show_label(label(Msg::ThemeColor(key)));
                            });
                            ui.push_id(key, |ui| {
                                ui.horizontal(|ui| self.color_row(ui, lang, model, key, edits));
                            });
                            ui.end_row();
                        }
                    }
                });
        });
    }

    /// The name field of a copy or a rename, with its error; `Enter` confirms.
    pub(super) fn naming_row(
        &mut self,
        ui: &mut Ui,
        lang: Lang,
        model: &SettingsModel,
        edits: &mut Edits,
    ) {
        let label = |msg: Msg| i18n::text(lang, &msg);
        let Some(naming) = &mut self.naming else {
            return;
        };
        let (mut confirm, mut cancel) = (false, false);
        ui.horizontal(|ui| {
            ui.show_label(label(Msg::NewThemeName));
            let response =
                ui.add(TextEdit::singleline(&mut naming.text).desired_width(FIELD_WIDTH));
            confirm =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            confirm |= ui.show_button(label(Msg::Confirm)).clicked();
            cancel = ui.show_button(label(Msg::Cancel)).clicked();
        });
        if let Some(error) = naming.error {
            error_label(ui, label(Msg::ThemeNameError(error)));
        }
        if cancel {
            self.naming = None;
        } else if confirm {
            self.confirm_name(model, edits);
        }
    }

    /// The color button of `key`, which opens a picker; for the selection foreground, the
    /// "Keep the text color" checkbox that empties it.
    pub(super) fn color_row(
        &mut self,
        ui: &mut Ui,
        lang: Lang,
        model: &SettingsModel,
        key: ColorKey,
        edits: &mut Edits,
    ) {
        let palette = model.edited_palette();
        let saved = key.get(palette);
        let shown = self.shown_color(key, saved).unwrap_or(palette.foreground);
        let mut srgb = [shown.r, shown.g, shown.b];
        let popup = ui
            .add_enabled_ui(saved.is_some(), |ui| {
                // The id the button gives its popup: the next automatic id of its `Ui`.
                let popup = ui.auto_id_with("popup");
                egui::color_picker::color_edit_button_srgb(ui, &mut srgb);
                popup
            })
            .inner;
        let open = saved.is_some() && Popup::is_id_open(ui.ctx(), popup);
        if open && self.hex.as_ref().is_some_and(|hex| hex.key == key) {
            // The picker takes over the typed code.
            self.hex = None;
        }
        let [r, g, b] = srgb;
        if let Some(op) = self.picker_frame(key, saved, Rgb { r, g, b }, open) {
            let _ = edits.theme_op(model, op);
        }
        let typed = self.hex.as_ref().filter(|hex| hex.key == key);
        let invalid = typed.is_some_and(|hex| hex.invalid);
        let shown = self.shown_color(key, saved).unwrap_or(palette.foreground);
        let mut text = typed.map_or_else(|| format_color(shown), |hex| hex.text.clone());
        let font = TextStyle::Monospace.resolve(ui.style());
        let width = HEX_CHARS * ui.fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
        let response = ui.add_enabled(
            saved.is_some(),
            TextEdit::singleline(&mut text)
                .font(TextStyle::Monospace)
                .align(egui::Align2::CENTER_TOP)
                .desired_width(width),
        );
        if response.changed() {
            self.hex = Some(HexInput {
                key,
                text,
                invalid: false,
            });
        }
        if response.lost_focus() {
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                // Escape drops the typed code.
                self.hex = None;
            } else if let Some(op) = self.commit_hex(key, saved) {
                let _ = edits.theme_op(model, op);
            }
        }
        if key == ColorKey::SelectionForeground {
            let mut keep = saved.is_none();
            if ui
                .checkbox(&mut keep, i18n::text(lang, &Msg::KeepTextColor))
                .changed()
            {
                let color = (!keep).then_some(palette.foreground);
                let _ = edits.theme_op(model, ThemeOp::SetColor(key, color));
            }
        }
        if invalid {
            error_label(ui, i18n::text(lang, &Msg::InvalidHexColor));
        }
    }
}
