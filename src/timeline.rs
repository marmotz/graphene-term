//! Command blocks as a timeline: finished blocks, each frozen in a terminal of its own,
//! and the current block, whose content lives in the session's grid.
//!
//! The shell hooks send `start` when a command starts and `end` when it ends
//! ([`crate::signals`]). On `start`, the live grid (prompt and typed command) is
//! detached into the current block's header; on `end`, the live grid (the command's
//! output) is detached into its output and the block joins the finished ones. Each
//! detach hands the session a fresh grid, so a command can only ever redraw its own
//! block. Without signals nothing is detached: the current block keeps growing.

use std::collections::VecDeque;
use std::mem;

use alacritty_terminal::event::{EventListener, VoidListener};
use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::Color;

use crate::signals::{Chunk, Signal};

/// Identifier of a command block, increasing with time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u64);

/// How a finished command ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Exited with 0.
    Success,
    /// Exited with this non-zero code.
    Failure(i32),
    /// The `end` signal carried no status.
    Unknown,
    /// The command of the current block still runs. Never the status of a finished
    /// block: only shown by its card.
    Running,
}

impl Status {
    fn from_signal(status: Option<i32>) -> Self {
        match status {
            Some(0) => Status::Success,
            Some(code) => Status::Failure(code),
            None => Status::Unknown,
        }
    }
}

/// What the current block holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The shell draws its prompt and the user types: the live grid is the header.
    Prompt,
    /// The command runs: the header is frozen, the live grid is the output.
    Running,
}

/// A finished block.
pub struct Block {
    pub id: BlockId,
    /// Prompt and typed command, frozen at `start`.
    pub header: Term<VoidListener>,
    /// The command's output, frozen at `end`.
    pub output: Term<VoidListener>,
    /// The command line from the `start` signal, empty when the hook sent none.
    pub command: String,
    pub status: Status,
    pub folded: bool,
    /// Output rows hidden below, 0 = bottom.
    pub scroll: usize,
    /// Set by a screen clear: the view shows this block at its top.
    pub pin_top: bool,
}

impl Block {
    /// Number of frozen rows the block holds.
    pub fn rows(&self) -> usize {
        self.header.screen_lines() + self.output.screen_lines()
    }

    /// Whether the command printed nothing.
    pub fn output_is_empty(&self) -> bool {
        content_rows(self.output.grid()) == 0
    }

    /// Reflows the header and the output for `columns` when they were frozen with other
    /// columns. Called when the block is used, never on every block at once.
    pub fn reflow(&mut self, columns: usize) {
        reflow(&mut self.header, columns);
        reflow(&mut self.output, columns);
    }
}

/// The block whose content is in the live grid.
pub struct Current {
    pub id: BlockId,
    pub phase: Phase,
    /// Prompt and typed command, frozen once the command runs.
    pub header: Option<Term<VoidListener>>,
    pub command: String,
    pub folded: bool,
    /// Output rows hidden below, 0 = bottom.
    pub scroll: usize,
    /// Set by a screen clear: the view shows this block at its top.
    pub pin_top: bool,
    /// The shell sent `prompt` for this block: its line editor reads the command line,
    /// with Graphene's editor key bindings. Never set without the shell integration.
    pub editing: bool,
    /// Where the cursor stood when `prompt` came, just before the prompt is drawn: the
    /// line counted from the top of the live history (so it stays on its text while the
    /// screen scrolls) and the column.
    prompt_start: Option<(usize, Column)>,
}

impl Current {
    fn new(id: BlockId) -> Self {
        Current {
            id,
            phase: Phase::Prompt,
            header: None,
            command: String::new(),
            folded: false,
            scroll: 0,
            pin_top: false,
            editing: false,
            prompt_start: None,
        }
    }

    /// Point of the live grid `term` where the current prompt starts, the start of the
    /// live history when it is gone (history cleared, reflow). `None` before `prompt`.
    pub fn prompt_start<L>(&self, term: &Term<L>) -> Option<Point> {
        let (line, column) = self.prompt_start?;
        let grid = term.grid();
        let history = grid.history_size() as i32;
        let last = grid.screen_lines() as i32 - 1;
        let line = (line as i32 - history).clamp(-history, last);
        let column = column.min(Column(grid.columns() - 1));
        Some(Point::new(Line(line), column))
    }
}

/// The blocks of a session.
pub struct Timeline {
    /// Finished blocks, oldest first.
    blocks: VecDeque<Block>,
    current: Current,
    next_id: u64,
    /// View offset from the bottom of the timeline, in half lines.
    pub scroll: usize,
    /// Cap on the frozen rows, and history size of the live grid.
    scrollback: usize,
    /// Signals received on the alternate screen, applied once it is left.
    pending: Vec<Signal>,
}

impl Timeline {
    /// An empty timeline whose frozen rows are capped at `scrollback`, the history size
    /// of the session's terminal.
    pub fn new(scrollback: usize) -> Self {
        Timeline {
            blocks: VecDeque::new(),
            current: Current::new(BlockId(0)),
            next_id: 1,
            scroll: 0,
            scrollback,
            pending: Vec::new(),
        }
    }

    /// Finished blocks, oldest first.
    pub fn blocks(&self) -> &VecDeque<Block> {
        &self.blocks
    }

    pub fn block(&self, id: BlockId) -> Option<&Block> {
        self.blocks.iter().find(|block| block.id == id)
    }

    pub fn block_mut(&mut self, id: BlockId) -> Option<&mut Block> {
        self.blocks.iter_mut().find(|block| block.id == id)
    }

    pub fn current(&self) -> &Current {
        &self.current
    }

    pub fn current_mut(&mut self) -> &mut Current {
        &mut self.current
    }

    /// Every frozen terminal, top to bottom: each finished block's header then output,
    /// then the current block's header. Each comes with its block and whether it is a
    /// header.
    pub fn frozen(&self) -> impl Iterator<Item = (BlockId, bool, &Term<VoidListener>)> {
        let current = &self.current;
        self.blocks
            .iter()
            .flat_map(|block| {
                [
                    (block.id, true, &block.header),
                    (block.id, false, &block.output),
                ]
            })
            .chain(
                current
                    .header
                    .iter()
                    .map(|header| (current.id, true, header)),
            )
    }

    /// Mutable [`Timeline::frozen`].
    pub fn frozen_mut(&mut self) -> impl Iterator<Item = (BlockId, bool, &mut Term<VoidListener>)> {
        let current = &mut self.current;
        self.blocks
            .iter_mut()
            .flat_map(|block| {
                [
                    (block.id, true, &mut block.header),
                    (block.id, false, &mut block.output),
                ]
            })
            .chain(
                current
                    .header
                    .iter_mut()
                    .map(|header| (current.id, true, header)),
            )
    }

    /// The block shown at the top of the view after a screen clear, if any.
    pub fn pinned(&self) -> Option<BlockId> {
        if self.current.pin_top {
            return Some(self.current.id);
        }
        self.blocks
            .iter()
            .rev()
            .find(|block| block.pin_top)
            .map(|block| block.id)
    }

    /// Frozen rows held by the finished blocks and the current header.
    pub fn frozen_rows(&self) -> usize {
        self.blocks.iter().map(Block::rows).sum::<usize>()
            + self
                .current
                .header
                .as_ref()
                .map_or(0, |header| header.screen_lines())
    }

    /// Whether block `id` is folded.
    pub fn is_folded(&self, id: BlockId) -> bool {
        if id == self.current.id {
            return self.current.folded;
        }
        self.block(id).is_some_and(|block| block.folded)
    }

    /// Folds block `id` when it is unfolded, unfolds it otherwise.
    pub fn toggle_fold(&mut self, id: BlockId) {
        if id == self.current.id {
            self.current.folded = !self.current.folded;
        } else if let Some(block) = self.block_mut(id) {
            block.folded = !block.folded;
        }
    }

    /// Reflows block `id` (or the current header) when it was frozen with other columns
    /// than `columns`. Returns whether it changed.
    pub fn reflow(&mut self, id: BlockId, columns: usize) -> bool {
        if let Some(block) = self.block_mut(id) {
            let stale = block.header.columns() != columns || block.output.columns() != columns;
            block.reflow(columns);
            return stale;
        }
        match self.current.header.as_mut() {
            Some(header) if self.current.id == id && header.columns() != columns => {
                reflow(header, columns);
                true
            }
            _ => false,
        }
    }

    /// Folds every finished block when any of them is unfolded, otherwise unfolds them
    /// all.
    pub fn toggle_fold_all(&mut self) {
        let fold = self.blocks.iter().any(|block| !block.folded);
        for block in &mut self.blocks {
            block.folded = fold;
        }
    }

    /// Applies a chunk of the PTY stream to `term`, the session's terminal. `Bytes`
    /// must be passed after the parser handled them: they only release the signals
    /// held while the alternate screen was active.
    pub fn apply<L: EventListener>(&mut self, term: &mut Term<L>, chunk: Chunk<'_>) {
        let alt = term.mode().contains(TermMode::ALT_SCREEN);
        if !alt && !self.pending.is_empty() {
            for signal in mem::take(&mut self.pending) {
                self.signal(term, signal);
            }
        }
        match chunk {
            Chunk::Bytes(_) => {}
            Chunk::Signal(signal) if alt => self.pending.push(signal),
            Chunk::Signal(signal) => self.signal(term, signal),
            Chunk::Clear if alt => {}
            Chunk::Clear => self.clear(term),
        }
    }

    fn signal<L: EventListener>(&mut self, term: &mut Term<L>, signal: Signal) {
        match (signal, self.current.phase) {
            (Signal::Start { command }, Phase::Prompt) => {
                self.current.header = Some(detach(term, self.scrollback));
                self.current.command = command;
                self.current.phase = Phase::Running;
                self.current.editing = false;
            }
            (Signal::End { status }, Phase::Running) => {
                let output = detach(term, self.scrollback);
                let id = BlockId(self.next_id);
                self.next_id += 1;
                let current = mem::replace(&mut self.current, Current::new(id));
                let header = current.header.unwrap_or_else(|| empty_term(term.columns()));
                let mut block = Block {
                    id: current.id,
                    header,
                    output,
                    command: current.command,
                    status: Status::from_signal(status),
                    folded: current.folded,
                    scroll: current.scroll,
                    pin_top: current.pin_top,
                };
                // A `clear` command: its block is empty, the next prompt goes on top.
                if block.pin_top && block.output_is_empty() {
                    block.pin_top = false;
                    self.current.pin_top = true;
                }
                self.blocks.push_back(block);
                self.evict();
            }
            (Signal::Prompt, Phase::Prompt) => {
                let grid = term.grid();
                let cursor = grid.cursor.point;
                let line = (grid.history_size() as i32 + cursor.line.0).max(0) as usize;
                self.current.editing = true;
                self.current.prompt_start = Some((line, cursor.column));
            }
            // A start while running (missed end), an end at the prompt, a prompt while
            // running: ignored.
            _ => {}
        }
    }

    /// A screen clear on the main screen: what was on screen before is dropped from the
    /// live grid and the current block is shown at the top of the view.
    fn clear<L: EventListener>(&mut self, term: &mut Term<L>) {
        term.grid_mut().clear_history();
        // The prompt is redrawn at the top of the cleared screen.
        if self.current.prompt_start.is_some() {
            self.current.prompt_start = Some((0, Column(0)));
        }
        for block in &mut self.blocks {
            block.pin_top = false;
        }
        self.current.pin_top = true;
    }

    /// Changes the cap of the frozen rows, from a reloaded configuration: the oldest
    /// blocks are dropped at once when it is lowered.
    pub fn set_scrollback(&mut self, scrollback: usize) {
        self.scrollback = scrollback;
        self.evict();
    }

    /// Drops the oldest blocks, whole, while the frozen rows exceed the scrollback.
    fn evict(&mut self) {
        let mut rows = self.frozen_rows();
        while rows > self.scrollback {
            let Some(block) = self.blocks.pop_front() else {
                break;
            };
            rows -= block.rows();
        }
    }
}

/// Number of rows holding content in `grid`: the history, then the screen down to the
/// last non-blank row or the row where the cursor stands past the first column.
pub fn content_rows(grid: &Grid<Cell>) -> usize {
    let history = grid.history_size();
    let cursor = grid.cursor.point;
    let cursor_holds_text = cursor.column > Column(0) || grid.cursor.input_needs_wrap;
    let last = (0..grid.screen_lines()).rev().find(|&line| {
        let line = Line(line as i32);
        (cursor_holds_text && cursor.line == line) || !grid[line].is_clear()
    });
    history + last.map_or(0, |line| line + 1)
}

/// Moves the live grid of `term` into a terminal sized to its content rows, and gives
/// `term` a fresh grid of the same size whose cursor keeps the colors and charsets of
/// the old one, at the origin.
pub fn detach<L: EventListener>(term: &mut Term<L>, scrollback: usize) -> Term<VoidListener> {
    let lines = term.screen_lines();
    let columns = term.columns();
    let old = mem::replace(term.grid_mut(), Grid::new(lines, columns, scrollback));
    let cursor = &mut term.grid_mut().cursor;
    *cursor = old.cursor.clone();
    cursor.point = Point::new(Line(0), Column(0));
    cursor.input_needs_wrap = false;
    term.selection = None;
    freeze(old)
}

/// A terminal holding exactly the content rows of `grid`, with no history.
fn freeze(mut grid: Grid<Cell>) -> Term<VoidListener> {
    let columns = grid.columns();
    let history = grid.history_size();
    let rows = content_rows(&grid);
    let screen = rows.saturating_sub(history).max(1);

    // Drop the blank rows below the content, then pull the history into the screen.
    let cursor = &mut grid.cursor;
    cursor.point.line = cursor.point.line.min(Line(screen as i32 - 1));
    cursor.input_needs_wrap = false;
    grid.resize::<Color>(false, screen, columns);
    grid.resize::<Color>(false, history + screen, columns);
    grid.update_history(0);

    let mut term = empty_term_sized(columns, grid.screen_lines());
    *term.grid_mut() = grid;
    term
}

/// Reflows a frozen terminal for `columns`, keeping all its rows.
fn reflow(term: &mut Term<VoidListener>, columns: usize) {
    if term.columns() == columns {
        return;
    }
    let lines = term.screen_lines();
    let mut grid = mem::replace(term.grid_mut(), Grid::new(1, 1, 0));
    // Rows pushed out by the reflow go to the history instead of being lost.
    grid.update_history(lines * grid.columns());
    grid.cursor.point = Point::new(Line(lines as i32 - 1), Column(0));
    grid.resize::<Color>(true, lines, columns);
    *term = freeze(grid);
}

fn empty_term(columns: usize) -> Term<VoidListener> {
    empty_term_sized(columns, 1)
}

fn empty_term_sized(columns: usize, lines: usize) -> Term<VoidListener> {
    let config = Config {
        scrolling_history: 0,
        ..Config::default()
    };
    Term::new(config, &Size { columns, lines }, VoidListener)
}

/// Dimensions of a frozen terminal.
struct Size {
    columns: usize,
    lines: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// Test helpers shared with the view tests: a session fed through the scanner, the
/// parser and the timeline, and the signals the hooks send.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    use crate::signals::Scanner;

    pub const TOKEN: &str = "0123456789abcdef0123456789abcdef";
    pub const COLUMNS: usize = 20;
    pub const LINES: usize = 5;

    pub struct Session {
        pub term: Term<VoidListener>,
        pub timeline: Timeline,
        scanner: Scanner,
        parser: Processor<StdSyncHandler>,
    }

    impl Session {
        pub fn new(scrollback: usize) -> Self {
            Self::sized(COLUMNS, LINES, scrollback)
        }

        pub fn sized(columns: usize, lines: usize, scrollback: usize) -> Self {
            let config = Config {
                scrolling_history: scrollback,
                ..Config::default()
            };
            Session {
                term: Term::new(config, &TermSize::new(columns, lines), VoidListener),
                timeline: Timeline::new(scrollback),
                scanner: Scanner::new(TOKEN),
                parser: Processor::new(),
            }
        }

        pub fn feed(&mut self, bytes: &str) {
            let chunks: Vec<Chunk<'_>> = self.scanner.split(bytes.as_bytes()).collect();
            for chunk in chunks {
                if let Chunk::Bytes(bytes) = chunk {
                    self.parser.advance(&mut self.term, bytes);
                }
                self.timeline.apply(&mut self.term, chunk);
            }
        }

        /// Prompt, typed command, its output lines, then its end.
        pub fn command(&mut self, name: &str, output: &[&str], status: i32) {
            self.feed(&format!("$ {name}\r\n{}", start(name)));
            for line in output {
                self.feed(&format!("{line}\r\n"));
            }
            self.feed(&end(status));
        }

        pub fn live(&self) -> Vec<String> {
            texts(&self.term)
        }
    }

    pub fn start(command: &str) -> String {
        format!("\x1b]6973;{TOKEN};start;{}\x07", base64(command.as_bytes()))
    }

    pub fn base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for group in bytes.chunks(3) {
            let n = group
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
            for i in 0..4 {
                if i <= group.len() {
                    out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    pub fn end(status: i32) -> String {
        format!("\x1b]6973;{TOKEN};end;{status}\x07")
    }

    pub fn prompt() -> String {
        format!("\x1b]6973;{TOKEN};prompt\x07")
    }

    /// Every row of `term`, history first, trailing spaces trimmed.
    pub fn texts<L>(term: &Term<L>) -> Vec<String> {
        let grid = term.grid();
        let top = -(grid.history_size() as i32);
        (top..grid.screen_lines() as i32)
            .map(|line| {
                let row = &grid[Line(line)];
                (0..grid.columns())
                    .map(|column| row[Column(column)].c)
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use alacritty_terminal::vte::ansi::NamedColor;

    #[test]
    fn content_rows_ends_at_the_last_text_or_the_cursor_past_column_zero() {
        let mut session = Session::new(100);
        assert_eq!(content_rows(session.term.grid()), 0);
        session.feed("a\r\n\r\n");
        assert_eq!(content_rows(session.term.grid()), 1);
        session.feed("b");
        assert_eq!(content_rows(session.term.grid()), 3);
    }

    #[test]
    fn detach_keeps_content_rows_with_history_and_resets_the_live_grid() {
        let mut session = Session::new(100);
        session.feed("1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\n");
        assert_eq!(session.term.grid().history_size(), 3);

        let frozen = detach(&mut session.term, 100);

        assert_eq!(texts(&frozen), ["1", "2", "3", "4", "5", "6", "7"]);
        assert_eq!(frozen.grid().history_size(), 0);
        assert_eq!(frozen.columns(), COLUMNS);
        assert_eq!(session.live(), ["", "", "", "", ""]);
        assert_eq!(
            session.term.grid().cursor.point,
            Point::new(Line(0), Column(0))
        );
        assert_eq!(session.term.screen_lines(), LINES);
    }

    #[test]
    fn detach_carries_the_sgr_template_to_the_live_grid() {
        let mut session = Session::new(100);
        session.feed("x\x1b[31m");

        detach(&mut session.term, 100);
        session.feed("red");

        let cell = &session.term.grid()[Point::new(Line(0), Column(0))];
        assert_eq!(cell.c, 'r');
        assert_eq!(cell.fg, Color::Named(NamedColor::Red));
    }

    #[test]
    fn detach_of_an_empty_grid_gives_one_blank_row() {
        let mut session = Session::new(100);
        let frozen = detach(&mut session.term, 100);
        assert_eq!(frozen.screen_lines(), 1);
        assert_eq!(content_rows(frozen.grid()), 0);
    }

    #[test]
    fn start_and_end_split_header_and_output() {
        let mut session = Session::new(100);
        session.command("ls", &["a", "b"], 0);
        session.command("false", &[], 1);

        let blocks = session.timeline.blocks();
        assert_eq!(blocks.len(), 2);

        assert_eq!(blocks[0].id, BlockId(0));
        assert_eq!(texts(&blocks[0].header), ["$ ls"]);
        assert_eq!(texts(&blocks[0].output), ["a", "b"]);
        assert_eq!(blocks[0].command, "ls");
        assert_eq!(blocks[0].status, Status::Success);

        assert_eq!(texts(&blocks[1].header), ["$ false"]);
        assert!(blocks[1].output_is_empty());
        assert_eq!(blocks[1].status, Status::Failure(1));

        let current = session.timeline.current();
        assert_eq!(current.id, BlockId(2));
        assert_eq!(current.phase, Phase::Prompt);
        assert!(current.header.is_none());
    }

    #[test]
    fn prompt_signal_starts_editing_until_the_command_runs() {
        let mut session = Session::new(100);
        session.feed("motd\r\n");
        assert!(!session.timeline.current().editing);
        assert_eq!(session.timeline.current().prompt_start(&session.term), None);

        session.feed(&format!("{}$ ls", prompt()));
        let current = session.timeline.current();
        assert!(current.editing);
        assert_eq!(
            current.prompt_start(&session.term),
            Some(Point::new(Line(1), Column(0)))
        );

        session.feed(&format!("\r\n{}", start("ls")));
        assert!(!session.timeline.current().editing);

        // A prompt while the command runs is ignored.
        session.feed(&prompt());
        assert!(!session.timeline.current().editing);

        // The next block edits again only once its prompt is signalled.
        session.feed(&end(0));
        assert!(!session.timeline.current().editing);
        session.feed(&format!("{}$ ", prompt()));
        assert!(session.timeline.current().editing);
    }

    #[test]
    fn prompt_start_follows_the_screen_scroll_and_the_clear() {
        let mut session = Session::new(100);
        session.feed(&format!("{}$ a\r\n> b", prompt()));
        // Five more lines scroll the prompt into the history.
        session.feed("\r\n\r\n\r\n\r\n\r\n");
        let top = -(session.term.grid().history_size() as i32);
        assert_eq!(top, -2);
        assert_eq!(
            session.timeline.current().prompt_start(&session.term),
            Some(Point::new(Line(-2), Column(0)))
        );

        session.feed("\x1b[H\x1b[2J\x1b[3J$ ");
        assert_eq!(
            session.timeline.current().prompt_start(&session.term),
            Some(Point::new(Line(0), Column(0)))
        );
    }

    #[test]
    fn end_without_status_is_unknown() {
        let mut session = Session::new(100);
        session.feed(&format!("$ x\r\n{}", start("x")));
        session.feed(&format!("\x1b]6973;{TOKEN};end\x07"));
        assert_eq!(session.timeline.blocks()[0].status, Status::Unknown);
    }

    #[test]
    fn end_at_the_prompt_is_ignored() {
        let mut session = Session::new(100);
        session.feed("$ ");
        session.feed(&end(0));

        assert!(session.timeline.blocks().is_empty());
        assert_eq!(session.timeline.current().phase, Phase::Prompt);
        assert_eq!(session.live()[0], "$");
    }

    #[test]
    fn start_while_running_is_ignored() {
        let mut session = Session::new(100);
        session.feed(&format!("$ a\r\n{}", start("a")));
        session.feed(&format!("out\r\n{}", start("b")));

        let current = session.timeline.current();
        assert_eq!(current.phase, Phase::Running);
        assert_eq!(current.command, "a");
        assert_eq!(session.live()[0], "out");
    }

    #[test]
    fn running_block_has_its_header_frozen_and_output_live() {
        let mut session = Session::new(100);
        session.feed(&format!("$ top\r\n{}progress", start("top")));

        let current = session.timeline.current();
        assert_eq!(current.phase, Phase::Running);
        assert_eq!(texts(current.header.as_ref().unwrap()), ["$ top"]);
        assert_eq!(session.live()[0], "progress");
    }

    #[test]
    fn without_signals_one_block_keeps_growing() {
        let mut session = Session::new(100);
        for line in 0..20 {
            session.feed(&format!("$ cmd {line}\r\nout\r\n"));
        }
        assert!(session.timeline.blocks().is_empty());
        assert_eq!(session.timeline.current().phase, Phase::Prompt);
        assert_eq!(content_rows(session.term.grid()), 40);
    }

    #[test]
    fn signals_on_the_alternate_screen_wait_until_it_is_left() {
        let mut session = Session::new(100);
        session.feed(&format!("$ vim\r\n{}", start("vim")));
        session.feed("\x1b[?1049h");
        session.feed(&format!("screen{}", end(0)));

        assert!(session.timeline.blocks().is_empty());
        assert_eq!(session.timeline.current().phase, Phase::Running);

        session.feed("\x1b[?1049l");

        assert_eq!(session.timeline.blocks().len(), 1);
        assert_eq!(session.timeline.blocks()[0].status, Status::Success);
        assert_eq!(session.timeline.current().phase, Phase::Prompt);
    }

    #[test]
    fn clear_on_the_alternate_screen_does_not_pin() {
        let mut session = Session::new(100);
        session.feed("\x1b[?1049h\x1b[2J\x1b[?1049l");
        assert_eq!(session.timeline.pinned(), None);
    }

    #[test]
    fn clear_command_pins_the_next_prompt() {
        let mut session = Session::new(100);
        session.command("ls", &["a"], 0);
        session.feed(&format!("$ clear\r\n{}", start("clear")));
        session.feed("\x1b[H\x1b[2J\x1b[3J");
        assert_eq!(session.timeline.pinned(), Some(BlockId(1)));

        session.feed(&end(0));

        let blocks = session.timeline.blocks();
        assert!(blocks[1].output_is_empty());
        assert!(!blocks[1].pin_top);
        assert_eq!(session.timeline.pinned(), Some(BlockId(2)));
    }

    #[test]
    fn command_that_clears_then_prints_stays_pinned() {
        let mut session = Session::new(100);
        session.feed(&format!("$ watch\r\n{}", start("watch")));
        session.feed("old\r\n\x1b[H\x1b[2Jnew\r\n");
        session.feed(&end(0));

        let block = &session.timeline.blocks()[0];
        assert!(block.pin_top);
        assert_eq!(texts(&block.output), ["new"]);
        assert_eq!(session.timeline.pinned(), Some(BlockId(0)));
        assert!(!session.timeline.current().pin_top);
    }

    #[test]
    fn ctrl_l_at_the_prompt_pins_the_current_block_and_drops_the_old_prompt() {
        let mut session = Session::new(100);
        session.command("ls", &["a"], 0);
        session.feed("$ ");
        session.feed("\x1b[H\x1b[2J$ ");

        assert_eq!(session.timeline.pinned(), Some(BlockId(1)));
        assert_eq!(session.term.grid().history_size(), 0);

        session.command("pwd", &["/"], 0);
        let block = &session.timeline.blocks()[1];
        assert_eq!(texts(&block.header), ["$ $ pwd"]);
        assert!(block.pin_top);
    }

    #[test]
    fn a_new_clear_moves_the_pin() {
        let mut session = Session::new(100);
        session.feed("\x1b[2J");
        session.command("a", &["x"], 0);
        assert_eq!(session.timeline.pinned(), Some(BlockId(0)));

        session.feed("\x1b[2J");
        assert_eq!(session.timeline.pinned(), Some(BlockId(1)));
        assert!(!session.timeline.blocks()[0].pin_top);
    }

    #[test]
    fn eviction_drops_the_oldest_blocks_whole() {
        let mut session = Session::new(10);
        for name in ["a", "b", "c", "d"] {
            session.command(name, &["1", "2"], 0);
        }

        // Each block holds 3 rows: 10 rows keep the last 3 blocks.
        let ids: Vec<BlockId> = session.timeline.blocks().iter().map(|b| b.id).collect();
        assert_eq!(ids, [BlockId(1), BlockId(2), BlockId(3)]);
        assert!(session.timeline.frozen_rows() <= 10);
    }

    #[test]
    fn lowering_the_scrollback_evicts_at_once() {
        let mut session = Session::new(100);
        for name in ["a", "b", "c", "d"] {
            session.command(name, &["1", "2"], 0);
        }
        assert_eq!(session.timeline.blocks().len(), 4);

        session.timeline.set_scrollback(6);

        let ids: Vec<BlockId> = session.timeline.blocks().iter().map(|b| b.id).collect();
        assert_eq!(ids, [BlockId(2), BlockId(3)]);
    }

    #[test]
    fn fold_toggles_per_block_and_for_all_finished_blocks() {
        let mut session = Session::new(100);
        session.command("a", &["x"], 0);
        session.command("b", &["y"], 0);

        session.timeline.toggle_fold(BlockId(0));
        assert!(session.timeline.is_folded(BlockId(0)));
        assert!(!session.timeline.is_folded(BlockId(1)));

        session.timeline.toggle_fold_all();
        assert!(session.timeline.is_folded(BlockId(0)));
        assert!(session.timeline.is_folded(BlockId(1)));
        assert!(!session.timeline.is_folded(BlockId(2)));

        session.timeline.toggle_fold_all();
        assert!(!session.timeline.is_folded(BlockId(0)));
        assert!(!session.timeline.is_folded(BlockId(1)));

        session.timeline.toggle_fold(BlockId(2));
        assert!(session.timeline.is_folded(BlockId(2)));
    }

    #[test]
    fn current_fold_carries_to_the_finished_block() {
        let mut session = Session::new(100);
        session.feed(&format!("$ a\r\n{}", start("a")));
        session.timeline.toggle_fold(BlockId(0));
        session.feed(&format!("x\r\n{}", end(0)));
        assert!(session.timeline.blocks()[0].folded);
    }

    #[test]
    fn reflow_narrows_and_widens_a_frozen_block_without_losing_rows() {
        let mut session = Session::new(100);
        session.command("echo", &["0123456789abcdefghij0123456789"], 0);
        let block = session.timeline.block_mut(BlockId(0)).unwrap();
        assert_eq!(texts(&block.output), ["0123456789abcdefghij", "0123456789"]);

        block.reflow(10);
        assert_eq!(block.output.columns(), 10);
        assert_eq!(
            texts(&block.output),
            ["0123456789", "abcdefghij", "0123456789"]
        );
        assert_eq!(texts(&block.header), ["$ echo"]);

        block.reflow(40);
        assert_eq!(texts(&block.output), ["0123456789abcdefghij0123456789"]);
    }
}
