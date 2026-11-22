//! Global hotkey of the quick terminal: the `quick_terminal.hotkey` shortcut, written
//! as in `keybindings.toml`, registered with the system so that it toggles the quick
//! terminal from any application.
//!
//! - This module: the translation of a [`Shortcut`] into native key codes, tested on
//!   every platform, and [`Hotkey`], the registration of the running platform.
//! - `x11`: a key grab on the root window, in a thread of its own.
//! - `macos`: a Carbon hot key, on the main thread.
//!
//! A key the platform cannot name is an error, as is a shortcut another application
//! already holds: both are reported and leave the process running without a hotkey.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod x11;

use std::fmt;
use std::sync::Arc;

use winit::keyboard::{ModifiersState, NamedKey};

use crate::keybindings::{KeyId, Shortcut};

/// What a press of the hotkey runs. It is called from the hotkey thread on X11.
pub type OnPress = Arc<dyn Fn() + Send + Sync>;

/// Why the hotkey cannot be registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    /// The key has no code on this platform or keyboard.
    NoEquivalent(String),
    /// Another application already holds the shortcut.
    Taken(String),
    /// Any other failure, described.
    Failed(String),
}

impl fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoEquivalent(hotkey) => {
                write!(
                    f,
                    "the global hotkey \"{hotkey}\" has no key on this system"
                )
            }
            Self::Taken(hotkey) => write!(
                f,
                "the global hotkey \"{hotkey}\" is already used by another application"
            ),
            Self::Failed(reason) => write!(f, "cannot register the global hotkey: {reason}"),
        }
    }
}

/// The registered global hotkey of the process.
pub struct Hotkey {
    /// The shortcut registered, as written; empty when there is none.
    current: String,
    #[cfg(target_os = "linux")]
    grabber: Option<x11::Grabber>,
    #[cfg(target_os = "macos")]
    registration: macos::Registration,
    #[cfg(not(target_os = "macos"))]
    on_press: OnPress,
}

impl Hotkey {
    /// No hotkey yet; `on_press` runs at each press of the one [`Hotkey::set`]
    /// registers. On macOS, called on the main thread.
    pub fn new(on_press: OnPress) -> Self {
        Self {
            current: String::new(),
            #[cfg(target_os = "linux")]
            grabber: None,
            #[cfg(target_os = "macos")]
            registration: macos::Registration::new(on_press),
            #[cfg(not(target_os = "macos"))]
            on_press,
        }
    }

    /// Registers `text` in place of the current hotkey, or none when it is empty. A
    /// text equal to the registered one changes nothing. On error, no hotkey is left.
    pub fn set(&mut self, text: &str) -> Result<(), HotkeyError> {
        if text == self.current {
            return Ok(());
        }
        self.clear();
        if text.is_empty() {
            return Ok(());
        }
        let shortcut = Shortcut::parse(text).map_err(HotkeyError::Failed)?;
        self.register(text, shortcut)?;
        self.current = text.to_owned();
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn register(&mut self, text: &str, shortcut: Shortcut) -> Result<(), HotkeyError> {
        let key =
            x11_key(shortcut.key()).ok_or_else(|| HotkeyError::NoEquivalent(text.to_owned()))?;
        if self.grabber.is_none() {
            let grabber = x11::Grabber::start(self.on_press.clone())
                .map_err(|error| HotkeyError::Failed(format!("{error:#}")))?;
            self.grabber = Some(grabber);
        }
        let Some(grabber) = self.grabber.as_mut() else {
            return Err(HotkeyError::Failed("no X11 connection".into()));
        };
        grabber.grab(text, key, x11_modifiers(shortcut.mods()))
    }

    #[cfg(target_os = "macos")]
    fn register(&mut self, text: &str, shortcut: Shortcut) -> Result<(), HotkeyError> {
        let code = macos_key_code(shortcut.key())
            .ok_or_else(|| HotkeyError::NoEquivalent(text.to_owned()))?;
        self.registration
            .register(text, code, carbon_modifiers(shortcut.mods()))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn register(&mut self, _text: &str, _shortcut: Shortcut) -> Result<(), HotkeyError> {
        let _ = &self.on_press;
        Err(HotkeyError::Failed("unsupported platform".into()))
    }

    /// Releases the registered hotkey.
    fn clear(&mut self) {
        if self.current.is_empty() {
            return;
        }
        #[cfg(target_os = "linux")]
        if let Some(grabber) = self.grabber.as_mut() {
            grabber.ungrab();
        }
        #[cfg(target_os = "macos")]
        self.registration.unregister();
        self.current.clear();
    }
}

/// A key as X11 names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X11Key {
    /// A keysym, looked up in the keyboard mapping.
    Keysym(u32),
    /// A physical key of the digit row, by its keycode (evdev keycodes plus 8).
    Keycode(u8),
}

/// Keysyms of the named keys a shortcut may use (`<X11/keysymdef.h>` and
/// `<X11/XF86keysym.h>`); the function keys are computed from `F1`.
const X11_NAMED: &[(NamedKey, u32)] = &[
    (NamedKey::ArrowLeft, 0xff51),
    (NamedKey::ArrowUp, 0xff52),
    (NamedKey::ArrowRight, 0xff53),
    (NamedKey::ArrowDown, 0xff54),
    (NamedKey::PageUp, 0xff55),
    (NamedKey::PageDown, 0xff56),
    (NamedKey::Home, 0xff50),
    (NamedKey::End, 0xff57),
    (NamedKey::Insert, 0xff63),
    (NamedKey::Delete, 0xffff),
    (NamedKey::Backspace, 0xff08),
    (NamedKey::Tab, 0xff09),
    (NamedKey::Enter, 0xff0d),
    (NamedKey::Escape, 0xff1b),
    (NamedKey::Space, 0x0020),
    (NamedKey::ContextMenu, 0xff67),
    (NamedKey::PrintScreen, 0xff61),
    (NamedKey::Pause, 0xff13),
    (NamedKey::ScrollLock, 0xff14),
    (NamedKey::Copy, 0x1008_ff57),
    (NamedKey::Cut, 0x1008_ff58),
    (NamedKey::Paste, 0x1008_ff6d),
];

/// Keysym of `F1`; `F2` to `F24` follow it.
const X11_F1: u32 = 0xffbe;

/// The function keys, in order.
const FUNCTION_KEYS: [NamedKey; 24] = [
    NamedKey::F1,
    NamedKey::F2,
    NamedKey::F3,
    NamedKey::F4,
    NamedKey::F5,
    NamedKey::F6,
    NamedKey::F7,
    NamedKey::F8,
    NamedKey::F9,
    NamedKey::F10,
    NamedKey::F11,
    NamedKey::F12,
    NamedKey::F13,
    NamedKey::F14,
    NamedKey::F15,
    NamedKey::F16,
    NamedKey::F17,
    NamedKey::F18,
    NamedKey::F19,
    NamedKey::F20,
    NamedKey::F21,
    NamedKey::F22,
    NamedKey::F23,
    NamedKey::F24,
];

/// The X11 name of `key`: its keysym, a Latin-1 character being its own keysym, or the
/// keycode of a digit matched on its physical key. `None` for a key X11 cannot name.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn x11_key(key: KeyId) -> Option<X11Key> {
    match key {
        KeyId::Char(c) => {
            let code = u32::from(c);
            ((0x20..=0x7e).contains(&code) || (0xa0..=0xff).contains(&code))
                .then_some(X11Key::Keysym(code))
        }
        // `KEY_1` is evdev 2 and `KEY_0` evdev 11; X11 adds 8.
        KeyId::Digit(0) => Some(X11Key::Keycode(19)),
        KeyId::Digit(digit @ 1..=9) => Some(X11Key::Keycode(9 + digit)),
        KeyId::Digit(_) => None,
        KeyId::Named(named) => {
            if let Some(index) = FUNCTION_KEYS.iter().position(|&f| f == named) {
                return Some(X11Key::Keysym(X11_F1 + index as u32));
            }
            X11_NAMED
                .iter()
                .find(|&&(known, _)| known == named)
                .map(|&(_, keysym)| X11Key::Keysym(keysym))
        }
    }
}

/// X11 modifier masks: `Shift`, `Control`, `Mod1` (Alt) and `Mod4` (Super).
pub const X11_SHIFT: u16 = 1 << 0;
const X11_CONTROL: u16 = 1 << 2;
const X11_MOD1: u16 = 1 << 3;
const X11_MOD4: u16 = 1 << 6;

/// The X11 modifier mask of `mods`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn x11_modifiers(mods: ModifiersState) -> u16 {
    [
        (ModifiersState::SHIFT, X11_SHIFT),
        (ModifiersState::CONTROL, X11_CONTROL),
        (ModifiersState::ALT, X11_MOD1),
        (ModifiersState::SUPER, X11_MOD4),
    ]
    .iter()
    .filter(|(modifier, _)| mods.contains(*modifier))
    .fold(0, |mask, (_, bit)| mask | bit)
}

/// The keycodes typing `keysym` in a keyboard mapping (`GetKeyboardMapping` from
/// `min_keycode`, `per_keycode` keysyms per keycode), and whether they take `Shift`.
/// Every key is returned, a keysym being often on several (`F24`); unshifted keys are
/// preferred, and the other groups of the layout are ignored. `None` when no key types
/// it.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn find_keycodes(
    min_keycode: u8,
    per_keycode: u8,
    keysyms: &[u32],
    keysym: u32,
) -> Option<(Vec<u8>, bool)> {
    let per = usize::from(per_keycode).max(1);
    for column in 0..per.min(2) {
        let codes: Vec<u8> = keysyms
            .chunks(per)
            .enumerate()
            .filter(|(_, row)| row.get(column) == Some(&keysym))
            .filter_map(|(index, _)| u8::try_from(usize::from(min_keycode) + index).ok())
            .collect();
        if !codes.is_empty() {
            return Some((codes, column == 1));
        }
    }
    None
}

/// Whether a key press at `time` is an automatic repeat: X11 sends a release and a
/// press of the same time for each repeat of a held key.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn is_repeat(last_release: Option<u32>, time: u32) -> bool {
    last_release == Some(time)
}

/// macOS virtual key codes (`kVK_*` of `HIToolbox/Events.h`) of the characters of the
/// US ANSI layout: a character shortcut is matched on that physical key.
const MACOS_CHARS: &[(char, u32)] = &[
    ('a', 0x00),
    ('s', 0x01),
    ('d', 0x02),
    ('f', 0x03),
    ('h', 0x04),
    ('g', 0x05),
    ('z', 0x06),
    ('x', 0x07),
    ('c', 0x08),
    ('v', 0x09),
    ('b', 0x0b),
    ('q', 0x0c),
    ('w', 0x0d),
    ('e', 0x0e),
    ('r', 0x0f),
    ('y', 0x10),
    ('t', 0x11),
    ('1', 0x12),
    ('2', 0x13),
    ('3', 0x14),
    ('4', 0x15),
    ('6', 0x16),
    ('5', 0x17),
    ('=', 0x18),
    ('9', 0x19),
    ('7', 0x1a),
    ('-', 0x1b),
    ('8', 0x1c),
    ('0', 0x1d),
    (']', 0x1e),
    ('o', 0x1f),
    ('u', 0x20),
    ('[', 0x21),
    ('i', 0x22),
    ('p', 0x23),
    ('l', 0x25),
    ('j', 0x26),
    ('\'', 0x27),
    ('k', 0x28),
    (';', 0x29),
    ('\\', 0x2a),
    (',', 0x2b),
    ('/', 0x2c),
    ('n', 0x2d),
    ('m', 0x2e),
    ('.', 0x2f),
    ('`', 0x32),
];

/// macOS virtual key codes of the named keys. `Insert` is `kVK_Help`, where Mac
/// keyboards have it; `Delete` is `kVK_ForwardDelete`.
const MACOS_NAMED: &[(NamedKey, u32)] = &[
    (NamedKey::Enter, 0x24),
    (NamedKey::Tab, 0x30),
    (NamedKey::Space, 0x31),
    (NamedKey::Backspace, 0x33),
    (NamedKey::Escape, 0x35),
    (NamedKey::Insert, 0x72),
    (NamedKey::Home, 0x73),
    (NamedKey::PageUp, 0x74),
    (NamedKey::Delete, 0x75),
    (NamedKey::End, 0x77),
    (NamedKey::PageDown, 0x79),
    (NamedKey::ArrowLeft, 0x7b),
    (NamedKey::ArrowRight, 0x7c),
    (NamedKey::ArrowDown, 0x7d),
    (NamedKey::ArrowUp, 0x7e),
    (NamedKey::F1, 0x7a),
    (NamedKey::F2, 0x78),
    (NamedKey::F3, 0x63),
    (NamedKey::F4, 0x76),
    (NamedKey::F5, 0x60),
    (NamedKey::F6, 0x61),
    (NamedKey::F7, 0x62),
    (NamedKey::F8, 0x64),
    (NamedKey::F9, 0x65),
    (NamedKey::F10, 0x6d),
    (NamedKey::F11, 0x67),
    (NamedKey::F12, 0x6f),
    (NamedKey::F13, 0x69),
    (NamedKey::F14, 0x6b),
    (NamedKey::F15, 0x71),
    (NamedKey::F16, 0x6a),
    (NamedKey::F17, 0x40),
    (NamedKey::F18, 0x4f),
    (NamedKey::F19, 0x50),
    (NamedKey::F20, 0x5a),
];

/// The macOS virtual key code of `key`. `None` for a key Mac keyboards lack (`F21` and
/// above, `PrintScreen`, `Copy`...) or a character off the US ANSI layout.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn macos_key_code(key: KeyId) -> Option<u32> {
    let char_code = |c: char| {
        MACOS_CHARS
            .iter()
            .find(|&&(known, _)| known == c)
            .map(|&(_, code)| code)
    };
    match key {
        KeyId::Char(c) => char_code(c),
        KeyId::Digit(digit) => char::from_digit(digit.into(), 10).and_then(char_code),
        KeyId::Named(named) => MACOS_NAMED
            .iter()
            .find(|&&(known, _)| known == named)
            .map(|&(_, code)| code),
    }
}

/// Carbon modifier masks (`cmdKey`, `shiftKey`, `optionKey`, `controlKey` of
/// `HIToolbox/Events.h`).
const CARBON_CMD: u32 = 1 << 8;
const CARBON_SHIFT: u32 = 1 << 9;
const CARBON_OPTION: u32 = 1 << 11;
const CARBON_CONTROL: u32 = 1 << 12;

/// The Carbon modifier mask of `mods`: `Super` is the Command key, as winit reports it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn carbon_modifiers(mods: ModifiersState) -> u32 {
    [
        (ModifiersState::SHIFT, CARBON_SHIFT),
        (ModifiersState::CONTROL, CARBON_CONTROL),
        (ModifiersState::ALT, CARBON_OPTION),
        (ModifiersState::SUPER, CARBON_CMD),
    ]
    .iter()
    .filter(|(modifier, _)| mods.contains(*modifier))
    .fold(0, |mask, (_, bit)| mask | bit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> KeyId {
        Shortcut::parse(text).unwrap().key()
    }

    fn mods(text: &str) -> ModifiersState {
        Shortcut::parse(text).unwrap().mods()
    }

    #[test]
    fn characters_are_their_own_latin1_keysym() {
        assert_eq!(x11_key(key("Ctrl+`")), Some(X11Key::Keysym(0x60)));
        // Letters are lowercased by the parser: the keysym of the unshifted key.
        assert_eq!(x11_key(key("Ctrl+Shift+T")), Some(X11Key::Keysym(0x74)));
        assert_eq!(x11_key(key("Super+é")), Some(X11Key::Keysym(0xe9)));
        assert_eq!(x11_key(key("Space")), Some(X11Key::Keysym(0x20)));
    }

    #[test]
    fn named_keys_have_their_keysym() {
        assert_eq!(x11_key(key("F1")), Some(X11Key::Keysym(0xffbe)));
        assert_eq!(x11_key(key("F12")), Some(X11Key::Keysym(0xffc9)));
        assert_eq!(x11_key(key("F24")), Some(X11Key::Keysym(0xffd5)));
        assert_eq!(x11_key(key("PageDown")), Some(X11Key::Keysym(0xff56)));
        assert_eq!(x11_key(key("Delete")), Some(X11Key::Keysym(0xffff)));
        assert_eq!(x11_key(key("Paste")), Some(X11Key::Keysym(0x1008_ff6d)));
    }

    #[test]
    fn every_named_key_of_a_shortcut_has_a_keysym() {
        for name in [
            "Left",
            "Right",
            "Up",
            "Down",
            "PageUp",
            "PageDown",
            "Home",
            "End",
            "Insert",
            "Delete",
            "Backspace",
            "Tab",
            "Enter",
            "Escape",
            "Copy",
            "Paste",
            "Cut",
            "ContextMenu",
            "PrintScreen",
            "Pause",
            "ScrollLock",
        ] {
            assert!(x11_key(key(name)).is_some(), "{name}");
        }
    }

    #[test]
    fn alt_digits_are_matched_on_the_digit_row() {
        assert_eq!(x11_key(key("Alt+1")), Some(X11Key::Keycode(10)));
        assert_eq!(x11_key(key("Alt+9")), Some(X11Key::Keycode(18)));
        assert_eq!(x11_key(key("Alt+0")), Some(X11Key::Keycode(19)));
    }

    #[test]
    fn characters_beyond_latin1_have_no_x11_key() {
        assert_eq!(x11_key(key("Ctrl+€")), None);
        assert_eq!(x11_key(key("Ctrl+ж")), None);
    }

    #[test]
    fn x11_modifiers_follow_the_core_masks() {
        assert_eq!(x11_modifiers(mods("Ctrl+F12")), 1 << 2);
        assert_eq!(
            x11_modifiers(mods("Ctrl+Shift+Alt+Super+T")),
            (1 << 0) | (1 << 2) | (1 << 3) | (1 << 6)
        );
        assert_eq!(x11_modifiers(mods("F12")), 0);
    }

    #[test]
    fn keycode_lookup_prefers_the_unshifted_key() {
        // Keycodes 10 to 12, two keysyms each: `1 !`, `2 @`, `a A`.
        let mapping = [0x31, 0x21, 0x32, 0x40, 0x61, 0x41];
        assert_eq!(
            find_keycodes(10, 2, &mapping, 0x61),
            Some((vec![12], false))
        );
        assert_eq!(find_keycodes(10, 2, &mapping, 0x21), Some((vec![10], true)));
        assert_eq!(find_keycodes(10, 2, &mapping, 0x7e), None);
        // A keysym found unshifted on a later key wins over a shifted earlier one.
        let mapping = [0x31, 0x2b, 0x2b, 0x00];
        assert_eq!(
            find_keycodes(10, 2, &mapping, 0x2b),
            Some((vec![11], false))
        );
    }

    #[test]
    fn keycode_lookup_returns_every_key_of_the_keysym() {
        // `F24` on keycodes 93 and 95.
        let mapping = [0xffd5, 0, 0xffc9, 0, 0xffd5, 0];
        assert_eq!(
            find_keycodes(93, 2, &mapping, 0xffd5),
            Some((vec![93, 95], false))
        );
    }

    #[test]
    fn keycode_lookup_ignores_the_other_layout_groups() {
        // One keycode, four keysyms: group 2 types `ф`.
        let mapping = [0x61, 0x41, 0x6c6, 0x6e6];
        assert_eq!(find_keycodes(8, 4, &mapping, 0x6c6), None);
    }

    #[test]
    fn a_release_and_a_press_of_the_same_time_are_a_repeat() {
        assert!(!is_repeat(None, 100));
        assert!(is_repeat(Some(100), 100));
        assert!(!is_repeat(Some(90), 100));
    }

    #[test]
    fn macos_keys_have_their_virtual_code() {
        assert_eq!(macos_key_code(key("Ctrl+`")), Some(0x32));
        assert_eq!(macos_key_code(key("Super+T")), Some(0x11));
        assert_eq!(macos_key_code(key("F1")), Some(0x7a));
        assert_eq!(macos_key_code(key("F12")), Some(0x6f));
        assert_eq!(macos_key_code(key("F20")), Some(0x5a));
        assert_eq!(macos_key_code(key("Space")), Some(0x31));
        assert_eq!(macos_key_code(key("Alt+1")), Some(0x12));
        assert_eq!(macos_key_code(key("Alt+0")), Some(0x1d));
    }

    #[test]
    fn keys_mac_keyboards_lack_have_no_virtual_code() {
        for name in [
            "F21",
            "F24",
            "PrintScreen",
            "Pause",
            "Copy",
            "Ctrl+é",
            "Ctrl+€",
        ] {
            assert_eq!(macos_key_code(key(name)), None, "{name}");
        }
    }

    #[test]
    fn carbon_modifiers_map_super_to_command() {
        assert_eq!(carbon_modifiers(mods("Super+T")), 1 << 8);
        assert_eq!(
            carbon_modifiers(mods("Ctrl+Shift+Alt+T")),
            (1 << 12) | (1 << 9) | (1 << 11)
        );
    }

    #[test]
    fn an_empty_hotkey_registers_nothing() {
        let mut hotkey = Hotkey::new(Arc::new(|| {}));
        assert_eq!(hotkey.set(""), Ok(()));
        assert_eq!(hotkey.current, "");
    }

    #[test]
    fn errors_name_the_hotkey() {
        assert_eq!(
            HotkeyError::Taken("Ctrl+`".into()).to_string(),
            "the global hotkey \"Ctrl+`\" is already used by another application"
        );
        assert_eq!(
            HotkeyError::NoEquivalent("Ctrl+€".into()).to_string(),
            "the global hotkey \"Ctrl+€\" has no key on this system"
        );
    }
}
