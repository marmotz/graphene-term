//! Translation of keyboard input into PTY bytes and application actions.

use alacritty_terminal::term::TermMode;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use crate::i18n::Direction;

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
    #[allow(dead_code, reason = "reached through the command palette")]
    EqualisePanes,
    /// Moves the active pane into a new tab; no default key.
    #[allow(dead_code, reason = "reached through the command palette")]
    DetachPane,
    /// Opens a tab after the last one.
    NewTab,
    NextTab,
    PreviousTab,
    /// Shows the tab of this number, counted from 1.
    GoToTab(u8),
}

/// Letter a shortcut is matched on: the character the key types when it is an ASCII
/// letter, else the letter of the physical key, so shortcuts still work on layouts
/// without Latin letters.
fn shortcut_letter(key: &Key, physical: PhysicalKey) -> Option<char> {
    if let Key::Character(c) = key {
        let mut chars = c.chars();
        if let (Some(ch), None) = (chars.next(), chars.next())
            && ch.is_ascii_alphabetic()
        {
            return Some(ch.to_ascii_lowercase());
        }
    }
    let PhysicalKey::Code(code) = physical else {
        return None;
    };
    let letter = match code {
        KeyCode::KeyA => 'a',
        KeyCode::KeyB => 'b',
        KeyCode::KeyC => 'c',
        KeyCode::KeyD => 'd',
        KeyCode::KeyE => 'e',
        KeyCode::KeyF => 'f',
        KeyCode::KeyG => 'g',
        KeyCode::KeyH => 'h',
        KeyCode::KeyI => 'i',
        KeyCode::KeyJ => 'j',
        KeyCode::KeyK => 'k',
        KeyCode::KeyL => 'l',
        KeyCode::KeyM => 'm',
        KeyCode::KeyN => 'n',
        KeyCode::KeyO => 'o',
        KeyCode::KeyP => 'p',
        KeyCode::KeyQ => 'q',
        KeyCode::KeyR => 'r',
        KeyCode::KeyS => 's',
        KeyCode::KeyT => 't',
        KeyCode::KeyU => 'u',
        KeyCode::KeyV => 'v',
        KeyCode::KeyW => 'w',
        KeyCode::KeyX => 'x',
        KeyCode::KeyY => 'y',
        KeyCode::KeyZ => 'z',
        _ => return None,
    };
    Some(letter)
}

/// Tab number of a digit key `1` to `9`, matched on the physical key so it works on
/// layouts where the digits need Shift (AZERTY).
fn tab_digit(physical: PhysicalKey) -> Option<u8> {
    let PhysicalKey::Code(code) = physical else {
        return None;
    };
    let digit = match code {
        KeyCode::Digit1 => 1,
        KeyCode::Digit2 => 2,
        KeyCode::Digit3 => 3,
        KeyCode::Digit4 => 4,
        KeyCode::Digit5 => 5,
        KeyCode::Digit6 => 6,
        KeyCode::Digit7 => 7,
        KeyCode::Digit8 => 8,
        KeyCode::Digit9 => 9,
        _ => return None,
    };
    Some(digit)
}

/// Direction of an arrow key.
fn arrow_direction(key: &Key) -> Option<Direction> {
    match key {
        Key::Named(NamedKey::ArrowLeft) => Some(Direction::Left),
        Key::Named(NamedKey::ArrowRight) => Some(Direction::Right),
        Key::Named(NamedKey::ArrowUp) => Some(Direction::Up),
        Key::Named(NamedKey::ArrowDown) => Some(Direction::Down),
        _ => None,
    }
}

/// Returns the application action bound to a key combination, if any. Letters are
/// matched by the character they type (see [`shortcut_letter`] for the `physical`
/// fallback), named keys by their logical value.
pub fn action_for(key: &Key, physical: PhysicalKey, mods: ModifiersState) -> Option<Action> {
    let ctrl_shift = mods.control_key() && mods.shift_key() && !mods.alt_key();
    let ctrl_shift_alt = mods.control_key() && mods.shift_key() && mods.alt_key();
    if mods == ModifiersState::ALT
        && let Some(digit) = tab_digit(physical)
    {
        return Some(Action::GoToTab(digit));
    }
    if ctrl_shift_alt && let Some(direction) = arrow_direction(key) {
        return Some(Action::ResizePane(direction));
    }
    if ctrl_shift && let Some(direction) = arrow_direction(key) {
        return Some(Action::FocusPane(direction));
    }
    if ctrl_shift_alt && let Some(letter) = shortcut_letter(key, physical) {
        return match letter {
            'c' => Some(Action::CopyCommand),
            'o' => Some(Action::CopyOutput),
            'f' => Some(Action::ToggleFoldAll),
            _ => None,
        };
    }
    if ctrl_shift && let Some(letter) = shortcut_letter(key, physical) {
        return match letter {
            'c' => Some(Action::Copy),
            'v' => Some(Action::Paste),
            'f' => Some(Action::ToggleFold),
            'e' => Some(Action::SplitRight),
            'o' => Some(Action::SplitDown),
            'w' => Some(Action::ClosePane),
            'n' => Some(Action::NextPane),
            'b' => Some(Action::PreviousPane),
            'z' => Some(Action::ToggleZoom),
            't' => Some(Action::NewTab),
            _ => None,
        };
    }
    // Other `Ctrl+Shift` characters stay with the shell.
    if mods.control_key() && mods.shift_key() && matches!(key, Key::Character(_)) {
        return None;
    }
    match key {
        Key::Character(c) if mods.control_key() && !mods.alt_key() => match c.as_str() {
            "=" | "+" => Some(Action::FontSizeIncrease),
            "-" | "_" => Some(Action::FontSizeDecrease),
            "0" if !mods.shift_key() => Some(Action::FontSizeReset),
            _ => None,
        },
        Key::Named(NamedKey::Tab) if mods == ModifiersState::CONTROL => Some(Action::NextTab),
        Key::Named(NamedKey::Tab) if mods == ModifiersState::CONTROL | ModifiersState::SHIFT => {
            Some(Action::PreviousTab)
        }
        Key::Named(NamedKey::PageDown) if mods == ModifiersState::CONTROL => Some(Action::NextTab),
        Key::Named(NamedKey::PageUp) if mods == ModifiersState::CONTROL => {
            Some(Action::PreviousTab)
        }
        Key::Named(NamedKey::Paste) => Some(Action::Paste),
        Key::Named(NamedKey::Copy) => Some(Action::Copy),
        Key::Named(NamedKey::PageUp) if mods == ModifiersState::SHIFT => Some(Action::ScrollPageUp),
        Key::Named(NamedKey::PageDown) if mods == ModifiersState::SHIFT => {
            Some(Action::ScrollPageDown)
        }
        Key::Named(NamedKey::PageUp) if mods == ModifiersState::CONTROL | ModifiersState::SHIFT => {
            Some(Action::PreviousBlock)
        }
        Key::Named(NamedKey::PageDown)
            if mods == ModifiersState::CONTROL | ModifiersState::SHIFT =>
        {
            Some(Action::NextBlock)
        }
        Key::Named(NamedKey::Home) if mods == ModifiersState::SHIFT => Some(Action::ScrollToTop),
        Key::Named(NamedKey::End) if mods == ModifiersState::SHIFT => Some(Action::ScrollToBottom),
        Key::Named(NamedKey::Insert) if mods == ModifiersState::SHIFT => Some(Action::Paste),
        _ => None,
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

    /// No physical key information, as for synthetic events.
    const UNIDENTIFIED: PhysicalKey =
        PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified);

    fn action_for(key: &Key, mods: ModifiersState) -> Option<Action> {
        super::action_for(key, UNIDENTIFIED, mods)
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
    fn app_actions() {
        let cs = ModifiersState::CONTROL | ModifiersState::SHIFT;
        assert_eq!(action_for(&ch("C"), cs), Some(Action::Copy));
        assert_eq!(action_for(&ch("V"), cs), Some(Action::Paste));
        assert_eq!(action_for(&ch("c"), ModifiersState::CONTROL), None);
        assert_eq!(
            action_for(&named(NamedKey::PageUp), ModifiersState::SHIFT),
            Some(Action::ScrollPageUp)
        );
        assert_eq!(action_for(&named(NamedKey::PageUp), NONE), None);
        assert_eq!(
            action_for(&ch("="), ModifiersState::CONTROL),
            Some(Action::FontSizeIncrease)
        );
    }

    #[test]
    fn block_actions() {
        let cs = ModifiersState::CONTROL | ModifiersState::SHIFT;
        let csa = cs | ModifiersState::ALT;
        assert_eq!(
            action_for(&named(NamedKey::PageUp), cs),
            Some(Action::PreviousBlock)
        );
        assert_eq!(
            action_for(&named(NamedKey::PageDown), cs),
            Some(Action::NextBlock)
        );
        assert_eq!(action_for(&ch("C"), csa), Some(Action::CopyCommand));
        assert_eq!(action_for(&ch("c"), csa), Some(Action::CopyCommand));
        assert_eq!(action_for(&ch("O"), csa), Some(Action::CopyOutput));
        assert_eq!(action_for(&ch("F"), cs), Some(Action::ToggleFold));
        assert_eq!(action_for(&ch("f"), cs), Some(Action::ToggleFold));
        assert_eq!(action_for(&ch("F"), csa), Some(Action::ToggleFoldAll));
        // Ctrl+F alone stays with the shell.
        assert_eq!(action_for(&ch("f"), ModifiersState::CONTROL), None);
        // Plain Ctrl+Shift+C/V unchanged.
        assert_eq!(action_for(&ch("C"), cs), Some(Action::Copy));
        assert_eq!(action_for(&ch("V"), cs), Some(Action::Paste));
        assert_eq!(action_for(&ch("V"), csa), None);
        // Alt+PageUp stays with the shell.
        assert_eq!(
            action_for(&named(NamedKey::PageUp), ModifiersState::ALT),
            None
        );
    }

    #[test]
    fn pane_actions() {
        let cs = ModifiersState::CONTROL | ModifiersState::SHIFT;
        let csa = cs | ModifiersState::ALT;
        for (letter, action) in [
            ("E", Action::SplitRight),
            ("O", Action::SplitDown),
            ("W", Action::ClosePane),
            ("N", Action::NextPane),
            ("B", Action::PreviousPane),
            ("Z", Action::ToggleZoom),
        ] {
            assert_eq!(action_for(&ch(letter), cs), Some(action), "{letter}");
            assert_eq!(
                action_for(&ch(&letter.to_lowercase()), cs),
                Some(action),
                "{letter}"
            );
            // Without Shift, the key stays with the shell.
            assert_eq!(action_for(&ch(letter), ModifiersState::CONTROL), None);
        }
        for (key, direction) in [
            (NamedKey::ArrowLeft, Direction::Left),
            (NamedKey::ArrowRight, Direction::Right),
            (NamedKey::ArrowUp, Direction::Up),
            (NamedKey::ArrowDown, Direction::Down),
        ] {
            assert_eq!(
                action_for(&named(key), cs),
                Some(Action::FocusPane(direction))
            );
            assert_eq!(
                action_for(&named(key), csa),
                Some(Action::ResizePane(direction))
            );
            assert_eq!(action_for(&named(key), ModifiersState::CONTROL), None);
        }
        // Ctrl+Shift+Alt+O copies the output, Ctrl+Shift+O splits down.
        assert_eq!(action_for(&ch("O"), csa), Some(Action::CopyOutput));
    }

    #[test]
    fn tab_actions() {
        let ctrl = ModifiersState::CONTROL;
        let cs = ctrl | ModifiersState::SHIFT;
        assert_eq!(action_for(&ch("T"), cs), Some(Action::NewTab));
        assert_eq!(action_for(&ch("t"), cs), Some(Action::NewTab));
        assert_eq!(
            action_for(&named(NamedKey::Tab), ctrl),
            Some(Action::NextTab)
        );
        assert_eq!(
            action_for(&named(NamedKey::Tab), cs),
            Some(Action::PreviousTab)
        );
        assert_eq!(
            action_for(&named(NamedKey::PageDown), ctrl),
            Some(Action::NextTab)
        );
        assert_eq!(
            action_for(&named(NamedKey::PageUp), ctrl),
            Some(Action::PreviousTab)
        );
        // Ctrl+Shift+PageUp/PageDown still move between blocks; Tab alone is the shell's.
        assert_eq!(
            action_for(&named(NamedKey::PageUp), cs),
            Some(Action::PreviousBlock)
        );
        assert_eq!(action_for(&named(NamedKey::Tab), NONE), None);
    }

    #[test]
    fn alt_digits_go_to_a_tab_by_their_physical_key() {
        let alt = ModifiersState::ALT;
        let codes = [
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
        ];
        for (number, code) in (1..=9).zip(codes) {
            assert_eq!(
                super::action_for(&ch(&number.to_string()), PhysicalKey::Code(code), alt),
                Some(Action::GoToTab(number))
            );
        }
        // AZERTY: the Digit1 key types `&` without Shift.
        assert_eq!(
            super::action_for(&ch("&"), PhysicalKey::Code(KeyCode::Digit1), alt),
            Some(Action::GoToTab(1))
        );
        // Digit0, other modifiers and the logical digit alone are not tab shortcuts.
        assert_eq!(
            super::action_for(&ch("0"), PhysicalKey::Code(KeyCode::Digit0), alt),
            None
        );
        assert_eq!(
            super::action_for(
                &ch("1"),
                PhysicalKey::Code(KeyCode::Digit1),
                alt | ModifiersState::CONTROL
            ),
            None
        );
        assert_eq!(action_for(&ch("1"), alt), None);
    }

    #[test]
    fn letters_fall_back_on_the_physical_key() {
        let cs = ModifiersState::CONTROL | ModifiersState::SHIFT;
        let cyrillic = ch("У");
        assert_eq!(
            super::action_for(&cyrillic, PhysicalKey::Code(KeyCode::KeyE), cs),
            Some(Action::SplitRight)
        );
        // The typed letter wins over its physical place (AZERTY: Z on the W key).
        assert_eq!(
            super::action_for(&ch("Z"), PhysicalKey::Code(KeyCode::KeyW), cs),
            Some(Action::ToggleZoom)
        );
        assert_eq!(action_for(&cyrillic, cs), None);
    }

    #[test]
    fn unmapped_ctrl_shift_letters_produce_control_codes() {
        let cs = ModifiersState::CONTROL | ModifiersState::SHIFT;
        for letter in ["A", "D", "G", "X"] {
            assert_eq!(action_for(&ch(letter), cs), None, "{letter}");
        }
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
