//! UI translations: the language of the user interface and the text of every label.
//!
//! The texts live in one Fluent file per language under `i18n/`, embedded in the
//! binary: `en-US.ftl` is the reference, `fr.ftl` the French translation. [`Msg`] is
//! the API of the code: [`fluent`] maps each variant to its Fluent identifier and
//! arguments through an exhaustive `match`, and the tests check that every identifier
//! exists in every file.

use std::sync::LazyLock;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentError, FluentResource};
use unic_langid::LanguageIdentifier;

use crate::theme::{ColorGroup, ColorKey, NameError};

/// Language of the user interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    En,
    Fr,
}

impl Lang {
    /// Language of the system locale, read from `LC_ALL`, then `LC_MESSAGES`, then
    /// `LANG`. On macOS, an app launched from Finder gets none of them: the first
    /// language preferred in the system settings comes next.
    pub fn detect() -> Self {
        let var = |name| std::env::var(name).ok();
        #[cfg(target_os = "macos")]
        let system = sys_locale::get_locale();
        #[cfg(not(target_os = "macos"))]
        let system: Option<String> = None;
        Self::from_locale_or_system(
            var("LC_ALL").as_deref(),
            var("LC_MESSAGES").as_deref(),
            var("LANG").as_deref(),
            system.as_deref(),
        )
    }

    /// Language of the first non-empty locale value, in POSIX precedence order. The
    /// language is the part before `_` and `.` (`fr_FR.UTF-8` gives `fr`): `fr` gives
    /// French, anything else, or no value at all, English.
    pub fn from_locale(
        lc_all: Option<&str>,
        lc_messages: Option<&str>,
        lang: Option<&str>,
    ) -> Self {
        Self::from_locale_or_system(lc_all, lc_messages, lang, None)
    }

    /// [`Lang::from_locale`], then the `system` language, a BCP 47 tag such as
    /// `fr-FR`, when no variable has a value.
    pub fn from_locale_or_system(
        lc_all: Option<&str>,
        lc_messages: Option<&str>,
        lang: Option<&str>,
        system: Option<&str>,
    ) -> Self {
        let locale = [lc_all, lc_messages, lang, system]
            .into_iter()
            .flatten()
            .find(|value| !value.is_empty());
        let Some(locale) = locale else {
            return Lang::En;
        };
        let language = locale
            .split(['_', '-', '.', '@'])
            .next()
            .unwrap_or_default();
        if language.eq_ignore_ascii_case("fr") {
            Lang::Fr
        } else {
            Lang::En
        }
    }
}

/// Direction of a pane focus or resize action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// What a confirmation dialog closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseTarget {
    Pane,
    Tab,
    Window,
}

/// A section of the settings tab, in sidebar order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SettingsSection {
    #[default]
    Language,
    Font,
    Window,
    Scrollback,
    Cursor,
    Selection,
    Tabs,
    Theme,
    KeyboardShortcuts,
    Notifications,
    QuickTerminal,
    Updates,
}

impl SettingsSection {
    /// Every section, in sidebar order.
    pub const ALL: [SettingsSection; 12] = [
        SettingsSection::Language,
        SettingsSection::Font,
        SettingsSection::Window,
        SettingsSection::Scrollback,
        SettingsSection::Cursor,
        SettingsSection::Selection,
        SettingsSection::Tabs,
        SettingsSection::Theme,
        SettingsSection::KeyboardShortcuts,
        SettingsSection::Notifications,
        SettingsSection::QuickTerminal,
        SettingsSection::Updates,
    ];
}

/// A field of the settings tab, one per setting of the file but the keyboard
/// shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SettingsField {
    Language,
    FontFamily,
    FontSize,
    FontLigatures,
    FontFeatures,
    StartupMode,
    WindowWidth,
    WindowHeight,
    Padding,
    BackgroundOpacity,
    WindowProgress,
    RestoreSession,
    ScrollbackLines,
    CursorShape,
    CursorBlinking,
    WordSeparators,
    TabPosition,
    /// "Follow the system appearance": sets or removes `theme.light` and `theme.dark`.
    ThemeFollow,
    ThemeName,
    ThemeLight,
    ThemeDark,
    UpdatesCheck,
    UpdatesInterval,
    NotificationsProgram,
    NotificationsCommandFinished,
    NotificationsCommandThreshold,
    NotificationsBell,
    QuickTerminalEnabled,
    QuickTerminalHotkey,
    QuickTerminalEdge,
    QuickTerminalSize,
    QuickTerminalScreen,
    QuickTerminalAnimation,
    QuickTerminalHideOnFocusLoss,
    QuickTerminalAutostart,
}

impl SettingsField {
    /// Every field.
    pub const ALL: [SettingsField; 35] = [
        SettingsField::Language,
        SettingsField::FontFamily,
        SettingsField::FontSize,
        SettingsField::FontLigatures,
        SettingsField::FontFeatures,
        SettingsField::StartupMode,
        SettingsField::WindowWidth,
        SettingsField::WindowHeight,
        SettingsField::Padding,
        SettingsField::BackgroundOpacity,
        SettingsField::WindowProgress,
        SettingsField::RestoreSession,
        SettingsField::ScrollbackLines,
        SettingsField::CursorShape,
        SettingsField::CursorBlinking,
        SettingsField::WordSeparators,
        SettingsField::TabPosition,
        SettingsField::ThemeFollow,
        SettingsField::ThemeName,
        SettingsField::ThemeLight,
        SettingsField::ThemeDark,
        SettingsField::UpdatesCheck,
        SettingsField::UpdatesInterval,
        SettingsField::NotificationsProgram,
        SettingsField::NotificationsCommandFinished,
        SettingsField::NotificationsCommandThreshold,
        SettingsField::NotificationsBell,
        SettingsField::QuickTerminalEnabled,
        SettingsField::QuickTerminalHotkey,
        SettingsField::QuickTerminalEdge,
        SettingsField::QuickTerminalSize,
        SettingsField::QuickTerminalScreen,
        SettingsField::QuickTerminalAnimation,
        SettingsField::QuickTerminalHideOnFocusLoss,
        SettingsField::QuickTerminalAutostart,
    ];
}

/// How a program run by `-E` ended, shown by the message of its pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// The program returned this exit code.
    Code(i32),
    /// The program was killed by this signal.
    Signal(i32),
    /// The status could not be read.
    Unknown,
}

impl From<Option<std::process::ExitStatus>> for Exit {
    fn from(status: Option<std::process::ExitStatus>) -> Self {
        use std::os::unix::process::ExitStatusExt;
        match status {
            Some(status) => match (status.code(), status.signal()) {
                (Some(code), _) => Exit::Code(code),
                (None, Some(signal)) => Exit::Signal(signal),
                (None, None) => Exit::Unknown,
            },
            None => Exit::Unknown,
        }
    }
}

/// A translatable label. Messages carrying a value have fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    // Existing terminal actions.
    Copy,
    Paste,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToTop,
    ScrollToBottom,
    FontSizeIncrease,
    FontSizeDecrease,
    FontSizeReset,
    FontSizeResetAll,
    // Command blocks actions.
    PreviousBlock,
    NextBlock,
    CopyCommand,
    CopyOutput,
    ToggleFold,
    ToggleFoldAll,
    /// The action opening the search bar of a pane.
    OpenSearch,
    /// Placeholder of the empty search field.
    SearchPlaceholder,
    /// Counter of the search bar when its query is not a valid regex.
    InvalidRegex,
    /// Zone selector of the search bar: headers and outputs.
    SearchZoneBoth,
    /// Zone selector of the search bar: the commands only.
    SearchZoneCommand,
    /// Zone selector of the search bar: the outputs only.
    SearchZoneOutput,
    /// Status selector of the search bar: every block.
    SearchStatusAll,
    /// Status selector of the search bar: the blocks that succeeded.
    SearchStatusSuccess,
    /// Status selector of the search bar: the blocks that failed.
    SearchStatusFailure,
    // Tabs and panes actions.
    NewTab,
    CloseTab,
    ClosePane,
    NextTab,
    PreviousTab,
    /// 1-based tab number.
    GoToTab(usize),
    SplitRight,
    SplitDown,
    FocusPane(Direction),
    ResizePane(Direction),
    NextPane,
    PreviousPane,
    ToggleZoom,
    ToggleBroadcastAll,
    #[allow(dead_code)] // Used by the context menu; its event integration is issue #103.
    ZoomPane,
    #[allow(dead_code)]
    UnzoomPane,
    #[allow(dead_code)]
    SplitVertical,
    #[allow(dead_code)]
    SplitHorizontal,
    #[allow(dead_code)]
    Themes,
    /// Last row of a theme menu, opening the themes that do not fit in it.
    MoreThemes,
    #[allow(dead_code)]
    FoldBlock,
    #[allow(dead_code)]
    UnfoldBlock,
    CommandPalette,
    EqualisePanes,
    DetachPane,
    /// The rename action, the tab menu entry and the heading of the rename box.
    RenameTab,
    /// The settings action.
    OpenSettings,
    Quit,
    /// Sidebar entry of the settings tab.
    Section(SettingsSection),
    /// Label of a settings field.
    Field(SettingsField),
    /// Font family choice that leaves `font.family` empty.
    SystemMonospace,
    /// Screen choice under the mouse cursor (`quick_terminal.screen = "active"`).
    ActiveScreen,
    /// Primary screen choice (`quick_terminal.screen = "primary"`).
    PrimaryScreen,
    /// Heading of the unknown keys of the file.
    UnknownKeys,
    /// 1-based line of the file.
    AtLine(usize),
    /// Note shown when the file cannot be parsed and the settings are disabled.
    FixFileByHand,
    /// Why the quick terminal section is disabled under Wayland.
    QuickTerminalUnavailableOnWayland,
    /// Why the other quick terminal settings are disabled while it is turned off, and
    /// when turning it on applies.
    QuickTerminalDisabled,
    /// Prefix of an error writing the file.
    CannotSave,
    /// A number field holds text that is not a number.
    NotANumber,
    /// An integer field holds a fractional number.
    NotAWholeNumber,
    /// A number field is below its minimum.
    AtLeast(String),
    /// A number field is above its maximum.
    AtMost(String),
    /// Button setting the shortcut of an empty shortcut field.
    AddShortcut,
    /// Tooltip of the button adding a shortcut to an action.
    AddShortcutHint,
    /// Button capturing a new shortcut in place of the current one.
    ChangeShortcut,
    /// Tooltip of the button removing a shortcut.
    RemoveShortcut,
    /// An action without shortcut.
    NoShortcut,
    /// Shown while the next key press is captured as a shortcut.
    PressShortcut,
    /// A captured shortcut is bound to this other action (its label).
    AlreadyUsedBy(String),
    /// Button binding a captured shortcut over the other action.
    Replace,
    /// Hint of the field filtering the shortcuts list by action name.
    FilterActions,
    /// No action name contains the filter text.
    NoActionMatches,
    /// Button capturing a shortcut to find the actions bound to it.
    SearchShortcut,
    /// No action is bound to the shortcut searched for.
    NoActionForShortcut,
    // Theme editor of the settings tab.
    /// Heading of the theme editor block.
    ThemeEditor,
    /// What the theme editor is for, under its heading.
    ThemeEditorIntro,
    /// How to change a color, above the colors.
    ThemeColorsHint,
    /// A typed color code is not `#rrggbb`.
    InvalidHexColor,
    /// Button opening the user themes folder.
    OpenThemesFolder,
    /// Label of the theme editor's theme selector.
    EditedTheme,
    /// Button copying the edited theme to a new user theme.
    Duplicate,
    /// Button renaming the edited user theme.
    Rename,
    /// Button deleting the edited user theme.
    Delete,
    /// Label of the name field of a copy or a rename.
    NewThemeName,
    /// Button confirming the name of a copy or a rename.
    Confirm,
    /// Confirmation of the deletion of this theme.
    ConfirmDeleteTheme(String),
    /// Hint shown for a built-in theme, which cannot be changed.
    BuiltInTheme,
    /// Checkbox leaving the selection foreground empty.
    KeepTextColor,
    /// Heading of a group of colors.
    ThemeColorGroup(ColorGroup),
    /// Label of a theme color.
    ThemeColor(ColorKey),
    /// Why a theme name is refused.
    ThemeNameError(NameError),
    /// Hint shown in the command palette's empty search field.
    PalettePlaceholder,
    // Secret command palette entries.
    SecretAnswer,
    SecretAuthor,
    SecretGraphene,
    // Command palette entries for open tabs and panes (1-based numbers).
    TabEntry {
        tab: usize,
        title: String,
    },
    PaneEntry {
        tab: usize,
        pane: usize,
        title: String,
    },
    // Close confirmation dialog.
    ConfirmCloseTitle(CloseTarget),
    /// Foreground programs still running, at least one.
    ProgramsRunning(Vec<String>),
    Close,
    Cancel,
    // Update banner.
    /// A newer release, by its tag.
    UpdateAvailable(String),
    OpenRelease,
    IgnoreVersion,
    // Command blocks.
    /// Summary of a folded block: its hidden output rows.
    LinesHidden(usize),
    // Desktop notifications.
    /// Title of the notification of a command that succeeded, or whose status is
    /// unknown.
    CommandFinished,
    /// Title of the notification of a command that failed, by its exit code.
    CommandFailed(i32),
    /// Body of the notification of a bell.
    Bell,
    /// Action of a notification: shows the pane that raised it.
    ShowPane,
    // `-E`.
    /// Shown in the pane once its program ended: how, and that a key closes it.
    ProgramExited(Exit),
}

/// Every language, for tests that must cover each of them.
#[cfg(test)]
pub const LANGS: [Lang; 2] = [Lang::En, Lang::Fr];

impl Lang {
    /// BCP 47 tag of the language, also the name of its file in `i18n/`.
    fn tag(self) -> &'static str {
        match self {
            Lang::En => "en-US",
            Lang::Fr => "fr",
        }
    }

    /// Fluent source of the language, embedded in the binary.
    fn source(self) -> &'static str {
        match self {
            Lang::En => include_str!("../i18n/en-US.ftl"),
            Lang::Fr => include_str!("../i18n/fr.ftl"),
        }
    }
}

type Bundle = FluentBundle<FluentResource>;

/// The bundle of every language, in [`Lang`] order, parsed on first use. The
/// concurrent bundle is `Sync`: notifications are formatted on another thread.
static BUNDLES: LazyLock<[Bundle; 2]> = LazyLock::new(|| [Lang::En, Lang::Fr].map(bundle));

/// Bundle of `lang`, from its embedded file. A file that does not parse panics: the
/// tests parse every file, so it cannot ship.
fn bundle(lang: Lang) -> Bundle {
    let tag = lang.tag();
    let id: LanguageIdentifier = tag.parse().expect("valid language tag");
    let resource = FluentResource::try_new(lang.source().to_owned())
        .unwrap_or_else(|(_, errors)| panic!("i18n/{tag}.ftl: {errors:?}"));
    let mut bundle = FluentBundle::new_concurrent(vec![id]);
    // No RTL in the interface, and the isolation marks would show in the grid font.
    bundle.set_use_isolating(false);
    bundle
        .add_resource(resource)
        .unwrap_or_else(|errors| panic!("i18n/{tag}.ftl: {errors:?}"));
    bundle
}

/// Text of the message `id` with `args` in `bundle`, and the formatting errors.
/// `None` when the bundle has no such message.
fn format(bundle: &Bundle, id: &str, args: &FluentArgs) -> Option<(String, Vec<FluentError>)> {
    let pattern = bundle.get_message(id)?.value()?;
    let mut errors = Vec::new();
    let text = bundle
        .format_pattern(pattern, Some(args), &mut errors)
        .into_owned();
    Some((text, errors))
}

/// Text of `msg` in `lang`.
pub fn text(lang: Lang, msg: &Msg) -> String {
    let (id, args) = fluent(msg);
    match format(&BUNDLES[lang as usize], id, &args) {
        Some((text, errors)) => {
            if !errors.is_empty() {
                log::warn!("i18n: {id} in {}: {errors:?}", lang.tag());
            }
            text
        }
        // The tests check that every message exists in every language.
        None => id.to_owned(),
    }
}

/// Fluent identifier of `msg` and its arguments. Exhaustive: a new [`Msg`] variant
/// fails to compile here until it gets an identifier, and the tests then require it
/// in the file of every language.
fn fluent(msg: &Msg) -> (&'static str, FluentArgs<'_>) {
    let mut args = FluentArgs::new();
    let id = match msg {
        Msg::Copy => "copy",
        Msg::Paste => "paste",
        Msg::ScrollPageUp => "scroll-page-up",
        Msg::ScrollPageDown => "scroll-page-down",
        Msg::ScrollToTop => "scroll-to-top",
        Msg::ScrollToBottom => "scroll-to-bottom",
        Msg::FontSizeIncrease => "font-size-increase",
        Msg::FontSizeDecrease => "font-size-decrease",
        Msg::FontSizeReset => "font-size-reset",
        Msg::FontSizeResetAll => "font-size-reset-all",
        Msg::PreviousBlock => "previous-block",
        Msg::NextBlock => "next-block",
        Msg::CopyCommand => "copy-command",
        Msg::CopyOutput => "copy-output",
        Msg::ToggleFold => "toggle-fold",
        Msg::ToggleFoldAll => "toggle-fold-all",
        Msg::OpenSearch => "open-search",
        Msg::SearchPlaceholder => "search-placeholder",
        Msg::InvalidRegex => "invalid-regex",
        Msg::SearchZoneBoth => "search-zone-both",
        Msg::SearchZoneCommand => "search-zone-command",
        Msg::SearchZoneOutput => "search-zone-output",
        Msg::SearchStatusAll => "search-status-all",
        Msg::SearchStatusSuccess => "search-status-success",
        Msg::SearchStatusFailure => "search-status-failure",
        Msg::NewTab => "new-tab",
        Msg::CloseTab => "close-tab",
        Msg::ClosePane => "close-pane",
        Msg::NextTab => "next-tab",
        Msg::PreviousTab => "previous-tab",
        Msg::GoToTab(tab) => {
            args.set("tab", *tab);
            "go-to-tab"
        }
        Msg::SplitRight => "split-right",
        Msg::SplitDown => "split-down",
        Msg::FocusPane(direction) => {
            args.set("direction", direction_key(*direction));
            "focus-pane"
        }
        Msg::ResizePane(direction) => {
            args.set("direction", direction_key(*direction));
            "resize-pane"
        }
        Msg::NextPane => "next-pane",
        Msg::PreviousPane => "previous-pane",
        Msg::ToggleZoom => "toggle-zoom",
        Msg::ToggleBroadcastAll => "toggle-broadcast-all",
        Msg::ZoomPane => "zoom-pane",
        Msg::UnzoomPane => "unzoom-pane",
        Msg::SplitVertical => "split-vertical",
        Msg::SplitHorizontal => "split-horizontal",
        Msg::Themes => "themes",
        Msg::MoreThemes => "more-themes",
        Msg::FoldBlock => "fold-block",
        Msg::UnfoldBlock => "unfold-block",
        Msg::CommandPalette => "command-palette",
        Msg::EqualisePanes => "equalise-panes",
        Msg::DetachPane => "detach-pane",
        Msg::RenameTab => "rename-tab",
        Msg::OpenSettings => "open-settings",
        Msg::Quit => "quit",
        Msg::Section(section) => {
            args.set("section", section_key(*section));
            "settings-section"
        }
        Msg::Field(field) => {
            args.set("field", field_key(*field));
            "settings-field"
        }
        Msg::SystemMonospace => "system-monospace",
        Msg::ActiveScreen => "active-screen",
        Msg::PrimaryScreen => "primary-screen",
        Msg::UnknownKeys => "unknown-keys",
        Msg::AtLine(line) => {
            args.set("line", *line);
            "at-line"
        }
        Msg::FixFileByHand => "fix-file-by-hand",
        Msg::QuickTerminalUnavailableOnWayland => "quick-terminal-unavailable-on-wayland",
        Msg::QuickTerminalDisabled => "quick-terminal-disabled",
        Msg::CannotSave => "cannot-save",
        Msg::NotANumber => "not-a-number",
        Msg::NotAWholeNumber => "not-a-whole-number",
        Msg::AtLeast(min) => {
            args.set("min", min.as_str());
            "at-least"
        }
        Msg::AtMost(max) => {
            args.set("max", max.as_str());
            "at-most"
        }
        Msg::AddShortcut => "add-shortcut",
        Msg::AddShortcutHint => "add-shortcut-hint",
        Msg::ChangeShortcut => "change-shortcut",
        Msg::RemoveShortcut => "remove-shortcut",
        Msg::NoShortcut => "no-shortcut",
        Msg::PressShortcut => "press-shortcut",
        Msg::AlreadyUsedBy(action) => {
            args.set("action", action.as_str());
            "already-used-by"
        }
        Msg::Replace => "replace",
        Msg::FilterActions => "filter-actions",
        Msg::NoActionMatches => "no-action-matches",
        Msg::SearchShortcut => "search-shortcut",
        Msg::NoActionForShortcut => "no-action-for-shortcut",
        Msg::ThemeEditor => "theme-editor",
        Msg::ThemeEditorIntro => "theme-editor-intro",
        Msg::ThemeColorsHint => "theme-colors-hint",
        Msg::InvalidHexColor => "invalid-hex-color",
        Msg::OpenThemesFolder => "open-themes-folder",
        Msg::EditedTheme => "edited-theme",
        Msg::Duplicate => "duplicate",
        Msg::Rename => "rename",
        Msg::Delete => "delete",
        Msg::NewThemeName => "new-theme-name",
        Msg::Confirm => "confirm",
        Msg::ConfirmDeleteTheme(name) => {
            args.set("name", name.as_str());
            "confirm-delete-theme"
        }
        Msg::BuiltInTheme => "built-in-theme",
        Msg::KeepTextColor => "keep-text-color",
        Msg::ThemeColorGroup(group) => {
            args.set("group", color_group_key(*group));
            "theme-color-group"
        }
        Msg::ThemeColor(key) => {
            args.set("key", color_key(*key));
            "theme-color"
        }
        Msg::ThemeNameError(error) => {
            args.set("error", name_error_key(*error));
            "theme-name-error"
        }
        Msg::PalettePlaceholder => "palette-placeholder",
        Msg::SecretAnswer => "secret-answer",
        Msg::SecretAuthor => "secret-author",
        Msg::SecretGraphene => "secret-graphene",
        Msg::TabEntry { tab, title } => {
            args.set("tab", *tab);
            args.set("title", title.as_str());
            "tab-entry"
        }
        Msg::PaneEntry { tab, pane, title } => {
            args.set("tab", *tab);
            args.set("pane", *pane);
            args.set("title", title.as_str());
            "pane-entry"
        }
        Msg::ConfirmCloseTitle(target) => {
            args.set("target", close_target_key(*target));
            "confirm-close-title"
        }
        Msg::ProgramsRunning(programs) => {
            args.set("count", programs.len());
            args.set("programs", programs.join(", "));
            "programs-running"
        }
        Msg::Close => "close",
        Msg::Cancel => "cancel",
        Msg::UpdateAvailable(tag) => {
            args.set("tag", tag.as_str());
            "update-available"
        }
        Msg::OpenRelease => "open-release",
        Msg::IgnoreVersion => "ignore-version",
        Msg::LinesHidden(rows) => {
            args.set("rows", *rows);
            "lines-hidden"
        }
        Msg::CommandFinished => "command-finished",
        Msg::CommandFailed(code) => {
            args.set("code", *code);
            "command-failed"
        }
        Msg::Bell => "bell",
        Msg::ShowPane => "show-pane",
        Msg::ProgramExited(exit) => {
            let (kind, value) = match exit {
                Exit::Code(code) => ("code", Some(*code)),
                Exit::Signal(signal) => ("signal", Some(*signal)),
                Exit::Unknown => ("unknown", None),
            };
            args.set("exit", kind);
            if let Some(value) = value {
                args.set("value", value);
            }
            "program-exited"
        }
    };
    (id, args)
}

/// Selector key of a direction in the Fluent files.
fn direction_key(direction: Direction) -> &'static str {
    match direction {
        Direction::Left => "left",
        Direction::Right => "right",
        Direction::Up => "up",
        Direction::Down => "down",
    }
}

/// Selector key of a close target in the Fluent files.
fn close_target_key(target: CloseTarget) -> &'static str {
    match target {
        CloseTarget::Pane => "pane",
        CloseTarget::Tab => "tab",
        CloseTarget::Window => "window",
    }
}

/// Selector key of a settings section in the Fluent files.
fn section_key(section: SettingsSection) -> &'static str {
    match section {
        SettingsSection::Language => "language",
        SettingsSection::Font => "font",
        SettingsSection::Window => "window",
        SettingsSection::Scrollback => "scrollback",
        SettingsSection::Cursor => "cursor",
        SettingsSection::Selection => "selection",
        SettingsSection::Tabs => "tabs",
        SettingsSection::Theme => "theme",
        SettingsSection::KeyboardShortcuts => "keyboard-shortcuts",
        SettingsSection::Notifications => "notifications",
        SettingsSection::QuickTerminal => "quick-terminal",
        SettingsSection::Updates => "updates",
    }
}

/// Selector key of a settings field in the Fluent files.
fn field_key(field: SettingsField) -> &'static str {
    match field {
        SettingsField::Language => "language",
        SettingsField::FontFamily => "font-family",
        SettingsField::FontSize => "font-size",
        SettingsField::FontLigatures => "font-ligatures",
        SettingsField::FontFeatures => "font-features",
        SettingsField::StartupMode => "startup-mode",
        SettingsField::WindowWidth => "window-width",
        SettingsField::WindowHeight => "window-height",
        SettingsField::Padding => "padding",
        SettingsField::BackgroundOpacity => "background-opacity",
        SettingsField::WindowProgress => "window-progress",
        SettingsField::RestoreSession => "restore-session",
        SettingsField::ScrollbackLines => "scrollback-lines",
        SettingsField::CursorShape => "cursor-shape",
        SettingsField::CursorBlinking => "cursor-blinking",
        SettingsField::WordSeparators => "word-separators",
        SettingsField::TabPosition => "tab-position",
        SettingsField::ThemeFollow => "theme-follow",
        SettingsField::ThemeName => "theme-name",
        SettingsField::ThemeLight => "theme-light",
        SettingsField::ThemeDark => "theme-dark",
        SettingsField::UpdatesCheck => "updates-check",
        SettingsField::UpdatesInterval => "updates-interval",
        SettingsField::NotificationsProgram => "notifications-program",
        SettingsField::NotificationsCommandFinished => "notifications-command-finished",
        SettingsField::NotificationsCommandThreshold => "notifications-command-threshold",
        SettingsField::NotificationsBell => "notifications-bell",
        SettingsField::QuickTerminalEnabled => "quick-terminal-enabled",
        SettingsField::QuickTerminalHotkey => "quick-terminal-hotkey",
        SettingsField::QuickTerminalEdge => "quick-terminal-edge",
        SettingsField::QuickTerminalSize => "quick-terminal-size",
        SettingsField::QuickTerminalScreen => "quick-terminal-screen",
        SettingsField::QuickTerminalAnimation => "quick-terminal-animation",
        SettingsField::QuickTerminalHideOnFocusLoss => "quick-terminal-hide-on-focus-loss",
        SettingsField::QuickTerminalAutostart => "quick-terminal-autostart",
    }
}

/// Selector key of a group of theme colors in the Fluent files.
fn color_group_key(group: ColorGroup) -> &'static str {
    match group {
        ColorGroup::Terminal => "terminal",
        ColorGroup::Normal => "normal",
        ColorGroup::Bright => "bright",
        ColorGroup::Interface => "interface",
    }
}

/// Selector key of a theme color in the Fluent files.
fn color_key(key: ColorKey) -> &'static str {
    match key {
        ColorKey::Foreground => "foreground",
        ColorKey::Background => "background",
        ColorKey::Cursor => "cursor",
        ColorKey::CursorText => "cursor-text",
        ColorKey::SelectionBackground => "selection-background",
        ColorKey::SelectionForeground => "selection-foreground",
        ColorKey::NormalBlack => "normal-black",
        ColorKey::NormalRed => "normal-red",
        ColorKey::NormalGreen => "normal-green",
        ColorKey::NormalYellow => "normal-yellow",
        ColorKey::NormalBlue => "normal-blue",
        ColorKey::NormalMagenta => "normal-magenta",
        ColorKey::NormalCyan => "normal-cyan",
        ColorKey::NormalWhite => "normal-white",
        ColorKey::BrightBlack => "bright-black",
        ColorKey::BrightRed => "bright-red",
        ColorKey::BrightGreen => "bright-green",
        ColorKey::BrightYellow => "bright-yellow",
        ColorKey::BrightBlue => "bright-blue",
        ColorKey::BrightMagenta => "bright-magenta",
        ColorKey::BrightCyan => "bright-cyan",
        ColorKey::BrightWhite => "bright-white",
        ColorKey::TabBarBackground => "tab-bar-background",
        ColorKey::TabActiveBackground => "tab-active-background",
        ColorKey::TabActiveText => "tab-active-text",
        ColorKey::TabInactiveText => "tab-inactive-text",
        ColorKey::PaneBorder => "pane-border",
        ColorKey::PaneBroadcast => "pane-broadcast",
        ColorKey::PaletteBackground => "palette-background",
        ColorKey::PaletteText => "palette-text",
        ColorKey::PaletteMatch => "palette-match",
        ColorKey::PaletteSelectionBackground => "palette-selection-background",
        ColorKey::BannerBackground => "banner-background",
        ColorKey::BannerText => "banner-text",
        ColorKey::SearchMatchBackground => "search-match-background",
        ColorKey::SearchCurrentBackground => "search-current-background",
    }
}

/// Selector key of a theme name error in the Fluent files.
fn name_error_key(error: NameError) -> &'static str {
    match error {
        NameError::Empty => "empty",
        NameError::Invalid => "invalid",
        NameError::Taken => "taken",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIRECTIONS: [Direction; 4] = [
        Direction::Left,
        Direction::Right,
        Direction::Up,
        Direction::Down,
    ];
    const TARGETS: [CloseTarget; 3] = [CloseTarget::Pane, CloseTarget::Tab, CloseTarget::Window];

    /// Index of the variant, through an exhaustive match: a new variant fails to compile
    /// here until it gets an index, and `all_messages` must then cover it.
    fn variant_index(msg: &Msg) -> usize {
        match msg {
            Msg::Copy => 0,
            Msg::Paste => 1,
            Msg::ScrollPageUp => 2,
            Msg::ScrollPageDown => 3,
            Msg::ScrollToTop => 4,
            Msg::ScrollToBottom => 5,
            Msg::FontSizeIncrease => 6,
            Msg::FontSizeDecrease => 7,
            Msg::FontSizeReset => 8,
            Msg::FontSizeResetAll => 103,
            Msg::PreviousBlock => 9,
            Msg::NextBlock => 10,
            Msg::CopyCommand => 11,
            Msg::CopyOutput => 12,
            Msg::ToggleFold => 13,
            Msg::ToggleFoldAll => 14,
            Msg::NewTab => 15,
            Msg::ClosePane => 16,
            Msg::NextTab => 17,
            Msg::PreviousTab => 18,
            Msg::GoToTab(_) => 19,
            Msg::SplitRight => 20,
            Msg::SplitDown => 21,
            Msg::FocusPane(_) => 22,
            Msg::ResizePane(_) => 23,
            Msg::NextPane => 24,
            Msg::PreviousPane => 25,
            Msg::ToggleZoom => 26,
            Msg::ToggleBroadcastAll => 68,
            Msg::ZoomPane => 61,
            Msg::UnzoomPane => 62,
            Msg::SplitVertical => 63,
            Msg::SplitHorizontal => 64,
            Msg::Themes => 65,
            Msg::MoreThemes => 86,
            Msg::FoldBlock => 66,
            Msg::UnfoldBlock => 67,
            Msg::CommandPalette => 27,
            Msg::EqualisePanes => 28,
            Msg::DetachPane => 29,
            Msg::RenameTab => 100,
            Msg::TabEntry { .. } => 30,
            Msg::PalettePlaceholder => 40,
            Msg::PaneEntry { .. } => 31,
            Msg::ConfirmCloseTitle(_) => 32,
            Msg::ProgramsRunning(_) => 33,
            Msg::Close => 34,
            Msg::Cancel => 35,
            Msg::UpdateAvailable(_) => 36,
            Msg::OpenRelease => 37,
            Msg::IgnoreVersion => 38,
            Msg::LinesHidden(_) => 39,
            Msg::OpenSettings => 41,
            Msg::CloseTab => 59,
            Msg::Quit => 60,
            Msg::Section(_) => 42,
            Msg::Field(_) => 43,
            Msg::SystemMonospace => 44,
            Msg::UnknownKeys => 45,
            Msg::AtLine(_) => 46,
            Msg::FixFileByHand => 47,
            Msg::CannotSave => 48,
            Msg::NotANumber => 49,
            Msg::NotAWholeNumber => 50,
            Msg::AtLeast(_) => 51,
            Msg::AtMost(_) => 52,
            Msg::AddShortcut => 53,
            Msg::AddShortcutHint => 104,
            Msg::ChangeShortcut => 99,
            Msg::RemoveShortcut => 54,
            Msg::NoShortcut => 55,
            Msg::PressShortcut => 56,
            Msg::AlreadyUsedBy(_) => 57,
            Msg::Replace => 58,
            Msg::ThemeEditor => 69,
            Msg::EditedTheme => 70,
            Msg::Duplicate => 71,
            Msg::Rename => 72,
            Msg::Delete => 73,
            Msg::NewThemeName => 74,
            Msg::Confirm => 75,
            Msg::ConfirmDeleteTheme(_) => 76,
            Msg::BuiltInTheme => 77,
            Msg::KeepTextColor => 78,
            Msg::ThemeColorGroup(_) => 79,
            Msg::ThemeColor(_) => 80,
            Msg::ThemeNameError(_) => 81,
            Msg::ThemeEditorIntro => 82,
            Msg::ThemeColorsHint => 83,
            Msg::InvalidHexColor => 84,
            Msg::OpenThemesFolder => 85,
            Msg::SecretAnswer => 87,
            Msg::SecretAuthor => 88,
            Msg::SecretGraphene => 89,
            Msg::FilterActions => 90,
            Msg::NoActionMatches => 91,
            Msg::SearchShortcut => 92,
            Msg::NoActionForShortcut => 93,
            Msg::CommandFinished => 94,
            Msg::CommandFailed(_) => 95,
            Msg::Bell => 96,
            Msg::ShowPane => 97,
            Msg::QuickTerminalUnavailableOnWayland => 98,
            Msg::ActiveScreen => 101,
            Msg::PrimaryScreen => 102,
            Msg::ProgramExited(_) => 103,
            Msg::OpenSearch => 105,
            Msg::SearchPlaceholder => 106,
            Msg::InvalidRegex => 107,
            Msg::SearchZoneBoth => 108,
            Msg::SearchZoneCommand => 109,
            Msg::SearchZoneOutput => 110,
            Msg::SearchStatusAll => 111,
            Msg::SearchStatusSuccess => 112,
            Msg::SearchStatusFailure => 113,
            Msg::QuickTerminalDisabled => 114,
        }
    }
    const VARIANT_COUNT: usize = 115;

    /// Every variant, with every value of its enum fields.
    fn all_messages() -> Vec<Msg> {
        let mut messages = vec![
            Msg::Copy,
            Msg::Paste,
            Msg::ScrollPageUp,
            Msg::ScrollPageDown,
            Msg::ScrollToTop,
            Msg::ScrollToBottom,
            Msg::FontSizeIncrease,
            Msg::FontSizeDecrease,
            Msg::FontSizeReset,
            Msg::FontSizeResetAll,
            Msg::PreviousBlock,
            Msg::NextBlock,
            Msg::CopyCommand,
            Msg::CopyOutput,
            Msg::ToggleFold,
            Msg::ToggleFoldAll,
            Msg::OpenSearch,
            Msg::SearchPlaceholder,
            Msg::InvalidRegex,
            Msg::SearchZoneBoth,
            Msg::SearchZoneCommand,
            Msg::SearchZoneOutput,
            Msg::SearchStatusAll,
            Msg::SearchStatusSuccess,
            Msg::SearchStatusFailure,
            Msg::NewTab,
            Msg::CloseTab,
            Msg::ClosePane,
            Msg::NextTab,
            Msg::PreviousTab,
            Msg::GoToTab(3),
            Msg::SplitRight,
            Msg::SplitDown,
            Msg::NextPane,
            Msg::PreviousPane,
            Msg::ToggleZoom,
            Msg::ToggleBroadcastAll,
            Msg::ZoomPane,
            Msg::UnzoomPane,
            Msg::SplitVertical,
            Msg::SplitHorizontal,
            Msg::Themes,
            Msg::MoreThemes,
            Msg::FoldBlock,
            Msg::UnfoldBlock,
            Msg::CommandPalette,
            Msg::EqualisePanes,
            Msg::DetachPane,
            Msg::RenameTab,
            Msg::OpenSettings,
            Msg::Quit,
            Msg::PalettePlaceholder,
            Msg::SecretAnswer,
            Msg::SecretAuthor,
            Msg::SecretGraphene,
            Msg::TabEntry {
                tab: 2,
                title: "~/dev".into(),
            },
            Msg::PaneEntry {
                tab: 2,
                pane: 3,
                title: "vim".into(),
            },
            Msg::ProgramsRunning(vec!["vim".into()]),
            Msg::ProgramsRunning(vec!["vim".into(), "cargo".into()]),
            Msg::Close,
            Msg::Cancel,
            Msg::UpdateAvailable("v0.2.0".into()),
            Msg::OpenRelease,
            Msg::IgnoreVersion,
            Msg::LinesHidden(1),
            Msg::LinesHidden(42),
            Msg::SystemMonospace,
            Msg::ActiveScreen,
            Msg::PrimaryScreen,
            Msg::UnknownKeys,
            Msg::AtLine(12),
            Msg::FixFileByHand,
            Msg::QuickTerminalUnavailableOnWayland,
            Msg::QuickTerminalDisabled,
            Msg::CannotSave,
            Msg::NotANumber,
            Msg::NotAWholeNumber,
            Msg::AtLeast("6".into()),
            Msg::AtMost("72".into()),
            Msg::AddShortcut,
            Msg::AddShortcutHint,
            Msg::ChangeShortcut,
            Msg::RemoveShortcut,
            Msg::NoShortcut,
            Msg::PressShortcut,
            Msg::AlreadyUsedBy("Copy".into()),
            Msg::Replace,
            Msg::FilterActions,
            Msg::NoActionMatches,
            Msg::SearchShortcut,
            Msg::NoActionForShortcut,
            Msg::ThemeEditor,
            Msg::ThemeEditorIntro,
            Msg::ThemeColorsHint,
            Msg::InvalidHexColor,
            Msg::OpenThemesFolder,
            Msg::EditedTheme,
            Msg::Duplicate,
            Msg::Rename,
            Msg::Delete,
            Msg::NewThemeName,
            Msg::Confirm,
            Msg::ConfirmDeleteTheme("my-theme".into()),
            Msg::BuiltInTheme,
            Msg::KeepTextColor,
            Msg::CommandFinished,
            Msg::CommandFailed(2),
            Msg::Bell,
            Msg::ShowPane,
            Msg::ProgramExited(Exit::Code(1)),
            Msg::ProgramExited(Exit::Signal(9)),
            Msg::ProgramExited(Exit::Unknown),
        ];
        messages.extend(DIRECTIONS.map(Msg::FocusPane));
        messages.extend(DIRECTIONS.map(Msg::ResizePane));
        messages.extend(TARGETS.map(Msg::ConfirmCloseTitle));
        messages.extend(SettingsSection::ALL.map(Msg::Section));
        messages.extend(SettingsField::ALL.map(Msg::Field));
        messages.extend(ColorGroup::ALL.map(Msg::ThemeColorGroup));
        messages.extend(ColorKey::ALL.map(Msg::ThemeColor));
        messages.extend(
            [NameError::Empty, NameError::Invalid, NameError::Taken].map(Msg::ThemeNameError),
        );
        messages
    }

    #[test]
    fn program_exited_texts_tell_how_and_that_a_key_closes() {
        assert_eq!(
            text(Lang::En, &Msg::ProgramExited(Exit::Code(0))),
            "Program ended (code 0). Press any key to close."
        );
        assert_eq!(
            text(Lang::En, &Msg::ProgramExited(Exit::Signal(9))),
            "Program ended (signal 9). Press any key to close."
        );
        assert_eq!(
            text(Lang::En, &Msg::ProgramExited(Exit::Unknown)),
            "Program ended. Press any key to close."
        );
        assert_eq!(
            text(Lang::Fr, &Msg::ProgramExited(Exit::Code(2))),
            "Programme terminé (code 2). Appuyez sur une touche pour fermer."
        );
        assert_eq!(
            text(Lang::Fr, &Msg::ProgramExited(Exit::Signal(15))),
            "Programme terminé (signal 15). Appuyez sur une touche pour fermer."
        );
        assert_eq!(
            text(Lang::Fr, &Msg::ProgramExited(Exit::Unknown)),
            "Programme terminé. Appuyez sur une touche pour fermer."
        );
    }

    #[test]
    fn exit_reads_the_code_then_the_signal_of_the_status() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::ExitStatus;
        // Raw wait statuses: the code in the second byte, the signal in the first.
        assert_eq!(Exit::from(Some(ExitStatus::from_raw(0))), Exit::Code(0));
        assert_eq!(
            Exit::from(Some(ExitStatus::from_raw(3 << 8))),
            Exit::Code(3)
        );
        assert_eq!(Exit::from(Some(ExitStatus::from_raw(9))), Exit::Signal(9));
        assert_eq!(Exit::from(None), Exit::Unknown);
    }

    #[test]
    fn all_messages_covers_every_variant() {
        let mut covered = [false; VARIANT_COUNT];
        for msg in all_messages() {
            covered[variant_index(&msg)] = true;
        }
        assert!(covered.iter().all(|&c| c), "missing variants: {covered:?}");
    }

    #[test]
    fn every_message_has_a_text_in_every_language() {
        for lang in LANGS {
            for msg in all_messages() {
                assert!(!text(lang, &msg).trim().is_empty(), "{lang:?} {msg:?}");
            }
        }
    }

    /// Identifiers of the messages defined in the Fluent file of `lang`.
    fn message_ids(lang: Lang) -> Vec<&'static str> {
        let resource = fluent_syntax::parser::parse(lang.source())
            .unwrap_or_else(|(_, errors)| panic!("{lang:?}: {errors:?}"));
        resource
            .body
            .into_iter()
            .filter_map(|entry| match entry {
                fluent_syntax::ast::Entry::Message(message) => Some(message.id.name),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_message_is_in_the_file_of_every_language_and_formats_without_error() {
        for lang in LANGS {
            // A bundle of this file alone: a message missing from it is not hidden by
            // the bundle of another language.
            let bundle = bundle(lang);
            for msg in all_messages() {
                let (id, args) = fluent(&msg);
                let (text, errors) = format(&bundle, id, &args)
                    .unwrap_or_else(|| panic!("{id} missing from i18n/{}.ftl", lang.tag()));
                assert!(errors.is_empty(), "{lang:?} {msg:?}: {errors:?}");
                assert!(!text.trim().is_empty(), "{lang:?} {msg:?}");
            }
        }
    }

    #[test]
    fn no_file_defines_an_identifier_unknown_to_the_reference() {
        let reference = message_ids(Lang::En);
        for lang in LANGS {
            for id in message_ids(lang) {
                assert!(
                    reference.contains(&id),
                    "{id} of {lang:?} is not in en-US.ftl"
                );
            }
        }
    }

    #[test]
    fn every_identifier_of_the_reference_is_used_by_a_message() {
        let used: Vec<_> = all_messages().iter().map(|msg| fluent(msg).0).collect();
        for id in message_ids(Lang::En) {
            assert!(used.contains(&id), "{id} of en-US.ftl has no Msg");
        }
    }

    #[test]
    fn every_file_starts_with_its_review_status() {
        for lang in LANGS {
            let first = lang.source().lines().next();
            assert!(
                matches!(
                    first,
                    Some("## Status: reviewed") | Some("## Status: unreviewed")
                ),
                "{lang:?}: {first:?}"
            );
        }
    }

    #[test]
    fn every_selector_value_has_its_own_text() {
        // A selector key missing from a file falls back to the default variant
        // silently: distinct texts show that each value has its own.
        let groups: [Vec<Msg>; 7] = [
            DIRECTIONS.map(Msg::FocusPane).to_vec(),
            DIRECTIONS.map(Msg::ResizePane).to_vec(),
            TARGETS.map(Msg::ConfirmCloseTitle).to_vec(),
            SettingsField::ALL.map(Msg::Field).to_vec(),
            ColorGroup::ALL.map(Msg::ThemeColorGroup).to_vec(),
            [NameError::Empty, NameError::Invalid, NameError::Taken]
                .map(Msg::ThemeNameError)
                .to_vec(),
            [Exit::Code(1), Exit::Signal(1), Exit::Unknown]
                .map(Msg::ProgramExited)
                .to_vec(),
        ];
        for lang in LANGS {
            for group in &groups {
                let mut texts: Vec<_> = group.iter().map(|msg| text(lang, msg)).collect();
                texts.sort();
                texts.dedup();
                assert_eq!(texts.len(), group.len(), "{lang:?} {group:?}");
            }
        }
    }

    #[test]
    fn plurals_follow_the_rules_of_the_language() {
        // French takes the singular for 0, English the plural.
        assert_eq!(text(Lang::En, &Msg::LinesHidden(0)), "0 lines hidden");
        assert_eq!(text(Lang::Fr, &Msg::LinesHidden(0)), "0 ligne masquée");
        assert_eq!(
            text(Lang::Fr, &Msg::ProgramsRunning(vec!["vim".into()])),
            "vim est toujours en cours d'exécution"
        );
        // No digit grouping and no isolation marks around the values.
        assert_eq!(text(Lang::En, &Msg::GoToTab(12345)), "Go to tab 12345");
    }

    #[test]
    fn texts_differ_between_languages() {
        // Names that are the same in every language.
        let same = [
            Msg::SecretGraphene,
            Msg::Section(SettingsSection::Notifications),
            Msg::Section(SettingsSection::QuickTerminal),
            Msg::Field(SettingsField::FontLigatures),
        ];
        for msg in all_messages().into_iter().filter(|msg| !same.contains(msg)) {
            assert_ne!(text(Lang::En, &msg), text(Lang::Fr, &msg), "{msg:?}");
        }
    }

    #[test]
    fn messages_with_values_include_them() {
        assert_eq!(text(Lang::En, &Msg::GoToTab(4)), "Go to tab 4");
        assert_eq!(
            text(Lang::En, &Msg::AlreadyUsedBy("New tab".into())),
            "Already used by New tab"
        );
        assert_eq!(
            text(Lang::Fr, &Msg::AlreadyUsedBy("Nouvel onglet".into())),
            "Déjà utilisé par Nouvel onglet"
        );
        assert_eq!(text(Lang::En, &Msg::LinesHidden(1)), "1 line hidden");
        assert_eq!(text(Lang::En, &Msg::LinesHidden(12)), "12 lines hidden");
        assert_eq!(text(Lang::Fr, &Msg::LinesHidden(1)), "1 ligne masquée");
        assert_eq!(text(Lang::Fr, &Msg::LinesHidden(12)), "12 lignes masquées");
        assert_eq!(
            text(Lang::En, &Msg::CommandFailed(2)),
            "Command failed (code 2)"
        );
        assert_eq!(
            text(Lang::Fr, &Msg::CommandFailed(127)),
            "Commande échouée (code 127)"
        );
        assert_eq!(
            text(
                Lang::Fr,
                &Msg::PaneEntry {
                    tab: 2,
                    pane: 3,
                    title: "vim".into()
                }
            ),
            "Onglet 2 > panneau 3 : vim"
        );
        assert_eq!(
            text(Lang::En, &Msg::ProgramsRunning(vec!["vim".into()])),
            "vim is still running"
        );
        assert_eq!(
            text(
                Lang::En,
                &Msg::ProgramsRunning(vec!["vim".into(), "cargo".into()])
            ),
            "2 programs are still running: vim, cargo"
        );
        assert_eq!(
            text(Lang::Fr, &Msg::UpdateAvailable("v0.2.0".into())),
            "Graphene v0.2.0 est disponible"
        );
    }

    #[test]
    fn settings_texts() {
        assert_eq!(text(Lang::En, &Msg::OpenSettings), "Settings");
        assert_eq!(text(Lang::Fr, &Msg::OpenSettings), "Paramètres");
        assert_eq!(
            text(Lang::En, &Msg::Section(SettingsSection::KeyboardShortcuts)),
            "Keyboard shortcuts"
        );
        assert_eq!(
            text(Lang::Fr, &Msg::Section(SettingsSection::Updates)),
            "Mises à jour"
        );
        // Sidebar entries are distinct within a language.
        for lang in LANGS {
            let mut labels: Vec<_> = SettingsSection::ALL
                .map(|section| text(lang, &Msg::Section(section)))
                .to_vec();
            labels.sort();
            labels.dedup();
            assert_eq!(labels.len(), SettingsSection::ALL.len(), "{lang:?}");
        }
        assert_eq!(text(Lang::En, &Msg::AddShortcutHint), "Add shortcut");
        assert_eq!(
            text(Lang::Fr, &Msg::AddShortcutHint),
            "Ajouter un raccourci"
        );
        assert_eq!(text(Lang::En, &Msg::AtLine(12)), "line 12");
        assert_eq!(text(Lang::Fr, &Msg::AtLine(12)), "ligne 12");
        assert_eq!(
            text(Lang::En, &Msg::AtLeast("6".into())),
            "Must be at least 6"
        );
        assert_eq!(
            text(Lang::Fr, &Msg::AtMost("72".into())),
            "Doit valoir au plus 72"
        );
        assert_eq!(
            text(Lang::Fr, &Msg::Field(SettingsField::ThemeFollow)),
            "Suivre l'apparence du système"
        );
    }

    #[test]
    fn theme_color_labels_are_distinct_within_a_language() {
        for lang in LANGS {
            let mut labels: Vec<_> = ColorKey::ALL
                .map(|key| text(lang, &Msg::ThemeColor(key)))
                .to_vec();
            labels.sort();
            labels.dedup();
            assert_eq!(labels.len(), ColorKey::ALL.len(), "{lang:?}");
        }
        assert_eq!(
            text(Lang::Fr, &Msg::ConfirmDeleteTheme("mine".into())),
            "Supprimer le thème mine ? Son fichier est effacé."
        );
        assert_eq!(
            text(Lang::En, &Msg::ThemeNameError(NameError::Invalid)),
            "A name cannot contain / or \\ nor start with a dot"
        );
    }

    #[test]
    fn tab_and_quit_action_texts() {
        assert_eq!(text(Lang::En, &Msg::CloseTab), "Close the tab");
        assert_eq!(text(Lang::Fr, &Msg::CloseTab), "Fermer l'onglet");
        assert_eq!(text(Lang::En, &Msg::Quit), "Quit Graphene");
        assert_eq!(text(Lang::Fr, &Msg::Quit), "Quitter Graphene");
    }

    #[test]
    fn detect_takes_the_language_before_territory_and_encoding() {
        assert_eq!(Lang::from_locale(None, None, Some("fr_FR.UTF-8")), Lang::Fr);
        assert_eq!(Lang::from_locale(None, None, Some("fr")), Lang::Fr);
        assert_eq!(Lang::from_locale(None, None, Some("fr.UTF-8")), Lang::Fr);
        assert_eq!(Lang::from_locale(None, None, Some("fr_BE@euro")), Lang::Fr);
        assert_eq!(Lang::from_locale(None, None, Some("en_US.UTF-8")), Lang::En);
    }

    #[test]
    fn detect_precedence_is_lc_all_then_lc_messages_then_lang() {
        let fr = Some("fr_FR.UTF-8");
        let de = Some("de_DE.UTF-8");
        assert_eq!(Lang::from_locale(fr, de, de), Lang::Fr);
        assert_eq!(Lang::from_locale(de, fr, fr), Lang::En);
        assert_eq!(Lang::from_locale(None, fr, de), Lang::Fr);
        assert_eq!(Lang::from_locale(None, de, fr), Lang::En);
        assert_eq!(Lang::from_locale(None, None, fr), Lang::Fr);
    }

    #[test]
    fn detect_skips_empty_values() {
        let fr = Some("fr_FR.UTF-8");
        assert_eq!(Lang::from_locale(Some(""), Some(""), fr), Lang::Fr);
        assert_eq!(Lang::from_locale(Some(""), fr, Some("en_US")), Lang::Fr);
    }

    #[test]
    fn detect_uses_the_system_language_without_locale_variables() {
        assert_eq!(
            Lang::from_locale_or_system(None, None, None, Some("fr-FR")),
            Lang::Fr
        );
        assert_eq!(
            Lang::from_locale_or_system(None, None, None, Some("fr")),
            Lang::Fr
        );
        assert_eq!(
            Lang::from_locale_or_system(Some(""), None, Some(""), Some("fr-CA")),
            Lang::Fr
        );
        assert_eq!(
            Lang::from_locale_or_system(None, None, None, Some("en-US")),
            Lang::En
        );
        // A locale variable wins over the system language.
        assert_eq!(
            Lang::from_locale_or_system(None, None, Some("en_US.UTF-8"), Some("fr-FR")),
            Lang::En
        );
    }

    #[test]
    fn detect_falls_back_to_english() {
        assert_eq!(Lang::from_locale(None, None, None), Lang::En);
        assert_eq!(Lang::from_locale(Some(""), None, Some("")), Lang::En);
        assert_eq!(Lang::from_locale(None, None, Some("C")), Lang::En);
        assert_eq!(Lang::from_locale(None, None, Some("POSIX")), Lang::En);
        assert_eq!(Lang::from_locale(None, None, Some("de_DE.UTF-8")), Lang::En);
    }
}
