//! Text rendering of the terminal rows with glyphon.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, Style,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Weight, Wrap,
};

use super::scene::TextRun;
use crate::layout::{CellSize, GridLayout};

/// Shaped buffers of one visible row, one per text run.
#[derive(Default)]
struct RowBuffers {
    hash: u64,
    /// (column, buffer) pairs; only the first `len` entries are in use.
    buffers: Vec<(usize, Buffer)>,
    len: usize,
}

pub struct TextGrid {
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: TextRenderer,
    rows: Vec<RowBuffers>,
    /// Font family name; `None` uses the system monospace font.
    family: Option<String>,
    metrics: Metrics,
    cell: CellSize,
}

fn family(name: &Option<String>) -> Family<'_> {
    match name {
        Some(name) => Family::Name(name),
        None => Family::Monospace,
    }
}

fn hash_runs(runs: &[TextRun]) -> u64 {
    let mut hasher = DefaultHasher::new();
    runs.hash(&mut hasher);
    hasher.finish()
}

impl TextGrid {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        family: Option<String>,
        font_size: f32,
    ) -> Self {
        let mut font_system = FontSystem::new();
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let (metrics, cell) = Self::measure(&mut font_system, &family, font_size);
        Self {
            font_system,
            swash_cache: SwashCache::new(),
            viewport,
            atlas,
            renderer,
            rows: Vec::new(),
            family,
            metrics,
            cell,
        }
    }

    /// Computes the cell size from the advance of a reference glyph.
    fn measure(
        font_system: &mut FontSystem,
        name: &Option<String>,
        font_size: f32,
    ) -> (Metrics, CellSize) {
        let line_height = (font_size * 1.3).ceil();
        let metrics = Metrics::new(font_size, line_height);
        let mut buffer = Buffer::new(font_system, metrics);
        buffer.set_text(
            "M",
            &Attrs::new().family(family(name)),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(font_system, false);
        // Keep the exact advance: rounding it would make text drift off the grid.
        let width = buffer
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.first().map(|g| g.w))
            .unwrap_or(font_size * 0.6)
            .max(1.0);
        (
            metrics,
            CellSize {
                width,
                height: line_height,
            },
        )
    }

    pub fn cell_size(&self) -> CellSize {
        self.cell
    }

    pub fn set_font_size(&mut self, font_size: f32) {
        let (metrics, cell) = Self::measure(&mut self.font_system, &self.family, font_size);
        self.metrics = metrics;
        self.cell = cell;
        self.rows.clear();
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: (u32, u32),
        layout: &GridLayout,
        rows: &[Vec<TextRun>],
    ) -> anyhow::Result<()> {
        self.viewport.update(
            queue,
            Resolution {
                width: screen.0,
                height: screen.1,
            },
        );

        // Grow or shrink the row cache, then reshape only rows that changed.
        self.rows.resize_with(rows.len(), RowBuffers::default);

        let base = Attrs::new().family(family(&self.family));
        for (row, runs) in self.rows.iter_mut().zip(rows) {
            let hash = hash_runs(runs);
            if row.hash == hash && row.len == runs.len() {
                continue;
            }
            row.hash = hash;
            row.len = runs.len();
            while row.buffers.len() < runs.len() {
                let mut buffer = Buffer::new(&mut self.font_system, self.metrics);
                buffer.set_wrap(Wrap::None);
                row.buffers.push((0, buffer));
            }
            for ((column, buffer), run) in row.buffers.iter_mut().zip(runs) {
                let mut attrs =
                    base.clone()
                        .color(Color::rgb(run.color.r, run.color.g, run.color.b));
                if run.bold {
                    attrs = attrs.weight(Weight::BOLD);
                }
                if run.italic {
                    attrs = attrs.style(Style::Italic);
                }
                *column = run.column;
                buffer.set_text(&run.text, &attrs, Shaping::Advanced, None);
                buffer.shape_until_scroll(&mut self.font_system, false);
            }
        }

        let bounds = TextBounds {
            left: 0,
            top: 0,
            right: screen.0 as i32,
            bottom: screen.1 as i32,
        };
        let areas = self.rows.iter().enumerate().flat_map(|(line, row)| {
            row.buffers[..row.len].iter().map(move |(column, buffer)| {
                let (left, top) = layout.cell_origin(*column, line);
                TextArea {
                    buffer,
                    left,
                    top,
                    scale: 1.0,
                    bounds,
                    default_color: Color::rgb(255, 255, 255),
                    custom_glyphs: &[],
                }
            })
        });

        self.renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            areas,
            &mut self.swash_cache,
        )?;
        Ok(())
    }

    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) -> anyhow::Result<()> {
        self.renderer.render(&self.atlas, &self.viewport, pass)?;
        Ok(())
    }

    pub fn trim(&mut self) {
        self.atlas.trim();
    }
}
