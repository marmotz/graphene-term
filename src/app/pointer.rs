//! Mouse buttons, moves and wheel of a session.

use super::*;

impl Session {
    pub(super) fn mouse_input(
        &mut self,
        state: ElementState,
        button: MouseButton,
        default_font_size: f32,
        event_loop: &ActiveEventLoop,
    ) {
        let Some(button) = mouse::button_from(button) else {
            return;
        };
        match state {
            ElementState::Pressed => self.mouse_press(button, default_font_size, event_loop),
            ElementState::Released => self.mouse_release(button),
        }
    }

    /// A press goes to the first of: the overlay, the banner, the tab bar, a divider, a
    /// pane bar, a pane. While an overlay is open, it takes every press.
    pub(super) fn mouse_press(
        &mut self,
        button: mouse::Button,
        default_font_size: f32,
        event_loop: &ActiveEventLoop,
    ) {
        if self.overlay.is_some() {
            self.overlay_press(button, default_font_size, event_loop);
            return;
        }
        // The other buttons do nothing while a pane is dragged.
        if self.pane_dragging() {
            return;
        }
        if button == mouse::Button::Left && self.banner_press() {
            return;
        }
        let (x, y) = self.mouse_pixel();
        let bar = self.tab_bar();
        if bar.rect.contains(x, y) {
            if let Some(hit) = bar.hit(x, y) {
                self.tab_bar_press(button, hit, &bar, event_loop);
            }
            return;
        }
        let layout = self.shown_layout();
        if button == mouse::Button::Left
            && let Some(divider) = layout.divider_at(x, y)
        {
            self.drag = Some(Drag::Divider {
                node_path: divider.node_path.clone(),
                axis: divider.axis,
            });
            self.update_pointer();
            return;
        }
        if let Some((pane, hit)) = self.pane_bar_at(&layout, x, y) {
            self.pane_bar_press(button, pane, hit, event_loop);
            return;
        }
        // A press in an inactive pane makes it active, then acts as in the active one.
        let Some(target) = layout.pane_at(x, y) else {
            return;
        };
        if button == mouse::Button::Left
            && self.modifiers.control_key()
            && self.broadcast_press(target)
        {
            return;
        }
        if self.workspace.active_pane_id() != Some(target) {
            self.focus_pane(target);
        }
        let (position, modifiers, lang) = (self.mouse_position, self.modifiers, self.lang);
        let Some(pane) = self.workspace.pane_mut(target) else {
            return;
        };
        let (hit, _) = pane.pointer_hit(position);
        let link = match hit {
            Hit::Cell {
                source,
                line,
                column,
                ..
            } if button == mouse::Button::Left => {
                let point = Point::new(line, alacritty_terminal::index::Column(column));
                pane.link_at(source, point)
                    .map(|link| (source, point, link))
            }
            _ => None,
        };
        // The margin, the chevrons and the gaps are outside the application's grid:
        // always Graphene's.
        if button == mouse::Button::Left {
            match hit {
                Hit::Margin { row } => {
                    pane.margin_press(row);
                    return;
                }
                Hit::Chevron { row } if pane.chevron_press(row) => return,
                Hit::Gap => return,
                Hit::Chevron { .. } | Hit::Cell { .. } | Hit::Blank => {}
            }
        }
        let mode = pane.mode();
        // Only the live rows are the application's: elsewhere the mouse is Graphene's.
        let captured = selection::mouse_reserved(mode, modifiers) && pane.reports_at(&hit);
        if captured
            && modifiers.control_key()
            && let Some((source, point, link)) = link
        {
            // `Ctrl` + press on a link: Graphene's, never reported; opened on release.
            self.link_press = Some(LinkPress {
                pane: target,
                press: links::Press {
                    cell: (source, point),
                    clicks: 1,
                    mods: modifiers,
                    captured: true,
                },
                link,
            });
            return;
        }
        if captured {
            // The gesture belongs to the application; a click clears the selection.
            pane.clear_selection();
            let cell = pane.mouse_cell(position);
            if let Some(bytes) = pane.app_mouse.press(button, cell, modifiers, mode) {
                pane.terminal.write(bytes);
            }
            return;
        }
        match button {
            mouse::Button::Left => {
                let id = pane.id;
                let dragging = pane.left_press(position, modifiers, lang);
                let clicks = pane.last_clicks();
                // The selection starts as usual: the release tells whether it was a click
                // on the link.
                self.link_press = link.map(|(source, point, link)| LinkPress {
                    pane: id,
                    press: links::Press {
                        cell: (source, point),
                        clicks,
                        mods: modifiers,
                        captured: false,
                    },
                    link,
                });
                if dragging {
                    self.drag = Some(Drag::Selection {
                        pane: id,
                        auto_scroll_deadline: None,
                    });
                }
            }
            mouse::Button::Middle => self.paste_primary(),
            mouse::Button::Right => self.open_pane_menu(target, hit),
        }
    }

    /// A left press with `Ctrl` in the content of `target`: when it is the broadcast
    /// click (see [`selection::is_broadcast_press`]), records it without focusing the
    /// pane or reporting it to the application. `false` when the press goes on as usual.
    pub(super) fn broadcast_press(&mut self, target: PaneId) -> bool {
        let (position, modifiers) = (self.mouse_position, self.modifiers);
        let Some(pane) = self.workspace.pane_mut(target) else {
            return false;
        };
        let (hit, _) = pane.pointer_hit(position);
        // The margin, the chevrons and the gaps keep their own press.
        let (on_link, plain) = match hit {
            Hit::Cell {
                source,
                line,
                column,
                ..
            } => {
                let point = Point::new(line, alacritty_terminal::index::Column(column));
                (pane.link_at(source, point).is_some(), true)
            }
            Hit::Blank => (false, true),
            Hit::Margin { .. } | Hit::Chevron { .. } | Hit::Gap => (false, false),
        };
        if !plain || !selection::is_broadcast_press(modifiers, on_link) {
            return false;
        }
        let captured = selection::mouse_reserved(pane.mode(), modifiers) && pane.reports_at(&hit);
        self.drag = Some(Drag::BroadcastClick {
            pane: target,
            press: position,
            captured,
        });
        true
    }

    /// The pointer moved with a broadcast click pending.
    pub(super) fn broadcast_click_moved(&mut self) {
        let Some(Drag::BroadcastClick {
            pane,
            press,
            captured,
        }) = self.drag
        else {
            return;
        };
        let scale = self.window.scale_factor() as f32;
        let past =
            pane_bar::drag_started((press.x as f32, press.y as f32), self.mouse_pixel(), scale);
        if selection::broadcast_move(past, captured) != selection::BroadcastMove::Select {
            return;
        }
        // The press becomes the start of a rectangular selection, as it was before.
        self.drag = None;
        if self.workspace.active_pane_id() != Some(pane) {
            self.focus_pane(pane);
        }
        let (modifiers, lang) = (self.modifiers, self.lang);
        // The move that follows extends the selection to the pointer.
        if let Some(target) = self.workspace.pane_mut(pane)
            && target.left_press(press, modifiers, lang)
        {
            self.drag = Some(Drag::Selection {
                pane,
                auto_scroll_deadline: None,
            });
        }
    }

    /// The release of a broadcast click: toggles the pane unless the pointer left it.
    pub(super) fn broadcast_click_released(&mut self, pane: PaneId, press: PhysicalPosition<f64>) {
        let scale = self.window.scale_factor() as f32;
        let past =
            pane_bar::drag_started((press.x as f32, press.y as f32), self.mouse_pixel(), scale);
        if selection::broadcast_release_toggles(past) && self.workspace.toggle_broadcast_click(pane)
        {
            self.window.request_redraw();
        }
    }

    /// A press of `button` on `hit` in the bar of `pane`: never reported to the
    /// application (see [`pane_bar::press_action`]).
    pub(super) fn pane_bar_press(
        &mut self,
        button: mouse::Button,
        pane: PaneId,
        hit: PaneBarHit,
        event_loop: &ActiveEventLoop,
    ) {
        let clicks = if (button, hit) == (mouse::Button::Left, PaneBarHit::Title) {
            self.pane_bar_clicks.click(pane, Instant::now())
        } else {
            0
        };
        let ctrl = self.modifiers.control_key();
        match pane_bar::press_action(button, hit, clicks, ctrl) {
            Some(PaneBarAction::Broadcast) => {
                if self.workspace.toggle_broadcast_click(pane) {
                    self.window.request_redraw();
                }
            }
            Some(PaneBarAction::Close) => self.request_close(Target::Pane(pane), event_loop),
            Some(PaneBarAction::Menu) => self.open_pane_menu(pane, Hit::Blank),
            Some(PaneBarAction::Focus { zoom }) => {
                if self.workspace.active_pane_id() != Some(pane) {
                    self.focus_pane(pane);
                }
                if zoom && self.workspace.toggle_zoom() {
                    self.relayout();
                    self.update_pointer();
                }
                // The pane is dragged once the pointer moves past the threshold.
                self.drag = Some(Drag::Pane {
                    pane,
                    press: self.mouse_position,
                    started: false,
                    was_zoomed: false,
                    tab_hover: None,
                    target: None,
                });
            }
            None => {}
        }
    }

    /// Ends the gesture of `button`, then opens the link it was pressed on when the
    /// press and release make a click on it.
    pub(super) fn mouse_release(&mut self, button: mouse::Button) {
        let link_press = match button {
            mouse::Button::Left => self.link_press.take(),
            _ => None,
        };
        self.release_gesture(button);
        if let Some(press) = link_press {
            self.release_link(press);
        }
        self.update_hover(false);
        self.update_pointer();
    }

    pub(super) fn release_gesture(&mut self, button: mouse::Button) {
        if button == mouse::Button::Left
            && let Some(Drag::BroadcastClick { pane, press, .. }) = self.drag
        {
            self.drag = None;
            self.broadcast_click_released(pane, press);
            return;
        }
        if button == mouse::Button::Left && matches!(self.drag, Some(Drag::Pane { .. })) {
            self.drop_pane();
            return;
        }
        if button == mouse::Button::Left && matches!(self.drag, Some(Drag::Divider { .. })) {
            self.drag = None;
            self.update_pointer();
            return;
        }
        if button == mouse::Button::Left
            && let Some(Drag::Tab { index, grab_offset }) = self.drag
        {
            self.drag = None;
            let mut bar = self.tab_bar();
            let (x, y) = self.mouse_pixel();
            let to = bar.dragged(index, bar.along(x, y) - grab_offset);
            self.workspace.move_tab(index, to);
            self.window.request_redraw();
            self.update_pointer();
            return;
        }
        let (position, modifiers) = (self.mouse_position, self.modifiers);
        let Some(pane) = self.workspace.active_pane_mut() else {
            return;
        };
        if pane.app_mouse.owns(button) {
            let cell = pane.mouse_cell(position);
            let mode = pane.mode();
            if let Some(bytes) = pane.app_mouse.release(button, cell, modifiers, mode) {
                pane.terminal.write(bytes);
            }
        } else if button == mouse::Button::Left
            && let Some(Drag::Selection { pane: id, .. }) = self.drag.take()
            && let Some(pane) = self.workspace.pane_mut(id)
        {
            pane.left_release(position, modifiers);
        }
    }

    pub(super) fn cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        self.mouse_position = position;
        let menu_layout = self.menu_layout();
        if let (Some(Overlay::Menu(menu)), Some(layout)) = (&mut self.overlay, menu_layout) {
            menu.hover_at(&layout, position.x as f32, position.y as f32);
            self.window.request_redraw();
            self.update_pointer();
            return;
        }
        if std::mem::take(&mut self.tooltip_hidden) && self.tooltip_uri().is_some() {
            self.window.request_redraw();
        }
        self.update_hover(false);
        self.update_pointer();
        if matches!(self.drag, Some(Drag::Pane { .. })) {
            self.pane_drag_moved();
            return;
        }
        if matches!(self.drag, Some(Drag::Tab { .. })) {
            self.window.request_redraw();
            return;
        }
        if matches!(self.drag, Some(Drag::BroadcastClick { .. })) {
            self.broadcast_click_moved();
            if !matches!(self.drag, Some(Drag::Selection { .. })) {
                return;
            }
        }
        if let Some(Drag::Divider { node_path, axis }) = &self.drag {
            let along = match axis {
                Axis::Horizontal => position.x,
                Axis::Vertical => position.y,
            } as f32;
            let geometry = self.geometry();
            let node_path = node_path.clone();
            if self.workspace.set_divider(&node_path, along, geometry) {
                self.relayout();
            }
            return;
        }
        if let Some(Drag::Selection { pane: id, .. }) = self.drag
            && let Some(pane) = self.workspace.pane_mut(id)
            && !pane.app_mouse.active()
        {
            pane.update_selection(position);
            return;
        }
        let modifiers = self.modifiers;
        let over_active = self.pane_under_mouse() == self.workspace.active_pane_id();
        let Some(pane) = self.workspace.active_pane_mut() else {
            return;
        };
        // A gesture the application owns follows the pointer anywhere; a bare motion is
        // only reported over the application's rows of the active pane.
        if !pane.app_mouse.active()
            && !(over_active && pane.reports_at(&pane.pointer_hit(position).0))
        {
            return;
        }
        let cell = pane.mouse_cell(position);
        let mode = pane.mode();
        if let Some(bytes) = pane.app_mouse.motion(cell, modifiers, mode) {
            pane.terminal.write(bytes);
        }
    }

    pub(super) fn auto_scroll(&mut self, now: Instant) -> Option<Instant> {
        let position = self.mouse_position;
        let Some(Drag::Selection {
            pane: id,
            auto_scroll_deadline,
        }) = &mut self.drag
        else {
            return None;
        };
        let pane = self.workspace.pane_mut(*id)?;
        let delta = pane.auto_scroll_delta(position.y);
        if delta == 0 {
            *auto_scroll_deadline = None;
            return None;
        }
        match *auto_scroll_deadline {
            Some(deadline) if now < deadline => {}
            _ => {
                pane.scroll_timeline(delta as isize);
                pane.update_selection(position);
                *auto_scroll_deadline = Some(now + selection::AUTO_SCROLL_INTERVAL);
            }
        }
        *auto_scroll_deadline
    }

    pub(super) fn mouse_wheel(&mut self, delta: MouseScrollDelta) {
        // The menus fit in the window: "More..." pages long theme lists instead of a
        // scroll, and the wheel does nothing while a menu is open.
        if matches!(&self.overlay, Some(Overlay::Menu(_))) {
            return;
        }
        if let Some(Overlay::Palette(palette)) = &mut self.overlay {
            let rows = match delta {
                MouseScrollDelta::LineDelta(_, y) => y,
                MouseScrollDelta::PixelDelta(position) => {
                    position.y as f32 / self.renderer.ui_line_height(UiFont::Palette)
                }
            };
            // Wheel up shows the rows above.
            palette.scroll_by(-rows.round() as isize);
            self.window.request_redraw();
            return;
        }
        if self.overlay.is_some() {
            return;
        }
        let (position, modifiers) = (self.mouse_position, self.modifiers);
        if let Some(id) = self.pane_under_mouse()
            && let Some(pane) = self.workspace.pane_mut(id)
        {
            pane.mouse_wheel(delta, position, modifiers);
        }
        // Scrolling moves another cell under the mouse, and hides the link tooltip.
        if !self.tooltip_hidden && self.tooltip_uri().is_some() {
            self.window.request_redraw();
        }
        self.tooltip_hidden = true;
        self.update_hover(false);
        self.update_pointer();
    }
}
