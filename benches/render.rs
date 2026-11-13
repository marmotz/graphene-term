//! Rendering benchmarks on recorded PTY output (see `fixtures/README.md`).
//!
//! - `parse`: bytes into a fresh `Term`;
//! - `frame`: view rows + `Frame::capture` + `scene::build`;
//! - `glyphs_cold` / `glyphs_warm`: glyph cache lookups for every visible cell, with a
//!   cache emptied before each pass (rasterization and atlas placement included) or
//!   already filled;
//! - `gpu_prepare`: scene upload, draw and submit on a headless GPU device, either with
//!   an unchanged frame (`idle`) or alternating with a blank frame (`changed`, two
//!   frames per iteration). Skipped when no GPU adapter is available.

use std::hint::black_box;

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use graphene::bench::{
    CellSize, Frame, GlyphKey, Glyphs, GridLayout, Headless, Palette, ShellState, scene, view_rows,
};

/// Terminal size of the recordings.
const COLUMNS: usize = 120;
const LINES: usize = 32;
const PADDING: f32 = 8.0;
const FONT_SIZE: f32 = 14.0;

struct Fixture {
    name: &'static str,
    bytes: &'static [u8],
    /// Lines scrolled back into the history before capturing.
    scroll: i32,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "btop",
        bytes: include_bytes!("fixtures/btop.raw"),
        scroll: 0,
    },
    Fixture {
        name: "cat",
        bytes: include_bytes!("fixtures/cat.raw"),
        scroll: 0,
    },
    Fixture {
        name: "vim",
        bytes: include_bytes!("fixtures/vim.raw"),
        scroll: 0,
    },
    Fixture {
        name: "history",
        bytes: include_bytes!("fixtures/cat.raw"),
        scroll: 500,
    },
];

fn new_term() -> Term<VoidListener> {
    Term::new(
        Config::default(),
        &TermSize::new(COLUMNS, LINES),
        VoidListener,
    )
}

fn feed(term: &mut Term<VoidListener>, bytes: &[u8]) {
    let mut parser: Processor = Processor::new();
    parser.advance(term, bytes);
}

fn term_for(fixture: &Fixture) -> Term<VoidListener> {
    let mut term = new_term();
    feed(&mut term, fixture.bytes);
    if fixture.scroll != 0 {
        term.scroll_display(Scroll::Delta(fixture.scroll));
    }
    term
}

/// Frame of the viewport of `term`, with no shell integration.
fn capture(term: &Term<VoidListener>, palette: &Palette) -> Frame {
    let shell = ShellState::new();
    let rows = view_rows(term, &shell, term.grid().display_offset());
    Frame::capture(term, &rows, &shell, palette, true)
}

fn layout(cell: CellSize) -> GridLayout {
    GridLayout {
        columns: COLUMNS,
        lines: LINES,
        ..GridLayout::new(0, 0, cell, PADDING)
    }
}

fn parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    // `history` parses the same bytes as `cat`.
    for fixture in FIXTURES.iter().filter(|f| f.scroll == 0) {
        group.throughput(Throughput::Bytes(fixture.bytes.len() as u64));
        group.bench_function(fixture.name, |b| {
            b.iter_batched_ref(
                new_term,
                |term| feed(term, black_box(fixture.bytes)),
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
        let term = term_for(fixture);
        group.bench_function(fixture.name, |b| {
            b.iter(|| {
                let frame = capture(black_box(&term), &palette);
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
        .map(|fixture| (fixture.name, capture(&term_for(fixture), &palette)))
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
    let blank = capture(&new_term(), &palette);

    let mut group = c.benchmark_group("gpu_prepare");
    for fixture in FIXTURES {
        let frame = capture(&term_for(fixture), &palette);
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
