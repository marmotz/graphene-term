//! X11 window state set before the window is mapped, and the X11 ID of a window.
//!
//! The terminal window is created hidden and shown once it can draw. winit asks for the
//! maximized state with a client message, which window managers ignore before the
//! window is mapped, so a hidden window would open at its configured size. Writing the
//! `_NET_WM_STATE` property instead makes the window manager map it maximized at once.

use anyhow::Context;
use wgpu::rwh::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode};
use x11rb::wrapper::ConnectionExt as _;

/// Atoms of the maximized state, both directions.
const MAXIMIZED: [&str; 2] = [
    "_NET_WM_STATE_MAXIMIZED_VERT",
    "_NET_WM_STATE_MAXIMIZED_HORZ",
];

/// Marks the hidden `window` maximized for its first mapping. `Ok(false)` means the
/// window is not an X11 window.
pub fn preset_maximized(window: &Window) -> anyhow::Result<bool> {
    let Some(id) = window_id(window)? else {
        return Ok(false);
    };
    let (connection, _) = x11rb::connect(None)?;
    preset_maximized_on(&connection, id)?;
    Ok(true)
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

/// Writes the maximized `_NET_WM_STATE` of `window` through `connection`, and waits
/// until the server has applied it: winit maps the window through its own connection.
fn preset_maximized_on(connection: &impl Connection, window: u32) -> anyhow::Result<()> {
    let state = connection
        .intern_atom(false, b"_NET_WM_STATE")?
        .reply()?
        .atom;
    let mut atoms = [0; 2];
    for (atom, name) in atoms.iter_mut().zip(MAXIMIZED) {
        *atom = connection
            .intern_atom(false, name.as_bytes())?
            .reply()?
            .atom;
    }
    connection.change_property32(PropMode::REPLACE, window, state, AtomEnum::ATOM, &atoms)?;
    connection.sync()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::protocol::xproto::{CreateWindowAux, WindowClass};

    #[test]
    fn preset_maximized_writes_both_directions_before_mapping() {
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

        preset_maximized_on(&connection, window).unwrap();

        let state = connection
            .intern_atom(false, b"_NET_WM_STATE")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let value: Vec<u32> = connection
            .get_property(false, window, state, AtomEnum::ATOM, 0, 8)
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
        assert_eq!(names, MAXIMIZED);
        connection.destroy_window(window).unwrap();
        connection.flush().unwrap();
    }
}
