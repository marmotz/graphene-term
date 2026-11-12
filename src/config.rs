//! User configuration: `config.toml` in the config directory, read key by key so that one
//! invalid value or unknown key never discards the rest of the file.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};

use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::vte::ansi::{CursorShape as TermCursorShape, CursorStyle};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use toml::Spanned;
use toml::de::{DeTable, DeValue, ValueDeserializer};

/// The fully commented default configuration, written on first launch.
pub const DEFAULT_CONFIG: &str = include_str!("../config.default.toml");

/// Name of the configuration file inside the config directory.
pub const CONFIG_FILE: &str = "config.toml";

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

/// `$XDG_CONFIG_HOME/graphene`, else `$HOME/.config/graphene`.
pub fn config_dir() -> Option<PathBuf> {
    config_dir_from(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

fn config_dir_from(xdg_config_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    // The XDG spec ignores an empty or relative `XDG_CONFIG_HOME`.
    let xdg = xdg_config_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let base = xdg.or_else(|| {
        home.filter(|home| !home.is_empty())
            .map(|home| PathBuf::from(home).join(".config"))
    })?;
    Some(base.join("graphene"))
}

/// Loads `config.toml` from the config directory. A missing file is created from
/// [`DEFAULT_CONFIG`] (a write failure is only logged) and yields the default.
pub fn load() -> (Config, Vec<ConfigError>) {
    match config_dir() {
        Some(dir) => load_from(&dir),
        None => {
            log::warn!("neither XDG_CONFIG_HOME nor HOME is set: using the default configuration");
            (Config::default(), Vec::new())
        }
    }
}

fn load_from(dir: &Path) -> (Config, Vec<ConfigError>) {
    let file = dir.join(CONFIG_FILE);
    match std::fs::read_to_string(&file) {
        Ok(text) => Config::parse(&text, &Config::default(), &file),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Err(error) =
                std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&file, DEFAULT_CONFIG))
            {
                log::warn!("cannot write {}: {error}", file.display());
            }
            (Config::default(), Vec::new())
        }
        Err(error) => (
            Config::default(),
            vec![ConfigError {
                file,
                line: None,
                message: format!("cannot read the file: {error}"),
            }],
        ),
    }
}

/// An error found in a configuration file.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigError {
    pub file: PathBuf,
    /// 1-based line of the faulty key or value, when known.
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.file.file_name().map_or_else(
            || self.file.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        match self.line {
            Some(line) => write!(f, "{name}:{line}: {}", self.message),
            None => write!(f, "{name}: {}", self.message),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Follows `LANG`.
    #[default]
    Auto,
    En,
    Fr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorShape {
    #[default]
    Block,
    Beam,
    Underline,
}

impl From<CursorShape> for TermCursorShape {
    fn from(shape: CursorShape) -> Self {
        match shape {
            CursorShape::Block => Self::Block,
            CursorShape::Beam => Self::Beam,
            CursorShape::Underline => Self::Underline,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FontConfig {
    /// `None` uses the system monospace font.
    pub family: Option<String>,
    pub size: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowConfig {
    /// Initial size, in logical pixels.
    pub width: u32,
    pub height: u32,
    /// Minimum padding around the grid, in logical pixels.
    pub padding: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CursorConfig {
    pub shape: CursorShape,
    pub blinking: bool,
}

/// `updates.interval`: time between two update checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateInterval {
    /// 6 hours for a dev build, 24 hours for a stable one.
    #[default]
    Auto,
    Hours6,
    Hours12,
    Hours24,
    Days2,
    Days3,
    Week1,
}

impl UpdateInterval {
    /// Accepted values, in the order shown to the user.
    pub const VALUES: [(&'static str, UpdateInterval); 7] = [
        ("auto", UpdateInterval::Auto),
        ("6h", UpdateInterval::Hours6),
        ("12h", UpdateInterval::Hours12),
        ("24h", UpdateInterval::Hours24),
        ("2d", UpdateInterval::Days2),
        ("3d", UpdateInterval::Days3),
        ("1w", UpdateInterval::Week1),
    ];

    pub fn parse(value: &str) -> Option<Self> {
        Self::VALUES
            .iter()
            .find(|(name, _)| *name == value)
            .map(|&(_, interval)| interval)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UpdatesConfig {
    /// `false`: no network call at all.
    pub check: bool,
    pub interval: UpdateInterval,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThemeConfig {
    pub name: String,
    #[allow(dead_code, reason = "applied by the light/dark appearance task")]
    pub light: Option<String>,
    #[allow(dead_code, reason = "applied by the light/dark appearance task")]
    pub dark: Option<String>,
}

/// The user configuration. `Default` holds the built-in values, so a missing or empty
/// file behaves like no configuration at all.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    #[allow(dead_code, reason = "applied by the language task")]
    pub language: Language,
    pub font: FontConfig,
    pub window: WindowConfig,
    pub scrollback_lines: usize,
    pub cursor: CursorConfig,
    pub word_separators: String,
    #[allow(dead_code, reason = "applied by the tab bar position task")]
    pub tab_position: TabPosition,
    pub theme: ThemeConfig,
    /// Raw `"keys" = "action"` pairs, interpreted by the keybindings task.
    #[allow(dead_code, reason = "applied by the keybindings task")]
    pub keybindings: BTreeMap<String, String>,
    #[allow(dead_code, reason = "applied by the update check thread task")]
    pub updates: UpdatesConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: Language::Auto,
            font: FontConfig {
                family: None,
                size: DEFAULT_FONT_SIZE,
            },
            window: WindowConfig {
                width: 960,
                height: 600,
                padding: 8.0,
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
            },
            keybindings: BTreeMap::new(),
            updates: UpdatesConfig {
                check: true,
                interval: UpdateInterval::Auto,
            },
        }
    }
}

/// Result of validating a value read from the file.
enum Checked<T> {
    Valid(T),
    /// Out of range: brought back into range, with an error.
    Clamped(T, String),
    /// Rejected: the previous value stays, with an error.
    Invalid(String),
}

impl Config {
    /// Parses a configuration file. A missing key takes its default value, an invalid
    /// value keeps the one of `previous`, an unknown key is ignored; each problem is
    /// reported as an error. A TOML syntax error returns `previous` unchanged.
    pub fn parse(text: &str, previous: &Config, file: &Path) -> (Config, Vec<ConfigError>) {
        let mut reader = Reader {
            text,
            file,
            errors: Vec::new(),
        };
        let root = match DeTable::parse(text) {
            Ok(root) => root.into_inner(),
            Err(error) => {
                reader.error(error.span(), error.message().trim_end().to_owned());
                return (previous.clone(), reader.errors);
            }
        };

        let mut config = Config::default();
        for (key, value) in root.iter() {
            let name = key.get_ref().as_ref();
            match name {
                "language" => reader.set(
                    value,
                    name,
                    &mut config.language,
                    &previous.language,
                    Checked::Valid,
                ),
                "font" | "window" | "scrollback" | "cursor" | "selection" | "tabs" | "theme"
                | "keybindings" | "updates" => {
                    let DeValue::Table(table) = value.get_ref() else {
                        reader.error(Some(value.span()), format!("`{name}` must be a table"));
                        continue;
                    };
                    for (sub_key, sub_value) in table.iter() {
                        reader.read_section_key(name, sub_key, sub_value, &mut config, previous);
                    }
                }
                _ => reader.error(Some(key.span()), format!("unknown key `{name}`")),
            }
        }
        // Keys are visited in alphabetical order: report errors in file order.
        reader.errors.sort_by_key(|error| error.line);
        (config, reader.errors)
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
            ..Default::default()
        }
    }
}

struct Reader<'a> {
    text: &'a str,
    file: &'a Path,
    errors: Vec<ConfigError>,
}

impl Reader<'_> {
    fn error(&mut self, span: Option<Range<usize>>, message: String) {
        let line = span.map(|span| {
            let end = span.start.min(self.text.len());
            self.text.as_bytes()[..end]
                .iter()
                .filter(|&&b| b == b'\n')
                .count()
                + 1
        });
        self.errors.push(ConfigError {
            file: self.file.to_path_buf(),
            line,
            message,
        });
    }

    /// Deserializes `value` as `T`, validates it, then stores it in `target`; on error
    /// `target` gets `previous`.
    fn set<T: DeserializeOwned, U: Clone>(
        &mut self,
        value: &Spanned<DeValue<'_>>,
        path: &str,
        target: &mut U,
        previous: &U,
        check: impl FnOnce(T) -> Checked<U>,
    ) {
        let checked = match T::deserialize(ValueDeserializer::from(value.clone())) {
            Ok(raw) => check(raw),
            Err(error) => Checked::Invalid(error.message().trim_end().to_owned()),
        };
        match checked {
            Checked::Valid(v) => *target = v,
            Checked::Clamped(v, message) => {
                self.error(Some(value.span()), format!("`{path}`: {message}"));
                *target = v;
            }
            Checked::Invalid(message) => {
                self.error(
                    Some(value.span()),
                    format!("invalid value for `{path}`: {message}"),
                );
                *target = previous.clone();
            }
        }
    }

    fn read_section_key(
        &mut self,
        section: &str,
        key: &Spanned<std::borrow::Cow<'_, str>>,
        value: &Spanned<DeValue<'_>>,
        config: &mut Config,
        previous: &Config,
    ) {
        let name = key.get_ref().as_ref();
        let path = format!("{section}.{name}");
        let path = path.as_str();
        match (section, name) {
            ("font", "family") => self.set(value, path, &mut config.font.family, &previous.font.family, |family: String| {
                Checked::Valid(Some(family).filter(|f| !f.is_empty()))
            }),
            ("font", "size") => self.set(value, path, &mut config.font.size, &previous.font.size, |size: f32| {
                if !size.is_finite() {
                    Checked::Invalid("not a number".to_owned())
                } else if (MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&size) {
                    Checked::Valid(size)
                } else {
                    let clamped = size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
                    Checked::Clamped(clamped, format!("{size} is out of {MIN_FONT_SIZE}..={MAX_FONT_SIZE}, using {clamped}"))
                }
            }),
            ("window", "width") => self.set(value, path, &mut config.window.width, &previous.window.width, positive_size),
            ("window", "height") => self.set(value, path, &mut config.window.height, &previous.window.height, positive_size),
            ("window", "padding") => self.set(value, path, &mut config.window.padding, &previous.window.padding, |padding: f32| {
                if padding.is_finite() && padding >= 0.0 {
                    Checked::Valid(padding)
                } else {
                    Checked::Invalid("must be a non-negative number".to_owned())
                }
            }),
            ("scrollback", "lines") => self.set(value, path, &mut config.scrollback_lines, &previous.scrollback_lines, |lines: usize| {
                if lines <= MAX_SCROLLBACK_LINES {
                    Checked::Valid(lines)
                } else {
                    Checked::Clamped(MAX_SCROLLBACK_LINES, format!("{lines} is above the maximum, using {MAX_SCROLLBACK_LINES}"))
                }
            }),
            ("cursor", "shape") => self.set(value, path, &mut config.cursor.shape, &previous.cursor.shape, Checked::Valid),
            ("cursor", "blinking") => self.set(value, path, &mut config.cursor.blinking, &previous.cursor.blinking, Checked::Valid),
            ("selection", "word_separators") => self.set(value, path, &mut config.word_separators, &previous.word_separators, Checked::Valid),
            ("tabs", "position") => self.set(value, path, &mut config.tab_position, &previous.tab_position, Checked::Valid),
            ("theme", "name") => self.set(value, path, &mut config.theme.name, &previous.theme.name, non_empty),
            ("theme", "light") => self.set(value, path, &mut config.theme.light, &previous.theme.light, |name: String| non_empty(name).map(Some)),
            ("theme", "dark") => self.set(value, path, &mut config.theme.dark, &previous.theme.dark, |name: String| non_empty(name).map(Some)),
            ("updates", "check") => self.set(value, path, &mut config.updates.check, &previous.updates.check, Checked::Valid),
            ("updates", "interval") => self.set(value, path, &mut config.updates.interval, &previous.updates.interval, |interval: String| {
                match UpdateInterval::parse(&interval) {
                    Some(interval) => Checked::Valid(interval),
                    None => {
                        let names: Vec<_> = UpdateInterval::VALUES.iter().map(|(name, _)| format!("\"{name}\"")).collect();
                        Checked::Clamped(UpdateInterval::Auto, format!("invalid value \"{interval}\", expected one of {}; using \"auto\"", names.join(", ")))
                    }
                }
            }),
            ("keybindings", keys) => {
                let previous_action = previous.keybindings.get(keys).cloned();
                let mut action = previous_action.clone();
                self.set(value, path, &mut action, &previous_action, |action: String| Checked::Valid(Some(action)));
                if let Some(action) = action {
                    config.keybindings.insert(keys.to_owned(), action);
                }
            }
            _ => self.error(Some(key.span()), format!("unknown key `{path}`")),
        }
    }
}

impl<T> Checked<T> {
    fn map<U>(self, f: impl FnOnce(T) -> U) -> Checked<U> {
        match self {
            Checked::Valid(v) => Checked::Valid(f(v)),
            Checked::Clamped(v, message) => Checked::Clamped(f(v), message),
            Checked::Invalid(message) => Checked::Invalid(message),
        }
    }
}

fn positive_size(size: u32) -> Checked<u32> {
    if size > 0 {
        Checked::Valid(size)
    } else {
        Checked::Invalid("must be greater than 0".to_owned())
    }
}

fn non_empty(name: String) -> Checked<String> {
    if name.is_empty() {
        Checked::Invalid("must not be empty".to_owned())
    } else {
        Checked::Valid(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::term;

    fn file() -> PathBuf {
        PathBuf::from("/home/user/.config/graphene/config.toml")
    }

    fn parse(text: &str) -> (Config, Vec<ConfigError>) {
        Config::parse(text, &Config::default(), &file())
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
    fn escape_chars_are_alacritty_defaults_without_colon() {
        assert_eq!(
            SEMANTIC_ESCAPE_CHARS,
            term::SEMANTIC_ESCAPE_CHARS.replace(':', "")
        );
    }

    #[test]
    fn config_dir_prefers_xdg_config_home() {
        assert_eq!(
            config_dir_from(Some("/xdg".into()), Some("/home/u".into())),
            Some(PathBuf::from("/xdg/graphene"))
        );
    }

    #[test]
    fn config_dir_falls_back_to_home() {
        let expected = Some(PathBuf::from("/home/u/.config/graphene"));
        assert_eq!(config_dir_from(None, Some("/home/u".into())), expected);
        assert_eq!(
            config_dir_from(Some("".into()), Some("/home/u".into())),
            expected
        );
        assert_eq!(
            config_dir_from(Some("relative".into()), Some("/home/u".into())),
            expected
        );
        assert_eq!(config_dir_from(None, None), None);
        assert_eq!(config_dir_from(None, Some("".into())), None);
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
    fn every_key_is_read() {
        let text = r#"
language = "fr"

[font]
family = "JetBrains Mono"
size = 11.5

[window]
width = 1200
height = 800
padding = 4

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

[keybindings]
"Ctrl+Shift+C" = "copy"
"Shift+PageUp" = "none"

[updates]
check = false
interval = "2d"
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
                },
                window: WindowConfig {
                    width: 1200,
                    height: 800,
                    padding: 4.0,
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
                },
                keybindings: BTreeMap::from([
                    ("Ctrl+Shift+C".to_owned(), "copy".to_owned()),
                    ("Shift+PageUp".to_owned(), "none".to_owned()),
                ]),
                updates: UpdatesConfig {
                    check: false,
                    interval: UpdateInterval::Days2,
                },
            }
        );
    }

    #[test]
    fn empty_font_family_is_the_system_font() {
        let (config, errors) = parse("[font]\nfamily = \"\"\n");
        assert!(errors.is_empty());
        assert_eq!(config.font.family, None);
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

    #[test]
    fn invalid_keybinding_keeps_the_previous_action() {
        let mut previous = Config::default();
        previous
            .keybindings
            .insert("Ctrl+Q".to_owned(), "quit".to_owned());
        let text = "[keybindings]\n\"Ctrl+Q\" = 1\n\"Ctrl+W\" = true\n";
        let (config, errors) = Config::parse(text, &previous, &file());
        assert_eq!(config.keybindings, previous.keybindings);
        assert_eq!(errors.len(), 2);
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
    }

    #[test]
    fn missing_file_is_created_from_the_default() {
        let dir = std::env::temp_dir().join(format!("graphene-config-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let nested = dir.join("graphene");

        assert_eq!(load_from(&nested), (Config::default(), Vec::new()));
        let written = std::fs::read_to_string(nested.join(CONFIG_FILE)).unwrap();
        assert_eq!(written, DEFAULT_CONFIG);

        std::fs::write(nested.join(CONFIG_FILE), "[font]\nsize = 9\n").unwrap();
        assert_eq!(load_from(&nested).0.font.size, 9.0);

        std::fs::remove_dir_all(&dir).unwrap();
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
