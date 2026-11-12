//! UI translations: the language of the user interface and the text of every label.
//!
//! English and French are shipped, English is the fallback. Each language has one
//! exhaustive `match`, so adding a [`Msg`] variant fails to compile until every
//! language translates it.

// The tab bar, the command palette and the dialogs consume these labels as they land.
#![allow(dead_code, reason = "labels are consumed by the UI tasks as they land")]

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
    // Tabs and panes actions.
    NewTab,
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
    CommandPalette,
    EqualisePanes,
    DetachPane,
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
        Msg::NewTab => "New tab",
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
        Msg::CommandPalette => "Command palette",
        Msg::EqualisePanes => "Equalise panes",
        Msg::DetachPane => "Move pane to a new tab",
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
        Msg::NewTab => "Nouvel onglet",
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
        Msg::CommandPalette => "Palette de commandes",
        Msg::EqualisePanes => "Égaliser les panneaux",
        Msg::DetachPane => "Déplacer le panneau dans un nouvel onglet",
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
            Msg::NewTab => 9,
            Msg::ClosePane => 10,
            Msg::NextTab => 11,
            Msg::PreviousTab => 12,
            Msg::GoToTab(_) => 13,
            Msg::SplitRight => 14,
            Msg::SplitDown => 15,
            Msg::FocusPane(_) => 16,
            Msg::ResizePane(_) => 17,
            Msg::NextPane => 18,
            Msg::PreviousPane => 19,
            Msg::ToggleZoom => 20,
            Msg::CommandPalette => 21,
            Msg::EqualisePanes => 22,
            Msg::DetachPane => 23,
            Msg::TabEntry { .. } => 24,
            Msg::PaneEntry { .. } => 25,
            Msg::ConfirmCloseTitle(_) => 26,
            Msg::ProgramsRunning(_) => 27,
            Msg::Close => 28,
            Msg::Cancel => 29,
            Msg::UpdateAvailable(_) => 30,
            Msg::OpenRelease => 31,
            Msg::IgnoreVersion => 32,
        }
    }
    const VARIANT_COUNT: usize = 33;

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
            Msg::NewTab,
            Msg::ClosePane,
            Msg::NextTab,
            Msg::PreviousTab,
            Msg::GoToTab(3),
            Msg::SplitRight,
            Msg::SplitDown,
            Msg::NextPane,
            Msg::PreviousPane,
            Msg::ToggleZoom,
            Msg::CommandPalette,
            Msg::EqualisePanes,
            Msg::DetachPane,
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
        ];
        messages.extend(DIRECTIONS.map(Msg::FocusPane));
        messages.extend(DIRECTIONS.map(Msg::ResizePane));
        messages.extend(TARGETS.map(Msg::ConfirmCloseTitle));
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
