//! Rendering benchmarks on recorded PTY output (see `fixtures/README.md`).
//!
//! - `parse`: bytes into a fresh session (`Term` and command block timeline), through
//!   the PTY loop's `process`;
//! - `frame`: view rows + `Frame::capture` + `scene::build`;
//! - `glyphs_cold` / `glyphs_warm`: glyph cache lookups for every visible cell, with a
//!   cache emptied before each pass (rasterization and atlas placement included) or
//!   already filled;
//! - `gpu_prepare`: scene upload, draw and submit on a headless GPU device, either with
//!   an unchanged frame (`idle`) or alternating with a blank frame (`changed`, two
//!   frames per iteration). Skipped when no GPU adapter is available.

use std::hint::black_box;

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use graphene::bench::{
    CellSize, Frame, GlyphKey, Glyphs, GridLayout, Headless, Palette, Rect, Scanner, Timeline,
    process, scene, scroll_block, view_rows,
};

/// Terminal size of the recordings.
const COLUMNS: usize = 120;
const LINES: usize = 32;
const PADDING: f32 = 8.0;
const FONT_SIZE: f32 = 14.0;

/// Session token of the block signals in the fixtures.
const TOKEN: &str = "0123456789abcdef0123456789abcdef";

struct Fixture {
    name: &'static str,
    bytes: &'static [u8],
    /// Rows of the current block scrolled back before capturing.
    scroll: isize,
    /// Commands the recording is split into, each one a block between a start and an
    /// end signal; 0 sends no signal (one block, like a shell without hooks).
    commands: usize,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "btop",
        bytes: include_bytes!("fixtures/btop.raw"),
        scroll: 0,
        commands: 0,
    },
    Fixture {
        name: "cat",
        bytes: include_bytes!("fixtures/cat.raw"),
        scroll: 0,
        commands: 0,
    },
    Fixture {
        name: "vim",
        bytes: include_bytes!("fixtures/vim.raw"),
        scroll: 0,
        commands: 0,
    },
    Fixture {
        name: "history",
        bytes: include_bytes!("fixtures/cat.raw"),
        scroll: 500,
        commands: 0,
    },
    Fixture {
        name: "timeline",
        bytes: include_bytes!("fixtures/cat.raw"),
        scroll: 0,
        commands: 8,
    },
];

/// A session: the live terminal and its blocks.
struct Session {
    term: Term<VoidListener>,
    timeline: Timeline,
}

fn new_term() -> Term<VoidListener> {
    Term::new(
        Config::default(),
        &TermSize::new(COLUMNS, LINES),
        VoidListener,
    )
}

fn new_session() -> Session {
    Session {
        term: new_term(),
        timeline: Timeline::new(Config::default().scrolling_history),
    }
}

fn feed(session: &mut Session, bytes: &[u8]) {
    let mut parser: Processor = Processor::new();
    let mut scanner = Scanner::new(TOKEN);
    let Session { term, timeline } = session;
    process(&mut scanner, &mut parser, timeline, term, bytes);
}

/// The bytes of `fixture`, split into its commands at line ends, each one wrapped in a
/// prompt, a start signal and an end signal.
fn bytes_for(fixture: &Fixture) -> Vec<u8> {
    if fixture.commands == 0 {
        return fixture.bytes.to_vec();
    }
    let size = fixture.bytes.len().div_ceil(fixture.commands);
    let mut bytes = Vec::new();
    let mut rest = fixture.bytes;
    while !rest.is_empty() {
        let cut = rest
            .iter()
            .skip(size)
            .position(|&b| b == b'\n')
            .map_or(rest.len(), |at| size + at + 1);
        let (output, tail) = rest.split_at(cut);
        // "Y2F0" is "cat" in base64.
        bytes.extend_from_slice(format!("$ cat\r\n\x1b]6973;{TOKEN};start;Y2F0\x07").as_bytes());
        bytes.extend_from_slice(output);
        bytes.extend_from_slice(format!("\x1b]6973;{TOKEN};end;0\x07").as_bytes());
        rest = tail;
    }
    bytes.extend_from_slice(b"$ ");
    bytes
}

fn session_for(fixture: &Fixture) -> Session {
    let mut session = new_session();
    feed(&mut session, &bytes_for(fixture));
    if fixture.scroll != 0 {
        let Session { term, timeline } = &mut session;
        let id = timeline.current().id;
        scroll_block(timeline, term, id, LINES, fixture.scroll);
    }
    session
}

/// Frame of the view over the blocks of `session`.
fn capture(session: &Session, palette: &Palette) -> Frame {
    let rows = view_rows(&session.timeline, &session.term, LINES);
    Frame::capture(&session.term, &session.timeline, &rows, palette, true, None)
}

fn layout(cell: CellSize) -> GridLayout {
    GridLayout {
        columns: COLUMNS,
        lines: LINES,
        ..GridLayout::new(Rect::from_size(0, 0), cell, PADDING)
    }
}

fn parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    // `history` parses the same bytes as `cat`.
    for fixture in FIXTURES.iter().filter(|f| f.scroll == 0) {
        let bytes = bytes_for(fixture);
        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_function(fixture.name, |b| {
            b.iter_batched_ref(
                new_session,
                |session| feed(session, black_box(&bytes)),
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn frame(c: &mut Criterion) {
    let palette = Palette::default();
    let layout = layout(CellSize {
        width: 8.4,
        height: 19.0,
    });
    let mut group = c.benchmark_group("frame");
    for fixture in FIXTURES {
        let session = session_for(fixture);
        group.bench_function(fixture.name, |b| {
            b.iter(|| {
                let frame = capture(black_box(&session), &palette);
                scene::build(&frame, &layout, true)
            })
        });
    }
    group.finish();
}

/// Looks up the glyph of every drawn cell of `frame`; returns how many have pixels.
fn resolve_glyphs(glyphs: &mut Glyphs, frame: &Frame) -> usize {
    frame
        .rows
        .iter()
        .flatten()
        .filter(|cell| !cell.spacer && cell.c != ' ')
        .filter_map(|cell| {
            glyphs.get(GlyphKey {
                c: cell.c,
                bold: cell.bold,
                italic: cell.italic,
                ui: None,
            })
        })
        .count()
}

fn glyphs(c: &mut Criterion) {
    let palette = Palette::default();
    let mut glyphs = Glyphs::new(None, FONT_SIZE);
    let mut cold = c.benchmark_group("glyphs_cold");
    let frames: Vec<_> = FIXTURES
        .iter()
        .map(|fixture| (fixture.name, capture(&session_for(fixture), &palette)))
        .collect();
    for (name, frame) in &frames {
        cold.bench_function(*name, |b| {
            b.iter(|| {
                glyphs.clear();
                resolve_glyphs(&mut glyphs, black_box(frame))
            })
        });
    }
    cold.finish();
    let mut warm = c.benchmark_group("glyphs_warm");
    for (name, frame) in &frames {
        glyphs.clear();
        resolve_glyphs(&mut glyphs, frame);
        glyphs.take_uploads();
        warm.bench_function(*name, |b| {
            b.iter(|| resolve_glyphs(&mut glyphs, black_box(frame)))
        });
    }
    warm.finish();
}

fn gpu_prepare(c: &mut Criterion) {
    let probe_cell = CellSize {
        width: 8.4,
        height: 19.0,
    };
    let size = |cell: CellSize| {
        (
            ((COLUMNS + 1) as f32 * cell.width + 2.0 * PADDING).ceil() as u32,
            (LINES as f32 * cell.height + 2.0 * PADDING).ceil() as u32,
        )
    };
    let (width, height) = size(probe_cell);
    let Some(mut renderer) = Headless::new(width, height, Glyphs::new(None, FONT_SIZE)) else {
        eprintln!("gpu_prepare: no GPU adapter available, skipped");
        return;
    };
    let layout = layout(renderer.cell_size());
    let palette = Palette::default();
    let blank = capture(&new_session(), &palette);

    let mut group = c.benchmark_group("gpu_prepare");
    for fixture in FIXTURES {
        let frame = capture(&session_for(fixture), &palette);
        group.bench_function(format!("{}/idle", fixture.name), |b| {
            b.iter(|| renderer.render(black_box(&frame), &layout).unwrap())
        });
        group.bench_function(format!("{}/changed", fixture.name), |b| {
            b.iter(|| {
                renderer.render(&blank, &layout).unwrap();
                renderer.render(black_box(&frame), &layout).unwrap();
            })
        });
    }
    group.finish();
}

criterion_group!(benches, parse, frame, glyphs, gpu_prepare);
criterion_main!(benches);
