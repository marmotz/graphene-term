//! Instanced rendering of solid shapes (cell backgrounds, cursor, underlines, cards,
//! chevrons): rectangles, optionally with rounded corners, and triangles, antialiased
//! by a signed distance in the fragment shader.

use std::borrow::Cow;
use std::mem;
use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct RectInstance {
    pub position: [f32; 2],
    pub size: [f32; 2],
    /// Linear RGBA color.
    pub color: [f32; 4],
    /// Corner radius in pixels, for [`Shape::Rect`]. 0 for square corners.
    pub radius: f32,
    /// A [`Shape`] as `u32`.
    pub shape: u32,
    _padding: [u32; 2],
}

/// What a [`RectInstance`] draws inside its box.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// The whole box, corners rounded by the instance's radius.
    Rect = 0,
    /// A triangle pointing right: left edge of the box to the middle of its right edge.
    TriangleRight = 1,
    /// A triangle pointing down: top edge of the box to the middle of its bottom edge.
    TriangleDown = 2,
}

impl RectInstance {
    /// A plain rectangle with square corners, drawn exactly as the rasterizer covers it.
    pub fn new(position: [f32; 2], size: [f32; 2], color: [f32; 4]) -> Self {
        Self {
            position,
            size,
            color,
            radius: 0.0,
            shape: Shape::Rect as u32,
            _padding: [0; 2],
        }
    }

    /// The same box with its corners rounded by `radius` pixels.
    pub fn with_radius(self, radius: f32) -> Self {
        Self { radius, ..self }
    }

    /// The same box drawing `shape`.
    pub fn with_shape(self, shape: Shape) -> Self {
        Self {
            shape: shape as u32,
            ..self
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    screen: [f32; 2],
    _padding: [f32; 2],
}

pub struct RectRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    capacity: usize,
}

const INITIAL_CAPACITY: usize = 1024;

impl RectRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rect shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("rect.wgsl"))),
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rect uniforms"),
            contents: bytemuck::bytes_of(&Uniforms {
                screen: [1.0, 1.0],
                _padding: [0.0; 2],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rect bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rect bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
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
                4 => Uint32
            ],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(instance_layout)],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::default(),
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            cache: None,
            multiview_mask: None,
        });

        let instance_buffer = Self::create_instance_buffer(device, INITIAL_CAPACITY);

        Self {
            pipeline,
            uniform_buffer,
            bind_group,
            instance_buffer,
            capacity: INITIAL_CAPACITY,
        }
    }

    fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rect instances"),
            size: (capacity * mem::size_of::<RectInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Uploads all rectangles for the frame.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: (u32, u32),
        rects: &[RectInstance],
    ) {
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&Uniforms {
                screen: [screen.0 as f32, screen.1 as f32],
                _padding: [0.0; 2],
            }),
        );
        if rects.len() > self.capacity {
            self.capacity = rects.len().next_power_of_two();
            self.instance_buffer = Self::create_instance_buffer(device, self.capacity);
        }
        if !rects.is_empty() {
            queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(rects));
        }
    }

    /// Draws a range of the rectangles uploaded by [`Self::prepare`].
    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>, range: Range<u32>) {
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
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
        let shaped = rect.with_radius(5.0).with_shape(Shape::TriangleDown);
        assert_eq!((shaped.radius, shaped.shape), (5.0, 2));
        assert_eq!(shaped.position, rect.position);
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

    #[test]
    fn triangle_right_lights_its_box_only_with_the_tip_on_the_right() {
        let rect =
            RectInstance::new([8.0, 8.0], [48.0, 48.0], WHITE).with_shape(Shape::TriangleRight);
        let Some(pixels) = draw(&[rect]) else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        for y in 0..SIZE {
            for x in 0..SIZE {
                if !(8..56).contains(&x) || !(8..56).contains(&y) {
                    assert_eq!(red(&pixels, x, y), 0, "outside ({x}, {y})");
                }
            }
        }
        // Base along the left edge, tip at the middle of the right edge.
        assert_eq!(red(&pixels, 10, 12), 255);
        assert_eq!(red(&pixels, 10, 51), 255);
        assert_eq!(red(&pixels, 52, 32), 255);
        assert_eq!(red(&pixels, 52, 12), 0);
        assert_eq!(red(&pixels, 52, 51), 0);
    }

    #[test]
    fn triangle_down_lights_its_box_only_with_the_tip_at_the_bottom() {
        let rect =
            RectInstance::new([8.0, 8.0], [48.0, 48.0], WHITE).with_shape(Shape::TriangleDown);
        let Some(pixels) = draw(&[rect]) else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        for y in 0..SIZE {
            for x in 0..SIZE {
                if !(8..56).contains(&x) || !(8..56).contains(&y) {
                    assert_eq!(red(&pixels, x, y), 0, "outside ({x}, {y})");
                }
            }
        }
        // Base along the top edge, tip at the middle of the bottom edge.
        assert_eq!(red(&pixels, 12, 10), 255);
        assert_eq!(red(&pixels, 51, 10), 255);
        assert_eq!(red(&pixels, 32, 52), 255);
        assert_eq!(red(&pixels, 12, 52), 0);
        assert_eq!(red(&pixels, 51, 52), 0);
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
