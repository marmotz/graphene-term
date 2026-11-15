//! The settings window: a second winit window on the shared GPU context, drawn with
//! egui and redrawn on demand.

mod view;
mod visuals;

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use egui::ViewportId;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::i18n::{self, Lang, Msg};
use crate::palette::Palette;
use crate::renderer::Gpu;

use view::View;

const INITIAL_SIZE: LogicalSize<u32> = LogicalSize::new(760, 520);
const MIN_SIZE: LogicalSize<u32> = LogicalSize::new(520, 360);

/// What the owner of a settings window does after one of its events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Keep,
    /// The user closed the window: drop it.
    Close,
}

pub struct SettingsWindow {
    window: Arc<Window>,
    gpu: Arc<Gpu>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    egui: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    view: View,
    lang: Lang,
    /// When egui asked for the next frame, for a delayed repaint (tooltips, animations).
    repaint_at: Option<Instant>,
}

impl SettingsWindow {
    /// Opens the window on `gpu`, colored by `palette` and labelled in `lang`.
    pub fn open(
        event_loop: &ActiveEventLoop,
        gpu: Arc<Gpu>,
        palette: &Palette,
        lang: Lang,
    ) -> anyhow::Result<Self> {
        let attributes = Window::default_attributes()
            .with_title(i18n::text(lang, &Msg::OpenSettings))
            .with_inner_size(INITIAL_SIZE)
            .with_min_inner_size(MIN_SIZE);
        let window = Arc::new(event_loop.create_window(attributes)?);

        let surface = gpu.create_surface(window.clone())?;
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = egui_wgpu::preferred_framebuffer_format(&caps.formats)
            .context("settings surface has no supported format")?;
        let alpha_mode = caps
            .alpha_modes
            .iter()
            .copied()
            .find(|&mode| mode == wgpu::CompositeAlphaMode::Opaque)
            .or_else(|| caps.alpha_modes.first().copied())
            .context("settings surface has no alpha mode")?;
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&gpu.device, &config);

        let renderer =
            egui_wgpu::Renderer::new(&gpu.device, format, egui_wgpu::RendererOptions::default());
        let max_texture_side = gpu.device.limits().max_texture_dimension_2d as usize;
        let egui = egui_winit::State::new(
            egui::Context::default(),
            ViewportId::ROOT,
            event_loop,
            Some(window.scale_factor() as f32),
            None,
            Some(max_texture_side),
        );

        let mut settings = Self {
            window,
            gpu,
            surface,
            config,
            egui,
            renderer,
            view: View::default(),
            lang,
            repaint_at: None,
        };
        settings.apply(palette, lang);
        Ok(settings)
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// Brings the window to the front.
    pub fn focus(&self) {
        self.window.focus_window();
    }

    /// Recolors the window with `palette` and relabels it in `lang`, after a reload.
    pub fn apply(&mut self, palette: &Palette, lang: Lang) {
        let ctx = self.egui.egui_ctx();
        ctx.set_theme(visuals::theme(palette.background));
        ctx.set_visuals(visuals::visuals(palette));
        if lang != self.lang {
            self.lang = lang;
            self.window.set_title(&i18n::text(lang, &Msg::OpenSettings));
        }
        self.window.request_redraw();
    }

    /// Next time the window wants a frame, when egui asked for a delayed one.
    pub fn deadline(&self) -> Option<Instant> {
        self.repaint_at
    }

    /// Requests the delayed frame once its time has come.
    pub fn wake(&mut self, now: Instant) {
        if self.repaint_at.is_some_and(|at| at <= now) {
            self.repaint_at = None;
            self.window.request_redraw();
        }
    }

    pub fn window_event(&mut self, event_loop: &ActiveEventLoop, event: &WindowEvent) -> Outcome {
        match event {
            WindowEvent::CloseRequested => return Outcome::Close,
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.redraw(event_loop) {
                    log::error!("settings window render failed: {error:#}");
                }
                return Outcome::Keep;
            }
            WindowEvent::Resized(size) => self.resize(size.width, size.height),
            _ => {}
        }
        if self.egui.on_window_event(&self.window, event).repaint {
            self.window.request_redraw();
        }
        Outcome::Keep
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.gpu.device, &self.config);
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        let input = self.egui.take_egui_input(&self.window);
        let ctx = self.egui.egui_ctx().clone();
        let view = &mut self.view;
        let lang = self.lang;
        let output = ctx.run_ui(input, |ui| view.show(ui, lang));
        self.egui.handle_platform_output_with_event_loop(
            &self.window,
            event_loop,
            output.platform_output,
        );
        let delay = output
            .viewport_output
            .get(&ViewportId::ROOT)
            .map_or(Duration::MAX, |viewport| viewport.repaint_delay);
        self.repaint_at = if delay.is_zero() {
            self.window.request_redraw();
            None
        } else {
            Instant::now().checked_add(delay)
        };

        let device = &self.gpu.device;
        let queue = &self.gpu.queue;
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                self.renderer.update_texture(device, queue, *id, delta);
            }
        }
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: output.pixels_per_point,
        };

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Suboptimal(_) => {
                self.surface.configure(device, &self.config);
                self.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface = self.gpu.create_surface(self.window.clone())?;
                self.surface.configure(device, &self.config);
                self.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("settings surface validation error")
            }
        };
        let target = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("settings"),
        });
        let callbacks =
            self.renderer
                .update_buffers(device, queue, &mut encoder, &primitives, &screen);
        let [r, g, b, a] = ctx
            .global_style()
            .visuals
            .panel_fill
            .to_normalized_gamma_f32();
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("settings pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(r),
                            g: f64::from(g),
                            b: f64::from(b),
                            a: f64::from(a),
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer
                .render(&mut pass.forget_lifetime(), &primitives, &screen);
        }
        queue.submit(callbacks.into_iter().chain([encoder.finish()]));
        // Freed after the submit: the frame may still use them.
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
        self.window.pre_present_notify();
        queue.present(frame);
        Ok(())
    }
}
