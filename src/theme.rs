//! Color themes: TOML files read key by key into a [`Palette`], built-in themes embedded
//! in the binary, and resolution of a theme name.
//!
//! A theme file has a `[colors]` table (with `[colors.normal]` and `[colors.bright]` for
//! the 16 ANSI colors) and a `[ui]` table. A missing or invalid color keeps the value of
//! the base palette (`graphene-night`), so a partial user theme still works.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use alacritty_terminal::vte::ansi::Rgb;
use toml::Spanned;
use toml::de::{DeTable, DeValue};

use crate::config::{CONFIG_FILE, ConfigError, ConfigErrorKind};
use crate::palette::{Palette, build_indexed};

/// Theme used when the configured one cannot be found.
pub const DEFAULT_THEME: &str = "graphene-night";

/// Subdirectory of the config directory holding user themes.
pub const THEMES_DIR: &str = "themes";

/// Built-in themes, by name.
pub const BUILT_IN: &[(&str, &str)] = &[
    (
        "graphene-night",
        include_str!("../themes/graphene-night.toml"),
    ),
    ("graphene-day", include_str!("../themes/graphene-day.toml")),
    (
        "solarized-dark",
        include_str!("../themes/solarized-dark.toml"),
    ),
    (
        "solarized-light",
        include_str!("../themes/solarized-light.toml"),
    ),
    ("gruvbox-dark", include_str!("../themes/gruvbox-dark.toml")),
    (
        "gruvbox-light",
        include_str!("../themes/gruvbox-light.toml"),
    ),
    (
        "catppuccin-mocha",
        include_str!("../themes/catppuccin-mocha.toml"),
    ),
    (
        "catppuccin-latte",
        include_str!("../themes/catppuccin-latte.toml"),
    ),
];

/// Reference (`&` or `&mut`) to the required color a [`ColorKey`] stores in a
/// [`Palette`]; `None` for the optional selection foreground.
macro_rules! color_slot {
    ($key:expr, $palette:expr, $($r:tt)+) => {{
        let palette = $palette;
        match $key {
            ColorKey::Foreground => Some($($r)+ palette.foreground),
            ColorKey::Background => Some($($r)+ palette.background),
            ColorKey::Cursor => Some($($r)+ palette.cursor),
            ColorKey::CursorText => Some($($r)+ palette.cursor_text),
            ColorKey::SelectionBackground => Some($($r)+ palette.selection_background),
            ColorKey::SelectionForeground => None,
            ColorKey::NormalBlack => Some($($r)+ palette.indexed[0]),
            ColorKey::NormalRed => Some($($r)+ palette.indexed[1]),
            ColorKey::NormalGreen => Some($($r)+ palette.indexed[2]),
            ColorKey::NormalYellow => Some($($r)+ palette.indexed[3]),
            ColorKey::NormalBlue => Some($($r)+ palette.indexed[4]),
            ColorKey::NormalMagenta => Some($($r)+ palette.indexed[5]),
            ColorKey::NormalCyan => Some($($r)+ palette.indexed[6]),
            ColorKey::NormalWhite => Some($($r)+ palette.indexed[7]),
            ColorKey::BrightBlack => Some($($r)+ palette.indexed[8]),
            ColorKey::BrightRed => Some($($r)+ palette.indexed[9]),
            ColorKey::BrightGreen => Some($($r)+ palette.indexed[10]),
            ColorKey::BrightYellow => Some($($r)+ palette.indexed[11]),
            ColorKey::BrightBlue => Some($($r)+ palette.indexed[12]),
            ColorKey::BrightMagenta => Some($($r)+ palette.indexed[13]),
            ColorKey::BrightCyan => Some($($r)+ palette.indexed[14]),
            ColorKey::BrightWhite => Some($($r)+ palette.indexed[15]),
            ColorKey::TabBarBackground => Some($($r)+ palette.ui.tab_bar_background),
            ColorKey::TabActiveBackground => Some($($r)+ palette.ui.tab_active_background),
            ColorKey::TabActiveText => Some($($r)+ palette.ui.tab_active_text),
            ColorKey::TabInactiveText => Some($($r)+ palette.ui.tab_inactive_text),
            ColorKey::PaneBorder => Some($($r)+ palette.ui.pane_border),
            ColorKey::PaneBroadcast => Some($($r)+ palette.ui.pane_broadcast),
            ColorKey::PaletteBackground => Some($($r)+ palette.ui.palette_background),
            ColorKey::PaletteText => Some($($r)+ palette.ui.palette_text),
            ColorKey::PaletteMatch => Some($($r)+ palette.ui.palette_match),
            ColorKey::PaletteSelectionBackground => {
                Some($($r)+ palette.ui.palette_selection_background)
            }
            ColorKey::BannerBackground => Some($($r)+ palette.ui.banner_background),
            ColorKey::BannerText => Some($($r)+ palette.ui.banner_text),
        }
    }};
}

/// Every color of a theme file, with its key path. The parser and the theme editor
/// both resolve keys through this table, so they cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorKey {
    Foreground,
    Background,
    Cursor,
    CursorText,
    SelectionBackground,
    SelectionForeground,
    NormalBlack,
    NormalRed,
    NormalGreen,
    NormalYellow,
    NormalBlue,
    NormalMagenta,
    NormalCyan,
    NormalWhite,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    TabBarBackground,
    TabActiveBackground,
    TabActiveText,
    TabInactiveText,
    PaneBorder,
    PaneBroadcast,
    PaletteBackground,
    PaletteText,
    PaletteMatch,
    PaletteSelectionBackground,
    BannerBackground,
    BannerText,
}

impl ColorKey {
    /// Every key, in theme file order: terminal colors, normal ANSI, bright ANSI, UI.
    pub const ALL: [ColorKey; 34] = [
        Self::Foreground,
        Self::Background,
        Self::Cursor,
        Self::CursorText,
        Self::SelectionBackground,
        Self::SelectionForeground,
        Self::NormalBlack,
        Self::NormalRed,
        Self::NormalGreen,
        Self::NormalYellow,
        Self::NormalBlue,
        Self::NormalMagenta,
        Self::NormalCyan,
        Self::NormalWhite,
        Self::BrightBlack,
        Self::BrightRed,
        Self::BrightGreen,
        Self::BrightYellow,
        Self::BrightBlue,
        Self::BrightMagenta,
        Self::BrightCyan,
        Self::BrightWhite,
        Self::TabBarBackground,
        Self::TabActiveBackground,
        Self::TabActiveText,
        Self::TabInactiveText,
        Self::PaneBorder,
        Self::PaneBroadcast,
        Self::PaletteBackground,
        Self::PaletteText,
        Self::PaletteMatch,
        Self::PaletteSelectionBackground,
        Self::BannerBackground,
        Self::BannerText,
    ];

    /// Key path in a theme file, e.g. `["colors", "normal", "red"]`.
    pub fn path(self) -> &'static [&'static str] {
        match self {
            Self::Foreground => &["colors", "foreground"],
            Self::Background => &["colors", "background"],
            Self::Cursor => &["colors", "cursor"],
            Self::CursorText => &["colors", "cursor_text"],
            Self::SelectionBackground => &["colors", "selection_background"],
            Self::SelectionForeground => &["colors", "selection_foreground"],
            Self::NormalBlack => &["colors", "normal", "black"],
            Self::NormalRed => &["colors", "normal", "red"],
            Self::NormalGreen => &["colors", "normal", "green"],
            Self::NormalYellow => &["colors", "normal", "yellow"],
            Self::NormalBlue => &["colors", "normal", "blue"],
            Self::NormalMagenta => &["colors", "normal", "magenta"],
            Self::NormalCyan => &["colors", "normal", "cyan"],
            Self::NormalWhite => &["colors", "normal", "white"],
            Self::BrightBlack => &["colors", "bright", "black"],
            Self::BrightRed => &["colors", "bright", "red"],
            Self::BrightGreen => &["colors", "bright", "green"],
            Self::BrightYellow => &["colors", "bright", "yellow"],
            Self::BrightBlue => &["colors", "bright", "blue"],
            Self::BrightMagenta => &["colors", "bright", "magenta"],
            Self::BrightCyan => &["colors", "bright", "cyan"],
            Self::BrightWhite => &["colors", "bright", "white"],
            Self::TabBarBackground => &["ui", "tab_bar_background"],
            Self::TabActiveBackground => &["ui", "tab_active_background"],
            Self::TabActiveText => &["ui", "tab_active_text"],
            Self::TabInactiveText => &["ui", "tab_inactive_text"],
            Self::PaneBorder => &["ui", "pane_border"],
            Self::PaneBroadcast => &["ui", "pane_broadcast"],
            Self::PaletteBackground => &["ui", "palette_background"],
            Self::PaletteText => &["ui", "palette_text"],
            Self::PaletteMatch => &["ui", "palette_match"],
            Self::PaletteSelectionBackground => &["ui", "palette_selection_background"],
            Self::BannerBackground => &["ui", "banner_background"],
            Self::BannerText => &["ui", "banner_text"],
        }
    }

    /// The key whose path is `path`.
    pub fn from_path(path: &[&str]) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.path() == path)
    }

    /// Color of this key in `palette`; `None` only for an unset selection foreground.
    pub fn get(self, palette: &Palette) -> Option<Rgb> {
        if self == Self::SelectionForeground {
            return palette.selection_foreground;
        }
        color_slot!(self, palette, &).copied()
    }

    #[allow(dead_code)] // Used by the theme editor, issues #161 and #162.
    /// Sets this key in `palette`. `None` only applies to the selection foreground and
    /// is ignored for the other keys.
    pub fn set(self, palette: &mut Palette, color: Option<Rgb>) {
        if self == Self::SelectionForeground {
            palette.selection_foreground = color;
        } else if let (Some(slot), Some(color)) = (color_slot!(self, palette, &mut), color) {
            *slot = color;
        }
    }
}

#[allow(dead_code)] // Used by the theme editor model, issue #162.
/// Why a name cannot be used for a new theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    Empty,
    /// Holds `/` or `\`, or starts with `.`: it would point outside the themes directory.
    Invalid,
    /// A user or built-in theme already has this name.
    Taken,
}

impl std::fmt::Display for NameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Empty => "the theme name is empty",
            Self::Invalid => "the theme name cannot contain `/` or `\\` nor start with `.`",
            Self::Taken => "a theme with this name already exists",
        })
    }
}

impl std::error::Error for NameError {}

/// Whether `name` can be read from or written to the themes directory.
fn is_plain(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && !name.starts_with('.')
}

#[allow(dead_code)] // Used by the theme editor model, issue #162.
/// Checks a name for a new theme (duplicate or rename): non-empty, no path separator,
/// no leading `.`, and not already used by a user or built-in theme.
pub fn valid_name(name: &str, config_dir: Option<&Path>) -> Result<(), NameError> {
    if name.is_empty() {
        Err(NameError::Empty)
    } else if !is_plain(name) {
        Err(NameError::Invalid)
    } else if available(config_dir).iter().any(|theme| theme == name) {
        Err(NameError::Taken)
    } else {
        Ok(())
    }
}

/// User file of a theme, `<config_dir>/themes/<name>.toml`; `None` for a name that
/// would point outside the themes directory.
pub fn user_file(config_dir: &Path, name: &str) -> Option<PathBuf> {
    is_plain(name).then(|| config_dir.join(THEMES_DIR).join(format!("{name}.toml")))
}

/// The `graphene-night` palette, parsed once.
pub fn default_palette() -> &'static Palette {
    static DEFAULT: OnceLock<Palette> = OnceLock::new();
    DEFAULT.get_or_init(|| {
        let text = built_in(DEFAULT_THEME).expect("graphene-night is built in");
        let (palette, errors) = parse(text, &Palette::blank(), Path::new("graphene-night.toml"));
        debug_assert!(errors.is_empty(), "graphene-night has errors: {errors:?}");
        palette
    })
}

/// Source of a built-in theme.
pub fn built_in(name: &str) -> Option<&'static str> {
    BUILT_IN
        .iter()
        .find(|(built_in, _)| *built_in == name)
        .map(|(_, text)| *text)
}

/// Lists user themes (`themes/*.toml`) before built-in themes, without duplicates.
///
/// If `config_dir` is absent or its themes directory cannot be read, only built-in
/// themes are returned.
pub fn available(config_dir: Option<&Path>) -> Vec<String> {
    let mut themes: Vec<String> = config_dir
        .and_then(|dir| std::fs::read_dir(dir.join(THEMES_DIR)).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "toml"))
        .filter_map(|path| path.file_stem()?.to_str().map(str::to_owned))
        .collect();
    themes.sort();
    themes.dedup();
    for (name, _) in BUILT_IN {
        if !themes.iter().any(|theme| theme == name) {
            themes.push((*name).to_owned());
        }
    }
    themes
}

/// Resolves a theme name: `<config_dir>/themes/<name>.toml` first, then the built-in
/// themes. An unknown name yields an error and `graphene-night`.
pub fn load(name: &str, config_dir: Option<&Path>) -> (Palette, Vec<ConfigError>) {
    let base = default_palette();
    // A name with a path separator would read outside the themes directory.
    let plain = is_plain(name);

    if let Some(file) = config_dir.and_then(|dir| user_file(dir, name)) {
        match std::fs::read_to_string(&file) {
            Ok(text) => return parse(&text, base, &file),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return (
                    base.clone(),
                    vec![ConfigError::read(
                        file,
                        format!("cannot read the theme: {error}"),
                    )],
                );
            }
        }
    }

    if let Some(text) = built_in(name).filter(|_| plain) {
        return parse(text, base, Path::new(&format!("{name}.toml")));
    }

    let file = config_dir.map_or_else(|| PathBuf::from(CONFIG_FILE), |dir| dir.join(CONFIG_FILE));
    (
        base.clone(),
        vec![ConfigError {
            file,
            line: None,
            key: None,
            kind: ConfigErrorKind::InvalidValue,
            message: format!("unknown theme `{name}`, using `{DEFAULT_THEME}`"),
        }],
    )
}

/// Parses a theme file on top of `base`: every color found replaces the one of `base`,
/// a missing or invalid one keeps it. A TOML syntax error returns `base` unchanged.
pub fn parse(text: &str, base: &Palette, file: &Path) -> (Palette, Vec<ConfigError>) {
    let mut reader = Reader {
        text,
        file,
        errors: Vec::new(),
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
            return (base.clone(), reader.errors);
        }
    };

    let mut palette = base.clone();
    for (key, value) in root.iter() {
        let name = key.get_ref().as_ref();
        let Some(table) = reader.table(name, value) else {
            continue;
        };
        match name {
            "colors" => {
                for (key, value) in table.iter() {
                    let name = key.get_ref().as_ref();
                    match name {
                        "normal" | "bright" => {
                            let path = format!("colors.{name}");
                            let Some(table) = reader.table(&path, value) else {
                                continue;
                            };
                            for (key, value) in table.iter() {
                                let color = key.get_ref().as_ref();
                                reader.key_color(
                                    &mut palette,
                                    &["colors", name, color],
                                    key,
                                    value,
                                );
                            }
                        }
                        _ => reader.key_color(&mut palette, &["colors", name], key, value),
                    }
                }
            }
            "ui" => {
                for (key, value) in table.iter() {
                    let name = key.get_ref().as_ref();
                    reader.key_color(&mut palette, &["ui", name], key, value);
                }
            }
            _ => reader.unknown(key, name),
        }
    }
    let ansi: [Rgb; 16] = palette.indexed[..16].try_into().expect("16 ANSI colors");
    palette.indexed = build_indexed(ansi);
    // Keys are visited in alphabetical order: report errors in file order.
    reader.errors.sort_by_key(|error| error.line);
    (palette, reader.errors)
}

/// Formats a color as `#rrggbb`, the inverse of [`parse_color`].
pub fn format_color(color: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

/// Parses `#rrggbb`.
pub fn parse_color(text: &str) -> Option<Rgb> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(Rgb {
        r: (value >> 16) as u8,
        g: (value >> 8) as u8,
        b: value as u8,
    })
}

struct Reader<'a> {
    text: &'a str,
    file: &'a Path,
    errors: Vec<ConfigError>,
}

impl<'a> Reader<'a> {
    fn error(
        &mut self,
        span: Option<Range<usize>>,
        key: Option<&str>,
        kind: ConfigErrorKind,
        message: String,
    ) {
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
            key: key.map(str::to_owned),
            kind,
            message,
        });
    }

    fn unknown(&mut self, key: &Spanned<std::borrow::Cow<'_, str>>, path: &str) {
        self.error(
            Some(key.span()),
            Some(path),
            ConfigErrorKind::UnknownKey,
            format!("unknown key `{path}`"),
        );
    }

    fn table<'v, 'de>(
        &mut self,
        path: &str,
        value: &'v Spanned<DeValue<'de>>,
    ) -> Option<&'v DeTable<'de>> {
        match value.get_ref() {
            DeValue::Table(table) => Some(table),
            _ => {
                self.error(
                    Some(value.span()),
                    Some(path),
                    ConfigErrorKind::InvalidValue,
                    format!("`{path}` must be a table"),
                );
                None
            }
        }
    }

    fn string<'v>(&mut self, value: &'v Spanned<DeValue<'_>>, path: &str) -> Option<&'v str> {
        match value.get_ref() {
            DeValue::String(text) => Some(text.as_ref()),
            _ => {
                self.error(
                    Some(value.span()),
                    Some(path),
                    ConfigErrorKind::InvalidValue,
                    format!("invalid value for `{path}`: expected a \"#rrggbb\" string"),
                );
                None
            }
        }
    }

    /// Stores a `#rrggbb` color in `target`; on error `target` is left unchanged.
    fn color(&mut self, value: &Spanned<DeValue<'_>>, path: &str, target: &mut Rgb) {
        let Some(text) = self.string(value, path) else {
            return;
        };
        match parse_color(text) {
            Some(color) => *target = color,
            None => self.error(
                Some(value.span()),
                Some(path),
                ConfigErrorKind::InvalidValue,
                format!("invalid value for `{path}`: `{text}` is not a \"#rrggbb\" color"),
            ),
        }
    }

    /// Reads the color at `path` into `palette`, or reports an unknown key.
    fn key_color(
        &mut self,
        palette: &mut Palette,
        path: &[&str],
        key: &Spanned<std::borrow::Cow<'_, str>>,
        value: &Spanned<DeValue<'_>>,
    ) {
        let dotted = path.join(".");
        match ColorKey::from_path(path) {
            Some(ColorKey::SelectionForeground) => {
                self.optional_color(value, &dotted, &mut palette.selection_foreground);
            }
            Some(color_key) => {
                if let Some(slot) = color_slot!(color_key, &mut *palette, &mut) {
                    self.color(value, &dotted, slot);
                }
            }
            None => self.unknown(key, &dotted),
        }
    }

    /// Like [`Self::color`], an empty string meaning `None`.
    fn optional_color(
        &mut self,
        value: &Spanned<DeValue<'_>>,
        path: &str,
        target: &mut Option<Rgb>,
    ) {
        if let Some("") = self.string_quiet(value) {
            *target = None;
            return;
        }
        let mut color = target.unwrap_or(Rgb { r: 0, g: 0, b: 0 });
        let errors = self.errors.len();
        self.color(value, path, &mut color);
        if self.errors.len() == errors {
            *target = Some(color);
        }
    }

    fn string_quiet<'v>(&self, value: &'v Spanned<DeValue<'_>>) -> Option<&'v str> {
        match value.get_ref() {
            DeValue::String(text) => Some(text.as_ref()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::UiColors;

    fn rgb(hex: u32) -> Rgb {
        Rgb {
            r: (hex >> 16) as u8,
            g: (hex >> 8) as u8,
            b: hex as u8,
        }
    }

    fn parse_on_default(text: &str) -> (Palette, Vec<ConfigError>) {
        parse(text, default_palette(), Path::new("test.toml"))
    }

    /// WCAG contrast ratio between two colors: 1 for identical colors, 21 for black
    /// on white.
    fn contrast(a: Rgb, b: Rgb) -> f64 {
        fn luminance(color: Rgb) -> f64 {
            let channel = |value: u8| {
                let c = f64::from(value) / 255.0;
                if c <= 0.039_28 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
        }
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn contrast_ratio_matches_wcag() {
        assert!((contrast(rgb(0x000000), rgb(0xffffff)) - 21.0).abs() < 1e-9);
        assert!((contrast(rgb(0x1c1f26), rgb(0x1c1f26)) - 1.0).abs() < 1e-9);
        assert_eq!(
            contrast(rgb(0x123456), rgb(0xabcdef)),
            contrast(rgb(0xabcdef), rgb(0x123456))
        );
    }

    #[test]
    fn every_built_in_theme_keeps_the_palette_readable() {
        for (name, text) in BUILT_IN {
            let (palette, _) = parse(text, &Palette::blank(), Path::new(name));
            let ui = palette.ui;
            let selection = contrast(ui.palette_background, ui.palette_selection_background);
            assert!(
                selection >= 1.1,
                "theme {name}: selected row contrast {selection:.2}"
            );
            let border = contrast(ui.palette_background, ui.pane_border);
            assert!(
                border >= 1.1,
                "theme {name}: palette border contrast {border:.2}"
            );
            let text = contrast(ui.palette_text, ui.palette_selection_background);
            assert!(
                text >= 4.5,
                "theme {name}: selected row text contrast {text:.2}"
            );
        }
    }

    #[test]
    fn every_built_in_theme_keeps_the_pane_bars_readable() {
        use crate::chrome::scene::pane_bar_colors;
        for (name, text) in BUILT_IN {
            let (palette, _) = parse(text, &Palette::blank(), Path::new(name));
            for active in [true, false] {
                for broadcast in [true, false] {
                    let (background, text) = pane_bar_colors(&palette.ui, active, broadcast);
                    let ratio = contrast(text, background);
                    assert!(
                        ratio >= 4.5,
                        "theme {name}: pane bar text contrast {ratio:.2} (active: {active}, broadcast: {broadcast})"
                    );
                }
            }
        }
    }

    #[test]
    fn every_built_in_theme_defines_a_distinct_broadcast_color() {
        for (name, text) in BUILT_IN {
            assert!(text.contains("\npane_broadcast = \"#"), "theme {name}");
            let (palette, _) = parse(text, &Palette::blank(), Path::new(name));
            assert_ne!(
                palette.ui.pane_broadcast, palette.ui.palette_match,
                "theme {name}"
            );
        }
    }

    #[test]
    fn every_built_in_theme_parses_without_error() {
        for (name, text) in BUILT_IN {
            let (_, errors) = parse(text, &Palette::blank(), Path::new(name));
            assert_eq!(errors, Vec::new(), "theme {name}");
        }
    }

    #[test]
    fn every_built_in_theme_sets_every_color() {
        // Parsed on two different bases, a complete theme gives the same palette.
        let white = {
            let mut p = Palette::blank();
            let white = rgb(0xffffff);
            p.indexed = build_indexed([white; 16]);
            p.foreground = white;
            p.background = white;
            p.cursor = white;
            p.cursor_text = white;
            p.selection_background = white;
            p.selection_foreground = Some(white);
            p.ui = UiColors::uniform(white);
            p
        };
        for (name, text) in BUILT_IN {
            let (on_blank, _) = parse(text, &Palette::blank(), Path::new(name));
            let (on_white, _) = parse(text, &white, Path::new(name));
            assert_eq!(on_blank, on_white, "theme {name}");
        }
    }

    #[test]
    fn third_party_themes_carry_their_licence() {
        for (name, text) in BUILT_IN {
            if !name.starts_with("graphene-") {
                assert!(text.contains("Copyright (c)"), "theme {name}");
                assert!(
                    text.contains("Permission is hereby granted"),
                    "theme {name}"
                );
            }
        }
    }

    #[test]
    fn graphene_night_matches_the_original_palette() {
        let p = default_palette();
        let ansi = [
            0x1c1f26, 0xf0717b, 0x9ed072, 0xe7c664, 0x6cb6ff, 0xc792ea, 0x5fd7d7, 0xc8ccd4,
            0x5c6370, 0xff8b92, 0xb5e890, 0xffd982, 0x8ecaff, 0xdcb0ff, 0x82eaea, 0xf2f4f8,
        ]
        .map(rgb);
        assert_eq!(p.indexed, build_indexed(ansi));
        assert_eq!(p.foreground, rgb(0xd7dae0));
        assert_eq!(p.background, rgb(0x14161b));
        assert_eq!(p.cursor, rgb(0x82aaff));
        assert_eq!(p.cursor_text, rgb(0x14161b));
        assert_eq!(p.selection_background, rgb(0x33508a));
        assert_eq!(p.selection_foreground, None);
    }

    #[test]
    fn partial_theme_falls_back_to_graphene_night() {
        let (p, errors) = parse_on_default(
            r##"
[colors]
background = "#000000"
selection_foreground = "#ffffff"

[colors.bright]
red = "#ff0000"

[ui]
banner_background = "#123456"
"##,
        );
        assert_eq!(errors, Vec::new());
        let night = default_palette();
        assert_eq!(p.background, rgb(0x000000));
        assert_eq!(p.foreground, night.foreground);
        assert_eq!(p.selection_foreground, Some(rgb(0xffffff)));
        assert_eq!(p.indexed[9], rgb(0xff0000));
        assert_eq!(p.indexed[1], night.indexed[1]);
        assert_eq!(p.indexed[16..], night.indexed[16..]);
        assert_eq!(p.ui.banner_background, rgb(0x123456));
        assert_eq!(p.ui.banner_text, night.ui.banner_text);
    }

    #[test]
    fn invalid_colors_and_unknown_keys_are_reported_with_their_line() {
        let (p, errors) = parse_on_default(
            r##"[colors]
foreground = "red"
background = 12
cursor = "#82aaff"
glow = "#ffffff"
selection_foreground = "#12"

[colors.normal]
orange = "#ff8800"

[ui]
banner = "#000000"

[extra]
"##,
        );
        let night = default_palette();
        assert_eq!(p.foreground, night.foreground);
        assert_eq!(p.background, night.background);
        assert_eq!(p.selection_foreground, None);
        let lines: Vec<_> = errors.iter().map(|e| (e.line, e.message.clone())).collect();
        assert_eq!(
            lines,
            vec![
                (
                    Some(2),
                    "invalid value for `colors.foreground`: `red` is not a \"#rrggbb\" color"
                        .to_owned()
                ),
                (
                    Some(3),
                    "invalid value for `colors.background`: expected a \"#rrggbb\" string"
                        .to_owned()
                ),
                (Some(5), "unknown key `colors.glow`".to_owned()),
                (
                    Some(6),
                    "invalid value for `colors.selection_foreground`: `#12` is not a \"#rrggbb\" color"
                        .to_owned()
                ),
                (Some(9), "unknown key `colors.normal.orange`".to_owned()),
                (Some(12), "unknown key `ui.banner`".to_owned()),
                (Some(14), "unknown key `extra`".to_owned()),
            ]
        );
        assert!(errors.iter().all(|e| e.file == Path::new("test.toml")));
    }

    #[test]
    fn theme_errors_carry_the_key_path_of_the_faulty_color() {
        let (_, errors) = parse_on_default(
            "[colors]\nforeground = \"red\"\nglow = \"#ffffff\"\n[colors.normal]\nred = 1\n[ui]\nbanner = \"#000000\"\n",
        );
        let keys: Vec<_> = errors.iter().map(|e| (e.key.as_deref(), e.kind)).collect();
        assert_eq!(
            keys,
            vec![
                (Some("colors.foreground"), ConfigErrorKind::InvalidValue),
                (Some("colors.glow"), ConfigErrorKind::UnknownKey),
                (Some("colors.normal.red"), ConfigErrorKind::InvalidValue),
                (Some("ui.banner"), ConfigErrorKind::UnknownKey),
            ]
        );
    }

    #[test]
    fn theme_syntax_error_has_no_key_path() {
        let (_, errors) = parse_on_default("[colors\n");
        assert_eq!(errors[0].key, None);
        assert_eq!(errors[0].kind, ConfigErrorKind::Syntax);
    }

    #[test]
    fn section_that_is_not_a_table_is_reported() {
        let (_, errors) = parse_on_default("colors = 1\n[ui]\n");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "`colors` must be a table");
    }

    #[test]
    fn syntax_error_keeps_the_base_palette() {
        let (p, errors) = parse_on_default("[colors\nforeground = \"#000000\"\n");
        assert_eq!(&p, default_palette());
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, Some(1));
    }

    #[test]
    fn empty_selection_foreground_clears_it() {
        let mut base = default_palette().clone();
        base.selection_foreground = Some(rgb(0x010203));
        let (p, errors) = parse(
            "[colors]\nselection_foreground = \"\"\n",
            &base,
            Path::new("t"),
        );
        assert_eq!(errors, Vec::new());
        assert_eq!(p.selection_foreground, None);
    }

    #[test]
    fn parse_color_accepts_only_hash_rrggbb() {
        assert_eq!(parse_color("#a1B2c3"), Some(rgb(0xa1b2c3)));
        for text in ["a1b2c3", "#a1b2c", "#a1b2c3d", "#gggggg", "", "#"] {
            assert_eq!(parse_color(text), None, "{text}");
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("graphene-theme-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(THEMES_DIR)).unwrap();
        dir
    }

    #[test]
    fn available_lists_sorted_user_themes_before_built_ins() {
        let dir = temp_dir("available-sorted");
        let themes_dir = dir.join(THEMES_DIR);
        std::fs::write(themes_dir.join("zebra.toml"), "").unwrap();
        std::fs::write(themes_dir.join("amber.toml"), "").unwrap();

        let available = available(Some(&dir));
        assert_eq!(&available[..2], ["amber", "zebra"]);
        assert_eq!(
            &available[2..],
            BUILT_IN.iter().map(|(name, _)| *name).collect::<Vec<_>>()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn available_deduplicates_user_themes_that_shadow_built_ins() {
        let dir = temp_dir("available-collision");
        std::fs::write(dir.join(THEMES_DIR).join("gruvbox-dark.toml"), "").unwrap();

        let available = available(Some(&dir));
        assert_eq!(
            available
                .iter()
                .filter(|name| *name == "gruvbox-dark")
                .count(),
            1
        );
        assert_eq!(available[0], "gruvbox-dark");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn available_ignores_non_toml_files_and_directories() {
        let dir = temp_dir("available-file-types");
        let themes_dir = dir.join(THEMES_DIR);
        std::fs::write(themes_dir.join("readme.txt"), "").unwrap();
        std::fs::write(themes_dir.join("backup.toml.bak"), "").unwrap();
        std::fs::create_dir(themes_dir.join("folder.toml")).unwrap();
        std::fs::write(themes_dir.join("mine.toml"), "").unwrap();

        let available = available(Some(&dir));
        assert_eq!(available[0], "mine");
        assert!(
            !available
                .iter()
                .any(|name| { ["readme", "backup", "folder"].contains(&name.as_str()) })
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn available_without_a_config_directory_returns_built_ins() {
        assert_eq!(
            available(None),
            BUILT_IN
                .iter()
                .map(|(name, _)| (*name).to_owned())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn available_with_a_missing_themes_directory_returns_built_ins() {
        let dir = std::env::temp_dir().join(format!(
            "graphene-theme-available-missing-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            available(Some(&dir)),
            BUILT_IN
                .iter()
                .map(|(name, _)| (*name).to_owned())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn load_finds_built_in_themes() {
        let (p, errors) = load("gruvbox-dark", None);
        assert_eq!(errors, Vec::new());
        assert_eq!(p.background, rgb(0x282828));
    }

    #[test]
    fn user_theme_overrides_built_in() {
        let dir = temp_dir("override");
        std::fs::write(
            dir.join(THEMES_DIR).join("gruvbox-dark.toml"),
            "[colors]\nbackground = \"#010203\"\n",
        )
        .unwrap();
        let (p, errors) = load("gruvbox-dark", Some(&dir));
        assert_eq!(errors, Vec::new());
        assert_eq!(p.background, rgb(0x010203));
        // The rest comes from graphene-night, not from the built-in gruvbox.
        assert_eq!(p.foreground, default_palette().foreground);

        let (p, _) = load("graphene-day", Some(&dir));
        assert_eq!(p.background, rgb(0xf6f7f9));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn user_theme_errors_point_at_its_file() {
        let dir = temp_dir("errors");
        let file = dir.join(THEMES_DIR).join("mine.toml");
        std::fs::write(&file, "[colors]\nforeground = \"nope\"\n").unwrap();
        let (_, errors) = load("mine", Some(&dir));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, file);
        assert_eq!(errors[0].line, Some(2));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unknown_theme_falls_back_with_an_error() {
        let dir = temp_dir("unknown");
        for name in ["no-such-theme", "../config", ".hidden", ""] {
            let (p, errors) = load(name, Some(&dir));
            assert_eq!(&p, default_palette(), "{name}");
            assert_eq!(errors.len(), 1, "{name}");
            assert_eq!(errors[0].file, dir.join(CONFIG_FILE));
            assert_eq!(
                errors[0].message,
                format!("unknown theme `{name}`, using `graphene-night`")
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn color_keys_are_distinct_and_cover_every_palette_color() {
        let paths: std::collections::HashSet<_> =
            ColorKey::ALL.iter().map(|key| key.path()).collect();
        assert_eq!(paths.len(), ColorKey::ALL.len());

        // Setting every key to white on a blank palette yields an all-white palette.
        let white = rgb(0xffffff);
        let mut palette = Palette::blank();
        for key in ColorKey::ALL {
            key.set(&mut palette, Some(white));
        }
        assert_eq!(&palette.indexed[..16], &[white; 16]);
        assert_eq!(palette.ui, UiColors::uniform(white));
        for color in [
            palette.foreground,
            palette.background,
            palette.cursor,
            palette.cursor_text,
            palette.selection_background,
        ] {
            assert_eq!(color, white);
        }
        assert_eq!(palette.selection_foreground, Some(white));
    }

    #[test]
    fn color_key_get_set_and_path_round_trip() {
        for (i, key) in ColorKey::ALL.into_iter().enumerate() {
            assert_eq!(ColorKey::from_path(key.path()), Some(key));

            let mut palette = default_palette().clone();
            let color = rgb(0x010203 + i as u32);
            key.set(&mut palette, Some(color));
            assert_eq!(key.get(&palette), Some(color), "{key:?}");
            // Only this key changed.
            for other in ColorKey::ALL.into_iter().filter(|other| *other != key) {
                assert_eq!(other.get(&palette), other.get(default_palette()), "{key:?}");
            }

            // The parser reads the key at its path.
            let (table, name) = key.path().split_at(key.path().len() - 1);
            let text = format!(
                "[{}]\n{} = \"{}\"\n",
                table.join("."),
                name[0],
                format_color(color)
            );
            let (parsed, errors) = parse_on_default(&text);
            assert_eq!(errors, Vec::new(), "{key:?}");
            assert_eq!(key.get(&parsed), Some(color), "{key:?}");
        }
        assert_eq!(ColorKey::from_path(&["colors", "normal", "orange"]), None);
        assert_eq!(ColorKey::from_path(&["ui"]), None);
    }

    #[test]
    fn none_only_clears_the_selection_foreground() {
        let mut palette = default_palette().clone();
        ColorKey::SelectionForeground.set(&mut palette, None);
        assert_eq!(ColorKey::SelectionForeground.get(&palette), None);
        ColorKey::Foreground.set(&mut palette, None);
        assert_eq!(palette.foreground, default_palette().foreground);
    }

    #[test]
    fn format_color_is_the_inverse_of_parse_color() {
        for hex in [0x000000, 0xffffff, 0x0a0b0c, 0x1c1f26, 0xabcdef] {
            let color = rgb(hex);
            let text = format_color(color);
            assert_eq!(text, format!("#{hex:06x}"));
            assert_eq!(parse_color(&text), Some(color));
        }
    }

    #[test]
    fn user_file_points_inside_the_themes_directory() {
        let dir = Path::new("/config");
        assert_eq!(
            user_file(dir, "mine"),
            Some(dir.join(THEMES_DIR).join("mine.toml"))
        );
        for name in ["", "a/b", "a\\b", ".hidden", "../config"] {
            assert_eq!(user_file(dir, name), None, "{name}");
        }
    }

    #[test]
    fn valid_name_rejects_empty_invalid_and_taken_names() {
        let dir = temp_dir("valid-name");
        std::fs::write(dir.join(THEMES_DIR).join("mine.toml"), "").unwrap();

        assert_eq!(valid_name("fresh", Some(&dir)), Ok(()));
        assert_eq!(valid_name("my theme", None), Ok(()));
        assert_eq!(valid_name("", Some(&dir)), Err(NameError::Empty));
        for name in ["a/b", "a\\b", ".hidden", "../x"] {
            assert_eq!(
                valid_name(name, Some(&dir)),
                Err(NameError::Invalid),
                "{name}"
            );
        }
        assert_eq!(valid_name("mine", Some(&dir)), Err(NameError::Taken));
        assert_eq!(
            valid_name("graphene-night", Some(&dir)),
            Err(NameError::Taken)
        );
        assert_eq!(valid_name("solarized-dark", None), Err(NameError::Taken));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
