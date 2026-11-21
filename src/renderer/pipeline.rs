//! Building blocks shared by the render pipelines of the layers: each one draws
//! instanced, alpha-blended triangle strips with a vertex uniform buffer at binding 0.
//! Their buffers skip the uploads of contents the GPU already has: a redraw of an
//! unchanged window writes nothing.

use std::marker::PhantomData;

use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    screen: [f32; 2],
    _padding: [f32; 2],
}

/// Uniform buffer holding the window size, read by the vertex shaders.
pub struct ScreenUniform {
    buffer: wgpu::Buffer,
    /// Size last written, `None` before the first write.
    screen: Option<(u32, u32)>,
}

impl ScreenUniform {
    pub fn new(device: &wgpu::Device, label: &str) -> Self {
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            screen: None,
        }
    }

    pub fn binding(&self) -> wgpu::BindingResource<'_> {
        self.buffer.as_entire_binding()
    }

    /// Writes `screen` unless it is the size already written. Returns whether it wrote.
    pub fn write(&mut self, queue: &wgpu::Queue, screen: (u32, u32)) -> bool {
        if self.screen == Some(screen) {
            return false;
        }
        self.screen = Some(screen);
        let uniforms = Uniforms {
            screen: [screen.0 as f32, screen.1 as f32],
            _padding: [0.0; 2],
        };
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(&uniforms));
        true
    }
}

/// Vertex buffer of `T` instances, grown as needed.
pub struct InstanceBuffer<T> {
    label: &'static str,
    buffer: wgpu::Buffer,
    /// Instances the buffer can hold.
    capacity: usize,
    /// Bytes last uploaded, at the start of the buffer.
    uploaded: Vec<u8>,
    instance: PhantomData<T>,
}

impl<T: Pod> InstanceBuffer<T> {
    pub fn new(device: &wgpu::Device, label: &'static str, capacity: usize) -> Self {
        Self {
            label,
            buffer: Self::create(device, label, capacity),
            capacity,
            uploaded: Vec::new(),
            instance: PhantomData,
        }
    }

    fn create(device: &wgpu::Device, label: &str, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (capacity * size_of::<T>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Uploads `items`, growing the buffer when they do not fit, unless they are the
    /// instances already uploaded. Returns whether it wrote.
    pub fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, items: &[T]) -> bool {
        let bytes: &[u8] = bytemuck::cast_slice(items);
        if items.len() > self.capacity {
            self.capacity = items.len().next_power_of_two();
            self.buffer = Self::create(device, self.label, self.capacity);
            self.uploaded.clear();
        } else if bytes == self.uploaded.as_slice() {
            return false;
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytes);
        }
        self.uploaded.clear();
        self.uploaded.extend_from_slice(bytes);
        true
    }

    pub fn slice(&self) -> wgpu::BufferSlice<'_> {
        self.buffer.slice(..)
    }
}

/// Layout entry of a uniform buffer read by the vertex shader.
pub fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Pipeline drawing one triangle strip per instance of `instances` with the `vs_main`
/// and `fs_main` entry points of `shader`, alpha-blended into a `format` target.
pub fn instanced_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    instances: wgpu::VertexBufferLayout<'_>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[Some(instances)],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
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
    })
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

    #[test]
    fn instance_buffer_skips_unchanged_uploads_and_grows() {
        let Some((device, queue)) = device() else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        let mut buffer = InstanceBuffer::<[f32; 2]>::new(&device, "test instances", 2);
        assert!(!buffer.write(&device, &queue, &[]), "nothing to upload yet");
        assert!(buffer.write(&device, &queue, &[[1.0, 2.0]]));
        assert!(!buffer.write(&device, &queue, &[[1.0, 2.0]]));
        assert!(buffer.write(&device, &queue, &[[1.0, 3.0]]));
        assert!(buffer.write(&device, &queue, &[[1.0, 3.0], [4.0, 5.0], [6.0, 7.0]]));
        assert_eq!(buffer.capacity, 4);
        assert!(!buffer.write(&device, &queue, &[[1.0, 3.0], [4.0, 5.0], [6.0, 7.0]]));
        assert!(
            buffer.write(&device, &queue, &[]),
            "the empty set is a change"
        );
    }

    #[test]
    fn screen_uniform_is_written_once_per_size() {
        let Some((device, queue)) = device() else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        let mut uniform = ScreenUniform::new(&device, "test uniforms");
        assert!(uniform.write(&queue, (10, 20)));
        assert!(!uniform.write(&queue, (10, 20)));
        assert!(uniform.write(&queue, (10, 21)));
    }

    #[test]
    fn uniform_entry_is_a_vertex_uniform_buffer_at_its_binding() {
        let entry = uniform_entry(3);
        assert_eq!(entry.binding, 3);
        assert_eq!(entry.visibility, wgpu::ShaderStages::VERTEX);
        assert!(matches!(
            entry.ty,
            wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            }
        ));
        assert_eq!(entry.count, None);
    }
}
