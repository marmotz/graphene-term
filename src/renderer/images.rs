//! Textured quads of the terminal images. One texture per image frame ([`Pixels`]),
//! uploaded the first time a frame is drawn and dropped once no image store holds it
//! any more: a new frame or a changed image is a new `Pixels`, so a new texture.

use std::borrow::Cow;
use std::collections::HashMap;
use std::mem;
use std::ops::Range;
use std::sync::{Arc, Weak};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::scene::ImageQuad;
use crate::graphics::Pixels;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
struct ImageInstance {
    position: [f32; 2],
    size: [f32; 2],
    uv: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    screen: [f32; 2],
    _padding: [f32; 2],
}

/// The texture of one image frame.
struct Texture {
    pixels: Weak<Pixels>,
    /// `None` when the frame is larger than the GPU allows: it is not drawn.
    bind_group: Option<wgpu::BindGroup>,
}

/// Quads sharing one texture, as a range of the instance buffer.
struct Batch {
    uid: u64,
    instances: Range<u32>,
}

pub struct ImageRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Texture format of the frames: sRGB when the target is, so the sampled colors
    /// are linear like the rest of the scene.
    format: wgpu::TextureFormat,
    instance_buffer: wgpu::Buffer,
    capacity: usize,
    textures: HashMap<u64, Texture>,
    /// Batches of each group uploaded by [`Self::prepare`].
    groups: Vec<Vec<Batch>>,
}

const INITIAL_CAPACITY: usize = 64;

impl ImageRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("image.wgsl"))),
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("image uniforms"),
            contents: bytemuck::bytes_of(&Uniforms {
                screen: [1.0, 1.0],
                _padding: [0.0; 2],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image uniform layout"),
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
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image uniforms"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image pipeline layout"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&texture_layout)],
            ..Default::default()
        });
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<ImageInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2,
                1 => Float32x2,
                2 => Float32x4
            ],
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image pipeline"),
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

        Self {
            pipeline,
            uniform_buffer,
            uniform_bind_group,
            texture_layout,
            sampler,
            format: if format.is_srgb() {
                wgpu::TextureFormat::Rgba8UnormSrgb
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            },
            instance_buffer: Self::create_instance_buffer(device, INITIAL_CAPACITY),
            capacity: INITIAL_CAPACITY,
            textures: HashMap::new(),
            groups: Vec::new(),
        }
    }

    fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image instances"),
            size: (capacity * mem::size_of::<ImageInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Uploads the quads of each group (one per pane) and the textures they need, and
    /// drops the textures of frames no longer held by any image store.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: (u32, u32),
        groups: &[&[ImageQuad]],
    ) {
        self.textures
            .retain(|_, texture| texture.pixels.strong_count() > 0);
        self.groups.clear();
        let total: usize = groups.iter().map(|group| group.len()).sum();
        if total == 0 {
            return;
        }
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&Uniforms {
                screen: [screen.0 as f32, screen.1 as f32],
                _padding: [0.0; 2],
            }),
        );

        let mut instances = Vec::with_capacity(total);
        for group in groups {
            let mut batches: Vec<Batch> = Vec::new();
            for quad in group.iter() {
                let uid = quad.pixels.uid();
                self.upload(device, queue, &quad.pixels);
                let index = instances.len() as u32;
                instances.push(ImageInstance {
                    position: quad.position,
                    size: quad.size,
                    uv: quad.uv,
                });
                match batches.last_mut() {
                    Some(batch) if batch.uid == uid => batch.instances.end = index + 1,
                    _ => batches.push(Batch {
                        uid,
                        instances: index..index + 1,
                    }),
                }
            }
            self.groups.push(batches);
        }
        if instances.len() > self.capacity {
            self.capacity = instances.len().next_power_of_two();
            self.instance_buffer = Self::create_instance_buffer(device, self.capacity);
        }
        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));
    }

    /// Creates the texture of `pixels` unless it exists.
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, pixels: &Arc<Pixels>) {
        let uid = pixels.uid();
        if self.textures.contains_key(&uid) {
            return;
        }
        let max = device.limits().max_texture_dimension_2d;
        let bind_group = if pixels.width > max || pixels.height > max {
            log::warn!(
                "image of {}x{} px not drawn: larger than the GPU limit of {max} px",
                pixels.width,
                pixels.height
            );
            None
        } else {
            Some(self.create_texture(device, queue, pixels))
        };
        self.textures.insert(
            uid,
            Texture {
                pixels: Arc::downgrade(pixels),
                bind_group,
            },
        );
    }

    fn create_texture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pixels: &Pixels,
    ) -> wgpu::BindGroup {
        let size = wgpu::Extent3d {
            width: pixels.width,
            height: pixels.height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pixels.width * 4),
                rows_per_image: Some(pixels.height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image texture"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    /// Number of textures kept.
    #[cfg(test)]
    fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// Draws the quads of group `index` uploaded by [`Self::prepare`].
    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>, index: usize) {
        let Some(batches) = self.groups.get(index) else {
            return;
        };
        if batches.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        for batch in batches {
            let Some(bind_group) = self
                .textures
                .get(&batch.uid)
                .and_then(|texture| texture.bind_group.as_ref())
            else {
                continue;
            };
            pass.set_bind_group(1, bind_group, &[]);
            pass.draw(0..4, batch.instances.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
    }

    fn quad(pixels: &Arc<Pixels>) -> ImageQuad {
        ImageQuad {
            position: [0.0, 0.0],
            size: [4.0, 4.0],
            uv: [0.0, 0.0, 1.0, 1.0],
            pixels: pixels.clone(),
        }
    }

    #[test]
    fn textures_are_shared_by_quads_and_dropped_with_their_pixels() {
        let Some((device, queue)) = device() else {
            return;
        };
        let mut renderer = ImageRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        let a = Pixels::new(2, 2, vec![255; 16]).unwrap();
        let b = Pixels::new(1, 1, vec![255; 4]).unwrap();
        let first = [quad(&a), quad(&a)];
        let second = [quad(&b)];
        renderer.prepare(&device, &queue, (8, 8), &[&first, &second]);
        assert_eq!(renderer.texture_count(), 2);
        assert_eq!(renderer.groups[0].len(), 1, "two quads of a, one batch");
        assert_eq!(renderer.groups[0][0].instances, 0..2);
        assert_eq!(renderer.groups[1][0].instances, 2..3);

        drop((first, second, b));
        renderer.prepare(&device, &queue, (8, 8), &[]);
        assert_eq!(renderer.texture_count(), 1, "b left every store");
        assert!(renderer.groups.is_empty());
        drop(a);
    }
}
