//! Shell state built from the OSC 133 marks: command blocks, their status, and the cell
//! tags that anchor them in the grid.
//!
//! A block is anchored by tagging the cells the shell writes: while the prompt or the
//! input is drawn, the cursor template carries an OSC 8 hyperlink with the reserved
//! scheme [`BLOCK_LINK_SCHEME`], so every written cell gets it. Cells keep it through
//! scroll, history eviction and reflow, so the rows of a block are found again from the
//! grid alone ([`index`], [`block_bounds`]), with no line number to keep up to date.

use std::collections::BTreeMap;
use std::ops::Range;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Dimensions, Row};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::{Cell, Hyperlink};
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::Handler;

use crate::osc133::Mark;

/// URI scheme of the hyperlinks used as block tags. Not a real link: the links feature
/// must ignore it.
pub const BLOCK_LINK_SCHEME: &str = "graphene-block";

/// Max number of known blocks, the default scrollback size: older blocks have left the
/// history by then.
const MAX_BLOCKS: usize = 10_000;

/// Identifier of a command block, increasing with time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u64);

/// What the shell is drawing or doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// No block in progress: before the first prompt, or after a command end.
    #[default]
    Idle,
    /// The prompt is drawn.
    Prompt,
    /// The user types the command.
    Input,
    /// The command runs.
    Running,
}

/// Status of a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The command runs.
    Running,
    /// The command exited with 0.
    Success,
    /// The command exited with this non-zero code.
    Failure(i32),
    /// No command ran yet, or its end was never reported.
    Unknown,
}

/// What is known about a block besides its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockInfo {
    pub status: Status,
}

/// Role of a grid row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowRole {
    /// Before the first block.
    None,
    /// Starts with the prompt of this block.
    Prompt(BlockId),
    /// Starts with the command of this block.
    Input(BlockId),
    /// Output of this block (any row with no tag after it).
    Output(BlockId),
}

impl RowRole {
    pub fn block(self) -> Option<BlockId> {
        match self {
            Self::None => None,
            Self::Prompt(id) | Self::Input(id) | Self::Output(id) => Some(id),
        }
    }
}

/// Where a block lies in the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code, reason = "read by the block selection and copy actions")]
pub struct BlockBounds {
    /// First row of the prompt.
    pub first_row: Line,
    /// First cell of the command. Before anything is typed, the cell after the prompt.
    pub input_start: Point,
    /// Last cell of the command, `None` when nothing was typed.
    pub input_end: Option<Point>,
    /// Rows after the command, up to the next block, or up to the cursor row for the
    /// last block. Empty when the command printed nothing yet.
    pub output: Range<Line>,
}

/// Kind of tagged cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TagKind {
    Prompt,
    /// Right prompt: a prompt cell, kept apart so it is never taken as the input start.
    RightPrompt,
    Input,
}

impl TagKind {
    fn name(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::RightPrompt => "right-prompt",
            Self::Input => "input",
        }
    }
}

/// Shell integration state of one terminal. Lives behind the `Term` lock order:
/// lock `Term` first, then this.
#[derive(Debug, Default)]
pub struct ShellState {
    pub phase: Phase,
    current: Option<BlockId>,
    next_id: u64,
    blocks: BTreeMap<BlockId, BlockInfo>,
    /// Tag the cursor template carries in `Prompt` / `Input`.
    tag: Option<Hyperlink>,
}

impl ShellState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Block being drawn or run, or the last one.
    pub fn current(&self) -> Option<BlockId> {
        self.current
    }

    pub fn block(&self, id: BlockId) -> Option<&BlockInfo> {
        self.blocks.get(&id)
    }

    /// Known blocks, oldest first.
    pub fn blocks(&self) -> impl DoubleEndedIterator<Item = (BlockId, &BlockInfo)> {
        self.blocks.iter().map(|(id, info)| (*id, info))
    }

    /// Applies a mark at the current cursor position. Ignored on the alternate screen.
    pub fn apply<L: EventListener>(&mut self, term: &mut Term<L>, mark: Mark) {
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return;
        }
        match mark {
            Mark::PromptStart => self.prompt_start(term),
            Mark::RightPrompt => {
                self.ensure_block();
                self.set_tag(term, TagKind::RightPrompt);
            }
            Mark::InputStart => {
                self.ensure_block();
                self.set_tag(term, TagKind::Input);
            }
            Mark::OutputStart => {
                if matches!(self.phase, Phase::Prompt | Phase::Input) {
                    self.tag = None;
                    clear_our_tag(term);
                    self.set_status(Status::Running);
                    self.phase = Phase::Running;
                }
            }
            Mark::CommandEnd(code) => {
                // Only the end of a command that ran: a shell may report a status for
                // an empty command line too.
                if self.phase == Phase::Running {
                    clear_our_tag(term);
                    self.set_status(match code {
                        Some(0) => Status::Success,
                        Some(code) => Status::Failure(code),
                        None => Status::Unknown,
                    });
                    self.phase = Phase::Idle;
                }
            }
            Mark::Hyperlink => self.after_bytes(term),
        }
    }

    /// Keeps the template tag right after parsed bytes: applied again in `Prompt` /
    /// `Input` when something removed it (a real OSC 8 link closed inside the prompt, a
    /// reset, a cursor restore), dropped outside them.
    pub fn after_bytes<L: EventListener>(&mut self, term: &mut Term<L>) {
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return;
        }
        let template = &mut term.grid_mut().cursor.template;
        let link = template.hyperlink();
        match (self.phase, &self.tag) {
            (Phase::Prompt | Phase::Input, Some(tag)) => {
                let stale = link
                    .as_ref()
                    .is_none_or(|link| is_ours(link) && link != tag);
                if stale {
                    template.set_hyperlink(Some(tag.clone()));
                }
            }
            _ => {
                if link.as_ref().is_some_and(is_ours) {
                    template.set_hyperlink(None);
                }
            }
        }
    }

    fn prompt_start<L: EventListener>(&mut self, term: &mut Term<L>) {
        // Shells draw the prompt again in place (resize, `reset-prompt`, clear line):
        // the block goes on when its cells are still under the cursor.
        let redraw = matches!(self.phase, Phase::Prompt | Phase::Input)
            && self.current.is_some_and(|id| tagged_from_cursor(term, id));
        if !redraw {
            // `A` implies a fresh line: every block starts on its own row.
            if term.grid().cursor.point.column != Column(0) {
                term.carriage_return();
                term.linefeed();
            }
            self.start_block();
        }
        self.set_tag(term, TagKind::Prompt);
    }

    /// Starts a block on `B` / right prompt marks without a prompt start before them
    /// (a theme that rebuilt the prompt after it was wrapped).
    fn ensure_block(&mut self) {
        if !(matches!(self.phase, Phase::Prompt | Phase::Input) && self.current.is_some()) {
            self.start_block();
        }
    }

    fn start_block(&mut self) {
        if let Some(info) = self.current.and_then(|id| self.blocks.get_mut(&id))
            && info.status == Status::Running
        {
            info.status = Status::Unknown;
        }
        let id = BlockId(self.next_id);
        self.next_id += 1;
        self.blocks.insert(
            id,
            BlockInfo {
                status: Status::Unknown,
            },
        );
        while self.blocks.len() > MAX_BLOCKS {
            self.blocks.pop_first();
        }
        self.current = Some(id);
    }

    fn set_status(&mut self, status: Status) {
        if let Some(info) = self.current.and_then(|id| self.blocks.get_mut(&id)) {
            info.status = status;
        }
    }

    fn set_tag<L: EventListener>(&mut self, term: &mut Term<L>, kind: TagKind) {
        let Some(id) = self.current else { return };
        let tag = Hyperlink::new(
            Some(format!("{BLOCK_LINK_SCHEME}-{}-{}", id.0, kind.name())),
            format!("{BLOCK_LINK_SCHEME}://{}/{}", id.0, kind.name()),
        );
        self.phase = match kind {
            TagKind::Prompt | TagKind::RightPrompt => Phase::Prompt,
            TagKind::Input => Phase::Input,
        };
        // A real link opened by the prompt is kept: the tag comes back when it closes.
        let template = &mut term.grid_mut().cursor.template;
        if template.hyperlink().as_ref().is_none_or(is_ours) {
            template.set_hyperlink(Some(tag.clone()));
        }
        self.tag = Some(tag);
    }
}

/// Removes the template tag, never a real link.
fn clear_our_tag<L: EventListener>(term: &mut Term<L>) {
    let template = &mut term.grid_mut().cursor.template;
    if template.hyperlink().as_ref().is_some_and(is_ours) {
        template.set_hyperlink(None);
    }
}

fn is_ours(link: &Hyperlink) -> bool {
    parse_tag(link.uri()).is_some()
}

fn parse_tag(uri: &str) -> Option<(BlockId, TagKind)> {
    let rest = uri.strip_prefix(BLOCK_LINK_SCHEME)?.strip_prefix("://")?;
    let (id, kind) = rest.split_once('/')?;
    let kind = match kind {
        "prompt" => TagKind::Prompt,
        "right-prompt" => TagKind::RightPrompt,
        "input" => TagKind::Input,
        _ => return None,
    };
    Some((BlockId(id.parse().ok()?), kind))
}

fn cell_tag(cell: &Cell) -> Option<(BlockId, TagKind)> {
    // Most cells carry no extra: skip them without touching the hyperlink.
    cell.extra.as_ref()?;
    parse_tag(cell.hyperlink()?.uri())
}

/// Tag of the first tagged cell of the row.
fn row_tag(row: &Row<Cell>) -> Option<(BlockId, TagKind)> {
    row[..].iter().find_map(cell_tag)
}

/// Whether a cell of block `id` lies on the cursor row or below it.
fn tagged_from_cursor<L>(term: &Term<L>, id: BlockId) -> bool {
    let grid = term.grid();
    let cursor = grid.cursor.point.line.0;
    (cursor..=grid.bottommost_line().0).any(|line| {
        grid[Line(line)][..]
            .iter()
            .any(|cell| cell_tag(cell).is_some_and(|(block, _)| block == id))
    })
}

/// Roles of the rows in `lines`, a range of valid grid lines (history lines are
/// negative). A row takes the tag of its first tagged cell; a row with no tag continues
/// the block above it as output.
pub fn index<L>(term: &Term<L>, lines: Range<Line>) -> Vec<RowRole> {
    let grid = term.grid();
    debug_assert!(lines.start >= grid.topmost_line());
    debug_assert!(lines.end <= grid.bottommost_line() + 1);

    // A range starting inside an output: find its block above.
    let mut block = (grid.topmost_line().0..lines.start.0)
        .rev()
        .find_map(|line| row_tag(&grid[Line(line)]))
        .map(|(id, _)| id);

    (lines.start.0..lines.end.0)
        .map(|line| match row_tag(&grid[Line(line)]) {
            Some((id, TagKind::Input)) => {
                block = Some(id);
                RowRole::Input(id)
            }
            Some((id, TagKind::Prompt | TagKind::RightPrompt)) => {
                block = Some(id);
                RowRole::Prompt(id)
            }
            None => block.map_or(RowRole::None, RowRole::Output),
        })
        .collect()
}

/// Where block `id` lies in the grid, `None` when none of its rows is left.
#[allow(dead_code, reason = "read by the block selection and copy actions")]
pub fn block_bounds<L>(term: &Term<L>, id: BlockId) -> Option<BlockBounds> {
    let grid = term.grid();
    let top = grid.topmost_line();
    let roles = index(term, top..grid.bottommost_line() + 1);
    let line_at = |row: usize| Line(top.0 + row as i32);
    let is_head = |role: RowRole| matches!(role, RowRole::Prompt(b) | RowRole::Input(b) if b == id);

    let first = roles.iter().position(|role| is_head(*role))?;
    let span_end = roles[first..]
        .iter()
        .position(|role| role.block() != Some(id))
        .map_or(roles.len(), |len| first + len);
    let last_head = (first..span_end).rev().find(|&row| is_head(roles[row]))?;

    let mut input_start = None;
    let mut input_end = None;
    let mut after_prompt = None;
    for row in first..=last_head {
        let line = line_at(row);
        for (column, cell) in grid[line][..].iter().enumerate() {
            let point = Point::new(line, Column(column));
            match cell_tag(cell) {
                Some((block, TagKind::Input)) if block == id => {
                    input_start.get_or_insert(point);
                    input_end = Some(point);
                }
                Some((block, TagKind::Prompt)) if block == id => {
                    after_prompt = Some(next_cell(term, point));
                }
                _ => {}
            }
        }
    }
    let input_start = input_start
        .or(after_prompt)
        .unwrap_or(Point::new(line_at(first), Column(0)));

    let output_start = line_at(last_head + 1);
    let output_end = if span_end < roles.len() {
        line_at(span_end)
    } else {
        grid.cursor.point.line + 1
    };

    Some(BlockBounds {
        first_row: line_at(first),
        input_start,
        input_end,
        output: output_start..output_end.max(output_start),
    })
}

/// The cell after `point`, wrapping to the next row; `point` itself at the grid end.
#[allow(dead_code, reason = "read by the block selection and copy actions")]
fn next_cell<L>(term: &Term<L>, point: Point) -> Point {
    if point.column + 1 < term.columns() {
        Point::new(point.line, point.column + 1)
    } else if point.line < term.bottommost_line() {
        Point::new(point.line + 1, Column(0))
    } else {
        point
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc133::Scanner;
    use crate::pty;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::Processor;

    const A: &str = "\x1b]133;A\x07";
    const B: &str = "\x1b]133;B\x07";
    const C: &str = "\x1b]133;C\x07";
    const RIGHT: &str = "\x1b]133;P;k=r\x07";

    fn d(code: i32) -> String {
        format!("\x1b]133;D;{code}\x07")
    }

    struct Harness {
        term: Term<VoidListener>,
        shell: ShellState,
        scanner: Scanner,
        processor: Processor,
    }

    impl Harness {
        fn new(columns: usize, lines: usize) -> Self {
            Self::with_history(columns, lines, 100)
        }

        fn with_history(columns: usize, lines: usize, history: usize) -> Self {
            let config = Config {
                scrolling_history: history,
                ..Config::default()
            };
            Self {
                term: Term::new(config, &TermSize::new(columns, lines), VoidListener),
                shell: ShellState::new(),
                scanner: Scanner::new(),
                processor: Processor::new(),
            }
        }

        fn feed(&mut self, bytes: &str) -> &mut Self {
            pty::process(
                &mut self.scanner,
                &mut self.processor,
                &mut self.shell,
                &mut self.term,
                bytes.as_bytes(),
            );
            self
        }

        /// One full command: prompt, input, output, end.
        fn command(&mut self, prompt: &str, input: &str, output: &str, code: i32) -> &mut Self {
            let end = d(code);
            self.feed(&format!("{A}{prompt}{B}{input}\r\n{C}{output}{end}"))
        }

        fn tag(&self, line: i32, column: usize) -> Option<(BlockId, TagKind)> {
            cell_tag(&self.term.grid()[Point::new(Line(line), Column(column))])
        }

        fn text(&self, line: i32) -> String {
            let row = &self.term.grid()[Line(line)];
            row[..]
                .iter()
                .map(|cell| cell.c)
                .collect::<String>()
                .trim_end()
                .to_owned()
        }

        fn screen_roles(&self) -> Vec<RowRole> {
            let lines = self.term.screen_lines() as i32;
            index(&self.term, Line(0)..Line(lines))
        }
    }

    const ID0: BlockId = BlockId(0);
    const ID1: BlockId = BlockId(1);

    #[test]
    fn tags_prompt_and_input_cells_but_not_output() {
        let mut h = Harness::new(20, 6);
        h.command("$ ", "ls", "out\r\n", 0);

        assert_eq!(h.text(0), "$ ls");
        assert_eq!(h.tag(0, 0), Some((ID0, TagKind::Prompt)));
        assert_eq!(h.tag(0, 1), Some((ID0, TagKind::Prompt)));
        assert_eq!(h.tag(0, 2), Some((ID0, TagKind::Input)));
        assert_eq!(h.tag(0, 3), Some((ID0, TagKind::Input)));
        assert_eq!(h.tag(0, 4), None);
        assert_eq!(h.tag(1, 0), None, "output is not tagged");
        assert_eq!(h.shell.block(ID0).unwrap().status, Status::Success);
        assert_eq!(h.shell.phase, Phase::Idle);
        assert!(h.term.grid().cursor.template.hyperlink().is_none());
    }

    #[test]
    fn statuses_follow_the_marks() {
        let mut h = Harness::new(20, 6);
        h.feed(&format!("{A}$ {B}"));
        assert_eq!(h.shell.phase, Phase::Input);
        assert_eq!(h.shell.block(ID0).unwrap().status, Status::Unknown);

        h.feed(&format!("false\r\n{C}"));
        assert_eq!(h.shell.phase, Phase::Running);
        assert_eq!(h.shell.block(ID0).unwrap().status, Status::Running);

        h.feed(&d(1));
        assert_eq!(h.shell.block(ID0).unwrap().status, Status::Failure(1));
        assert_eq!(h.shell.phase, Phase::Idle);

        h.feed(&format!("{A}$ {B}true\r\n{C}\x1b]133;D\x07"));
        assert_eq!(h.shell.block(ID1).unwrap().status, Status::Unknown);
    }

    #[test]
    fn running_block_becomes_unknown_when_a_new_prompt_starts() {
        let mut h = Harness::new(20, 6);
        h.feed(&format!("{A}$ {B}sleep\r\n{C}"));
        h.feed(&format!("{A}$ {B}"));

        assert_eq!(h.shell.block(ID0).unwrap().status, Status::Unknown);
        assert_eq!(h.shell.current(), Some(ID1));
    }

    #[test]
    fn command_end_without_a_command_is_ignored() {
        let mut h = Harness::new(20, 6);
        h.feed(&format!("{A}$ {B}"));
        h.feed(&d(130));

        assert_eq!(h.shell.phase, Phase::Input);
        assert_eq!(h.shell.block(ID0).unwrap().status, Status::Unknown);
        h.feed("x");
        assert_eq!(h.tag(0, 2), Some((ID0, TagKind::Input)));
    }

    #[test]
    fn prompt_start_mid_row_starts_a_fresh_line() {
        let mut h = Harness::new(20, 6);
        h.command("$ ", "printf x", "x", 0);
        h.feed(&format!("{A}$ "));

        assert_eq!(h.text(1), "x");
        assert_eq!(h.text(2), "$");
        assert_eq!(h.tag(2, 0), Some((ID1, TagKind::Prompt)));
    }

    #[test]
    fn prompt_start_at_column_zero_adds_no_line() {
        let mut h = Harness::new(20, 6);
        h.command("$ ", "ls", "out\r\n", 0);
        h.feed(&format!("{A}$ "));

        assert_eq!(h.text(2), "$");
    }

    #[test]
    fn prompt_redrawn_in_place_keeps_its_block() {
        let mut h = Harness::new(20, 6);
        h.feed(&format!("{A}$ {B}ab"));
        // `reset-prompt`: back to the prompt start, drawn again.
        h.feed(&format!("\r{A}$ {B}ab\x1b[K"));

        assert_eq!(h.shell.current(), Some(ID0));
        assert_eq!(h.tag(0, 0), Some((ID0, TagKind::Prompt)));
        assert_eq!(h.tag(0, 2), Some((ID0, TagKind::Input)));
    }

    #[test]
    fn empty_command_line_starts_a_new_block() {
        let mut h = Harness::new(20, 6);
        h.feed(&format!("{A}$ {B}\r\n{A}$ {B}"));

        assert_eq!(h.shell.current(), Some(ID1));
        assert_eq!(
            h.screen_roles()[..2],
            [RowRole::Prompt(ID0), RowRole::Prompt(ID1)]
        );
    }

    #[test]
    fn input_start_without_prompt_start_opens_a_block() {
        let mut h = Harness::new(20, 6);
        h.feed(&format!("$ {B}ls"));

        assert_eq!(h.shell.current(), Some(ID0));
        assert_eq!(h.shell.phase, Phase::Input);
        assert_eq!(h.tag(0, 0), None);
        assert_eq!(h.tag(0, 2), Some((ID0, TagKind::Input)));
        assert_eq!(h.screen_roles()[0], RowRole::Input(ID0));
    }

    #[test]
    fn right_prompt_cells_are_prompt_cells() {
        let mut h = Harness::new(20, 6);
        h.feed(&format!("{A}$ {B}\x1b[15G{RIGHT}10:00{B}\x1b[3G"));

        assert_eq!(h.tag(0, 14), Some((ID0, TagKind::RightPrompt)));
        assert_eq!(h.shell.phase, Phase::Input);
        h.feed("ls");
        assert_eq!(h.tag(0, 2), Some((ID0, TagKind::Input)));
        assert_eq!(h.screen_roles()[0], RowRole::Prompt(ID0));
    }

    #[test]
    fn right_prompt_alone_opens_a_block() {
        let mut h = Harness::new(20, 6);
        h.feed(&format!("$ \x1b[15G{RIGHT}10:00{B}\x1b[3G"));

        assert_eq!(h.shell.current(), Some(ID0));
        assert_eq!(h.tag(0, 14), Some((ID0, TagKind::RightPrompt)));
        assert_eq!(h.shell.phase, Phase::Input);
    }

    #[test]
    fn tag_comes_back_after_a_link_closed_inside_the_prompt() {
        let mut h = Harness::new(30, 6);
        h.feed(&format!(
            "{A}\x1b]8;;file:///home\x1b\\~\x1b]8;;\x1b\\ $ {B}ls"
        ));

        let link = h.term.grid()[Point::new(Line(0), Column(0))]
            .hyperlink()
            .unwrap();
        assert_eq!(link.uri(), "file:///home", "the real link is kept");
        assert_eq!(h.tag(0, 1), Some((ID0, TagKind::Prompt)));
        assert_eq!(h.tag(0, 2), Some((ID0, TagKind::Prompt)));
        assert_eq!(h.tag(0, 4), Some((ID0, TagKind::Input)));
    }

    #[test]
    fn tag_comes_back_after_a_reset_inside_the_prompt() {
        let mut h = Harness::new(20, 6);
        h.feed(&format!("{A}\x1b[0m$ {B}\x1bc"));
        h.feed("ls");

        assert_eq!(h.tag(0, 0), Some((ID0, TagKind::Input)));
    }

    #[test]
    fn real_link_open_at_output_start_is_kept() {
        let mut h = Harness::new(30, 6);
        h.feed(&format!("{A}$ {B}ls\r\n\x1b]8;;https://x.org\x1b\\{C}x"));

        let cell = &h.term.grid()[Point::new(Line(1), Column(0))];
        assert_eq!(cell.hyperlink().unwrap().uri(), "https://x.org");
    }

    #[test]
    fn stale_tag_restored_outside_a_prompt_is_dropped() {
        let mut h = Harness::new(20, 6);
        // Cursor saved inside the prompt, restored by the command output.
        h.feed(&format!("{A}\x1b7$ {B}ls\r\n{C}\x1b8"));
        h.feed("\r\n\r\nout");

        assert_eq!(h.tag(2, 0), None);
    }

    #[test]
    fn marks_are_ignored_on_the_alternate_screen() {
        let mut h = Harness::new(20, 6);
        h.feed(&format!("{A}$ {B}vim\r\n{C}\x1b[?1049h{A}x{B}y"));

        assert_eq!(h.shell.current(), Some(ID0));
        assert_eq!(h.shell.phase, Phase::Running);
        assert_eq!(h.tag(0, 0), None, "alternate screen cells");
        h.feed("\x1b[?1049l");
        assert_eq!(h.tag(0, 0), Some((ID0, TagKind::Prompt)));
    }

    #[test]
    fn index_assigns_roles_to_rows() {
        let mut h = Harness::new(20, 8);
        h.feed("motd\r\n");
        h.command("$ ", "ls", "a\r\nb\r\n", 0);
        h.feed(&format!("{A}user\r\n$ {B}echo \\\r\n> x"));

        assert_eq!(
            h.screen_roles(),
            [
                RowRole::None,
                RowRole::Prompt(ID0),
                RowRole::Output(ID0),
                RowRole::Output(ID0),
                RowRole::Prompt(ID1),
                RowRole::Prompt(ID1),
                RowRole::Input(ID1),
                RowRole::Output(ID1),
            ]
        );
    }

    #[test]
    fn index_starting_inside_an_output_finds_its_block() {
        let mut h = Harness::new(20, 8);
        h.command("$ ", "ls", "a\r\nb\r\nc\r\n", 0);

        assert_eq!(index(&h.term, Line(2)..Line(4)), [RowRole::Output(ID0); 2]);
        assert_eq!(h.screen_roles()[0], RowRole::Prompt(ID0));
    }

    #[test]
    fn index_follows_scroll_into_the_history() {
        let mut h = Harness::new(20, 4);
        h.command("$ ", "seq", "1\r\n2\r\n3\r\n4\r\n5\r\n", 0);
        h.feed(&format!("{A}$ "));

        let top = h.term.topmost_line();
        assert_eq!(top, Line(-3));
        let roles = index(&h.term, top..Line(4));
        assert_eq!(roles[0], RowRole::Prompt(ID0));
        assert_eq!(roles[1..6], [RowRole::Output(ID0); 5]);
        assert_eq!(roles[6], RowRole::Prompt(ID1));
    }

    #[test]
    fn index_after_history_eviction() {
        let mut h = Harness::with_history(20, 4, 2);
        h.command("$ ", "seq", "1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n", 0);

        // The prompt row left the history: the rest has no block above it.
        let top = h.term.topmost_line();
        assert!(
            index(&h.term, top..Line(4))
                .iter()
                .all(|role| *role == RowRole::None)
        );

        h.feed(&format!("{A}$ {B}"));
        let roles = index(&h.term, h.term.topmost_line()..Line(4));
        assert_eq!(roles.last(), Some(&RowRole::Prompt(ID1)));
        assert_eq!(block_bounds(&h.term, ID0), None);
    }

    #[test]
    fn index_after_a_resize_with_reflow() {
        let mut h = Harness::new(10, 6);
        h.feed(&format!("{A}$ {B}echo abcdefghij\r\n{C}out\r\n"));
        // The command wraps on two rows.
        assert_eq!(
            h.screen_roles()[..3],
            [
                RowRole::Prompt(ID0),
                RowRole::Input(ID0),
                RowRole::Output(ID0)
            ]
        );

        h.term.resize(TermSize::new(30, 6));
        let roles = index(&h.term, h.term.topmost_line()..Line(6));
        let first = roles
            .iter()
            .position(|r| *r == RowRole::Prompt(ID0))
            .unwrap();
        assert_eq!(
            roles[first + 1],
            RowRole::Output(ID0),
            "rows joined back by the reflow"
        );
        let bounds = block_bounds(&h.term, ID0).unwrap();
        let end = bounds.input_end.unwrap();
        assert_eq!(end.line, bounds.input_start.line);
        assert_eq!(
            end.column - bounds.input_start.column,
            "echo abcdefghij".len() - 1
        );
    }

    #[test]
    fn bounds_of_a_finished_block() {
        let mut h = Harness::new(20, 8);
        h.command("$ ", "ls", "a\r\nb\r\n", 0);
        h.feed(&format!("{A}$ "));

        let bounds = block_bounds(&h.term, ID0).unwrap();
        assert_eq!(bounds.first_row, Line(0));
        assert_eq!(bounds.input_start, Point::new(Line(0), Column(2)));
        assert_eq!(bounds.input_end, Some(Point::new(Line(0), Column(3))));
        assert_eq!(bounds.output, Line(1)..Line(3));
    }

    #[test]
    fn bounds_of_a_multi_line_command() {
        let mut h = Harness::new(20, 8);
        h.feed(&format!("{A}user\r\n$ {B}echo \\\r\n> x\r\n{C}x\r\n"));

        let bounds = block_bounds(&h.term, ID0).unwrap();
        assert_eq!(bounds.first_row, Line(0));
        assert_eq!(bounds.input_start, Point::new(Line(1), Column(2)));
        assert_eq!(bounds.input_end, Some(Point::new(Line(2), Column(2))));
        assert_eq!(bounds.output, Line(3)..Line(5));
    }

    #[test]
    fn bounds_of_a_running_block_stop_at_the_cursor() {
        let mut h = Harness::new(20, 8);
        h.feed(&format!("{A}$ {B}make\r\n{C}"));
        assert_eq!(block_bounds(&h.term, ID0).unwrap().output, Line(1)..Line(2));

        h.feed("one\r\ntwo");
        assert_eq!(block_bounds(&h.term, ID0).unwrap().output, Line(1)..Line(3));
    }

    #[test]
    fn input_start_before_anything_is_typed() {
        let mut h = Harness::new(20, 8);
        h.feed(&format!("{A}$ {B}\x1b[15G{RIGHT}10:00{B}\x1b[3G"));

        let bounds = block_bounds(&h.term, ID0).unwrap();
        assert_eq!(bounds.input_start, Point::new(Line(0), Column(2)));
        assert_eq!(bounds.input_end, None);
        assert_eq!(bounds.output, Line(1)..Line(1));
    }

    #[test]
    fn unknown_block_has_no_bounds() {
        let mut h = Harness::new(20, 8);
        h.command("$ ", "ls", "", 0);
        assert_eq!(block_bounds(&h.term, BlockId(42)), None);
    }

    #[test]
    fn ids_are_capped() {
        let mut shell = ShellState::new();
        for _ in 0..MAX_BLOCKS + 5 {
            shell.start_block();
        }

        assert_eq!(shell.blocks.len(), MAX_BLOCKS);
        assert_eq!(shell.blocks().next().unwrap().0, BlockId(5));
        assert_eq!(shell.current(), Some(BlockId(MAX_BLOCKS as u64 + 4)));
    }

    #[test]
    fn foreign_links_are_not_tags() {
        assert_eq!(parse_tag("https://graphene-block/1/prompt"), None);
        assert_eq!(parse_tag("graphene-block://x/prompt"), None);
        assert_eq!(parse_tag("graphene-block://1/other"), None);
        assert_eq!(
            parse_tag("graphene-block://7/input"),
            Some((BlockId(7), TagKind::Input))
        );
    }
}
