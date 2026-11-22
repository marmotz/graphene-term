//! UI translations: the language of the user interface and the text of every label.
//!
//! English and French are shipped, English is the fallback. Each language has one
//! exhaustive `match`, so adding a [`Msg`] variant fails to compile until every
//! language translates it.

use std::borrow::Cow;

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

/// A section of the settings window, in sidebar order.
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

/// A field of the settings window, one per setting of the file but the keyboard
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
    QuickTerminalHotkey,
    QuickTerminalEdge,
    QuickTerminalSize,
    QuickTerminalScreen,
    QuickTerminalAnimation,
    QuickTerminalHideOnFocusLoss,
}

impl SettingsField {
    /// Every field.
    pub const ALL: [SettingsField; 33] = [
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
        SettingsField::QuickTerminalHotkey,
        SettingsField::QuickTerminalEdge,
        SettingsField::QuickTerminalSize,
        SettingsField::QuickTerminalScreen,
        SettingsField::QuickTerminalAnimation,
        SettingsField::QuickTerminalHideOnFocusLoss,
    ];
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
    // Command blocks actions.
    PreviousBlock,
    NextBlock,
    CopyCommand,
    CopyOutput,
    ToggleFold,
    ToggleFoldAll,
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
    /// The settings action, and the settings window's title.
    OpenSettings,
    Quit,
    /// Sidebar entry of the settings window.
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
    /// Button adding a shortcut to an action.
    AddShortcut,
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
    // Theme editor of the settings window.
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
}

/// Every language, for tests that must cover each of them.
#[cfg(test)]
pub const LANGS: [Lang; 2] = [Lang::En, Lang::Fr];

/// Text of `msg` in `lang`.
pub fn text(lang: Lang, msg: &Msg) -> Cow<'static, str> {
    match lang {
        Lang::En => english(msg),
        Lang::Fr => french(msg),
    }
}

fn english(msg: &Msg) -> Cow<'static, str> {
    let text = match msg {
        Msg::Copy => "Copy",
        Msg::Paste => "Paste",
        Msg::ScrollPageUp => "Scroll up one page",
        Msg::ScrollPageDown => "Scroll down one page",
        Msg::ScrollToTop => "Scroll to top",
        Msg::ScrollToBottom => "Scroll to bottom",
        Msg::FontSizeIncrease => "Increase font size",
        Msg::FontSizeDecrease => "Decrease font size",
        Msg::FontSizeReset => "Reset font size",
        Msg::PreviousBlock => "Previous command block",
        Msg::NextBlock => "Next command block",
        Msg::CopyCommand => "Copy block command",
        Msg::CopyOutput => "Copy block output",
        Msg::ToggleFold => "Fold / unfold block",
        Msg::ToggleFoldAll => "Fold / unfold all blocks",
        Msg::NewTab => "New tab",
        Msg::CloseTab => "Close the tab",
        Msg::ClosePane => "Close pane",
        Msg::NextTab => "Next tab",
        Msg::PreviousTab => "Previous tab",
        Msg::GoToTab(tab) => return format!("Go to tab {tab}").into(),
        Msg::SplitRight => "Split right",
        Msg::SplitDown => "Split down",
        Msg::FocusPane(direction) => match direction {
            Direction::Left => "Focus pane on the left",
            Direction::Right => "Focus pane on the right",
            Direction::Up => "Focus pane above",
            Direction::Down => "Focus pane below",
        },
        Msg::ResizePane(direction) => match direction {
            Direction::Left => "Resize pane to the left",
            Direction::Right => "Resize pane to the right",
            Direction::Up => "Resize pane upwards",
            Direction::Down => "Resize pane downwards",
        },
        Msg::NextPane => "Next pane",
        Msg::PreviousPane => "Previous pane",
        Msg::ToggleZoom => "Zoom / unzoom pane",
        Msg::ToggleBroadcastAll => "Broadcast input to all panes / stop",
        Msg::ZoomPane => "Zoom the pane",
        Msg::UnzoomPane => "Unzoom the pane",
        Msg::SplitVertical => "Split vertically",
        Msg::SplitHorizontal => "Split horizontally",
        Msg::Themes => "Themes",
        Msg::MoreThemes => "More...",
        Msg::FoldBlock => "Fold block",
        Msg::UnfoldBlock => "Unfold block",
        Msg::CommandPalette => "Command palette",
        Msg::OpenSettings => "Settings",
        Msg::Quit => "Quit Graphene",
        Msg::Section(section) => match section {
            SettingsSection::Font => "Font",
            SettingsSection::Window => "Window",
            SettingsSection::Scrollback => "Scrollback",
            SettingsSection::Cursor => "Cursor",
            SettingsSection::Selection => "Selection",
            SettingsSection::Tabs => "Tabs",
            SettingsSection::Language => "Language",
            SettingsSection::Theme => "Theme",
            SettingsSection::KeyboardShortcuts => "Keyboard shortcuts",
            SettingsSection::Notifications => "Notifications",
            SettingsSection::QuickTerminal => "Quick terminal",
            SettingsSection::Updates => "Updates",
        },
        Msg::Field(field) => match field {
            SettingsField::Language => "Language",
            SettingsField::FontFamily => "Family",
            SettingsField::FontSize => "Size",
            SettingsField::FontLigatures => "Ligatures",
            SettingsField::FontFeatures => "OpenType features",
            SettingsField::StartupMode => "Startup mode",
            SettingsField::WindowWidth => "Width",
            SettingsField::WindowHeight => "Height",
            SettingsField::Padding => "Padding",
            SettingsField::BackgroundOpacity => "Background opacity",
            SettingsField::WindowProgress => "Show program progress",
            SettingsField::RestoreSession => "Restore the session at launch",
            SettingsField::ScrollbackLines => "Lines kept",
            SettingsField::CursorShape => "Shape",
            SettingsField::CursorBlinking => "Blinking",
            SettingsField::WordSeparators => "Word separators",
            SettingsField::TabPosition => "Position",
            SettingsField::ThemeFollow => "Follow the system appearance",
            SettingsField::ThemeName => "Theme",
            SettingsField::ThemeLight => "Light theme",
            SettingsField::ThemeDark => "Dark theme",
            SettingsField::UpdatesCheck => "Check for updates",
            SettingsField::UpdatesInterval => "Check interval",
            SettingsField::NotificationsProgram => "Notifications from programs",
            SettingsField::NotificationsCommandFinished => "Notify when a long command finishes",
            SettingsField::NotificationsCommandThreshold => "Long command threshold (seconds)",
            SettingsField::NotificationsBell => "Notify on terminal bell",
            SettingsField::QuickTerminalHotkey => "Global shortcut",
            SettingsField::QuickTerminalEdge => "Screen edge",
            SettingsField::QuickTerminalSize => "Size (% of the screen)",
            SettingsField::QuickTerminalScreen => "Screen",
            SettingsField::QuickTerminalAnimation => "Slide duration (ms)",
            SettingsField::QuickTerminalHideOnFocusLoss => "Hide when the focus is lost",
        },
        Msg::SystemMonospace => "System monospace",
        Msg::ActiveScreen => "Screen under the mouse cursor",
        Msg::PrimaryScreen => "Primary screen",
        Msg::UnknownKeys => "Unknown keys",
        Msg::AtLine(line) => return format!("line {line}").into(),
        Msg::FixFileByHand => {
            "The file cannot be read: fix it by hand. The settings stay disabled until then."
        }
        Msg::QuickTerminalUnavailableOnWayland => "The quick terminal is not available on Wayland",
        Msg::CannotSave => "Cannot save the change",
        Msg::NotANumber => "Enter a number",
        Msg::NotAWholeNumber => "Enter a whole number",
        Msg::AtLeast(min) => return format!("Must be at least {min}").into(),
        Msg::AtMost(max) => return format!("Must be at most {max}").into(),
        Msg::AddShortcut => "Add",
        Msg::ChangeShortcut => "Change",
        Msg::RemoveShortcut => "Remove this shortcut",
        Msg::NoShortcut => "No shortcut",
        Msg::PressShortcut => "Press a shortcut · Esc to cancel",
        Msg::AlreadyUsedBy(action) => return format!("Already used by {action}").into(),
        Msg::Replace => "Replace",
        Msg::FilterActions => "Filter actions",
        Msg::NoActionMatches => "No action matches",
        Msg::SearchShortcut => "Find a shortcut",
        Msg::NoActionForShortcut => "No action uses this shortcut",
        Msg::ThemeEditor => "Theme editor",
        Msg::ThemeEditorIntro => {
            "Change the colors of your own themes. Built-in themes cannot be changed: duplicate one to get a copy you can edit."
        }
        Msg::ThemeColorsHint => {
            "Click a swatch to pick a color, or type or paste its #rrggbb code. Each change is saved to the theme file right away."
        }
        Msg::InvalidHexColor => "Enter a color as #rrggbb",
        Msg::OpenThemesFolder => "Open the themes folder",
        Msg::EditedTheme => "Theme to edit",
        Msg::Duplicate => "Duplicate",
        Msg::Rename => "Rename",
        Msg::Delete => "Delete",
        Msg::NewThemeName => "New name",
        Msg::Confirm => "Confirm",
        Msg::ConfirmDeleteTheme(name) => {
            return format!("Delete the theme {name}? Its file is removed.").into();
        }
        Msg::BuiltInTheme => {
            "This built-in theme cannot be changed. Click Duplicate to create a copy you can edit."
        }
        Msg::KeepTextColor => "Keep the text color",
        Msg::ThemeColorGroup(group) => match group {
            ColorGroup::Terminal => "Terminal colors",
            ColorGroup::Normal => "Normal ANSI colors",
            ColorGroup::Bright => "Bright ANSI colors",
            ColorGroup::Interface => "Interface colors",
        },
        Msg::ThemeColor(key) => english_color(*key),
        Msg::ThemeNameError(error) => match error {
            NameError::Empty => "Enter a name",
            NameError::Invalid => "A name cannot contain / or \\ nor start with a dot",
            NameError::Taken => "A theme with this name already exists",
        },
        Msg::EqualisePanes => "Equalise panes",
        Msg::DetachPane => "Move pane to a new tab",
        Msg::RenameTab => "Rename the tab",
        Msg::PalettePlaceholder => "Type a command, a tab or a pane",
        Msg::SecretAnswer => "The Answer to the Ultimate Question",
        Msg::SecretAuthor => "marmotz on GitHub",
        Msg::SecretGraphene => "Graphene",
        Msg::TabEntry { tab, title } => return format!("Tab {tab}: {title}").into(),
        Msg::PaneEntry { tab, pane, title } => {
            return format!("Tab {tab} > pane {pane}: {title}").into();
        }
        Msg::ConfirmCloseTitle(target) => match target {
            CloseTarget::Pane => "Close this pane?",
            CloseTarget::Tab => "Close this tab?",
            CloseTarget::Window => "Close the window?",
        },
        Msg::ProgramsRunning(programs) => {
            return match programs.as_slice() {
                [program] => format!("{program} is still running"),
                _ => format!(
                    "{} programs are still running: {}",
                    programs.len(),
                    programs.join(", ")
                ),
            }
            .into();
        }
        Msg::Close => "Close",
        Msg::Cancel => "Cancel",
        Msg::UpdateAvailable(tag) => return format!("Graphene {tag} is available").into(),
        Msg::OpenRelease => "Open release",
        Msg::IgnoreVersion => "Ignore this version",
        Msg::LinesHidden(1) => "1 line hidden",
        Msg::LinesHidden(rows) => return format!("{rows} lines hidden").into(),
        Msg::CommandFinished => "Command finished",
        Msg::CommandFailed(code) => return format!("Command failed (code {code})").into(),
        Msg::Bell => "Bell",
        Msg::ShowPane => "Show pane",
    };
    Cow::Borrowed(text)
}

fn french(msg: &Msg) -> Cow<'static, str> {
    let text = match msg {
        Msg::Copy => "Copier",
        Msg::Paste => "Coller",
        Msg::ScrollPageUp => "Remonter d'une page",
        Msg::ScrollPageDown => "Descendre d'une page",
        Msg::ScrollToTop => "Aller tout en haut",
        Msg::ScrollToBottom => "Aller tout en bas",
        Msg::FontSizeIncrease => "Agrandir la police",
        Msg::FontSizeDecrease => "Réduire la police",
        Msg::FontSizeReset => "Taille de police par défaut",
        Msg::PreviousBlock => "Bloc de commande précédent",
        Msg::NextBlock => "Bloc de commande suivant",
        Msg::CopyCommand => "Copier la commande du bloc",
        Msg::CopyOutput => "Copier la sortie du bloc",
        Msg::ToggleFold => "Replier / déplier le bloc",
        Msg::ToggleFoldAll => "Replier / déplier tous les blocs",
        Msg::NewTab => "Nouvel onglet",
        Msg::CloseTab => "Fermer l'onglet",
        Msg::ClosePane => "Fermer le panneau",
        Msg::NextTab => "Onglet suivant",
        Msg::PreviousTab => "Onglet précédent",
        Msg::GoToTab(tab) => return format!("Aller à l'onglet {tab}").into(),
        Msg::SplitRight => "Diviser à droite",
        Msg::SplitDown => "Diviser en bas",
        Msg::FocusPane(direction) => match direction {
            Direction::Left => "Aller au panneau de gauche",
            Direction::Right => "Aller au panneau de droite",
            Direction::Up => "Aller au panneau du haut",
            Direction::Down => "Aller au panneau du bas",
        },
        Msg::ResizePane(direction) => match direction {
            Direction::Left => "Redimensionner le panneau vers la gauche",
            Direction::Right => "Redimensionner le panneau vers la droite",
            Direction::Up => "Redimensionner le panneau vers le haut",
            Direction::Down => "Redimensionner le panneau vers le bas",
        },
        Msg::NextPane => "Panneau suivant",
        Msg::PreviousPane => "Panneau précédent",
        Msg::ToggleZoom => "Agrandir / réduire le panneau",
        Msg::ToggleBroadcastAll => "Diffuser la saisie à tous les panneaux / arrêter",
        Msg::ZoomPane => "Agrandir le panneau",
        Msg::UnzoomPane => "Réduire le panneau",
        Msg::SplitVertical => "Diviser verticalement",
        Msg::SplitHorizontal => "Diviser horizontalement",
        Msg::Themes => "Thèmes",
        Msg::MoreThemes => "Plus...",
        Msg::FoldBlock => "Replier le bloc",
        Msg::UnfoldBlock => "Déplier le bloc",
        Msg::CommandPalette => "Palette de commandes",
        Msg::OpenSettings => "Paramètres",
        Msg::Quit => "Quitter Graphene",
        Msg::Section(section) => match section {
            SettingsSection::Font => "Police",
            SettingsSection::Window => "Fenêtre",
            SettingsSection::Scrollback => "Historique",
            SettingsSection::Cursor => "Curseur",
            SettingsSection::Selection => "Sélection",
            SettingsSection::Tabs => "Onglets",
            SettingsSection::Language => "Langue",
            SettingsSection::Theme => "Thème",
            SettingsSection::KeyboardShortcuts => "Raccourcis clavier",
            SettingsSection::Notifications => "Notifications",
            SettingsSection::QuickTerminal => "Quick terminal",
            SettingsSection::Updates => "Mises à jour",
        },
        Msg::Field(field) => match field {
            SettingsField::Language => "Langue",
            SettingsField::FontFamily => "Famille",
            SettingsField::FontSize => "Taille",
            SettingsField::FontLigatures => "Ligatures",
            SettingsField::FontFeatures => "Fonctionnalités OpenType",
            SettingsField::StartupMode => "Mode au démarrage",
            SettingsField::WindowWidth => "Largeur",
            SettingsField::WindowHeight => "Hauteur",
            SettingsField::Padding => "Marge intérieure",
            SettingsField::BackgroundOpacity => "Opacité du fond",
            SettingsField::WindowProgress => "Afficher la progression des programmes",
            SettingsField::RestoreSession => "Restaurer la session au lancement",
            SettingsField::ScrollbackLines => "Lignes conservées",
            SettingsField::CursorShape => "Forme",
            SettingsField::CursorBlinking => "Clignotement",
            SettingsField::WordSeparators => "Séparateurs de mots",
            SettingsField::TabPosition => "Emplacement",
            SettingsField::ThemeFollow => "Suivre l'apparence du système",
            SettingsField::ThemeName => "Thème",
            SettingsField::ThemeLight => "Thème clair",
            SettingsField::ThemeDark => "Thème sombre",
            SettingsField::UpdatesCheck => "Rechercher les mises à jour",
            SettingsField::UpdatesInterval => "Intervalle de recherche",
            SettingsField::NotificationsProgram => "Notifications des programmes",
            SettingsField::NotificationsCommandFinished => "Signaler la fin d'une commande longue",
            SettingsField::NotificationsCommandThreshold => {
                "Seuil d'une commande longue (secondes)"
            }
            SettingsField::NotificationsBell => "Signaler la sonnerie du terminal",
            SettingsField::QuickTerminalHotkey => "Raccourci global",
            SettingsField::QuickTerminalEdge => "Bord de l'écran",
            SettingsField::QuickTerminalSize => "Taille (% de l'écran)",
            SettingsField::QuickTerminalScreen => "Écran",
            SettingsField::QuickTerminalAnimation => "Durée du glissement (ms)",
            SettingsField::QuickTerminalHideOnFocusLoss => "Masquer à la perte du focus",
        },
        Msg::SystemMonospace => "Monospace du système",
        Msg::ActiveScreen => "Écran sous le pointeur de la souris",
        Msg::PrimaryScreen => "Écran principal",
        Msg::UnknownKeys => "Clés inconnues",
        Msg::AtLine(line) => return format!("ligne {line}").into(),
        Msg::FixFileByHand => {
            "Le fichier est illisible : corrigez-le à la main. Les paramètres restent désactivés d'ici là."
        }
        Msg::QuickTerminalUnavailableOnWayland => {
            "Le quick terminal n'est pas disponible sous Wayland"
        }
        Msg::CannotSave => "Impossible d'enregistrer la modification",
        Msg::NotANumber => "Saisissez un nombre",
        Msg::NotAWholeNumber => "Saisissez un nombre entier",
        Msg::AtLeast(min) => return format!("Doit valoir au moins {min}").into(),
        Msg::AtMost(max) => return format!("Doit valoir au plus {max}").into(),
        Msg::AddShortcut => "Ajouter",
        Msg::ChangeShortcut => "Modifier",
        Msg::RemoveShortcut => "Supprimer ce raccourci",
        Msg::NoShortcut => "Aucun raccourci",
        Msg::PressShortcut => "Appuyez sur un raccourci · Échap pour annuler",
        Msg::AlreadyUsedBy(action) => return format!("Déjà utilisé par {action}").into(),
        Msg::Replace => "Remplacer",
        Msg::FilterActions => "Filtrer les actions",
        Msg::NoActionMatches => "Aucune action ne correspond",
        Msg::SearchShortcut => "Rechercher un raccourci",
        Msg::NoActionForShortcut => "Aucune action n'utilise ce raccourci",
        Msg::ThemeEditor => "Éditeur de thème",
        Msg::ThemeEditorIntro => {
            "Modifiez les couleurs de vos propres thèmes. Les thèmes intégrés ne sont pas modifiables : dupliquez-en un pour obtenir une copie modifiable."
        }
        Msg::ThemeColorsHint => {
            "Cliquez sur une pastille pour choisir une couleur, ou saisissez ou collez son code #rrggbb. Chaque changement est enregistré aussitôt dans le fichier du thème."
        }
        Msg::InvalidHexColor => "Saisissez une couleur au format #rrggbb",
        Msg::OpenThemesFolder => "Ouvrir le dossier des thèmes",
        Msg::EditedTheme => "Thème à modifier",
        Msg::Duplicate => "Dupliquer",
        Msg::Rename => "Renommer",
        Msg::Delete => "Supprimer",
        Msg::NewThemeName => "Nouveau nom",
        Msg::Confirm => "Valider",
        Msg::ConfirmDeleteTheme(name) => {
            return format!("Supprimer le thème {name} ? Son fichier est effacé.").into();
        }
        Msg::BuiltInTheme => {
            "Ce thème intégré n'est pas modifiable. Cliquez sur Dupliquer pour en créer une copie modifiable."
        }
        Msg::KeepTextColor => "Garder la couleur du texte",
        Msg::ThemeColorGroup(group) => match group {
            ColorGroup::Terminal => "Couleurs du terminal",
            ColorGroup::Normal => "Couleurs ANSI normales",
            ColorGroup::Bright => "Couleurs ANSI vives",
            ColorGroup::Interface => "Couleurs de l'interface",
        },
        Msg::ThemeColor(key) => french_color(*key),
        Msg::ThemeNameError(error) => match error {
            NameError::Empty => "Saisissez un nom",
            NameError::Invalid => "Un nom ne peut pas contenir / ou \\ ni commencer par un point",
            NameError::Taken => "Un thème porte déjà ce nom",
        },
        Msg::EqualisePanes => "Égaliser les panneaux",
        Msg::DetachPane => "Déplacer le panneau dans un nouvel onglet",
        Msg::RenameTab => "Renommer l'onglet",
        Msg::PalettePlaceholder => "Tapez une commande, un onglet ou un panneau",
        Msg::SecretAnswer => "La grande question sur la vie, l'univers et le reste",
        Msg::SecretAuthor => "marmotz sur GitHub",
        Msg::SecretGraphene => "Graphene",
        Msg::TabEntry { tab, title } => return format!("Onglet {tab} : {title}").into(),
        Msg::PaneEntry { tab, pane, title } => {
            return format!("Onglet {tab} > panneau {pane} : {title}").into();
        }
        Msg::ConfirmCloseTitle(target) => match target {
            CloseTarget::Pane => "Fermer ce panneau ?",
            CloseTarget::Tab => "Fermer cet onglet ?",
            CloseTarget::Window => "Fermer la fenêtre ?",
        },
        Msg::ProgramsRunning(programs) => {
            return match programs.as_slice() {
                [program] => format!("{program} est toujours en cours d'exécution"),
                _ => format!(
                    "{} programmes sont toujours en cours d'exécution : {}",
                    programs.len(),
                    programs.join(", ")
                ),
            }
            .into();
        }
        Msg::Close => "Fermer",
        Msg::Cancel => "Annuler",
        Msg::UpdateAvailable(tag) => return format!("Graphene {tag} est disponible").into(),
        Msg::OpenRelease => "Voir la version",
        Msg::IgnoreVersion => "Ignorer cette version",
        Msg::LinesHidden(1) => "1 ligne masquée",
        Msg::LinesHidden(rows) => return format!("{rows} lignes masquées").into(),
        Msg::CommandFinished => "Commande terminée",
        Msg::CommandFailed(code) => return format!("Commande échouée (code {code})").into(),
        Msg::Bell => "Sonnerie",
        Msg::ShowPane => "Afficher le panneau",
    };
    Cow::Borrowed(text)
}

/// English label of a theme color.
fn english_color(key: ColorKey) -> &'static str {
    match key {
        ColorKey::Foreground => "Text",
        ColorKey::Background => "Background",
        ColorKey::Cursor => "Cursor",
        ColorKey::CursorText => "Text under the cursor",
        ColorKey::SelectionBackground => "Selection background",
        ColorKey::SelectionForeground => "Selected text",
        ColorKey::NormalBlack => "Normal black",
        ColorKey::NormalRed => "Normal red",
        ColorKey::NormalGreen => "Normal green",
        ColorKey::NormalYellow => "Normal yellow",
        ColorKey::NormalBlue => "Normal blue",
        ColorKey::NormalMagenta => "Normal magenta",
        ColorKey::NormalCyan => "Normal cyan",
        ColorKey::NormalWhite => "Normal white",
        ColorKey::BrightBlack => "Bright black",
        ColorKey::BrightRed => "Bright red",
        ColorKey::BrightGreen => "Bright green",
        ColorKey::BrightYellow => "Bright yellow",
        ColorKey::BrightBlue => "Bright blue",
        ColorKey::BrightMagenta => "Bright magenta",
        ColorKey::BrightCyan => "Bright cyan",
        ColorKey::BrightWhite => "Bright white",
        ColorKey::TabBarBackground => "Tab bar background",
        ColorKey::TabActiveBackground => "Active tab background",
        ColorKey::TabActiveText => "Active tab text",
        ColorKey::TabInactiveText => "Inactive tab text",
        ColorKey::PaneBorder => "Pane border",
        ColorKey::PaneBroadcast => "Broadcast pane title bar",
        ColorKey::PaletteBackground => "Command palette background",
        ColorKey::PaletteText => "Command palette text",
        ColorKey::PaletteMatch => "Command palette matches",
        ColorKey::PaletteSelectionBackground => "Command palette selection",
        ColorKey::BannerBackground => "Banner background",
        ColorKey::BannerText => "Banner text",
    }
}

/// French label of a theme color.
fn french_color(key: ColorKey) -> &'static str {
    match key {
        ColorKey::Foreground => "Texte",
        ColorKey::Background => "Fond",
        ColorKey::Cursor => "Curseur",
        ColorKey::CursorText => "Texte sous le curseur",
        ColorKey::SelectionBackground => "Fond de la sélection",
        ColorKey::SelectionForeground => "Texte sélectionné",
        ColorKey::NormalBlack => "Noir normal",
        ColorKey::NormalRed => "Rouge normal",
        ColorKey::NormalGreen => "Vert normal",
        ColorKey::NormalYellow => "Jaune normal",
        ColorKey::NormalBlue => "Bleu normal",
        ColorKey::NormalMagenta => "Magenta normal",
        ColorKey::NormalCyan => "Cyan normal",
        ColorKey::NormalWhite => "Blanc normal",
        ColorKey::BrightBlack => "Noir vif",
        ColorKey::BrightRed => "Rouge vif",
        ColorKey::BrightGreen => "Vert vif",
        ColorKey::BrightYellow => "Jaune vif",
        ColorKey::BrightBlue => "Bleu vif",
        ColorKey::BrightMagenta => "Magenta vif",
        ColorKey::BrightCyan => "Cyan vif",
        ColorKey::BrightWhite => "Blanc vif",
        ColorKey::TabBarBackground => "Fond de la barre d'onglets",
        ColorKey::TabActiveBackground => "Fond de l'onglet actif",
        ColorKey::TabActiveText => "Texte de l'onglet actif",
        ColorKey::TabInactiveText => "Texte des onglets inactifs",
        ColorKey::PaneBorder => "Bordure des panneaux",
        ColorKey::PaneBroadcast => "Barre de titre des panneaux diffusés",
        ColorKey::PaletteBackground => "Fond de la palette de commandes",
        ColorKey::PaletteText => "Texte de la palette de commandes",
        ColorKey::PaletteMatch => "Lettres trouvées dans la palette",
        ColorKey::PaletteSelectionBackground => "Sélection dans la palette",
        ColorKey::BannerBackground => "Fond du bandeau",
        ColorKey::BannerText => "Texte du bandeau",
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
        }
    }
    const VARIANT_COUNT: usize = 103;

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
            Msg::PreviousBlock,
            Msg::NextBlock,
            Msg::CopyCommand,
            Msg::CopyOutput,
            Msg::ToggleFold,
            Msg::ToggleFoldAll,
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
            Msg::CannotSave,
            Msg::NotANumber,
            Msg::NotAWholeNumber,
            Msg::AtLeast("6".into()),
            Msg::AtMost("72".into()),
            Msg::AddShortcut,
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
