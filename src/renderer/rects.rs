//! Instanced rendering of solid shapes (cell backgrounds, cursor, underlines, cards,
//! chevrons, hexagons): rectangles, optionally with rounded corners, chevron strokes and
//! hexagon outlines, antialiased by a signed distance in the fragment shader.

use std::borrow::Cow;
use std::mem;
use std::ops::Range;

use bytemuck::{Pod, Zeroable};

use super::pipeline::{InstanceBuffer, ScreenUniform, instanced_pipeline, uniform_entry};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct RectInstance {
    pub position: [f32; 2],
    pub size: [f32; 2],
    /// Linear RGBA color.
    pub color: [f32; 4],
    /// Corner radius in pixels, for [`Shape::Rect`]. 0 for square corners. Half the
    /// stroke width, for [`Shape::Chevron`].
    pub radius: f32,
    /// A [`Shape`] as `u32`.
    pub shape: u32,
    /// Bend of a [`Shape::Chevron`], from -1 (a V pointing up) through 0 (a straight
    /// line) to 1 (a V pointing down).
    pub bend: f32,
    _padding: u32,
}

/// What a [`RectInstance`] draws inside its box.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// The whole box, corners rounded by the instance's radius.
    Rect = 0,
    /// A stroke with round ends across the box, bent into a V by the instance's bend:
    /// its ends at the box's left and right edges, its middle at the box's horizontal
    /// center, the bend spanning the box's height. The stroke is the instance's radius
    /// on each side of that line, inside the box.
    Chevron = 1,
    /// The outline of a regular hexagon with a vertex at the middle of the box's top
    /// and bottom edges, its flat sides on the box's left and right edges: a box
    /// `sqrt(3)` sides wide and 2 sides high holds it exactly. The stroke is twice the
    /// instance's radius, inside the box.
    Hexagon = 2,
    /// Darkens one row of physical pixels out of 3 with the instance's color, leaves the
    /// others untouched: the scanlines of the hidden CRT theme.
    Scanlines = 3,
}

/// Color of a dark row of [`Shape::Scanlines`]: black at 18 % opacity.
pub const SCANLINE_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.18];

impl RectInstance {
    /// A plain rectangle with square corners, drawn exactly as the rasterizer covers it.
    pub fn new(position: [f32; 2], size: [f32; 2], color: [f32; 4]) -> Self {
        Self {
            position,
            size,
            color,
            radius: 0.0,
            shape: Shape::Rect as u32,
            bend: 0.0,
            _padding: 0,
        }
    }

    /// The same box with its corners rounded by `radius` pixels.
    pub fn with_radius(self, radius: f32) -> Self {
        Self { radius, ..self }
    }

    /// The same box drawing a [`Shape::Chevron`] `stroke` pixels wide, bent by `bend`.
    pub fn chevron(self, stroke: f32, bend: f32) -> Self {
        Self {
            radius: stroke / 2.0,
            shape: Shape::Chevron as u32,
            bend,
            ..self
        }
    }

    /// The same box drawing the outline of a [`Shape::Hexagon`], `stroke` pixels wide.
    pub fn hexagon(self, stroke: f32) -> Self {
        Self {
            radius: stroke / 2.0,
            ..self.with_shape(Shape::Hexagon)
        }
    }

    /// Scanlines over the box at `position` of `size`, see [`Shape::Scanlines`].
    pub fn scanlines(position: [f32; 2], size: [f32; 2]) -> Self {
        Self::new(position, size, SCANLINE_COLOR).with_shape(Shape::Scanlines)
    }

    /// The same box drawing `shape`.
    pub fn with_shape(self, shape: Shape) -> Self {
        Self {
            shape: shape as u32,
            ..self
        }
    }
}

pub struct RectRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: ScreenUniform,
    bind_group: wgpu::BindGroup,
    instance_buffer: InstanceBuffer<RectInstance>,
}

const INITIAL_CAPACITY: usize = 1024;

impl RectRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rect shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("rect.wgsl"))),
        });

        let uniform_buffer = ScreenUniform::new(device, "rect uniforms");

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rect bind group layout"),
            entries: &[uniform_entry(0)],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rect bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rect pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            ..Default::default()
        });

        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<RectInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2,
                1 => Float32x2,
                2 => Float32x4,
                3 => Float32,
                4 => Uint32,
                5 => Float32
            ],
        };

        let pipeline = instanced_pipeline(
            device,
            "rect pipeline",
            &pipeline_layout,
            &shader,
            format,
            instance_layout,
        );

        let instance_buffer = InstanceBuffer::new(device, "rect instances", INITIAL_CAPACITY);

        Self {
            pipeline,
            uniform_buffer,
            bind_group,
            instance_buffer,
        }
    }

    /// Uploads all rectangles for the frame.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: (u32, u32),
        rects: &[RectInstance],
    ) {
        self.uniform_buffer.write(queue, screen);
        self.instance_buffer.write(device, queue, rects);
    }

    /// Draws a range of the rectangles uploaded by [`Self::prepare`].
    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>, range: Range<u32>) {
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice());
        pass.draw(0..4, range);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: u32 = 64;
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    /// Draws `rects` over a black `SIZE` x `SIZE` target and reads it back as RGBA rows.
    /// `None` when no GPU adapter is available.
    fn draw(rects: &[RectInstance]) -> Option<Vec<u8>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut renderer = RectRenderer::new(&device, format);
        renderer.prepare(&device, &queue, (SIZE, SIZE), rects);

        let bytes_per_row = SIZE * 4;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(bytes_per_row * SIZE),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            renderer.render(&mut pass, 0..rects.len() as u32);
        }
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(SIZE),
                },
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));
        buffer.map_async(wgpu::MapMode::Read, .., |result| result.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let pixels = buffer.get_mapped_range(..).unwrap().to_vec();
        Some(pixels)
    }

    /// Red channel of the pixel at `(x, y)`.
    fn red(pixels: &[u8], x: u32, y: u32) -> u8 {
        pixels[((y * SIZE + x) * 4) as usize]
    }

    #[test]
    fn instance_stays_pod_and_aligned() {
        assert_eq!(mem::size_of::<RectInstance>(), 48);
        let rect = RectInstance::new([1.0, 2.0], [3.0, 4.0], WHITE);
        assert_eq!((rect.radius, rect.shape), (0.0, Shape::Rect as u32));
        let rounded = rect.with_radius(5.0);
        assert_eq!((rounded.radius, rounded.shape), (5.0, Shape::Rect as u32));
        let chevron = rect.chevron(3.0, -0.5);
        assert_eq!(
            (chevron.radius, chevron.shape, chevron.bend),
            (1.5, Shape::Chevron as u32, -0.5)
        );
        assert_eq!(chevron.position, rect.position);
        let hexagon = rect.hexagon(2.0);
        assert_eq!(
            (hexagon.radius, hexagon.shape),
            (1.0, Shape::Hexagon as u32)
        );
    }

    #[test]
    fn hexagon_covers_its_center_but_not_the_corners_of_its_box() {
        // A pointy-top hexagon of side 24 in a 41.57 x 48 box at (11, 8): a stroke as
        // wide as its inradius fills it.
        let side = 24.0_f32;
        let size = [3.0_f32.sqrt() * side, 2.0 * side];
        let position = [11.0, 8.0];
        let inradius = size[0] / 2.0;
        let filled = RectInstance::new(position, size, WHITE).hexagon(inradius);
        let Some(pixels) = draw(&[filled]) else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        let (cx, cy) = (32, 32);
        assert_eq!(red(&pixels, cx, cy), 255, "center");
        // The corners of the box are outside the hexagon.
        for (x, y) in [(12, 9), (51, 9), (12, 55), (51, 55)] {
            assert_eq!(red(&pixels, x, y), 0, "corner ({x}, {y})");
        }
        // Its vertices touch the middle of the top and bottom edges, its flat sides
        // the left and right ones.
        for (x, y) in [(cx, 11), (cx, 52), (12, cy), (51, cy)] {
            assert_eq!(red(&pixels, x, y), 255, "edge ({x}, {y})");
        }

        // A thin outline leaves the center empty and lights its sides.
        let outline = RectInstance::new(position, size, WHITE).hexagon(3.0);
        let pixels = draw(&[outline]).unwrap();
        assert_eq!(red(&pixels, cx, cy), 0, "center of the outline");
        assert!(red(&pixels, 12, cy) > 127, "left side");
        assert!(red(&pixels, 51, cy) > 127, "right side");
        for (x, y) in [(12, 9), (51, 9), (12, 55), (51, 55)] {
            assert_eq!(red(&pixels, x, y), 0, "corner ({x}, {y})");
        }
    }

    #[test]
    fn rounded_rect_leaves_its_corners_at_the_clear_color() {
        let rect = RectInstance::new([8.0, 8.0], [48.0, 48.0], WHITE).with_radius(12.0);
        let Some(pixels) = draw(&[rect]) else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        for (x, y) in [(8, 8), (55, 8), (8, 55), (55, 55)] {
            assert_eq!(red(&pixels, x, y), 0, "corner ({x}, {y})");
        }
        assert_eq!(red(&pixels, 32, 32), 255);
        // Straight edges stay filled away from the corners.
        assert_eq!(red(&pixels, 8, 32), 255);
        assert_eq!(red(&pixels, 32, 8), 255);
        // Antialiased curve: partial coverage somewhere along a corner.
        assert!((8..20).any(|i| matches!(red(&pixels, i, i), 1..=254)));
    }

    /// Middle of the lit rows of column `x`.
    fn lit_center(pixels: &[u8], x: u32) -> f32 {
        let lit: Vec<u32> = (0..SIZE).filter(|&y| red(pixels, x, y) > 127).collect();
        assert!(!lit.is_empty(), "column {x} unlit");
        lit.iter().sum::<u32>() as f32 / lit.len() as f32 + 0.5
    }

    #[test]
    fn chevron_bends_from_a_v_pointing_down_to_a_v_pointing_up() {
        // A 48 x 16 box at (8, 24), a 4 px stroke: 12 px of bend room.
        let draw_bent = |bend: f32| {
            draw(&[RectInstance::new([8.0, 24.0], [48.0, 16.0], WHITE).chevron(4.0, bend)])
        };
        let Some(down) = draw_bent(1.0) else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        let flat = draw_bent(0.0).unwrap();
        let up = draw_bent(-1.0).unwrap();
        for pixels in [&down, &flat, &up] {
            for y in 0..SIZE {
                for x in 0..SIZE {
                    if !(8..56).contains(&x) || !(24..40).contains(&y) {
                        assert_eq!(red(pixels, x, y), 0, "outside ({x}, {y})");
                    }
                }
            }
        }
        // Middle (x 32) against an end (x 12): 12 px of bend room, 20 / 22 of it
        // between those columns.
        let drop = |pixels: &[u8]| lit_center(pixels, 32) - lit_center(pixels, 12);
        assert!((drop(&down) - 10.9).abs() < 1.0, "down {}", drop(&down));
        assert!(drop(&flat).abs() < 0.5, "flat {}", drop(&flat));
        assert!((lit_center(&flat, 32) - 32.0).abs() < 0.5);
        assert!((drop(&up) + 10.9).abs() < 1.0, "up {}", drop(&up));
    }

    #[test]
    fn scanlines_darken_one_row_out_of_three() {
        // Over a white background, scanlines on the left half only.
        let background = RectInstance::new([0.0, 0.0], [SIZE as f32, SIZE as f32], WHITE);
        let lines = RectInstance::scanlines([0.0, 0.0], [32.0, SIZE as f32]);
        assert_eq!(lines.shape, Shape::Scanlines as u32);
        let Some(pixels) = draw(&[background, lines]) else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        // 255 * (1 - 0.18), give or take the rounding.
        let dark = 209;
        for y in 0..SIZE {
            let left = red(&pixels, 10, y);
            if y % 3 == 0 {
                assert!(left.abs_diff(dark) <= 1, "row {y}: {left}");
            } else {
                assert_eq!(left, 255, "row {y}");
            }
            // Outside the scanlines rect, nothing changes.
            assert_eq!(red(&pixels, 40, y), 255, "row {y} outside");
        }
    }

    #[test]
    fn plain_rect_covers_whole_pixels_without_antialiasing() {
        // Off the pixel grid: the rasterizer covers rows and columns whose centers
        // fall inside, in full color, as before shapes existed.
        let rect = RectInstance::new([10.3, 10.3], [20.0, 1.0], WHITE);
        let Some(pixels) = draw(&[rect]) else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        for x in 0..SIZE {
            let expected = if (10..30).contains(&x) { 255 } else { 0 };
            assert_eq!(red(&pixels, x, 10), expected, "column {x}");
            assert_eq!(red(&pixels, x, 9), 0);
            assert_eq!(red(&pixels, x, 11), 0);
        }
    }
}
