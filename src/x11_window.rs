//! X11 window state set before the window is mapped, the X11 ID of a window, and the
//! position of the mouse cursor.
//!
//! The terminal window is created hidden and shown once it can draw. winit asks for the
//! maximized state with a client message, which window managers ignore before the
//! window is mapped, so a hidden window would open at its configured size. Writing the
//! `_NET_WM_STATE` property instead makes the window manager map it maximized at once.
//! The quick terminal sets its own states the same way.

use anyhow::Context;
use wgpu::rwh::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode};
use x11rb::wrapper::ConnectionExt as _;

/// Atoms of the maximized state, both directions.
pub const MAXIMIZED: [&str; 2] = [
    "_NET_WM_STATE_MAXIMIZED_VERT",
    "_NET_WM_STATE_MAXIMIZED_HORZ",
];

/// Atoms of the quick terminal: out of the taskbar and the pager, above the other
/// windows, on every desktop.
pub const QUICK: [&str; 4] = [
    "_NET_WM_STATE_SKIP_TASKBAR",
    "_NET_WM_STATE_SKIP_PAGER",
    "_NET_WM_STATE_ABOVE",
    "_NET_WM_STATE_STICKY",
];

/// Gives the hidden `window` the `_NET_WM_STATE` atoms `states` for its first mapping.
/// `Ok(false)` means the window is not an X11 window.
pub fn preset_states(window: &Window, states: &[&str]) -> anyhow::Result<bool> {
    let Some(id) = window_id(window)? else {
        return Ok(false);
    };
    let (connection, _) = x11rb::connect(None)?;
    preset_states_on(&connection, id, states)?;
    Ok(true)
}

/// Position of the mouse cursor on the X11 root window, in physical pixels.
pub fn cursor_position() -> anyhow::Result<(i32, i32)> {
    let (connection, screen) = x11rb::connect(None)?;
    let root = connection.setup().roots[screen].root;
    let pointer = connection.query_pointer(root)?.reply()?;
    Ok((pointer.root_x.into(), pointer.root_y.into()))
}

/// The X11 ID of `window`, `None` when it is not an X11 window.
pub fn window_id(window: &Window) -> anyhow::Result<Option<u32>> {
    let id = match window.window_handle()?.as_raw() {
        RawWindowHandle::Xlib(handle) => {
            u32::try_from(handle.window).context("X11 window ID overflow")?
        }
        RawWindowHandle::Xcb(handle) => handle.window.get(),
        _ => return Ok(None),
    };
    Ok(Some(id))
}

/// Writes the `_NET_WM_STATE` atoms `states` of `window` through `connection`, and
/// waits until the server has applied them: winit maps the window through its own
/// connection.
fn preset_states_on(
    connection: &impl Connection,
    window: u32,
    states: &[&str],
) -> anyhow::Result<()> {
    let state = connection
        .intern_atom(false, b"_NET_WM_STATE")?
        .reply()?
        .atom;
    let atoms = states
        .iter()
        .map(|name| {
            Ok(connection
                .intern_atom(false, name.as_bytes())?
                .reply()?
                .atom)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    connection.change_property32(PropMode::REPLACE, window, state, AtomEnum::ATOM, &atoms)?;
    connection.sync()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::protocol::xproto::{CreateWindowAux, WindowClass};

    #[test]
    fn preset_states_writes_the_maximized_and_quick_states_before_mapping() {
        for states in [&MAXIMIZED[..], &QUICK[..]] {
            preset_states_writes(states);
        }
    }

    #[test]
    fn cursor_position_is_read_on_the_root_window() {
        if x11rb::connect(None).is_err() {
            eprintln!("skipped: no X11 display");
            return;
        }
        let (x, y) = cursor_position().unwrap();
        assert!(x >= 0 && y >= 0, "cursor at {x},{y}");
    }

    fn preset_states_writes(states: &[&str]) {
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

        preset_states_on(&connection, window, states).unwrap();

        let state = connection
            .intern_atom(false, b"_NET_WM_STATE")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let value: Vec<u32> = connection
            .get_property(false, window, state, AtomEnum::ATOM, 0, 16)
            .unwrap()
            .reply()
            .unwrap()
            .value32()
            .unwrap()
            .collect();
        let names: Vec<String> = value
            .iter()
            .map(|&atom| {
                let reply = connection.get_atom_name(atom).unwrap().reply().unwrap();
                String::from_utf8(reply.name).unwrap()
            })
            .collect();
        assert_eq!(names, states);
        connection.destroy_window(window).unwrap();
        connection.flush().unwrap();
    }
}
