//! The settings window: a second winit window on the shared GPU context, drawn with
//! egui and redrawn on demand. Each change is written to `config.toml` at once; the
//! hot reload applies it to the terminal window and refreshes this one.

mod fonts;
mod model;
mod view;
mod visuals;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use egui::ViewportId;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use crate::appearance::Appearance;
use crate::config::{self, Config, ConfigError};
use crate::config_edit::{self, ConfigEdit};
use crate::i18n::{self, Lang, Msg};
use crate::palette::Palette;
use crate::renderer::Gpu;
use crate::{opener, theme, theme_edit};

pub use model::LoadErrors;
use model::{Change, SettingsModel, ThemeOp};
use view::{Edits, View};

const INITIAL_SIZE: LogicalSize<u32> = LogicalSize::new(1000, 700);
const MIN_SIZE: LogicalSize<u32> = LogicalSize::new(640, 420);

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
    model: SettingsModel,
    /// The config directory, holding the `config.toml` and `keybindings.toml` edited.
    dir: PathBuf,
    lang: Lang,
    /// When egui asked for the next frame, for a delayed repaint (tooltips, animations).
    repaint_at: Option<Instant>,
    /// Modifiers held, for a captured shortcut.
    modifiers: ModifiersState,
    /// The colors shown in the terminal window while a color of the active theme is
    /// picked.
    preview: LivePreview,
}

impl SettingsWindow {
    /// Opens the window on `gpu`, colored by `palette` and labelled in `lang`, showing
    /// `config` and the `errors` of its last load under the system `appearance`; changes
    /// are written to the files of `dir`.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        event_loop: &ActiveEventLoop,
        gpu: Arc<Gpu>,
        palette: &Palette,
        lang: Lang,
        config: &Config,
        errors: &LoadErrors,
        appearance: Appearance,
        dir: PathBuf,
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
        let surface_config = wgpu::SurfaceConfiguration {
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
        surface.configure(&gpu.device, &surface_config);

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

        // The database cosmic-text builds, so the sans-serif family is the palette's.
        let system = cosmic_text::FontSystem::new();
        let fonts = system.db();
        let ctx = egui.egui_ctx();
        if let Some(font) = fonts::sans_serif(fonts) {
            fonts::install(ctx, font);
        }
        ctx.all_styles_mut(|style| style.text_styles = fonts::text_styles());
        let mut model = SettingsModel::new(config, errors, Some(dir.clone()), appearance);
        model.set_font_families(model::monospace_families(fonts));

        let mut settings = Self {
            window,
            gpu,
            surface,
            config: surface_config,
            egui,
            renderer,
            view: View::default(),
            model,
            dir,
            lang,
            repaint_at: None,
            modifiers: ModifiersState::empty(),
            preview: LivePreview::default(),
        };
        settings.apply(palette, lang, config, errors, appearance);
        Ok(settings)
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// Brings the window to the front.
    pub fn focus(&self) {
        self.window.focus_window();
    }

    /// Recolors the window with `palette`, relabels it in `lang` and shows `config`
    /// and its `errors` under the system `appearance`, after a reload.
    pub fn apply(
        &mut self,
        palette: &Palette,
        lang: Lang,
        config: &Config,
        errors: &LoadErrors,
        appearance: Appearance,
    ) {
        self.model.refresh(config, errors, appearance);
        self.view.reset_inputs();
        self.model
            .set_theme_choices(theme::available(Some(&self.dir)));
        let ctx = self.egui.egui_ctx();
        ctx.set_theme(visuals::theme(palette.background));
        ctx.set_visuals(visuals::visuals(palette));
        if lang != self.lang {
            self.lang = lang;
            self.window.set_title(&i18n::text(lang, &Msg::OpenSettings));
        }
        self.window.request_redraw();
    }

    /// The colors the terminal window shows after the last event of this window, when
    /// they changed: the active theme with the color being picked, then the active
    /// theme as loaded once the picker closes. Read after each event, the window closing
    /// included.
    pub fn take_preview(&mut self) -> Option<Palette> {
        let wanted = self.model.live_preview(self.view.picked());
        self.preview.update(wanted, || self.model.active_palette())
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
            WindowEvent::CloseRequested => return self.close(),
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.redraw(event_loop) {
                    log::error!("settings window render failed: {error:#}");
                }
                return Outcome::Keep;
            }
            WindowEvent::Resized(size) => self.resize(size.width, size.height),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            // A captured shortcut is read from the winit event, to match it as the
            // terminal window does; egui does not see the key press.
            WindowEvent::KeyboardInput { event: key, .. }
                if self.view.capturing() && key.state == ElementState::Pressed =>
            {
                let changes = self.view.captured_key(
                    &self.model,
                    &key.logical_key,
                    key.physical_key,
                    self.modifiers,
                );
                if !changes.is_empty() {
                    self.write(&Edits {
                        changes,
                        ..Edits::default()
                    });
                }
                self.window.request_redraw();
                return Outcome::Keep;
            }
            // Escape alone closes the window, unless egui uses it (an open popup, a
            // focused field) or it dismisses a shortcut conflict.
            WindowEvent::KeyboardInput { event: key, .. }
                if closes_on_escape(&key.logical_key, key.state, self.modifiers)
                    && !self.egui_takes_escape() =>
            {
                if self.view.escape() {
                    self.window.request_redraw();
                    return Outcome::Keep;
                }
                return self.close();
            }
            _ => {}
        }
        if self.egui.on_window_event(&self.window, event).repaint {
            self.window.request_redraw();
        }
        Outcome::Keep
    }

    /// Writes the color of a picker still open, then lets the window close.
    fn close(&mut self) -> Outcome {
        if let Some(op) = self.view.finish_picking() {
            let mut edits = Edits::default();
            if let Ok(theme_op) = self.model.theme_op(op) {
                edits.theme_ops.push(theme_op);
                self.write(&edits);
            }
        }
        Outcome::Close
    }

    /// Whether egui handles `Escape` itself: it closes a popup or drops a field's focus.
    fn egui_takes_escape(&self) -> bool {
        let ctx = self.egui.egui_ctx();
        egui::Popup::is_any_open(ctx) || ctx.memory(|memory| memory.focused().is_some())
    }

    /// Writes the `edits` of a frame to the files, and shows the result before the
    /// reload reports it.
    fn write(&mut self, edits: &Edits) {
        if !edits.changes.is_empty() || !edits.theme_ops.is_empty() {
            let edited = self.model.edited_theme().to_owned();
            let (next, result) = write_edits(&self.dir, &edited, &edits.changes, &edits.theme_ops);
            let (config, errors) = config::reload_from(&self.dir, self.model.config());
            self.model.written(config, errors);
            self.model
                .set_theme_choices(theme::available(Some(&self.dir)));
            if let Some(name) = next {
                self.model.edit_theme(&name);
            }
            if let Err(error) = result {
                log::warn!("{error}");
                self.model.write_failed(&error);
            }
        }
        if let Some(name) = &edits.edit_theme {
            self.model.edit_theme(name);
        }
        if edits.open_themes_dir {
            match themes_dir(&self.dir) {
                Ok(dir) => opener::open(&dir.to_string_lossy()),
                Err(error) => log::warn!("cannot create {}: {error}", theme::THEMES_DIR),
            }
        }
        self.window.request_redraw();
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
        let model = &self.model;
        let lang = self.lang;
        let mut edits = Edits::default();
        let mut output = ctx.run_ui(input, |ui| edits = view.show(ui, lang, model));
        if !edits.is_empty() {
            self.write(&edits);
        }
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

        // Drained, not borrowed: a dropped `TexturesDelta` still holding deltas is a
        // debug assertion failure.
        let mut textures = std::mem::take(&mut output.textures_delta);
        for (id, deltas) in textures.set.drain() {
            for delta in deltas {
                self.renderer
                    .update_texture(&self.gpu.device, &self.gpu.queue, id, &delta);
            }
        }
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        let painted = self.paint(&ctx, &primitives, output.pixels_per_point);
        // Freed after the submit, or with no frame at all: the frame may still use them.
        for id in textures.free.drain() {
            self.renderer.free_texture(&id);
        }
        painted
    }

    /// Paints `primitives` on the next surface texture and presents it.
    fn paint(
        &mut self,
        ctx: &egui::Context,
        primitives: &[egui::ClippedPrimitive],
        pixels_per_point: f32,
    ) -> anyhow::Result<()> {
        let device = &self.gpu.device;
        let queue = &self.gpu.queue;
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point,
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
                .update_buffers(device, queue, &mut encoder, primitives, &screen);
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
                .render(&mut pass.forget_lifetime(), primitives, &screen);
        }
        queue.submit(callbacks.into_iter().chain([encoder.finish()]));
        self.window.pre_present_notify();
        queue.present(frame);
        Ok(())
    }
}

/// The colors last sent to the terminal window as a preview.
#[derive(Debug, Default)]
struct LivePreview {
    shown: Option<Palette>,
}

impl LivePreview {
    /// Takes the `wanted` preview of this event; returns the colors to show when they
    /// changed. When the preview ends, the terminal gets the `active` theme's colors
    /// once: the written color, or the file's when the write failed.
    fn update(
        &mut self,
        wanted: Option<Palette>,
        active: impl FnOnce() -> Palette,
    ) -> Option<Palette> {
        match wanted {
            Some(palette) if self.shown.as_ref() == Some(&palette) => None,
            Some(palette) => {
                self.shown = Some(palette.clone());
                Some(palette)
            }
            None => self.shown.take().map(|_| active()),
        }
    }
}

/// Whether a key event is a press of `Escape` with no modifier held.
fn closes_on_escape(key: &Key, state: ElementState, mods: ModifiersState) -> bool {
    state == ElementState::Pressed && mods.is_empty() && *key == Key::Named(NamedKey::Escape)
}

/// Applies `changes` to the files of `dir`: the shortcuts to `keybindings.toml`, the rest
/// to `config.toml`, one write per file touched.
fn write_changes(dir: &Path, changes: &[Change]) -> Result<(), ConfigError> {
    let (shortcuts, others): (Vec<&Change>, Vec<&Change>) =
        changes.iter().partition(|change| is_shortcut(change));
    if !others.is_empty() {
        write_file(
            &dir.join(config::CONFIG_FILE),
            config_edit::default_file_text,
            &others,
        )?;
    }
    if !shortcuts.is_empty() {
        write_file(
            &dir.join(config::KEYBINDINGS_FILE),
            config_edit::default_keybindings_text,
            &shortcuts,
        )?;
    }
    Ok(())
}

/// Writes the configuration `changes`, then each theme operation on the theme `edited`
/// followed by its configuration changes: a theme file that cannot be written leaves the
/// configuration alone. Stops at the first error. Returns the theme a copy or a rename
/// made, to show it in the editor, with the result.
/// The user themes folder of `config_dir`, created when missing so it can be opened.
fn themes_dir(config_dir: &Path) -> std::io::Result<PathBuf> {
    let dir = config_dir.join(theme::THEMES_DIR);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn write_edits(
    dir: &Path,
    edited: &str,
    changes: &[Change],
    theme_ops: &[(Vec<Change>, ThemeOp)],
) -> (Option<String>, Result<(), ConfigError>) {
    let mut next = None;
    let result = (|| {
        if !changes.is_empty() {
            write_changes(dir, changes)?;
        }
        let mut edited = edited.to_owned();
        for (changes, op) in theme_ops {
            write_theme_op(dir, &edited, op)?;
            if let ThemeOp::Duplicate(name) | ThemeOp::Rename(name) = op {
                edited.clone_from(name);
                next = Some(name.clone());
            }
            if !changes.is_empty() {
                write_changes(dir, changes)?;
            }
        }
        Ok(())
    })();
    (next, result)
}

/// Applies `op` to the user theme `name` of `dir`.
fn write_theme_op(dir: &Path, name: &str, op: &ThemeOp) -> Result<(), ConfigError> {
    match op {
        ThemeOp::SetColor(key, color) => theme_edit::set_color(dir, name, *key, *color),
        ThemeOp::Duplicate(to) => theme_edit::duplicate(dir, name, to),
        ThemeOp::Rename(to) => theme_edit::rename(dir, name, to),
        ThemeOp::Delete => theme_edit::delete(dir, name),
    }
}

/// Whether `change` edits a shortcut, which lives in `keybindings.toml`.
fn is_shortcut(change: &Change) -> bool {
    let (Change::Set(path, _) | Change::Remove(path)) = change;
    path.first().is_some_and(|key| key == "keybindings")
}

/// Applies `changes` to `file` in one write; a missing file starts from `default_text`.
fn write_file(
    file: &Path,
    default_text: fn() -> String,
    changes: &[&Change],
) -> Result<(), ConfigError> {
    config_edit::write(file, default_text, |edit: &mut ConfigEdit| {
        for change in changes {
            match change {
                Change::Set(path, value) => {
                    let path: Vec<&str> = path.iter().map(String::as_str).collect();
                    edit.set(&path, value.clone())?;
                }
                Change::Remove(path) => {
                    let path: Vec<&str> = path.iter().map(String::as_str).collect();
                    edit.remove(&path);
                }
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use model::Value;

    #[test]
    fn the_themes_folder_is_created_before_it_is_opened() {
        let config_dir = std::env::temp_dir().join(format!(
            "graphene-settings-themes-dir-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&config_dir);
        std::fs::create_dir_all(&config_dir).unwrap();

        let dir = themes_dir(&config_dir).unwrap();
        assert_eq!(dir, config_dir.join(theme::THEMES_DIR));
        assert!(dir.is_dir());
        // An existing folder is kept.
        assert_eq!(themes_dir(&config_dir).unwrap(), dir);
        let _ = std::fs::remove_dir_all(&config_dir);
    }

    #[test]
    fn changes_are_written_in_one_write_keeping_the_file() {
        let dir =
            std::env::temp_dir().join(format!("graphene-settings-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(config::CONFIG_FILE);
        std::fs::write(
            &file,
            "# mine\n[theme]\nname = \"gruvbox-dark\"\nlight = \"x\"\n",
        )
        .unwrap();
        let path = |segments: &[&str]| segments.iter().map(|s| (*s).to_owned()).collect();
        write_changes(
            &dir,
            &[
                Change::Set(path(&["font", "size"]), Value::Float(16.0)),
                Change::Remove(path(&["theme", "light"])),
            ],
        )
        .unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("# mine\n"), "{text}");
        assert!(!text.contains("light"), "{text}");
        let (config, errors) = Config::parse(&text, &Config::default(), &file);
        assert_eq!(errors, vec![]);
        assert_eq!(config.font.size, 16.0);
        assert_eq!(config.theme.name, "gruvbox-dark");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn escape_alone_pressed_closes() {
        let escape = Key::Named(NamedKey::Escape);
        let none = ModifiersState::empty();
        assert!(closes_on_escape(&escape, ElementState::Pressed, none));
        assert!(!closes_on_escape(&escape, ElementState::Released, none));
        assert!(!closes_on_escape(
            &escape,
            ElementState::Pressed,
            ModifiersState::SHIFT
        ));
        assert!(!closes_on_escape(
            &Key::Named(NamedKey::Enter),
            ElementState::Pressed,
            none
        ));
    }

    #[test]
    fn a_change_that_fails_is_an_error() {
        let dir =
            std::env::temp_dir().join(format!("graphene-settings-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(config::CONFIG_FILE);
        std::fs::write(&file, "font = 3\n").unwrap();
        let error = write_changes(
            &dir,
            &[Change::Set(
                vec!["font".into(), "size".into()],
                Value::Float(16.0),
            )],
        )
        .unwrap_err();
        assert_eq!(error.key.as_deref(), Some("font"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "font = 3\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn temporary_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("graphene-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn path(segments: &[&str]) -> Vec<String> {
        segments.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn shortcuts_go_to_keybindings_toml_and_the_rest_to_config_toml() {
        let dir = temporary_dir("split");
        std::fs::write(
            dir.join(config::CONFIG_FILE),
            "# mine\n[font]\nsize = 12.0\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(config::KEYBINDINGS_FILE),
            "# keys\n[keybindings]\n\"Ctrl+Alt+Y\" = \"new_tab\"\n",
        )
        .unwrap();
        write_changes(
            &dir,
            &[
                Change::Set(path(&["font", "size"]), Value::Float(16.0)),
                Change::Set(
                    path(&["keybindings", "Ctrl+Alt+T"]),
                    Value::Text("new_tab".into()),
                ),
                Change::Remove(path(&["keybindings", "Ctrl+Alt+Y"])),
            ],
        )
        .unwrap();

        let config_text = std::fs::read_to_string(dir.join(config::CONFIG_FILE)).unwrap();
        assert!(config_text.starts_with("# mine\n"), "{config_text}");
        assert!(!config_text.contains("keybindings"), "{config_text}");
        assert!(config_text.contains("size = 16.0"), "{config_text}");
        let keys_text = std::fs::read_to_string(dir.join(config::KEYBINDINGS_FILE)).unwrap();
        assert!(keys_text.starts_with("# keys\n"), "{keys_text}");
        assert!(
            keys_text.contains("\"Ctrl+Alt+T\" = \"new_tab\""),
            "{keys_text}"
        );
        assert!(!keys_text.contains("Ctrl+Alt+Y"), "{keys_text}");
        assert!(!keys_text.contains("size"), "{keys_text}");

        let (config, errors) = config::reload_from(&dir, &Config::default());
        assert_eq!(errors, vec![]);
        assert_eq!(config.font.size, 16.0);
        assert_eq!(
            config.keybindings.get("Ctrl+Alt+T").map(String::as_str),
            Some("new_tab")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_shortcut_change_leaves_config_toml_alone() {
        let dir = temporary_dir("only-shortcuts");
        std::fs::write(dir.join(config::CONFIG_FILE), "font = 3\n").unwrap();
        write_changes(
            &dir,
            &[Change::Set(
                path(&["keybindings", "Ctrl+Alt+T"]),
                Value::Text("new_tab".into()),
            )],
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(config::CONFIG_FILE)).unwrap(),
            "font = 3\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_shortcuts_file_starts_from_its_template() {
        let dir = temporary_dir("missing-keybindings");
        write_changes(
            &dir,
            &[Change::Set(
                path(&["keybindings", "Ctrl+Alt+Y"]),
                Value::Text("new_tab".into()),
            )],
        )
        .unwrap();
        let text = std::fs::read_to_string(dir.join(config::KEYBINDINGS_FILE)).unwrap();
        let template = config_edit::default_keybindings_text();
        let header = template.split("\n[keybindings]\n").next().unwrap();
        assert!(text.starts_with(header), "{text}");
        assert!(text.contains("\n# \"Ctrl+Shift+C\" = \"copy\"\n"), "{text}");
        assert!(text.contains("\"Ctrl+Alt+Y\" = \"new_tab\""), "{text}");
        assert!(!dir.join(config::CONFIG_FILE).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_keybindings_table_stays_after_the_last_shortcut_is_removed() {
        let dir = temporary_dir("last-shortcut");
        write_changes(
            &dir,
            &[Change::Set(
                path(&["keybindings", "Ctrl+Alt+Y"]),
                Value::Text("new_tab".into()),
            )],
        )
        .unwrap();
        write_changes(
            &dir,
            &[Change::Remove(path(&["keybindings", "Ctrl+Alt+Y"]))],
        )
        .unwrap();
        let text = std::fs::read_to_string(dir.join(config::KEYBINDINGS_FILE)).unwrap();
        assert!(text.contains("\n[keybindings]\n"), "{text}");
        assert!(!text.contains("Ctrl+Alt+Y\" ="), "{text}");
        let (_, errors) = config::reload_from(&dir, &Config::default());
        assert!(
            errors
                .iter()
                .all(|e| e.kind != crate::config::ConfigErrorKind::Syntax),
            "{errors:?}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn theme_name(name: &str) -> Change {
        Change::Set(path(&["theme", "name"]), Value::Text(name.into()))
    }

    #[test]
    fn a_copy_is_written_before_the_configuration_points_to_it() {
        let dir = temporary_dir("theme-duplicate");
        std::fs::write(dir.join(config::CONFIG_FILE), "# mine\n").unwrap();
        let (next, result) = write_edits(
            &dir,
            theme::DEFAULT_THEME,
            &[],
            &[(vec![theme_name("copy")], ThemeOp::Duplicate("copy".into()))],
        );
        result.unwrap();
        assert_eq!(next.as_deref(), Some("copy"));
        assert!(dir.join("themes/copy.toml").is_file());
        let (config, errors) = config::reload_from(&dir, &Config::default());
        assert_eq!(errors, vec![]);
        assert_eq!(config.theme.name, "copy");
        assert_eq!(
            theme::load("copy", Some(&dir)).0,
            theme::load(theme::DEFAULT_THEME, None).0
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn theme_operations_follow_a_rename() {
        let dir = temporary_dir("theme-rename");
        std::fs::create_dir_all(dir.join("themes")).unwrap();
        std::fs::write(dir.join("themes/mine.toml"), "# kept\n[colors]\n").unwrap();
        let green = alacritty_terminal::vte::ansi::Rgb { r: 0, g: 255, b: 0 };
        let (next, result) = write_edits(
            &dir,
            "mine",
            &[],
            &[
                (vec![theme_name("ours")], ThemeOp::Rename("ours".into())),
                (
                    vec![],
                    ThemeOp::SetColor(theme::ColorKey::Cursor, Some(green)),
                ),
            ],
        );
        result.unwrap();
        assert_eq!(next.as_deref(), Some("ours"));
        assert!(!dir.join("themes/mine.toml").exists());
        let text = std::fs::read_to_string(dir.join("themes/ours.toml")).unwrap();
        assert!(text.starts_with("# kept\n"), "{text}");
        assert!(text.contains("cursor = \"#00ff00\""), "{text}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_theme_file_that_cannot_be_written_leaves_the_configuration_alone() {
        let dir = temporary_dir("theme-fail");
        std::fs::write(dir.join(config::CONFIG_FILE), "# mine\n").unwrap();
        // A built-in theme has no file to delete.
        let (next, result) = write_edits(
            &dir,
            theme::DEFAULT_THEME,
            &[],
            &[(vec![theme_name("gruvbox-dark")], ThemeOp::Delete)],
        );
        assert!(result.is_err());
        assert_eq!(next, None);
        assert_eq!(
            std::fs::read_to_string(dir.join(config::CONFIG_FILE)).unwrap(),
            "# mine\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_invalid_shortcuts_file_is_left_untouched() {
        let dir = temporary_dir("invalid-keybindings");
        std::fs::write(dir.join(config::KEYBINDINGS_FILE), "[keybindings\n").unwrap();
        let error = write_changes(
            &dir,
            &[Change::Set(
                path(&["keybindings", "Ctrl+Alt+T"]),
                Value::Text("new_tab".into()),
            )],
        )
        .unwrap_err();
        assert_eq!(error.kind, crate::config::ConfigErrorKind::Syntax);
        assert_eq!(
            std::fs::read_to_string(dir.join(config::KEYBINDINGS_FILE)).unwrap(),
            "[keybindings\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn palette_with_cursor(cursor: alacritty_terminal::vte::ansi::Rgb) -> Palette {
        let mut palette = theme::default_palette().clone();
        palette.cursor = cursor;
        palette
    }

    #[test]
    fn the_preview_is_sent_while_it_changes_then_ends_on_the_active_theme() {
        use alacritty_terminal::vte::ansi::Rgb;
        let green = palette_with_cursor(Rgb { r: 0, g: 255, b: 0 });
        let blue = palette_with_cursor(Rgb { r: 0, g: 0, b: 255 });
        let active = || palette_with_cursor(Rgb { r: 1, g: 2, b: 3 });
        let mut preview = LivePreview::default();
        assert_eq!(preview.update(None, active), None);
        assert_eq!(
            preview.update(Some(green.clone()), active),
            Some(green.clone())
        );
        // An event that does not change the color sends nothing.
        assert_eq!(preview.update(Some(green.clone()), active), None);
        assert_eq!(preview.update(Some(blue.clone()), active), Some(blue));
        // The picker closed: the active theme as loaded, once.
        assert_eq!(preview.update(None, active), Some(active()));
        assert_eq!(preview.update(None, active), None);
    }
}
