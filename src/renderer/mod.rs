//! GPU renderer: owns the wgpu surface and draws [`Frame`]s.

mod atlas;
mod cells;
mod glyphs;
mod images;
mod offscreen;
mod pipeline;
pub mod progress_bar;
mod rects;
pub mod scene;
mod ui_text;
#[cfg(target_os = "linux")]
mod x11_presenter;

use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use winit::window::Window;

use crate::frame::Frame;
use crate::layout::{CellSize, GridLayout, Rect};
use crate::perf;
use cells::{CellRenderer, GlyphGroup};
pub use glyphs::{FontOptions, GlyphKey, Glyphs, GridSize, UiFont};
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

/// Records drawing over the window into the encoder and texture view of a frame, after
/// the interface and the banner and before the overlay: the settings tab, drawn with
/// egui. It also gets the size of that texture in pixels, which can lag behind the
/// window's own size until its `Resized` event: drawing past it is a GPU error.
pub type ContentPainter<'a> =
    &'a mut dyn FnMut(&mut wgpu::CommandEncoder, &wgpu::TextureView, (u32, u32));

/// One pane to draw: its frame, the layout placing its grid in the window and the size
/// of its grid font, whose cell is the layout's.
#[derive(Clone, Copy)]
pub struct PaneFrame<'a> {
    pub frame: &'a Frame,
    pub layout: &'a GridLayout,
    pub font_size: GridSize,
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
    /// Whether scanlines cover each pane, over its text: the hidden CRT theme is shown.
    scanlines: bool,
}

/// Appends `items` to `rects`, and then with `scanlines` a [`RectInstance::scanlines`]
/// covering `rect`; returns the range they take.
fn push_rects(
    rects: &mut Vec<RectInstance>,
    items: &[RectInstance],
    scanlines: Option<Rect>,
) -> Range<u32> {
    let start = rects.len() as u32;
    rects.extend_from_slice(items);
    if let Some(rect) = scanlines {
        rects.push(RectInstance::scanlines(
            [rect.x, rect.y],
            [rect.width, rect.height],
        ));
    }
    start..rects.len() as u32
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
            scanlines: false,
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
        let ligatures = self.glyphs.ligatures();
        let base_size = self.glyphs.font_size();
        let scenes: Vec<_> = panes
            .iter()
            .map(|pane| {
                scene::build(
                    pane.frame,
                    pane.layout,
                    pane.font_size,
                    base_size,
                    self.srgb,
                    ligatures,
                )
            })
            .collect();

        let mut rects = Vec::new();
        let scanlines = self.scanlines;
        let prepared_panes = panes
            .iter()
            .zip(&scenes)
            .map(|(pane, scene)| PreparedPane {
                scissor: scissor(pane.layout.rect, screen),
                under: push_rects(&mut rects, &scene.under, None),
                over: push_rects(
                    &mut rects,
                    &scene.over,
                    scanlines.then_some(pane.layout.rect),
                ),
            })
            .collect();
        let mut push = |items: &[RectInstance]| push_rects(&mut rects, items, None);
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
        let mut groups: Vec<GlyphGroup<'_>> = scenes
            .iter()
            .map(|scene| GlyphGroup {
                glyphs: &scene.glyphs,
                runs: &scene.runs,
            })
            .collect();
        groups.push(ui_glyphs.as_slice().into());
        groups.push(banner_glyphs.as_slice().into());
        groups.push(overlay_glyphs.as_slice().into());
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
    /// With `content`, it paints between the banner and the overlay, in a pass of its
    /// own: the overlay is drawn in a last pass.
    fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        prepared: &Prepared,
        content: Option<ContentPainter<'_>>,
    ) {
        let [r, g, b, a] = prepared.clear.map(f64::from);
        let mut pass = begin_pass(
            encoder,
            view,
            wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
        );
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
        if let Some(content) = content {
            drop(pass);
            content(encoder, view, prepared.screen);
            pass = begin_pass(encoder, view, wgpu::LoadOp::Load);
        }
        self.rects.render(&mut pass, prepared.overlay.clone());
        self.cells.render(&mut pass, prepared.panes.len() + 2);
    }
}

/// Starts the render pass drawing into `view`, which it first clears or keeps (`load`).
fn begin_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("terminal pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// How often a frame read back from the GPU is checked for, until it can be sent to
/// the X server.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const READBACK_POLL: Duration = Duration::from_millis(1);

/// Refresh period of a monitor refreshing `millihertz` times per thousand seconds,
/// 60 Hz when unknown.
#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
fn frame_interval(millihertz: Option<u32>) -> Duration {
    let millihertz = millihertz.filter(|&rate| rate > 0).unwrap_or(60_000);
    Duration::from_secs_f64(1000.0 / f64::from(millihertz))
}

/// When the frame after one drawn at `last` may be drawn, `interval` later; `None`
/// when that is not after `now`.
#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
fn next_frame(last: Option<Instant>, interval: Duration, now: Instant) -> Option<Instant> {
    last.map(|last| last + interval).filter(|&due| due > now)
}

/// Backends tried in order for the window. Prefer Vulkan, then probe platform alternatives
/// if it cannot expose an alpha-capable surface.
#[cfg(target_os = "linux")]
const BACKEND_ATTEMPTS: [wgpu::Backends; 2] = [wgpu::Backends::VULKAN, wgpu::Backends::GL];
#[cfg(not(target_os = "linux"))]
const BACKEND_ATTEMPTS: [wgpu::Backends; 2] = [wgpu::Backends::VULKAN, wgpu::Backends::all()];

/// Returns the first alpha-capable backend; if none is available, keeps the first
/// working opaque backend so startup can continue. With `opaque_enough`, the first
/// working backend is kept even when opaque: the next ones are not tried.
async fn first_transparent<T, F, Fut>(
    backends: &[wgpu::Backends],
    opaque_enough: bool,
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
            Ok((value, false)) if opaque_enough => return Ok((value, false)),
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

/// Whether a window the X11 presentation could draw is presented by the GPU surface
/// instead: it is opaque, and the surface takes the format the layers draw in
/// (`surface_fits`). The X11 presentation reads every frame back from the GPU.
#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
fn presents_directly(opacity: f32, surface_fits: bool) -> bool {
    surface_fits && opacity >= 1.0
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

/// GPU context shared by every window of the process: one instance, adapter, device
/// and queue, so a second window does not pay the GPU initialisation again.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// Creates the GPU context with the surface of the first window: the adapter must be
    /// compatible with it, so the backends of [`BACKEND_ATTEMPTS`] are tried in order on
    /// that surface. Returns the context and the surface, for [`Renderer::new`].
    pub async fn new(
        window: Arc<Window>,
        display: winit::event_loop::OwnedDisplayHandle,
    ) -> anyhow::Result<(Arc<Self>, wgpu::Surface<'static>)> {
        // On X11 an opaque surface is enough: the X11 presenter shows transparency.
        // Probing OpenGL there costs tens of milliseconds and fails on NVIDIA.
        let opaque_enough = matches!(
            wgpu::rwh::HasDisplayHandle::display_handle(&display)?.as_raw(),
            wgpu::rwh::RawDisplayHandle::Xlib(_) | wgpu::rwh::RawDisplayHandle::Xcb(_)
        );
        let ((instance, surface, adapter), _) =
            first_transparent(&BACKEND_ATTEMPTS, opaque_enough, |backends| {
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
                    Ok(((instance, surface, adapter), alpha.is_transparent()))
                }
            })
            .await?;
        perf::mark("GPU adapter ready");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await?;
        install_error_handler(&device);
        perf::mark("GPU device ready");
        let gpu = Self {
            instance,
            adapter,
            device,
            queue,
        };
        Ok((Arc::new(gpu), surface))
    }

    /// Creates a surface for another window on this context.
    pub fn create_surface(&self, window: Arc<Window>) -> anyhow::Result<wgpu::Surface<'static>> {
        Ok(self.instance.create_surface(window)?)
    }
}

/// Logs the GPU errors no error scope catches instead of letting wgpu panic on them:
/// the faulty frame is lost but the application goes on.
fn install_error_handler(device: &wgpu::Device) {
    device.on_uncaptured_error(Arc::new(|error| log::error!("GPU error: {error}")));
}

pub struct Renderer {
    surface: Option<wgpu::Surface<'static>>,
    config: Option<wgpu::SurfaceConfiguration>,
    layers: Layers,
    /// Format the layers draw in, see [`Self::format`].
    format: wgpu::TextureFormat,
    alpha_mode: SurfaceAlphaMode,
    size: (u32, u32),
    #[cfg(target_os = "linux")]
    offscreen: Option<offscreen::OffscreenTarget>,
    #[cfg(target_os = "linux")]
    x11_presenter: Option<x11_presenter::X11Presenter>,
    /// With `x11_presenter`: the window is opaque and presented by the surface, which
    /// skips the readback of each frame.
    #[cfg(target_os = "linux")]
    direct: bool,
    /// Shortest time between two frames presented through X11: the monitor refresh
    /// period. The GPU surface is paced by the monitor itself.
    #[cfg(target_os = "linux")]
    frame_interval: Duration,
    /// When the last frame was presented through X11.
    #[cfg(target_os = "linux")]
    last_x11_frame: Option<Instant>,
    // After the surface: a surface must not outlive its instance.
    gpu: Arc<Gpu>,
    // Dropped last: the surface must not outlive the window.
    window: Arc<Window>,
}

impl Renderer {
    /// Prepares the drawing of `window` on `gpu`, with `surface`, created for that
    /// window on `gpu` (by [`Gpu::new`] for the first window). `glyphs` is created
    /// beforehand (CPU only), so the cell size is known and the shell spawned before
    /// the slow GPU step.
    pub fn new(
        gpu: Arc<Gpu>,
        surface: wgpu::Surface<'static>,
        window: Arc<Window>,
        _display: winit::event_loop::OwnedDisplayHandle,
        glyphs: Glyphs,
        background_opacity: f32,
    ) -> anyhow::Result<Self> {
        let size = window.inner_size();
        let device = &gpu.device;
        let caps = surface.get_capabilities(&gpu.adapter);
        let (native_alpha_mode, alpha_mode) = alpha_mode(&caps.alpha_modes);
        #[cfg(target_os = "linux")]
        let mut alpha_mode = alpha_mode;
        #[cfg(target_os = "linux")]
        let mut x11_presenter = None;
        #[cfg(target_os = "linux")]
        if !alpha_mode.is_transparent() {
            match x11_presenter::X11Presenter::new(&window, &_display) {
                Ok(presenter) => x11_presenter = presenter,
                Err(error) => log::warn!("X11 ARGB presentation unavailable: {error:#}"),
            }
        }
        #[cfg(target_os = "linux")]
        let mut x11_format = x11_presenter
            .as_ref()
            .and_then(|_| x11_presenter::offscreen_format(&gpu.adapter));
        #[cfg(target_os = "linux")]
        if x11_presenter.is_some() && x11_format.is_none() {
            log::warn!("GPU has no renderable 8-bit X11 ARGB offscreen format");
            x11_presenter = None;
        }
        #[cfg(target_os = "linux")]
        let using_x11_presenter = x11_presenter.is_some();
        #[cfg(not(target_os = "linux"))]
        let using_x11_presenter = false;

        let surface_config = |format| wgpu::SurfaceConfiguration {
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
        #[cfg(target_os = "linux")]
        let mut offscreen = None;
        let (format, config) = if using_x11_presenter {
            #[cfg(target_os = "linux")]
            {
                let format = x11_format.take().context("missing X11 offscreen format")?;
                let target =
                    offscreen::OffscreenTarget::new(device, format, size.width, size.height)?;
                alpha_mode = SurfaceAlphaMode::Premultiplied;
                offscreen = Some(target);
                // An opaque window is presented by the surface when it takes the format
                // the layers draw in (see `Renderer::choose_x11_presentation`).
                let direct = caps
                    .formats
                    .contains(&format)
                    .then(|| surface_config(format));
                (format, direct)
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
            let config = surface_config(format);
            surface.configure(device, &config);
            (format, Some(config))
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

        let mut layers = Layers::new(device, format, glyphs);
        layers.alpha_mode = alpha_mode;
        perf::mark("renderer ready");

        let mut renderer = Self {
            surface: Some(surface),
            config,
            layers,
            format,
            alpha_mode,
            size: (size.width.max(1), size.height.max(1)),
            #[cfg(target_os = "linux")]
            offscreen,
            #[cfg(target_os = "linux")]
            x11_presenter,
            #[cfg(target_os = "linux")]
            direct: false,
            #[cfg(target_os = "linux")]
            frame_interval: frame_interval(
                window
                    .current_monitor()
                    .and_then(|monitor| monitor.refresh_rate_millihertz()),
            ),
            #[cfg(target_os = "linux")]
            last_x11_frame: None,
            gpu,
            window,
        };
        renderer.set_background_opacity(background_opacity);
        Ok(renderer)
    }

    /// GPU context this renderer draws with, shared with the other windows.
    pub fn gpu(&self) -> &Arc<Gpu> {
        &self.gpu
    }

    /// Sets the opacity of the default background, ignored when the surface cannot be
    /// transparent.
    pub fn set_background_opacity(&mut self, opacity: f32) {
        self.layers.background_opacity = if self.alpha_mode.is_transparent() {
            opacity
        } else {
            1.0
        };
        #[cfg(target_os = "linux")]
        self.choose_x11_presentation(opacity);
    }

    /// With the X11 presentation, presents an opaque window with the surface instead,
    /// and goes back to X11 when it becomes transparent.
    #[cfg(target_os = "linux")]
    fn choose_x11_presentation(&mut self, opacity: f32) {
        if self.x11_presenter.is_none() {
            return;
        }
        let direct = presents_directly(opacity, self.config.is_some());
        if direct == self.direct && direct == self.surface.is_some() {
            return;
        }
        if direct {
            if let Err(error) = self.finish_x11(true) {
                log::error!("X11 presentation failed: {error:#}");
            }
            let surface = match self.surface.take() {
                Some(surface) => surface,
                None => match self.gpu.create_surface(self.window.clone()) {
                    Ok(surface) => surface,
                    Err(error) => {
                        log::warn!("could not create the GPU surface: {error:#}");
                        return;
                    }
                },
            };
            if let Some(config) = &mut self.config {
                (config.width, config.height) = self.size;
                surface.configure(&self.gpu.device, config);
            }
            self.surface = Some(surface);
            log::info!("opaque window: presented by the GPU surface");
        } else {
            // The X11 presentation draws into the window itself: release the swapchain.
            self.surface = None;
            log::info!("transparent window: presented through X11");
        }
        self.direct = direct;
    }

    /// Cell of the base font size, see [`Glyphs::font_size`].
    pub fn cell_size(&self) -> CellSize {
        self.layers.glyphs.cell_size()
    }

    /// Cell of the grid font at `size`, measured on first use.
    pub fn cell_size_at(&mut self, size: GridSize) -> CellSize {
        self.layers.glyphs.cell_size_at(size)
    }

    /// Whether the surface is sRGB, so colors must be linearized for it (see
    /// [`scene::gpu_color`]).
    pub fn is_srgb(&self) -> bool {
        self.layers.srgb
    }

    /// Covers each pane with scanlines, over its text, or stops: on while the hidden CRT
    /// theme is shown.
    pub fn set_scanlines(&mut self, scanlines: bool) {
        self.layers.scanlines = scanlines;
    }

    /// Whether the panes are covered with scanlines, see [`Self::set_scanlines`].
    pub fn scanlines(&self) -> bool {
        self.layers.scanlines
    }

    /// Changes the base size of the grid font, see [`Glyphs::set_font_size`].
    pub fn set_font_size(&mut self, font_size: f32) {
        self.layers.glyphs.set_font_size(font_size);
    }

    /// Changes the font (family, ligatures, features) and size.
    pub fn set_font(&mut self, font: &FontOptions, font_size: f32) {
        self.layers.glyphs.set_font(font, font_size);
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
        #[cfg(target_os = "linux")]
        if let Err(error) = self.finish_x11(true) {
            log::error!("X11 presentation failed: {error:#}");
        }
        #[cfg(target_os = "linux")]
        if let Some(target) = &self.offscreen {
            match offscreen::OffscreenTarget::new(&self.gpu.device, target.format, size.0, size.1) {
                Ok(target) => self.offscreen = Some(target),
                Err(error) => {
                    log::error!("could not resize X11 ARGB render target: {error:#}");
                    return;
                }
            }
        }
        if let Some(config) = &mut self.config {
            (config.width, config.height) = size;
            if let Some(surface) = &self.surface {
                surface.configure(&self.gpu.device, config);
            }
        }
        self.size = size;
    }

    /// Draws `panes`, then the interface `ui` over them. Returns whether it reached the
    /// screen (`false` when the surface was not ready and a new redraw was requested or
    /// will come).
    pub fn render(&mut self, panes: &[PaneFrame<'_>], ui: &UiLayer) -> anyhow::Result<bool> {
        self.render_with(panes, ui, None)
    }

    /// [`Self::render`], with `content` painted over the interface, under the overlay.
    pub fn render_with(
        &mut self,
        panes: &[PaneFrame<'_>],
        ui: &UiLayer,
        content: Option<ContentPainter<'_>>,
    ) -> anyhow::Result<bool> {
        let screen = self.size;
        let prepared = self
            .layers
            .prepare(&self.gpu.device, &self.gpu.queue, screen, panes, ui);

        #[cfg(target_os = "linux")]
        if self.x11_presenter.is_some() && !self.direct {
            return self.render_x11(&prepared, content);
        }

        self.render_surface(&prepared, content)
    }

    /// Format of the textures frames are drawn into.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Configures the surface again and asks for a new frame, after it became outdated
    /// or was recreated.
    fn reconfigure_surface(&self) -> anyhow::Result<()> {
        self.surface
            .as_ref()
            .context("GPU surface is missing")?
            .configure(
                &self.gpu.device,
                self.config.as_ref().context("surface config is missing")?,
            );
        self.window.request_redraw();
        Ok(())
    }

    fn render_surface(
        &mut self,
        prepared: &Prepared,
        content: Option<ContentPainter<'_>>,
    ) -> anyhow::Result<bool> {
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
                self.reconfigure_surface()?;
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface = Some(self.gpu.create_surface(self.window.clone())?);
                self.reconfigure_surface()?;
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Validation => anyhow::bail!("surface validation error"),
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.layers.draw(&mut encoder, &view, prepared, content);

        self.gpu.queue.submit(Some(encoder.finish()));
        self.window.pre_present_notify();
        self.gpu.queue.present(output);
        Ok(true)
    }

    /// Draws the frame into the offscreen target and starts reading it back, without
    /// waiting for the GPU: [`Self::finish_presentation`] sends it to the X server.
    #[cfg(target_os = "linux")]
    fn render_x11(
        &mut self,
        prepared: &Prepared,
        content: Option<ContentPainter<'_>>,
    ) -> anyhow::Result<bool> {
        self.last_x11_frame = Some(Instant::now());
        // The previous frame reaches the window first.
        self.finish_x11(true)?;
        let target = self
            .offscreen
            .as_mut()
            .context("X11 offscreen target is missing")?;
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.layers
            .draw(&mut encoder, &target.view, prepared, content);
        target.start_read(&self.gpu.queue, encoder)?;
        Ok(true)
    }

    /// Sends the frame read back from the GPU to the X server, waiting for the GPU
    /// when `wait`. Returns whether a frame is still pending.
    #[cfg(target_os = "linux")]
    fn finish_x11(&mut self, wait: bool) -> anyhow::Result<bool> {
        let (Some(target), Some(presenter)) = (&mut self.offscreen, &mut self.x11_presenter) else {
            return Ok(false);
        };
        let (size, bytes_per_row, format) = (target.size, target.bytes_per_row, target.format);
        let window = &self.window;
        let presented = target.finish_read(&self.gpu.device, wait, |pixels| {
            window.pre_present_notify();
            presenter.present(pixels, size.0, size.1, bytes_per_row, format)
        })?;
        match presented {
            Some(result) => result.map(|()| false),
            None => Ok(target.is_reading()),
        }
    }

    /// When the next frame may be drawn, `None` when it may be now. Frames presented
    /// through X11 are not paced by the monitor: more than one per refresh period
    /// would never be seen.
    pub fn frame_due(&self, now: Instant) -> Option<Instant> {
        #[cfg(target_os = "linux")]
        if self.x11_presenter.is_some() && !self.direct {
            return next_frame(self.last_x11_frame, self.frame_interval, now);
        }
        #[cfg(not(target_os = "linux"))]
        let _ = now;
        None
    }

    /// Finishes the presentation of the last frame when it was left to the GPU: the
    /// X11 presentation reads frames back without waiting. Returns when to check
    /// again, `None` when no frame is pending.
    pub fn finish_presentation(&mut self, now: Instant) -> Option<Instant> {
        #[cfg(target_os = "linux")]
        match self.finish_x11(false) {
            Ok(true) => return Some(now + READBACK_POLL),
            Ok(false) => {}
            Err(error) => log::error!("X11 presentation failed: {error:#}"),
        }
        #[cfg(not(target_os = "linux"))]
        let _ = now;
        None
    }
}

/// Renderer drawing into an offscreen texture, without window: used by the benchmarks
/// to measure the GPU preparation and the presentation of a frame.
pub struct Headless {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target: offscreen::OffscreenTarget,
    screen: (u32, u32),
    layers: Layers,
    /// Packed pixels of the last frame read back by [`Self::present_x11`].
    pixels: Vec<u8>,
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
        let target = offscreen::OffscreenTarget::new(&device, format, width, height).ok()?;
        let layers = Layers::new(&device, format, glyphs);
        Some(Self {
            device,
            queue,
            target,
            screen: (width, height),
            layers,
            pixels: Vec::new(),
        })
    }

    pub fn cell_size(&self) -> CellSize {
        self.layers.glyphs.cell_size()
    }

    /// Prepares, draws and submits `frame` alone, without waiting for the GPU.
    pub fn render(&mut self, frame: &Frame, layout: &GridLayout) -> anyhow::Result<()> {
        let font_size = self.layers.glyphs.font_size();
        let pane = PaneFrame {
            frame,
            layout,
            font_size,
        };
        self.render_panes(&[pane], &UiLayer::default())
    }

    /// Prepares, draws and submits `panes` and the interface `ui`, without waiting for
    /// the GPU.
    pub fn render_panes(&mut self, panes: &[PaneFrame<'_>], ui: &UiLayer) -> anyhow::Result<()> {
        let encoder = self.encode(panes, ui);
        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    fn encode(&mut self, panes: &[PaneFrame<'_>], ui: &UiLayer) -> wgpu::CommandEncoder {
        let prepared = self
            .layers
            .prepare(&self.device, &self.queue, self.screen, panes, ui);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.layers
            .draw(&mut encoder, &self.target.view, &prepared, None);
        encoder
    }

    /// Draws `frame` and waits for the GPU: a frame presented by the GPU surface.
    pub fn present_direct(&mut self, frame: &Frame, layout: &GridLayout) -> anyhow::Result<()> {
        self.render(frame, layout)?;
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        Ok(())
    }

    /// Draws `frame`, reads it back and packs it as an X11 ARGB image, like the X11
    /// presentation does before sending it to the server. Returns the packed bytes.
    pub fn present_x11(&mut self, frame: &Frame, layout: &GridLayout) -> anyhow::Result<&[u8]> {
        self.start_x11(frame, layout)?;
        self.finish_x11(true)?;
        Ok(&self.pixels)
    }

    /// Draws `frame` and starts reading it back, without waiting for the GPU: the
    /// part of an X11 presentation a redraw runs.
    pub fn start_x11(&mut self, frame: &Frame, layout: &GridLayout) -> anyhow::Result<()> {
        let font_size = self.layers.glyphs.font_size();
        let pane = PaneFrame {
            frame,
            layout,
            font_size,
        };
        let encoder = self.encode(&[pane], &UiLayer::default());
        self.target.start_read(&self.queue, encoder)
    }

    /// Packs the frame read back since [`Self::start_x11`] as an X11 ARGB image,
    /// waiting for the GPU when `wait`. Returns whether it was read back.
    pub fn finish_x11(&mut self, wait: bool) -> anyhow::Result<bool> {
        let target = &mut self.target;
        let (size, bytes_per_row, format) = (target.size, target.bytes_per_row, target.format);
        let pixels = &mut self.pixels;
        let packed = target.finish_read(&self.device, wait, |source| {
            offscreen::PixelLayout::ARGB32.pack(
                source,
                size.0,
                size.1,
                bytes_per_row,
                format,
                pixels,
            )
        })?;
        packed.transpose().map(|packed| packed.is_some())
    }

    /// Waits until the GPU ran everything submitted.
    pub fn wait_gpu(&self) -> anyhow::Result<()> {
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        Ok(())
    }
}

/// Packs the `width` x `height` RGBA (or BGRA when `bgra`) rows of `source`,
/// `bytes_per_row` apart, as the usual X11 ARGB visual wants them, into `out`: the CPU
/// part of the X11 presentation, for the benchmarks.
pub fn pack_argb32(
    source: &[u8],
    (width, height): (u32, u32),
    bytes_per_row: u32,
    bgra: bool,
    out: &mut Vec<u8>,
) -> anyhow::Result<usize> {
    let format = if bgra {
        wgpu::TextureFormat::Bgra8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8UnormSrgb
    };
    offscreen::PixelLayout::ARGB32.pack(source, width, height, bytes_per_row, format, out)
}

#[cfg(test)]
impl Headless {
    /// Waits for the GPU and reads the target back, as tightly packed RGBA rows.
    fn read_pixels(&mut self) -> Vec<u8> {
        let encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let (width, height) = self.target.size;
        let row = self.target.bytes_per_row as usize;
        self.target.start_read(&self.queue, encoder).unwrap();
        self.target
            .finish_read(&self.device, true, |source| {
                (0..height as usize)
                    .flat_map(|y| &source[y * row..y * row + width as usize * 4])
                    .copied()
                    .collect()
            })
            .unwrap()
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::vte::ansi::Rgb;

    use super::*;

    #[test]
    fn scanlines_cover_the_pane_after_its_over_rects() {
        let item = RectInstance::new([1.0, 2.0], [3.0, 4.0], [1.0; 4]);
        let pane = Rect {
            x: 10.0,
            y: 20.0,
            width: 300.0,
            height: 200.0,
        };
        let mut rects = vec![item];
        let range = push_rects(&mut rects, &[item, item], Some(pane));
        assert_eq!(range, 1..4);
        assert_eq!(rects[1..3], [item, item]);
        assert_eq!(
            rects[3],
            RectInstance::scanlines([10.0, 20.0], [300.0, 200.0])
        );
    }

    #[test]
    fn no_scanlines_rect_is_added_without_the_hidden_theme() {
        let item = RectInstance::new([1.0, 2.0], [3.0, 4.0], [1.0; 4]);
        let mut rects = Vec::new();
        assert_eq!(push_rects(&mut rects, &[item], None), 0..1);
        assert_eq!(rects, [item]);
    }

    #[test]
    fn frame_interval_is_the_monitor_refresh_period() {
        assert_eq!(
            frame_interval(Some(120_000)),
            Duration::from_secs_f64(1.0 / 120.0)
        );
        assert_eq!(frame_interval(None), Duration::from_secs_f64(1.0 / 60.0));
        assert_eq!(frame_interval(Some(0)), Duration::from_secs_f64(1.0 / 60.0));
    }

    #[test]
    fn a_frame_waits_one_interval_after_the_last() {
        let start = Instant::now();
        let interval = Duration::from_millis(8);
        assert_eq!(next_frame(None, interval, start), None);
        assert_eq!(
            next_frame(Some(start), interval, start + Duration::from_millis(3)),
            Some(start + interval)
        );
        assert_eq!(next_frame(Some(start), interval, start + interval), None);
        assert_eq!(
            next_frame(Some(start), interval, start + Duration::from_millis(20)),
            None
        );
    }

    #[test]
    fn only_an_opaque_window_skips_the_x11_presentation() {
        assert!(presents_directly(1.0, true));
        assert!(!presents_directly(0.99, true));
        assert!(!presents_directly(0.0, true));
        assert!(
            !presents_directly(1.0, false),
            "the surface format must fit"
        );
    }

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
        let glyphs = Glyphs::new(&FontOptions::default(), 30.0);
        let expected = glyphs.cell_size();
        let Some(renderer) = Headless::new(SIZE, SIZE, glyphs) else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        assert_eq!(renderer.cell_size(), expected);
        assert_ne!(
            expected,
            Glyphs::new(&FontOptions::default(), 14.0).cell_size()
        );
    }

    #[test]
    fn content_painter_gets_the_size_of_the_drawn_target() {
        let (width, height) = (SIZE, SIZE / 2);
        let Some(mut renderer) =
            Headless::new(width, height, Glyphs::new(&FontOptions::default(), 14.0))
        else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        let prepared = renderer.layers.prepare(
            &renderer.device,
            &renderer.queue,
            renderer.screen,
            &[],
            &UiLayer::default(),
        );
        let mut encoder = renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let mut received = None;
        let mut paint = |_: &mut wgpu::CommandEncoder, _: &wgpu::TextureView, size| {
            received = Some(size);
        };
        renderer.layers.draw(
            &mut encoder,
            &renderer.target.view,
            &prepared,
            Some(&mut paint),
        );
        renderer.queue.submit(Some(encoder.finish()));
        assert_eq!(received, Some((width, height)));
        assert_eq!(received, Some(renderer.target.size));
    }

    #[test]
    fn an_uncaptured_gpu_error_does_not_panic_once_the_handler_is_installed() {
        let Some(renderer) = Headless::new(SIZE, SIZE, Glyphs::new(&FontOptions::default(), 14.0))
        else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        install_error_handler(&renderer.device);
        let mut encoder = renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = begin_pass(
                &mut encoder,
                &renderer.target.view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
            // Twice the width of the target: a validation error.
            pass.set_scissor_rect(0, 0, SIZE * 2, SIZE);
        }
        renderer.queue.submit(Some(encoder.finish()));
        let _ = renderer.device.poll(wgpu::PollType::wait_indefinitely());
    }

    #[test]
    fn x11_presentation_packs_the_drawn_pixels_as_argb() {
        let Some(mut renderer) =
            Headless::new(SIZE, SIZE, Glyphs::new(&FontOptions::default(), 14.0))
        else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        let layout = GridLayout::new(Rect::from_size(SIZE, SIZE), renderer.cell_size(), 0.0);
        let frame = Frame {
            rows: vec![vec![cell('M')]],
            decorations: Vec::new(),
            cards: Vec::new(),
            row_tops: vec![0],
            block_colors: BlockColors {
                success: WHITE,
                failure: WHITE,
                running: WHITE,
                dim: WHITE,
                card_tint: WHITE,
            },
            cursor: None,
            ligature_break: None,
            background: BLACK,
            banner: None,
            lang: crate::i18n::Lang::EnUs,
            images: Vec::new(),
            progress_colors: crate::progress::ProgressColors {
                normal: WHITE,
                error: WHITE,
                paused: WHITE,
                track: WHITE,
            },
            progress_thickness: 0.0,
        };
        let packed = renderer.present_x11(&frame, &layout).unwrap().to_vec();
        let rgba = renderer.read_pixels();
        assert_eq!(packed.len(), rgba.len());
        // Little endian ARGB: blue, green, red, alpha in memory.
        let swapped: Vec<u8> = rgba
            .chunks(4)
            .flat_map(|px| [px[2], px[1], px[0], px[3]])
            .collect();
        assert_eq!(packed, swapped);
        assert!(packed.chunks(4).any(|px| px[2] > 0));
    }

    #[test]
    fn x11_readback_finishes_once_and_one_at_a_time() {
        let Some(mut renderer) =
            Headless::new(SIZE, SIZE, Glyphs::new(&FontOptions::default(), 14.0))
        else {
            eprintln!("no GPU adapter available, skipped");
            return;
        };
        let layout = GridLayout::new(Rect::from_size(SIZE, SIZE), renderer.cell_size(), 0.0);
        let frame = Frame {
            rows: vec![vec![cell('M')]],
            decorations: Vec::new(),
            cards: Vec::new(),
            row_tops: vec![0],
            block_colors: BlockColors {
                success: WHITE,
                failure: WHITE,
                running: WHITE,
                dim: WHITE,
                card_tint: WHITE,
            },
            cursor: None,
            ligature_break: None,
            background: BLACK,
            banner: None,
            lang: crate::i18n::Lang::EnUs,
            images: Vec::new(),
            progress_colors: crate::progress::ProgressColors {
                normal: WHITE,
                error: WHITE,
                paused: WHITE,
                track: WHITE,
            },
            progress_thickness: 0.0,
        };
        assert!(!renderer.finish_x11(false).unwrap(), "nothing to finish");
        renderer.start_x11(&frame, &layout).unwrap();
        assert!(
            renderer.start_x11(&frame, &layout).is_err(),
            "one read at a time"
        );
        renderer.wait_gpu().unwrap();
        assert!(renderer.finish_x11(false).unwrap());
        assert!(!renderer.finish_x11(true).unwrap(), "finished once");
        let synchronous = renderer.present_x11(&frame, &layout).unwrap().to_vec();
        renderer.start_x11(&frame, &layout).unwrap();
        assert!(renderer.finish_x11(true).unwrap());
        assert_eq!(renderer.pixels, synchronous);
    }

    #[test]
    fn grid_glyph_pixels_reach_the_target_inside_their_cell() {
        let Some(mut renderer) =
            Headless::new(SIZE, SIZE, Glyphs::new(&FontOptions::default(), 14.0))
        else {
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
                card_tint: WHITE,
            },
            cursor: None,
            ligature_break: None,
            background: BLACK,
            banner: None,
            lang: crate::i18n::Lang::EnUs,
            images: Vec::new(),
            progress_colors: crate::progress::ProgressColors {
                normal: WHITE,
                error: WHITE,
                paused: WHITE,
                track: WHITE,
            },
            progress_thickness: 0.0,
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
        let Some(mut renderer) =
            Headless::new(SIZE, SIZE, Glyphs::new(&FontOptions::default(), 14.0))
        else {
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
                fold_progress: 1.0,
                selected: false,
                scroll: None,
                progress: None,
            }],
            row_tops: vec![0],
            block_colors: BlockColors {
                success: WHITE,
                failure: WHITE,
                running: WHITE,
                dim: WHITE,
                card_tint: WHITE,
            },
            cursor: None,
            ligature_break: None,
            background: BLACK,
            banner: None,
            lang: crate::i18n::Lang::EnUs,
            images: Vec::new(),
            progress_colors: crate::progress::ProgressColors {
                normal: WHITE,
                error: WHITE,
                paused: WHITE,
                track: WHITE,
            },
            progress_thickness: 0.0,
        };
        renderer.render(&frame, &layout).unwrap();
        let pixels = renderer.read_pixels();
        let red = |x: f32, y: f32| pixels[((y as u32 * SIZE + x as u32) * 4) as usize];
        let left = layout.grid_left() - layout.card_pad;
        // The top-left corner pixel is outside the rounded corner; the card middle is
        // covered by the translucent tint: lighter than the background, not opaque.
        assert_eq!(red(left, 0.0), 0);
        let middle = red(left + 6.0, cell_size.height / 2.0);
        assert!((1..255).contains(&middle), "card middle: {middle}");
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
                card_tint: WHITE,
            },
            cursor: None,
            ligature_break: None,
            background: BLACK,
            banner: None,
            lang: crate::i18n::Lang::EnUs,
            images: Vec::new(),
            progress_colors: crate::progress::ProgressColors {
                normal: WHITE,
                error: WHITE,
                paused: WHITE,
                track: WHITE,
            },
            progress_thickness: 0.0,
        }
    }

    #[test]
    fn image_pixels_reach_the_target_centered_in_their_box() {
        use crate::frame::FrameImage;
        use crate::graphics::{ImageCell, Pixels, SourceRect};

        let Some(mut renderer) =
            Headless::new(SIZE, SIZE, Glyphs::new(&FontOptions::default(), 14.0))
        else {
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
        let Some(mut renderer) =
            Headless::new(SIZE, SIZE, Glyphs::new(&FontOptions::default(), 14.0))
        else {
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
                        font_size: renderer.layers.glyphs.font_size(),
                    },
                    PaneFrame {
                        frame: &right_frame,
                        layout: &right,
                        font_size: renderer.layers.glyphs.font_size(),
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
        let Some(mut renderer) =
            Headless::new(SIZE, SIZE, Glyphs::new(&FontOptions::default(), 14.0))
        else {
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
                    font_size: renderer.layers.glyphs.font_size(),
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
        let Some(mut renderer) =
            Headless::new(SIZE, SIZE, Glyphs::new(&FontOptions::default(), 14.0))
        else {
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
                    font_size: renderer.layers.glyphs.font_size(),
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
        let transparent_backend = BACKEND_ATTEMPTS[1];
        let result = pollster::block_on(first_transparent(&BACKEND_ATTEMPTS, false, |b| {
            tried.push(b);
            async move { Ok((b, b == transparent_backend)) }
        }));
        assert_eq!(result.unwrap(), (transparent_backend, true));
        assert_eq!(tried, BACKEND_ATTEMPTS);
    }

    #[test]
    fn first_transparent_uses_opaque_backend_when_none_support_transparency() {
        let mut tried = Vec::new();
        let result = pollster::block_on(first_transparent(&BACKEND_ATTEMPTS, false, |b| {
            tried.push(b);
            async move { Ok((b, false)) }
        }));
        assert_eq!(result.unwrap(), (wgpu::Backends::VULKAN, false));
        assert_eq!(tried, BACKEND_ATTEMPTS);
    }

    #[test]
    fn first_transparent_keeps_the_first_opaque_backend_when_opaque_is_enough() {
        let mut tried = Vec::new();
        let result = pollster::block_on(first_transparent(&BACKEND_ATTEMPTS, true, |b| {
            tried.push(b);
            async move { Ok((b, false)) }
        }));
        assert_eq!(result.unwrap(), (wgpu::Backends::VULKAN, false));
        assert_eq!(tried, [wgpu::Backends::VULKAN]);
    }

    #[test]
    fn first_transparent_still_falls_back_when_opaque_is_enough_but_vulkan_fails() {
        let fallback_backend = BACKEND_ATTEMPTS[1];
        let result =
            pollster::block_on(first_transparent(&BACKEND_ATTEMPTS, true, |b| async move {
                if b == wgpu::Backends::VULKAN {
                    anyhow::bail!("no Vulkan adapter")
                }
                Ok((b, false))
            }));
        assert_eq!(result.unwrap(), (fallback_backend, false));
    }

    #[test]
    fn first_transparent_falls_back_when_vulkan_fails() {
        let fallback_backend = BACKEND_ATTEMPTS[1];
        let result = pollster::block_on(first_transparent(
            &BACKEND_ATTEMPTS,
            false,
            |b| async move {
                if b == wgpu::Backends::VULKAN {
                    anyhow::bail!("no Vulkan adapter")
                }
                Ok((b, true))
            },
        ));
        assert_eq!(result.unwrap(), (fallback_backend, true));
    }

    #[test]
    fn first_transparent_returns_the_last_error_when_all_fail() {
        let result: anyhow::Result<((), bool)> = pollster::block_on(first_transparent(
            &BACKEND_ATTEMPTS,
            false,
            |b| async move { anyhow::bail!("failed with {b:?}") },
        ));
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains(&format!("{:?}", BACKEND_ATTEMPTS[1])),
            "{message}"
        );
    }
}
