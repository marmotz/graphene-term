//! egui drawing of the settings window: the sections sidebar and the selected
//! section's panel.

use egui::{CentralPanel, Panel, Ui};

use crate::i18n::{self, Lang, Msg, SettingsSection};

/// Width of the sections sidebar, in points.
const SIDEBAR_WIDTH: f32 = 180.0;

/// State of the view kept between frames.
#[derive(Debug, Default)]
pub struct View {
    /// Section shown in the right panel.
    pub section: SettingsSection,
}

impl View {
    /// Draws one frame.
    pub fn show(&mut self, ui: &mut Ui, lang: Lang) {
        let label = |msg: Msg| i18n::text(lang, &msg).into_owned();
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
                    {
                        self.section = section;
                    }
                }
            });
        CentralPanel::default().show(ui, |ui| {
            ui.heading(label(Msg::Section(self.section)));
            ui.separator();
        });
    }
}
