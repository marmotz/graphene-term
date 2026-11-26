//! Drawing a session window.

use std::ops::RangeInclusive;

use super::*;
use crate::renderer::scene::gpu_color;
use crate::view::ViewRow;

/// What the frame of a pane is captured from, besides its terminal: the frame drawn
/// last is drawn again while the key is the same.
#[derive(PartialEq)]
pub(super) struct FrameKey {
    /// Revision of the pane in the workspace: its terminal, timeline and selection.
    revision: u64,
    rows: Vec<ViewRow>,
    palette: Palette,
    focused: bool,
    cursor_visible: bool,
    dimmed: bool,
    lang: Lang,
    banner: Option<FrameBanner>,
    link: Option<(RowSource, RangeInclusive<Point>)>,
    /// Progress of the pane drawn on the card of its running block, with the bar's
    /// thickness: an indeterminate one changes at each frame of its animation.
    progress: Option<(ProgressView, f32)>,
}

/// Longest wait for the window manager to maximize a window shown maximized, before
/// its first frame is drawn anyway.
const MAXIMIZE_WAIT: Duration = Duration::from_millis(150);

/// Holds back the first frame of a window shown maximized while it keeps its creation
/// size: on X11 the window manager maximizes it just after mapping it, and a frame drawn
/// before would fill only a corner of the maximized window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct MaximizeHold {
    created: PhysicalSize<u32>,
    until: Instant,
}

impl MaximizeHold {
    /// Holds a window created with `created` and shown at `shown`.
    pub(super) fn new(created: PhysicalSize<u32>, shown: Instant) -> Self {
        Self {
            created,
            until: shown + MAXIMIZE_WAIT,
        }
    }

    /// When to draw the first frame of a window of `size`, `None` to draw it now.
    pub(super) fn wait(&self, size: PhysicalSize<u32>, now: Instant) -> Option<Instant> {
        (size == self.created && now < self.until).then_some(self.until)
    }
}

impl Session {
    /// Draws every pane of the shown tab, inactive ones dimmed when it shows several,
    /// then the pane title bars, the dividers, the active pane border and the tab bar, then the banner, then
    /// the open dialog or command palette.
    pub(super) fn redraw(&mut self) {
        let start = Instant::now();
        if let Some(due) = self.renderer.frame_due(start) {
            self.redraw_at = Some(due);
            return;
        }
        if let Some(hold) = self.maximize_hold {
            match hold.wait(self.window.inner_size(), start) {
                Some(until) => {
                    self.redraw_at = Some(until);
                    return;
                }
                None => self.maximize_hold = None,
            }
        }
        self.redraw_at = None;
        self.draw();
        self.redraw_stats.redraw_done(start);
    }

    /// Requests the redraw put off until the renderer takes a new frame, once due.
    /// Returns when it is due, `None` when no redraw is put off.
    pub(super) fn deferred_redraw(&mut self, now: Instant) -> Option<Instant> {
        let due = self.redraw_at?;
        if due > now {
            return Some(due);
        }
        self.redraw_at = None;
        self.window.request_redraw();
        None
    }

    /// Body of [`Self::redraw`].
    fn draw(&mut self) {
        self.sync_menu_theme();
        let dialog = self.dialog_layout();
        let palette = self.palette_layout();
        let rename = self.rename_tab_layout();
        let menu = self.menu_layout();
        let art = self.art_layout();
        let tooltip = self.tooltip_layout();
        let overlay = match (&dialog, &palette, &rename, &menu, &art) {
            (Some(dialog), ..) => Some(chrome::scene::Overlay::Dialog(dialog)),
            (_, Some(palette), ..) => Some(chrome::scene::Overlay::Palette(palette)),
            (_, _, Some(rename), ..) => Some(chrome::scene::Overlay::RenameTab(rename)),
            (_, _, _, Some(menu), _) => Some(chrome::scene::Overlay::Menu(menu)),
            (.., Some(art)) => Some(chrome::scene::Overlay::Art(art)),
            _ => None,
        };
        let mut banner = self.shown_banner().map(|shown| FrameBanner {
            layout: self.banner_layout(&shown),
            background: self.palette.ui.banner_background,
            text: self.palette.ui.banner_text,
        });
        let layout = self.shown_layout();
        let active = self.workspace.active_pane_id();
        let several = layout.panes.len() > 1;
        let link = self
            .active_link()
            .map(|hovered| (hovered.pane, hovered.source, hovered.link.range.clone()));
        let now = Instant::now();
        let progress_thickness = progress::bar_thickness(self.ui_metrics(UiFont::Chrome).scale);
        let shown: Vec<PaneId> = layout.panes.iter().map(|&(id, _)| id).collect();
        self.frames.retain(|id| shown.contains(&id));
        // A pane that changed through a shared reference, not seen by its revision yet.
        for &id in &shown {
            if self.workspace.pane(id).is_some_and(Pane::redraw_pending) {
                self.workspace.touch(id);
            }
        }
        for &id in &shown {
            let Some(pane) = self.workspace.pane(id) else {
                continue;
            };
            let is_active = Some(id) == active;
            let term = pane.terminal.term.lock();
            let mut timeline = pane::lock(&pane.terminal.timeline);
            let rows = pane::shown_rows(&pane.anchor, &term, &mut timeline);
            let focused = self.focused && is_active;
            let cursor_visible = !is_active || self.cursor_blink.visible();
            let dimmed = several && !is_active;
            let banner = if is_active { banner.take() } else { None };
            let link = link
                .as_ref()
                .filter(|(pane, ..)| *pane == id)
                .map(|(_, source, range)| (*source, range.clone()));
            let progress = self
                .workspace
                .pane_progress(id)
                .map(|progress| (self.progress_view(progress, now), progress_thickness));
            // A running fold animation changes the frame at each redraw.
            let animated = pane.fold_animations.borrow().deadline(now).is_some();
            let key = (!animated).then(|| FrameKey {
                revision: self.workspace.revision(id),
                rows: rows.clone(),
                palette: self.palette.clone(),
                focused,
                cursor_visible,
                dimmed,
                lang: self.lang,
                banner: banner.clone(),
                link: link.clone(),
                progress,
            });
            let palette = &self.palette;
            let lang = self.lang;
            self.frames.update(id, key, || {
                let mut frame = Frame::capture(
                    &*term,
                    &timeline,
                    &rows,
                    palette,
                    focused,
                    cursor_visible,
                    pane.selected_block,
                );
                frame.attach_images(&graphics::lock(&pane.terminal.images));
                if let Some((view, thickness)) = progress {
                    frame.show_progress(view, thickness);
                }
                pane.fold_animations
                    .borrow_mut()
                    .apply(&mut frame.cards, &rows, now);
                if let Some((source, range)) = &link {
                    frame.underline_link(&rows, *source, range);
                }
                frame.lang = lang;
                if dimmed {
                    frame = frame.dimmed(INACTIVE_PANE_DIM);
                }
                frame.banner = banner;
                frame
            });
        }
        let (base, scale) = (self.font_size, self.window.scale_factor() as f32);
        let panes: Vec<_> = shown
            .iter()
            .filter_map(|&id| {
                let frame = self.frames.get(id)?;
                let pane = self.workspace.pane(id)?;
                Some(PaneFrame {
                    frame,
                    layout: &pane.layout,
                    font_size: refresh::grid_size(base, pane.font_offset, scale),
                })
            })
            .collect();
        // A frame without its panes waits for them, unless it shows the settings tab.
        if panes.is_empty() && !self.settings_shown() {
            return;
        }
        let mut bar = self.tab_bar();
        if let Some(Drag::Tab { index, grab_offset }) = &self.drag {
            let (x, y) = self.mouse_pixel();
            bar.dragged(*index, bar.along(x, y) - grab_offset);
        }
        let mut pane_bars = self.pane_bars(&layout, active);
        match self.hovered_button {
            Some(chrome::ChromeButton::Settings) => bar.settings_hovered = true,
            Some(chrome::ChromeButton::TabClose(index)) => {
                if let Some(tab) = bar.tabs.get_mut(index) {
                    tab.close_hovered = true;
                }
            }
            Some(chrome::ChromeButton::PaneClose(id)) => {
                if let Some(pane_bar) = pane_bars.iter_mut().find(|b| b.pane == id) {
                    pane_bar.close_hovered = true;
                }
            }
            Some(chrome::ChromeButton::NewTab) => bar.new_tab_hovered = true,
            None => {}
        }
        let highlight = match &self.drag {
            Some(Drag::Pane {
                started: true,
                target: Some(target),
                ..
            }) => drop::highlight(*target, &layout, Some(&bar)),
            _ => None,
        };
        let mut ui = chrome::scene::build(
            &layout,
            active,
            &pane_bars,
            Some(&bar),
            overlay,
            &self.palette.ui,
            self.renderer.is_srgb(),
        );
        if let Some(honeycomb) = &self.honeycomb {
            let color = gpu_color(self.palette.cursor, self.renderer.is_srgb());
            let scale = self.window.scale_factor() as f32;
            ui.rects.extend(honeycomb.rects(now, scale, color));
        }
        if let Some(highlight) = highlight {
            chrome::scene::drop_highlight(
                highlight,
                &self.palette.ui,
                self.renderer.is_srgb(),
                &mut ui,
            );
        }
        if let Some(tooltip) = &tooltip {
            chrome::scene::tooltip(tooltip, &self.palette.ui, self.renderer.is_srgb(), &mut ui);
        }
        let area = self.settings_area();
        let gpu = self.renderer.gpu().clone();
        let rendered = match self.settings.as_mut() {
            Some(settings) => {
                settings.prepare(&gpu, area);
                // The size of the drawn texture, not of the window: right after a resize
                // the window is already at its new size while the surface is not yet.
                let mut paint = |encoder: &mut wgpu::CommandEncoder,
                                 view: &wgpu::TextureView,
                                 size: (u32, u32)| {
                    settings.paint(&gpu, encoder, view, size);
                };
                self.renderer.render_with(&panes, &ui, Some(&mut paint))
            }
            None => self.renderer.render(&panes, &ui),
        };
        match rendered {
            Ok(true) if self.first_frame.frame_drawn() => {
                perf::mark("first frame drawn after PTY output");
            }
            Ok(_) => {}
            Err(error) => log::error!("render failed: {error:#}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREATED: PhysicalSize<u32> = PhysicalSize::new(960, 600);

    #[test]
    fn maximize_hold_waits_while_the_window_keeps_its_creation_size() {
        let shown = Instant::now();
        let hold = MaximizeHold::new(CREATED, shown);
        assert_eq!(hold.wait(CREATED, shown), Some(shown + MAXIMIZE_WAIT));
    }

    #[test]
    fn maximize_hold_draws_once_the_window_is_maximized() {
        let shown = Instant::now();
        let hold = MaximizeHold::new(CREATED, shown);
        assert_eq!(hold.wait(PhysicalSize::new(2560, 1370), shown), None);
    }

    #[test]
    fn maximize_hold_draws_anyway_after_its_longest_wait() {
        let shown = Instant::now();
        let hold = MaximizeHold::new(CREATED, shown);
        assert_eq!(hold.wait(CREATED, shown + MAXIMIZE_WAIT), None);
    }
}
