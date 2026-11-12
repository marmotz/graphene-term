//! Text rendering of the terminal rows with glyphon.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use glyphon::{
    Buffer, Cache, Color, Resolution, Shaping, Style, TextArea, TextAtlas, TextBounds,
    TextRenderer, Viewport, Weight, Wrap,
};

use super::glyphs::Glyphs;
use super::scene::{OverlayText, TextRun};
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
    /// Font system and cell metrics.
    glyphs: Glyphs,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: TextRenderer,
    rows: Vec<RowBuffers>,
    /// Draws the overlay text after the rectangles that cover the grid.
    overlay_renderer: TextRenderer,
    /// Shaped overlay buffers; only the first `overlay_len` are in use.
    overlay: Vec<Buffer>,
    overlay_len: usize,
    overlay_hash: u64,
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
        glyphs: Glyphs,
    ) -> Self {
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let overlay_renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        Self {
            glyphs,
            viewport,
            atlas,
            renderer,
            rows: Vec::new(),
            overlay_renderer,
            overlay: Vec::new(),
            overlay_len: 0,
            overlay_hash: 0,
        }
    }

    pub fn cell_size(&self) -> CellSize {
        self.glyphs.cell_size()
    }

    pub fn set_font_size(&mut self, font_size: f32) {
        self.glyphs.set_font_size(font_size);
        self.rows.clear();
        self.overlay.clear();
        self.overlay_len = 0;
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: (u32, u32),
        layout: &GridLayout,
        rows: &[Vec<TextRun>],
        overlay: &[OverlayText],
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

        let metrics = self.glyphs.metrics();
        let (font_system, swash_cache, base) = self.glyphs.text_system();
        for (row, runs) in self.rows.iter_mut().zip(rows) {
            let hash = hash_runs(runs);
            if row.hash == hash && row.len == runs.len() {
                continue;
            }
            row.hash = hash;
            row.len = runs.len();
            while row.buffers.len() < runs.len() {
                let mut buffer = Buffer::new(font_system, metrics);
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
                buffer.shape_until_scroll(font_system, false);
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
            font_system,
            &mut self.atlas,
            &self.viewport,
            areas,
            swash_cache,
        )?;

        self.shape_overlay(overlay);
        let (font_system, swash_cache, _) = self.glyphs.text_system();
        let areas = self.overlay[..self.overlay_len]
            .iter()
            .zip(overlay)
            .map(|(buffer, item)| TextArea {
                buffer,
                left: item.x,
                top: item.y,
                scale: 1.0,
                bounds,
                default_color: Color::rgb(255, 255, 255),
                custom_glyphs: &[],
            });
        self.overlay_renderer.prepare(
            device,
            queue,
            font_system,
            &mut self.atlas,
            &self.viewport,
            areas,
            swash_cache,
        )?;
        Ok(())
    }

    /// Reshapes the overlay buffers when the overlay text changed.
    fn shape_overlay(&mut self, overlay: &[OverlayText]) {
        let mut hasher = DefaultHasher::new();
        for item in overlay {
            item.text.hash(&mut hasher);
            (item.color.r, item.color.g, item.color.b).hash(&mut hasher);
        }
        let hash = hasher.finish();
        if hash == self.overlay_hash && self.overlay_len == overlay.len() {
            return;
        }
        self.overlay_hash = hash;
        self.overlay_len = overlay.len();
        let metrics = self.glyphs.metrics();
        let (font_system, _, base) = self.glyphs.text_system();
        while self.overlay.len() < overlay.len() {
            let mut buffer = Buffer::new(font_system, metrics);
            buffer.set_wrap(Wrap::None);
            self.overlay.push(buffer);
        }
        for (buffer, item) in self.overlay.iter_mut().zip(overlay) {
            let attrs = base
                .clone()
                .color(Color::rgb(item.color.r, item.color.g, item.color.b));
            buffer.set_text(&item.text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(font_system, false);
        }
    }

    /// Draws the grid text.
    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) -> anyhow::Result<()> {
        self.renderer.render(&self.atlas, &self.viewport, pass)?;
        Ok(())
    }

    /// Draws the overlay text.
    pub fn render_overlay(&self, pass: &mut wgpu::RenderPass<'_>) -> anyhow::Result<()> {
        self.overlay_renderer
            .render(&self.atlas, &self.viewport, pass)?;
        Ok(())
    }

    pub fn trim(&mut self) {
        self.atlas.trim();
    }
}
