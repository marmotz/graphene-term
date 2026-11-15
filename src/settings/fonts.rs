//! Text of the settings window: the command palette's sans-serif font, at the
//! palette's size, so both read alike.

use std::collections::BTreeMap;
use std::sync::Arc;

use cosmic_text::fontdb;
use egui::{FontData, FontFamily, FontId, FontInsert, FontPriority, InsertFontFamily, TextStyle};

use crate::renderer::UiFont;

/// Name the palette's font is installed under in egui.
const FONT_NAME: &str = "graphene-sans";

/// The sans-serif face `db` resolves (the one the palette is shaped with), regular
/// weight; `None` when the family is not installed, egui then keeps its own font.
pub fn sans_serif(db: &fontdb::Database) -> Option<FontData> {
    let id = db.query(&fontdb::Query {
        families: &[fontdb::Family::SansSerif],
        ..fontdb::Query::default()
    })?;
    db.with_face_data(id, |data, index| {
        FontData::from_blob(Arc::new(data.to_vec()), index)
    })
}

/// Installs `font` in front of egui's proportional fonts, egui's fonts staying as
/// fallback for the glyphs it lacks.
pub fn install(ctx: &egui::Context, font: FontData) {
    ctx.add_font(FontInsert::new(
        FONT_NAME,
        font,
        vec![InsertFontFamily {
            family: FontFamily::Proportional,
            priority: FontPriority::Highest,
        }],
    ));
}

/// Text sizes of the window: body and buttons at the palette's size, the others
/// scaled from egui's defaults by the same ratio.
pub fn text_styles() -> BTreeMap<TextStyle, FontId> {
    let body = UiFont::Palette.size();
    let ratio = body / 13.0;
    [
        (
            TextStyle::Small,
            FontId::proportional((9.0 * ratio).round()),
        ),
        (TextStyle::Body, FontId::proportional(body)),
        (TextStyle::Button, FontId::proportional(body)),
        (
            TextStyle::Heading,
            FontId::proportional((18.0 * ratio).round()),
        ),
        (TextStyle::Monospace, FontId::monospace(body)),
    ]
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_and_buttons_use_the_palette_size() {
        let styles = text_styles();
        let palette = UiFont::Palette.size();
        assert_eq!(styles[&TextStyle::Body], FontId::proportional(palette));
        assert_eq!(styles[&TextStyle::Button], FontId::proportional(palette));
        assert!(styles[&TextStyle::Heading].size > palette);
        assert!(styles[&TextStyle::Small].size < palette);
    }

    #[test]
    fn missing_sans_serif_family_keeps_the_egui_font() {
        let mut db = fontdb::Database::new();
        db.set_sans_serif_family("No Such Family");
        assert!(sans_serif(&db).is_none());
    }

    #[test]
    fn installed_sans_serif_family_is_loaded() {
        let mut db = cosmic_text::FontSystem::new().db().clone();
        let installed = db
            .faces()
            .find_map(|face| face.families.first())
            .map(|(name, _)| name.clone())
            .expect("tests need at least one system font");
        db.set_sans_serif_family(installed);
        let font = sans_serif(&db).expect("the installed family is found");
        assert!(!font.font.as_ref().as_ref().is_empty());
    }
}
