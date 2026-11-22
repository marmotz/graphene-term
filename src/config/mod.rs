//! User configuration: `config.toml` in the config directory, read key by key so that one
//! invalid value or unknown key never discards the rest of the file.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};

use alacritty_terminal::term::{Config as TermConfig, Osc52};
use alacritty_terminal::vte::ansi::{CursorShape as TermCursorShape, CursorStyle};
use cosmic_text::{FeatureTag, FontFeatures};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use toml::Spanned;
use toml::de::{DeTable, DeValue, ValueDeserializer};

use crate::appearance::Appearance;
use crate::i18n::Lang;
use crate::keybindings;

mod error;
mod files;
mod paths;
mod reader;
mod values;
pub use self::error::*;
pub use self::files::*;
pub use self::paths::*;
pub(crate) use self::reader::*;
pub use self::values::*;

/// The fully commented default configuration, written on first launch.
pub const DEFAULT_CONFIG: &str = include_str!("../../config.default.toml");

/// Name of the configuration file inside the config directory.
pub const CONFIG_FILE: &str = "config.toml";

/// The keyboard shortcuts file template, written on first launch.
pub const DEFAULT_KEYBINDINGS: &str = include_str!("../../keybindings.default.toml");

/// Name of the keyboard shortcuts file inside the config directory.
pub const KEYBINDINGS_FILE: &str = "keybindings.toml";

/// Characters ending a word on double-click.
///
/// Alacritty's default list without `:`, so that a URL (`https://...`) or `host:port`
/// stays one word.
pub const SEMANTIC_ESCAPE_CHARS: &str = ",│`|\"' ()[]{}<>\t";

pub const DEFAULT_FONT_SIZE: f32 = 14.0;
pub const MIN_FONT_SIZE: f32 = 6.0;
pub const MAX_FONT_SIZE: f32 = 72.0;
/// Upper bound of `scrollback.lines`.
pub const MAX_SCROLLBACK_LINES: usize = 100_000;

/// The user configuration. `Default` holds the built-in values, so a missing or empty
/// file behaves like no configuration at all.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub language: Language,
    pub font: FontConfig,
    pub window: WindowConfig,
    pub scrollback_lines: usize,
    pub cursor: CursorConfig,
    pub word_separators: String,
    pub tab_position: TabPosition,
    pub theme: ThemeConfig,
    /// Valid `"keys" = "action"` pairs, applied on top of the default shortcuts by
    /// [`crate::keybindings::Keybindings`].
    pub keybindings: BTreeMap<String, String>,
    #[allow(dead_code, reason = "applied by the update check thread task")]
    pub updates: UpdatesConfig,
    pub notifications: NotificationsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: Language::Auto,
            font: FontConfig {
                family: None,
                size: DEFAULT_FONT_SIZE,
                ligatures: true,
                features: FontFeatureList::default(),
            },
            window: WindowConfig {
                startup_mode: StartupMode::Default,
                width: 960,
                height: 600,
                padding: 8.0,
                background_opacity: 1.0,
                progress: true,
            },
            scrollback_lines: 10_000,
            cursor: CursorConfig {
                shape: CursorShape::Block,
                blinking: false,
            },
            word_separators: SEMANTIC_ESCAPE_CHARS.to_owned(),
            tab_position: TabPosition::Top,
            theme: ThemeConfig {
                name: "graphene-night".to_owned(),
                light: None,
                dark: None,
                unlocked: Vec::new(),
            },
            keybindings: BTreeMap::new(),
            updates: UpdatesConfig {
                check: true,
                interval: UpdateInterval::Auto,
            },
            notifications: NotificationsConfig {
                program: true,
                command_finished: true,
                command_threshold: 10,
                bell: false,
            },
        }
    }
}

impl Config {
    /// Parses a configuration file. A missing key takes its default value, an invalid
    /// value keeps the one of `previous`, an unknown key is ignored; each problem is
    /// reported as an error. A TOML syntax error returns `previous` unchanged.
    pub fn parse(text: &str, previous: &Config, file: &Path) -> (Config, Vec<ConfigError>) {
        let (config, errors, _) = Self::parse_with_unknown_keys(text, previous, file);
        (config, errors)
    }

    /// Parses a file and returns the unknown keys as structured paths for explicit
    /// cleanup. Value validation errors do not classify a recognized key as unknown.
    pub(crate) fn parse_with_unknown_keys(
        text: &str,
        previous: &Config,
        file: &Path,
    ) -> (Config, Vec<ConfigError>, Vec<UnknownConfigKey>) {
        let mut reader = Reader {
            text,
            file,
            errors: Vec::new(),
            unknown_keys: Vec::new(),
        };
        let root = match DeTable::parse(text) {
            Ok(root) => root.into_inner(),
            Err(error) => {
                reader.error(
                    error.span(),
                    None,
                    ConfigErrorKind::Syntax,
                    error.message().trim_end().to_owned(),
                );
                return (previous.clone(), reader.errors, reader.unknown_keys);
            }
        };

        let mut config = Config::default();
        for (key, value) in root.iter() {
            let name = key.get_ref().as_ref();
            match name {
                "language" => reader.set(
                    value,
                    &[name],
                    name,
                    &mut config.language,
                    &previous.language,
                    Checked::Valid,
                ),
                "keybindings" => reader.error(
                    Some(key.span()),
                    Some(key_path(&[name])),
                    ConfigErrorKind::Moved,
                    format!("`[keybindings]` moved to {KEYBINDINGS_FILE}"),
                ),
                "font" | "window" | "scrollback" | "cursor" | "selection" | "tabs" | "theme"
                | "updates" | "notifications" => {
                    let DeValue::Table(table) = value.get_ref() else {
                        reader.error(
                            Some(value.span()),
                            Some(key_path(&[name])),
                            ConfigErrorKind::InvalidValue,
                            format!("`{name}` must be a table"),
                        );
                        continue;
                    };
                    for (sub_key, sub_value) in table.iter() {
                        reader.read_section_key(name, sub_key, sub_value, &mut config, previous);
                    }
                }
                _ => reader.unknown_key(
                    None,
                    name,
                    matches!(value.get_ref(), DeValue::Table(_)),
                    Some(key.span()),
                    format!("unknown key `{name}`"),
                ),
            }
        }
        // Keys are visited in alphabetical order: report errors in file order.
        reader.errors.sort_by_key(|error| error.line);
        (config, reader.errors, reader.unknown_keys)
    }

    /// Parses `keybindings.toml`: returns the `[keybindings]` entries, the errors and the
    /// unknown keys (only the `keybindings` table is known at the root). An invalid binding
    /// keeps the action of `previous`; a TOML syntax error keeps `previous.keybindings`.
    pub(crate) fn parse_keybindings(
        text: &str,
        previous: &Config,
        file: &Path,
    ) -> (
        BTreeMap<String, String>,
        Vec<ConfigError>,
        Vec<UnknownConfigKey>,
    ) {
        let mut reader = Reader {
            text,
            file,
            errors: Vec::new(),
            unknown_keys: Vec::new(),
        };
        let root = match DeTable::parse(text) {
            Ok(root) => root.into_inner(),
            Err(error) => {
                reader.error(
                    error.span(),
                    None,
                    ConfigErrorKind::Syntax,
                    error.message().trim_end().to_owned(),
                );
                return (
                    previous.keybindings.clone(),
                    reader.errors,
                    reader.unknown_keys,
                );
            }
        };

        let mut config = Config::default();
        for (key, value) in root.iter() {
            let name = key.get_ref().as_ref();
            if name != "keybindings" {
                reader.unknown_key(
                    None,
                    name,
                    matches!(value.get_ref(), DeValue::Table(_)),
                    Some(key.span()),
                    format!("unknown key `{name}`"),
                );
                continue;
            }
            let DeValue::Table(table) = value.get_ref() else {
                reader.error(
                    Some(value.span()),
                    Some(key_path(&[name])),
                    ConfigErrorKind::InvalidValue,
                    format!("`{name}` must be a table"),
                );
                continue;
            };
            for (sub_key, sub_value) in table.iter() {
                reader.read_section_key(name, sub_key, sub_value, &mut config, previous);
            }
        }
        // Keys are visited in alphabetical order: report errors in file order.
        reader.errors.sort_by_key(|error| error.line);
        (config.keybindings, reader.errors, reader.unknown_keys)
    }

    /// The part of the configuration handled by `alacritty_terminal`.
    pub fn term_config(&self) -> TermConfig {
        TermConfig {
            scrolling_history: self.scrollback_lines,
            semantic_escape_chars: self.word_separators.clone(),
            default_cursor_style: CursorStyle {
                shape: self.cursor.shape.into(),
                blinking: self.cursor.blinking,
            },
            // Programs may set the clipboard, never read it: any output (a file shown,
            // a remote host) could otherwise exfiltrate it.
            osc52: Osc52::OnlyCopy,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::term;

    fn file() -> PathBuf {
        PathBuf::from("/home/user/.config/graphene/config.toml")
    }

    #[test]
    fn auto_language_is_the_detected_one() {
        assert_eq!(Language::Auto.resolve(|| Lang::Fr), Lang::Fr);
        assert_eq!(Language::Auto.resolve(|| Lang::En), Lang::En);
    }

    #[test]
    fn forced_language_ignores_the_locale() {
        let detect = || panic!("the locale is not read for a forced language");
        assert_eq!(Language::En.resolve(detect), Lang::En);
        assert_eq!(Language::Fr.resolve(detect), Lang::Fr);
    }

    fn parse(text: &str) -> (Config, Vec<ConfigError>) {
        Config::parse(text, &Config::default(), &file())
    }

    fn keybindings_path() -> PathBuf {
        PathBuf::from("/home/user/.config/graphene/keybindings.toml")
    }

    fn parse_keybindings(
        text: &str,
    ) -> (
        BTreeMap<String, String>,
        Vec<ConfigError>,
        Vec<UnknownConfigKey>,
    ) {
        Config::parse_keybindings(text, &Config::default(), &keybindings_path())
    }

    fn theme(light: Option<&str>, dark: Option<&str>) -> ThemeConfig {
        ThemeConfig {
            name: "gruvbox-dark".to_owned(),
            light: light.map(str::to_owned),
            dark: dark.map(str::to_owned),
            unlocked: Vec::new(),
        }
    }

    #[test]
    fn light_and_dark_pick_the_theme_by_appearance() {
        let theme = theme(Some("graphene-day"), Some("graphene-night"));
        assert!(theme.follows_appearance());
        assert_eq!(theme.active(Appearance::Light), "graphene-day");
        assert_eq!(theme.active(Appearance::Dark), "graphene-night");
    }

    #[test]
    fn without_both_light_and_dark_the_name_is_used() {
        for theme in [
            theme(None, None),
            theme(Some("graphene-day"), None),
            theme(None, Some("graphene-night")),
        ] {
            assert!(!theme.follows_appearance());
            assert_eq!(theme.active(Appearance::Light), "gruvbox-dark");
            assert_eq!(theme.active(Appearance::Dark), "gruvbox-dark");
        }
    }

    /// `config.default.toml` with every commented setting enabled: lines starting with
    /// `# ` are settings, lines starting with `##` are documentation.
    fn uncommented_default() -> String {
        DEFAULT_CONFIG
            .lines()
            .map(|line| line.strip_prefix("# ").unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn choice_values_are_the_accepted_names() {
        for (name, language) in Language::VALUES {
            let (config, errors) = parse(&format!("language = \"{name}\""));
            assert_eq!((config.language, errors), (language, vec![]), "{name}");
        }
        for (name, mode) in StartupMode::VALUES {
            let (config, errors) = parse(&format!("[window]\nstartup_mode = \"{name}\""));
            assert_eq!(
                (config.window.startup_mode, errors),
                (mode, vec![]),
                "{name}"
            );
        }
        for (name, shape) in CursorShape::VALUES {
            let (config, errors) = parse(&format!("[cursor]\nshape = \"{name}\""));
            assert_eq!((config.cursor.shape, errors), (shape, vec![]), "{name}");
        }
        for (name, position) in TabPosition::VALUES {
            let (config, errors) = parse(&format!("[tabs]\nposition = \"{name}\""));
            assert_eq!((config.tab_position, errors), (position, vec![]), "{name}");
        }
    }

    #[test]
    fn escape_chars_are_alacritty_defaults_without_colon() {
        assert_eq!(
            SEMANTIC_ESCAPE_CHARS,
            term::SEMANTIC_ESCAPE_CHARS.replace(':', "")
        );
    }

    #[test]
    fn empty_file_is_the_default() {
        assert_eq!(parse(""), (Config::default(), Vec::new()));
    }

    #[test]
    fn commented_default_file_is_the_default() {
        assert_eq!(parse(DEFAULT_CONFIG), (Config::default(), Vec::new()));
    }

    #[test]
    fn uncommented_default_file_is_the_default() {
        let text = uncommented_default();
        assert!(
            text.contains("\n[font]\n"),
            "settings must be commented with `# `"
        );
        assert_eq!(parse(&text), (Config::default(), Vec::new()));
    }

    #[test]
    fn theme_unlocked_keeps_hidden_themes_and_drops_unknown_names_silently() {
        let (config, errors) = parse("[theme]\nunlocked = [\"phosphor-crt\", \"nonexistent\"]\n");
        assert_eq!(errors, Vec::new());
        assert_eq!(config.theme.unlocked, ["phosphor-crt"]);
    }

    #[test]
    fn theme_unlocked_is_not_documented_in_the_default_file() {
        assert!(!DEFAULT_CONFIG.contains("unlocked"));
    }

    #[test]
    fn every_key_is_read() {
        let text = r#"
language = "fr"

[font]
family = "JetBrains Mono"
size = 11.5
ligatures = false
features = "ss01 -calt cv05=2"

[window]
startup_mode = "remember"
width = 1200
height = 800
padding = 4
background_opacity = 0.8
progress = false

[scrollback]
lines = 5000

[cursor]
shape = "beam"
blinking = true

[selection]
word_separators = " ,"

[tabs]
position = "left"

[theme]
name = "gruvbox-dark"
light = "graphene-day"
dark = "graphene-night"

[updates]
check = false
interval = "2d"

[notifications]
program = false
command_finished = false
command_threshold = 30
bell = true
"#;
        let (config, errors) = parse(text);
        assert_eq!(errors, Vec::new());
        assert_eq!(
            config,
            Config {
                language: Language::Fr,
                font: FontConfig {
                    family: Some("JetBrains Mono".to_owned()),
                    size: 11.5,
                    ligatures: false,
                    features: FontFeatureList(vec![
                        (FeatureTag::new(b"ss01"), 1),
                        (FeatureTag::new(b"calt"), 0),
                        (FeatureTag::new(b"cv05"), 2),
                    ]),
                },
                window: WindowConfig {
                    startup_mode: StartupMode::Remember,
                    width: 1200,
                    height: 800,
                    padding: 4.0,
                    background_opacity: 0.8,
                    progress: false,
                },
                scrollback_lines: 5000,
                cursor: CursorConfig {
                    shape: CursorShape::Beam,
                    blinking: true,
                },
                word_separators: " ,".to_owned(),
                tab_position: TabPosition::Left,
                theme: ThemeConfig {
                    name: "gruvbox-dark".to_owned(),
                    light: Some("graphene-day".to_owned()),
                    dark: Some("graphene-night".to_owned()),
                    unlocked: Vec::new(),
                },
                keybindings: BTreeMap::new(),
                updates: UpdatesConfig {
                    check: false,
                    interval: UpdateInterval::Days2,
                },
                notifications: NotificationsConfig {
                    program: false,
                    command_finished: false,
                    command_threshold: 30,
                    bell: true,
                },
            }
        );
    }

    #[test]
    fn invalid_command_threshold_keeps_the_previous_value() {
        let mut previous = Config::default();
        previous.notifications.command_threshold = 42;
        for value in ["0", "-5", "1.5", "\"ten\""] {
            let text = format!("[notifications]\ncommand_threshold = {value}\n");
            let (config, errors) = Config::parse(&text, &previous, &file());
            assert_eq!(config.notifications.command_threshold, 42, "{value}");
            assert_eq!(errors.len(), 1, "{value}");
            assert_eq!(errors[0].kind, ConfigErrorKind::InvalidValue, "{value}");
            assert_eq!(
                errors[0].key.as_deref(),
                Some("notifications.command_threshold"),
                "{value}"
            );
        }
    }

    #[test]
    fn command_threshold_of_one_second_is_accepted() {
        let (config, errors) = parse("[notifications]\ncommand_threshold = 1\n");
        assert_eq!(errors, Vec::new());
        assert_eq!(config.notifications.command_threshold, 1);
    }

    #[test]
    fn notifications_must_be_a_table() {
        let (config, errors) = parse("notifications = true\n");
        assert_eq!(config.notifications, Config::default().notifications);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ConfigErrorKind::InvalidValue);
    }

    #[test]
    fn empty_font_family_is_the_system_font() {
        let (config, errors) = parse("[font]\nfamily = \"\"\n");
        assert!(errors.is_empty());
        assert_eq!(config.font.family, None);
    }

    fn features(text: &str) -> Vec<(String, u32)> {
        FontFeatureList::parse(text)
            .unwrap()
            .0
            .into_iter()
            .map(|(tag, value)| (String::from_utf8_lossy(tag.as_bytes()).into_owned(), value))
            .collect()
    }

    #[test]
    fn font_features_accept_every_syntax_and_separator() {
        assert_eq!(
            features("ss01 +zero,-calt  cv05=2,,"),
            [
                ("ss01".to_owned(), 1),
                ("zero".to_owned(), 1),
                ("calt".to_owned(), 0),
                ("cv05".to_owned(), 2),
            ]
        );
        assert_eq!(features(""), []);
        assert_eq!(features(" , "), []);
    }

    #[test]
    fn last_occurrence_of_a_font_feature_wins() {
        assert_eq!(
            features("calt ss01 -calt"),
            [("ss01".to_owned(), 1), ("calt".to_owned(), 0)]
        );
    }

    #[test]
    fn invalid_font_feature_is_named_in_the_error() {
        for item in ["ss1", "ss012", "+ss01=2", "cv05=x", "cv05=-1", "zéro", "-"] {
            let error = FontFeatureList::parse(&format!("ss01 {item}")).unwrap_err();
            assert!(error.contains(&format!("\"{item}\"")), "{item}: {error}");
        }
    }

    #[test]
    fn font_features_display_parses_back_to_the_same_list() {
        let list = FontFeatureList::parse("+ss01, -calt cv05=2 zero=1").unwrap();
        assert_eq!(list.to_string(), "ss01 -calt cv05=2 zero");
        assert_eq!(FontFeatureList::parse(&list.to_string()), Ok(list));
    }

    #[test]
    fn font_features_convert_to_cosmic_text_features() {
        let list = FontFeatureList::parse("ss01 -calt").unwrap();
        let features = list.font_features().features;
        assert_eq!(features.len(), 2);
        assert_eq!(
            (features[0].tag, features[0].value),
            (FeatureTag::new(b"ss01"), 1)
        );
        assert_eq!(
            (features[1].tag, features[1].value),
            (FeatureTag::new(b"calt"), 0)
        );
    }

    #[test]
    fn invalid_font_features_keep_the_previous_value() {
        let mut previous = Config::default();
        previous.font.features = FontFeatureList::parse("ss01").unwrap();
        let (config, errors) =
            Config::parse("[font]\nfeatures = \"zero bad\"\n", &previous, &file());
        assert_eq!(config.font.features, previous.font.features);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ConfigErrorKind::InvalidValue);
        assert_eq!(errors[0].key.as_deref(), Some("font.features"));
        assert!(
            errors[0].message.contains("\"bad\""),
            "{}",
            errors[0].message
        );
    }

    #[test]
    fn invalid_ligatures_keep_the_previous_value() {
        let mut previous = Config::default();
        previous.font.ligatures = false;
        let (config, errors) = Config::parse("[font]\nligatures = \"no\"\n", &previous, &file());
        assert!(!config.font.ligatures);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].key.as_deref(), Some("font.ligatures"));
    }

    #[test]
    fn window_progress_defaults_to_true() {
        let (config, errors) = parse("");
        assert!(errors.is_empty());
        assert!(config.window.progress);
        let (config, errors) = parse("[window]\nprogress = false\n");
        assert!(errors.is_empty());
        assert!(!config.window.progress);
    }

    #[test]
    fn invalid_window_progress_keeps_the_previous_value() {
        let mut previous = Config::default();
        previous.window.progress = false;
        let (config, errors) = Config::parse("[window]\nprogress = 1\n", &previous, &file());
        assert!(!config.window.progress);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].key.as_deref(), Some("window.progress"));
    }

    #[test]
    fn missing_key_takes_the_default_not_the_previous_value() {
        let mut previous = Config::default();
        previous.font.size = 20.0;
        let (config, errors) = Config::parse("[window]\npadding = 2\n", &previous, &file());
        assert!(errors.is_empty());
        assert_eq!(config.font.size, DEFAULT_FONT_SIZE);
        assert_eq!(config.window.padding, 2.0);
    }

    #[test]
    fn unknown_keys_are_reported_with_their_line() {
        let (config, errors) = parse("colour = 1\n\n[font]\nsise = 12\nsize = 12\n[nope]\n");
        assert_eq!(config.font.size, 12.0);
        let lines: Vec<_> = errors
            .iter()
            .map(|e| (e.line, e.message.as_str()))
            .collect();
        assert_eq!(
            lines,
            vec![
                (Some(1), "unknown key `colour`"),
                (Some(4), "unknown key `font.sise`"),
                (Some(6), "unknown key `nope`"),
            ]
        );
    }

    #[test]
    fn startup_mode_is_read() {
        for (name, mode) in [
            ("default", StartupMode::Default),
            ("maximized", StartupMode::Maximized),
            ("remember", StartupMode::Remember),
        ] {
            let text = format!("[window]\nstartup_mode = \"{name}\"\n");
            let (config, errors) = parse(&text);
            assert!(errors.is_empty());
            assert_eq!(config.window.startup_mode, mode);
        }
    }

    #[test]
    fn unknown_startup_mode_keeps_the_previous_value() {
        let mut previous = Config::default();
        previous.window.startup_mode = StartupMode::Maximized;
        let text = "[window]\nstartup_mode = \"fullscreen\"\n";
        let (config, errors) = Config::parse(text, &previous, &file());
        assert_eq!(config.window.startup_mode, StartupMode::Maximized);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, Some(2));
        assert!(
            errors[0]
                .message
                .starts_with("invalid value for `window.startup_mode`")
        );
    }

    #[test]
    fn invalid_value_keeps_the_previous_value() {
        let mut previous = Config::default();
        previous.cursor.shape = CursorShape::Underline;
        previous.window.width = 700;
        let text = "[window]\nwidth = -3\nheight = 500\n\n[cursor]\nshape = \"triangle\"\n";
        let (config, errors) = Config::parse(text, &previous, &file());
        assert_eq!(config.window.width, 700);
        assert_eq!(config.window.height, 500);
        assert_eq!(config.cursor.shape, CursorShape::Underline);
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].line, Some(2));
        assert!(
            errors[0]
                .message
                .starts_with("invalid value for `window.width`")
        );
        assert_eq!(errors[1].line, Some(6));
        assert!(
            errors[1]
                .message
                .starts_with("invalid value for `cursor.shape`")
        );
        assert_eq!(errors[1].file, file());
    }

    #[test]
    fn wrong_type_is_an_invalid_value() {
        let (config, errors) =
            parse("[cursor]\nblinking = \"yes\"\n[selection]\nword_separators = 3\n");
        assert_eq!(config, Config::default());
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn section_that_is_not_a_table_is_an_error() {
        let (config, errors) = parse("font = 12\n");
        assert_eq!(config, Config::default());
        assert_eq!(errors[0].line, Some(1));
        assert_eq!(errors[0].message, "`font` must be a table");
    }

    fn keys(errors: &[ConfigError]) -> Vec<(Option<&str>, ConfigErrorKind)> {
        errors
            .iter()
            .map(|error| (error.key.as_deref(), error.kind))
            .collect()
    }

    #[test]
    fn invalid_value_error_carries_its_key_path() {
        let (_, errors) = parse("[window]\nwidth = 0\n");
        assert_eq!(
            keys(&errors),
            vec![(Some("window.width"), ConfigErrorKind::InvalidValue)]
        );
    }

    #[test]
    fn clamped_value_error_carries_its_key_path() {
        let (_, errors) = parse("[font]\nsize = 200\n");
        assert_eq!(
            keys(&errors),
            vec![(Some("font.size"), ConfigErrorKind::InvalidValue)]
        );
    }

    #[test]
    fn unknown_keys_carry_their_key_path() {
        let (_, errors) = parse("colour = 1\n[font]\nsise = 12\n[nope]\n");
        assert_eq!(
            keys(&errors),
            vec![
                (Some("colour"), ConfigErrorKind::UnknownKey),
                (Some("font.sise"), ConfigErrorKind::UnknownKey),
                (Some("nope"), ConfigErrorKind::UnknownKey),
            ]
        );
    }

    #[test]
    fn keybinding_error_carries_a_quoted_key_path() {
        let (_, errors, _) = parse_keybindings("[keybindings]\n\"Ctrl+Q\" = \"not_an_action\"\n");
        assert_eq!(
            keys(&errors),
            vec![(
                Some("keybindings.\"Ctrl+Q\""),
                ConfigErrorKind::InvalidValue
            )]
        );
        // The banner text keeps the unquoted path.
        assert_eq!(
            errors[0].to_string(),
            "keybindings.toml:2: invalid value for `keybindings.Ctrl+Q`: unknown action \"not_an_action\""
        );
    }

    #[test]
    fn section_that_is_not_a_table_carries_its_key_path() {
        let (_, errors) = parse("font = 1\n");
        assert_eq!(
            keys(&errors),
            vec![(Some("font"), ConfigErrorKind::InvalidValue)]
        );
    }

    #[test]
    fn syntax_error_has_no_key_path() {
        let (_, errors) = parse("[font]\nsize = 12\nfamily = \n");
        assert_eq!(keys(&errors), vec![(None, ConfigErrorKind::Syntax)]);
    }

    #[test]
    fn read_error_has_no_key_path() {
        let error = ConfigError::read(file(), "cannot read the file: denied".to_owned());
        assert_eq!(
            (error.key.as_deref(), error.kind),
            (None, ConfigErrorKind::Read)
        );
        assert_eq!(
            error.to_string(),
            "config.toml: cannot read the file: denied"
        );
    }

    #[test]
    fn key_path_quotes_segments_that_are_not_bare_keys() {
        assert_eq!(key_path(&["font", "size"]), "font.size");
        assert_eq!(
            key_path(&["keybindings", "Ctrl+X"]),
            "keybindings.\"Ctrl+X\""
        );
        assert_eq!(key_path(&["a", "b c", ""]), "a.\"b c\".\"\"");
        assert_eq!(key_path(&["k", "q\"u\\o"]), "k.\"q\\\"u\\\\o\"");
    }

    #[test]
    fn syntax_error_keeps_the_whole_previous_config() {
        let mut previous = Config::default();
        previous.font.size = 18.0;
        let (config, errors) = Config::parse("[font]\nsize = 12\nfamily = \n", &previous, &file());
        assert_eq!(config, previous);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, Some(3));
    }

    #[test]
    fn font_size_is_clamped() {
        let (config, errors) = parse("[font]\nsize = 200\n");
        assert_eq!(config.font.size, MAX_FONT_SIZE);
        assert_eq!(errors.len(), 1);
        let (config, _) = parse("[font]\nsize = 1\n");
        assert_eq!(config.font.size, MIN_FONT_SIZE);
    }

    #[test]
    fn background_opacity_is_clamped() {
        let (config, errors) = parse("[window]\nbackground_opacity = 1.5\n");
        assert_eq!(config.window.background_opacity, 1.0);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("1.5 is out of 0.0..=1.0"));
        let (config, errors) = parse("[window]\nbackground_opacity = -0.5\n");
        assert_eq!(config.window.background_opacity, 0.0);
        assert_eq!(errors.len(), 1);
        let (config, errors) = parse("[window]\nbackground_opacity = 0\n");
        assert_eq!(config.window.background_opacity, 0.0);
        assert!(errors.is_empty());
    }

    #[test]
    fn non_finite_background_opacity_keeps_the_previous_value() {
        let mut previous = Config::default();
        previous.window.background_opacity = 0.7;
        for value in ["nan", "inf"] {
            let text = format!("[window]\nbackground_opacity = {value}\n");
            let (config, errors) = Config::parse(&text, &previous, &file());
            assert_eq!(config.window.background_opacity, 0.7);
            assert_eq!(errors.len(), 1);
            assert!(
                errors[0].message.contains("not a number"),
                "{}",
                errors[0].message
            );
        }
    }

    #[test]
    fn scrollback_is_capped() {
        let (config, errors) = parse("[scrollback]\nlines = 1000000\n");
        assert_eq!(config.scrollback_lines, MAX_SCROLLBACK_LINES);
        assert_eq!(errors.len(), 1);
        let (config, errors) = parse("[scrollback]\nlines = 0\n");
        assert_eq!(config.scrollback_lines, 0);
        assert!(errors.is_empty());
    }

    #[test]
    fn sizes_must_not_be_negative() {
        let (config, errors) = parse("[window]\npadding = -1\nheight = 0\n");
        assert_eq!(config.window, Config::default().window);
        assert_eq!(errors.len(), 2);
    }

    pub(super) fn previous_with_shortcut() -> Config {
        let mut previous = Config::default();
        previous
            .keybindings
            .insert("Ctrl+Q".to_owned(), "copy".to_owned());
        previous
    }

    #[test]
    fn invalid_keybinding_keeps_the_previous_action() {
        let previous = previous_with_shortcut();
        let text = "[keybindings]\n\"Ctrl+Q\" = 1\n\"Ctrl+W\" = true\n";
        let (keybindings, errors, _) =
            Config::parse_keybindings(text, &previous, &keybindings_path());
        assert_eq!(keybindings, previous.keybindings);
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn unknown_keybinding_keys_or_actions_are_reported() {
        let previous = previous_with_shortcut();
        let text = "[keybindings]\n\"Ctrl+Q\" = \"not_an_action\"\n\"Ctrl+PgUp\" = \"copy\"\n\"Ctrl+Alt+T\" = \"new_tab\"\n";
        let (keybindings, errors, unknown) =
            Config::parse_keybindings(text, &previous, &keybindings_path());
        assert_eq!(
            keybindings,
            BTreeMap::from([
                ("Ctrl+Q".to_owned(), "copy".to_owned()),
                ("Ctrl+Alt+T".to_owned(), "new_tab".to_owned()),
            ])
        );
        assert!(unknown.is_empty());
        let messages: Vec<String> = errors.iter().map(ToString::to_string).collect();
        assert_eq!(
            messages,
            [
                "keybindings.toml:2: invalid value for `keybindings.Ctrl+Q`: unknown action \"not_an_action\"",
                "keybindings.toml:3: invalid value for `keybindings.Ctrl+PgUp`: unknown key \"PgUp\" in \"Ctrl+PgUp\"",
            ]
        );
    }

    #[test]
    fn keybindings_file_syntax_error_keeps_the_previous_shortcuts() {
        let previous = previous_with_shortcut();
        let (keybindings, errors, _) =
            Config::parse_keybindings("[keybindings\n", &previous, &keybindings_path());
        assert_eq!(keybindings, previous.keybindings);
        assert_eq!(keys(&errors), vec![(None, ConfigErrorKind::Syntax)]);
        assert_eq!(errors[0].file, keybindings_path());
    }

    #[test]
    fn keybindings_file_only_knows_the_keybindings_table() {
        let text = "language = \"fr\"\n[font]\nsize = 12\n[keybindings]\n\"F7\" = \"copy\"\n";
        let (keybindings, errors, unknown) =
            Config::parse_keybindings(text, &Config::default(), &keybindings_path());
        assert_eq!(
            keybindings,
            BTreeMap::from([("F7".to_owned(), "copy".to_owned())])
        );
        let paths: Vec<_> = unknown.iter().map(|key| key.path.as_str()).collect();
        assert_eq!(paths, ["font", "language"]);
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|e| e.kind == ConfigErrorKind::UnknownKey));
    }

    #[test]
    fn keybindings_section_of_config_file_is_moved_not_read() {
        let (config, errors, unknown) = Config::parse_with_unknown_keys(
            "[keybindings]\n\"F7\" = \"copy\"\n[font]\nsize = 12\n",
            &Config::default(),
            &file(),
        );
        assert!(config.keybindings.is_empty());
        assert_eq!(config.font.size, 12.0);
        assert!(unknown.is_empty(), "prune-config must keep the shortcuts");
        assert_eq!(
            keys(&errors),
            vec![(Some("keybindings"), ConfigErrorKind::Moved)]
        );
        assert_eq!(errors[0].line, Some(1));
        assert_eq!(
            errors[0].to_string(),
            "config.toml:1: `[keybindings]` moved to keybindings.toml"
        );
    }

    #[test]
    fn error_display_shows_file_name_and_line() {
        let (_, errors) = parse("\n[font]\nsise = 1\n");
        assert_eq!(
            errors[0].to_string(),
            "config.toml:3: unknown key `font.sise`"
        );
    }

    #[test]
    fn term_config_carries_the_terminal_settings() {
        let (config, _) = parse(
            "[scrollback]\nlines = 42\n[cursor]\nshape = \"underline\"\nblinking = true\n[selection]\nword_separators = \" \"\n",
        );
        let term = config.term_config();
        assert_eq!(term.scrolling_history, 42);
        assert_eq!(term.semantic_escape_chars, " ");
        assert_eq!(term.default_cursor_style.shape, TermCursorShape::Underline);
        assert!(term.default_cursor_style.blinking);
        assert_eq!(term.osc52, Osc52::OnlyCopy);
    }

    #[test]
    fn every_update_interval_is_accepted() {
        for (name, interval) in UpdateInterval::VALUES {
            let (config, errors) = parse(&format!("[updates]\ninterval = \"{name}\"\n"));
            assert!(errors.is_empty(), "{name}: {errors:?}");
            assert_eq!(config.updates.interval, interval);
        }
    }

    #[test]
    fn invalid_update_interval_falls_back_to_auto() {
        let mut previous = Config::default();
        previous.updates.interval = UpdateInterval::Week1;
        let (config, errors) = Config::parse("[updates]\ninterval = \"5h\"\n", &previous, &file());
        assert_eq!(config.updates.interval, UpdateInterval::Auto);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, Some(2));
        assert!(
            errors[0]
                .message
                .starts_with("`updates.interval`: invalid value \"5h\""),
            "{}",
            errors[0].message
        );
    }
}
