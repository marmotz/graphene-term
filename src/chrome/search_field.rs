//! Text field of the search bar (pure, no GPU access): a single-line editor with a
//! caret, unlike [`super::text_field::TextField`] whose caret stays after the text.
//! Typed text goes in at the caret; the arrows, `Home` and `End` move it; `Backspace`,
//! `Ctrl+Backspace` and `Delete` erase around it. A selected text, the whole of it, is
//! replaced by the next edit.

use winit::keyboard::{Key, NamedKey};

/// The text of the search field and its caret.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchField {
    text: String,
    /// Byte offset of the caret in `text`, always on a character boundary.
    caret: usize,
    /// The whole text is selected: the next edit replaces it.
    selected: bool,
}

impl SearchField {
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Byte offset of the caret in [`Self::text`].
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Whether the whole text is selected.
    pub fn selected(&self) -> bool {
        self.selected
    }

    /// Selects the whole text, the caret after it. An empty text has nothing to select.
    pub fn select_all(&mut self) {
        self.selected = !self.text.is_empty();
        self.caret = self.text.len();
    }

    /// Inserts `typed` at the caret, over a selected text. Line breaks are replaced by
    /// spaces, trailing ones dropped, and the other control characters left out, so a
    /// paste stays on one line. Returns whether the text changed.
    pub fn insert(&mut self, typed: &str) -> bool {
        let line = typed.lines().collect::<Vec<_>>().join(" ");
        let clean: String = line.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return false;
        }
        self.take_selection();
        self.text.insert_str(self.caret, &clean);
        self.caret += clean.len();
        true
    }

    /// Applies a key: `text` is what it types, `ctrl` whether `Ctrl` is held (then
    /// `Backspace` erases the word before the caret). Returns whether the text changed;
    /// the caret may move without it.
    pub fn key(&mut self, key: &Key, text: Option<&str>, ctrl: bool) -> bool {
        match key {
            Key::Named(NamedKey::Backspace) => {
                if self.take_selection() {
                    return true;
                }
                let start = if ctrl {
                    word_start(&self.text, self.caret)
                } else {
                    previous_boundary(&self.text, self.caret)
                };
                self.erase(start, self.caret)
            }
            Key::Named(NamedKey::Delete) => {
                if self.take_selection() {
                    return true;
                }
                let end = next_boundary(&self.text, self.caret);
                self.erase(self.caret, end)
            }
            Key::Named(NamedKey::ArrowLeft) => {
                if std::mem::take(&mut self.selected) {
                    self.caret = 0;
                } else {
                    self.caret = previous_boundary(&self.text, self.caret);
                }
                false
            }
            Key::Named(NamedKey::ArrowRight) => {
                if !std::mem::take(&mut self.selected) {
                    self.caret = next_boundary(&self.text, self.caret);
                }
                false
            }
            Key::Named(NamedKey::Home) => {
                self.selected = false;
                self.caret = 0;
                false
            }
            Key::Named(NamedKey::End) => {
                self.selected = false;
                self.caret = self.text.len();
                false
            }
            _ if ctrl => false,
            _ => self.insert(text.unwrap_or_default()),
        }
    }

    /// Erases a selected text; whether there was one.
    fn take_selection(&mut self) -> bool {
        if !std::mem::take(&mut self.selected) {
            return false;
        }
        self.text.clear();
        self.caret = 0;
        true
    }

    /// Erases the bytes `start..end`, the caret going to `start`.
    fn erase(&mut self, start: usize, end: usize) -> bool {
        if start == end {
            return false;
        }
        self.text.replace_range(start..end, "");
        self.caret = start;
        true
    }
}

/// Offset of the character before `at`, `0` at the start.
fn previous_boundary(text: &str, at: usize) -> usize {
    text[..at].char_indices().next_back().map_or(0, |(i, _)| i)
}

/// Offset after the character at `at`, the end of the text at its end.
fn next_boundary(text: &str, at: usize) -> usize {
    text[at..].chars().next().map_or(at, |c| at + c.len_utf8())
}

/// Start of the word before `at`: past the spaces before it, then past the word.
fn word_start(text: &str, at: usize) -> usize {
    let before = &text[..at];
    let trimmed = before.trim_end();
    trimmed
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(i, c)| i + c.len_utf8())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(field: &mut SearchField, text: &str) -> bool {
        field.key(&Key::Character(text.into()), Some(text), false)
    }

    fn named(field: &mut SearchField, key: NamedKey) -> bool {
        field.key(&Key::Named(key), None, false)
    }

    fn field(text: &str) -> SearchField {
        let mut field = SearchField::default();
        field.insert(text);
        field
    }

    #[test]
    fn typing_goes_in_at_the_caret() {
        let mut field = field("ac");
        assert_eq!(field.caret(), 2);
        assert!(!named(&mut field, NamedKey::ArrowLeft));
        assert!(typed(&mut field, "b"));
        assert_eq!(field.text(), "abc");
        assert_eq!(field.caret(), 2);
        assert!(!named(&mut field, NamedKey::Home));
        assert!(typed(&mut field, "é"));
        assert_eq!(field.text(), "éabc");
        assert!(!named(&mut field, NamedKey::End));
        assert!(typed(&mut field, "!"));
        assert_eq!(field.text(), "éabc!");
    }

    #[test]
    fn arrows_move_by_one_character_and_stop_at_the_ends() {
        let mut field = field("éa");
        named(&mut field, NamedKey::ArrowLeft);
        assert_eq!(field.caret(), "é".len());
        named(&mut field, NamedKey::ArrowLeft);
        named(&mut field, NamedKey::ArrowLeft);
        assert_eq!(field.caret(), 0);
        named(&mut field, NamedKey::ArrowRight);
        assert_eq!(field.caret(), "é".len());
        named(&mut field, NamedKey::ArrowRight);
        named(&mut field, NamedKey::ArrowRight);
        assert_eq!(field.caret(), field.text().len());
    }

    #[test]
    fn backspace_and_delete_erase_around_the_caret() {
        let mut field = field("abé");
        assert!(named(&mut field, NamedKey::Backspace));
        assert_eq!(field.text(), "ab");
        named(&mut field, NamedKey::Home);
        assert!(!named(&mut field, NamedKey::Backspace), "nothing before");
        assert!(named(&mut field, NamedKey::Delete));
        assert_eq!(field.text(), "b");
        named(&mut field, NamedKey::End);
        assert!(!named(&mut field, NamedKey::Delete), "nothing after");
    }

    #[test]
    fn ctrl_backspace_erases_the_word_before_the_caret() {
        let mut field = field("git log  --oneline  ");
        assert!(field.key(&Key::Named(NamedKey::Backspace), None, true));
        assert_eq!(field.text(), "git log  ");
        assert!(field.key(&Key::Named(NamedKey::Backspace), None, true));
        assert_eq!(field.text(), "git ");
        assert!(field.key(&Key::Named(NamedKey::Backspace), None, true));
        assert_eq!(field.text(), "");
        assert!(!field.key(&Key::Named(NamedKey::Backspace), None, true));
    }

    #[test]
    fn a_selected_text_is_replaced_by_the_next_edit() {
        let mut field = field("error");
        field.select_all();
        assert!(field.selected());
        assert!(typed(&mut field, "w"));
        assert_eq!(field.text(), "w");
        assert!(!field.selected());

        let mut field = self::field("error");
        field.select_all();
        assert!(named(&mut field, NamedKey::Delete));
        assert_eq!(field.text(), "");

        let mut field = self::field("error");
        field.select_all();
        assert!(!named(&mut field, NamedKey::ArrowLeft));
        assert_eq!((field.text(), field.caret()), ("error", 0));
        assert!(!field.selected());

        let mut empty = SearchField::default();
        empty.select_all();
        assert!(!empty.selected());
    }

    #[test]
    fn a_paste_stays_on_one_line() {
        let mut field = SearchField::default();
        assert!(field.insert("cargo\ntest\r\n"));
        assert_eq!(field.text(), "cargo test");
        assert!(!field.insert("\n"));
        assert!(field.insert("\tx"));
        assert_eq!(field.text(), "cargo testx");
    }

    #[test]
    fn control_characters_and_ctrl_keys_type_nothing() {
        let mut field = SearchField::default();
        assert!(!field.key(&Key::Character("c".into()), Some("\x03"), false));
        assert!(!field.key(&Key::Character("a".into()), Some("a"), true));
        assert!(!field.key(&Key::Named(NamedKey::Shift), None, false));
        assert_eq!(field.text(), "");
    }
}
