//! UI translations: the language of the user interface and the text of every label.
//!
//! English and French are shipped, English is the fallback. Each language has one
//! exhaustive `match`, so adding a [`Msg`] variant fails to compile until every
//! language translates it.

use std::borrow::Cow;

/// Language of the user interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    En,
    Fr,
}

impl Lang {
    /// Language of the system locale, read from `LC_ALL`, then `LC_MESSAGES`, then
    /// `LANG`.
    pub fn detect() -> Self {
        let var = |name| std::env::var(name).ok();
        Self::from_locale(
            var("LC_ALL").as_deref(),
            var("LC_MESSAGES").as_deref(),
            var("LANG").as_deref(),
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
        let locale = [lc_all, lc_messages, lang]
            .into_iter()
            .flatten()
            .find(|value| !value.is_empty());
        let Some(locale) = locale else {
            return Lang::En;
        };
        let language = locale.split(['_', '.', '@']).next().unwrap_or_default();
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
    Updates,
}

impl SettingsSection {
    /// Every section, in sidebar order.
    pub const ALL: [SettingsSection; 10] = [
        SettingsSection::Language,
        SettingsSection::Font,
        SettingsSection::Window,
        SettingsSection::Scrollback,
        SettingsSection::Cursor,
        SettingsSection::Selection,
        SettingsSection::Tabs,
        SettingsSection::Theme,
        SettingsSection::KeyboardShortcuts,
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
    StartupMode,
    WindowWidth,
    WindowHeight,
    Padding,
    BackgroundOpacity,
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
}

impl SettingsField {
    /// Every field.
    pub const ALL: [SettingsField; 19] = [
        SettingsField::Language,
        SettingsField::FontFamily,
        SettingsField::FontSize,
        SettingsField::StartupMode,
        SettingsField::WindowWidth,
        SettingsField::WindowHeight,
        SettingsField::Padding,
        SettingsField::BackgroundOpacity,
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
    #[allow(dead_code)]
    FoldBlock,
    #[allow(dead_code)]
    UnfoldBlock,
    CommandPalette,
    EqualisePanes,
    DetachPane,
    /// The settings action, and the settings window's title.
    OpenSettings,
    Quit,
    /// Sidebar entry of the settings window.
    Section(SettingsSection),
    /// Label of a settings field.
    Field(SettingsField),
    /// Font family choice that leaves `font.family` empty.
    SystemMonospace,
    /// Heading of the unknown keys of the file.
    UnknownKeys,
    /// 1-based line of the file.
    AtLine(usize),
    /// Note shown when the file cannot be parsed and the settings are disabled.
    FixFileByHand,
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
    /// Hint shown in the command palette's empty search field.
    PalettePlaceholder,
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
}

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
        Msg::ZoomPane => "Zoom the pane",
        Msg::UnzoomPane => "Unzoom the pane",
        Msg::SplitVertical => "Split vertically",
        Msg::SplitHorizontal => "Split horizontally",
        Msg::Themes => "Themes",
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
            SettingsSection::Updates => "Updates",
        },
        Msg::Field(field) => match field {
            SettingsField::Language => "Language",
            SettingsField::FontFamily => "Family",
            SettingsField::FontSize => "Size",
            SettingsField::StartupMode => "Startup mode",
            SettingsField::WindowWidth => "Width",
            SettingsField::WindowHeight => "Height",
            SettingsField::Padding => "Padding",
            SettingsField::BackgroundOpacity => "Background opacity",
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
        },
        Msg::SystemMonospace => "System monospace",
        Msg::UnknownKeys => "Unknown keys",
        Msg::AtLine(line) => return format!("line {line}").into(),
        Msg::FixFileByHand => {
            "The file cannot be read: fix it by hand. The settings stay disabled until then."
        }
        Msg::CannotSave => "Cannot save the change",
        Msg::NotANumber => "Enter a number",
        Msg::NotAWholeNumber => "Enter a whole number",
        Msg::AtLeast(min) => return format!("Must be at least {min}").into(),
        Msg::AtMost(max) => return format!("Must be at most {max}").into(),
        Msg::AddShortcut => "Add",
        Msg::RemoveShortcut => "Remove this shortcut",
        Msg::NoShortcut => "No shortcut",
        Msg::PressShortcut => "Press a shortcut · Esc to cancel",
        Msg::AlreadyUsedBy(action) => return format!("Already used by {action}").into(),
        Msg::Replace => "Replace",
        Msg::EqualisePanes => "Equalise panes",
        Msg::DetachPane => "Move pane to a new tab",
        Msg::PalettePlaceholder => "Type a command, a tab or a pane",
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
        Msg::ZoomPane => "Agrandir le panneau",
        Msg::UnzoomPane => "Réduire le panneau",
        Msg::SplitVertical => "Diviser verticalement",
        Msg::SplitHorizontal => "Diviser horizontalement",
        Msg::Themes => "Thèmes",
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
            SettingsSection::Updates => "Mises à jour",
        },
        Msg::Field(field) => match field {
            SettingsField::Language => "Langue",
            SettingsField::FontFamily => "Famille",
            SettingsField::FontSize => "Taille",
            SettingsField::StartupMode => "Mode au démarrage",
            SettingsField::WindowWidth => "Largeur",
            SettingsField::WindowHeight => "Hauteur",
            SettingsField::Padding => "Marge intérieure",
            SettingsField::BackgroundOpacity => "Opacité du fond",
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
        },
        Msg::SystemMonospace => "Monospace du système",
        Msg::UnknownKeys => "Clés inconnues",
        Msg::AtLine(line) => return format!("ligne {line}").into(),
        Msg::FixFileByHand => {
            "Le fichier est illisible : corrigez-le à la main. Les paramètres restent désactivés d'ici là."
        }
        Msg::CannotSave => "Impossible d'enregistrer la modification",
        Msg::NotANumber => "Saisissez un nombre",
        Msg::NotAWholeNumber => "Saisissez un nombre entier",
        Msg::AtLeast(min) => return format!("Doit valoir au moins {min}").into(),
        Msg::AtMost(max) => return format!("Doit valoir au plus {max}").into(),
        Msg::AddShortcut => "Ajouter",
        Msg::RemoveShortcut => "Supprimer ce raccourci",
        Msg::NoShortcut => "Aucun raccourci",
        Msg::PressShortcut => "Appuyez sur un raccourci · Échap pour annuler",
        Msg::AlreadyUsedBy(action) => return format!("Déjà utilisé par {action}").into(),
        Msg::Replace => "Remplacer",
        Msg::EqualisePanes => "Égaliser les panneaux",
        Msg::DetachPane => "Déplacer le panneau dans un nouvel onglet",
        Msg::PalettePlaceholder => "Tapez une commande, un onglet ou un panneau",
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
    };
    Cow::Borrowed(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LANGS: [Lang; 2] = [Lang::En, Lang::Fr];
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
            Msg::ZoomPane => 61,
            Msg::UnzoomPane => 62,
            Msg::SplitVertical => 63,
            Msg::SplitHorizontal => 64,
            Msg::Themes => 65,
            Msg::FoldBlock => 66,
            Msg::UnfoldBlock => 67,
            Msg::CommandPalette => 27,
            Msg::EqualisePanes => 28,
            Msg::DetachPane => 29,
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
            Msg::RemoveShortcut => 54,
            Msg::NoShortcut => 55,
            Msg::PressShortcut => 56,
            Msg::AlreadyUsedBy(_) => 57,
            Msg::Replace => 58,
        }
    }
    const VARIANT_COUNT: usize = 68;

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
            Msg::ZoomPane,
            Msg::UnzoomPane,
            Msg::SplitVertical,
            Msg::SplitHorizontal,
            Msg::Themes,
            Msg::FoldBlock,
            Msg::UnfoldBlock,
            Msg::CommandPalette,
            Msg::EqualisePanes,
            Msg::DetachPane,
            Msg::OpenSettings,
            Msg::Quit,
            Msg::PalettePlaceholder,
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
            Msg::UnknownKeys,
            Msg::AtLine(12),
            Msg::FixFileByHand,
            Msg::CannotSave,
            Msg::NotANumber,
            Msg::NotAWholeNumber,
            Msg::AtLeast("6".into()),
            Msg::AtMost("72".into()),
            Msg::AddShortcut,
            Msg::RemoveShortcut,
            Msg::NoShortcut,
            Msg::PressShortcut,
            Msg::AlreadyUsedBy("Copy".into()),
            Msg::Replace,
        ];
        messages.extend(DIRECTIONS.map(Msg::FocusPane));
        messages.extend(DIRECTIONS.map(Msg::ResizePane));
        messages.extend(TARGETS.map(Msg::ConfirmCloseTitle));
        messages.extend(SettingsSection::ALL.map(Msg::Section));
        messages.extend(SettingsField::ALL.map(Msg::Field));
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
        for msg in all_messages() {
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
    fn detect_falls_back_to_english() {
        assert_eq!(Lang::from_locale(None, None, None), Lang::En);
        assert_eq!(Lang::from_locale(Some(""), None, Some("")), Lang::En);
        assert_eq!(Lang::from_locale(None, None, Some("C")), Lang::En);
        assert_eq!(Lang::from_locale(None, None, Some("POSIX")), Lang::En);
        assert_eq!(Lang::from_locale(None, None, Some("de_DE.UTF-8")), Lang::En);
    }
}
