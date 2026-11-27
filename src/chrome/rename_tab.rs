//! Box renaming a tab (pure, no GPU access): its text field, prefilled with the shown
//! title, the keys it reacts to and where it sits, centered like the command palette.

use winit::keyboard::{Key, NamedKey};

use super::command_palette::{MARGIN, MAX_WIDTH, PADDING, ROW_PADDING, TEXT_PADDING, TOP};
use super::tab_bar::UiMetrics;
use super::text_field::TextField;
use crate::i18n::{self, Lang, Msg};
use crate::layout::Rect;
use crate::terminal::PaneId;

/// What a key asks of the open rename box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameOutcome {
    /// Close the box and give the tab holding the pane `pane` this title, trimmed; an
    /// empty one goes back to the automatic title.
    Rename { pane: PaneId, title: String },
    /// Close the box and leave the title alone.
    Cancel,
}

/// The open rename box of a tab, known by one of its panes so that it stays the same
/// tab when another one closes meanwhile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameTab {
    pane: PaneId,
    field: TextField,
}

impl RenameTab {
    /// A box renaming the tab holding `pane`, its field holding `title`, all selected.
    pub fn new(pane: PaneId, title: &str) -> Self {
        Self {
            pane,
            field: TextField::with_selected(title),
        }
    }

    pub fn field(&self) -> &TextField {
        &self.field
    }

    /// Handles a key: `Enter` renames, `Escape` cancels, the other keys edit the field
    /// (see [`TextField::key`]). `None` while the box stays open.
    pub fn key(&mut self, key: &Key, text: Option<&str>) -> Option<RenameOutcome> {
        match key {
            Key::Named(NamedKey::Escape) => Some(RenameOutcome::Cancel),
            Key::Named(NamedKey::Enter) => Some(RenameOutcome::Rename {
                pane: self.pane,
                title: self.field.text().trim().to_owned(),
            }),
            _ => {
                self.field.key(key, text);
                None
            }
        }
    }
}

/// The rename box placed in a window, in physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct RenameTabLayout {
    pub rect: Rect,
    /// Heading of the box and its left edge and top.
    pub heading: String,
    pub heading_x: f32,
    pub heading_y: f32,
    /// The text field.
    pub input: Rect,
    pub input_text: String,
    /// Left edge and top of the field text.
    pub text_x: f32,
    pub input_text_y: f32,
    /// Width the field text and the heading are truncated to.
    pub text_width: f32,
    /// The text cursor, after the text.
    pub caret: Rect,
    /// Highlight behind the text while it is all selected.
    pub selection: Option<Rect>,
}

impl RenameTabLayout {
    /// Lays `rename` out in a `window`: centered and at most [`MAX_WIDTH`] wide like the
    /// palette, its top at [`TOP`] of the height, a heading above the field; texts are
    /// measured by `measure` in pixels.
    pub fn new(
        rename: &RenameTab,
        lang: Lang,
        window: Rect,
        metrics: UiMetrics,
        mut measure: impl FnMut(&str) -> f32,
    ) -> Self {
        let scale = metrics.scale;
        let line = metrics.line_height;
        let (padding, row_padding, text_padding) =
            (PADDING * scale, ROW_PADDING * scale, TEXT_PADDING * scale);
        let width = (MAX_WIDTH * scale)
            .min(window.width - 2.0 * MARGIN * scale)
            .max(0.0)
            .round();
        let row_height = (line + 2.0 * row_padding).round();
        let x = (window.x + (window.width - width) / 2.0).round();
        let y = (window.y + window.height * TOP).round();
        let inner_width = (width - 2.0 * padding).max(0.0);

        let heading_x = x + padding + text_padding;
        let heading_y = y + padding + row_padding;
        let input = Rect {
            x: x + padding,
            y: y + padding + row_height,
            width: inner_width,
            height: row_height,
        };
        let text_x = input.x + text_padding;
        let text_width = (input.width - 2.0 * text_padding).max(0.0);
        let input_text_y = input.y + row_padding;
        let input_text = rename.field().text().to_owned();
        let text_end = measure(&input_text).min(text_width);
        let caret = Rect {
            x: (text_x + text_end).round(),
            y: input_text_y,
            width: scale.round().max(1.0),
            height: line,
        };
        let selection = rename.field().selected().then(|| Rect {
            x: text_x,
            y: input_text_y,
            width: text_end.round(),
            height: line,
        });
        Self {
            rect: Rect {
                x,
                y,
                width,
                height: 2.0 * row_height + 2.0 * padding,
            },
            heading: i18n::text(lang, &Msg::RenameTab),
            heading_x,
            heading_y,
            input,
            input_text,
            text_x,
            input_text_y,
            text_width,
            caret,
            selection,
        }
    }

    /// Whether the pixel (`x`, `y`) is in the box.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.rect.contains(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(rename: &mut RenameTab, text: &str) -> Option<RenameOutcome> {
        rename.key(&Key::Character(text.into()), Some(text))
    }

    fn named(rename: &mut RenameTab, key: NamedKey) -> Option<RenameOutcome> {
        rename.key(&Key::Named(key), None)
    }

    #[test]
    fn enter_renames_with_the_trimmed_text() {
        let mut rename = RenameTab::new(PaneId(2), "~/dev");
        assert_eq!(typed(&mut rename, " build"), None);
        assert_eq!(typed(&mut rename, " "), None);
        assert_eq!(
            named(&mut rename, NamedKey::Enter),
            Some(RenameOutcome::Rename {
                pane: PaneId(2),
                title: "build".to_owned()
            })
        );
    }

    #[test]
    fn an_emptied_field_renames_to_nothing() {
        let mut rename = RenameTab::new(PaneId(0), "~/dev");
        assert_eq!(named(&mut rename, NamedKey::Backspace), None);
        assert_eq!(
            named(&mut rename, NamedKey::Enter),
            Some(RenameOutcome::Rename {
                pane: PaneId(0),
                title: String::new()
            })
        );
    }

    #[test]
    fn escape_cancels() {
        let mut rename = RenameTab::new(PaneId(0), "~/dev");
        typed(&mut rename, "x");
        assert_eq!(
            named(&mut rename, NamedKey::Escape),
            Some(RenameOutcome::Cancel)
        );
    }

    fn layout(rename: &RenameTab, window: Rect) -> RenameTabLayout {
        let metrics = UiMetrics {
            line_height: 18.0,
            scale: 1.0,
        };
        RenameTabLayout::new(rename, Lang::En, window, metrics, |t| {
            t.chars().count() as f32 * 8.0
        })
    }

    #[test]
    fn layout_is_centered_like_the_palette_with_a_heading() {
        let rename = RenameTab::new(PaneId(0), "~/dev");
        let layout = layout(&rename, Rect::from_size(1000, 600));
        assert_eq!(layout.rect.width, 600.0);
        assert_eq!(layout.rect.x, 200.0);
        assert_eq!(layout.rect.y, 90.0);
        assert_eq!(layout.heading, "Rename the tab");
        assert!(layout.heading_y < layout.input.y);
        assert_eq!(layout.input_text, "~/dev");
        // The prefilled text is selected and the caret sits after it.
        let selection = layout.selection.unwrap();
        assert_eq!(selection.x, layout.text_x);
        assert_eq!(selection.width, 40.0);
        assert_eq!(layout.caret.x, layout.text_x + 40.0);
        assert!(layout.rect.contains(layout.input.x, layout.input.y));
        assert!(layout.contains(250.0, 100.0));
        assert!(!layout.contains(10.0, 10.0));
    }

    #[test]
    fn layout_drops_the_selection_once_edited() {
        let mut rename = RenameTab::new(PaneId(0), "~/dev");
        typed(&mut rename, "ab");
        let layout = layout(&rename, Rect::from_size(300, 600));
        assert_eq!(layout.selection, None);
        assert_eq!(layout.input_text, "ab");
        // A narrow window keeps the margins.
        assert_eq!(layout.rect.width, 268.0);
    }
}
