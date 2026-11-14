//! Translation of keyboard input into PTY bytes and application actions.

use alacritty_terminal::term::TermMode;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::i18n::{Direction, Msg};

/// Actions handled by Graphene Term itself rather than forwarded to the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Copy,
    Paste,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToTop,
    ScrollToBottom,
    FontSizeIncrease,
    FontSizeDecrease,
    FontSizeReset,
    PreviousBlock,
    NextBlock,
    CopyCommand,
    CopyOutput,
    ToggleFold,
    ToggleFoldAll,
    /// Splits the active pane, the new one on its right.
    SplitRight,
    /// Splits the active pane, the new one below it.
    SplitDown,
    ClosePane,
    /// Gives the focus to the neighbour pane in a direction.
    FocusPane(Direction),
    /// Grows the active pane toward a direction.
    ResizePane(Direction),
    NextPane,
    PreviousPane,
    /// Zooms the active pane to the whole tab, or back.
    ToggleZoom,
    /// Gives every pane of the tab an equal share; no default key.
    EqualisePanes,
    /// Moves the active pane into a new tab; no default key.
    DetachPane,
    /// Opens a tab after the last one.
    NewTab,
    NextTab,
    PreviousTab,
    /// Shows the tab of this number, counted from 1.
    GoToTab(u8),
    /// Opens the command palette.
    CommandPalette,
}

impl Action {
    /// Every action the command palette lists, in its default order. The tabs are
    /// listed by the palette itself, so `GoToTab` is left out, and so is the palette.
    pub const ALL: [Action; 34] = [
        Action::NewTab,
        Action::ClosePane,
        Action::SplitRight,
        Action::SplitDown,
        Action::NextTab,
        Action::PreviousTab,
        Action::NextPane,
        Action::PreviousPane,
        Action::FocusPane(Direction::Left),
        Action::FocusPane(Direction::Right),
        Action::FocusPane(Direction::Up),
        Action::FocusPane(Direction::Down),
        Action::ResizePane(Direction::Left),
        Action::ResizePane(Direction::Right),
        Action::ResizePane(Direction::Up),
        Action::ResizePane(Direction::Down),
        Action::ToggleZoom,
        Action::EqualisePanes,
        Action::DetachPane,
        Action::Copy,
        Action::Paste,
        Action::CopyCommand,
        Action::CopyOutput,
        Action::PreviousBlock,
        Action::NextBlock,
        Action::ToggleFold,
        Action::ToggleFoldAll,
        Action::ScrollPageUp,
        Action::ScrollPageDown,
        Action::ScrollToTop,
        Action::ScrollToBottom,
        Action::FontSizeIncrease,
        Action::FontSizeDecrease,
        Action::FontSizeReset,
    ];

    /// Stable name of the action: the snake_case of its variant, the direction or tab
    /// number appended (`focus_pane_left`, `go_to_tab_3`). Stored in the palette's
    /// recent file, and the name the keybindings use.
    pub fn id(self) -> String {
        let direction = |direction| match direction {
            Direction::Left => "left",
            Direction::Right => "right",
            Direction::Up => "up",
            Direction::Down => "down",
        };
        let name = match self {
            Action::Copy => "copy",
            Action::Paste => "paste",
            Action::ScrollPageUp => "scroll_page_up",
            Action::ScrollPageDown => "scroll_page_down",
            Action::ScrollToTop => "scroll_to_top",
            Action::ScrollToBottom => "scroll_to_bottom",
            Action::FontSizeIncrease => "font_size_increase",
            Action::FontSizeDecrease => "font_size_decrease",
            Action::FontSizeReset => "font_size_reset",
            Action::PreviousBlock => "previous_block",
            Action::NextBlock => "next_block",
            Action::CopyCommand => "copy_command",
            Action::CopyOutput => "copy_output",
            Action::ToggleFold => "toggle_fold",
            Action::ToggleFoldAll => "toggle_fold_all",
            Action::SplitRight => "split_right",
            Action::SplitDown => "split_down",
            Action::ClosePane => "close_pane",
            Action::FocusPane(d) => return format!("focus_pane_{}", direction(d)),
            Action::ResizePane(d) => return format!("resize_pane_{}", direction(d)),
            Action::NextPane => "next_pane",
            Action::PreviousPane => "previous_pane",
            Action::ToggleZoom => "toggle_zoom",
            Action::EqualisePanes => "equalise_panes",
            Action::DetachPane => "detach_pane",
            Action::NewTab => "new_tab",
            Action::NextTab => "next_tab",
            Action::PreviousTab => "previous_tab",
            Action::GoToTab(number) => return format!("go_to_tab_{number}"),
            Action::CommandPalette => "command_palette",
        };
        name.to_owned()
    }

    /// The action of an [`Action::id`]: the palette's actions, the palette itself and
    /// `go_to_tab_N` for any tab number from 1.
    pub fn from_id(id: &str) -> Option<Action> {
        if let Some(number) = id.strip_prefix("go_to_tab_") {
            // `u8::from_str` accepts a leading `+`.
            if !number.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            return number.parse().ok().filter(|&n| n > 0).map(Action::GoToTab);
        }
        Action::ALL
            .into_iter()
            .chain([Action::CommandPalette])
            .find(|action| action.id() == id)
    }

    /// Label of the action in the user's language.
    pub fn label(self) -> Msg {
        match self {
            Action::Copy => Msg::Copy,
            Action::Paste => Msg::Paste,
            Action::ScrollPageUp => Msg::ScrollPageUp,
            Action::ScrollPageDown => Msg::ScrollPageDown,
            Action::ScrollToTop => Msg::ScrollToTop,
            Action::ScrollToBottom => Msg::ScrollToBottom,
            Action::FontSizeIncrease => Msg::FontSizeIncrease,
            Action::FontSizeDecrease => Msg::FontSizeDecrease,
            Action::FontSizeReset => Msg::FontSizeReset,
            Action::PreviousBlock => Msg::PreviousBlock,
            Action::NextBlock => Msg::NextBlock,
            Action::CopyCommand => Msg::CopyCommand,
            Action::CopyOutput => Msg::CopyOutput,
            Action::ToggleFold => Msg::ToggleFold,
            Action::ToggleFoldAll => Msg::ToggleFoldAll,
            Action::SplitRight => Msg::SplitRight,
            Action::SplitDown => Msg::SplitDown,
            Action::ClosePane => Msg::ClosePane,
            Action::FocusPane(d) => Msg::FocusPane(d),
            Action::ResizePane(d) => Msg::ResizePane(d),
            Action::NextPane => Msg::NextPane,
            Action::PreviousPane => Msg::PreviousPane,
            Action::ToggleZoom => Msg::ToggleZoom,
            Action::EqualisePanes => Msg::EqualisePanes,
            Action::DetachPane => Msg::DetachPane,
            Action::NewTab => Msg::NewTab,
            Action::NextTab => Msg::NextTab,
            Action::PreviousTab => Msg::PreviousTab,
            Action::GoToTab(number) => Msg::GoToTab(usize::from(number)),
            Action::CommandPalette => Msg::CommandPalette,
        }
    }
}

/// xterm modifier parameter: 1 + shift(1) + alt(2) + ctrl(4).
fn modifier_param(mods: ModifiersState) -> u8 {
    1 + mods.shift_key() as u8 + 2 * mods.alt_key() as u8 + 4 * mods.control_key() as u8
}

fn has_mods(mods: ModifiersState) -> bool {
    mods.shift_key() || mods.alt_key() || mods.control_key()
}

/// Sequence for cursor-style keys (arrows, Home, End): `CSI 1;m X` or `SS3 X` / `CSI X`.
fn cursor_key(letter: char, mods: ModifiersState, mode: TermMode) -> Vec<u8> {
    if has_mods(mods) {
        format!("\x1b[1;{}{letter}", modifier_param(mods)).into_bytes()
    } else if mode.contains(TermMode::APP_CURSOR) {
        format!("\x1bO{letter}").into_bytes()
    } else {
        format!("\x1b[{letter}").into_bytes()
    }
}

/// Sequence for tilde-style keys (PageUp, Delete, F5...): `CSI n ~` or `CSI n;m ~`.
fn tilde_key(code: u8, mods: ModifiersState) -> Vec<u8> {
    if has_mods(mods) {
        format!("\x1b[{code};{}~", modifier_param(mods)).into_bytes()
    } else {
        format!("\x1b[{code}~").into_bytes()
    }
}

/// Sequence for F1-F4: `SS3 X` or `CSI 1;m X`.
fn ss3_key(letter: char, mods: ModifiersState) -> Vec<u8> {
    if has_mods(mods) {
        format!("\x1b[1;{}{letter}", modifier_param(mods)).into_bytes()
    } else {
        format!("\x1bO{letter}").into_bytes()
    }
}

fn with_alt(mut bytes: Vec<u8>, mods: ModifiersState) -> Vec<u8> {
    if mods.alt_key() {
        bytes.insert(0, 0x1b);
    }
    bytes
}

/// Control code produced by Ctrl+<char>, following xterm conventions.
fn control_code(c: char) -> Option<u8> {
    match c.to_ascii_lowercase() {
        c @ 'a'..='z' => Some(c as u8 - b'a' + 1),
        '@' | ' ' | '2' => Some(0x00),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '7' | '/' => Some(0x1f),
        '8' | '?' => Some(0x7f),
        _ => None,
    }
}

/// Converts a key press into the bytes to write to the PTY.
///
/// `text` is the text produced by the key according to the keyboard layout.
pub fn key_to_bytes(
    key: &Key,
    text: Option<&str>,
    mods: ModifiersState,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if let Key::Named(named) = key {
        let bytes = match named {
            NamedKey::Enter => with_alt(b"\r".to_vec(), mods),
            NamedKey::Tab if mods.shift_key() => b"\x1b[Z".to_vec(),
            NamedKey::Tab => with_alt(b"\t".to_vec(), mods),
            NamedKey::Backspace if mods.control_key() => with_alt(vec![0x08], mods),
            NamedKey::Backspace => with_alt(vec![0x7f], mods),
            NamedKey::Escape => with_alt(vec![0x1b], mods),
            NamedKey::Space if mods.control_key() => with_alt(vec![0x00], mods),
            NamedKey::Space => with_alt(b" ".to_vec(), mods),
            NamedKey::ArrowUp => cursor_key('A', mods, mode),
            NamedKey::ArrowDown => cursor_key('B', mods, mode),
            NamedKey::ArrowRight => cursor_key('C', mods, mode),
            NamedKey::ArrowLeft => cursor_key('D', mods, mode),
            NamedKey::Home => cursor_key('H', mods, mode),
            NamedKey::End => cursor_key('F', mods, mode),
            NamedKey::Insert => tilde_key(2, mods),
            NamedKey::Delete => tilde_key(3, mods),
            NamedKey::PageUp => tilde_key(5, mods),
            NamedKey::PageDown => tilde_key(6, mods),
            NamedKey::F1 => ss3_key('P', mods),
            NamedKey::F2 => ss3_key('Q', mods),
            NamedKey::F3 => ss3_key('R', mods),
            NamedKey::F4 => ss3_key('S', mods),
            NamedKey::F5 => tilde_key(15, mods),
            NamedKey::F6 => tilde_key(17, mods),
            NamedKey::F7 => tilde_key(18, mods),
            NamedKey::F8 => tilde_key(19, mods),
            NamedKey::F9 => tilde_key(20, mods),
            NamedKey::F10 => tilde_key(21, mods),
            NamedKey::F11 => tilde_key(23, mods),
            NamedKey::F12 => tilde_key(24, mods),
            _ => return None,
        };
        return Some(bytes);
    }

    if let Key::Character(c) = key
        && mods.control_key()
        && let mut chars = c.chars()
        && let (Some(ch), None) = (chars.next(), chars.next())
        && let Some(code) = control_code(ch)
    {
        return Some(with_alt(vec![code], mods));
    }

    let text = text.filter(|t| !t.is_empty())?;
    // Drop control characters the platform may have generated: handled above.
    if text.chars().all(|c| c.is_control()) {
        return None;
    }
    Some(with_alt(text.as_bytes().to_vec(), mods))
}

/// Wraps pasted text according to the terminal mode.
pub fn paste_bytes(text: &str, mode: TermMode) -> Vec<u8> {
    if mode.contains(TermMode::BRACKETED_PASTE) {
        // Strip escape characters to prevent breaking out of the bracket.
        let sanitized: String = text.chars().filter(|&c| c != '\x1b').collect();
        format!("\x1b[200~{sanitized}\x1b[201~").into_bytes()
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(s: &str) -> Key {
        Key::Character(s.into())
    }

    fn named(k: NamedKey) -> Key {
        Key::Named(k)
    }

    const NONE: ModifiersState = ModifiersState::empty();

    #[test]
    fn action_ids_are_unique_snake_case() {
        let mut ids: Vec<String> = Action::ALL
            .into_iter()
            .chain([Action::CommandPalette])
            .chain((1..=9).map(Action::GoToTab))
            .map(Action::id)
            .collect();
        assert!(
            ids.iter().all(|id| id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')),
            "{ids:?}"
        );
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count);
        assert_eq!(Action::FocusPane(Direction::Left).id(), "focus_pane_left");
        assert_eq!(Action::GoToTab(3).id(), "go_to_tab_3");
        assert_eq!(Action::FontSizeReset.id(), "font_size_reset");
    }

    #[test]
    fn from_id_is_the_inverse_of_id() {
        let actions = Action::ALL
            .into_iter()
            .chain([Action::CommandPalette])
            .chain((1..=12).map(Action::GoToTab));
        for action in actions {
            assert_eq!(Action::from_id(&action.id()), Some(action), "{action:?}");
        }
        for id in [
            "quit",
            "",
            "Copy",
            "go_to_tab_0",
            "go_to_tab_+1",
            "go_to_tab_",
            "go_to_tab_256",
        ] {
            assert_eq!(Action::from_id(id), None, "{id}");
        }
    }

    #[test]
    fn the_palette_lists_every_action_but_itself_and_the_tab_numbers() {
        assert!(!Action::ALL.contains(&Action::CommandPalette));
        assert!(!Action::ALL.iter().any(|a| matches!(a, Action::GoToTab(_))));
        for (index, action) in Action::ALL.iter().enumerate() {
            assert!(!Action::ALL[..index].contains(action), "{action:?} twice");
        }
        // Each label is a distinct text.
        let mut labels: Vec<String> = Action::ALL
            .into_iter()
            .map(|a| crate::i18n::text(crate::i18n::Lang::En, &a.label()).into_owned())
            .collect();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), Action::ALL.len());
    }

    #[test]
    fn plain_text_is_forwarded() {
        let b = key_to_bytes(&ch("é"), Some("é"), NONE, TermMode::empty());
        assert_eq!(b, Some("é".as_bytes().to_vec()));
    }

    #[test]
    fn ctrl_letters_produce_control_codes() {
        let b = key_to_bytes(&ch("c"), None, ModifiersState::CONTROL, TermMode::empty());
        assert_eq!(b, Some(vec![0x03]));
        let b = key_to_bytes(&ch("["), None, ModifiersState::CONTROL, TermMode::empty());
        assert_eq!(b, Some(vec![0x1b]));
    }

    #[test]
    fn alt_prefixes_escape() {
        let b = key_to_bytes(&ch("b"), Some("b"), ModifiersState::ALT, TermMode::empty());
        assert_eq!(b, Some(b"\x1bb".to_vec()));
    }

    #[test]
    fn arrows_follow_cursor_mode() {
        let up = named(NamedKey::ArrowUp);
        assert_eq!(
            key_to_bytes(&up, None, NONE, TermMode::empty()),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_to_bytes(&up, None, NONE, TermMode::APP_CURSOR),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            key_to_bytes(&up, None, ModifiersState::CONTROL, TermMode::empty()),
            Some(b"\x1b[1;5A".to_vec())
        );
    }

    #[test]
    fn special_keys() {
        let m = TermMode::empty();
        assert_eq!(
            key_to_bytes(&named(NamedKey::Enter), None, NONE, m),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            key_to_bytes(&named(NamedKey::Backspace), None, NONE, m),
            Some(vec![0x7f])
        );
        assert_eq!(
            key_to_bytes(&named(NamedKey::Tab), None, ModifiersState::SHIFT, m),
            Some(b"\x1b[Z".to_vec())
        );
        assert_eq!(
            key_to_bytes(&named(NamedKey::Delete), None, NONE, m),
            Some(b"\x1b[3~".to_vec())
        );
        assert_eq!(
            key_to_bytes(&named(NamedKey::F1), None, NONE, m),
            Some(b"\x1bOP".to_vec())
        );
        assert_eq!(
            key_to_bytes(&named(NamedKey::F5), None, ModifiersState::SHIFT, m),
            Some(b"\x1b[15;2~".to_vec())
        );
    }

    #[test]
    fn unknown_named_key_is_ignored() {
        let b = key_to_bytes(&named(NamedKey::Shift), None, NONE, TermMode::empty());
        assert_eq!(b, None);
    }

    #[test]
    fn ctrl_shift_letters_produce_control_codes() {
        let cs = ModifiersState::CONTROL | ModifiersState::SHIFT;
        let b = key_to_bytes(&ch("X"), None, cs, TermMode::empty());
        assert_eq!(b, Some(vec![0x18]));
    }

    #[test]
    fn paste_is_bracketed_when_requested() {
        assert_eq!(
            paste_bytes("a\x1bb", TermMode::BRACKETED_PASTE),
            b"\x1b[200~ab\x1b[201~".to_vec()
        );
        assert_eq!(paste_bytes("a\nb", TermMode::empty()), b"a\rb".to_vec());
    }
}
