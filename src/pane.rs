//! A pane: one shell and everything the window keeps about it (grid placement,
//! selection, scrolling, mouse reporting, title).
//!
//! A pane never touches the window: it records that it needs a redraw and the text it
//! wants in the primary selection, and the window drains both (see
//! [`Pane::take_redraw`] and [`Pane::take_primary`]), so a hidden pane never redraws.

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{self, TermMode};
use winit::dpi::PhysicalPosition;
use winit::event::MouseScrollDelta;
use winit::keyboard::ModifiersState;

use crate::block_actions::{self, Direction, Part};
use crate::frame;
use crate::i18n::{self, Lang, Msg};
use crate::input;
use crate::input_editor::{self, EditorCommand};
use crate::layout::{GridLayout, Hit};
use crate::mouse;
use crate::selection::{self, ClickTracker, SelectionSnapshot};
use crate::terminal::{EventProxy, PaneId, Terminal};
use crate::timeline::{BlockId, Timeline};
use crate::timeline_selection::{self, End, TimelineSelection};
use crate::view::{self, Anchor, RowSource, ScrollTarget, ViewRow};
use crate::workspace::TitleSource;

/// How often a pane without OSC title has its fallback title (its directory) read
/// again from `/proc`.
pub const TITLE_REFRESH: Duration = Duration::from_secs(1);

pub struct Pane {
    pub id: PaneId,
    pub terminal: Terminal,
    pub layout: GridLayout,
    /// Last title set by the shell or a program (OSC 0/2), `None` after a reset.
    pub title: Option<String>,
    /// Fractional lines accumulated from touchpad scrolling.
    scroll_accumulator: f64,
    click_tracker: ClickTracker,
    /// Gesture owned by the application (mouse reporting).
    pub app_mouse: mouse::Reporter,
    /// Finished selection of the live grid kept while its text is unchanged.
    selection_snapshot: Option<SelectionSnapshot>,
    /// Keyboard selection at the prompt waiting for the shell to move its cursor.
    extend_pending: Option<input_editor::PendingExtend>,
    /// Text selection over the timeline, set on the grids it covers.
    pub selection: Option<TimelineSelection>,
    /// Block selected whole by a margin click or a block navigation, shown as a
    /// highlighted card instead of highlighted cells.
    pub selected_block: Option<BlockId>,
    /// What the view looked like at the last frame, to keep it still while output
    /// arrives.
    pub anchor: Cell<Anchor>,
    /// When the fallback title was last read, `None` before the first time.
    title_checked: Option<Instant>,
    /// The pane changed what it shows since the window last asked.
    redraw: Cell<bool>,
    /// Text to put in the primary selection, waiting for the window.
    primary: Option<String>,
}

impl TitleSource for Pane {
    fn osc_title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    fn current_dir(&self) -> Option<PathBuf> {
        self.terminal.current_dir()
    }

    fn shell_name(&self) -> Option<String> {
        self.terminal.shell_name()
    }
}

impl Pane {
    pub fn new(id: PaneId, terminal: Terminal, layout: GridLayout) -> Self {
        Self {
            id,
            terminal,
            layout,
            title: None,
            scroll_accumulator: 0.0,
            click_tracker: ClickTracker::default(),
            app_mouse: mouse::Reporter::default(),
            selection_snapshot: None,
            extend_pending: None,
            selection: None,
            selected_block: None,
            anchor: Cell::default(),
            title_checked: None,
            redraw: Cell::new(false),
            primary: None,
        }
    }

    fn request_redraw(&self) {
        self.redraw.set(true);
    }

    /// Whether the pane needs a redraw since the last call.
    pub fn take_redraw(&self) -> bool {
        self.redraw.replace(false)
    }

    /// Text the pane wants in the primary selection, since the last call.
    pub fn take_primary(&mut self) -> Option<String> {
        self.primary.take()
    }

    fn set_primary(&mut self, text: String) {
        self.primary = Some(text);
    }

    /// Whether the fallback title is due for a new read at `now` (see
    /// [`TITLE_REFRESH`]); marks it read when it is. Never due with an OSC title.
    pub fn title_refresh_due(&mut self, now: Instant) -> bool {
        if self.title.is_some() {
            return false;
        }
        let due = refresh_due(self.title_checked, now);
        if due {
            self.title_checked = Some(now);
        }
        due
    }

    /// Places the pane in `layout`; the shell is resized only when its cell count
    /// changed.
    pub fn resize(&mut self, layout: GridLayout) {
        let changed = cells_changed(&self.layout, &layout);
        self.layout = layout;
        if changed {
            self.terminal.resize(layout.window_size());
        }
        self.request_redraw();
    }

    pub fn mode(&self) -> TermMode {
        *self.terminal.term.lock().mode()
    }

    /// Whether the rich input editor is active (see [`input_editor::rich_mode`]).
    pub fn rich_mode(&self, modifiers: ModifiersState) -> bool {
        let term = self.terminal.term.lock();
        let timeline = lock(&self.terminal.timeline);
        let current = timeline.current();
        input_editor::rich_mode(current.phase, current.editing, *term.mode(), modifiers)
    }

    /// Sends bytes typed on the keyboard: like any keystroke, they clear the selection
    /// and bring the view back to the bottom.
    pub fn type_bytes(&mut self, bytes: Vec<u8>) {
        self.clear_selection();
        self.scroll_to_bottom();
        self.terminal.write(bytes);
    }

    /// Runs an editor command at the prompt: a move clears the selection, an extension
    /// or `SelectAll` starts the keyboard selection; every command writes its private
    /// sequence, which the shell binds.
    pub fn run_editor_command(&mut self, command: EditorCommand) {
        match command {
            EditorCommand::Move(_) => self.clear_selection(),
            EditorCommand::Extend(_) => self.start_extend(false),
            EditorCommand::SelectAll => self.start_extend(true),
            EditorCommand::InsertNewline | EditorCommand::DeleteWord(_) => {}
        }
        self.scroll_to_bottom();
        self.terminal.write(input_editor::command_bytes(command));
    }

    /// Replaces the selection by `typed` when it lies in the input (see
    /// [`input_editor::input_selection`]): moves the shell cursor to its end, erases
    /// it and types `typed`, in one PTY write so no shell redraw interleaves, then
    /// clears it. Returns whether it did; otherwise the key keeps its normal path.
    pub fn replace_selection(&mut self, typed: &[u8]) -> bool {
        let bytes =
            {
                let term = self.terminal.term.lock();
                let timeline = lock(&self.terminal.timeline);
                let Some(bounds) = self.selection.as_ref().and_then(|selection| {
                    input_editor::input_selection(selection, &timeline, &*term)
                }) else {
                    return false;
                };
                input_editor::replacement_bytes(&*term, term.grid().cursor.point, bounds, typed)
            };
        self.clear_selection();
        self.scroll_to_bottom();
        self.terminal.write(bytes);
        true
    }

    /// Before an extension sequence is written: the selection to grow (a new one at the
    /// cursor without one, the whole input for `select_all`), extended once the shell
    /// moved its cursor (see [`Pane::finish_extend`]).
    fn start_extend(&mut self, select_all: bool) {
        self.selected_block = None;
        let text = {
            let mut term = self.terminal.term.lock();
            let mut timeline = lock(&self.terminal.timeline);
            let selection = if select_all {
                input_editor::select_all(&timeline, &*term)
            } else {
                input_editor::extend_selection(self.selection.take(), &timeline, &*term)
            };
            timeline_selection::apply(Some(&selection), &mut *term, &mut timeline);
            self.selection = Some(selection);
            self.selection_snapshot = SelectionSnapshot::capture(&*term);
            self.extend_pending = Some(input_editor::PendingExtend::at(&*term));
            timeline_selection::text(&*term, &timeline)
        };
        self.request_redraw();
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            self.set_primary(text);
        }
    }

    /// After new PTY output: once the shell moved its cursor for a pending extension,
    /// moves the selection end there, then keeps its text and writes it to the primary
    /// selection, as a mouse release does.
    pub fn finish_extend(&mut self) {
        let Some(pending) = self.extend_pending else {
            return;
        };
        let text = {
            let mut term = self.terminal.term.lock();
            let mut timeline = lock(&self.terminal.timeline);
            let Some(cursor) = pending.moved(&*term) else {
                return;
            };
            let Some(selection) = self.selection.as_mut() else {
                self.extend_pending = None;
                return;
            };
            selection.update(input_editor::cursor_end(cursor), &*term);
            timeline_selection::apply(Some(selection), &mut *term, &mut timeline);
            self.selection_snapshot = SelectionSnapshot::capture(&*term);
            timeline_selection::text(&*term, &timeline)
        };
        self.extend_pending = None;
        self.request_redraw();
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            self.set_primary(text);
        }
    }

    /// What lies under the pixel `position`, and whether it is the fold chevron of a
    /// card that has one.
    pub fn pointer_hit(&self, position: PhysicalPosition<f64>) -> (Hit, bool) {
        let term = self.terminal.term.lock();
        let mut timeline = lock(&self.terminal.timeline);
        let rows = shown_rows(&self.anchor, &term, &mut timeline);
        let hit = self.layout.hit(position.x as f32, position.y as f32, &rows);
        let over_chevron = match hit {
            Hit::Chevron { row } => frame::has_chevron(&*term, &timeline, &rows, row),
            _ => false,
        };
        (hit, over_chevron)
    }

    /// Whether a mouse event over `hit` goes to the application when it captures the
    /// mouse (see [`mouse::reports_at`]).
    pub fn reports_at(&self, hit: &Hit) -> bool {
        mouse::reports_at(hit, self.mode().contains(TermMode::ALT_SCREEN))
    }

    /// Scrolls the timeline by `rows` rows (positive shows older blocks), stopping at
    /// its ends.
    pub fn scroll_timeline(&self, rows: isize) {
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
        self.request_redraw();
    }

    /// `Shift+PageUp/PageDown`: scrolls the selected block by a page when it can scroll,
    /// otherwise the timeline. `pages` is positive toward older rows.
    pub fn scroll_page(&self, pages: isize) {
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
    pub fn scroll_to_top(&self) {
        let term = self.terminal.term.lock();
        let mut timeline = lock(&self.terminal.timeline);
        let lines = term.screen_lines();
        timeline.scroll = view::max_timeline_scroll(&timeline, &term, lines);
        self.request_redraw();
    }

    /// Brings the view back to the bottom: the timeline and the current block at rest.
    pub fn scroll_to_bottom(&self) {
        let mut timeline = lock(&self.terminal.timeline);
        timeline.scroll = 0;
        timeline.current_mut().scroll = 0;
        self.request_redraw();
    }

    /// Rows scrolled by a page: the view lines.
    fn page_rows(&self) -> isize {
        self.layout.lines as isize
    }

    /// Changes the folds with `change`, given the selected block, then redraws.
    pub fn change_folds(&mut self, change: impl FnOnce(&mut Timeline, Option<BlockId>)) {
        change(&mut lock(&self.terminal.timeline), self.selected_block);
        self.request_redraw();
    }

    /// Folds or unfolds the target block.
    pub fn toggle_fold(&mut self) {
        self.change_folds(|timeline, selected| {
            if let Some(id) = block_actions::target(timeline, selected) {
                timeline.toggle_fold(id);
            }
        });
    }

    /// Sends a paste to the shell. Like a keystroke, it clears the selection.
    pub fn send_paste(&mut self, text: &str) {
        self.clear_selection();
        let bytes = input::paste_bytes(text, self.mode());
        self.scroll_to_bottom();
        self.terminal.write(bytes);
    }

    /// Cleaned text of the selected block, or of the text selection.
    pub fn selected_text(&self) -> Option<String> {
        let term = self.terminal.term.lock();
        let timeline = lock(&self.terminal.timeline);
        if let Some(id) = self.selected_block {
            return block_actions::block_text(&timeline, id);
        }
        self.selection.as_ref()?;
        timeline_selection::text(&*term, &timeline)
    }

    /// Text of one part of the target block.
    pub fn block_part_text(&self, part: Part) -> Option<String> {
        let timeline = lock(&self.terminal.timeline);
        block_actions::target(&timeline, self.selected_block)
            .and_then(|id| block_actions::text(&timeline, id, part))
    }

    /// Selects the neighbour block in `direction` and scrolls it into view.
    pub fn navigate_block(&mut self, direction: Direction) {
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
    pub fn margin_press(&mut self, row: usize) {
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
        self.request_redraw();
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            self.set_primary(text);
        }
    }

    /// Left press on the chevron disc of the card starting on view row `row`: toggles
    /// the fold of its block when it has a chevron. Returns whether it had one.
    pub fn chevron_press(&mut self, row: usize) -> bool {
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
            self.request_redraw();
        }
        toggled
    }

    /// View rows shown for `term` and `timeline`, the locked terminal and timeline.
    pub fn view_rows(
        &self,
        term: &term::Term<EventProxy>,
        timeline: &mut Timeline,
    ) -> Vec<ViewRow> {
        shown_rows(&self.anchor, term, timeline)
    }

    /// Clears the text selection and the selected block.
    pub fn clear_selection(&mut self) {
        self.selection_snapshot = None;
        self.extend_pending = None;
        let had_block = self.selected_block.take().is_some();
        let had_text = self.selection.take().is_some() && {
            let mut term = self.terminal.term.lock();
            let mut timeline = lock(&self.terminal.timeline);
            timeline_selection::clear(&mut *term, &mut timeline)
        };
        if had_block || had_text {
            self.request_redraw();
        }
    }

    /// After new PTY output, for a selection reaching the live grid (frozen blocks never
    /// change): restores the live part erased while its text was redrawn unchanged, and
    /// clears the whole selection when the selected live text really changed, or when the
    /// live grid was detached into a block.
    pub fn validate_selection(&mut self) {
        let valid = {
            let mut term = self.terminal.term.lock();
            let timeline = lock(&self.terminal.timeline);
            match self.selection.as_ref() {
                Some(selection) if selection.covers_live(&timeline) => {
                    !selection.is_stale(&timeline)
                        && self
                            .selection_snapshot
                            .as_mut()
                            .is_none_or(|snapshot| snapshot.validate(&mut *term))
                }
                _ => true,
            }
        };
        if !valid {
            self.clear_selection();
        }
    }

    /// Source, grid point and cell side under the pixel `position`. `term` and
    /// `timeline` are the locked terminal and timeline.
    fn mouse_target(
        &self,
        position: PhysicalPosition<f64>,
        term: &term::Term<EventProxy>,
        timeline: &mut Timeline,
    ) -> Option<End> {
        let rows = self.view_rows(term, timeline);
        let (column, row, side) = self
            .layout
            .point_at(position.x as f32, position.y as f32, &rows);
        view::point(&rows, row, column).map(|(source, point)| End {
            source,
            point,
            side,
        })
    }

    /// Viewport cell (column, line) under the pixel `position`, clamped to the grid, as
    /// reported to the application: the live grid's line under the cursor (its viewport
    /// is never scrolled), line 0 over a frozen row.
    pub fn mouse_cell(&self, position: PhysicalPosition<f64>) -> (usize, usize) {
        let term = self.terminal.term.lock();
        let mut timeline = lock(&self.terminal.timeline);
        let Some(End { source, point, .. }) = self.mouse_target(position, &term, &mut timeline)
        else {
            return (0, 0);
        };
        let last = term.screen_lines() as i32 - 1;
        let line = match source {
            RowSource::Live => point.line.0.clamp(0, last),
            _ => 0,
        };
        (point.column.0, line as usize)
    }

    /// Left press on the summary label of a folded block: unfolds it. Returns whether
    /// the press was on a summary.
    fn summary_press(&mut self, position: PhysicalPosition<f64>, lang: Lang) -> bool {
        let unfolded = {
            let term = self.terminal.term.lock();
            let mut timeline = lock(&self.terminal.timeline);
            let rows = self.view_rows(&term, &mut timeline);
            let (column, row, _) =
                self.layout
                    .point_at(position.x as f32, position.y as f32, &rows);
            // The summary row is blank: its label starts on the first column.
            let hit = rows
                .get(row)
                .filter(|row| matches!(row.source, RowSource::Summary(_)))
                .filter(|row| {
                    let label = i18n::text(lang, &Msg::LinesHidden(row.hidden));
                    column < label.chars().count()
                })
                .and_then(|row| row.block);
            if let Some(id) = hit {
                timeline.toggle_fold(id);
            }
            hit.is_some()
        };
        if unfolded {
            self.request_redraw();
        }
        unfolded
    }

    /// Left press at `position` in the grid: starts or extends a selection. Returns
    /// whether a selection drag started.
    pub fn left_press(
        &mut self,
        position: PhysicalPosition<f64>,
        modifiers: ModifiersState,
        lang: Lang,
    ) -> bool {
        if self.summary_press(position, lang) {
            return false;
        }
        // The selection changes until the release: no text to keep meanwhile.
        self.selection_snapshot = None;
        self.extend_pending = None;
        if self.selected_block.take().is_some() {
            self.request_redraw();
        }
        let mut term = self.terminal.term.lock();
        let mut timeline = lock(&self.terminal.timeline);
        let Some(at) = self.mouse_target(position, &term, &mut timeline) else {
            return false;
        };
        let clicks = self.click_tracker.click(at.point, Instant::now());
        // `Shift` extends the existing selection, across blocks too.
        match self.selection.as_mut() {
            Some(selection) if modifiers.shift_key() => selection.update(at, &*term),
            _ => {
                let kind = selection::selection_type(clicks, modifiers);
                self.selection = Some(TimelineSelection::new(kind, at, &timeline, &*term));
            }
        }
        timeline_selection::apply(self.selection.as_ref(), &mut *term, &mut timeline);
        self.request_redraw();
        true
    }

    /// End of a selection drag at `position`.
    pub fn left_release(&mut self, position: PhysicalPosition<f64>, modifiers: ModifiersState) {
        let mut moves = Vec::new();
        let text = {
            let mut term = self.terminal.term.lock();
            let mut timeline = lock(&self.terminal.timeline);
            match self.selection.as_ref() {
                // A click without drag selects nothing; in the input, it moves the shell
                // cursor there.
                Some(selection) if selection.is_empty(&timeline, &*term) => {
                    self.selection = None;
                    timeline_selection::clear(&mut *term, &mut timeline);
                    let current = timeline.current();
                    let rich = input_editor::rich_mode(
                        current.phase,
                        current.editing,
                        *term.mode(),
                        modifiers,
                    );
                    if rich
                        && let Some(End {
                            source: RowSource::Live,
                            point,
                            ..
                        }) = self.mouse_target(position, &term, &mut timeline)
                    {
                        let floor = timeline.current().prompt_start(&*term);
                        let cursor = term.grid().cursor.point;
                        moves = input_editor::click_moves(&*term, cursor, point, floor);
                    }
                    None
                }
                Some(selection) => {
                    if selection.covers_live(&timeline) {
                        self.selection_snapshot = SelectionSnapshot::capture(&*term);
                    }
                    timeline_selection::text(&*term, &timeline)
                }
                None => None,
            }
        };
        self.request_redraw();
        if !moves.is_empty() {
            self.terminal.write(moves);
        }
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            self.set_primary(text);
        }
    }

    /// Moves the selection end to the cell under `position`, in whichever block it
    /// lies.
    pub fn update_selection(&mut self, position: PhysicalPosition<f64>) {
        let mut term = self.terminal.term.lock();
        let mut timeline = lock(&self.terminal.timeline);
        let Some(at) = self.mouse_target(position, &term, &mut timeline) else {
            return;
        };
        let Some(selection) = self.selection.as_mut() else {
            return;
        };
        selection.update(at, &*term);
        timeline_selection::apply(Some(selection), &mut *term, &mut timeline);
        self.request_redraw();
    }

    /// Rows to auto-scroll by while a selection is dragged at height `y` (see
    /// [`selection::auto_scroll_delta`]); 0 inside the grid or without selection.
    pub fn auto_scroll_delta(&self, y: f64) -> i32 {
        if self.selection.is_none() {
            return 0;
        }
        let term = self.terminal.term.lock();
        let mut timeline = lock(&self.terminal.timeline);
        let rows = self.view_rows(&term, &mut timeline);
        let overflow = self.layout.vertical_overflow(y as f32, &rows);
        selection::auto_scroll_delta(overflow, *term.mode())
    }

    /// Mouse wheel over the pane at `position`: reported to the application when it
    /// captures the mouse there, otherwise scrolls the block or the timeline under it.
    pub fn mouse_wheel(
        &mut self,
        delta: MouseScrollDelta,
        position: PhysicalPosition<f64>,
        modifiers: ModifiersState,
    ) {
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

        let (hit, _) = self.pointer_hit(position);
        if self.reports_at(&hit) {
            let mode = self.mode();
            let (column, line) = self.mouse_cell(position);
            if let Some(bytes) = mouse::wheel_bytes(whole, column, line, modifiers, mode) {
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
}

/// Whether a fallback title last read at `checked` is due for a new read at `now`.
fn refresh_due(checked: Option<Instant>, now: Instant) -> bool {
    checked.is_none_or(|checked| now.saturating_duration_since(checked) >= TITLE_REFRESH)
}

/// Whether going from `old` to `new` changes the grid's cell count, so the shell must
/// be resized.
fn cells_changed(old: &GridLayout, new: &GridLayout) -> bool {
    (old.columns, old.lines) != (new.columns, new.lines)
}

/// Locks the timeline. Only while the terminal is locked, or without it: lock order
/// `Term`, then `Timeline`.
pub fn lock(timeline: &Mutex<Timeline>) -> MutexGuard<'_, Timeline> {
    timeline.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Rows of the view over `timeline`, whose current block lives in `term`, kept still
/// with `anchor` while output arrives (see [`view::prepare`]).
pub fn shown_rows(
    anchor: &Cell<Anchor>,
    term: &term::Term<EventProxy>,
    timeline: &mut Timeline,
) -> Vec<ViewRow> {
    let mut shown = anchor.get();
    let rows = view::prepare(timeline, term, term.screen_lines(), &mut shown);
    anchor.set(shown);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{CellSize, Rect};

    fn layout(width: u32, height: u32) -> GridLayout {
        let cell = CellSize {
            width: 10.0,
            height: 20.0,
        };
        GridLayout::new(Rect::from_size(width, height), cell, 8.0)
    }

    #[test]
    fn fallback_title_is_read_at_most_once_per_refresh_interval() {
        let now = Instant::now();
        assert!(refresh_due(None, now));
        assert!(!refresh_due(Some(now), now + TITLE_REFRESH / 2));
        assert!(refresh_due(Some(now), now + TITLE_REFRESH));
        // A clock going backwards never makes it due early.
        assert!(!refresh_due(Some(now + TITLE_REFRESH), now));
    }

    #[test]
    fn same_cell_count_needs_no_shell_resize() {
        // A few pixels more than the cell width do not add a column.
        assert!(!cells_changed(&layout(800, 600), &layout(801, 605)));
    }

    #[test]
    fn other_cell_count_needs_a_shell_resize() {
        assert!(cells_changed(&layout(800, 600), &layout(900, 600)));
        assert!(cells_changed(&layout(800, 600), &layout(800, 700)));
    }

    #[test]
    fn moved_rect_with_same_size_needs_no_shell_resize() {
        let moved = GridLayout::new(
            Rect {
                x: 400.0,
                y: 30.0,
                width: 800.0,
                height: 600.0,
            },
            layout(800, 600).cell,
            8.0,
        );
        assert!(!cells_changed(&layout(800, 600), &moved));
    }
}
