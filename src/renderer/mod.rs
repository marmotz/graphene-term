//! GPU renderer: owns the wgpu surface and draws [`Frame`]s.

mod atlas;
mod cells;
mod glyphs;
mod rects;
pub mod scene;

use std::sync::Arc;

use anyhow::Context;
use winit::window::Window;

use crate::frame::Frame;
use crate::layout::{CellSize, GridLayout};
use crate::perf;
use cells::CellRenderer;
pub use glyphs::{GlyphKey, Glyphs};
use rects::RectRenderer;

/// GPU pipelines drawing a [`Frame`], shared by the window [`Renderer`] and the
/// [`Headless`] one used by the benchmarks.
struct Layers {
    rects: RectRenderer,
    cells: CellRenderer,
    /// Font system, cell metrics and glyph cache feeding `cells`.
    glyphs: Glyphs,
    srgb: bool,
}

/// What [`Layers::prepare`] uploaded, needed to record the draw calls.
struct Prepared {
    clear: [f32; 4],
    under_count: u32,
    total: u32,
}

impl Layers {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, glyphs: Glyphs) -> Self {
        Self {
            rects: RectRenderer::new(device, format),
            cells: CellRenderer::new(device, format, glyphs.atlas_size()),
            glyphs,
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
    ) -> Prepared {
        let scene = scene::build(frame, layout, self.srgb);

        let mut rects = scene.under;
        let under_count = rects.len() as u32;
        rects.extend(scene.over);
        let total = rects.len() as u32;
        self.rects.prepare(device, queue, screen, &rects);
        self.cells.prepare(
            device,
            queue,
            screen,
            &mut self.glyphs,
            &scene.glyphs,
            &scene.overlay,
        );
        Prepared {
            clear: scene.clear,
            under_count,
            total,
        }
    }

    /// Records the render pass drawing what [`Self::prepare`] uploaded into `view`.
    fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        prepared: &Prepared,
    ) {
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
        self.cells.render(&mut pass);
        self.rects
            .render(&mut pass, prepared.under_count..prepared.total);
        self.cells.render_overlay(&mut pass);
    }
}

/// Backends tried in order for the window. Vulkan alone skips probing the other
/// backends (~100 ms less at startup on the measured machine); all backends are the
/// fallback when Vulkan is missing or has no adapter compatible with the surface.
const BACKEND_ATTEMPTS: [wgpu::Backends; 2] = [wgpu::Backends::VULKAN, wgpu::Backends::all()];

/// Runs `attempt` for each of `backends` in order and returns the first success, or
/// the last error when every attempt fails.
async fn first_success<T, F, Fut>(backends: &[wgpu::Backends], mut attempt: F) -> anyhow::Result<T>
where
    F: FnMut(wgpu::Backends) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<T>>,
{
    let mut last_error = anyhow::anyhow!("no GPU backend to try");
    for &b in backends {
        match attempt(b).await {
            Ok(value) => return Ok(value),
            Err(error) => {
                log::info!("GPU backends {b:?} unusable: {error:#}");
                last_error = error;
            }
        }
    }
    Err(last_error)
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
    /// Does the GPU work for `window`. `glyphs` is created beforehand (CPU only), so
    /// the cell size is known and the shell spawned before this slow step.
    pub async fn new(
        window: Arc<Window>,
        display: winit::event_loop::OwnedDisplayHandle,
        glyphs: Glyphs,
    ) -> anyhow::Result<Self> {
        let size = window.inner_size();
        let (instance, surface, adapter) = first_success(&BACKEND_ATTEMPTS, |backends| {
            let window = window.clone();
            let display = display.clone();
            async move {
                let mut desc = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display));
                desc.backends = backends;
                let instance = wgpu::Instance::new(desc);
                let surface = instance.create_surface(window)?;
                perf::mark("GPU instance and surface created");
                let adapter = instance
                    .request_adapter(&wgpu::RequestAdapterOptions {
                        compatible_surface: Some(&surface),
                        ..Default::default()
                    })
                    .await
                    .with_context(|| format!("no compatible GPU adapter ({backends:?})"))?;
                Ok((instance, surface, adapter))
            }
        })
        .await?;
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

        let layers = Layers::new(&device, format, glyphs);
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
        self.layers.glyphs.cell_size()
    }

    pub fn set_font_size(&mut self, font_size: f32) {
        self.layers.glyphs.set_font_size(font_size);
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
            .prepare(&self.device, &self.queue, screen, frame, layout);

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
        self.layers.draw(&mut encoder, &view, &prepared);

        self.queue.submit(Some(encoder.finish()));
        self.window.pre_present_notify();
        self.queue.present(output);
        Ok(true)
    }
}

/// Renderer drawing into an offscreen texture, without window: used by the benchmarks
/// to measure the GPU preparation of a frame.
pub struct Headless {
    device: wgpu::Device,
    queue: wgpu::Queue,
    #[cfg_attr(not(test), allow(dead_code))]
    texture: wgpu::Texture,
    target: wgpu::TextureView,
    screen: (u32, u32),
    layers: Layers,
}

impl Headless {
    /// Creates a `width` x `height` target on the default adapter, drawing with
    /// `glyphs`. `None` when no GPU adapter is available.
    pub fn new(width: u32, height: u32, glyphs: Glyphs) -> Option<Self> {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let layers = Layers::new(&device, format, glyphs);
        Some(Self {
            device,
            queue,
            texture,
            target,
            screen: (width, height),
            layers,
        })
    }

    pub fn cell_size(&self) -> CellSize {
        self.layers.glyphs.cell_size()
    }

    /// Prepares, draws and submits `frame`, without waiting for the GPU.
    pub fn render(&mut self, frame: &Frame, layout: &GridLayout) -> anyhow::Result<()> {
        let prepared = self
            .layers
            .prepare(&self.device, &self.queue, self.screen, frame, layout);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.layers.draw(&mut encoder, &self.target, &prepared);
        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }
}

#[cfg(test)]
impl Headless {
    /// Waits for the GPU and reads the target back, as tightly packed RGBA rows.
    /// The target width must be a multiple of 64 pixels.
    fn read_pixels(&self) -> Vec<u8> {
        let (width, height) = self.screen;
        let bytes_per_row = width * 4;
        assert_eq!(bytes_per_row % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT, 0);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("headless readback"),
            size: u64::from(bytes_per_row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        buffer.map_async(wgpu::MapMode::Read, .., |result| result.unwrap());
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        buffer.get_mapped_range(..).unwrap().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::vte::ansi::Rgb;

    use super::*;
    use crate::frame::{BlockColors, FrameCell};
    use crate::layout::Rect;

    const SIZE: u32 = 64;
    const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
    const WHITE: Rgb = Rgb {
        r: 255,
        g: 255,
        b: 255,
    };

    fn cell(c: char) -> FrameCell {
        FrameCell {
            c,
            fg: WHITE,
            bg: BLACK,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            spacer: false,
        }
    }

    #[test]
    fn renderer_draws_with_the_glyphs_it_is_given() {
        let glyphs = Glyphs::new(None, 30.0);
        let expected = glyphs.cell_size();
        let Some(renderer) = Headless::new(SIZE, SIZE, glyphs) else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        assert_eq!(renderer.cell_size(), expected);
        assert_ne!(expected, Glyphs::new(None, 14.0).cell_size());
    }

    #[test]
    fn grid_glyph_pixels_reach_the_target_inside_their_cell() {
        let Some(mut renderer) = Headless::new(SIZE, SIZE, Glyphs::new(None, 14.0)) else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        let cell_size = renderer.cell_size();
        let layout = GridLayout::new(Rect::from_size(SIZE, SIZE), cell_size, 0.0);
        let frame = Frame {
            rows: vec![vec![cell(' '), cell('M'), cell(' ')]],
            decorations: Vec::new(),
            cards: Vec::new(),
            row_tops: vec![0],
            block_colors: BlockColors {
                success: WHITE,
                failure: WHITE,
                running: WHITE,
                dim: WHITE,
                card: WHITE,
                card_selected: WHITE,
            },
            cursor: None,
            background: BLACK,
            banner: None,
            lang: crate::i18n::Lang::En,
        };
        renderer.render(&frame, &layout).unwrap();
        let pixels = renderer.read_pixels();

        let lit = |x: u32, y: u32| pixels[((y * SIZE + x) * 4) as usize] > 0;
        let lit_columns: Vec<u32> = (0..SIZE)
            .filter(|&x| (0..SIZE).any(|y| lit(x, y)))
            .collect();
        let (first, last) = (lit_columns[0], *lit_columns.last().unwrap());
        // Only the 'M' of the second cell is drawn, inside that cell, after the margin
        // and the card padding (one pixel of antialiasing slack on each side).
        assert!(first as f32 >= layout.cell_x(1) - 1.0, "{lit_columns:?}");
        assert!(last as f32 <= layout.cell_x(2) + 1.0, "{lit_columns:?}");
        assert!((0..SIZE).all(|x| !lit(x, cell_size.height as u32 + 1)));
        // Full coverage somewhere: the mask is tinted with the cell color.
        assert!(pixels.chunks(4).any(|px| px[0] == 255));
    }

    #[test]
    fn rounded_card_leaves_its_corner_pixel_at_the_background() {
        let Some(mut renderer) = Headless::new(SIZE, SIZE, Glyphs::new(None, 14.0)) else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        let cell_size = renderer.cell_size();
        let layout = GridLayout::new(Rect::from_size(SIZE, SIZE), cell_size, 0.0);
        let frame = Frame {
            rows: vec![vec![cell(' ')]],
            decorations: Vec::new(),
            cards: vec![crate::frame::Card {
                rows: 0..1,
                clipped_top: false,
                status: None,
                chevron: None,
                selected: false,
                scroll: None,
            }],
            row_tops: vec![0],
            block_colors: BlockColors {
                success: WHITE,
                failure: WHITE,
                running: WHITE,
                dim: WHITE,
                card: WHITE,
                card_selected: WHITE,
            },
            cursor: None,
            background: BLACK,
            banner: None,
            lang: crate::i18n::Lang::En,
        };
        renderer.render(&frame, &layout).unwrap();
        let pixels = renderer.read_pixels();
        let red = |x: f32, y: f32| pixels[((y as u32 * SIZE + x as u32) * 4) as usize];
        let left = layout.grid_left() - layout.card_pad;
        // The top-left corner pixel is outside the rounded corner; the card middle is
        // fully covered.
        assert_eq!(red(left, 0.0), 0);
        assert_eq!(red(left + 6.0, cell_size.height / 2.0), 255);
    }

    #[test]
    fn vulkan_is_tried_first_then_all_backends() {
        assert_eq!(
            BACKEND_ATTEMPTS,
            [wgpu::Backends::VULKAN, wgpu::Backends::all()]
        );
    }

    #[test]
    fn first_success_stops_at_the_first_working_backends() {
        let mut tried = Vec::new();
        let result = pollster::block_on(first_success(&BACKEND_ATTEMPTS, |b| {
            tried.push(b);
            async move { Ok(b) }
        }));
        assert_eq!(result.unwrap(), wgpu::Backends::VULKAN);
        assert_eq!(tried, [wgpu::Backends::VULKAN]);
    }

    #[test]
    fn first_success_falls_back_when_vulkan_fails() {
        let mut tried = Vec::new();
        let result = pollster::block_on(first_success(&BACKEND_ATTEMPTS, |b| {
            tried.push(b);
            async move {
                if b == wgpu::Backends::VULKAN {
                    anyhow::bail!("no Vulkan adapter")
                }
                Ok(b)
            }
        }));
        assert_eq!(result.unwrap(), wgpu::Backends::all());
        assert_eq!(tried, BACKEND_ATTEMPTS);
    }

    #[test]
    fn first_success_returns_the_last_error_when_all_fail() {
        let result: anyhow::Result<()> =
            pollster::block_on(first_success(&BACKEND_ATTEMPTS, |b| async move {
                anyhow::bail!("failed with {b:?}")
            }));
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains(&format!("{:?}", wgpu::Backends::all())),
            "{message}"
        );
    }
}
