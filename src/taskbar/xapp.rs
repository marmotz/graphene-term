//! X11 window hints of the XApp library, read by the Cinnamon window lists (through
//! Muffin) instead of the Unity launcher entry: `_NET_WM_XAPP_PROGRESS`, 0 to 100, and
//! `_NET_WM_XAPP_PROGRESS_PULSE`, 1 for an indeterminate progress. Both are written on
//! the terminal window itself; other window managers ignore them.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use super::Shown;
use crate::progress::ProgressState;

/// Progress hint, a CARDINAL from 0 to 100; 0 removes the progress.
const PROGRESS: &[u8] = b"_NET_WM_XAPP_PROGRESS";
/// Indeterminate progress hint, a CARDINAL: 1 on, 0 off.
const PULSE: &[u8] = b"_NET_WM_XAPP_PROGRESS_PULSE";

/// The `PROGRESS` and `PULSE` values showing `shown`. As with XApp, an indeterminate
/// progress has no value, and no progress writes both to 0.
fn hint_values(shown: Option<Shown>) -> (u32, u32) {
    match shown {
        None => (0, 0),
        Some(Shown {
            state: ProgressState::Indeterminate,
            ..
        }) => (0, 1),
        Some(Shown { value, .. }) => (u32::from(value.min(100)), 0),
    }
}

/// The progress hints of one X11 window, through a connection of their own.
pub struct XAppHints {
    connection: RustConnection,
    window: u32,
    progress: u32,
    pulse: u32,
}

impl XAppHints {
    /// Connects to the display to write the hints of `window`.
    pub fn connect(window: u32) -> anyhow::Result<Self> {
        let (connection, _) = x11rb::connect(None)?;
        Self::on(connection, window)
    }

    fn on(connection: RustConnection, window: u32) -> anyhow::Result<Self> {
        let progress = connection.intern_atom(false, PROGRESS)?.reply()?.atom;
        let pulse = connection.intern_atom(false, PULSE)?.reply()?.atom;
        Ok(Self {
            connection,
            window,
            progress,
            pulse,
        })
    }

    /// Writes the hints showing `shown`.
    pub fn show(&self, shown: Option<Shown>) -> anyhow::Result<()> {
        let (progress, pulse) = hint_values(shown);
        for (atom, value) in [(self.progress, progress), (self.pulse, pulse)] {
            self.connection.change_property32(
                PropMode::REPLACE,
                self.window,
                atom,
                AtomEnum::CARDINAL,
                &[value],
            )?;
        }
        self.connection.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::protocol::xproto::{CreateWindowAux, WindowClass};

    fn shown(state: ProgressState, value: u8) -> Option<Shown> {
        Some(Shown { state, value })
    }

    #[test]
    fn a_value_is_the_progress_hint() {
        assert_eq!(hint_values(shown(ProgressState::Normal, 0)), (0, 0));
        assert_eq!(hint_values(shown(ProgressState::Normal, 42)), (42, 0));
        assert_eq!(hint_values(shown(ProgressState::Error, 60)), (60, 0));
        assert_eq!(hint_values(shown(ProgressState::Paused, 30)), (30, 0));
        assert_eq!(hint_values(shown(ProgressState::Normal, 100)), (100, 0));
    }

    #[test]
    fn an_indeterminate_progress_pulses() {
        assert_eq!(hint_values(shown(ProgressState::Indeterminate, 0)), (0, 1));
    }

    #[test]
    fn no_progress_clears_both_hints() {
        assert_eq!(hint_values(None), (0, 0));
    }

    fn cardinal(connection: &RustConnection, window: u32, atom: u32) -> Vec<u32> {
        connection
            .get_property(false, window, atom, AtomEnum::CARDINAL, 0, 1)
            .unwrap()
            .reply()
            .unwrap()
            .value32()
            .unwrap()
            .collect()
    }

    #[test]
    fn show_writes_both_hints_on_the_window() {
        let Ok((connection, screen)) = x11rb::connect(None) else {
            eprintln!("skipped: no X11 display");
            return;
        };
        let root = connection.setup().roots[screen].root;
        let window = connection.generate_id().unwrap();
        connection
            .create_window(
                0,
                window,
                root,
                0,
                0,
                10,
                10,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new(),
            )
            .unwrap();
        connection.flush().unwrap();
        let (reader, _) = x11rb::connect(None).unwrap();
        let hints = XAppHints::on(connection, window).unwrap();

        hints.show(shown(ProgressState::Normal, 42)).unwrap();
        hints.connection.sync().unwrap();
        assert_eq!(cardinal(&reader, window, hints.progress), [42]);
        assert_eq!(cardinal(&reader, window, hints.pulse), [0]);

        hints.show(shown(ProgressState::Indeterminate, 0)).unwrap();
        hints.connection.sync().unwrap();
        assert_eq!(cardinal(&reader, window, hints.progress), [0]);
        assert_eq!(cardinal(&reader, window, hints.pulse), [1]);

        hints.show(None).unwrap();
        hints.connection.sync().unwrap();
        assert_eq!(cardinal(&reader, window, hints.progress), [0]);
        assert_eq!(cardinal(&reader, window, hints.pulse), [0]);

        hints.connection.destroy_window(window).unwrap();
        hints.connection.flush().unwrap();
    }
}
