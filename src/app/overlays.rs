//! Dialog, command palette and context menus of a session.

use super::*;

impl Session {
    /// The open dialog laid out in the window.
    pub(super) fn dialog_layout(&mut self) -> Option<DialogLayout> {
        let Some(Overlay::Confirm(confirm)) = &self.overlay else {
            return None;
        };
        let size = self.window.inner_size();
        let metrics = self.ui_metrics(UiFont::Chrome);
        let renderer = &mut self.renderer;
        Some(DialogLayout::new(
            confirm,
            self.lang,
            Rect::from_size(size.width, size.height),
            metrics,
            |text| renderer.ui_text_width(UiFont::Chrome, text),
        ))
    }

    /// Applies the user's `answer` to the open dialog and closes it.
    pub(super) fn answer_dialog(&mut self, answer: Answer, event_loop: &ActiveEventLoop) {
        let Some(Overlay::Confirm(confirm)) = self.overlay.take() else {
            return;
        };
        self.window.request_redraw();
        self.update_pointer();
        if answer == Answer::Close {
            self.close(&confirm.target, event_loop);
        }
    }

    /// Opens the command palette over the window, its recent actions read from the
    /// recent file.
    pub(super) fn open_palette(&mut self, default_font_size: f32) {
        let recent = command_palette::recent_path()
            .map(|path| command_palette::load_recent(&path))
            .unwrap_or_default();
        let context = command_palette::Context {
            pane: self.workspace.active_pane().map(Pane::palette_context),
            paste: self
                .clipboard
                .as_mut()
                .and_then(|clipboard| clipboard.get_text().ok())
                .is_some_and(|text| !text.is_empty()),
            font_size: self.font_size,
            default_font_size,
        };
        let entries = command_palette::entries(
            &self.workspace,
            self.geometry(),
            &context,
            self.lang,
            &self.keybindings,
        );
        self.overlay = Some(Overlay::Palette(CommandPalette::new(
            self.lang, entries, recent,
        )));
        self.window.request_redraw();
        self.update_pointer();
    }

    /// The open command palette laid out in the window.
    pub(super) fn palette_layout(&mut self) -> Option<PaletteLayout> {
        let Some(Overlay::Palette(palette)) = &self.overlay else {
            return None;
        };
        let size = self.window.inner_size();
        let metrics = self.ui_metrics(UiFont::Palette);
        let renderer = &mut self.renderer;
        Some(PaletteLayout::new(
            palette,
            self.lang,
            Rect::from_size(size.width, size.height),
            metrics,
            |text| renderer.ui_text_width(UiFont::Palette, text),
        ))
    }

    /// The open context menu laid out in the window.
    pub(super) fn menu_layout(&mut self) -> Option<MenuLayout> {
        let Some(Overlay::Menu(menu)) = &self.overlay else {
            return None;
        };
        let size = self.window.inner_size();
        let metrics = self.ui_metrics(UiFont::Palette);
        let renderer = &mut self.renderer;
        Some(MenuLayout::new(
            menu,
            Rect::from_size(size.width, size.height),
            metrics,
            |text| renderer.ui_text_width(UiFont::Palette, text),
        ))
    }

    /// The open ASCII drawing laid out in the window.
    pub(super) fn art_layout(&mut self) -> Option<ArtLayout> {
        if !matches!(self.overlay, Some(Overlay::Art)) {
            return None;
        }
        let size = self.window.inner_size();
        let metrics = self.ui_metrics(UiFont::Mono);
        let renderer = &mut self.renderer;
        Some(ArtLayout::new(
            Rect::from_size(size.width, size.height),
            metrics,
            |text| renderer.ui_text_width(UiFont::Mono, text),
        ))
    }

    /// Opens the ASCII drawing over the window.
    pub(super) fn open_art(&mut self) {
        self.overlay = Some(Overlay::Art);
        self.window.request_redraw();
        self.update_pointer();
    }

    /// Closes the ASCII drawing.
    pub(super) fn close_art(&mut self) {
        if matches!(self.overlay, Some(Overlay::Art)) {
            self.overlay = None;
            self.window.request_redraw();
            self.update_pointer();
        }
    }

    /// How many themes a menu shows before "More...": as many as the window height holds.
    pub(super) fn theme_rows(&self) -> usize {
        let height = self.window.inner_size().height as f32;
        context_menu::rows_fitting(height, self.ui_metrics(UiFont::Palette))
    }

    /// Opens a pane menu at the pointer, retaining the pane's text selection.
    pub(super) fn open_pane_menu(&mut self, pane_id: PaneId, hit: Hit) {
        let tab_index = self
            .workspace
            .tab_of(pane_id)
            .unwrap_or(self.workspace.active_index());
        let Some(pane) = self.workspace.pane(pane_id) else {
            return;
        };
        let block = pane.block_at(&hit).and_then(|id| {
            let timeline = pane::lock(&pane.terminal.timeline);
            let block = timeline.block(id)?;
            Some(BlockState {
                id,
                command_copyable: crate::block_actions::text(
                    &timeline,
                    id,
                    crate::block_actions::Part::Command,
                )
                .is_some(),
                output_copyable: crate::block_actions::text(
                    &timeline,
                    id,
                    crate::block_actions::Part::Output,
                )
                .is_some(),
                folded: block.folded,
            })
        });
        let items = {
            let tab = self.workspace.tabs().get(tab_index);
            let themes = theme::available(config::config_dir().as_deref());
            let active_theme = self.theme.active(self.appearance).to_owned();
            let theme_rows = self.theme_rows();
            let context = MenuContext {
                copyable: pane.copy_content().is_some(),
                last_pane: self
                    .workspace
                    .tabs()
                    .iter()
                    .map(|tab| tab.tree.leaves().len())
                    .sum::<usize>()
                    == 1,
                single_pane_tab: tab.is_some_and(|tab| tab.is_single()),
                zoomed: tab.is_some_and(|tab| tab.zoomed),
                only_tab: self.workspace.tabs().len() == 1,
                block,
                themes: &themes,
                active_theme: &active_theme,
                theme_rows,
            };
            context_menu::pane_menu(&context, &self.keybindings, self.lang)
        };
        self.open_menu(items);
    }

    /// Opens a tab menu at the pointer without selecting the clicked tab.
    pub(super) fn open_tab_menu(&mut self, index: usize) {
        let Some(tab) = self.workspace.tabs().get(index) else {
            return;
        };
        let themes = theme::available(config::config_dir().as_deref());
        let active_theme = self.theme.active(self.appearance).to_owned();
        let context = MenuContext {
            copyable: false,
            last_pane: self
                .workspace
                .tabs()
                .iter()
                .map(|tab| tab.tree.leaves().len())
                .sum::<usize>()
                == 1,
            single_pane_tab: tab.is_single(),
            zoomed: tab.zoomed,
            only_tab: self.workspace.tabs().len() == 1,
            block: None,
            themes: &themes,
            active_theme: &active_theme,
            theme_rows: self.theme_rows(),
        };
        let items = context_menu::tab_menu(index, &context, &self.keybindings, self.lang);
        self.open_menu(items);
    }

    pub(super) fn open_menu(&mut self, items: Vec<context_menu::Item>) {
        let (x, y) = self.mouse_pixel();
        let active_theme = self.theme.active(self.appearance).to_owned();
        self.overlay = Some(Overlay::Menu(
            ContextMenu::new((x, y), items).with_active_theme(active_theme),
        ));
        self.window.request_redraw();
        self.update_pointer();
    }

    /// Applies a menu outcome, closing the menu before running its command.
    pub(super) fn menu_outcome(
        &mut self,
        outcome: context_menu::Outcome,
        default_font_size: f32,
        event_loop: &ActiveEventLoop,
    ) {
        if !matches!(self.overlay, Some(Overlay::Menu(_))) {
            return;
        }
        self.overlay = None;
        self.window.request_redraw();
        self.update_pointer();
        let context_menu::Outcome::Run(command) = outcome else {
            return;
        };
        match command {
            MenuCommand::Action(action) => self.run_action(action, default_font_size, event_loop),
            MenuCommand::CloseTab(index) => {
                if let Some(target) = Target::tab(&self.workspace, index) {
                    self.request_close(target, event_loop);
                }
            }
            MenuCommand::BlockCopy(id, part) => {
                let text = self
                    .workspace
                    .active_pane()
                    .and_then(|pane| pane.block_text_of(id, part));
                self.set_clipboard(text);
            }
            MenuCommand::BlockFold(id) => {
                if let Some(pane) = self.workspace.active_pane_mut() {
                    pane.toggle_fold_of(id);
                }
            }
            MenuCommand::Theme(name) => {
                if self.write_theme(&name) {
                    // The previewed colors stay until the reload shows the same ones.
                    self.menu_theme = None;
                }
            }
        }
    }

    /// Makes `name` the active theme in the configuration file; returns whether it was
    /// written.
    pub(super) fn write_theme(&mut self, name: &str) -> bool {
        let Some(file) = config::config_file() else {
            let error = ConfigError::read(
                PathBuf::from("config.toml"),
                "cannot resolve the configuration file path".to_owned(),
            );
            self.show_config_error(error);
            return false;
        };
        let key = self.theme.active_key(self.appearance);
        match config_edit::write(
            &file,
            config_edit::default_file_text,
            |edit: &mut ConfigEdit| edit.set(key, name),
        ) {
            Ok(()) => true,
            Err(error) => {
                self.show_config_error(error);
                false
            }
        }
    }

    /// Shows the theme the context menu points at in the terminal, and the colors from
    /// before once it points at no theme or closes.
    pub(super) fn sync_menu_theme(&mut self) {
        let wanted = match &self.overlay {
            Some(Overlay::Menu(menu)) => menu.previewed_theme(),
            _ => None,
        };
        let shown = self.menu_theme.as_ref().map(|theme| theme.name.as_str());
        match menu_theme_step(shown, wanted) {
            MenuThemeStep::Keep => {}
            MenuThemeStep::Show(name) => {
                let name = name.to_owned();
                let (palette, _) = theme::load(&name, config::config_dir().as_deref());
                let saved = self
                    .menu_theme
                    .take()
                    .map_or_else(|| self.palette.clone(), |theme| theme.saved);
                self.preview_palette(&palette);
                self.menu_theme = Some(MenuTheme { name, saved });
            }
            MenuThemeStep::Restore => {
                if let Some(theme) = self.menu_theme.take() {
                    self.preview_palette(&theme.saved);
                }
            }
        }
    }

    pub(super) fn show_config_error(&mut self, error: ConfigError) {
        log::warn!("{error}");
        self.config_banner = Banner::config_errors(std::slice::from_ref(&error));
        self.relayout();
    }

    /// Applies what a key or a click on the open palette asks: closes it, then runs the
    /// chosen entry. An action run is remembered in the recent file.
    pub(super) fn palette_outcome(
        &mut self,
        outcome: Outcome,
        default_font_size: f32,
        event_loop: &ActiveEventLoop,
    ) {
        let Some(Overlay::Palette(palette)) = self.overlay.take() else {
            return;
        };
        self.window.request_redraw();
        self.update_pointer();
        let Outcome::Run(command) = outcome else {
            return;
        };
        match command {
            Command::Action(action) => {
                if let Some(path) = command_palette::recent_path() {
                    let recent = command_palette::remember(palette.recent(), action);
                    if let Err(error) = command_palette::save_recent(&path, &recent) {
                        log::warn!("cannot write {}: {error}", path.display());
                    }
                }
                self.run_action(action, default_font_size, event_loop);
            }
            Command::Tab(index) => self.change_tab(|workspace| workspace.select(index)),
            Command::Pane(id) => self.focus_pane(id),
            Command::Secret(Secret::Answer) => opener::open(secrets::answer_url(self.lang)),
            Command::Secret(Secret::Author) => opener::open(secrets::AUTHOR_URL),
            Command::Secret(Secret::Graphene) => self.open_art(),
        }
    }

    /// Handles a press while an overlay is open: a button of the dialog answers it, a
    /// row of the palette runs it and a press outside the palette closes it, any press
    /// closes the ASCII drawing; anything else is ignored.
    pub(super) fn overlay_press(
        &mut self,
        button: mouse::Button,
        default_font_size: f32,
        event_loop: &ActiveEventLoop,
    ) {
        if matches!(self.overlay, Some(Overlay::Art)) {
            self.close_art();
            return;
        }
        let (x, y) = self.mouse_pixel();
        if matches!(self.overlay, Some(Overlay::Menu(_))) {
            let hit = self
                .menu_layout()
                .map_or(MenuHit::Outside, |layout| layout.hit(x, y));
            let outcome = match &self.overlay {
                Some(Overlay::Menu(menu))
                    if button == mouse::Button::Left || hit == MenuHit::Outside =>
                {
                    menu.choose(hit)
                }
                _ => None,
            };
            if let Some(outcome) = outcome {
                self.menu_outcome(outcome, default_font_size, event_loop);
            }
            return;
        }
        if !matches!(self.overlay, Some(Overlay::Palette(_))) {
            if button == mouse::Button::Left
                && let Some(DialogHit::Button(answer)) =
                    self.dialog_layout().and_then(|layout| layout.hit(x, y))
            {
                self.answer_dialog(answer, event_loop);
            }
            return;
        }
        let outcome = match self.palette_layout().and_then(|layout| layout.hit(x, y)) {
            None => Some(Outcome::Close),
            Some(PaletteHit::Row(index)) if button == mouse::Button::Left => match &self.overlay {
                Some(Overlay::Palette(palette)) => palette.choose(index),
                _ => None,
            },
            Some(_) => None,
        };
        if let Some(outcome) = outcome {
            self.palette_outcome(outcome, default_font_size, event_loop);
        }
    }

    /// Handles a press on the tab bar: a tab is shown and grabbed, its close button or a
    /// middle click closes it, the `+` button opens a tab.
    pub(super) fn tab_bar_press(
        &mut self,
        button: mouse::Button,
        hit: TabBarHit,
        bar: &TabBarLayout,
        event_loop: &ActiveEventLoop,
    ) {
        match (button, hit) {
            (mouse::Button::Left, TabBarHit::Tab(index)) => {
                self.change_tab(|workspace| workspace.select(index));
                let (x, y) = self.mouse_pixel();
                let tab = bar.tabs[index].rect;
                self.drag = Some(Drag::Tab {
                    index,
                    grab_offset: bar.along(x, y) - bar.along(tab.x, tab.y),
                });
            }
            (mouse::Button::Left, TabBarHit::Close(index))
            | (mouse::Button::Middle, TabBarHit::Tab(index) | TabBarHit::Close(index)) => {
                if let Some(target) = Target::tab(&self.workspace, index) {
                    self.request_close(target, event_loop);
                }
            }
            (mouse::Button::Left, TabBarHit::NewTab) => self.new_tab(),
            (mouse::Button::Right, TabBarHit::Tab(index) | TabBarHit::Close(index)) => {
                self.open_tab_menu(index);
            }
            _ => {}
        }
    }
}
