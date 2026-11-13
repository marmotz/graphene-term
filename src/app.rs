//! Window and event handling.

use std::cell::Cell;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Point, Side};
use alacritty_terminal::term::{self, TermMode};
use arboard::{GetExtLinux, LinuxClipboardKind, SetExtLinux};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::banner::{Banner, BannerHit, BannerLayout, Shown, UpdateBanner, UpdateEffect};
use crate::block_actions::{self, Direction, Part};
use crate::config::{self, Config, MAX_FONT_SIZE, MIN_FONT_SIZE};
use crate::frame::{self, Frame, FrameBanner};
use crate::i18n::{self, Lang, Msg};
use crate::input::{self, Action};
use crate::layout::{GridLayout, Hit};
use crate::mouse;
use crate::opener;
use crate::palette::Palette;
use crate::perf;
use crate::renderer::{Glyphs, Renderer};
use crate::selection::{self, ClickTracker, SelectionSnapshot};
use crate::terminal::{EventProxy, Terminal, UserEvent};
use crate::theme;
use crate::timeline::{BlockId, Timeline};
use crate::update::checker::Checker;
use crate::view::{self, Anchor, RowSource, ScrollTarget, ViewRow};

const DEFAULT_TITLE: &str = "Graphene Term";

struct Session {
    window: Arc<Window>,
    renderer: Renderer,
    terminal: Terminal,
    layout: GridLayout,
    palette: Palette,
    clipboard: Option<arboard::Clipboard>,
    focused: bool,
    modifiers: ModifiersState,
    font_size: f32,
    /// Minimum padding around the grid, in logical pixels.
    padding: f32,
    /// Fractional lines accumulated from touchpad scrolling.
    scroll_accumulator: f64,
    /// Last known cursor position, in physical pixels.
    mouse_position: PhysicalPosition<f64>,
    /// A selection drag is in progress.
    left_pressed: bool,
    click_tracker: ClickTracker,
    /// Gesture owned by the application (mouse reporting).
    app_mouse: mouse::Reporter,
    /// Pointer shape last applied to the window; `None` before the first update.
    pointer: Option<winit::window::CursorIcon>,
    /// When the next auto-scroll step is due during a drag past the grid edges.
    auto_scroll_deadline: Option<Instant>,
    /// Finished selection of the live grid kept while its text is unchanged.
    selection_snapshot: Option<SelectionSnapshot>,
    /// Grid holding the text selection: the live one or a frozen block's.
    selection_source: Option<RowSource>,
    /// Block selected whole by a margin click or a block navigation, shown as a
    /// highlighted card instead of highlighted cells.
    selected_block: Option<BlockId>,
    /// What the view looked like at the last frame, to keep it still while output
    /// arrives.
    anchor: Cell<Anchor>,
    /// Configuration error banner; `None` once dismissed or without error.
    config_banner: Option<Banner>,
    /// Update banner: the newer release reported by the check thread.
    update: UpdateBanner,
    /// Tag the user just chose to ignore, forwarded to the update check thread.
    ignored_tag: Option<String>,
    lang: Lang,
    /// Logs the first frame drawn after the shell's first output (`graphene::perf`).
    first_frame: perf::FirstFrame,
}

impl Session {
    /// Banner shown at the top of the window: the configuration errors first, then the
    /// update notice.
    fn shown_banner(&self) -> Option<Shown> {
        Shown::pick(self.config_banner.as_ref(), &self.update, self.lang)
    }

    /// Places `shown` in pixels.
    fn banner_layout(&self, shown: &Shown) -> BannerLayout {
        let width = self.window.inner_size().width as f32;
        BannerLayout::new(shown.banner(), self.renderer.cell_size(), width)
    }

    /// Handles a left press on the banner, before any selection. Returns whether the
    /// banner took it.
    fn banner_press(&mut self) -> bool {
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
        self.window.request_redraw();
        true
    }

    fn compute_layout(&self) -> GridLayout {
        let size = self.window.inner_size();
        let scale = self.window.scale_factor() as f32;
        GridLayout::new(
            size.width,
            size.height,
            self.renderer.cell_size(),
            self.padding * scale,
        )
    }

    /// Recomputes the grid and resizes the PTY when the cell count changed.
    fn relayout(&mut self) {
        let layout = self.compute_layout();
        let changed = (layout.columns, layout.lines) != (self.layout.columns, self.layout.lines);
        self.layout = layout;
        if changed {
            self.terminal.resize(layout.window_size());
        }
        self.window.request_redraw();
    }

    fn apply_font_size(&mut self, font_size: f32) {
        self.font_size = font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        let scale = self.window.scale_factor() as f32;
        self.renderer.set_font_size(self.font_size * scale);
        self.relayout();
    }

    fn mode(&self) -> TermMode {
        *self.terminal.term.lock().mode()
    }

    /// Hand over a fold chevron, arrow while the application captures the mouse under
    /// the pointer, I-beam otherwise; the window is only updated when the shape changes.
    fn update_pointer(&mut self) {
        let (hit, over_chevron) = self.pointer_hit();
        let mode = self.mode();
        let captured = if mouse::reports_at(&hit, mode.contains(TermMode::ALT_SCREEN)) {
            mode
        } else {
            TermMode::empty()
        };
        let icon = mouse::pointer_icon(captured, over_chevron);
        if self.pointer != Some(icon) {
            self.window.set_cursor(icon);
            self.pointer = Some(icon);
        }
    }

    /// What lies under the mouse cursor, and whether it is the fold chevron of a card
    /// that has one.
    fn pointer_hit(&self) -> (Hit, bool) {
        let term = self.terminal.term.lock();
        let mut timeline = lock(&self.terminal.timeline);
        let rows = shown_rows(&self.anchor, &term, &mut timeline);
        let hit = self.layout.hit(
            self.mouse_position.x as f32,
            self.mouse_position.y as f32,
            &rows,
        );
        let over_chevron = match hit {
            Hit::Chevron { row } => frame::has_chevron(&*term, &timeline, &rows, row),
            _ => false,
        };
        (hit, over_chevron)
    }

    /// Whether a mouse event over `hit` goes to the application when it captures the
    /// mouse (see [`mouse::reports_at`]).
    fn reports_at(&self, hit: &Hit) -> bool {
        mouse::reports_at(hit, self.mode().contains(TermMode::ALT_SCREEN))
    }

    /// Scrolls the timeline by `rows` rows (positive shows older blocks), stopping at
    /// its ends.
    fn scroll_timeline(&self, rows: isize) {
        self.scroll(ScrollTarget::Timeline, rows);
    }

    /// Scrolls `target` by `rows` rows (positive shows older rows), stopping at its ends:
    /// a block's output stops there, the timeline does not take over.
    fn scroll(&self, target: ScrollTarget, rows: isize) {
        let term = self.terminal.term.lock();
        let mut timeline = lock(&self.terminal.timeline);
        let lines = term.screen_lines();
        match target {
            ScrollTarget::Block(id) => {
                view::scroll_block(&mut timeline, &term, id, lines, rows);
            }
            ScrollTarget::Timeline => {
                view::scroll_timeline(&mut timeline, &term, lines, rows * 2);
            }
        }
        self.window.request_redraw();
    }

    /// `Shift+PageUp/PageDown`: scrolls the selected block by a page when it can scroll,
    /// otherwise the timeline. `pages` is positive toward older rows.
    fn scroll_page(&self, pages: isize) {
        let target = {
            let term = self.terminal.term.lock();
            let timeline = lock(&self.terminal.timeline);
            let lines = term.screen_lines();
            view::page_target(self.selected_block, |id| {
                view::can_scroll(&timeline, &*term, id, lines)
            })
        };
        self.scroll(target, pages * self.page_rows());
    }

    /// Scrolls the timeline to its top.
    fn scroll_to_top(&self) {
        let term = self.terminal.term.lock();
        let mut timeline = lock(&self.terminal.timeline);
        let lines = term.screen_lines();
        timeline.scroll = view::max_timeline_scroll(&timeline, &term, lines);
        self.window.request_redraw();
    }

    /// Brings the view back to the bottom: the timeline and the current block at rest.
    fn scroll_to_bottom(&self) {
        let mut timeline = lock(&self.terminal.timeline);
        timeline.scroll = 0;
        timeline.current_mut().scroll = 0;
        self.window.request_redraw();
    }

    /// Rows scrolled by a page: the view lines.
    fn page_rows(&self) -> isize {
        self.layout.lines as isize
    }

    /// Changes the folds with `change`, given the selected block, then redraws.
    fn change_folds(&mut self, change: impl FnOnce(&mut Timeline, Option<BlockId>)) {
        change(&mut lock(&self.terminal.timeline), self.selected_block);
        self.window.request_redraw();
    }

    /// Folds or unfolds the target block.
    fn toggle_fold(&mut self) {
        self.change_folds(|timeline, selected| {
            if let Some(id) = block_actions::target(timeline, selected) {
                timeline.toggle_fold(id);
            }
        });
    }

    fn paste(&mut self) {
        let Some(text) = self.clipboard.as_mut().and_then(|c| c.get_text().ok()) else {
            return;
        };
        self.send_paste(&text);
    }

    /// Sends a paste to the shell. Like a keystroke, it clears the selection.
    fn send_paste(&mut self, text: &str) {
        self.clear_selection();
        let bytes = input::paste_bytes(text, self.mode());
        self.scroll_to_bottom();
        self.terminal.write(bytes);
    }

    /// Cleaned text of the selected block, or of the text selection.
    fn selected_text(&self) -> Option<String> {
        let term = self.terminal.term.lock();
        let timeline = lock(&self.terminal.timeline);
        if let Some(id) = self.selected_block {
            return block_actions::block_text(&timeline, id);
        }
        match self.selection_source? {
            RowSource::Live => selection::selected_text(&*term),
            source => selection::selected_text(view::frozen_term(&timeline, source)?),
        }
    }

    /// Writes the cleaned selection to the clipboard. Without selection, the shortcut is
    /// still swallowed so it never sends `^C`.
    fn copy(&mut self) {
        let Some(text) = self.selected_text() else {
            return;
        };
        if let Some(Err(error)) = self.clipboard.as_mut().map(|c| c.set_text(text)) {
            log::warn!("copy failed: {error}");
        }
    }

    /// Copies one part of the target block to the clipboard. An empty part leaves the
    /// clipboard untouched.
    fn copy_block_part(&mut self, part: Part) {
        let text = {
            let timeline = lock(&self.terminal.timeline);
            block_actions::target(&timeline, self.selected_block)
                .and_then(|id| block_actions::text(&timeline, id, part))
        };
        let Some(text) = text else {
            return;
        };
        if let Some(Err(error)) = self.clipboard.as_mut().map(|c| c.set_text(text)) {
            log::warn!("copy failed: {error}");
        }
    }

    /// Selects the neighbour block in `direction` and scrolls it into view.
    fn navigate_block(&mut self, direction: Direction) {
        let selected = {
            let term = self.terminal.term.lock();
            let mut timeline = lock(&self.terminal.timeline);
            block_actions::neighbour(&timeline, self.selected_block, direction).map(|id| {
                view::reveal(&mut timeline, &term, term.screen_lines(), id);
                (id, block_actions::block_text(&timeline, id))
            })
        };
        if let Some((id, text)) = selected {
            self.select_block(id, text);
        }
    }

    /// Left press in the margin on view row `row`: selects the block of a finished block
    /// row, ignored elsewhere.
    fn margin_press(&mut self, row: usize) {
        let selected = {
            let term = self.terminal.term.lock();
            let mut timeline = lock(&self.terminal.timeline);
            let rows = shown_rows(&self.anchor, &term, &mut timeline);
            let row = row.min(rows.len().saturating_sub(1));
            rows.get(row)
                .and_then(|row| row.block)
                .filter(|id| timeline.block(*id).is_some())
                .map(|id| (id, block_actions::block_text(&timeline, id)))
        };
        if let Some((id, text)) = selected {
            self.select_block(id, text);
        }
    }

    /// Selects block `id` whole, shown as a highlighted card instead of highlighted
    /// cells, then writes `text` to the primary selection, as a mouse release does.
    fn select_block(&mut self, id: BlockId, text: Option<String>) {
        self.clear_selection();
        self.selected_block = Some(id);
        self.window.request_redraw();
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            self.set_primary(text);
        }
    }

    /// Left press on the chevron disc of the card starting on view row `row`: toggles
    /// the fold of its block when it has a chevron. Returns whether it had one.
    fn chevron_press(&mut self, row: usize) -> bool {
        let toggled = {
            let term = self.terminal.term.lock();
            let mut timeline = lock(&self.terminal.timeline);
            let rows = shown_rows(&self.anchor, &term, &mut timeline);
            let Some(id) = rows.get(row).and_then(|row| row.block) else {
                return false;
            };
            let chevron = frame::has_chevron(&*term, &timeline, &rows, row);
            if chevron {
                timeline.toggle_fold(id);
            }
            chevron
        };
        if toggled {
            self.window.request_redraw();
        }
        toggled
    }

    /// View rows shown for `term` and `timeline`, the locked terminal and timeline.
    fn view_rows(&self, term: &term::Term<EventProxy>, timeline: &mut Timeline) -> Vec<ViewRow> {
        shown_rows(&self.anchor, term, timeline)
    }

    /// Clears the text selection and the selected block.
    fn clear_selection(&mut self) {
        self.selection_snapshot = None;
        let had_block = self.selected_block.take().is_some();
        let source = self.selection_source.take();
        let mut term = self.terminal.term.lock();
        let mut timeline = lock(&self.terminal.timeline);
        let had_text =
            source.is_some_and(|source| clear_source_selection(&mut term, &mut timeline, source));
        if had_block || had_text {
            self.window.request_redraw();
        }
    }

    /// After new PTY output: restores a live selection `Term` erased while its text was
    /// redrawn unchanged, and clears it when the selected text really changed. Frozen
    /// blocks never change.
    fn validate_selection(&mut self) {
        if self.selection_source != Some(RowSource::Live) {
            return;
        }
        let Some(snapshot) = self.selection_snapshot.as_mut() else {
            return;
        };
        if !snapshot.validate(&mut self.terminal.term.lock()) {
            self.selection_snapshot = None;
            self.selection_source = None;
        }
    }

    /// Source, grid point and cell side under the mouse cursor. `term` and `timeline`
    /// are the locked terminal and timeline.
    fn mouse_target(
        &self,
        term: &term::Term<EventProxy>,
        timeline: &mut Timeline,
    ) -> Option<(RowSource, Point, Side)> {
        let rows = self.view_rows(term, timeline);
        let (column, row, side) = self.layout.point_at(
            self.mouse_position.x as f32,
            self.mouse_position.y as f32,
            &rows,
        );
        view::point(&rows, row, column).map(|(source, point)| (source, point, side))
    }

    /// Viewport cell (column, line) under the mouse cursor, clamped to the grid, as
    /// reported to the application: the live grid's line under the cursor (its viewport
    /// is never scrolled), line 0 over a frozen row.
    fn mouse_cell(&self) -> (usize, usize) {
        let term = self.terminal.term.lock();
        let mut timeline = lock(&self.terminal.timeline);
        let Some((source, point, _)) = self.mouse_target(&term, &mut timeline) else {
            return (0, 0);
        };
        let last = term.screen_lines() as i32 - 1;
        let line = match source {
            RowSource::Live => point.line.0.clamp(0, last),
            _ => 0,
        };
        (point.column.0, line as usize)
    }

    fn mouse_input(&mut self, state: ElementState, button: MouseButton) {
        let Some(button) = mouse::button_from(button) else {
            return;
        };
        match state {
            ElementState::Pressed => self.mouse_press(button),
            ElementState::Released => self.mouse_release(button),
        }
    }

    fn mouse_press(&mut self, button: mouse::Button) {
        if button == mouse::Button::Left && self.banner_press() {
            return;
        }
        let (hit, _) = self.pointer_hit();
        // The margin, the chevrons and the gaps are outside the application's grid:
        // always Graphene's.
        if button == mouse::Button::Left {
            match hit {
                Hit::Margin { row } => {
                    self.margin_press(row);
                    return;
                }
                Hit::Chevron { row } if self.chevron_press(row) => return,
                Hit::Gap => return,
                Hit::Chevron { .. } | Hit::Cell { .. } | Hit::Blank => {}
            }
        }
        let mode = self.mode();
        // Only the live rows are the application's: elsewhere the mouse is Graphene's.
        if selection::mouse_reserved(mode, self.modifiers) && self.reports_at(&hit) {
            // The gesture belongs to the application; a click clears the selection.
            self.clear_selection();
            let cell = self.mouse_cell();
            if let Some(bytes) = self.app_mouse.press(button, cell, self.modifiers, mode) {
                self.terminal.write(bytes);
            }
            return;
        }
        match button {
            mouse::Button::Left => self.left_press(),
            mouse::Button::Middle => self.paste_primary(),
            mouse::Button::Right => {}
        }
    }

    fn mouse_release(&mut self, button: mouse::Button) {
        if self.app_mouse.owns(button) {
            let cell = self.mouse_cell();
            let mode = self.mode();
            if let Some(bytes) = self.app_mouse.release(button, cell, self.modifiers, mode) {
                self.terminal.write(bytes);
            }
        } else if button == mouse::Button::Left {
            self.left_release();
        }
    }

    /// Left press on the summary label of a folded block: unfolds it. Returns whether
    /// the press was on a summary.
    fn summary_press(&mut self) -> bool {
        let unfolded = {
            let term = self.terminal.term.lock();
            let mut timeline = lock(&self.terminal.timeline);
            let rows = self.view_rows(&term, &mut timeline);
            let (column, row, _) = self.layout.point_at(
                self.mouse_position.x as f32,
                self.mouse_position.y as f32,
                &rows,
            );
            // The summary row is blank: its label starts on the first column.
            let hit = rows
                .get(row)
                .filter(|row| matches!(row.source, RowSource::Summary(_)))
                .filter(|row| {
                    let label = i18n::text(self.lang, &Msg::LinesHidden(row.hidden));
                    column < label.chars().count()
                })
                .and_then(|row| row.block);
            if let Some(id) = hit {
                timeline.toggle_fold(id);
            }
            hit.is_some()
        };
        if unfolded {
            self.window.request_redraw();
        }
        unfolded
    }

    fn left_press(&mut self) {
        if self.summary_press() {
            return;
        }
        // The selection changes until the release: no text to keep meanwhile.
        self.selection_snapshot = None;
        if self.selected_block.take().is_some() {
            self.window.request_redraw();
        }
        let mut term = self.terminal.term.lock();
        let mut timeline = lock(&self.terminal.timeline);
        let Some((source, point, side)) = self.mouse_target(&term, &mut timeline) else {
            return;
        };
        let clicks = self.click_tracker.click(point, Instant::now());
        // A selection lives in one source for now: `Shift` extends it there only.
        if let Some(previous) = self.selection_source.take()
            && previous != source
        {
            clear_source_selection(&mut term, &mut timeline, previous);
        }
        let started = match source {
            RowSource::Live => {
                selection::start_selection(&mut *term, point, side, clicks, self.modifiers);
                true
            }
            source => match view::frozen_term_mut(&mut timeline, source) {
                Some(frozen) => {
                    selection::start_selection(frozen, point, side, clicks, self.modifiers);
                    true
                }
                None => false,
            },
        };
        if started {
            self.selection_source = Some(source);
            self.left_pressed = true;
        }
        self.window.request_redraw();
    }

    fn left_release(&mut self) {
        if !std::mem::take(&mut self.left_pressed) {
            return;
        }
        let text = {
            let mut term = self.terminal.term.lock();
            let mut timeline = lock(&self.terminal.timeline);
            match self.selection_source {
                Some(RowSource::Live) => {
                    let text = selection::finish_selection(&mut *term);
                    self.selection_snapshot = SelectionSnapshot::capture(&*term);
                    text
                }
                Some(source) => view::frozen_term_mut(&mut timeline, source)
                    .and_then(selection::finish_selection),
                None => None,
            }
        };
        self.window.request_redraw();
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            self.set_primary(text);
        }
    }

    fn cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        self.mouse_position = position;
        self.update_pointer();
        if self.left_pressed && !self.app_mouse.active() {
            self.update_selection();
            return;
        }
        // A gesture the application owns follows the pointer anywhere; a bare motion is
        // only reported over the application's rows.
        if !self.app_mouse.active() && !self.reports_at(&self.pointer_hit().0) {
            return;
        }
        let cell = self.mouse_cell();
        let mode = self.mode();
        if let Some(bytes) = self.app_mouse.motion(cell, self.modifiers, mode) {
            self.terminal.write(bytes);
        }
    }

    /// Moves the selection end to the cell under the mouse cursor, kept in the source
    /// where the selection started.
    fn update_selection(&self) {
        let Some(source) = self.selection_source else {
            return;
        };
        let mut term = self.terminal.term.lock();
        let mut timeline = lock(&self.terminal.timeline);
        let Some(pointer) = self.mouse_target(&term, &mut timeline) else {
            return;
        };
        let Some((point, side)) = view::selection_end(&timeline, &*term, source, pointer) else {
            return;
        };
        let selection = match source {
            RowSource::Live => term.selection.as_mut(),
            source => view::frozen_term_mut(&mut timeline, source)
                .and_then(|frozen| frozen.selection.as_mut()),
        };
        if let Some(selection) = selection {
            selection.update(point, side);
            self.window.request_redraw();
        }
    }

    /// Auto-scroll while dragging a selection past the top or bottom edge: once per
    /// `AUTO_SCROLL_INTERVAL`, scrolls the timeline and moves the selection end onto the
    /// edge row. Returns when the next step is due, `None` when auto-scroll is idle.
    fn auto_scroll(&mut self, now: Instant) -> Option<Instant> {
        let delta = {
            let term = self.terminal.term.lock();
            let mut timeline = lock(&self.terminal.timeline);
            let rows = self.view_rows(&term, &mut timeline);
            let overflow = self
                .layout
                .vertical_overflow(self.mouse_position.y as f32, &rows);
            selection::auto_scroll_delta(overflow, *term.mode())
        };
        if !self.left_pressed || delta == 0 || self.selection_source.is_none() {
            self.auto_scroll_deadline = None;
            return None;
        }
        match self.auto_scroll_deadline {
            Some(deadline) if now < deadline => {}
            _ => {
                self.scroll_timeline(delta as isize);
                self.update_selection();
                self.auto_scroll_deadline = Some(now + selection::AUTO_SCROLL_INTERVAL);
            }
        }
        self.auto_scroll_deadline
    }

    fn set_primary(&mut self, text: String) {
        let Some(clipboard) = self.clipboard.as_mut() else {
            return;
        };
        // Some Wayland compositors do not support the primary selection.
        if let Err(error) = clipboard
            .set()
            .clipboard(LinuxClipboardKind::Primary)
            .text(text)
        {
            log::debug!("primary selection unavailable: {error}");
        }
    }

    fn paste_primary(&mut self) {
        let Some(clipboard) = self.clipboard.as_mut() else {
            return;
        };
        match clipboard
            .get()
            .clipboard(LinuxClipboardKind::Primary)
            .text()
        {
            Ok(text) => self.send_paste(&text),
            Err(error) => log::debug!("primary selection unavailable: {error}"),
        }
    }

    fn run_action(&mut self, action: Action, default_font_size: f32) {
        match action {
            Action::Copy => self.copy(),
            Action::Paste => self.paste(),
            Action::ScrollPageUp => self.scroll_page(1),
            Action::ScrollPageDown => self.scroll_page(-1),
            Action::ScrollToTop => self.scroll_to_top(),
            Action::ScrollToBottom => self.scroll_to_bottom(),
            Action::FontSizeIncrease => self.apply_font_size(self.font_size + 1.0),
            Action::FontSizeDecrease => self.apply_font_size(self.font_size - 1.0),
            Action::FontSizeReset => self.apply_font_size(default_font_size),
            Action::PreviousBlock => self.navigate_block(Direction::Previous),
            Action::NextBlock => self.navigate_block(Direction::Next),
            Action::CopyCommand => self.copy_block_part(Part::Command),
            Action::CopyOutput => self.copy_block_part(Part::Output),
            Action::ToggleFold => self.toggle_fold(),
            Action::ToggleFoldAll => self.change_folds(|timeline, _| timeline.toggle_fold_all()),
        }
    }

    fn mouse_wheel(&mut self, delta: MouseScrollDelta) {
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, y) => y as f64 * 3.0,
            MouseScrollDelta::PixelDelta(pos) => pos.y / self.layout.cell.height as f64,
        };
        self.scroll_accumulator += lines;
        let whole = self.scroll_accumulator.trunc() as i32;
        if whole == 0 {
            return;
        }
        self.scroll_accumulator -= whole as f64;

        let (hit, _) = self.pointer_hit();
        if self.reports_at(&hit) {
            let mode = self.mode();
            let (column, line) = self.mouse_cell();
            if let Some(bytes) = mouse::wheel_bytes(whole, column, line, self.modifiers, mode) {
                self.terminal.write(bytes);
                return;
            }
        }
        let target = {
            let term = self.terminal.term.lock();
            let timeline = lock(&self.terminal.timeline);
            let lines = term.screen_lines();
            mouse::wheel_target(&hit, |id| view::can_scroll(&timeline, &*term, id, lines))
        };
        self.scroll(target, whole as isize);
    }

    fn handle_terminal_event(&mut self, event: TermEvent, event_loop: &ActiveEventLoop) {
        match event {
            TermEvent::Wakeup => {
                if self.first_frame.output() {
                    perf::mark("first PTY output");
                }
                self.validate_selection();
                self.update_pointer();
                self.window.request_redraw()
            }
            TermEvent::MouseCursorDirty | TermEvent::CursorBlinkingChange => {
                self.window.request_redraw()
            }
            TermEvent::Title(title) => self.window.set_title(&title),
            TermEvent::ResetTitle => self.window.set_title(DEFAULT_TITLE),
            TermEvent::PtyWrite(text) => self.terminal.write(text.into_bytes()),
            TermEvent::ClipboardStore(_, text) => {
                if let Some(clipboard) = self.clipboard.as_mut() {
                    let _ = clipboard.set_text(text);
                }
            }
            TermEvent::ClipboardLoad(_, format) => {
                let text = self
                    .clipboard
                    .as_mut()
                    .and_then(|c| c.get_text().ok())
                    .unwrap_or_default();
                self.terminal.write(format(&text).into_bytes());
            }
            TermEvent::ColorRequest(index, format) => {
                let color = self.terminal.term.lock().colors()[index]
                    .unwrap_or_else(|| self.palette.by_index(index));
                self.terminal.write(format(color).into_bytes());
            }
            TermEvent::TextAreaSizeRequest(format) => {
                self.terminal
                    .write(format(self.layout.window_size()).into_bytes());
            }
            TermEvent::Bell => self
                .window
                .request_user_attention(Some(winit::window::UserAttentionType::Informational)),
            TermEvent::Exit | TermEvent::ChildExit(_) => event_loop.exit(),
        }
    }
}

pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    config: Config,
    /// Palette of the configured theme.
    palette: Palette,
    session: Option<Session>,
    update_checker: Checker,
    error: Option<anyhow::Error>,
    /// Banner reporting the errors of the configuration and theme files.
    config_banner: Option<Banner>,
}

impl App {
    fn create_session(&self, event_loop: &ActiveEventLoop) -> anyhow::Result<Session> {
        let attributes = Window::default_attributes()
            .with_title(DEFAULT_TITLE)
            .with_inner_size(LogicalSize::new(
                self.config.window.width,
                self.config.window.height,
            ));
        let window = Arc::new(event_loop.create_window(attributes)?);
        perf::mark("window created");

        // Shell first, GPU second: the PTY reader thread parses the shell output while
        // the renderer is being created, so the first frame shows it complete.
        let scale = window.scale_factor() as f32;
        let glyphs = Glyphs::new(
            self.config.font.family.clone(),
            self.config.font.size * scale,
        );
        perf::mark("glyphs ready");

        let size = window.inner_size();
        let layout = GridLayout::new(
            size.width,
            size.height,
            glyphs.cell_size(),
            self.config.window.padding * scale,
        );
        let terminal = Terminal::spawn(
            EventProxy::new(self.proxy.clone()),
            layout.window_size(),
            self.config.term_config(),
        )?;
        perf::mark("shell spawned");

        let renderer = pollster::block_on(Renderer::new(
            window.clone(),
            event_loop.owned_display_handle(),
            glyphs,
        ))?;

        let clipboard = arboard::Clipboard::new()
            .map_err(|e| log::warn!("clipboard unavailable: {e}"))
            .ok();

        Ok(Session {
            window,
            renderer,
            terminal,
            layout,
            palette: self.palette.clone(),
            clipboard,
            focused: true,
            modifiers: ModifiersState::empty(),
            font_size: self.config.font.size,
            padding: self.config.window.padding,
            scroll_accumulator: 0.0,
            mouse_position: PhysicalPosition::default(),
            left_pressed: false,
            click_tracker: ClickTracker::default(),
            app_mouse: mouse::Reporter::default(),
            pointer: None,
            auto_scroll_deadline: None,
            selection_snapshot: None,
            selection_source: None,
            selected_block: None,
            anchor: Cell::default(),
            config_banner: self.config_banner.clone(),
            update: UpdateBanner::default(),
            ignored_tag: None,
            lang: Lang::detect(),
            first_frame: perf::FirstFrame::default(),
        })
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.error = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.session.is_some() {
            return;
        }
        match self.create_session(event_loop) {
            Ok(mut session) => {
                session.update_pointer();
                self.session = Some(session);
            }
            Err(error) => return self.fail(event_loop, error),
        }
        self.update_checker.apply(&self.config.updates);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let deadline = self
            .session
            .as_mut()
            .and_then(|s| s.auto_scroll(Instant::now()));
        event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        match event {
            UserEvent::Terminal(event) => session.handle_terminal_event(event, event_loop),
            UserEvent::Update(release) => {
                session.update.set(release);
                session.window.request_redraw();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let default_font_size = self.config.font.size;
        let Some(session) = self.session.as_mut() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                session.renderer.resize(size.width, size.height);
                session.relayout();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let font_size = session.font_size;
                session.apply_font_size(font_size);
            }
            WindowEvent::Focused(focused) => {
                session.focused = focused;
                if let Some(bytes) = mouse::focus_report(focused, session.mode()) {
                    session.terminal.write(bytes);
                }
                session.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => session.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(action) = input::action_for(&event.logical_key, session.modifiers) {
                    session.run_action(action, default_font_size);
                    return;
                }
                let mode = session.mode();
                if let Some(bytes) = input::key_to_bytes(
                    &event.logical_key,
                    event.text.as_deref(),
                    session.modifiers,
                    mode,
                ) {
                    session.clear_selection();
                    session.scroll_to_bottom();
                    session.terminal.write(bytes);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => session.mouse_wheel(delta),
            WindowEvent::CursorMoved { position, .. } => session.cursor_moved(position),
            WindowEvent::MouseInput { state, button, .. } => {
                session.mouse_input(state, button);
                if let Some(tag) = session.ignored_tag.take() {
                    self.update_checker.ignore(&tag);
                }
            }
            WindowEvent::RedrawRequested => {
                let mut frame = {
                    let term = session.terminal.term.lock();
                    let mut timeline = lock(&session.terminal.timeline);
                    let rows = shown_rows(&session.anchor, &term, &mut timeline);
                    Frame::capture(
                        &*term,
                        &timeline,
                        &rows,
                        &session.palette,
                        session.focused,
                        session.selected_block,
                    )
                };
                frame.lang = session.lang;
                frame.banner = session.shown_banner().map(|shown| FrameBanner {
                    layout: session.banner_layout(&shown),
                    background: session.palette.ui.banner_background,
                    text: session.palette.ui.banner_text,
                });
                match session.renderer.render(&frame, &session.layout) {
                    Ok(true) if session.first_frame.frame_drawn() => {
                        perf::mark("first frame drawn after PTY output");
                    }
                    Ok(_) => {}
                    Err(error) => log::error!("render failed: {error:#}"),
                }
            }
            _ => {}
        }
    }
}

/// Locks the timeline. Only while the terminal is locked, or without it: lock order
/// `Term`, then `Timeline`.
fn lock(timeline: &Mutex<Timeline>) -> MutexGuard<'_, Timeline> {
    timeline.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Rows of the view over `timeline`, whose current block lives in `term`, kept still
/// with `anchor` while output arrives (see [`view::prepare`]).
fn shown_rows(
    anchor: &Cell<Anchor>,
    term: &term::Term<EventProxy>,
    timeline: &mut Timeline,
) -> Vec<ViewRow> {
    let mut shown = anchor.get();
    let rows = view::prepare(timeline, term, term.screen_lines(), &mut shown);
    anchor.set(shown);
    rows
}

/// Clears the text selection held by `source`. Returns whether there was one.
fn clear_source_selection(
    term: &mut term::Term<EventProxy>,
    timeline: &mut Timeline,
    source: RowSource,
) -> bool {
    match source {
        RowSource::Live => term.selection.take().is_some(),
        source => view::frozen_term_mut(timeline, source)
            .is_some_and(|frozen| frozen.selection.take().is_some()),
    }
}

/// Loads the configuration file and its theme. Errors are logged and reported by the
/// returned banner.
fn load_config() -> (Config, Palette, Option<Banner>) {
    let (config, mut errors) = config::load();
    let (palette, theme_errors) = theme::load(&config.theme.name, config::config_dir().as_deref());
    errors.extend(theme_errors);
    for error in &errors {
        log::warn!("{error}");
    }
    (config, palette, Banner::config_errors(&errors))
}

pub fn run() -> anyhow::Result<()> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let update_proxy = event_loop.create_proxy();
    let (config, palette, config_banner) = load_config();
    let mut app = App {
        update_checker: Checker::new(move |release| {
            // The event loop may already be closed during shutdown.
            let _ = update_proxy.send_event(UserEvent::Update(release));
        }),
        proxy: event_loop.create_proxy(),
        config,
        palette,
        session: None,
        error: None,
        config_banner,
    };
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
