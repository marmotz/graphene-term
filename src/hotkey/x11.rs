//! X11 side of the global hotkey: a passive key grab on the root window, through an
//! `x11rb` connection of its own whose events a thread reads.
//!
//! The grab is made for the shortcut's modifiers with and without `Lock` and `Mod2`
//! (Caps Lock and Num Lock), which X11 counts as modifiers too. `BadAccess` means
//! another client already grabs the same combination.

use std::sync::Arc;

use anyhow::Context;
use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask};
use x11rb::protocol::{ErrorKind, Event};
use x11rb::rust_connection::RustConnection;

use super::{HotkeyError, OnPress, X11_SHIFT, X11Key, find_keycodes, is_repeat};

/// Lock modifiers a grab is repeated with: none, Caps Lock, Num Lock, both.
const LOCK_VARIANTS: [u16; 4] = [0, 1 << 1, 1 << 4, (1 << 1) | (1 << 4)];

/// The grab of the hotkey, and the connection holding it.
pub struct Grabber {
    connection: Arc<RustConnection>,
    root: u32,
    /// The keycodes and the modifiers grabbed.
    grabbed: Option<(Vec<u8>, u16)>,
}

impl Grabber {
    /// Connects to the X server and reads its events in a thread, calling `on_press` at
    /// each press of a grabbed key that is not an automatic repeat. The thread ends
    /// with the connection, at the end of the X session.
    pub fn start(on_press: OnPress) -> anyhow::Result<Self> {
        let (connection, screen) = x11rb::connect(None).context("cannot connect to X11")?;
        let root = connection.setup().roots[screen].root;
        let connection = Arc::new(connection);
        let reader = connection.clone();
        std::thread::Builder::new()
            .name("quick-terminal-hotkey".into())
            .spawn(move || listen(&reader, &*on_press))?;
        Ok(Self {
            connection,
            root,
            grabbed: None,
        })
    }

    /// Grabs `key` with the modifier mask `mods`, in place of the current grab: every
    /// key typing it. `text` names the hotkey in errors.
    pub fn grab(&mut self, text: &str, key: X11Key, mods: u16) -> Result<(), HotkeyError> {
        self.ungrab();
        let (keycodes, shifted) = match key {
            X11Key::Keycode(code) => (vec![code], false),
            X11Key::Keysym(keysym) => self
                .keycodes_of(keysym)?
                .ok_or_else(|| HotkeyError::NoEquivalent(text.to_owned()))?,
        };
        let mods = if shifted { mods | X11_SHIFT } else { mods };
        for (done, &keycode) in keycodes.iter().enumerate() {
            if let Err(error) = self.grab_keycode(text, keycode, mods) {
                self.release(&keycodes[..done], mods);
                return Err(error);
            }
        }
        self.grabbed = Some((keycodes, mods));
        Ok(())
    }

    /// Grabs `keycode` with `mods` and each lock variant, or none of them.
    fn grab_keycode(&self, text: &str, keycode: u8, mods: u16) -> Result<(), HotkeyError> {
        let failed = |error: ReplyError| HotkeyError::Failed(error.to_string());
        for (done, lock) in LOCK_VARIANTS.iter().enumerate() {
            let result = self
                .connection
                .grab_key(
                    false,
                    self.root,
                    ModMask::from(mods | lock),
                    keycode,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                )
                .map_err(|error| failed(error.into()))
                .and_then(|cookie| {
                    cookie.check().map_err(|error| match &error {
                        ReplyError::X11Error(x11) if x11.error_kind == ErrorKind::Access => {
                            HotkeyError::Taken(text.to_owned())
                        }
                        _ => failed(error),
                    })
                });
            if let Err(error) = result {
                self.release_locks(keycode, mods, &LOCK_VARIANTS[..done]);
                return Err(error);
            }
        }
        Ok(())
    }

    /// Releases the current grab.
    pub fn ungrab(&mut self) {
        if let Some((keycodes, mods)) = self.grabbed.take() {
            self.release(&keycodes, mods);
        }
    }

    /// Releases the grabs of `keycodes` with `mods`, every lock variant.
    fn release(&self, keycodes: &[u8], mods: u16) {
        for &keycode in keycodes {
            self.release_locks(keycode, mods, &LOCK_VARIANTS);
        }
    }

    /// Releases the grabs of `keycode` with `mods` and each of `locks`.
    fn release_locks(&self, keycode: u8, mods: u16, locks: &[u16]) {
        for lock in locks {
            let released = self
                .connection
                .ungrab_key(keycode, self.root, ModMask::from(mods | lock))
                .map(|cookie| cookie.ignore_error());
            if let Err(error) = released {
                log::debug!("cannot release the global hotkey: {error}");
            }
        }
        if let Err(error) = self.connection.flush() {
            log::debug!("cannot release the global hotkey: {error}");
        }
    }

    /// The keycodes typing `keysym` in the current keyboard mapping, and whether they
    /// take `Shift`.
    fn keycodes_of(&self, keysym: u32) -> Result<Option<(Vec<u8>, bool)>, HotkeyError> {
        let setup = self.connection.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let count = max.saturating_sub(min).saturating_add(1);
        let mapping = self
            .connection
            .get_keyboard_mapping(min, count)
            .map_err(|error| HotkeyError::Failed(error.to_string()))?
            .reply()
            .map_err(|error| HotkeyError::Failed(error.to_string()))?;
        Ok(find_keycodes(
            min,
            mapping.keysyms_per_keycode,
            &mapping.keysyms,
            keysym,
        ))
    }
}

impl Drop for Grabber {
    fn drop(&mut self) {
        self.ungrab();
    }
}

/// Reads the events of `connection` until it closes: each press of the grabbed key
/// runs `on_press`, its automatic repeats aside.
fn listen(connection: &RustConnection, on_press: &(dyn Fn() + Send + Sync)) {
    let mut last_release = None;
    loop {
        match connection.wait_for_event() {
            Ok(Event::KeyPress(press)) => {
                if !is_repeat(last_release, press.time) {
                    log::debug!("global hotkey pressed");
                    on_press();
                }
            }
            Ok(Event::KeyRelease(release)) => last_release = Some(release.time),
            Ok(_) => {}
            Err(error) => {
                log::debug!("global hotkey connection closed: {error}");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey::{x11_key, x11_modifiers};
    use crate::keybindings::Shortcut;

    /// A combination no desktop binds, so that the test grab disturbs nothing.
    const UNUSED: &str = "Ctrl+Shift+Alt+Super+F24";

    fn grabber() -> Option<Grabber> {
        if x11rb::connect(None).is_err() {
            eprintln!("skipped: no X11 display");
            return None;
        }
        Some(Grabber::start(Arc::new(|| {})).unwrap())
    }

    fn parts(text: &str) -> (X11Key, u16) {
        let shortcut = Shortcut::parse(text).unwrap();
        (
            x11_key(shortcut.key()).unwrap(),
            x11_modifiers(shortcut.mods()),
        )
    }

    #[test]
    fn grab_key_takes_the_combination_once() {
        let Some(mut first) = grabber() else {
            return;
        };
        let (key, mods) = parts(UNUSED);
        match first.grab(UNUSED, key, mods) {
            Ok(()) => {}
            Err(HotkeyError::NoEquivalent(_)) => {
                eprintln!("skipped: F24 has no keycode on this keyboard");
                return;
            }
            Err(error) => panic!("{error}"),
        }
        // Another client grabbing it gets `BadAccess`.
        let mut second = grabber().unwrap();
        assert_eq!(
            second.grab(UNUSED, key, mods),
            Err(HotkeyError::Taken(UNUSED.into()))
        );
        // Once released, it is free again.
        first.ungrab();
        assert_eq!(second.grab(UNUSED, key, mods), Ok(()));
        second.ungrab();
    }
}
