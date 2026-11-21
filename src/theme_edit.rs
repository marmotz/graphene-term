//! Edits of the user theme files (`<config_dir>/themes/<name>.toml`) made by the theme
//! editor of the settings window. Every write goes through [`ConfigEdit`], so comments,
//! whitespace and the keys left alone are kept, and lands atomically, so the watcher sees
//! one change.
//!
//! These functions only touch theme files; the matching `config.toml` changes (a renamed
//! or deleted theme that was selected) are computed by the settings model.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use alacritty_terminal::vte::ansi::Rgb;

use crate::atomic_file;
use crate::config::{ConfigError, ConfigErrorKind};
use crate::config_edit::{self, ConfigEdit};
use crate::theme::{self, ColorKey};

/// Sets `key` of the user theme `name` to `color` (`None` writes `""`, which only means
/// something for the selection foreground). Only that key changes; a missing theme file
/// (a built-in theme) is a [`ConfigErrorKind::Read`] error.
pub fn set_color(
    config_dir: &Path,
    name: &str,
    key: ColorKey,
    color: Option<Rgb>,
) -> Result<(), ConfigError> {
    let file = theme_file(config_dir, name)?;
    config_edit::write_existing(&file, |edit| edit.set(key.path(), color_value(color)))
}

/// Copies the theme `from` (user file, else built-in source) to a new user theme `to`.
/// The copy keeps the source text and its comments, except the description header of a
/// built-in theme ("Graphene night: the default dark theme."), which no longer describes
/// it; a licence header is kept. The copy also gets every
/// color key the source leaves out (or holds an invalid value for) set to its effective
/// value, so it is complete. An existing `to` file is never overwritten.
pub fn duplicate(config_dir: &Path, from: &str, to: &str) -> Result<(), ConfigError> {
    let target = theme_file(config_dir, to)?;
    let (text, source) = source_text(config_dir, from)?;
    let (palette, _) = theme::load(from, Some(config_dir));

    let mut edit = ConfigEdit::parse(&text, &source)?;
    for key in ColorKey::ALL {
        if !holds_valid_color(&edit, key) {
            edit.set(key.path(), color_value(key.get(&palette)))?;
        }
    }

    if target.exists() {
        return Err(exists_error(&target, to));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| config_edit::write_error(&target, &error))?;
    }
    atomic_file::write_atomic(&target, &edit.to_string())
        .map_err(|error| config_edit::write_error(&target, &error))
}

/// Renames the user theme `from` to `to`; refused when `to` already has a file.
pub fn rename(config_dir: &Path, from: &str, to: &str) -> Result<(), ConfigError> {
    let source = theme_file(config_dir, from)?;
    let target = theme_file(config_dir, to)?;
    if target.exists() {
        return Err(exists_error(&target, to));
    }
    fs::rename(&source, &target).map_err(|error| config_edit::write_error(&source, &error))
}

/// Deletes the user theme file `name`.
pub fn delete(config_dir: &Path, name: &str) -> Result<(), ConfigError> {
    let file = theme_file(config_dir, name)?;
    fs::remove_file(&file).map_err(|error| config_edit::write_error(&file, &error))
}

fn theme_file(config_dir: &Path, name: &str) -> Result<PathBuf, ConfigError> {
    theme::user_file(config_dir, name).ok_or_else(|| ConfigError {
        file: config_dir.join(theme::THEMES_DIR),
        line: None,
        key: None,
        kind: ConfigErrorKind::InvalidValue,
        message: format!("invalid theme name `{name}`"),
    })
}

/// Text of the theme `name` and the file it is reported under: the user file when it
/// exists, else the built-in source.
fn source_text(config_dir: &Path, name: &str) -> Result<(String, PathBuf), ConfigError> {
    let file = theme_file(config_dir, name)?;
    match fs::read_to_string(&file) {
        Ok(text) => return Ok((text, file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(ConfigError::read(
                file,
                format!("cannot read the theme: {error}"),
            ));
        }
    }
    theme::built_in(name)
        .map(|text| {
            (
                without_description(text).to_owned(),
                PathBuf::from(format!("{name}.toml")),
            )
        })
        .ok_or_else(|| ConfigError::read(file, format!("unknown theme `{name}`")))
}

/// `text` without its leading comment block, unless that block holds a licence, which
/// must stay with the colors it covers.
fn without_description(text: &str) -> &str {
    let header_len: usize = text
        .split_inclusive('\n')
        .take_while(|line| {
            let line = line.trim();
            line.is_empty() || line.starts_with('#')
        })
        .map(str::len)
        .sum();
    let (header, rest) = text.split_at(header_len);
    if header.contains("Copyright") {
        text
    } else {
        rest
    }
}

/// Whether the document sets `key` to a value the theme parser accepts.
fn holds_valid_color(edit: &ConfigEdit, key: ColorKey) -> bool {
    edit.value(key.path())
        .and_then(toml_edit::Value::as_str)
        .is_some_and(|text| {
            theme::parse_color(text).is_some()
                || (key == ColorKey::SelectionForeground && text.is_empty())
        })
}

fn color_value(color: Option<Rgb>) -> String {
    color.map(theme::format_color).unwrap_or_default()
}

fn exists_error(file: &Path, name: &str) -> ConfigError {
    ConfigError {
        file: file.to_path_buf(),
        line: None,
        key: None,
        kind: ConfigErrorKind::Write,
        message: format!("a theme named `{name}` already exists"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("graphene-theme-edit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(theme::THEMES_DIR)).unwrap();
        dir
    }

    fn write_theme(dir: &Path, name: &str, text: &str) -> PathBuf {
        let file = theme::user_file(dir, name).unwrap();
        fs::write(&file, text).unwrap();
        file
    }

    fn rgb(value: u32) -> Rgb {
        Rgb {
            r: (value >> 16) as u8,
            g: (value >> 8) as u8,
            b: value as u8,
        }
    }

    const PARTIAL: &str = "# My theme.\n\n[colors]\n# Main text.\nforeground = \"#111111\" # mine\n\n[colors.normal]\nred = \"#aa0000\"\n";

    #[test]
    fn set_color_only_changes_the_edited_key() {
        let dir = temp_dir("set-color");
        let file = write_theme(&dir, "mine", PARTIAL);

        set_color(&dir, "mine", ColorKey::NormalRed, Some(rgb(0x12ab34))).unwrap();

        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            PARTIAL.replace("#aa0000", "#12ab34")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn set_color_adds_a_missing_key_and_keeps_the_others() {
        let dir = temp_dir("set-color-missing");
        let file = write_theme(&dir, "mine", PARTIAL);

        set_color(&dir, "mine", ColorKey::PaneBorder, Some(rgb(0x010203))).unwrap();

        let text = fs::read_to_string(&file).unwrap();
        assert!(text.starts_with(PARTIAL), "{text}");
        let (palette, errors) = theme::load("mine", Some(&dir));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(palette.ui.pane_border, rgb(0x010203));
        assert_eq!(palette.foreground, rgb(0x111111));
        // Absent keys stay absent: still inherited from the default theme.
        assert!(!text.contains("background"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn set_color_none_writes_an_empty_selection_foreground() {
        let dir = temp_dir("set-color-none");
        let file = write_theme(
            &dir,
            "mine",
            "[colors]\nselection_foreground = \"#ffffff\"\n",
        );

        set_color(&dir, "mine", ColorKey::SelectionForeground, None).unwrap();

        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "[colors]\nselection_foreground = \"\"\n"
        );
        let (palette, _) = theme::load("mine", Some(&dir));
        assert_eq!(palette.selection_foreground, None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn set_color_fails_on_a_built_in_theme() {
        let dir = temp_dir("set-color-built-in");

        let error =
            set_color(&dir, "graphene-day", ColorKey::Foreground, Some(rgb(0))).unwrap_err();

        assert_eq!(error.kind, ConfigErrorKind::Read);
        assert!(theme::user_file(&dir, "graphene-day").is_some_and(|file| !file.exists()));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn duplicate_of_a_partial_user_theme_is_complete_and_keeps_its_comments() {
        let dir = temp_dir("duplicate-partial");
        write_theme(&dir, "mine", PARTIAL);

        duplicate(&dir, "mine", "copy").unwrap();

        let text = fs::read_to_string(theme::user_file(&dir, "copy").unwrap()).unwrap();
        assert!(
            text.starts_with(
                "# My theme.\n\n[colors]\n# Main text.\nforeground = \"#111111\" # mine\n"
            ),
            "{text}"
        );
        let edit = ConfigEdit::parse(&text, Path::new("copy.toml")).unwrap();
        for key in ColorKey::ALL {
            assert!(holds_valid_color(&edit, key), "{key:?} missing in\n{text}");
        }
        assert_eq!(
            theme::load("copy", Some(&dir)).0,
            theme::load("mine", Some(&dir)).0
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn duplicate_replaces_an_invalid_color_with_its_effective_value() {
        let dir = temp_dir("duplicate-invalid");
        write_theme(&dir, "mine", "[colors]\nforeground = \"blue\"\n");

        duplicate(&dir, "mine", "copy").unwrap();

        let (palette, errors) = theme::load("copy", Some(&dir));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(palette.foreground, theme::default_palette().foreground);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn duplicate_of_a_third_party_built_in_keeps_its_licence_and_palette() {
        let dir = temp_dir("duplicate-built-in");

        duplicate(&dir, "solarized-dark", "my-solarized").unwrap();

        let text = fs::read_to_string(theme::user_file(&dir, "my-solarized").unwrap()).unwrap();
        let source = theme::built_in("solarized-dark").unwrap();
        let header: String = source
            .lines()
            .take_while(|line| line.starts_with('#') || line.is_empty())
            .map(|line| format!("{line}\n"))
            .collect();
        assert!(header.contains("Copyright"));
        assert!(text.starts_with(&header), "{text}");
        let (copy, errors) = theme::load("my-solarized", Some(&dir));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(copy, theme::load("solarized-dark", None).0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn duplicate_of_a_graphene_built_in_drops_its_description() {
        let dir = temp_dir("duplicate-graphene");

        duplicate(&dir, "graphene-night", "my-night").unwrap();

        let text = fs::read_to_string(theme::user_file(&dir, "my-night").unwrap()).unwrap();
        assert!(text.starts_with("[colors]\n"), "{text}");
        assert!(!text.contains("built-in theme"), "{text}");
        let (copy, errors) = theme::load("my-night", Some(&dir));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(copy, theme::load("graphene-night", None).0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_a_leading_comment_block_without_licence_is_dropped() {
        assert_eq!(
            without_description("# About\n#\n\n[colors]\n# kept\nforeground = \"#000000\"\n"),
            "[colors]\n# kept\nforeground = \"#000000\"\n"
        );
        let licensed = "# Theme\n# Copyright (c) 2011 Someone\n\n[colors]\n";
        assert_eq!(without_description(licensed), licensed);
        assert_eq!(without_description("[colors]\n"), "[colors]\n");
    }

    #[test]
    fn duplicate_creates_the_themes_directory() {
        let dir = temp_dir("duplicate-no-dir");
        fs::remove_dir_all(dir.join(theme::THEMES_DIR)).unwrap();

        duplicate(&dir, "graphene-night", "copy").unwrap();

        assert!(theme::user_file(&dir, "copy").unwrap().is_file());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn duplicate_refuses_an_existing_target() {
        let dir = temp_dir("duplicate-exists");
        write_theme(&dir, "mine", PARTIAL);
        let target = write_theme(&dir, "other", "# Keep me.\n");

        let error = duplicate(&dir, "mine", "other").unwrap_err();

        assert_eq!(error.kind, ConfigErrorKind::Write);
        assert_eq!(fs::read_to_string(target).unwrap(), "# Keep me.\n");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn duplicate_of_an_unknown_theme_fails() {
        let dir = temp_dir("duplicate-unknown");

        let error = duplicate(&dir, "nope", "copy").unwrap_err();

        assert_eq!(error.kind, ConfigErrorKind::Read);
        assert!(!theme::user_file(&dir, "copy").unwrap().exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rename_moves_the_file() {
        let dir = temp_dir("rename");
        let source = write_theme(&dir, "mine", PARTIAL);

        rename(&dir, "mine", "renamed").unwrap();

        assert!(!source.exists());
        assert_eq!(
            fs::read_to_string(theme::user_file(&dir, "renamed").unwrap()).unwrap(),
            PARTIAL
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rename_refuses_an_existing_target() {
        let dir = temp_dir("rename-exists");
        let source = write_theme(&dir, "mine", PARTIAL);
        let target = write_theme(&dir, "other", "# Keep me.\n");

        let error = rename(&dir, "mine", "other").unwrap_err();

        assert_eq!(error.kind, ConfigErrorKind::Write);
        assert_eq!(fs::read_to_string(source).unwrap(), PARTIAL);
        assert_eq!(fs::read_to_string(target).unwrap(), "# Keep me.\n");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_names_are_refused() {
        let dir = temp_dir("invalid-names");
        write_theme(&dir, "mine", PARTIAL);

        for name in ["../escape", ".hidden", "a/b", ""] {
            assert_eq!(
                duplicate(&dir, "mine", name).unwrap_err().kind,
                ConfigErrorKind::InvalidValue,
                "{name}"
            );
            assert_eq!(
                rename(&dir, "mine", name).unwrap_err().kind,
                ConfigErrorKind::InvalidValue,
                "{name}"
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn delete_removes_the_file() {
        let dir = temp_dir("delete");
        let file = write_theme(&dir, "mine", PARTIAL);

        delete(&dir, "mine").unwrap();

        assert!(!file.exists());
        assert_eq!(
            delete(&dir, "mine").unwrap_err().kind,
            ConfigErrorKind::Write
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
