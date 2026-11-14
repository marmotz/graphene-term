//! GPU renderer: owns the wgpu surface and draws [`Frame`]s.

mod atlas;
mod cells;
mod glyphs;
mod images;
mod rects;
pub mod scene;
mod ui_text;
#[cfg(target_os = "linux")]
mod x11_presenter;

use std::ops::Range;
use std::sync::Arc;

use anyhow::Context;
use winit::window::Window;

use crate::frame::Frame;
use crate::layout::{CellSize, GridLayout, Rect};
use crate::perf;
use cells::CellRenderer;
pub use glyphs::{GlyphKey, Glyphs, UiFont};
use images::ImageRenderer;
pub use rects::RectInstance;
use rects::RectRenderer;
use scene::GlyphInstance;
use ui_text::UiText;
pub use ui_text::UiTextItem;

/// The interface drawn over the panes (tab bar, dividers, active pane border): its
/// rectangles, then its text over them. The overlay (confirmation dialog) is drawn
/// last, over everything else, the banner included.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiLayer {
    pub rects: Vec<RectInstance>,
    pub texts: Vec<UiTextItem>,
    pub overlay_rects: Vec<RectInstance>,
    pub overlay_texts: Vec<UiTextItem>,
}

/// One pane to draw: its frame and the layout placing its grid in the window.
#[derive(Clone, Copy)]
pub struct PaneFrame<'a> {
    pub frame: &'a Frame,
    pub layout: &'a GridLayout,
}

/// Clear color when there is no pane to take it from.
const NO_PANE_CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// Scissor rectangle `[x, y, width, height]` covering `rect`, in whole pixels and
/// inside the `screen`. `None` when nothing of `rect` is on screen.
fn scissor(rect: Rect, screen: (u32, u32)) -> Option<[u32; 4]> {
    let clamp = |value: f32, max: u32| (value.max(0.0) as u32).min(max);
    let left = clamp(rect.x.floor(), screen.0);
    let top = clamp(rect.y.floor(), screen.1);
    let right = clamp((rect.x + rect.width).ceil(), screen.0);
    let bottom = clamp((rect.y + rect.height).ceil(), screen.1);
    (right > left && bottom > top).then(|| [left, top, right - left, bottom - top])
}

/// GPU pipelines drawing a [`Frame`], shared by the window [`Renderer`] and the
/// [`Headless`] one used by the benchmarks.
struct Layers {
    rects: RectRenderer,
    images: ImageRenderer,
    cells: CellRenderer,
    /// Font system, cell metrics and glyph cache feeding `cells`.
    glyphs: Glyphs,
    /// Shaped lines of the interface text.
    ui_text: UiText,
    srgb: bool,
    /// Opacity of the clear color, 1.0 when the surface cannot be transparent.
    background_opacity: f32,
    /// Alpha representation expected by the presentation backend.
    alpha_mode: SurfaceAlphaMode,
}

/// What [`Layers::prepare`] uploaded for one pane, as ranges of the rect buffer. Its
/// glyphs are the glyph group of the same index.
struct PreparedPane {
    /// Clip of everything the pane draws; `None` when the pane is off screen.
    scissor: Option<[u32; 4]>,
    under: Range<u32>,
    over: Range<u32>,
}

/// What [`Layers::prepare`] uploaded, needed to record the draw calls.
struct Prepared {
    clear: [f32; 4],
    screen: (u32, u32),
    panes: Vec<PreparedPane>,
    /// Interface rects (tab bar, dividers, active pane border), over every pane; the
    /// interface text is the glyph group after the panes'.
    ui: Range<u32>,
    /// Banner rects, over the interface; their glyphs are the glyph group after the
    /// interface text.
    banner: Range<u32>,
    /// Overlay rects (dialog), last; their glyphs are the last glyph group.
    overlay: Range<u32>,
}

impl Layers {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, glyphs: Glyphs) -> Self {
        Self {
            rects: RectRenderer::new(device, format),
            images: ImageRenderer::new(device, format),
            cells: CellRenderer::new(device, format, glyphs.atlas_size()),
            glyphs,
            ui_text: UiText::default(),
            srgb: format.is_srgb(),
            background_opacity: 1.0,
            alpha_mode: SurfaceAlphaMode::Premultiplied,
        }
    }

    /// Builds the scene of each pane and uploads them with the interface `ui`.
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: (u32, u32),
        panes: &[PaneFrame<'_>],
        ui: &UiLayer,
    ) -> Prepared {
        let scenes: Vec<_> = panes
            .iter()
            .map(|pane| scene::build(pane.frame, pane.layout, self.srgb))
            .collect();

        let mut rects = Vec::new();
        let mut push = |items: &[RectInstance]| {
            let start = rects.len() as u32;
            rects.extend_from_slice(items);
            start..rects.len() as u32
        };
        let prepared_panes = panes
            .iter()
            .zip(&scenes)
            .map(|(pane, scene)| PreparedPane {
                scissor: scissor(pane.layout.rect, screen),
                under: push(&scene.under),
                over: push(&scene.over),
            })
            .collect();
        let ui_rects = push(&ui.rects);
        let banner_rects: Vec<_> = scenes
            .iter()
            .flat_map(|scene| scene.overlay_rects.iter().copied())
            .collect();
        let banner = push(&banner_rects);
        let overlay = push(&ui.overlay_rects);
        self.rects.prepare(device, queue, screen, &rects);
        let images: Vec<_> = scenes.iter().map(|scene| scene.images.as_slice()).collect();
        self.images.prepare(device, queue, screen, &images);

        let banner_glyphs: Vec<GlyphInstance> = scenes
            .iter()
            .flat_map(|scene| scene.overlay.iter().copied())
            .collect();
        let mut ui_glyphs = Vec::new();
        self.ui_text
            .glyphs(&mut self.glyphs, &ui.texts, &mut ui_glyphs);
        let mut overlay_glyphs = Vec::new();
        self.ui_text
            .glyphs(&mut self.glyphs, &ui.overlay_texts, &mut overlay_glyphs);
        let mut groups: Vec<&[GlyphInstance]> =
            scenes.iter().map(|scene| scene.glyphs.as_slice()).collect();
        groups.push(&ui_glyphs);
        groups.push(&banner_glyphs);
        groups.push(&overlay_glyphs);
        self.cells
            .prepare(device, queue, screen, &mut self.glyphs, &groups);

        Prepared {
            clear: background_clear(
                scenes.first().map_or(NO_PANE_CLEAR, |scene| scene.clear),
                self.background_opacity,
                self.alpha_mode,
            ),
            screen,
            panes: prepared_panes,
            ui: ui_rects,
            banner,
            overlay,
        }
    }

    /// Records the render pass drawing what [`Self::prepare`] uploaded into `view`:
    /// clear, then each pane clipped to its rectangle (`under` rects, images, grid
    /// glyphs, `over` rects), then over the whole window the interface rects and text, the
    /// banner rects and glyphs, and the overlay rects and text.
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
        for (index, pane) in prepared.panes.iter().enumerate() {
            let Some([x, y, width, height]) = pane.scissor else {
                continue;
            };
            pass.set_scissor_rect(x, y, width, height);
            self.rects.render(&mut pass, pane.under.clone());
            self.images.render(&mut pass, index);
            self.cells.render(&mut pass, index);
            self.rects.render(&mut pass, pane.over.clone());
        }
        let (width, height) = prepared.screen;
        pass.set_scissor_rect(0, 0, width, height);
        self.rects.render(&mut pass, prepared.ui.clone());
        self.cells.render(&mut pass, prepared.panes.len());
        self.rects.render(&mut pass, prepared.banner.clone());
        self.cells.render(&mut pass, prepared.panes.len() + 1);
        self.rects.render(&mut pass, prepared.overlay.clone());
        self.cells.render(&mut pass, prepared.panes.len() + 2);
    }
}

/// Backends tried in order for the window. Prefer Vulkan, then probe platform alternatives
/// if it cannot expose an alpha-capable surface.
#[cfg(target_os = "linux")]
const BACKEND_ATTEMPTS: [wgpu::Backends; 2] = [wgpu::Backends::VULKAN, wgpu::Backends::GL];
#[cfg(not(target_os = "linux"))]
const BACKEND_ATTEMPTS: [wgpu::Backends; 2] = [wgpu::Backends::VULKAN, wgpu::Backends::all()];

/// Returns the first alpha-capable backend; if none is available, keeps the first
/// working opaque backend so startup can continue.
async fn first_transparent<T, F, Fut>(
    backends: &[wgpu::Backends],
    mut attempt: F,
) -> anyhow::Result<(T, bool)>
where
    F: FnMut(wgpu::Backends) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<(T, bool)>>,
{
    let mut last_error = anyhow::anyhow!("no GPU backend to try");
    let mut opaque = None;
    for &b in backends {
        match attempt(b).await {
            Ok((value, true)) => return Ok((value, true)),
            Ok((value, false)) => {
                if opaque.is_none() {
                    opaque = Some(value);
                }
            }
            Err(error) => {
                log::info!("GPU backends {b:?} unusable: {error:#}");
                last_error = error;
            }
        }
    }
    opaque.map_or(Err(last_error), |value| Ok((value, false)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SurfaceAlphaMode {
    Premultiplied,
    PostMultiplied,
    Inherit,
    Opaque,
}

impl SurfaceAlphaMode {
    fn is_transparent(self) -> bool {
        self != Self::Opaque
    }
}

/// Surface alpha mode and the corresponding pixel representation.
fn alpha_mode(modes: &[wgpu::CompositeAlphaMode]) -> (wgpu::CompositeAlphaMode, SurfaceAlphaMode) {
    [
        (
            wgpu::CompositeAlphaMode::PreMultiplied,
            SurfaceAlphaMode::Premultiplied,
        ),
        (
            wgpu::CompositeAlphaMode::PostMultiplied,
            SurfaceAlphaMode::PostMultiplied,
        ),
        (wgpu::CompositeAlphaMode::Inherit, SurfaceAlphaMode::Inherit),
    ]
    .into_iter()
    .find(|(mode, _)| modes.contains(mode))
    .unwrap_or_else(|| {
        (
            modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            SurfaceAlphaMode::Opaque,
        )
    })
}

/// Applies opacity using the representation required by the surface. For `Inherit`,
/// wgpu passes the mode to the native WSI when configuring the surface; winit has
/// already requested an alpha-capable X11 visual or non-opaque Wayland surface.
fn background_clear(color: [f32; 4], opacity: f32, alpha_mode: SurfaceAlphaMode) -> [f32; 4] {
    match alpha_mode {
        SurfaceAlphaMode::Premultiplied | SurfaceAlphaMode::Inherit => {
            scene::premultiplied(color, opacity)
        }
        SurfaceAlphaMode::PostMultiplied => {
            let mut color = color;
            color[3] *= opacity;
            color
        }
        SurfaceAlphaMode::Opaque => scene::premultiplied(color, 1.0),
    }
}

pub struct Renderer {
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: Option<wgpu::Surface<'static>>,
    config: Option<wgpu::SurfaceConfiguration>,
    layers: Layers,
    alpha_mode: SurfaceAlphaMode,
    size: (u32, u32),
    offscreen: Option<OffscreenTarget>,
    #[cfg(target_os = "linux")]
    x11_presenter: Option<x11_presenter::X11Presenter>,
    // Dropped last: the surface must not outlive the window.
    window: Arc<Window>,
}

struct OffscreenTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readback: wgpu::Buffer,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    bytes_per_row: u32,
}

impl OffscreenTarget {
    fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> anyhow::Result<Self> {
        let width = width.max(1);
        let height = height.max(1);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("X11 ARGB render target"),
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
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let unpadded_bytes_per_row = width.checked_mul(4).context("X11 ARGB row size overflow")?;
        let bytes_per_row = unpadded_bytes_per_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("X11 ARGB readback"),
            size: u64::from(bytes_per_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Ok(Self {
            texture,
            view,
            readback,
            format,
            size: (width, height),
            bytes_per_row,
        })
    }
}

fn x11_offscreen_format(adapter: &wgpu::Adapter) -> Option<wgpu::TextureFormat> {
    let required = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
    [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
    ]
    .into_iter()
    .find(|format| {
        adapter
            .get_texture_format_features(*format)
            .allowed_usages
            .contains(required)
    })
}

impl Renderer {
    /// Does the GPU work for `window`. `glyphs` is created beforehand (CPU only), so
    /// the cell size is known and the shell spawned before this slow step.
    pub async fn new(
        window: Arc<Window>,
        display: winit::event_loop::OwnedDisplayHandle,
        glyphs: Glyphs,
        background_opacity: f32,
    ) -> anyhow::Result<Self> {
        let size = window.inner_size();
        let ((instance, surface, adapter, caps), _) =
            first_transparent(&BACKEND_ATTEMPTS, |backends| {
                let window = window.clone();
                let display = display.clone();
                async move {
                    let mut desc =
                        wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display));
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
                    let caps = surface.get_capabilities(&adapter);
                    let (_, alpha) = alpha_mode(&caps.alpha_modes);
                    Ok(((instance, surface, adapter, caps), alpha.is_transparent()))
                }
            })
            .await?;
        perf::mark("GPU adapter ready");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await?;
        perf::mark("GPU device ready");

        let (native_alpha_mode, mut alpha_mode) = alpha_mode(&caps.alpha_modes);
        #[cfg(target_os = "linux")]
        let mut x11_presenter = None;
        #[cfg(target_os = "linux")]
        if !alpha_mode.is_transparent() {
            match x11_presenter::X11Presenter::new(&window, &display) {
                Ok(presenter) => x11_presenter = presenter,
                Err(error) => log::warn!("X11 ARGB presentation unavailable: {error:#}"),
            }
        }
        #[cfg(target_os = "linux")]
        let mut x11_format = x11_presenter
            .as_ref()
            .and_then(|_| x11_offscreen_format(&adapter));
        #[cfg(target_os = "linux")]
        if x11_presenter.is_some() && x11_format.is_none() {
            log::warn!("GPU has no renderable 8-bit X11 ARGB offscreen format");
            x11_presenter = None;
        }
        #[cfg(target_os = "linux")]
        let using_x11_presenter = x11_presenter.is_some();
        #[cfg(not(target_os = "linux"))]
        let using_x11_presenter = false;

        let (format, config, offscreen) = if using_x11_presenter {
            #[cfg(target_os = "linux")]
            {
                let format = x11_format.take().context("missing X11 offscreen format")?;
                let target = OffscreenTarget::new(&device, format, size.width, size.height)?;
                alpha_mode = SurfaceAlphaMode::Premultiplied;
                (format, None, Some(target))
            }
            #[cfg(not(target_os = "linux"))]
            unreachable!()
        } else {
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
                alpha_mode: native_alpha_mode,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
                color_space: wgpu::SurfaceColorSpace::Auto,
            };
            surface.configure(&device, &config);
            (format, Some(config), None)
        };

        if using_x11_presenter {
            log::info!("using native X11 ARGB presentation instead of the opaque GPU surface");
        } else if !alpha_mode.is_transparent() {
            log::warn!(
                "no GPU backend exposes a transparent surface (selected modes: {:?}); the window will be opaque and window.background_opacity is ignored",
                caps.alpha_modes
            );
        } else {
            log::debug!("using surface alpha mode {native_alpha_mode:?} via {alpha_mode:?}");
        }

        let mut layers = Layers::new(&device, format, glyphs);
        layers.alpha_mode = alpha_mode;
        perf::mark("renderer ready");

        let mut renderer = Self {
            instance,
            device,
            queue,
            surface: (!using_x11_presenter).then_some(surface),
            config,
            layers,
            alpha_mode,
            size: (size.width.max(1), size.height.max(1)),
            offscreen,
            #[cfg(target_os = "linux")]
            x11_presenter,
            window,
        };
        renderer.set_background_opacity(background_opacity);
        Ok(renderer)
    }

    /// Sets the opacity of the default background, ignored when the surface cannot be
    /// transparent.
    pub fn set_background_opacity(&mut self, opacity: f32) {
        self.layers.background_opacity = if self.alpha_mode.is_transparent() {
            opacity
        } else {
            1.0
        };
    }

    pub fn cell_size(&self) -> CellSize {
        self.layers.glyphs.cell_size()
    }

    /// Whether the surface is sRGB, so colors must be linearized for it (see
    /// [`scene::gpu_color`]).
    pub fn is_srgb(&self) -> bool {
        self.layers.srgb
    }

    pub fn set_font_size(&mut self, font_size: f32) {
        self.layers.glyphs.set_font_size(font_size);
    }

    /// Changes the font family (`None`: the system monospace font) and size.
    pub fn set_font(&mut self, family: Option<String>, font_size: f32) {
        self.layers.glyphs.set_font(family, font_size);
    }

    /// Scales the interface font to a window of scale factor `scale`.
    pub fn set_ui_scale(&mut self, scale: f32) {
        self.layers.glyphs.set_ui_scale(scale);
    }

    /// Line height of the interface font `font`, in pixels.
    pub fn ui_line_height(&self, font: UiFont) -> f32 {
        self.layers.glyphs.ui_line_height(font)
    }

    /// Width of `text` in the interface font `font`, in pixels.
    pub fn ui_text_width(&mut self, font: UiFont, text: &str) -> f32 {
        self.layers.glyphs.shape_ui(font, text).width
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let size = (width.max(1), height.max(1));
        if let Some(target) = &self.offscreen {
            match OffscreenTarget::new(&self.device, target.format, size.0, size.1) {
                Ok(target) => self.offscreen = Some(target),
                Err(error) => {
                    log::error!("could not resize X11 ARGB render target: {error:#}");
                    return;
                }
            }
        }
        if let (Some(surface), Some(config)) = (&self.surface, &mut self.config) {
            config.width = size.0;
            config.height = size.1;
            surface.configure(&self.device, config);
        }
        self.size = size;
    }

    /// Draws `panes`, then the interface `ui` over them. Returns whether it reached the
    /// screen (`false` when the surface was not ready and a new redraw was requested or
    /// will come).
    pub fn render(&mut self, panes: &[PaneFrame<'_>], ui: &UiLayer) -> anyhow::Result<bool> {
        let screen = self.size;
        let prepared = self
            .layers
            .prepare(&self.device, &self.queue, screen, panes, ui);

        #[cfg(target_os = "linux")]
        if self.x11_presenter.is_some() {
            return self.render_x11(&prepared);
        }

        self.render_surface(&prepared)
    }

    fn render_surface(&mut self, prepared: &Prepared) -> anyhow::Result<bool> {
        let output = self
            .surface
            .as_ref()
            .context("GPU surface is missing")?
            .get_current_texture();

        let output = match output {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Suboptimal(_) => {
                self.surface
                    .as_ref()
                    .context("GPU surface is missing")?
                    .configure(
                        &self.device,
                        self.config.as_ref().context("surface config is missing")?,
                    );
                self.window.request_redraw();
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface = Some(self.instance.create_surface(self.window.clone())?);
                self.surface
                    .as_ref()
                    .context("GPU surface is missing")?
                    .configure(
                        &self.device,
                        self.config.as_ref().context("surface config is missing")?,
                    );
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
        self.layers.draw(&mut encoder, &view, prepared);

        self.queue.submit(Some(encoder.finish()));
        self.window.pre_present_notify();
        self.queue.present(output);
        Ok(true)
    }

    #[cfg(target_os = "linux")]
    fn render_x11(&mut self, prepared: &Prepared) -> anyhow::Result<bool> {
        let target = self
            .offscreen
            .as_ref()
            .context("X11 offscreen target is missing")?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.layers.draw(&mut encoder, &target.view, prepared);
        encoder.copy_texture_to_buffer(
            target.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &target.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(target.bytes_per_row),
                    rows_per_image: Some(target.size.1),
                },
            },
            wgpu::Extent3d {
                width: target.size.0,
                height: target.size.1,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);

        let (send, receive) = std::sync::mpsc::sync_channel(1);
        target
            .readback
            .map_async(wgpu::MapMode::Read, .., move |result| {
                let _ = send.send(result);
            });
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        receive
            .recv()
            .context("GPU readback callback was dropped")??;

        let mapped = target.readback.get_mapped_range(..)?;
        self.window.pre_present_notify();
        let present_result = self
            .x11_presenter
            .as_mut()
            .context("X11 presenter is missing")?
            .present(
                &mapped,
                target.size.0,
                target.size.1,
                target.bytes_per_row,
                target.format,
            );
        drop(mapped);
        target.readback.unmap();
        present_result?;
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

    /// Prepares, draws and submits `frame` alone, without waiting for the GPU.
    pub fn render(&mut self, frame: &Frame, layout: &GridLayout) -> anyhow::Result<()> {
        self.render_panes(&[PaneFrame { frame, layout }], &UiLayer::default())
    }

    /// Prepares, draws and submits `panes` and the interface `ui`, without waiting for
    /// the GPU.
    pub fn render_panes(&mut self, panes: &[PaneFrame<'_>], ui: &UiLayer) -> anyhow::Result<()> {
        let prepared = self
            .layers
            .prepare(&self.device, &self.queue, self.screen, panes, ui);
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

    #[test]
    fn alpha_mode_prefers_premultiplied_and_recognizes_other_modes() {
        use wgpu::CompositeAlphaMode::*;
        assert_eq!(
            alpha_mode(&[Opaque, Inherit, PreMultiplied]),
            (PreMultiplied, SurfaceAlphaMode::Premultiplied)
        );
        assert_eq!(
            alpha_mode(&[Opaque, Inherit]),
            (Inherit, SurfaceAlphaMode::Inherit)
        );
        assert_eq!(
            alpha_mode(&[Opaque, PostMultiplied]),
            (PostMultiplied, SurfaceAlphaMode::PostMultiplied)
        );
        assert_eq!(alpha_mode(&[Opaque]), (Opaque, SurfaceAlphaMode::Opaque));
        assert_eq!(alpha_mode(&[]), (Auto, SurfaceAlphaMode::Opaque));
    }

    #[test]
    fn clear_alpha_respects_surface_representation_at_opacity_endpoints_and_midpoint() {
        let color = [0.8, 0.4, 0.2, 1.0];
        for (opacity, alpha) in [(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)] {
            assert_eq!(
                background_clear(color, opacity, SurfaceAlphaMode::Premultiplied),
                [color[0] * alpha, color[1] * alpha, color[2] * alpha, alpha]
            );
            assert_eq!(
                background_clear(color, opacity, SurfaceAlphaMode::PostMultiplied),
                [color[0], color[1], color[2], alpha]
            );
            assert_eq!(
                background_clear(color, opacity, SurfaceAlphaMode::Inherit),
                [color[0] * alpha, color[1] * alpha, color[2] * alpha, alpha]
            );
        }
    }

    #[test]
    fn opaque_surface_ignores_configured_background_opacity() {
        assert_eq!(
            background_clear([0.8, 0.4, 0.2, 1.0], 0.25, SurfaceAlphaMode::Opaque),
            [0.8, 0.4, 0.2, 1.0]
        );
    }
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
            image: None,
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
            images: Vec::new(),
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
            images: Vec::new(),
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

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn scissor_covers_the_pane_in_whole_pixels() {
        assert_eq!(
            scissor(rect(10.5, 20.2, 30.0, 40.0), (800, 600)),
            Some([10, 20, 31, 41])
        );
        assert_eq!(
            scissor(rect(0.0, 0.0, 800.0, 600.0), (800, 600)),
            Some([0, 0, 800, 600])
        );
    }

    #[test]
    fn scissor_stays_inside_the_screen() {
        assert_eq!(
            scissor(rect(-5.0, 590.0, 100.0, 50.0), (800, 600)),
            Some([0, 590, 95, 10])
        );
        assert_eq!(scissor(rect(900.0, 0.0, 10.0, 10.0), (800, 600)), None);
        assert_eq!(scissor(rect(10.0, 10.0, 0.0, 10.0), (800, 600)), None);
    }

    fn blank_frame(rows: Vec<Vec<FrameCell>>) -> Frame {
        Frame {
            row_tops: (0..rows.len() as u32).map(|row| 2 * row).collect(),
            decorations: vec![Default::default(); rows.len()],
            rows,
            cards: Vec::new(),
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
            images: Vec::new(),
        }
    }

    #[test]
    fn image_pixels_reach_the_target_centered_in_their_box() {
        use crate::frame::FrameImage;
        use crate::graphics::{ImageCell, Pixels, SourceRect};

        let Some(mut renderer) = Headless::new(SIZE, SIZE, Glyphs::new(None, 14.0)) else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        let cell_size = renderer.cell_size();
        let layout = GridLayout::new(Rect::from_size(SIZE, SIZE), cell_size, 0.0);
        let image_cell = |col| FrameCell {
            image: Some(ImageCell {
                image: 1,
                placement: 0,
                row: 0,
                col,
            }),
            ..cell(' ')
        };
        let mut frame = blank_frame(vec![vec![image_cell(0), image_cell(1), cell(' ')]]);
        // A red 1 x 1 image in a box of 2 x 1 cells: a square as high as the cell,
        // centered in the box.
        frame.images.push(FrameImage {
            image: 1,
            placement: 0,
            cols: 2,
            rows: 1,
            source: SourceRect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            pixels: Pixels::new(1, 1, vec![255, 0, 0, 255]).unwrap(),
        });
        renderer.render(&frame, &layout).unwrap();
        let pixels = renderer.read_pixels();
        let at = |x: f32, y: f32| {
            let index = ((y as u32 * SIZE + x as u32) * 4) as usize;
            [pixels[index], pixels[index + 1], pixels[index + 2]]
        };
        let center_x = layout.cell_x(1);
        let center_y = layout.row_top(0) + cell_size.height / 2.0;
        assert_eq!(at(center_x, center_y), [255, 0, 0]);
        // The box sides beyond the square stay at the background.
        let side = (2.0 * cell_size.width - cell_size.height) / 2.0;
        if side >= 2.0 {
            assert_eq!(at(layout.cell_x(0) + 0.5, center_y), [0, 0, 0]);
        }
    }

    #[test]
    fn each_pane_is_clipped_to_its_rect_and_the_ui_drawn_over_all() {
        let Some(mut renderer) = Headless::new(SIZE, SIZE, Glyphs::new(None, 14.0)) else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        let cell_size = renderer.cell_size();
        let half = SIZE as f32 / 2.0;
        // The left pane's row of white backgrounds is far wider than its rect.
        let left = GridLayout::new(rect(0.0, 0.0, half, SIZE as f32), cell_size, 0.0);
        let right = GridLayout::new(rect(half, 0.0, half, SIZE as f32), cell_size, 0.0);
        let mut wide = vec![cell(' '); 20];
        for c in &mut wide {
            c.bg = WHITE;
        }
        let left_frame = blank_frame(vec![wide]);
        let right_frame = blank_frame(vec![vec![cell(' ')]]);
        // A full-width interface rect along the bottom row.
        let ui = UiLayer {
            rects: vec![RectInstance::new(
                [0.0, SIZE as f32 - 1.0],
                [SIZE as f32, 1.0],
                [1.0; 4],
            )],
            ..UiLayer::default()
        };
        renderer
            .render_panes(
                &[
                    PaneFrame {
                        frame: &left_frame,
                        layout: &left,
                    },
                    PaneFrame {
                        frame: &right_frame,
                        layout: &right,
                    },
                ],
                &ui,
            )
            .unwrap();
        let pixels = renderer.read_pixels();
        let red = |x: u32, y: u32| pixels[((y * SIZE + x) * 4) as usize];

        let y = (cell_size.height / 2.0) as u32;
        assert_eq!(
            red(half as u32 - 1, y),
            255,
            "left pane drawn up to its edge"
        );
        assert!(
            (half as u32..SIZE).all(|x| red(x, y) == 0),
            "left pane leaked into the right one"
        );
        assert!(
            (0..SIZE).all(|x| red(x, SIZE - 1) == 255),
            "ui rect clipped"
        );
    }

    #[test]
    fn ui_text_is_drawn_over_the_panes_at_its_position() {
        let Some(mut renderer) = Headless::new(SIZE, SIZE, Glyphs::new(None, 14.0)) else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        let layout = GridLayout::new(Rect::from_size(SIZE, SIZE), renderer.cell_size(), 0.0);
        let frame = blank_frame(vec![vec![cell(' ')]]);
        let line_height = renderer.layers.glyphs.ui_line_height(UiFont::Chrome);
        let ui = UiLayer {
            rects: Vec::new(),
            texts: vec![UiTextItem {
                text: "MM".to_owned(),
                x: 32.0,
                y: 16.0,
                color: [1.0; 4],
                max_width: 32.0,
                font: UiFont::Chrome,
            }],
            ..UiLayer::default()
        };
        renderer
            .render_panes(
                &[PaneFrame {
                    frame: &frame,
                    layout: &layout,
                }],
                &ui,
            )
            .unwrap();
        let pixels = renderer.read_pixels();
        let lit: Vec<(u32, u32)> = (0..SIZE)
            .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
            .filter(|&(x, y)| pixels[((y * SIZE + x) * 4) as usize] > 0)
            .collect();
        assert!(!lit.is_empty(), "no ui text drawn");
        // Inside the line box, one pixel of antialiasing slack.
        assert!(
            lit.iter()
                .all(|&(x, y)| x + 1 >= 32 && y + 1 >= 16 && (y as f32) <= 16.0 + line_height),
            "{lit:?}"
        );
    }

    #[test]
    fn overlay_is_drawn_over_the_interface_with_its_own_text() {
        let Some(mut renderer) = Headless::new(SIZE, SIZE, Glyphs::new(None, 14.0)) else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        let layout = GridLayout::new(Rect::from_size(SIZE, SIZE), renderer.cell_size(), 0.0);
        let frame = blank_frame(vec![vec![cell(' ')]]);
        let text = |x: f32, color: [f32; 4]| UiTextItem {
            text: "MM".to_owned(),
            x,
            y: 0.0,
            color,
            max_width: 32.0,
            font: UiFont::Chrome,
        };
        let half = SIZE as f32 / 2.0;
        // White interface text in both halves; the overlay covers the left half in
        // black and draws its own red text in the right half.
        let ui = UiLayer {
            texts: vec![text(0.0, [1.0; 4]), text(half, [1.0; 4])],
            overlay_rects: vec![RectInstance::new(
                [0.0, 0.0],
                [half, SIZE as f32],
                [0.0, 0.0, 0.0, 1.0],
            )],
            overlay_texts: vec![text(half, [1.0, 0.0, 0.0, 1.0])],
            ..UiLayer::default()
        };
        renderer
            .render_panes(
                &[PaneFrame {
                    frame: &frame,
                    layout: &layout,
                }],
                &ui,
            )
            .unwrap();
        let pixels = renderer.read_pixels();
        let lit =
            |x: u32, y: u32, channel: u32| pixels[((y * SIZE + x) * 4 + channel) as usize] > 0;
        let left_lit = (0..SIZE)
            .flat_map(|y| (0..half as u32).map(move |x| (x, y)))
            .any(|(x, y)| lit(x, y, 0) || lit(x, y, 1));
        assert!(!left_lit, "interface text shows through the overlay");
        let right_red = (0..SIZE)
            .flat_map(|y| (half as u32..SIZE).map(move |x| (x, y)))
            .any(|(x, y)| lit(x, y, 0));
        assert!(right_red, "overlay text not drawn");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn vulkan_is_tried_before_opengl() {
        assert_eq!(
            BACKEND_ATTEMPTS,
            [wgpu::Backends::VULKAN, wgpu::Backends::GL]
        );
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn vulkan_is_tried_before_other_backends() {
        assert_eq!(
            BACKEND_ATTEMPTS,
            [wgpu::Backends::VULKAN, wgpu::Backends::all()]
        );
    }

    #[test]
    fn first_transparent_prefers_transparency_over_a_working_opaque_backend() {
        let mut tried = Vec::new();
        let result = pollster::block_on(first_transparent(&BACKEND_ATTEMPTS, |b| {
            tried.push(b);
            async move { Ok((b, b == wgpu::Backends::GL)) }
        }));
        assert_eq!(result.unwrap(), (wgpu::Backends::GL, true));
        assert_eq!(tried, BACKEND_ATTEMPTS);
    }

    #[test]
    fn first_transparent_uses_opaque_backend_when_none_support_transparency() {
        let mut tried = Vec::new();
        let result = pollster::block_on(first_transparent(&BACKEND_ATTEMPTS, |b| {
            tried.push(b);
            async move { Ok((b, false)) }
        }));
        assert_eq!(result.unwrap(), (wgpu::Backends::VULKAN, false));
        assert_eq!(tried, BACKEND_ATTEMPTS);
    }

    #[test]
    fn first_transparent_falls_back_when_vulkan_fails() {
        let result = pollster::block_on(first_transparent(&BACKEND_ATTEMPTS, |b| async move {
            if b == wgpu::Backends::VULKAN {
                anyhow::bail!("no Vulkan adapter")
            }
            Ok((b, true))
        }));
        assert_eq!(result.unwrap(), (wgpu::Backends::GL, true));
    }

    #[test]
    fn first_transparent_returns_the_last_error_when_all_fail() {
        let result: anyhow::Result<((), bool)> =
            pollster::block_on(first_transparent(&BACKEND_ATTEMPTS, |b| async move {
                anyhow::bail!("failed with {b:?}")
            }));
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains(&format!("{:?}", wgpu::Backends::GL)),
            "{message}"
        );
    }
}
