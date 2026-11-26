//! Pointer shape, hovered links and tooltips of a session.

use super::*;

impl Session {
    /// Hand over a fold chevron or an active link, arrow while the application captures
    /// the mouse under the pointer, I-beam otherwise; the window is only updated when the shape changes.
    /// Over a divider, or while dragging one, the resize arrows of its axis. Arrow over
    /// a pane bar, hand over a close button of a tab or a pane bar or over the `+`
    /// button, which is then drawn hovered.
    pub(super) fn update_pointer(&mut self) {
        let button = self.button_under_mouse();
        if button != self.hovered_button {
            self.hovered_button = button;
            self.window.request_redraw();
        }
        if button.is_some() {
            self.set_pointer(winit::window::CursorIcon::Pointer);
            return;
        }
        if self.pane_dragging() {
            self.set_pointer(winit::window::CursorIcon::Grabbing);
            return;
        }
        let (x, y) = self.mouse_pixel();
        if self.overlay.is_some()
            || matches!(self.drag, Some(Drag::Tab { .. }))
            || !self.geometry().area.contains(x, y)
        {
            self.set_pointer(winit::window::CursorIcon::Default);
            return;
        }
        let layout = self.shown_layout();
        let divider = match &self.drag {
            Some(Drag::Divider { axis, .. }) => Some(*axis),
            _ => layout.divider_at(x, y).map(|divider| divider.axis),
        };
        let icon = if let Some(axis) = divider {
            mouse::divider_icon(axis)
        } else if self.pane_bar_at(&layout, x, y).is_some() {
            winit::window::CursorIcon::Default
        } else {
            let id = layout
                .pane_at(x, y)
                .or_else(|| self.workspace.active_pane_id());
            let Some(pane) = id.and_then(|id| self.workspace.pane(id)) else {
                return;
            };
            let (hit, over_chevron) = pane.pointer_hit(self.mouse_position);
            let mode = pane.mode();
            let captured = if mouse::reports_at(&hit, mode.contains(TermMode::ALT_SCREEN)) {
                mode
            } else {
                TermMode::empty()
            };
            let over_link = self
                .active_link()
                .is_some_and(|hovered| hovered.pane == pane.id);
            mouse::pointer_icon(captured, over_chevron || over_link)
        };
        self.set_pointer(icon);
    }

    /// The close button of a tab or a pane bar, or the `+` button, under the mouse; none
    /// while an overlay is open or a gesture is in progress.
    pub(super) fn button_under_mouse(&self) -> Option<chrome::ChromeButton> {
        if self.overlay.is_some() || self.drag.is_some() {
            return None;
        }
        let (x, y) = self.mouse_pixel();
        let layout = self.shown_layout();
        let metrics = self.ui_metrics(UiFont::Chrome);
        chrome::button_at(Some(&self.tab_bar()), &layout.panes, x, y, metrics)
    }

    /// Looks the link under the mouse up again when the grid cell under it changed, or
    /// always with `force` (the content may have changed). No link while an overlay is
    /// open or a gesture is in progress. Redraws when the link changed.
    pub(super) fn update_hover(&mut self, force: bool) {
        let cell = if self.overlay.is_some() || self.drag.is_some() {
            None
        } else {
            let position = self.mouse_position;
            self.pane_under_mouse().and_then(|id| {
                let (source, point) = self.workspace.pane(id)?.cell_under(position)?;
                Some((id, source, point))
            })
        };
        if !force && cell == self.hovered_cell {
            return;
        }
        self.hovered_cell = cell;
        let hovered = cell.and_then(|(id, source, point)| {
            let link = self.workspace.pane(id)?.link_at(source, point)?;
            Some(HoveredLink {
                pane: id,
                source,
                link,
            })
        });
        if hovered != self.hovered_link {
            self.hovered_link = hovered;
            self.window.request_redraw();
        }
    }

    /// The link under the mouse when it is Graphene's (see [`links::active`]): pointed
    /// at, underlined and opened on a click.
    pub(super) fn active_link(&self) -> Option<&HoveredLink> {
        let hovered = self.hovered_link.as_ref()?;
        let pane = self.workspace.pane(hovered.pane)?;
        links::active(pane.captures(hovered.source), self.modifiers).then_some(hovered)
    }

    /// Target shown in the link tooltip: the URI of the active OSC 8 link, unless the
    /// wheel hid the tooltip since the mouse last moved.
    pub(super) fn tooltip_uri(&self) -> Option<&str> {
        let hovered = self.active_link()?;
        (hovered.link.explicit && !self.tooltip_hidden).then_some(hovered.link.uri.as_str())
    }

    /// The link tooltip laid out near the mouse, `None` while no tooltip is shown.
    pub(super) fn tooltip_layout(&mut self) -> Option<TooltipLayout> {
        let uri = self.tooltip_uri()?.to_owned();
        let size = self.window.inner_size();
        let metrics = self.ui_metrics(UiFont::Chrome);
        let mouse = self.mouse_pixel();
        let renderer = &mut self.renderer;
        TooltipLayout::new(
            &uri,
            mouse,
            Rect::from_size(size.width, size.height),
            metrics,
            |text| renderer.ui_text_width(UiFont::Chrome, text),
        )
    }

    /// The mouse left the window: nothing is hovered any more.
    pub(super) fn cursor_left(&mut self) {
        self.hovered_cell = None;
        if self.hovered_link.take().is_some() {
            self.window.request_redraw();
        }
        self.update_pointer();
        if self.hovered_button.take().is_some() {
            self.window.request_redraw();
        }
    }

    /// End of a left press on a link: opens it when the click calls for it (see
    /// [`links::click_opens`]) and it may be opened (see [`links::may_open`]).
    pub(super) fn release_link(&mut self, press: LinkPress) {
        if self.pane_under_mouse() != Some(press.pane) {
            return;
        }
        let Some(pane) = self.workspace.pane(press.pane) else {
            return;
        };
        let Some(cell) = pane.cell_under(self.mouse_position) else {
            return;
        };
        let same = pane.link_at(cell.0, cell.1).as_ref() == Some(&press.link);
        if !links::click_opens(&press.press, &cell, same) {
            return;
        }
        if links::may_open(&press.link, opener::hostname().as_deref()) {
            opener::open(&press.link.uri);
        } else {
            log::warn!("not opening {}: a file on another host", press.link.uri);
        }
    }

    /// Applies the pointer shape `icon` when it changed.
    pub(super) fn set_pointer(&mut self, icon: winit::window::CursorIcon) {
        if self.pointer != Some(icon) {
            self.window.set_cursor(icon);
            self.pointer = Some(icon);
        }
    }
}
