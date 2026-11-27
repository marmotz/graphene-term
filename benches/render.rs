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
//!   frames per iteration). Skipped when no GPU adapter is available;
//! - `frame_panes`: the frames of four shown panes when only one changed since the
//!   last redraw, as the window redraws them (the others come from its frame cache);
//! - `present`: a full HD frame drawn and waited for (`direct`, the GPU surface) or
//!   read back and packed as an X11 ARGB image (`x11`, the X11 presentation of a
//!   transparent window), `x11_async` the part of `x11` that keeps the event loop busy
//!   when the readback does not wait for the GPU, and `x11_pack/*` the CPU part of
//!   `x11` from an RGBA or a BGRA
//!   target. Skipped when no GPU adapter is available;
//! - `search`: a search over 10 000 lines of history, with a new query (`typed`, every
//!   grid searched) or the same one (`output`, only the live grid searched again).

use std::hint::black_box;
use std::time::{Duration, Instant};

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use graphene::bench::{
    CellSize, FontOptions, Frame, FrameCache, GlyphKey, Glyphs, GridLayout, GridSize, Headless,
    Palette, Query, Rect, Scanner, Searcher, Timeline, pack_argb32, process, scene, scroll_block,
    view_rows,
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
    process(
        &mut scanner,
        &mut parser,
        timeline,
        term,
        bytes,
        Instant::now(),
    );
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
    Frame::capture(
        &session.term,
        &session.timeline,
        &rows,
        palette,
        true,
        true,
        None,
    )
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
                let size = GridSize::new(14.0);
                scene::build(&frame, &layout, size, size, true, true)
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
            let size = glyphs.font_size();
            glyphs.get(GlyphKey::grid(cell.c, size, cell.bold, cell.italic))
        })
        .count()
}

fn glyphs(c: &mut Criterion) {
    let palette = Palette::default();
    let mut glyphs = Glyphs::new(&FontOptions::default(), FONT_SIZE);
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
    let Some(mut renderer) = Headless::new(
        width,
        height,
        Glyphs::new(&FontOptions::default(), FONT_SIZE),
    ) else {
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

/// Fixtures shown side by side in the `frame_panes` benchmark.
const PANES: [&str; 4] = ["btop", "cat", "vim", "history"];

fn frame_panes(c: &mut Criterion) {
    let palette = Palette::default();
    let sessions: Vec<_> = PANES
        .iter()
        .map(|name| session_for(FIXTURES.iter().find(|f| f.name == *name).unwrap()))
        .collect();
    let mut group = c.benchmark_group("frame_panes");
    let mut cache = FrameCache::default();
    let mut revisions = [0u64; PANES.len()];
    let mut next = 0;
    group.bench_function("one_changed", |b| {
        b.iter(|| {
            // One pane changed since the last redraw, in turn.
            revisions[next] += 1;
            next = (next + 1) % PANES.len();
            for (id, session) in sessions.iter().enumerate() {
                let rows = view_rows(&session.timeline, &session.term, LINES);
                let key = (revisions[id], rows.clone());
                cache.update(id, Some(key), || {
                    Frame::capture(
                        &session.term,
                        &session.timeline,
                        &rows,
                        &palette,
                        true,
                        true,
                        None,
                    )
                });
            }
            (0..PANES.len()).filter_map(|id| cache.get(id)).count()
        })
    });
    group.finish();
}

/// Lines of history searched by the `search` benchmark.
const SEARCH_LINES: usize = 10_000;

/// A session holding [`SEARCH_LINES`] lines of history: blocks of 100 output lines,
/// with a match every 10 lines.
fn search_session() -> Session {
    let mut bytes = String::new();
    for block in 0..SEARCH_LINES / 100 {
        // "bWFrZQ==" is "make" in base64.
        bytes.push_str(&format!("$ make\r\n\x1b]6973;{TOKEN};start;bWFrZQ==\x07"));
        for line in 0..100 {
            let word = if line % 10 == 0 {
                "warning"
            } else {
                "compiling"
            };
            bytes.push_str(&format!("{word} crate {block}-{line} src/lib.rs\r\n"));
        }
        bytes.push_str(&format!("\x1b]6973;{TOKEN};end;0\x07"));
    }
    bytes.push_str("$ ");
    let mut session = new_session();
    feed(&mut session, bytes.as_bytes());
    session
}

fn search(c: &mut Criterion) {
    let session = search_session();
    let query = Query {
        text: "warning".to_owned(),
        ..Query::default()
    };
    let mut group = c.benchmark_group("search");
    group.throughput(Throughput::Elements(SEARCH_LINES as u64));
    // A new query: every grid searched, as when a letter is typed.
    group.bench_function("typed", |b| {
        b.iter(|| {
            let mut searcher = Searcher::new();
            searcher.set_query(black_box(query.clone())).unwrap();
            searcher.search(&session.timeline, &session.term).list.len()
        })
    });
    // The same query: only the live grid searched, as when output arrives.
    let mut searcher = Searcher::new();
    searcher.set_query(query.clone()).unwrap();
    group.bench_function("output", |b| {
        b.iter(|| searcher.search(&session.timeline, &session.term).list.len())
    });
    group.finish();
}

/// Window size of the `present` benchmark.
const PRESENT_SIZE: (u32, u32) = (1920, 1080);

fn present(c: &mut Criterion) {
    let (width, height) = PRESENT_SIZE;
    let Some(mut renderer) = Headless::new(
        width,
        height,
        Glyphs::new(&FontOptions::default(), FONT_SIZE),
    ) else {
        eprintln!("present: no GPU adapter available, skipped");
        return;
    };
    let layout = layout(renderer.cell_size());
    let palette = Palette::default();
    let cat = FIXTURES.iter().find(|f| f.name == "cat").unwrap();
    let frame = capture(&session_for(cat), &palette);

    let mut group = c.benchmark_group("present");
    group.bench_function("direct", |b| {
        b.iter(|| renderer.present_direct(black_box(&frame), &layout).unwrap())
    });
    group.bench_function("x11", |b| {
        b.iter(|| {
            renderer
                .present_x11(black_box(&frame), &layout)
                .unwrap()
                .len()
        })
    });
    // The time a redraw keeps the event loop busy with the non-blocking readback:
    // drawing and starting the read, then packing once the GPU is done; the GPU work
    // in between leaves the loop free.
    group.bench_function("x11_async", |b| {
        b.iter_custom(|iters| {
            let mut busy = Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();
                renderer.start_x11(black_box(&frame), &layout).unwrap();
                busy += start.elapsed();
                renderer.wait_gpu().unwrap();
                let start = Instant::now();
                assert!(renderer.finish_x11(false).unwrap());
                busy += start.elapsed();
            }
            busy
        })
    });
    // The CPU part of `x11`: the pixels packed for the X server.
    let source = vec![0x80u8; (width * height * 4) as usize];
    let mut packed = Vec::new();
    for (name, bgra) in [("x11_pack/rgba", false), ("x11_pack/bgra", true)] {
        group.bench_function(name, |b| {
            b.iter(|| {
                pack_argb32(
                    black_box(&source),
                    PRESENT_SIZE,
                    width * 4,
                    bgra,
                    &mut packed,
                )
                .unwrap()
            })
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    parse,
    frame,
    frame_panes,
    glyphs,
    gpu_prepare,
    present,
    search
);
criterion_main!(benches);
