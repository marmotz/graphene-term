//! Single-line text editing shared by the command palette and the tab rename box (pure,
//! no GPU access): typed characters are appended, `Backspace` erases the last one. A
//! field opened with a text has it all selected, so that typing replaces it.

use winit::keyboard::{Key, NamedKey};

/// The text of a single-line field, the caret always after it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextField {
    text: String,
    /// The whole text is selected: the next edit replaces it.
    selected: bool,
}

impl TextField {
    /// A field holding `text`, all selected.
    pub fn with_selected(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            selected: !text.is_empty(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether the whole text is selected.
    pub fn selected(&self) -> bool {
        self.selected
    }

    /// Applies a key: `text` (the characters it types, control characters left out) is
    /// appended, `Backspace` erases the last character, and either replaces a selected
    /// text; `Left`, `Right`, `Home` and `End` only drop the selection. Returns whether
    /// the text changed.
    pub fn key(&mut self, key: &Key, text: Option<&str>) -> bool {
        match key {
            Key::Named(NamedKey::Backspace) => {
                if std::mem::take(&mut self.selected) {
                    self.text.clear();
                    return true;
                }
                self.text.pop().is_some()
            }
            Key::Named(
                NamedKey::ArrowLeft | NamedKey::ArrowRight | NamedKey::Home | NamedKey::End,
            ) => {
                self.selected = false;
                false
            }
            _ => {
                let typed: String = text
                    .unwrap_or_default()
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect();
                if typed.is_empty() {
                    return false;
                }
                if std::mem::take(&mut self.selected) {
                    self.text.clear();
                }
                self.text.push_str(&typed);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(field: &mut TextField, text: &str) -> bool {
        field.key(&Key::Character(text.into()), Some(text))
    }

    fn named(field: &mut TextField, key: NamedKey) -> bool {
        field.key(&Key::Named(key), None)
    }

    #[test]
    fn typing_appends_and_backspace_erases() {
        let mut field = TextField::default();
        assert!(!field.selected());
        assert!(typed(&mut field, "ab"));
        assert!(typed(&mut field, "é"));
        assert_eq!(field.text(), "abé");
        assert!(named(&mut field, NamedKey::Backspace));
        assert_eq!(field.text(), "ab");
        assert!(named(&mut field, NamedKey::Backspace));
        assert!(named(&mut field, NamedKey::Backspace));
        assert!(!named(&mut field, NamedKey::Backspace), "nothing left");
        assert_eq!(field.text(), "");
    }

    #[test]
    fn control_characters_are_not_text() {
        let mut field = TextField::default();
        assert!(!field.key(&Key::Character("p".into()), Some("\x10")));
        assert!(!field.key(&Key::Named(NamedKey::Shift), None));
        assert_eq!(field.text(), "");
    }

    #[test]
    fn a_selected_text_is_replaced_by_the_first_edit() {
        let mut field = TextField::with_selected("~/dev");
        assert!(field.selected());
        assert!(typed(&mut field, "x"));
        assert_eq!(field.text(), "x");
        assert!(!field.selected());

        let mut field = TextField::with_selected("~/dev");
        assert!(named(&mut field, NamedKey::Backspace));
        assert_eq!(field.text(), "");
        assert!(!field.selected());
    }

    #[test]
    fn moving_the_caret_keeps_the_text() {
        let mut field = TextField::with_selected("~/dev");
        assert!(!named(&mut field, NamedKey::End));
        assert!(!field.selected());
        assert!(typed(&mut field, "/x"));
        assert_eq!(field.text(), "~/dev/x");
        assert!(!TextField::with_selected("").selected());
    }
}
