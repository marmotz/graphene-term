//! The settings tab of a session: shown first in the tab bar, drawn with egui over the
//! content area, and given the events of the window while it is shown.

use super::*;

/// Who gets an event of the window while the settings tab is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SettingsRoute {
    pub settings: bool,
    pub window: bool,
}

/// What an event of the window is, as far as its route goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventKind {
    /// A key or the input method: the keyboard.
    Keyboard,
    /// A mouse button pressed.
    Press,
    /// A mouse wheel turned.
    Wheel,
    /// Drawn by the window, which paints the tab in its frame.
    Redraw,
    /// Asked of the window alone: closing it.
    Close,
    /// Everything else (moves, releases, focus, size, modifiers): seen by both.
    Other,
}

impl EventKind {
    pub(super) fn of(event: &WindowEvent) -> Self {
        match event {
            WindowEvent::KeyboardInput { .. } | WindowEvent::Ime(_) => Self::Keyboard,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            } => Self::Press,
            WindowEvent::MouseWheel { .. } => Self::Wheel,
            WindowEvent::RedrawRequested => Self::Redraw,
            WindowEvent::CloseRequested => Self::Close,
            _ => Self::Other,
        }
    }
}

/// Routes an event of `kind` while the settings tab is shown: the window keeps the
/// keyboard while an `overlay` (dialog, menu) is open, and the presses and the wheel
/// over its tab bar (`over_bar`) or an overlay; the tab gets the rest of them.
pub(super) fn settings_route(kind: EventKind, overlay: bool, over_bar: bool) -> SettingsRoute {
    let (settings, window) = match kind {
        EventKind::Keyboard => (!overlay, overlay),
        EventKind::Press | EventKind::Wheel => {
            let window = overlay || over_bar;
            (!window, window)
        }
        EventKind::Redraw | EventKind::Close => (false, true),
        EventKind::Other => (true, true),
    };
    SettingsRoute { settings, window }
}

impl Session {
    /// Whether the settings tab is the one shown.
    pub(super) fn settings_shown(&self) -> bool {
        self.settings.is_some()
    }

    /// Shows the settings tab `settings`, opened by [`App`].
    pub(super) fn show_settings(&mut self, settings: SettingsTab) {
        self.settings = Some(settings);
        // egui allows the input method while one of its text fields has the keyboard.
        self.clear_composition();
        self.window.set_ime_allowed(false);
        // The panes are hidden: nothing of theirs stays hovered.
        self.hovered_cell = None;
        self.hovered_link = None;
        self.update_pointer();
        self.window.request_redraw();
    }

    /// Leaves the settings tab, when shown, for the active terminal tab: a color being
    /// picked is written and the terminal gets the theme's colors back.
    pub(super) fn leave_settings(&mut self) {
        let Some(mut settings) = self.settings.take() else {
            return;
        };
        settings.leave();
        // The terminal takes the input method back from egui.
        self.window.set_ime_allowed(self.ime_allowed);
        self.ime_area = None;
        if let Some(palette) = settings.take_preview() {
            self.preview_palette(&palette);
        }
        self.update_hover(false);
        self.update_pointer();
        self.window.request_redraw();
    }

    /// Hands `event` to the settings tab when it is shown and takes it (see
    /// [`settings_route`]). Returns whether the window handles the event too.
    pub(super) fn settings_window_event(&mut self, event: &WindowEvent) -> bool {
        if self.settings.is_none() {
            return true;
        }
        let (x, y) = self.mouse_pixel();
        let over_bar = self.tab_bar().rect.contains(x, y);
        let route = settings_route(EventKind::of(event), self.overlay.is_some(), over_bar);
        if route.settings
            && let Some(settings) = self.settings.as_mut()
        {
            let outcome = settings.window_event(event);
            if let Some(palette) = settings.take_preview() {
                self.preview_palette(&palette);
            }
            if outcome == settings::Outcome::Leave {
                self.leave_settings();
            }
        }
        route.window
    }

    /// Where the settings tab is laid out: the area the panes take otherwise.
    pub(super) fn settings_area(&self) -> Rect {
        self.geometry().area
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [EventKind; 6] = [
        EventKind::Keyboard,
        EventKind::Press,
        EventKind::Wheel,
        EventKind::Redraw,
        EventKind::Close,
        EventKind::Other,
    ];

    fn route(settings: bool, window: bool) -> SettingsRoute {
        SettingsRoute { settings, window }
    }

    #[test]
    fn the_tab_takes_the_keyboard_unless_an_overlay_is_open() {
        assert_eq!(
            settings_route(EventKind::Keyboard, false, false),
            route(true, false)
        );
        // Over the bar too: the keyboard does not follow the mouse.
        assert_eq!(
            settings_route(EventKind::Keyboard, false, true),
            route(true, false)
        );
        assert_eq!(
            settings_route(EventKind::Keyboard, true, false),
            route(false, true)
        );
    }

    #[test]
    fn presses_and_the_wheel_over_the_bar_or_an_overlay_are_the_window_s() {
        for kind in [EventKind::Press, EventKind::Wheel] {
            assert_eq!(settings_route(kind, false, false), route(true, false));
            assert_eq!(settings_route(kind, false, true), route(false, true));
            assert_eq!(settings_route(kind, true, false), route(false, true));
        }
    }

    #[test]
    fn the_window_draws_and_closes_alone() {
        for kind in [EventKind::Redraw, EventKind::Close] {
            assert_eq!(settings_route(kind, false, false), route(false, true));
        }
    }

    #[test]
    fn moves_releases_and_window_changes_go_to_both() {
        assert_eq!(
            settings_route(EventKind::Other, false, true),
            route(true, true)
        );
        assert_eq!(
            settings_route(EventKind::Other, true, false),
            route(true, true)
        );
    }

    #[test]
    fn every_event_reaches_someone() {
        for kind in ALL {
            for overlay in [false, true] {
                for over_bar in [false, true] {
                    let route = settings_route(kind, overlay, over_bar);
                    assert!(route.settings || route.window, "{kind:?}");
                }
            }
        }
    }

    #[test]
    fn events_are_sorted_by_kind() {
        assert_eq!(
            EventKind::of(&WindowEvent::RedrawRequested),
            EventKind::Redraw
        );
        assert_eq!(
            EventKind::of(&WindowEvent::CloseRequested),
            EventKind::Close
        );
        assert_eq!(
            EventKind::of(&WindowEvent::Ime(winit::event::Ime::Enabled)),
            EventKind::Keyboard
        );
        assert_eq!(EventKind::of(&WindowEvent::Focused(true)), EventKind::Other);
    }
}
