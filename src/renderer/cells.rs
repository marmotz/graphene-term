//! Instanced rendering of the glyphs (grid cells and banner text) from the glyph atlas.
//!
//! Each [`GlyphInstance`] of the scene is resolved through [`Glyphs`] into one quad
//! anchored at its cell origin plus the glyph placement, aligned on whole pixels. Each
//! [`TextRun`] is shaped as a whole, each of its glyphs anchored at the origin of the
//! cell it is drawn from. The
//! atlas pages live in two textures (`R8Unorm` masks tinted with the instance color,
//! RGBA color glyphs drawn as is) fed by the uploads of newly rasterized glyphs.

use std::borrow::Cow;
use std::mem;
use std::ops::Range;

use bytemuck::{Pod, Zeroable};

use super::pipeline::{InstanceBuffer, ScreenUniform, instanced_pipeline, uniform_entry};

use super::atlas::Page;
use super::glyphs::{Glyph, Glyphs, Upload};
use super::scene::{GlyphInstance, TextRun};

/// One textured quad, as uploaded to the GPU.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct CellInstance {
    /// Top-left corner on screen, in whole pixels.
    pub position: [f32; 2],
    /// Size of the quad and of its atlas region, in pixels.
    pub size: [f32; 2],
    /// Top-left corner of the region in its atlas page, in pixels.
    pub uv: [f32; 2],
    /// Linear RGBA color, tinting mask glyphs.
    pub color: [f32; 4],
    /// 1 for a color glyph (color page), 0 for a mask.
    pub is_color: u32,
}

/// How many times a frame is resolved again when the atlas was reset while resolving it
/// (the glyphs resolved before the reset point to freed regions).
const MAX_RESOLVE_ATTEMPTS: usize = 2;

/// What one draw call shows: glyphs drawn one by one and runs shaped together.
#[derive(Debug, Clone, Copy, Default)]
pub struct GlyphGroup<'a> {
    pub glyphs: &'a [GlyphInstance],
    pub runs: &'a [TextRun],
}

impl<'a> From<&'a [GlyphInstance]> for GlyphGroup<'a> {
    fn from(glyphs: &'a [GlyphInstance]) -> Self {
        Self { glyphs, runs: &[] }
    }
}

/// The quad of `glyph` drawn from the cell at `origin`.
fn quad(glyph: Glyph, origin: [f32; 2], color: [f32; 4]) -> CellInstance {
    CellInstance {
        position: [
            (origin[0] + glyph.left as f32).round(),
            (origin[1] + glyph.top as f32).round(),
        ],
        size: [glyph.region.width as f32, glyph.region.height as f32],
        uv: [glyph.region.x as f32, glyph.region.y as f32],
        color,
        is_color: u32::from(glyph.page == Page::Color),
    }
}

/// Resolves `items` into quads appended to `out`. Glyphs with nothing to draw are
/// skipped.
pub fn resolve(glyphs: &mut Glyphs, items: &[GlyphInstance], out: &mut Vec<CellInstance>) {
    out.extend(items.iter().filter_map(|item| {
        let glyph = glyphs.get(item.key)?;
        Some(quad(glyph, item.origin, item.color))
    }));
}

/// Resolves `runs` into quads appended to `out`, each glyph in the color of the cell it
/// is drawn from. `text` is a scratch buffer, reused between calls.
pub fn resolve_runs(
    glyphs: &mut Glyphs,
    runs: &[TextRun],
    text: &mut String,
    out: &mut Vec<CellInstance>,
) {
    for run in runs {
        text.clear();
        text.extend(run.cells.iter().map(|cell| cell.c));
        let shaped = glyphs.shape_run(text, run.bold, run.italic);
        for run_glyph in shaped.iter() {
            let index = usize::from(run_glyph.cell);
            let Some(cell) = run.cells.get(index) else {
                continue;
            };
            let Some(glyph) = glyphs.run_glyph(run_glyph) else {
                continue;
            };
            let origin = [run.origin[0] + index as f32 * run.advance, run.origin[1]];
            out.push(quad(glyph, origin, cell.color));
        }
    }
}

/// Resolves each group of `groups` in order into `out` (cleared first): its glyphs,
/// then its runs. Returns the range of quads of each group. Starts over when the atlas
/// was reset meanwhile, so every quad points to a region of the current atlas.
pub fn resolve_groups(
    glyphs: &mut Glyphs,
    groups: &[GlyphGroup<'_>],
    out: &mut Vec<CellInstance>,
) -> Vec<Range<u32>> {
    let mut ranges = Vec::with_capacity(groups.len());
    let mut text = String::new();
    for _ in 0..MAX_RESOLVE_ATTEMPTS {
        let generation = glyphs.generation();
        out.clear();
        ranges.clear();
        for group in groups {
            let start = out.len() as u32;
            resolve(glyphs, group.glyphs, out);
            resolve_runs(glyphs, group.runs, &mut text, out);
            ranges.push(start..out.len() as u32);
        }
        if glyphs.generation() == generation {
            break;
        }
    }
    ranges
}

pub struct CellRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: ScreenUniform,
    bind_group: wgpu::BindGroup,
    mask_atlas: wgpu::Texture,
    color_atlas: wgpu::Texture,
    instance_buffer: InstanceBuffer<CellInstance>,
    /// Quads of the current frame, reused between frames.
    instances: Vec<CellInstance>,
    /// Quads of each group passed to [`Self::prepare`].
    groups: Vec<Range<u32>>,
}

const INITIAL_CAPACITY: usize = 4096;

fn create_atlas_texture(
    device: &wgpu::Device,
    label: &str,
    size: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

impl CellRenderer {
    /// `atlas_size` is the side of the [`Glyphs`] atlas pages.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, atlas_size: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cell shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("cell.wgsl"))),
        });

        let uniform_buffer = ScreenUniform::new(device, "cell uniforms");

        let mask_atlas = create_atlas_texture(
            device,
            "glyph mask atlas",
            atlas_size,
            wgpu::TextureFormat::R8Unorm,
        );
        // Emoji pixels are sRGB encoded: an sRGB view linearizes them for an sRGB target,
        // like the other colors of the scene.
        let color_format = if format.is_srgb() {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        let color_atlas =
            create_atlas_texture(device, "glyph color atlas", atlas_size, color_format);

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cell bind group layout"),
            entries: &[uniform_entry(0), texture_entry(1), texture_entry(2)],
        });

        let mask_view = mask_atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let color_view = color_atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cell bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&mask_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&color_view),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cell pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            ..Default::default()
        });

        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<CellInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2,
                1 => Float32x2,
                2 => Float32x2,
                3 => Float32x4,
                4 => Uint32
            ],
        };

        let pipeline = instanced_pipeline(
            device,
            "cell pipeline",
            &pipeline_layout,
            &shader,
            format,
            instance_layout,
        );

        Self {
            pipeline,
            uniform_buffer,
            bind_group,
            mask_atlas,
            color_atlas,
            instance_buffer: InstanceBuffer::new(device, "cell instances", INITIAL_CAPACITY),
            instances: Vec::new(),
            groups: Vec::new(),
        }
    }

    /// Resolves the glyph groups of the frame, uploads the newly rasterized glyphs to
    /// the atlas and the quads to the instance buffer. Each group is then drawn on its
    /// own by [`Self::render`].
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: (u32, u32),
        glyphs: &mut Glyphs,
        groups: &[GlyphGroup<'_>],
    ) {
        self.uniform_buffer.write(queue, screen);

        self.groups = resolve_groups(glyphs, groups, &mut self.instances);
        for upload in glyphs.take_uploads() {
            self.upload(queue, &upload);
        }

        self.instance_buffer.write(device, queue, &self.instances);
    }

    fn upload(&self, queue: &wgpu::Queue, upload: &Upload) {
        let texture = match upload.page {
            Page::Mask => &self.mask_atlas,
            Page::Color => &self.color_atlas,
        };
        let region = upload.region;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: region.x,
                    y: region.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &upload.data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(region.width * upload.page.bytes_per_pixel()),
                rows_per_image: Some(region.height),
            },
            wgpu::Extent3d {
                width: region.width,
                height: region.height,
                depth_or_array_layers: 1,
            },
        );
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, range: Range<u32>) {
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice());
        pass.draw(0..4, range);
    }

    /// Draws the glyphs of the group `index` uploaded by [`Self::prepare`].
    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>, index: usize) {
        if let Some(range) = self.groups.get(index) {
            self.draw(pass, range.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::glyphs::{FontOptions, GlyphKey};
    use crate::renderer::scene::RunCell;

    const FONT_SIZE: f32 = 14.0;
    const WHITE: [f32; 4] = [1.0; 4];

    fn item(c: char, origin: [f32; 2]) -> GlyphInstance {
        GlyphInstance {
            origin,
            key: GlyphKey {
                c,
                bold: false,
                italic: false,
                ui: None,
            },
            color: WHITE,
        }
    }

    #[test]
    fn quad_is_the_cell_origin_plus_the_glyph_placement() {
        let mut glyphs = Glyphs::new(&FontOptions::default(), FONT_SIZE);
        let glyph = glyphs
            .get(GlyphKey {
                c: 'M',
                bold: false,
                italic: false,
                ui: None,
            })
            .unwrap();
        let mut out = Vec::new();
        resolve(&mut glyphs, &[item('M', [16.4, 40.0])], &mut out);
        assert_eq!(
            out,
            vec![CellInstance {
                position: [(16.4 + glyph.left as f32).round(), 40.0 + glyph.top as f32],
                size: [glyph.region.width as f32, glyph.region.height as f32],
                uv: [glyph.region.x as f32, glyph.region.y as f32],
                color: WHITE,
                is_color: 0,
            }]
        );
    }

    #[test]
    fn glyphs_without_pixels_are_skipped() {
        let mut glyphs = Glyphs::new(&FontOptions::default(), FONT_SIZE);
        let mut out = Vec::new();
        resolve(
            &mut glyphs,
            &[item(' ', [0.0, 0.0]), item('a', [10.0, 0.0])],
            &mut out,
        );
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn groups_are_resolved_in_order_into_their_own_ranges() {
        let mut glyphs = Glyphs::new(&FontOptions::default(), FONT_SIZE);
        let mut out = Vec::new();
        let first = [item('a', [0.0, 0.0]), item('b', [10.0, 0.0])];
        let empty = [item(' ', [0.0, 0.0])];
        let last = [item('x', [0.0, 0.0])];
        let groups = [first.as_slice(), &empty, &last].map(GlyphGroup::from);
        let ranges = resolve_groups(&mut glyphs, &groups, &mut out);
        assert_eq!(ranges, [0..2, 2..2, 2..3]);
        assert_eq!(out.len(), 3);
        let x = glyphs
            .get(GlyphKey {
                c: 'x',
                bold: false,
                italic: false,
                ui: None,
            })
            .unwrap();
        assert_eq!(out[2].uv, [x.region.x as f32, x.region.y as f32]);
    }

    #[test]
    fn frame_is_resolved_again_after_an_atlas_reset() {
        const ATLAS: u32 = 64;
        let chars: Vec<char> = ('A'..='Z').chain('0'..='9').collect();
        let key = |c: char| GlyphKey {
            c,
            bold: false,
            italic: false,
            ui: None,
        };
        // How many of `chars` fit a fresh atlas before it resets.
        let mut probe = Glyphs::with_atlas_size(&FontOptions::default(), FONT_SIZE, ATLAS);
        let capacity = chars
            .iter()
            .position(|&c| {
                probe.get(key(c));
                probe.generation() > 0
            })
            .expect("the atlas never filled up");
        assert!(capacity >= 2 && capacity + 2 <= chars.len(), "{capacity}");

        // Leave room for two glyphs: the third glyph of the frame resets the atlas.
        let mut glyphs = Glyphs::with_atlas_size(&FontOptions::default(), FONT_SIZE, ATLAS);
        for &c in &chars[..capacity - 2] {
            glyphs.get(key(c));
        }
        let grid: Vec<_> = chars[capacity - 2..capacity + 2]
            .iter()
            .enumerate()
            .map(|(i, &c)| item(c, [i as f32 * 10.0, 0.0]))
            .collect();
        let mut out = Vec::new();
        resolve_groups(&mut glyphs, &[GlyphGroup::from(grid.as_slice())], &mut out);
        assert_eq!(glyphs.generation(), 1, "the frame did not reset the atlas");
        // Every quad points to the region its glyph holds in the current atlas.
        assert_eq!(out.len(), grid.len());
        for (quad, item) in out.iter().zip(&grid) {
            let glyph = glyphs.get(item.key).unwrap();
            assert_eq!(quad.uv, [glyph.region.x as f32, glyph.region.y as f32]);
        }
    }

    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

    fn run(text: &str, origin: [f32; 2], advance: f32) -> TextRun {
        TextRun {
            origin,
            advance,
            cells: text.chars().map(|c| RunCell { c, color: WHITE }).collect(),
            bold: false,
            italic: false,
            cursor: None,
        }
    }

    fn resolved_runs(glyphs: &mut Glyphs, runs: &[TextRun]) -> Vec<CellInstance> {
        let mut out = Vec::new();
        resolve_runs(glyphs, runs, &mut String::new(), &mut out);
        out
    }

    #[test]
    fn run_without_ligature_is_drawn_like_its_cells() {
        let mut glyphs = Glyphs::with_test_font(&FontOptions::default(), FONT_SIZE);
        let advance = glyphs.cell_size().width;
        let runs = [run("ab", [16.4, 40.0], advance)];
        let mut cells = Vec::new();
        resolve(
            &mut glyphs,
            &[item('a', [16.4, 40.0]), item('b', [16.4 + advance, 40.0])],
            &mut cells,
        );
        assert_eq!(resolved_runs(&mut glyphs, &runs), cells);
    }

    #[test]
    fn ligature_is_drawn_from_its_cells_in_their_colors() {
        let mut glyphs = Glyphs::with_test_font(&FontOptions::default(), FONT_SIZE);
        let mut arrow = run("->", [0.0, 0.0], 10.0);
        arrow.cells[1].color = RED;
        let quads = resolved_runs(&mut glyphs, &[arrow]);
        let shaped = glyphs.shape_run("->", false, false);
        let expected: Vec<_> = shaped
            .iter()
            .filter_map(|run_glyph| {
                let glyph = glyphs.run_glyph(run_glyph)?;
                let cell = f32::from(run_glyph.cell);
                let color = if run_glyph.cell == 1 { RED } else { WHITE };
                Some(quad(glyph, [cell * 10.0, 0.0], color))
            })
            .collect();
        assert!(!expected.is_empty());
        assert_eq!(quads, expected);
        // Not the glyphs of the characters drawn one by one.
        let mut cells = Vec::new();
        resolve(
            &mut glyphs,
            &[item('-', [0.0, 0.0]), item('>', [10.0, 0.0])],
            &mut cells,
        );
        assert_ne!(quads, cells);
    }

    #[test]
    fn group_runs_follow_its_glyphs_in_its_range() {
        let mut glyphs = Glyphs::with_test_font(&FontOptions::default(), FONT_SIZE);
        let single = [item('x', [0.0, 0.0])];
        let runs = [run("ab", [0.0, 20.0], 10.0)];
        let groups = [
            GlyphGroup {
                glyphs: &single,
                runs: &runs,
            },
            GlyphGroup::default(),
            GlyphGroup::from(single.as_slice()),
        ];
        let mut out = Vec::new();
        let ranges = resolve_groups(&mut glyphs, &groups, &mut out);
        assert_eq!(ranges, [0..3, 3..3, 3..4]);
        assert_eq!(out[1..3], resolved_runs(&mut glyphs, &runs)[..]);
    }

    #[test]
    fn runs_are_resolved_again_after_an_atlas_reset() {
        const ATLAS: u32 = 64;
        let chars: String = ('A'..='Z').chain('0'..='9').collect();
        let key = |c: char| GlyphKey {
            c,
            bold: false,
            italic: false,
            ui: None,
        };
        let mut probe = Glyphs::with_atlas_size(&FontOptions::default(), FONT_SIZE, ATLAS);
        let capacity = chars
            .chars()
            .position(|c| {
                probe.get(key(c));
                probe.generation() > 0
            })
            .expect("the atlas never filled up");
        assert!(capacity >= 2 && capacity + 2 <= chars.len(), "{capacity}");

        // Leave room for two glyphs: the third glyph of the run resets the atlas.
        let mut glyphs = Glyphs::with_atlas_size(&FontOptions::default(), FONT_SIZE, ATLAS);
        for c in chars[..capacity - 2].chars() {
            glyphs.get(key(c));
        }
        let text = &chars[capacity - 2..capacity + 2];
        let runs = [run(text, [0.0, 0.0], 10.0)];
        let mut out = Vec::new();
        let groups = [GlyphGroup {
            glyphs: &[],
            runs: &runs,
        }];
        resolve_groups(&mut glyphs, &groups, &mut out);
        assert_eq!(glyphs.generation(), 1, "the run did not reset the atlas");
        // Every quad points to the region its glyph holds in the current atlas.
        assert_eq!(out.len(), text.len());
        for (quad, c) in out.iter().zip(text.chars()) {
            let glyph = glyphs.get(key(c)).unwrap();
            assert_eq!(quad.uv, [glyph.region.x as f32, glyph.region.y as f32]);
        }
    }
}
