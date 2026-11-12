//! GPU renderer: owns the wgpu surface and draws [`Frame`]s.

mod rects;
pub mod scene;
mod text;

use std::sync::Arc;

use anyhow::Context;
use winit::window::Window;

use crate::frame::Frame;
use crate::layout::{CellSize, GridLayout};
use crate::perf;
use rects::RectRenderer;
use text::TextGrid;

/// GPU pipelines drawing a [`Frame`], shared by the window [`Renderer`] and the
/// [`Headless`] one used by the benchmarks.
struct Layers {
    rects: RectRenderer,
    text: TextGrid,
    srgb: bool,
}

/// What [`Layers::prepare`] uploaded, needed to record the draw calls.
struct Prepared {
    clear: [f32; 4],
    under_count: u32,
    total: u32,
}

impl Layers {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        family: Option<String>,
        font_size: f32,
    ) -> Self {
        Self {
            rects: RectRenderer::new(device, format),
            text: TextGrid::new(device, queue, format, family, font_size),
            srgb: format.is_srgb(),
        }
    }

    /// Builds the scene of `frame` and uploads it.
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: (u32, u32),
        frame: &Frame,
        layout: &GridLayout,
    ) -> anyhow::Result<Prepared> {
        let scene = scene::build(frame, layout, self.srgb);

        let mut rects = scene.under;
        let under_count = rects.len() as u32;
        rects.extend(scene.over);
        let total = rects.len() as u32;
        self.rects.prepare(device, queue, screen, &rects);
        self.text
            .prepare(device, queue, screen, layout, &scene.rows, &scene.overlay)?;
        Ok(Prepared {
            clear: scene.clear,
            under_count,
            total,
        })
    }

    /// Records the render pass drawing what [`Self::prepare`] uploaded into `view`.
    fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        prepared: &Prepared,
    ) -> anyhow::Result<()> {
        let [r, g, b, a] = prepared.clear.map(f64::from);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("terminal pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.rects.render(&mut pass, 0..prepared.under_count);
        self.text.render(&mut pass)?;
        self.rects
            .render(&mut pass, prepared.under_count..prepared.total);
        self.text.render_overlay(&mut pass)?;
        Ok(())
    }
}

pub struct Renderer {
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    layers: Layers,
    // Dropped last: the surface must not outlive the window.
    window: Arc<Window>,
}

impl Renderer {
    pub async fn new(
        window: Arc<Window>,
        display: winit::event_loop::OwnedDisplayHandle,
        family: Option<String>,
        font_size: f32,
    ) -> anyhow::Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(display),
        ));
        let surface = instance.create_surface(window.clone())?;
        perf::mark("GPU instance and surface created");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .context("no compatible GPU adapter")?;
        perf::mark("GPU adapter ready");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await?;
        perf::mark("GPU device ready");

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .context("surface has no supported format")?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);

        let layers = Layers::new(&device, &queue, format, family, font_size);
        perf::mark("renderer ready");

        Ok(Self {
            instance,
            device,
            queue,
            surface,
            config,
            layers,
            window,
        })
    }

    pub fn cell_size(&self) -> CellSize {
        self.layers.text.cell_size()
    }

    pub fn set_font_size(&mut self, font_size: f32) {
        self.layers.text.set_font_size(font_size);
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    /// Draws `frame`. Returns whether it reached the screen (`false` when the surface was
    /// not ready and a new redraw was requested or will come).
    pub fn render(&mut self, frame: &Frame, layout: &GridLayout) -> anyhow::Result<bool> {
        let screen = (self.config.width, self.config.height);
        let prepared = self
            .layers
            .prepare(&self.device, &self.queue, screen, frame, layout)?;

        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Suboptimal(_) => {
                self.surface.configure(&self.device, &self.config);
                self.window.request_redraw();
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface = self.instance.create_surface(self.window.clone())?;
                self.surface.configure(&self.device, &self.config);
                self.window.request_redraw();
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Validation => anyhow::bail!("surface validation error"),
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.layers.draw(&mut encoder, &view, &prepared)?;

        self.queue.submit(Some(encoder.finish()));
        self.window.pre_present_notify();
        self.queue.present(output);
        self.layers.text.trim();
        Ok(true)
    }
}

/// Renderer drawing into an offscreen texture, without window: used by the benchmarks
/// to measure the GPU preparation of a frame.
pub struct Headless {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target: wgpu::TextureView,
    screen: (u32, u32),
    layers: Layers,
}

impl Headless {
    /// Creates a `width` x `height` target on the default adapter. `None` when no GPU
    /// adapter is available.
    pub fn new(width: u32, height: u32, font_size: f32) -> Option<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("headless target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let target = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let layers = Layers::new(&device, &queue, format, None, font_size);
        Some(Self {
            device,
            queue,
            target,
            screen: (width, height),
            layers,
        })
    }

    pub fn cell_size(&self) -> CellSize {
        self.layers.text.cell_size()
    }

    /// Prepares, draws and submits `frame`, without waiting for the GPU.
    pub fn render(&mut self, frame: &Frame, layout: &GridLayout) -> anyhow::Result<()> {
        let prepared =
            self.layers
                .prepare(&self.device, &self.queue, self.screen, frame, layout)?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.layers.draw(&mut encoder, &self.target, &prepared)?;
        self.queue.submit(Some(encoder.finish()));
        self.layers.text.trim();
        Ok(())
    }
}
