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

use crate::config::{CONFIG_FILE, ConfigError};
use crate::palette::{Palette, UiColors, build_indexed};

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

const ANSI_NAMES: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

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

/// Resolves a theme name: `<config_dir>/themes/<name>.toml` first, then the built-in
/// themes. An unknown name yields an error and `graphene-night`.
pub fn load(name: &str, config_dir: Option<&Path>) -> (Palette, Vec<ConfigError>) {
    let base = default_palette();
    // A name with a path separator would read outside the themes directory.
    let plain = !name.is_empty() && !name.contains(['/', '\\']) && !name.starts_with('.');

    if let (true, Some(dir)) = (plain, config_dir) {
        let file = dir.join(THEMES_DIR).join(format!("{name}.toml"));
        match std::fs::read_to_string(&file) {
            Ok(text) => return parse(&text, base, &file),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return (
                    base.clone(),
                    vec![ConfigError {
                        file,
                        line: None,
                        message: format!("cannot read the theme: {error}"),
                    }],
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
            reader.error(error.span(), error.message().trim_end().to_owned());
            return (base.clone(), reader.errors);
        }
    };

    let mut palette = base.clone();
    let mut ansi: [Rgb; 16] = palette.indexed[..16].try_into().expect("16 ANSI colors");
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
                            let offset = if name == "bright" { 8 } else { 0 };
                            for (key, value) in table.iter() {
                                let color = key.get_ref().as_ref();
                                let path = format!("{path}.{color}");
                                match ANSI_NAMES.iter().position(|ansi| *ansi == color) {
                                    Some(i) => reader.color(value, &path, &mut ansi[offset + i]),
                                    None => reader.unknown(key, &path),
                                }
                            }
                        }
                        "selection_foreground" => {
                            reader.optional_color(value, &mut palette.selection_foreground);
                        }
                        _ => {
                            let path = format!("colors.{name}");
                            match colors_slot(&mut palette, name) {
                                Some(slot) => reader.color(value, &path, slot),
                                None => reader.unknown(key, &path),
                            }
                        }
                    }
                }
            }
            "ui" => {
                for (key, value) in table.iter() {
                    let name = key.get_ref().as_ref();
                    let path = format!("ui.{name}");
                    match ui_slot(&mut palette.ui, name) {
                        Some(slot) => reader.color(value, &path, slot),
                        None => reader.unknown(key, &path),
                    }
                }
            }
            _ => reader.unknown(key, name),
        }
    }
    palette.indexed = build_indexed(ansi);
    // Keys are visited in alphabetical order: report errors in file order.
    reader.errors.sort_by_key(|error| error.line);
    (palette, reader.errors)
}

fn colors_slot<'a>(palette: &'a mut Palette, name: &str) -> Option<&'a mut Rgb> {
    Some(match name {
        "foreground" => &mut palette.foreground,
        "background" => &mut palette.background,
        "cursor" => &mut palette.cursor,
        "cursor_text" => &mut palette.cursor_text,
        "selection_background" => &mut palette.selection_background,
        _ => return None,
    })
}

fn ui_slot<'a>(ui: &'a mut UiColors, name: &str) -> Option<&'a mut Rgb> {
    Some(match name {
        "tab_bar_background" => &mut ui.tab_bar_background,
        "tab_active_background" => &mut ui.tab_active_background,
        "tab_active_text" => &mut ui.tab_active_text,
        "tab_inactive_text" => &mut ui.tab_inactive_text,
        "pane_border" => &mut ui.pane_border,
        "palette_background" => &mut ui.palette_background,
        "palette_text" => &mut ui.palette_text,
        "palette_match" => &mut ui.palette_match,
        "banner_background" => &mut ui.banner_background,
        "banner_text" => &mut ui.banner_text,
        _ => return None,
    })
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

    fn unknown(&mut self, key: &Spanned<std::borrow::Cow<'_, str>>, path: &str) {
        self.error(Some(key.span()), format!("unknown key `{path}`"));
    }

    fn table<'v, 'de>(
        &mut self,
        path: &str,
        value: &'v Spanned<DeValue<'de>>,
    ) -> Option<&'v DeTable<'de>> {
        match value.get_ref() {
            DeValue::Table(table) => Some(table),
            _ => {
                self.error(Some(value.span()), format!("`{path}` must be a table"));
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
                format!("invalid value for `{path}`: `{text}` is not a \"#rrggbb\" color"),
            ),
        }
    }

    /// Like [`Self::color`], an empty string meaning `None`.
    fn optional_color(&mut self, value: &Spanned<DeValue<'_>>, target: &mut Option<Rgb>) {
        let path = "colors.selection_foreground";
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
}
