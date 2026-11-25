//! Geometry of a session window: banner, tab bar, pane bars and what lies under the mouse.

use super::*;

impl Session {
    /// Banner shown at the top of the window: the configuration errors first, then the
    /// update notice.
    pub(super) fn shown_banner(&self) -> Option<Shown> {
        Shown::pick(self.config_banner.as_ref(), &self.update, self.lang)
    }

    /// Places `shown` in pixels.
    pub(super) fn banner_layout(&self, shown: &Shown) -> BannerLayout {
        let width = self.window.inner_size().width as f32;
        BannerLayout::new(shown.banner(), self.renderer.cell_size(), width)
    }

    /// The window below the banner strip: where the tab bar and the panes are laid out.
    pub(super) fn chrome_area(&self) -> Rect {
        let size = self.window.inner_size();
        let height = match self.shown_banner() {
            Some(_) => BannerLayout::height(self.renderer.cell_size()),
            None => 0.0,
        };
        banner::below_banner(Rect::from_size(size.width, size.height), height)
    }

    /// Handles a left press on the banner, before any selection. Returns whether the
    /// banner took it.
    pub(super) fn banner_press(&mut self) -> bool {
        let Some(shown) = self.shown_banner() else {
            return false;
        };
        let (x, y) = (self.mouse_position.x as f32, self.mouse_position.y as f32);
        let Some(hit) = self.banner_layout(&shown).hit(x, y) else {
            return false;
        };
        match (shown, hit) {
            // The configuration error banner has no action: any click dismisses it.
            (Shown::Config(_), _) => self.config_banner = None,
            (Shown::Update(_), BannerHit::Action(index)) => match self.update.press(index) {
                Some(UpdateEffect::Open(url)) => opener::open(&url),
                Some(UpdateEffect::Ignore(tag)) => self.ignored_tag = Some(tag),
                None => {}
            },
            (Shown::Update(_), BannerHit::Body) => {}
        }
        // The banner may be gone: the panes take its strip back.
        self.relayout();
        true
    }

    /// Sizes of the interface text in `font`.
    pub(super) fn ui_metrics(&self, font: UiFont) -> UiMetrics {
        UiMetrics {
            line_height: self.renderer.ui_line_height(font),
            scale: self.window.scale_factor() as f32,
        }
    }

    /// Whether a pane is being dragged, past its threshold.
    pub(super) fn pane_dragging(&self) -> bool {
        matches!(self.drag, Some(Drag::Pane { started: true, .. }))
    }

    /// Size of the tab bar across its edge: it is always shown, holding the settings
    /// tab.
    pub(super) fn tab_bar_thickness(&self) -> f32 {
        tab_bar::thickness(
            self.tab_position,
            self.window.inner_size().width as f32,
            self.ui_metrics(UiFont::Chrome),
        )
    }

    /// The tab bar, the settings tab first.
    pub(super) fn tab_bar(&self) -> TabBarLayout {
        let count = self.workspace.tabs().len();
        let titles: Vec<String> = (0..count)
            .map(|index| {
                self.workspace
                    .tab_title(index)
                    .unwrap_or_else(|| special_days::titled(self.today.as_ref(), DEFAULT_TITLE))
            })
            .collect();
        let badges: Vec<u32> = self
            .workspace
            .tabs()
            .iter()
            .map(|tab| {
                tab.tree
                    .leaves()
                    .into_iter()
                    .filter_map(|id| self.workspace.pane(id))
                    .map(Pane::unseen)
                    .fold(0, u32::saturating_add)
            })
            .collect();
        let mut bar = TabBarLayout::new(
            self.chrome_area(),
            self.tab_position,
            &titles,
            self.workspace.active_index(),
            self.ui_metrics(UiFont::Chrome),
        );
        bar.set_badges(&badges);
        let now = Instant::now();
        let progress: Vec<_> = (0..count)
            .map(|index| {
                self.workspace
                    .tab_progress(index)
                    .map(|progress| self.progress_view(progress, now))
            })
            .collect();
        bar.set_progress(&progress, self.progress_colors());
        bar.set_settings_active(self.settings_shown());
        bar
    }

    /// Colors of the progress bars of the interface, from the theme.
    pub(super) fn progress_colors(&self) -> ProgressColors {
        ProgressColors::resolve(&self.palette, &Colors::default())
    }

    /// Area the tabs divide between their panes, and the sizes they are laid out with.
    pub(super) fn geometry(&self) -> Geometry {
        Geometry {
            area: self
                .tab_position
                .content_area(self.chrome_area(), self.tab_bar_thickness()),
            cell: self.renderer.cell_size(),
            divider: DIVIDER_THICKNESS,
            title_bar: pane_bar::height(self.ui_metrics(UiFont::Chrome)),
        }
    }

    /// Title bars of the panes of `layout`, the `active` one highlighted.
    pub(super) fn pane_bars(&self, layout: &Layout, active: Option<PaneId>) -> Vec<PaneBarLayout> {
        let metrics = self.ui_metrics(UiFont::Chrome);
        let colors = self.progress_colors();
        let now = Instant::now();
        layout
            .panes
            .iter()
            .map(|&(id, rect)| {
                let title = self
                    .workspace
                    .pane_title(id)
                    .unwrap_or_else(|| DEFAULT_TITLE.to_owned());
                let unseen = self.workspace.pane(id).map_or(0, Pane::unseen);
                let progress = self
                    .workspace
                    .pane_progress(id)
                    .map(|progress| self.progress_view(progress, now));
                let mut bar = PaneBarLayout::new(id, rect, title, Some(id) == active, metrics)
                    .with_badge(unseen, metrics)
                    .with_progress(progress, colors, metrics);
                if self.workspace.is_broadcast(id) {
                    bar.broadcast = Some(
                        self.workspace
                            .broadcast_count(self.workspace.active_index()),
                    );
                }
                bar
            })
            .collect()
    }

    /// Layout of the shown tab: no pane while the settings tab is shown.
    pub(super) fn shown_layout(&self) -> Layout {
        if self.settings_shown() {
            return Layout::default();
        }
        self.workspace
            .layout(self.workspace.active_index(), self.geometry())
    }

    /// The mouse position as a pixel, in physical pixels.
    pub(super) fn mouse_pixel(&self) -> (f32, f32) {
        (self.mouse_position.x as f32, self.mouse_position.y as f32)
    }

    /// The pane of the shown tab whose content is under the mouse: `None` over its
    /// title bar, which is never the application's.
    pub(super) fn pane_under_mouse(&self) -> Option<PaneId> {
        let (x, y) = self.mouse_pixel();
        let layout = self.shown_layout();
        if self.pane_bar_at(&layout, x, y).is_some() {
            return None;
        }
        layout.pane_at(x, y)
    }

    /// The pane bar of `layout` under the pixel `(x, y)`: its pane and what the pixel
    /// is over.
    pub(super) fn pane_bar_at(
        &self,
        layout: &Layout,
        x: f32,
        y: f32,
    ) -> Option<(PaneId, PaneBarHit)> {
        pane_bar::hit_at(&layout.panes, x, y, self.ui_metrics(UiFont::Chrome))
    }
}
