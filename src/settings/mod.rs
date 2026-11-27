//! The settings tab: the first tab of the terminal window, drawn with egui over the
//! window's content area, in the terminal's frame, and redrawn on demand. Each change is
//! written to `config.toml` at once; the hot reload applies it to the terminal and
//! refreshes the tab.

mod fonts;
mod model;
mod view;
mod visuals;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::ViewportId;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::Window;

use crate::appearance::Appearance;
use crate::config::{self, Config, ConfigError};
use crate::config_edit::{self, ConfigEdit};
use crate::i18n::Lang;
use crate::layout::Rect;
use crate::palette::Palette;
use crate::renderer::Gpu;
use crate::{opener, theme, theme_edit};

pub use model::LoadErrors;
use model::{Change, SettingsModel, ThemeOp};
use view::{Edits, View};

/// What the window does after an event of the settings tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Keep,
    /// The user left the tab with `Escape`: the previous terminal tab is shown again.
    Leave,
}

pub struct SettingsTab {
    /// The terminal window the tab is drawn in.
    window: Arc<Window>,
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
    /// The colors shown in the terminal while a color of the active theme is picked.
    preview: LivePreview,
    /// The frame [`Self::prepare`] ran last, for [`Self::paint`].
    frame: Option<Prepared>,
}

/// An egui frame ready to be painted.
struct Prepared {
    primitives: Vec<egui::ClippedPrimitive>,
    pixels_per_point: f32,
    /// Textures freed once the frame is painted: it may still use them.
    free: Vec<egui::TextureId>,
}

impl SettingsTab {
    /// Opens the tab in the terminal `window`, drawn on `gpu` into textures of `format`,
    /// colored by `palette` and labelled in `lang`, showing `config` and the `errors` of
    /// its last load under the system `appearance`; changes are written to the files of
    /// `dir`.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        event_loop: &ActiveEventLoop,
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        window: Arc<Window>,
        palette: &Palette,
        lang: Lang,
        config: &Config,
        errors: &LoadErrors,
        appearance: Appearance,
        dir: PathBuf,
    ) -> Self {
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
        model.set_wayland(is_wayland(event_loop));
        model.set_screens(
            event_loop
                .available_monitors()
                .filter_map(|monitor| monitor.name())
                .collect(),
        );

        let mut settings = Self {
            window,
            egui,
            renderer,
            view: View::default(),
            model,
            dir,
            lang,
            repaint_at: None,
            modifiers: ModifiersState::empty(),
            preview: LivePreview::default(),
            frame: None,
        };
        settings.apply(palette, lang, config, errors, appearance);
        settings
    }

    /// Recolors the tab with `palette`, relabels it in `lang` and shows `config` and its
    /// `errors` under the system `appearance`, after a reload.
    pub fn apply(
        &mut self,
        palette: &Palette,
        lang: Lang,
        config: &Config,
        errors: &LoadErrors,
        appearance: Appearance,
    ) {
        self.model.refresh(config, errors, appearance);
        self.model.set_lang(lang);
        self.view.reset_inputs();
        self.model
            .set_theme_choices(theme::available(Some(&self.dir), &config.theme.unlocked));
        recolor(self.egui.egui_ctx(), palette);
        self.lang = lang;
        self.window.request_redraw();
    }

    /// The colors the terminal shows after the last event of the tab, when they changed:
    /// the active theme with the color being picked, then the active theme as loaded
    /// once the picker closes. The tab takes them too, so both follow the picker. Read
    /// after each event, leaving the tab included.
    pub fn take_preview(&mut self) -> Option<Palette> {
        let wanted = self.model.live_preview(self.view.picked());
        let palette = self
            .preview
            .update(wanted, || self.model.active_palette())?;
        recolor(self.egui.egui_ctx(), &palette);
        self.window.request_redraw();
        Some(palette)
    }

    /// Next time the tab wants a frame, when egui asked for a delayed one.
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

    /// Handles an event of the window while the tab is shown. The pointer events over
    /// the tab bar and the overlays are the window's, not the tab's.
    pub fn window_event(&mut self, event: &WindowEvent) -> Outcome {
        match event {
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            // A captured shortcut is read from the winit event, to match it as the
            // terminal does; egui does not see the key press.
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
            // Escape alone leaves the tab, unless egui uses it (an open popup, a focused
            // field) or it dismisses a shortcut conflict.
            WindowEvent::KeyboardInput { event: key, .. }
                if leaves_on_escape(&key.logical_key, key.state, self.modifiers)
                    && !self.egui_takes_escape() =>
            {
                if self.view.escape() {
                    self.window.request_redraw();
                    return Outcome::Keep;
                }
                return Outcome::Leave;
            }
            _ => {}
        }
        if self.egui.on_window_event(&self.window, event).repaint {
            self.window.request_redraw();
        }
        Outcome::Keep
    }

    /// Writes the color of a picker still open, before the tab is left.
    pub fn leave(&mut self) {
        if let Some(op) = self.view.finish_picking() {
            let mut edits = Edits::default();
            if let Ok(theme_op) = self.model.theme_op(op) {
                edits.theme_ops.push(theme_op);
                self.write(&edits);
            }
        }
        // egui may have asked for the input method of a text field.
        self.window.set_ime_allowed(false);
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
            let unlocked = &self.model.config().theme.unlocked;
            let choices = theme::available(Some(&self.dir), unlocked);
            self.model.set_theme_choices(choices);
            if let Some(name) = next {
                self.model.edit_theme(&name);
            }
            if let Err(error) = result {
                log::warn!("{}", error.text(Lang::EnUs));
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

    /// Runs the egui frame of the tab laid out in `content`, in physical pixels, writes
    /// its changes and uploads its textures to `gpu`: [`Self::paint`] then draws it.
    pub fn prepare(&mut self, gpu: &Gpu, content: Rect) {
        let mut input = self.egui.take_egui_input(&self.window);
        let scale = self.window.scale_factor() as f32;
        input.screen_rect = Some(content_in_points(content, scale));
        let ctx = self.egui.egui_ctx().clone();
        let view = &mut self.view;
        let model = &self.model;
        let lang = self.lang;
        let mut edits = Edits::default();
        let mut output = ctx.run_ui(input, |ui| edits = view.show(ui, lang, model));
        if !edits.is_empty() {
            self.write(&edits);
        }
        self.egui
            .handle_platform_output(&self.window, output.platform_output);
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
                    .update_texture(&gpu.device, &gpu.queue, id, &delta);
            }
        }
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        // A frame left unpainted frees its textures now.
        if let Some(stale) = self.frame.take() {
            self.free(stale.free);
        }
        self.frame = Some(Prepared {
            primitives,
            pixels_per_point: output.pixels_per_point,
            free: textures.free.drain().collect(),
        });
    }

    /// Records the frame [`Self::prepare`] ran into `encoder`, over `view`, a texture of
    /// `size` pixels holding the terminal's frame.
    pub fn paint(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        let Some(frame) = self.frame.take() else {
            return;
        };
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.0, size.1],
            pixels_per_point: frame.pixels_per_point,
        };
        let callbacks = self.renderer.update_buffers(
            &gpu.device,
            &gpu.queue,
            encoder,
            &frame.primitives,
            &screen,
        );
        // Paint callbacks run before the frame's own commands.
        if !callbacks.is_empty() {
            gpu.queue.submit(callbacks);
        }
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("settings pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer
                .render(&mut pass.forget_lifetime(), &frame.primitives, &screen);
        }
        // Freed once recorded: the textures are only released after the submit.
        self.free(frame.free);
    }

    fn free(&mut self, ids: Vec<egui::TextureId>) {
        for id in ids {
            self.renderer.free_texture(&id);
        }
    }
}

/// The content area `content`, in physical pixels, in the points of a window of
/// `scale`: the rectangle egui lays the tab out in.
fn content_in_points(content: Rect, scale: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(content.x / scale, content.y / scale),
        egui::vec2(content.width / scale, content.height / scale),
    )
}

/// Draws the window with the colors of `palette` from the next frame on.
fn recolor(ctx: &egui::Context, palette: &Palette) {
    ctx.set_theme(visuals::theme(palette.background));
    ctx.set_visuals(visuals::visuals(palette));
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
fn leaves_on_escape(key: &Key, state: ElementState, mods: ModifiersState) -> bool {
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

/// Whether the process runs under Wayland, where the quick terminal is unavailable.
fn is_wayland(event_loop: &ActiveEventLoop) -> bool {
    #[cfg(target_os = "linux")]
    {
        use winit::platform::wayland::ActiveEventLoopExtWayland;
        event_loop.is_wayland()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = event_loop;
        false
    }
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
    fn font_ligatures_and_features_are_written_to_the_file() {
        let dir = std::env::temp_dir().join(format!(
            "graphene-settings-font-features-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(config::CONFIG_FILE);
        let path = |segments: &[&str]| segments.iter().map(|s| (*s).to_owned()).collect();
        write_changes(
            &dir,
            &[
                Change::Set(path(&["font", "ligatures"]), Value::Bool(false)),
                Change::Set(
                    path(&["font", "features"]),
                    Value::Text("ss01 -calt".into()),
                ),
            ],
        )
        .unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        let (config, errors) = Config::parse(&text, &Config::default(), &file);
        assert_eq!(errors, vec![]);
        assert!(!config.font.ligatures);
        assert_eq!(config.font.features.to_string(), "ss01 -calt");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unlocking_the_hidden_theme_writes_a_config_that_loads_it() {
        let dir =
            std::env::temp_dir().join(format!("graphene-settings-unlock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(config::CONFIG_FILE);
        std::fs::write(&file, "[theme]\nname = \"gruvbox-dark\"\n").unwrap();
        let (config, _) = Config::parse(
            &std::fs::read_to_string(&file).unwrap(),
            &Config::default(),
            &file,
        );
        let model = SettingsModel::new(
            &config,
            &model::LoadErrors::default(),
            None,
            crate::appearance::Appearance::Dark,
        );
        write_changes(&dir, &model.unlock_hidden_theme()).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        let (config, errors) = Config::parse(&text, &Config::default(), &file);
        assert_eq!(errors, vec![]);
        assert_eq!(config.theme.name, crate::easter_eggs::HIDDEN_THEME);
        assert_eq!(
            config.theme.unlocked,
            vec![crate::easter_eggs::HIDDEN_THEME]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_tab_is_laid_out_in_the_content_area_in_points() {
        let content = Rect {
            x: 0.0,
            y: 60.0,
            width: 2000.0,
            height: 1140.0,
        };
        let rect = content_in_points(content, 2.0);
        assert_eq!(rect.min, egui::pos2(0.0, 30.0));
        assert_eq!(rect.size(), egui::vec2(1000.0, 570.0));
        assert_eq!(content_in_points(content, 1.0).min, egui::pos2(0.0, 60.0));
    }

    #[test]
    fn escape_alone_pressed_leaves_the_tab() {
        let escape = Key::Named(NamedKey::Escape);
        let none = ModifiersState::empty();
        assert!(leaves_on_escape(&escape, ElementState::Pressed, none));
        assert!(!leaves_on_escape(&escape, ElementState::Released, none));
        assert!(!leaves_on_escape(
            &escape,
            ElementState::Pressed,
            ModifiersState::SHIFT
        ));
        assert!(!leaves_on_escape(
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
    fn recoloring_draws_the_window_with_the_previewed_colors() {
        let ctx = egui::Context::default();
        let palette = Palette {
            background: alacritty_terminal::vte::ansi::Rgb {
                r: 250,
                g: 250,
                b: 240,
            },
            ..Palette::default()
        };
        recolor(&ctx, &palette);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        assert_eq!(
            ctx.style_of(egui::Theme::Light).visuals,
            visuals::visuals(&palette)
        );
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
