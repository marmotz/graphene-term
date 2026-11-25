//! X11 window state set before the window is mapped, the X11 ID of a window, and the
//! position of the mouse cursor.
//!
//! The terminal window is created hidden and shown once it can draw. winit asks for the
//! maximized state with a client message, which window managers ignore before the
//! window is mapped, so a hidden window would open at its configured size. Writing the
//! `_NET_WM_STATE` property instead makes the window manager map it maximized at once.
//! The quick terminal sets its own states the same way, and is given the keyboard focus
//! by hand: it is a dock window, which window managers do not focus.
//!
//! The work area of each screen, the part the desktop panels leave, is read from the
//! root window properties set by the window manager and the panels.

use anyhow::Context;
use wgpu::rwh::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt, InputFocus, PropMode, Window as XWindow,
};
use x11rb::wrapper::ConnectionExt as _;

use crate::quick_terminal::{self, PixelRect, Strut};

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

/// Gives the mapped `window` the keyboard focus. `Ok(false)` means the window is not
/// an X11 window.
pub fn focus(window: &Window) -> anyhow::Result<bool> {
    let Some(id) = window_id(window)? else {
        return Ok(false);
    };
    let (connection, _) = x11rb::connect(None)?;
    focus_on(&connection, id)?;
    Ok(true)
}

/// Gives `window` the keyboard focus through `connection`; fails while the window is not
/// viewable.
fn focus_on(connection: &impl Connection, window: u32) -> anyhow::Result<()> {
    connection
        .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)?
        .check()?;
    Ok(())
}

/// Position of the mouse cursor on the X11 root window, in physical pixels.
pub fn cursor_position() -> anyhow::Result<(i32, i32)> {
    let (connection, screen) = x11rb::connect(None)?;
    let root = connection.setup().roots[screen].root;
    let pointer = connection.query_pointer(root)?.reply()?;
    Ok((pointer.root_x.into(), pointer.root_y.into()))
}

/// The work area of each of `monitors` (physical pixels), in the same order: the
/// per-screen areas of `_GTK_WORKAREAS_D<n>` (Mutter, Muffin) when published, else the
/// screen without the struts of the client windows (`_NET_WM_STRUT_PARTIAL`, else
/// `_NET_WM_STRUT`), else the screen itself.
pub fn work_areas(monitors: &[PixelRect]) -> anyhow::Result<Vec<PixelRect>> {
    let (connection, screen) = x11rb::connect(None)?;
    let screen = &connection.setup().roots[screen];
    let root = (
        screen.root,
        (
            u32::from(screen.width_in_pixels),
            u32::from(screen.height_in_pixels),
        ),
    );
    work_areas_on(&connection, root, monitors)
}

/// [`work_areas`] through `connection`, `root` being the root window and its size.
fn work_areas_on(
    connection: &impl Connection,
    (root, root_size): (XWindow, (u32, u32)),
    monitors: &[PixelRect],
) -> anyhow::Result<Vec<PixelRect>> {
    let gtk = quick_terminal::rects_from_cardinals(&gtk_work_areas(connection, root)?);
    if !gtk.is_empty() {
        return Ok(monitors
            .iter()
            .map(|&monitor| quick_terminal::gtk_work_area(monitor, &gtk).unwrap_or(monitor))
            .collect());
    }
    let struts = struts(connection, root, root_size)?;
    Ok(monitors
        .iter()
        .map(|&monitor| monitor.inset(quick_terminal::strut_insets(monitor, root_size, &struts)))
        .collect())
}

/// The cardinals of `_GTK_WORKAREAS_D<n>` for the current desktop `<n>`, empty when the
/// window manager does not publish them.
fn gtk_work_areas(connection: &impl Connection, root: XWindow) -> anyhow::Result<Vec<u32>> {
    let current = existing_atom(connection, "_NET_CURRENT_DESKTOP")?
        .map(|atom| cardinals(connection, root, atom, AtomEnum::CARDINAL))
        .transpose()?
        .and_then(|values| values.first().copied())
        .unwrap_or(0);
    match existing_atom(connection, &format!("_GTK_WORKAREAS_D{current}"))? {
        Some(atom) => cardinals(connection, root, atom, AtomEnum::CARDINAL),
        None => Ok(Vec::new()),
    }
}

/// The struts of the windows of `_NET_CLIENT_LIST`; a window gone since the list was read
/// is skipped.
fn struts(
    connection: &impl Connection,
    root: XWindow,
    root_size: (u32, u32),
) -> anyhow::Result<Vec<Strut>> {
    let Some(client_list) = existing_atom(connection, "_NET_CLIENT_LIST")? else {
        return Ok(Vec::new());
    };
    let windows = cardinals(connection, root, client_list, AtomEnum::WINDOW)?;
    let partial = existing_atom(connection, "_NET_WM_STRUT_PARTIAL")?;
    let full = existing_atom(connection, "_NET_WM_STRUT")?;
    let mut struts = Vec::new();
    for window in windows {
        let read = |atom: Option<Atom>| -> Vec<u32> {
            atom.and_then(|atom| cardinals(connection, window, atom, AtomEnum::CARDINAL).ok())
                .unwrap_or_default()
        };
        if let Ok(values) = <[u32; 12]>::try_from(read(partial).as_slice()) {
            struts.push(Strut::from_partial(&values));
        } else if let Ok(values) = <[u32; 4]>::try_from(read(full).as_slice()) {
            struts.push(Strut::from_full(&values, root_size));
        }
    }
    Ok(struts)
}

/// The atom named `name`, `None` when no client created it: no window can carry it.
fn existing_atom(connection: &impl Connection, name: &str) -> anyhow::Result<Option<Atom>> {
    let atom = connection.intern_atom(true, name.as_bytes())?.reply()?.atom;
    Ok((atom != x11rb::NONE).then_some(atom))
}

/// The 32-bit values of the property `atom` of `window`, of type `kind`; empty when the
/// window does not have it.
fn cardinals(
    connection: &impl Connection,
    window: XWindow,
    atom: Atom,
    kind: AtomEnum,
) -> anyhow::Result<Vec<u32>> {
    let reply = connection
        .get_property(false, window, atom, kind, 0, u32::MAX / 4)?
        .reply()?;
    Ok(reply.value32().map(Iterator::collect).unwrap_or_default())
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
    fn focus_needs_a_viewable_window_and_gives_it_the_focus() {
        let Ok((connection, screen)) = x11rb::connect(None) else {
            eprintln!("skipped: no X11 display");
            return;
        };
        let root = connection.setup().roots[screen].root;
        let previous = connection.get_input_focus().unwrap().reply().unwrap().focus;
        let window = connection.generate_id().unwrap();
        // Out of the window manager's hands and off screen, so the test window is
        // neither decorated nor seen.
        connection
            .create_window(
                0,
                window,
                root,
                -100,
                -100,
                10,
                10,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new().override_redirect(1),
            )
            .unwrap();
        assert!(focus_on(&connection, window).is_err(), "unmapped window");

        connection.map_window(window).unwrap();
        connection.sync().unwrap();
        focus_on(&connection, window).unwrap();
        let focused = connection.get_input_focus().unwrap().reply().unwrap().focus;
        assert_eq!(focused, window);

        connection
            .set_input_focus(InputFocus::PARENT, previous, x11rb::CURRENT_TIME)
            .unwrap();
        connection.destroy_window(window).unwrap();
        connection.flush().unwrap();
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

    #[test]
    fn work_areas_give_one_rectangle_per_screen_inside_it() {
        let Ok((connection, screen)) = x11rb::connect(None) else {
            eprintln!("skipped: no X11 display");
            return;
        };
        let screen = &connection.setup().roots[screen];
        let (width, height) = (
            u32::from(screen.width_in_pixels),
            u32::from(screen.height_in_pixels),
        );
        let halves = [
            PixelRect {
                x: 0,
                y: 0,
                width: width / 2,
                height,
            },
            PixelRect {
                x: (width / 2) as i32,
                y: 0,
                width: width - width / 2,
                height,
            },
        ];

        let areas = work_areas(&halves).unwrap();

        assert_eq!(areas.len(), halves.len());
        for (area, half) in areas.iter().zip(halves) {
            assert_eq!(
                half.intersection(*area),
                Some(*area),
                "{area:?} in {half:?}"
            );
        }
    }

    #[test]
    fn work_areas_skip_the_struts_of_client_windows() {
        let Ok((connection, screen)) = x11rb::connect(None) else {
            eprintln!("skipped: no X11 display");
            return;
        };
        let root = connection.setup().roots[screen].root;
        // A fake root window, so the test does not depend on the desktop it runs on.
        let fake_root = connection.generate_id().unwrap();
        let panel = connection.generate_id().unwrap();
        for window in [fake_root, panel] {
            connection
                .create_window(
                    0,
                    window,
                    root,
                    -100,
                    -100,
                    10,
                    10,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    0,
                    &CreateWindowAux::new().override_redirect(1),
                )
                .unwrap();
        }
        let atom = |name: &str| {
            connection
                .intern_atom(false, name.as_bytes())
                .unwrap()
                .reply()
                .unwrap()
                .atom
        };
        let gone = connection.generate_id().unwrap();
        connection
            .change_property32(
                PropMode::REPLACE,
                fake_root,
                atom("_NET_CLIENT_LIST"),
                AtomEnum::WINDOW,
                &[gone, panel],
            )
            .unwrap();
        // A 40-pixel panel along the bottom of the left screen only.
        connection
            .change_property32(
                PropMode::REPLACE,
                panel,
                atom("_NET_WM_STRUT_PARTIAL"),
                AtomEnum::CARDINAL,
                &[0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, 999],
            )
            .unwrap();
        connection.sync().unwrap();
        let left = PixelRect {
            x: 0,
            y: 0,
            width: 1000,
            height: 800,
        };
        let right = PixelRect { x: 1000, ..left };

        let areas = work_areas_on(&connection, (fake_root, (2000, 800)), &[left, right]);

        connection.destroy_window(panel).unwrap();
        connection.destroy_window(fake_root).unwrap();
        connection.flush().unwrap();
        let areas = areas.unwrap();
        // `_GTK_WORKAREAS_D<n>` is read on the fake root, which has none.
        assert_eq!(
            areas,
            [
                PixelRect {
                    height: 760,
                    ..left
                },
                right
            ]
        );
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
