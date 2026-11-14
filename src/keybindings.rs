//! Keyboard shortcuts of the application actions: the default table, the
//! `[keybindings]` section of the configuration on top of it, the lookup of a key event
//! and the label of a shortcut.
//!
//! A shortcut is written as modifiers (`Ctrl`, `Shift`, `Alt`, `Super`) joined with `+`,
//! then a key: a winit `NamedKey` name (`PageUp`, `F5`, `Insert`...) or one character.
//! Letters are matched case-insensitively. For other characters `Shift` is ignored,
//! because the layout already used it to produce the character (`Ctrl++`). Digits bound
//! with `Alt` are matched on the physical digit row, so `Alt+1` works on AZERTY.

use std::collections::{BTreeMap, HashMap};

use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use crate::i18n::Direction;
use crate::input::Action;

/// Action name that removes a shortcut, so its key goes to the shell.
pub const NONE_ACTION: &str = "none";

/// The key part of a shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum KeyId {
    /// A character, lowercased when it is a letter.
    Char(char),
    Named(NamedKey),
    /// A key of the digit row, matched on its physical key (with `Alt`).
    Digit(u8),
}

/// A key and the modifiers held with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    key: KeyId,
    mods: ModifiersState,
}

/// Names of the named keys a shortcut may use, compared case-insensitively. The first
/// name of a key is the one its label shows.
const NAMED_KEYS: &[(&str, NamedKey)] = &[
    ("Left", NamedKey::ArrowLeft),
    ("Right", NamedKey::ArrowRight),
    ("Up", NamedKey::ArrowUp),
    ("Down", NamedKey::ArrowDown),
    ("ArrowLeft", NamedKey::ArrowLeft),
    ("ArrowRight", NamedKey::ArrowRight),
    ("ArrowUp", NamedKey::ArrowUp),
    ("ArrowDown", NamedKey::ArrowDown),
    ("PageUp", NamedKey::PageUp),
    ("PageDown", NamedKey::PageDown),
    ("Home", NamedKey::Home),
    ("End", NamedKey::End),
    ("Insert", NamedKey::Insert),
    ("Delete", NamedKey::Delete),
    ("Backspace", NamedKey::Backspace),
    ("Tab", NamedKey::Tab),
    ("Enter", NamedKey::Enter),
    ("Escape", NamedKey::Escape),
    ("Space", NamedKey::Space),
    ("Copy", NamedKey::Copy),
    ("Paste", NamedKey::Paste),
    ("Cut", NamedKey::Cut),
    ("ContextMenu", NamedKey::ContextMenu),
    ("PrintScreen", NamedKey::PrintScreen),
    ("Pause", NamedKey::Pause),
    ("ScrollLock", NamedKey::ScrollLock),
    ("F1", NamedKey::F1),
    ("F2", NamedKey::F2),
    ("F3", NamedKey::F3),
    ("F4", NamedKey::F4),
    ("F5", NamedKey::F5),
    ("F6", NamedKey::F6),
    ("F7", NamedKey::F7),
    ("F8", NamedKey::F8),
    ("F9", NamedKey::F9),
    ("F10", NamedKey::F10),
    ("F11", NamedKey::F11),
    ("F12", NamedKey::F12),
    ("F13", NamedKey::F13),
    ("F14", NamedKey::F14),
    ("F15", NamedKey::F15),
    ("F16", NamedKey::F16),
    ("F17", NamedKey::F17),
    ("F18", NamedKey::F18),
    ("F19", NamedKey::F19),
    ("F20", NamedKey::F20),
    ("F21", NamedKey::F21),
    ("F22", NamedKey::F22),
    ("F23", NamedKey::F23),
    ("F24", NamedKey::F24),
];

/// The built-in shortcuts. For an action with several, the first is the one the
/// command palette shows.
const DEFAULTS: &[(&str, Action)] = &[
    ("Ctrl+Shift+C", Action::Copy),
    ("Copy", Action::Copy),
    ("Ctrl+Shift+V", Action::Paste),
    ("Shift+Insert", Action::Paste),
    ("Paste", Action::Paste),
    ("Shift+PageUp", Action::ScrollPageUp),
    ("Shift+PageDown", Action::ScrollPageDown),
    ("Shift+Home", Action::ScrollToTop),
    ("Shift+End", Action::ScrollToBottom),
    ("Ctrl+=", Action::FontSizeIncrease),
    ("Ctrl++", Action::FontSizeIncrease),
    ("Ctrl+-", Action::FontSizeDecrease),
    ("Ctrl+_", Action::FontSizeDecrease),
    ("Ctrl+0", Action::FontSizeReset),
    ("Ctrl+Shift+PageUp", Action::PreviousBlock),
    ("Ctrl+Shift+PageDown", Action::NextBlock),
    ("Ctrl+Shift+Alt+C", Action::CopyCommand),
    ("Ctrl+Shift+Alt+O", Action::CopyOutput),
    ("Ctrl+Shift+F", Action::ToggleFold),
    ("Ctrl+Shift+Alt+F", Action::ToggleFoldAll),
    ("Ctrl+Shift+E", Action::SplitRight),
    ("Ctrl+Shift+O", Action::SplitDown),
    ("Ctrl+Shift+W", Action::ClosePane),
    ("Ctrl+Shift+Left", Action::FocusPane(Direction::Left)),
    ("Ctrl+Shift+Right", Action::FocusPane(Direction::Right)),
    ("Ctrl+Shift+Up", Action::FocusPane(Direction::Up)),
    ("Ctrl+Shift+Down", Action::FocusPane(Direction::Down)),
    ("Ctrl+Shift+Alt+Left", Action::ResizePane(Direction::Left)),
    ("Ctrl+Shift+Alt+Right", Action::ResizePane(Direction::Right)),
    ("Ctrl+Shift+Alt+Up", Action::ResizePane(Direction::Up)),
    ("Ctrl+Shift+Alt+Down", Action::ResizePane(Direction::Down)),
    ("Ctrl+Shift+N", Action::NextPane),
    ("Ctrl+Shift+B", Action::PreviousPane),
    ("Ctrl+Shift+Z", Action::ToggleZoom),
    ("Ctrl+Shift+T", Action::NewTab),
    ("Ctrl+Tab", Action::NextTab),
    ("Ctrl+PageDown", Action::NextTab),
    ("Ctrl+Shift+Tab", Action::PreviousTab),
    ("Ctrl+PageUp", Action::PreviousTab),
    ("Alt+1", Action::GoToTab(1)),
    ("Alt+2", Action::GoToTab(2)),
    ("Alt+3", Action::GoToTab(3)),
    ("Alt+4", Action::GoToTab(4)),
    ("Alt+5", Action::GoToTab(5)),
    ("Alt+6", Action::GoToTab(6)),
    ("Alt+7", Action::GoToTab(7)),
    ("Alt+8", Action::GoToTab(8)),
    ("Alt+9", Action::GoToTab(9)),
    ("Ctrl+Shift+P", Action::CommandPalette),
];

/// Built-in key/action pairs, in the order shown in the default configuration.
pub(crate) fn defaults() -> impl Iterator<Item = (&'static str, Action)> + Clone {
    DEFAULTS.iter().copied()
}

impl Shortcut {
    /// Parses a shortcut written as in the configuration file (`Ctrl+Shift+C`).
    pub fn parse(text: &str) -> Result<Shortcut, String> {
        // `+` is both the separator and a key: `Ctrl++`, `+`.
        let (mods_text, key_text) = if text == "+" {
            ("", "+")
        } else if let Some(mods) = text.strip_suffix("++") {
            (mods, "+")
        } else {
            text.rsplit_once('+').unwrap_or(("", text))
        };
        let mut mods = ModifiersState::empty();
        if !mods_text.is_empty() {
            for part in mods_text.split('+') {
                mods |= match part.to_ascii_lowercase().as_str() {
                    "ctrl" | "control" => ModifiersState::CONTROL,
                    "shift" => ModifiersState::SHIFT,
                    "alt" => ModifiersState::ALT,
                    "super" => ModifiersState::SUPER,
                    _ => return Err(format!("unknown modifier \"{part}\" in \"{text}\"")),
                };
            }
        }
        let mut chars = key_text.chars();
        let key = match (chars.next(), chars.next()) {
            (None, _) => return Err(format!("missing key in \"{text}\"")),
            (Some(' '), None) => KeyId::Named(NamedKey::Space),
            (Some(c), None) => return Ok(Shortcut::character(c, mods)),
            _ => NAMED_KEYS
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key_text))
                .map(|&(_, key)| KeyId::Named(key))
                .ok_or_else(|| format!("unknown key \"{key_text}\" in \"{text}\""))?,
        };
        Ok(Shortcut { key, mods })
    }

    /// The shortcut of a character key: a letter lowercased, a digit with `Alt` on its
    /// physical key, any other character without `Shift`.
    fn character(c: char, mods: ModifiersState) -> Shortcut {
        if c.is_alphabetic() {
            let lower = c.to_lowercase().next().unwrap_or(c);
            return Shortcut {
                key: KeyId::Char(lower),
                mods,
            };
        }
        if mods.alt_key()
            && let Some(digit) = c.to_digit(10)
        {
            return Shortcut {
                key: KeyId::Digit(digit as u8),
                mods,
            };
        }
        Shortcut {
            key: KeyId::Char(c),
            mods: mods - ModifiersState::SHIFT,
        }
    }

    /// The shortcut as the command palette shows it (`Ctrl+Shift+C`).
    pub fn label(self) -> String {
        let mut label = String::new();
        for (flag, name) in [
            (ModifiersState::CONTROL, "Ctrl"),
            (ModifiersState::SHIFT, "Shift"),
            (ModifiersState::ALT, "Alt"),
            (ModifiersState::SUPER, "Super"),
        ] {
            if self.mods.contains(flag) {
                label.push_str(name);
                label.push('+');
            }
        }
        match self.key {
            KeyId::Char(c) => label.extend(c.to_uppercase()),
            KeyId::Digit(digit) => label.push_str(&digit.to_string()),
            KeyId::Named(key) => {
                let name = NAMED_KEYS
                    .iter()
                    .find(|&&(_, named)| named == key)
                    .map_or("?", |&(name, _)| name);
                label.push_str(name);
            }
        }
        label
    }
}

/// Parses one `"keys" = "action"` entry of the configuration; `None` for `"none"`.
pub fn parse_binding(keys: &str, action: &str) -> Result<(Shortcut, Option<Action>), String> {
    let shortcut = Shortcut::parse(keys)?;
    if action == NONE_ACTION {
        return Ok((shortcut, None));
    }
    let action = Action::from_id(action).ok_or_else(|| format!("unknown action \"{action}\""))?;
    Ok((shortcut, Some(action)))
}

/// The shortcut table: the defaults, then the configuration on top.
#[derive(Debug, Clone)]
pub struct Keybindings {
    actions: HashMap<Shortcut, Action>,
    /// Every shortcut ever bound, the configured ones first: the label of an action is
    /// the first of them still bound to it.
    order: Vec<Shortcut>,
}

impl Default for Keybindings {
    fn default() -> Self {
        Self::new(&BTreeMap::new())
    }
}

impl Keybindings {
    /// The defaults with the `[keybindings]` entries of the configuration applied. An
    /// invalid entry (already reported by the configuration) is skipped.
    pub fn new(config: &BTreeMap<String, String>) -> Self {
        let mut actions = HashMap::new();
        let mut order = Vec::new();
        for &(keys, action) in DEFAULTS {
            let shortcut = Shortcut::parse(keys).expect("valid default shortcut");
            actions.insert(shortcut, action);
            order.push(shortcut);
        }
        let mut configured = Vec::new();
        for (keys, action) in config {
            match parse_binding(keys, action) {
                Ok((shortcut, Some(action))) => {
                    actions.insert(shortcut, action);
                    configured.push(shortcut);
                }
                Ok((shortcut, None)) => {
                    actions.remove(&shortcut);
                }
                Err(message) => log::warn!("keybinding \"{keys}\" skipped: {message}"),
            }
        }
        configured.append(&mut order);
        Self {
            actions,
            order: configured,
        }
    }

    /// The action bound to a key event, if any. Letters are matched by the character
    /// they type, else by their physical key, so shortcuts still work on layouts
    /// without Latin letters; named keys by their logical value.
    pub fn action(&self, key: &Key, physical: PhysicalKey, mods: ModifiersState) -> Option<Action> {
        let get = |key, mods| self.actions.get(&Shortcut { key, mods }).copied();
        if mods.alt_key()
            && let Some(digit) = physical_digit(physical)
            && let Some(action) = get(KeyId::Digit(digit), mods)
        {
            return Some(action);
        }
        match key {
            Key::Named(named) => get(KeyId::Named(*named), mods),
            Key::Character(text) => {
                let mut chars = text.chars();
                let typed = match (chars.next(), chars.next()) {
                    (Some(c), None) => Some(c),
                    _ => None,
                };
                if let Some(c) = typed {
                    let shortcut = Shortcut::character(c, mods);
                    // A digit with `Alt` is only matched on the physical key, above.
                    if !matches!(shortcut.key, KeyId::Digit(_))
                        && let Some(action) = get(shortcut.key, shortcut.mods)
                    {
                        return Some(action);
                    }
                }
                if typed.is_some_and(|c| c.is_ascii_alphabetic()) {
                    return None;
                }
                physical_letter(physical).and_then(|letter| get(KeyId::Char(letter), mods))
            }
            _ => None,
        }
    }

    /// The shortcut of `action` as the command palette shows it, `None` when no key is
    /// bound to it.
    pub fn label(&self, action: Action) -> Option<String> {
        self.order
            .iter()
            .find(|shortcut| self.actions.get(shortcut) == Some(&action))
            .map(|shortcut| shortcut.label())
    }
}

/// The letter of a physical letter key.
fn physical_letter(physical: PhysicalKey) -> Option<char> {
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

/// The digit of a physical key of the digit row.
fn physical_digit(physical: PhysicalKey) -> Option<u8> {
    let PhysicalKey::Code(code) = physical else {
        return None;
    };
    let digit = match code {
        KeyCode::Digit0 => 0,
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
    const CTRL: ModifiersState = ModifiersState::CONTROL;
    const SHIFT: ModifiersState = ModifiersState::SHIFT;
    const ALT: ModifiersState = ModifiersState::ALT;

    fn cs() -> ModifiersState {
        CTRL | SHIFT
    }

    fn csa() -> ModifiersState {
        CTRL | SHIFT | ALT
    }

    /// No physical key information, as for synthetic events.
    const UNIDENTIFIED: PhysicalKey =
        PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified);

    fn digit_code(digit: u8) -> KeyCode {
        [
            KeyCode::Digit0,
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
        ][usize::from(digit)]
    }

    fn defaults() -> Keybindings {
        Keybindings::default()
    }

    fn with(entries: &[(&str, &str)]) -> Keybindings {
        Keybindings::new(
            &entries
                .iter()
                .map(|&(k, a)| (k.to_owned(), a.to_owned()))
                .collect(),
        )
    }

    fn action_for(bindings: &Keybindings, key: &Key, mods: ModifiersState) -> Option<Action> {
        bindings.action(key, UNIDENTIFIED, mods)
    }

    /// The key event a shortcut label names (`Ctrl+Shift+Left`, `Alt+3`).
    fn event(label: &str) -> (Key, PhysicalKey, ModifiersState) {
        let shortcut = Shortcut::parse(label).expect(label);
        match shortcut.key {
            KeyId::Char(c) => (ch(&c.to_string()), UNIDENTIFIED, shortcut.mods),
            KeyId::Named(key) => (named(key), UNIDENTIFIED, shortcut.mods),
            KeyId::Digit(digit) => (
                ch(&digit.to_string()),
                PhysicalKey::Code(digit_code(digit)),
                shortcut.mods,
            ),
        }
    }

    #[test]
    fn every_default_shortcut_triggers_its_action() {
        let bindings = defaults();
        for &(label, action) in DEFAULTS {
            let (key, physical, mods) = event(label);
            assert_eq!(
                bindings.action(&key, physical, mods),
                Some(action),
                "{label}"
            );
        }
    }

    #[test]
    fn labels_are_the_first_default_shortcut() {
        let bindings = defaults();
        let actions = Action::ALL
            .into_iter()
            .chain([Action::CommandPalette])
            .chain((1..=9).map(Action::GoToTab));
        for action in actions {
            let Some(label) = bindings.label(action) else {
                assert!(
                    matches!(action, Action::EqualisePanes | Action::DetachPane),
                    "{action:?} has no label"
                );
                continue;
            };
            let first = DEFAULTS
                .iter()
                .find(|&&(_, a)| a == action)
                .map(|(l, _)| *l);
            assert_eq!(Some(label.as_str()), first, "{action:?}");
        }
        assert_eq!(bindings.label(Action::GoToTab(10)), None);
        assert_eq!(
            bindings.label(Action::FontSizeIncrease).as_deref(),
            Some("Ctrl+=")
        );
        assert_eq!(
            bindings
                .label(Action::FocusPane(Direction::Left))
                .as_deref(),
            Some("Ctrl+Shift+Left")
        );
    }

    #[test]
    fn app_actions() {
        let bindings = defaults();
        let get = |key: &Key, mods| action_for(&bindings, key, mods);
        assert_eq!(get(&ch("C"), cs()), Some(Action::Copy));
        assert_eq!(get(&ch("V"), cs()), Some(Action::Paste));
        assert_eq!(get(&ch("c"), CTRL), None);
        assert_eq!(
            get(&named(NamedKey::PageUp), SHIFT),
            Some(Action::ScrollPageUp)
        );
        assert_eq!(get(&named(NamedKey::PageUp), NONE), None);
        assert_eq!(get(&ch("="), CTRL), Some(Action::FontSizeIncrease));
        assert_eq!(get(&ch("+"), CTRL), Some(Action::FontSizeIncrease));
        assert_eq!(get(&ch("-"), CTRL), Some(Action::FontSizeDecrease));
        assert_eq!(get(&ch("0"), CTRL), Some(Action::FontSizeReset));
        assert_eq!(get(&named(NamedKey::Insert), SHIFT), Some(Action::Paste));
        assert_eq!(get(&named(NamedKey::Paste), NONE), Some(Action::Paste));
        assert_eq!(get(&named(NamedKey::Copy), NONE), Some(Action::Copy));
        assert_eq!(get(&ch("P"), cs()), Some(Action::CommandPalette));
        assert_eq!(get(&ch("p"), cs()), Some(Action::CommandPalette));
    }

    #[test]
    fn block_actions() {
        let bindings = defaults();
        let get = |key: &Key, mods| action_for(&bindings, key, mods);
        assert_eq!(
            get(&named(NamedKey::PageUp), cs()),
            Some(Action::PreviousBlock)
        );
        assert_eq!(
            get(&named(NamedKey::PageDown), cs()),
            Some(Action::NextBlock)
        );
        assert_eq!(get(&ch("C"), csa()), Some(Action::CopyCommand));
        assert_eq!(get(&ch("c"), csa()), Some(Action::CopyCommand));
        assert_eq!(get(&ch("O"), csa()), Some(Action::CopyOutput));
        assert_eq!(get(&ch("F"), cs()), Some(Action::ToggleFold));
        assert_eq!(get(&ch("f"), cs()), Some(Action::ToggleFold));
        assert_eq!(get(&ch("F"), csa()), Some(Action::ToggleFoldAll));
        assert_eq!(get(&ch("f"), CTRL), None);
        assert_eq!(get(&ch("V"), csa()), None);
        assert_eq!(get(&named(NamedKey::PageUp), ALT), None);
    }

    #[test]
    fn pane_actions() {
        let bindings = defaults();
        let get = |key: &Key, mods| action_for(&bindings, key, mods);
        for (letter, action) in [
            ("E", Action::SplitRight),
            ("O", Action::SplitDown),
            ("W", Action::ClosePane),
            ("N", Action::NextPane),
            ("B", Action::PreviousPane),
            ("Z", Action::ToggleZoom),
        ] {
            assert_eq!(get(&ch(letter), cs()), Some(action), "{letter}");
            assert_eq!(
                get(&ch(&letter.to_lowercase()), cs()),
                Some(action),
                "{letter}"
            );
            assert_eq!(get(&ch(letter), CTRL), None, "{letter}");
        }
        for (key, direction) in [
            (NamedKey::ArrowLeft, Direction::Left),
            (NamedKey::ArrowRight, Direction::Right),
            (NamedKey::ArrowUp, Direction::Up),
            (NamedKey::ArrowDown, Direction::Down),
        ] {
            assert_eq!(get(&named(key), cs()), Some(Action::FocusPane(direction)));
            assert_eq!(get(&named(key), csa()), Some(Action::ResizePane(direction)));
            assert_eq!(get(&named(key), CTRL), None);
        }
    }

    #[test]
    fn tab_actions() {
        let bindings = defaults();
        let get = |key: &Key, mods| action_for(&bindings, key, mods);
        assert_eq!(get(&ch("T"), cs()), Some(Action::NewTab));
        assert_eq!(get(&named(NamedKey::Tab), CTRL), Some(Action::NextTab));
        assert_eq!(get(&named(NamedKey::Tab), cs()), Some(Action::PreviousTab));
        assert_eq!(get(&named(NamedKey::PageDown), CTRL), Some(Action::NextTab));
        assert_eq!(
            get(&named(NamedKey::PageUp), CTRL),
            Some(Action::PreviousTab)
        );
        assert_eq!(get(&named(NamedKey::Tab), NONE), None);
    }

    #[test]
    fn alt_digits_go_to_a_tab_by_their_physical_key() {
        let bindings = defaults();
        for number in 1..=9 {
            let physical = PhysicalKey::Code(digit_code(number));
            assert_eq!(
                bindings.action(&ch(&number.to_string()), physical, ALT),
                Some(Action::GoToTab(number))
            );
        }
        // AZERTY: the Digit1 key types `&` without Shift.
        let digit1 = PhysicalKey::Code(KeyCode::Digit1);
        assert_eq!(
            bindings.action(&ch("&"), digit1, ALT),
            Some(Action::GoToTab(1))
        );
        // Digit0, other modifiers and the logical digit alone are not tab shortcuts.
        let digit0 = PhysicalKey::Code(KeyCode::Digit0);
        assert_eq!(bindings.action(&ch("0"), digit0, ALT), None);
        assert_eq!(bindings.action(&ch("1"), digit1, ALT | CTRL), None);
        assert_eq!(action_for(&bindings, &ch("1"), ALT), None);
    }

    #[test]
    fn letters_fall_back_on_the_physical_key() {
        let bindings = defaults();
        let cyrillic = ch("У");
        assert_eq!(
            bindings.action(&cyrillic, PhysicalKey::Code(KeyCode::KeyE), cs()),
            Some(Action::SplitRight)
        );
        // The typed letter wins over its physical place (AZERTY: Z on the W key).
        assert_eq!(
            bindings.action(&ch("Z"), PhysicalKey::Code(KeyCode::KeyW), cs()),
            Some(Action::ToggleZoom)
        );
        assert_eq!(action_for(&bindings, &cyrillic, cs()), None);
    }

    #[test]
    fn unmapped_ctrl_shift_letters_stay_with_the_shell() {
        let bindings = defaults();
        for letter in ["A", "D", "G", "X"] {
            assert_eq!(action_for(&bindings, &ch(letter), cs()), None, "{letter}");
        }
    }

    #[test]
    fn shift_is_ignored_for_symbols_but_not_for_letters() {
        let bindings = defaults();
        // US layout: `+` is Shift+=.
        assert_eq!(
            action_for(&bindings, &ch("+"), cs()),
            Some(Action::FontSizeIncrease)
        );
        assert_eq!(
            action_for(&bindings, &ch("_"), cs()),
            Some(Action::FontSizeDecrease)
        );
        // `Ctrl+C` is not `Ctrl+Shift+C`.
        assert_eq!(action_for(&bindings, &ch("C"), CTRL), None);
        // A symbol bound with Shift is stored without it.
        let bindings = with(&[("Ctrl+Shift+!", "new_tab")]);
        assert_eq!(action_for(&bindings, &ch("!"), cs()), Some(Action::NewTab));
        assert_eq!(action_for(&bindings, &ch("!"), CTRL), Some(Action::NewTab));
        assert_eq!(bindings.label(Action::NewTab).as_deref(), Some("Ctrl+!"));
    }

    #[test]
    fn a_configured_shortcut_remaps_an_action() {
        let bindings = with(&[("Ctrl+Alt+T", "new_tab"), ("ctrl+shift+c", "paste")]);
        let get = |key: &Key, mods| action_for(&bindings, key, mods);
        assert_eq!(get(&ch("t"), CTRL | ALT), Some(Action::NewTab));
        assert_eq!(get(&ch("C"), cs()), Some(Action::Paste));
        // The default key of the action still works; the label shows the configured one.
        assert_eq!(get(&ch("T"), cs()), Some(Action::NewTab));
        assert_eq!(
            bindings.label(Action::NewTab).as_deref(),
            Some("Ctrl+Alt+T")
        );
        // Copy lost its first shortcut: the next one is shown.
        assert_eq!(bindings.label(Action::Copy).as_deref(), Some("Copy"));
        // An action without default key gets one.
        let bindings = with(&[("F7", "equalise_panes"), ("Alt+0", "go_to_tab_10")]);
        assert_eq!(
            action_for(&bindings, &named(NamedKey::F7), NONE),
            Some(Action::EqualisePanes)
        );
        assert_eq!(
            bindings.action(&ch("0"), PhysicalKey::Code(KeyCode::Digit0), ALT),
            Some(Action::GoToTab(10))
        );
        assert_eq!(
            bindings.label(Action::GoToTab(10)).as_deref(),
            Some("Alt+0")
        );
    }

    #[test]
    fn none_gives_the_key_back_to_the_shell() {
        let bindings = with(&[("Shift+PageUp", "none"), ("Ctrl+Shift+C", "none")]);
        assert_eq!(action_for(&bindings, &named(NamedKey::PageUp), SHIFT), None);
        assert_eq!(bindings.label(Action::ScrollPageUp), None);
        assert_eq!(action_for(&bindings, &ch("C"), cs()), None);
        assert_eq!(bindings.label(Action::Copy).as_deref(), Some("Copy"));
        // Removing a key that has no action is harmless.
        let bindings = with(&[("Ctrl+Shift+X", "none")]);
        assert_eq!(action_for(&bindings, &ch("X"), cs()), None);
    }

    #[test]
    fn invalid_entries_are_skipped() {
        let bindings = with(&[("Ctrl+Hyper+T", "new_tab"), ("Ctrl+T", "quit")]);
        assert_eq!(action_for(&bindings, &ch("t"), CTRL), None);
        assert_eq!(action_for(&bindings, &ch("T"), cs()), Some(Action::NewTab));
    }

    #[test]
    fn shortcut_syntax() {
        let parse = |text| Shortcut::parse(text).map(Shortcut::label);
        assert_eq!(parse("Ctrl+Shift+c").as_deref(), Ok("Ctrl+Shift+C"));
        assert_eq!(parse("shift+ctrl+C").as_deref(), Ok("Ctrl+Shift+C"));
        assert_eq!(parse("Super+Enter").as_deref(), Ok("Super+Enter"));
        assert_eq!(parse("Ctrl+pageup").as_deref(), Ok("Ctrl+PageUp"));
        assert_eq!(parse("Ctrl+ArrowLeft").as_deref(), Ok("Ctrl+Left"));
        assert_eq!(parse("F5").as_deref(), Ok("F5"));
        assert_eq!(parse("Ctrl++").as_deref(), Ok("Ctrl++"));
        assert_eq!(parse("+").as_deref(), Ok("+"));
        assert_eq!(parse("Ctrl+ ").as_deref(), Ok("Ctrl+Space"));
        assert_eq!(parse("Alt+3").as_deref(), Ok("Alt+3"));
        assert_eq!(parse("Ctrl+é").as_deref(), Ok("Ctrl+É"));
        assert_eq!(
            parse("Ctrl+Hyper+C"),
            Err("unknown modifier \"Hyper\" in \"Ctrl+Hyper+C\"".to_owned())
        );
        assert_eq!(
            parse("Ctrl+PgUp"),
            Err("unknown key \"PgUp\" in \"Ctrl+PgUp\"".to_owned())
        );
        assert_eq!(parse("Ctrl+"), Err("missing key in \"Ctrl+\"".to_owned()));
        assert_eq!(parse(""), Err("missing key in \"\"".to_owned()));
        assert!(parse("Ctrl++C").is_err());
    }

    #[test]
    fn binding_syntax() {
        assert_eq!(
            parse_binding("Ctrl+T", "new_tab").map(|(_, a)| a),
            Ok(Some(Action::NewTab))
        );
        assert_eq!(parse_binding("Ctrl+T", "none").map(|(_, a)| a), Ok(None));
        assert_eq!(
            parse_binding("Ctrl+T", "quit"),
            Err("unknown action \"quit\"".to_owned())
        );
        assert!(parse_binding("Hyper+T", "copy").is_err());
    }
}
