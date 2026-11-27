//! The application: its windows, their creation from the configuration, and the routing of the event loop to them.

use super::*;

/// Window size asked on the command line, in cells of the first pane's grid. A
/// dimension left unset keeps the size from the configuration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GridSize {
    pub columns: Option<u16>,
    pub lines: Option<u16>,
}

/// The program run by the first pane instead of the shell (`-e`, `-E`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    /// The program then its arguments.
    pub argv: Vec<String>,
    /// `-E`: the pane stays open once the program ends, until a key is pressed.
    pub hold: bool,
}

impl GridSize {
    pub(super) fn is_set(self) -> bool {
        self.columns.is_some() || self.lines.is_some()
    }
}

/// Inner size of a window whose single pane shows `grid`, in physical pixels, for
/// cells of `cell` and `padding` around them, `chrome_height` being the height above
/// the grid (banner and pane bar). A dimension `grid` leaves unset keeps `current`.
pub(super) fn window_size_for_grid(
    grid: GridSize,
    current: PhysicalSize<u32>,
    cell: CellSize,
    padding: f32,
    chrome_height: f32,
) -> PhysicalSize<u32> {
    let (width, height) = GridLayout::rect_size(
        grid.columns.unwrap_or(1).into(),
        grid.lines.unwrap_or(1).into(),
        cell,
        padding,
    );
    PhysicalSize::new(
        if grid.columns.is_some() {
            width
        } else {
            current.width
        },
        if grid.lines.is_some() {
            height + chrome_height.ceil() as u32
        } else {
            current.height
        },
    )
}

/// Size and maximized state of the window of a launch: the ones of the window it
/// reopens (`restored`) whatever the startup mode, else [`initial_window`].
pub(super) fn launch_window(
    config: &WindowConfig,
    remembered: Option<WindowState>,
    restored: Option<WindowState>,
) -> (LogicalSize<u32>, bool) {
    match restored {
        Some(state) => (LogicalSize::new(state.width, state.height), state.maximized),
        None => initial_window(config, remembered),
    }
}

/// Size and maximized state of a new window, from `window.startup_mode`. `remembered`
/// is the last window closed, only read in `remember` mode.
pub(super) fn initial_window(
    config: &WindowConfig,
    remembered: Option<WindowState>,
) -> (LogicalSize<u32>, bool) {
    let configured = LogicalSize::new(config.width, config.height);
    match (config.startup_mode, remembered) {
        (StartupMode::Default, _) | (StartupMode::Remember, None) => (configured, false),
        (StartupMode::Maximized, _) => (configured, true),
        (StartupMode::Remember, Some(state)) => {
            (LogicalSize::new(state.width, state.height), state.maximized)
        }
    }
}

/// The grid font of the renderer for the configured `font`.
pub(super) fn font_options(font: &FontConfig) -> FontOptions {
    FontOptions {
        family: font.family.clone(),
        ligatures: font.ligatures,
        features: font.features.font_features(),
    }
}

/// Whether a reload from `old` to `new` changes the grid font, so it must be loaded
/// again (which drops the glyph caches).
pub(super) fn font_changed(old: &FontConfig, new: &FontConfig) -> bool {
    new.family != old.family || new.ligatures != old.ligatures || new.features != old.features
}

/// The earlier of two optional deadlines.
pub(super) fn earliest(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

pub struct App {
    pub(super) proxy: EventLoopProxy<UserEvent>,
    pub(super) config: Config,
    /// Palette of the configured theme.
    pub(super) palette: Palette,
    pub(super) session: Option<Session>,
    pub(super) update_checker: Checker,
    pub(super) error: Option<anyhow::Error>,
    /// Banner reporting the errors of the configuration and theme files.
    pub(super) config_banner: Option<Banner>,
    /// Errors of the last load of the configuration and theme files, shown by the
    /// settings tab.
    pub(super) load_errors: LoadErrors,
    /// System appearance, which picks the `light` or `dark` theme.
    pub(super) appearance: Appearance,
    /// Whether the appearance thread runs: started once a configuration needs it.
    pub(super) appearance_watched: bool,
    /// Program given by `-e` or `-E`, run by the first pane instead of the shell; taken
    /// when that pane spawns.
    pub(super) first_program: Option<Program>,
    /// Size of the first window asked by `--columns` and `--lines`.
    pub(super) grid: GridSize,
    /// An ordinary window, or the quick terminal of `graphene --quick`.
    pub(super) mode: WindowMode,
    /// Exit code of the process once the event loop ends.
    pub(super) exit_code: i32,
    /// Global hotkey of the quick terminal process, created at its first registration.
    pub(super) hotkey: Option<crate::hotkey::Hotkey>,
    /// Why the hotkey could not be registered, shown by the configuration banner.
    pub(super) hotkey_error: Option<ConfigError>,
    /// Whether the session supports the quick terminal (not Wayland), known once the
    /// event loop resumed.
    pub(super) quick_supported: bool,
    /// Saves the window to the session while it runs, when this launch takes part in it
    /// (see [`crate::session::participates`]).
    pub(super) session_saver: Option<SessionSaver>,
    /// The saved window this launch reopens, taken when the window is created.
    pub(super) restored: Option<WindowSnapshot>,
}

/// Whether the window is created transparent for a background `opacity`. Elsewhere
/// always: winit only honours it at creation on X11, and the opacity can change by hot
/// reload. On macOS only below 1.0, and updated on reload: a transparent window also
/// clears the background of the title bar.
pub(super) fn window_transparent(opacity: f32, macos: bool) -> bool {
    !macos || opacity < 1.0
}

/// Creates the GPU context of `window` while `work` runs, which overlaps the slowest
/// startup step with the others. On macOS in sequence: AppKit views, which the surface
/// is created on, belong to the main thread.
fn gpu_alongside<T>(
    window: Arc<Window>,
    display: winit::event_loop::OwnedDisplayHandle,
    work: impl FnOnce() -> T,
) -> (anyhow::Result<(Arc<Gpu>, wgpu::Surface<'static>)>, T) {
    if cfg!(target_os = "macos") {
        let value = work();
        return (pollster::block_on(Gpu::new(window, display)), value);
    }
    std::thread::scope(|scope| {
        let gpu = scope.spawn(|| pollster::block_on(Gpu::new(window, display)));
        let value = work();
        let gpu = gpu
            .join()
            .unwrap_or_else(|_| Err(anyhow::anyhow!("GPU setup panicked")));
        (gpu, value)
    })
}

impl App {
    pub(super) fn create_session(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) -> anyhow::Result<Session> {
        let remembered = match self.config.window.startup_mode {
            StartupMode::Remember => window_state::path().and_then(|file| WindowState::load(&file)),
            StartupMode::Default | StartupMode::Maximized => None,
        };
        let restored = self.restored.take();
        let (mut initial_size, maximized) = launch_window(
            &self.config.window,
            remembered,
            restored.as_ref().map(|snapshot| snapshot.window),
        );
        let quick = matches!(self.mode, WindowMode::Quick(_));
        let maximized = maximized && !self.grid.is_set() && !quick;
        let today = special_days::today();
        let title = special_days::titled(today.as_ref(), DEFAULT_TITLE);
        let mut attributes = Window::default_attributes()
            .with_title(&title)
            .with_inner_size(initial_size)
            .with_maximized(maximized)
            // Shown once the renderer is ready: until then, it would stay
            // empty, at its configured size before the window manager maximizes it.
            .with_visible(false)
            .with_transparent(window_transparent(
                self.config.window.background_opacity,
                cfg!(target_os = "macos"),
            ));
        // The quick terminal goes to its own place, from `[quick_terminal]`.
        if let Some((x, y)) = restored.as_ref().filter(|_| !quick).and_then(|snapshot| {
            let monitors: Vec<String> = event_loop
                .available_monitors()
                .filter_map(|monitor| monitor.name())
                .collect();
            crate::session::restored_position(
                snapshot.placement.as_ref(),
                monitors.iter().map(String::as_str),
            )
        }) {
            attributes = attributes.with_position(PhysicalPosition::new(x, y));
        }
        if quick {
            // Created at its place, hidden until a command shows it: the shell is
            // already running when it first appears.
            attributes = attributes
                .with_decorations(false)
                .with_window_level(winit::window::WindowLevel::AlwaysOnTop);
            // A dock window: window managers neither play their opening effect on it
            // nor keep it inside the screen, which would stop its slide. They do not
            // focus it either: the quick terminal takes the focus itself.
            #[cfg(target_os = "linux")]
            {
                use winit::platform::x11::{WindowAttributesExtX11, WindowType};
                attributes = attributes.with_x11_window_type(vec![WindowType::Dock]);
            }
            if let Some(placement) = quick::placement(event_loop, &self.config.quick_terminal) {
                initial_size = placement.logical;
                attributes = attributes
                    .with_inner_size(placement.size)
                    .with_position(placement.position);
            }
        }
        let window = Arc::new(event_loop.create_window(attributes)?);
        if quick {
            quick::prepare_window(&window);
        }
        #[cfg(target_os = "linux")]
        if maximized
            && let Err(error) =
                crate::x11_window::preset_states(&window, &crate::x11_window::MAXIMIZED)
        {
            log::warn!("cannot open the window maximized: {error:#}");
        }
        perf::mark("window created");

        // The GPU context is created while the glyphs load and the shell starts: the
        // PTY reader thread then parses the shell output while the renderer is being
        // created, so the first frame shows it complete.
        let display = event_loop.owned_display_handle();
        let (gpu, first_pane) = gpu_alongside(window.clone(), display.clone(), || {
            self.spawn_workspace(&window, restored.as_ref())
        });
        let (glyphs, workspace) = first_pane?;
        let (gpu, surface) = gpu?;
        let scale = window.scale_factor() as f32;
        let mut renderer = Renderer::new(
            gpu,
            surface,
            window.clone(),
            display,
            glyphs,
            self.config.window.background_opacity,
        )?;
        renderer.set_scanlines(easter_eggs::shows_scanlines(
            self.config.theme.active(self.appearance),
        ));

        if !quick {
            window.set_visible(true);
            perf::mark("window shown");
        }
        let taskbar = crate::taskbar::spawn(&window);
        let maximize_hold = maximized.then(|| {
            redraw::MaximizeHold::new(initial_size.to_physical(scale.into()), Instant::now())
        });

        let clipboard = arboard::Clipboard::new()
            .map_err(|e| log::warn!("clipboard unavailable: {e}"))
            .ok();
        let lang = self.config.language.resolve(Lang::detect);

        let mut session = Session {
            window,
            proxy: self.proxy.clone(),
            term_config: self.config.term_config(),
            renderer,
            workspace,
            drag: None,
            overlay: None,
            palette: self.palette.clone(),
            menu_theme: None,
            theme: self.config.theme.clone(),
            appearance: self.appearance,
            clipboard,
            focused: true,
            cursor_blink: CursorBlink::new(Instant::now()),
            modifiers: ModifiersState::empty(),
            font_size: self.config.font.size,
            padding: self.config.window.padding,
            mouse_position: PhysicalPosition::default(),
            hovered_cell: None,
            hovered_link: None,
            link_press: None,
            pane_bar_clicks: ClickTracker::default(),
            tab_bar_clicks: ClickTracker::default(),
            font_wheel_accumulator: 0.0,
            tooltip_hidden: false,
            pointer: None,
            hovered_button: None,
            window_title: title,
            config_banner: self.config_banner.clone(),
            update: UpdateBanner::default(),
            ignored_tag: None,
            lang,
            tab_position: self.config.tab_position.into(),
            keybindings: Keybindings::new(&self.config.keybindings),
            first_frame: perf::FirstFrame::default(),
            redraw_stats: perf::RedrawStats::default(),
            redraw_at: None,
            maximize_hold,
            frames: FrameCache::default(),
            restore_size: initial_size,
            settings_requested: false,
            settings: None,
            sized_by_grid: self.grid.is_set(),
            konami: KonamiDetector::default(),
            honeycomb: None,
            today,
            midnight: special_days::until_midnight().map(|left| Instant::now() + left),
            notifications: self.config.notifications.clone(),
            notifier: notifications::backend::spawn(
                self.proxy.clone(),
                crate::i18n::text(lang, &crate::i18n::Msg::ShowPane).into(),
            ),
            badge: Badge::default(),
            progress_epoch: Instant::now(),
            taskbar,
            quick,
            quick_closed: false,
        };
        if restored.is_some() {
            // The restored panes were all given the size of a single pane.
            session.relayout();
        }
        Ok(session)
    }

    /// Loads the glyphs of the first window and spawns the shells of its workspace: the
    /// tabs of `restored` when given and at least one of their shells starts, else a
    /// single pane.
    fn spawn_workspace(
        &mut self,
        window: &Window,
        restored: Option<&WindowSnapshot>,
    ) -> anyhow::Result<(Glyphs, Workspace<Pane>)> {
        let scale = window.scale_factor() as f32;
        let mut glyphs = Glyphs::new(
            &font_options(&self.config.font),
            self.config.font.size * scale,
        );
        glyphs.set_ui_scale(scale);
        perf::mark("glyphs ready");

        let mut size = window.inner_size();
        // A single tab: no tab bar. A configuration error banner is already shown; the
        // update banner only comes later, from the check thread.
        let banner_height = match self.config_banner {
            Some(_) => BannerLayout::height(glyphs.cell_size()),
            None => 0.0,
        };
        let ui = UiMetrics {
            line_height: glyphs.ui_line_height(UiFont::Chrome),
            scale,
        };
        if self.grid.is_set() {
            let target = window_size_for_grid(
                self.grid,
                size,
                glyphs.cell_size(),
                self.config.window.padding * scale,
                banner_height + pane_bar::height(ui),
            );
            // Applied later, with a `Resized` event, when the platform cannot resize at
            // once.
            size = window.request_inner_size(target).unwrap_or(target);
        }
        let (_, content) = pane_bar::split(
            banner::below_banner(Rect::from_size(size.width, size.height), banner_height),
            ui,
        );
        let layout = GridLayout::new(
            content,
            glyphs.cell_size(),
            self.config.window.padding * scale,
        );
        let mut workspace = match restored {
            Some(snapshot) => self.restore_workspace(snapshot, layout),
            None => Workspace::new(),
        };
        if workspace.tabs().is_empty() {
            let id = workspace.allocate_id();
            let program = self.first_program.take();
            let hold = program.as_ref().is_some_and(|program| program.hold);
            let terminal = Terminal::spawn(
                EventProxy::new(self.proxy.clone(), id),
                layout.window_size(),
                self.config.term_config(),
                None,
                program.map(|program| program.argv),
            )?;
            let mut pane = Pane::new(id, terminal, layout, 0.0);
            if hold {
                pane.hold = Hold::Armed;
            }
            workspace.new_tab(id, pane);
        }
        workspace.set_progress_shown(self.config.window.progress);
        perf::mark("shell spawned");

        Ok((glyphs, workspace))
    }

    /// The tabs of `snapshot`, each shell started in its saved directory (or its nearest
    /// parent still there) with `layout`, resized once the window lays them out. A shell
    /// that fails is logged and left out.
    fn restore_workspace(&self, snapshot: &WindowSnapshot, layout: GridLayout) -> Workspace<Pane> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let workspace = Workspace::restore(
            &snapshot.tabs,
            snapshot.active_tab,
            |id, cwd, font_offset| {
                let cwd =
                    cwd.and_then(|cwd| crate::session::nearest_existing_dir(&cwd, home.clone()));
                match Terminal::spawn(
                    EventProxy::new(self.proxy.clone(), id),
                    layout.window_size(),
                    self.config.term_config(),
                    cwd,
                    None,
                ) {
                    Ok(terminal) => Some(Pane::new(id, terminal, layout, font_offset)),
                    Err(error) => {
                        log::error!("cannot start a shell of the restored session: {error:#}");
                        None
                    }
                }
            },
        );
        perf::mark("session restored");
        workspace
    }

    /// Reads the configuration and its theme again after a change on disk, and applies
    /// them to the open window.
    pub(super) fn reload_config(&mut self) {
        let (config, errors) = config::reload(&self.config);
        let (config, palette, _, load_errors) = with_theme(config, errors, self.appearance);
        let old = std::mem::replace(&mut self.config, config);
        self.watch_appearance();
        self.palette = palette;
        self.load_errors = load_errors;
        self.register_hotkey();
        if hotkey::hidden_start_turned_on(&old.quick_terminal, &self.config.quick_terminal) {
            self.start_hidden_quick();
        }
        self.sync_autostart(Some(hotkey::effective_autostart(&old.quick_terminal)));
        let banner = hotkey::config_banner(
            &self.load_errors,
            self.hotkey_error.as_ref(),
            self.config.language.resolve(Lang::detect),
        );
        self.config_banner = banner.clone();
        if let Some(settings) = self.session.as_mut().and_then(|s| s.settings.as_mut()) {
            settings.apply(
                &self.palette,
                self.config.language.resolve(Lang::detect),
                &self.config,
                &self.load_errors,
                self.appearance,
            );
        }
        if let Some(session) = self.session.as_mut() {
            session.apply_config(&old, &self.config, &self.palette, banner);
            session.renderer.set_scanlines(easter_eggs::shows_scanlines(
                self.config.theme.active(self.appearance),
            ));
            if old.updates != self.config.updates {
                self.update_checker.apply(&self.config.updates);
            }
        }
    }

    /// Starts the appearance thread when the configuration sets `light` and `dark` and it
    /// does not run yet. The returned receiver gets the first appearance read, so the
    /// first window can open with the right theme.
    pub(super) fn watch_appearance(&mut self) -> Option<mpsc::Receiver<Appearance>> {
        if self.appearance_watched || !self.config.theme.follows_appearance() {
            return None;
        }
        self.appearance_watched = true;
        let proxy = self.proxy.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        appearance::spawn(move |appearance| {
            // Only the first value is waited for; a full or closed channel is fine.
            let _ = sender.try_send(appearance);
            // The event loop may already be closed during shutdown.
            let _ = proxy.send_event(UserEvent::Appearance(appearance));
        });
        Some(receiver)
    }

    /// A new system appearance: the theme is loaded again when it depends on it.
    pub(super) fn set_appearance(&mut self, appearance: Appearance) {
        if appearance == self.appearance {
            return;
        }
        self.appearance = appearance;
        if self.config.theme.follows_appearance() {
            self.reload_config();
        }
    }

    /// Shows the settings tab of the window; it does nothing more when already shown.
    pub(super) fn open_settings(&mut self, event_loop: &ActiveEventLoop) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if session.settings.is_some() {
            return;
        }
        let Some(dir) = config::config_dir() else {
            log::warn!("neither XDG_CONFIG_HOME nor HOME is set: no settings tab");
            return;
        };
        let lang = self.config.language.resolve(Lang::detect);
        let settings = SettingsTab::open(
            event_loop,
            session.renderer.gpu(),
            session.renderer.format(),
            session.window.clone(),
            &self.palette,
            lang,
            &self.config,
            &self.load_errors,
            self.appearance,
            dir,
        );
        session.show_settings(settings);
    }

    /// Shows the settings tab when the last event asked for it.
    pub(super) fn open_requested_settings(&mut self, event_loop: &ActiveEventLoop) {
        let requested = self
            .session
            .as_mut()
            .is_some_and(|session| std::mem::take(&mut session.settings_requested));
        if requested {
            self.open_settings(event_loop);
        }
    }

    pub(super) fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.error = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.session.is_some() {
            return;
        }
        self.quick_supported = quick::quick_supported(event_loop);
        if self.quick_state().is_some() && !self.quick_supported {
            self.exit_code = crate::quick_terminal::control::WAYLAND_EXIT_CODE;
            return event_loop.exit();
        }
        match self.create_session(event_loop) {
            Ok(mut session) => {
                session.update_pointer();
                self.session = Some(session);
            }
            Err(error) => return self.fail(event_loop, error),
        }
        self.update_checker.apply(&self.config.updates);
        // The window is shown: a hidden quick terminal starts for the hotkey.
        self.start_hidden_quick();
        self.sync_autostart(None);
        if let Some(state) = self.quick_state() {
            let start_hidden = state.start_hidden;
            if let Err(error) = self.listen_quick() {
                return self.fail(event_loop, error);
            }
            if self.register_hotkey() {
                self.refresh_config_banner();
            }
            if !start_hidden {
                self.show_quick(event_loop);
            }
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(session) = &self.session
            && !session.sized_by_grid
            && self.mode == WindowMode::Normal
        {
            session.save_window_state();
        }
        if let Some(session) = &self.session
            && let Some(saver) = self.session_saver.as_mut()
        {
            let closed_at = crate::session::unix_seconds(std::time::SystemTime::now());
            saver.finish(&session.window_snapshot(Some(closed_at)));
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let deadline = self.session.as_mut().and_then(|session| {
            let deadline = earliest(session.auto_scroll(now), session.animate(now));
            let deadline = earliest(deadline, session.blink(now));
            let deadline = earliest(deadline, session.fold_animate(now));
            let deadline = earliest(deadline, session.progress_tick(now));
            session.sync_taskbar();
            let deadline = earliest(deadline, session.honeycomb_tick(now));
            let deadline = earliest(deadline, session.special_day_tick(now));
            let deadline = earliest(deadline, session.pane_drag_tick(now));
            let deadline = earliest(deadline, session.renderer.finish_presentation(now));
            let deadline = earliest(deadline, session.deferred_redraw(now));
            session.flush_panes();
            deadline
        });
        let deadline = earliest(deadline, self.quick_tick(event_loop, now));
        let deadline = earliest(
            deadline,
            self.session
                .as_ref()
                .zip(self.session_saver.as_mut())
                .map(|(session, saver)| saver.tick(now, || session.window_snapshot(None))),
        );
        let deadline = earliest(
            deadline,
            self.session
                .as_mut()
                .and_then(|session| session.settings.as_mut())
                .and_then(|settings| {
                    settings.wake(now);
                    settings.deadline()
                }),
        );
        event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::ConfigChanged => return self.reload_config(),
            UserEvent::Appearance(appearance) => return self.set_appearance(appearance),
            UserEvent::Quick(command) => return self.quick_command(event_loop, command),
            UserEvent::Terminal(..) | UserEvent::Update(_) | UserEvent::NotificationClicked(_) => {}
        }
        let Some(session) = self.session.as_mut() else {
            return;
        };
        match event {
            UserEvent::Terminal(pane, event) => {
                session.handle_terminal_event(pane, event, event_loop)
            }
            UserEvent::Update(release) => {
                session.update.set(release);
                // The banner may show or hide: the panes make room for it.
                session.relayout();
            }
            UserEvent::NotificationClicked(pane) => session.notification_clicked(pane),
            UserEvent::ConfigChanged | UserEvent::Appearance(_) | UserEvent::Quick(_) => {}
        }
        session.flush_panes();
        self.open_requested_settings(event_loop);
        self.reset_closed_quick(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let session_window = self.session.as_ref().map(|session| session.window.id());
        match route(id, session_window) {
            Route::Session => {
                if let WindowEvent::Focused(focused) = event {
                    self.quick_focus_changed(focused);
                }
                if let WindowEvent::MouseInput {
                    state: winit::event::ElementState::Pressed,
                    ..
                } = event
                {
                    self.quick_clicked();
                }
                self.session_window_event(event_loop, event);
                self.open_requested_settings(event_loop);
                self.reset_closed_quick(event_loop);
            }
            Route::Unknown => {}
        }
    }
}

/// Window an event is delivered to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Route {
    /// The terminal window.
    Session,
    /// A window `App` does not know (already closed): the event is dropped.
    Unknown,
}

/// Routes an event of window `id`; `session` is the terminal window.
pub(super) fn route(id: WindowId, session: Option<WindowId>) -> Route {
    if session == Some(id) {
        Route::Session
    } else {
        Route::Unknown
    }
}

pub(super) fn is_real_key_press(state: ElementState, is_synthetic: bool) -> bool {
    state == ElementState::Pressed && !is_synthetic
}

impl App {
    /// Handles an event of the terminal window.
    pub(super) fn session_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: WindowEvent,
    ) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if !session.settings_window_event(&event) {
            session.flush_panes();
            return;
        }

        match event {
            WindowEvent::CloseRequested => session.request_close(Target::Window, event_loop),
            WindowEvent::Resized(size) => {
                if !session.window.is_maximized() {
                    session.restore_size = size.to_logical(session.window.scale_factor());
                }
                session.renderer.resize(size.width, size.height);
                session.relayout();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let font_size = session.font_size;
                session.apply_font_size(font_size);
            }
            WindowEvent::Focused(focused) => {
                session.focused = focused;
                if !focused {
                    session.cancel_pane_drag();
                }
                if let Some(pane) = session.workspace.active_pane()
                    && let Some(bytes) = mouse::focus_report(focused, pane.mode())
                {
                    pane.terminal.write(bytes);
                }
                session.clear_watched();
                session.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                session.modifiers = modifiers.state();
                // `Ctrl` makes a link active where the application captures the mouse.
                if session.hovered_link.is_some() {
                    session.update_pointer();
                    session.window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } if is_real_key_press(event.state, is_synthetic) => {
                session.key_press(&event, event_loop);
            }
            WindowEvent::MouseWheel { delta, .. } => session.mouse_wheel(delta),
            WindowEvent::CursorMoved { position, .. } => session.cursor_moved(position),
            WindowEvent::CursorLeft { .. } => session.cursor_left(),
            WindowEvent::MouseInput { state, button, .. } => {
                session.mouse_input(state, button, event_loop);
                if let Some(tag) = session.ignored_tag.take() {
                    self.update_checker.ignore(&tag);
                }
            }
            WindowEvent::RedrawRequested => session.redraw(),
            _ => {}
        }
        session.flush_panes();
    }
}

/// Loads the configuration file and its theme. Errors are logged and reported by the
/// returned banner.
pub(super) fn load_config() -> (Config, Palette, Option<Banner>, LoadErrors) {
    let (config, errors) = config::load();
    with_theme(config, errors, Appearance::default())
}

/// Loads the theme of `config`; `errors`, those of the configuration file, and the
/// theme's are logged, reported by the returned banner and returned apart.
pub(super) fn with_theme(
    config: Config,
    errors: Vec<ConfigError>,
    appearance: Appearance,
) -> (Config, Palette, Option<Banner>, LoadErrors) {
    let name = config.theme.active(appearance);
    let (palette, theme_errors) = theme::load(name, config::config_dir().as_deref());
    let all: Vec<ConfigError> = errors.iter().chain(&theme_errors).cloned().collect();
    for error in &all {
        log::warn!("{}", error.text(Lang::EnUs));
    }
    let load_errors = LoadErrors {
        config: errors,
        theme: theme_errors,
    };
    let lang = config.language.resolve(Lang::detect);
    (
        config,
        palette,
        Banner::config_errors(&all, lang),
        load_errors,
    )
}

pub fn run(
    first_program: Option<Program>,
    grid: GridSize,
    no_restore: bool,
    restore_session: Option<PathBuf>,
) -> anyhow::Result<()> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    run_app(
        event_loop,
        WindowMode::Normal,
        first_program,
        grid,
        Restore {
            no_restore,
            handed_over: restore_session,
        },
    )
    .map(|_| ())
}

/// How a launch takes part in the session restoration.
#[derive(Debug, Default)]
struct Restore {
    /// `--no-restore`: out of the session.
    no_restore: bool,
    /// `--restore-session <file>`: the saved window this launch reopens.
    handed_over: Option<PathBuf>,
}

/// The exit code of a `--quick` process that must not run for `config`, before it takes
/// the lock: `DISABLED_EXIT_CODE` when the quick terminal is disabled. Checked before
/// Wayland, so that a disabled quick terminal never reports the platform.
fn quick_allowed(config: &Config) -> Option<i32> {
    (!config.quick_terminal.enabled).then_some(crate::quick_terminal::control::DISABLED_EXIT_CODE)
}

/// Runs the quick terminal process: see [`crate::run_quick`].
pub fn run_quick(hidden: bool) -> anyhow::Result<i32> {
    let (config, _) = config::load();
    if let Some(code) = quick_allowed(&config) {
        return Ok(code);
    }
    let dir = config::runtime_dir()?;
    let Some(lock) = crate::quick_terminal::control::claim(&dir, hidden)? else {
        return Ok(0);
    };
    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        // Neither in the Dock nor in the application switcher.
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    let event_loop = builder.build()?;
    let mode = WindowMode::Quick(QuickState::new(hidden));
    let code = run_app(
        event_loop,
        mode,
        None,
        GridSize::default(),
        Restore::default(),
    );
    drop(lock);
    code
}

impl App {
    /// Listens on the quick terminal socket, once the process knows it can run: a
    /// `--quick-toggle` that reaches the socket takes the window for started.
    fn listen_quick(&mut self) -> anyhow::Result<()> {
        let dir = config::runtime_dir()?;
        let listener = crate::quick_terminal::control::bind(&dir)?;
        let proxy = self.proxy.clone();
        crate::quick_terminal::control::serve(listener, move |command| {
            proxy.send_event(UserEvent::Quick(command)).is_ok()
        })?;
        Ok(())
    }
}

/// Runs `event_loop` with the application in `mode`, and returns the exit code.
/// `restore` tells how the window takes part in the session.
fn run_app(
    event_loop: EventLoop<UserEvent>,
    mode: WindowMode,
    first_program: Option<Program>,
    grid: GridSize,
    restore: Restore,
) -> anyhow::Result<i32> {
    let update_proxy = event_loop.create_proxy();
    let (config, palette, config_banner, load_errors) = load_config();
    let kind = match mode {
        WindowMode::Normal => WindowKind::Normal,
        WindowMode::Quick(_) => WindowKind::Quick,
    };
    let session_saver = crate::session::participates(
        config.window.restore_session,
        first_program.is_some(),
        grid.is_set(),
        restore.no_restore,
    )
    .then(|| start_session(kind, restore.handed_over.as_deref()))
    .flatten();
    let (session_saver, restored) = match session_saver {
        Some((saver, restored)) => (Some(saver), restored),
        None => (None, None),
    };
    if let Some(dir) = config::config_dir() {
        let proxy = event_loop.create_proxy();
        watcher::spawn(dir, move || {
            // The event loop may already be closed during shutdown.
            let _ = proxy.send_event(UserEvent::ConfigChanged);
        });
    }
    let mut app = App {
        update_checker: Checker::new(move |release| {
            // The event loop may already be closed during shutdown.
            let _ = update_proxy.send_event(UserEvent::Update(release));
        }),
        proxy: event_loop.create_proxy(),
        config,
        palette,
        session: None,
        error: None,
        config_banner,
        load_errors,
        appearance: Appearance::default(),
        appearance_watched: false,
        first_program,
        grid,
        mode,
        exit_code: 0,
        hotkey: None,
        hotkey_error: None,
        quick_supported: false,
        session_saver,
        restored,
    };
    if let Some(first) = app.watch_appearance()
        && let Ok(appearance) = first.recv_timeout(APPEARANCE_STARTUP_WAIT)
    {
        app.set_appearance(appearance);
    }
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error),
        None => Ok(app.exit_code),
    }
}

/// A saver for a new instance of `kind` in the session folder, with the saved window it
/// reopens: `handed_over` when given (see [`crate::session::adopt`]), else the one of
/// `kind` it picks, starting a process for each other window to reopen (see
/// [`crate::session::start`]; never for the quick terminal). `None`, logged, when the
/// folder cannot be found or the instance cannot be claimed.
fn start_session(
    kind: WindowKind,
    handed_over: Option<&std::path::Path>,
) -> Option<(SessionSaver, Option<WindowSnapshot>)> {
    let Some(dir) = crate::session::sessions_dir() else {
        log::warn!("neither XDG_STATE_HOME nor HOME is set: the session is not saved");
        return None;
    };
    let started = match handed_over {
        Some(file) => crate::session::adopt(&dir, file),
        None => crate::session::start(&dir, kind, spawn_restored_window),
    };
    match started {
        Ok((instance, restored)) => Some((SessionSaver::new(instance, Instant::now()), restored)),
        Err(error) => {
            log::warn!("cannot save the session in {}: {error}", dir.display());
            None
        }
    }
}

/// Starts `graphene --restore-session <file>`, the window that reopens `file`, in a
/// session of its own and without waiting for it. A thread reaps it when it exits.
fn spawn_restored_window(file: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--restore-session")
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: `setsid` is async-signal-safe, as `pre_exec` requires.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    // The window runs already: a thread that cannot start only leaves it unreaped.
    if let Err(error) = std::thread::Builder::new()
        .name("restored-window".to_owned())
        .spawn(move || {
            let _ = child.wait();
        })
    {
        log::warn!("cannot wait for a restored window: {error}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FontFeatureList;

    #[test]
    fn a_disabled_quick_terminal_process_exits_with_its_code() {
        let mut config = Config::default();
        assert_eq!(
            quick_allowed(&config),
            Some(crate::quick_terminal::control::DISABLED_EXIT_CODE)
        );
        config.quick_terminal.enabled = true;
        assert_eq!(quick_allowed(&config), None);
    }

    #[test]
    fn grid_size_gives_the_window_size_holding_that_grid() {
        let cell = CellSize {
            width: 9.0,
            height: 19.0,
        };
        let current = PhysicalSize::new(1000, 700);
        let grid = GridSize {
            columns: Some(100),
            lines: Some(30),
        };
        let size = window_size_for_grid(grid, current, cell, 8.0, 30.5);
        let content = Rect {
            x: 0.0,
            y: 30.5,
            width: size.width as f32,
            height: size.height as f32 - 30.5,
        };
        let layout = GridLayout::new(content, cell, 8.0);
        assert_eq!((layout.columns, layout.lines), (100, 30));
    }

    #[test]
    fn grid_size_keeps_the_dimension_it_leaves_unset() {
        let cell = CellSize {
            width: 9.0,
            height: 19.0,
        };
        let current = PhysicalSize::new(1000, 700);
        let only_lines = GridSize {
            columns: None,
            lines: Some(30),
        };
        let size = window_size_for_grid(only_lines, current, cell, 8.0, 30.0);
        assert_eq!(size.width, 1000);
        assert_ne!(size.height, 700);
        let only_columns = GridSize {
            columns: Some(100),
            lines: None,
        };
        let size = window_size_for_grid(only_columns, current, cell, 8.0, 30.0);
        assert_ne!(size.width, 1000);
        assert_eq!(size.height, 700);
        assert!(!GridSize::default().is_set());
    }

    #[test]
    fn macos_window_transparent_only_below_full_opacity() {
        assert!(!window_transparent(1.0, true));
        assert!(window_transparent(0.9, true));
        assert!(window_transparent(1.0, false));
        assert!(window_transparent(0.9, false));
    }

    #[test]
    fn events_of_the_terminal_window_go_to_the_session() {
        let terminal = WindowId::from(1);
        assert_eq!(route(terminal, Some(terminal)), Route::Session);
    }

    #[test]
    fn events_of_an_unknown_window_are_dropped() {
        let terminal = WindowId::from(1);
        assert_eq!(route(WindowId::from(2), Some(terminal)), Route::Unknown);
        assert_eq!(route(terminal, None), Route::Unknown);
    }

    #[test]
    fn synthetic_focus_key_presses_are_not_forwarded() {
        assert!(is_real_key_press(ElementState::Pressed, false));
        assert!(!is_real_key_press(ElementState::Pressed, true));
        assert!(!is_real_key_press(ElementState::Released, false));
        assert!(!is_real_key_press(ElementState::Released, true));
    }

    #[test]
    fn earliest_keeps_the_sooner_deadline() {
        let now = Instant::now();
        let later = now + Duration::from_millis(5);
        assert_eq!(earliest(Some(later), Some(now)), Some(now));
        assert_eq!(earliest(None, Some(later)), Some(later));
        assert_eq!(earliest(Some(now), None), Some(now));
        assert_eq!(earliest(None, None), None);
    }

    fn font(family: Option<&str>, size: f32) -> FontConfig {
        FontConfig {
            family: family.map(str::to_owned),
            size,
            ..Config::default().font
        }
    }

    #[test]
    fn font_is_reloaded_when_family_ligatures_or_features_change() {
        let base = font(None, 14.0);
        assert!(!font_changed(&base, &base));
        assert!(!font_changed(&base, &font(None, 12.0)));
        assert!(font_changed(&base, &font(Some("Fira Code"), 14.0)));
        let no_ligatures = FontConfig {
            ligatures: false,
            ..base.clone()
        };
        assert!(font_changed(&base, &no_ligatures));
        let features = FontConfig {
            features: FontFeatureList::parse("zero").unwrap(),
            ..base.clone()
        };
        assert!(font_changed(&base, &features));
    }

    #[test]
    fn font_options_carry_the_configured_font() {
        let config = FontConfig {
            family: Some("Fira Code".to_owned()),
            ligatures: false,
            features: FontFeatureList::parse("ss01 -calt").unwrap(),
            ..Config::default().font
        };
        let options = font_options(&config);
        assert_eq!(options.family.as_deref(), Some("Fira Code"));
        assert!(!options.ligatures);
        assert_eq!(options.features, config.features.font_features());
    }

    fn window_config(startup_mode: StartupMode) -> WindowConfig {
        WindowConfig {
            startup_mode,
            width: 960,
            height: 600,
            padding: 8.0,
            background_opacity: 1.0,
            progress: true,
            restore_session: false,
        }
    }

    const REMEMBERED: WindowState = WindowState {
        width: 1280,
        height: 720,
        maximized: true,
    };

    #[test]
    fn default_mode_uses_the_configured_size() {
        let config = window_config(StartupMode::Default);
        let expected = (LogicalSize::new(960, 600), false);
        assert_eq!(initial_window(&config, None), expected);
        assert_eq!(initial_window(&config, Some(REMEMBERED)), expected);
    }

    #[test]
    fn maximized_mode_keeps_the_configured_size_for_unmaximizing() {
        let config = window_config(StartupMode::Maximized);
        let expected = (LogicalSize::new(960, 600), true);
        assert_eq!(initial_window(&config, None), expected);
        assert_eq!(initial_window(&config, Some(REMEMBERED)), expected);
    }

    #[test]
    fn remember_mode_restores_the_last_window() {
        let config = window_config(StartupMode::Remember);
        assert_eq!(
            initial_window(&config, Some(REMEMBERED)),
            (LogicalSize::new(1280, 720), true)
        );
        let unmaximized = WindowState {
            maximized: false,
            ..REMEMBERED
        };
        assert_eq!(
            initial_window(&config, Some(unmaximized)),
            (LogicalSize::new(1280, 720), false)
        );
    }

    #[test]
    fn a_reopened_window_keeps_its_size_whatever_the_startup_mode() {
        let restored = WindowState {
            width: 800,
            height: 500,
            maximized: false,
        };
        for mode in [
            StartupMode::Default,
            StartupMode::Maximized,
            StartupMode::Remember,
        ] {
            let config = window_config(mode);
            assert_eq!(
                launch_window(&config, Some(REMEMBERED), Some(restored)),
                (LogicalSize::new(800, 500), false),
                "{mode:?}"
            );
            assert_eq!(
                launch_window(&config, Some(REMEMBERED), None),
                initial_window(&config, Some(REMEMBERED))
            );
        }
    }

    #[test]
    fn remember_mode_without_state_is_the_default() {
        let config = window_config(StartupMode::Remember);
        assert_eq!(
            initial_window(&config, None),
            (LogicalSize::new(960, 600), false)
        );
    }
}
