//! Text-preserving edits of `config.toml`: completion of missing options, cleanup of
//! unknown keys, and single-key writes from the settings tab ([`ConfigEdit`]).

use std::collections::HashSet;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

use crate::atomic_file::write_atomic;
use crate::config::{ConfigError, ConfigErrorKind, DEFAULT_CONFIG, DEFAULT_KEYBINDINGS, key_path};
use crate::i18n::ConfigMsg;

mod completion;
mod migration;
mod prune;
pub(crate) use self::completion::*;
pub(crate) use self::migration::*;
pub(crate) use self::prune::*;

fn option_lines() -> Vec<OptionLine> {
    let mut options = Vec::new();
    let mut section = None;
    let mut documentation = Vec::new();
    for line in DEFAULT_CONFIG.lines() {
        let trimmed = line.trim_start();
        let Some(body) = comment_body(line) else {
            if trimmed.starts_with('#') || trimmed.is_empty() {
                documentation.clear();
            }
            continue;
        };
        if let Some(header) = table_header(body) {
            section = Some(header.to_owned());
            documentation.clear();
            continue;
        }
        if let Some(keys) = commented_keys(section.as_deref(), body) {
            for key in keys {
                options.push(OptionLine {
                    section: section.clone(),
                    key,
                    line: body.to_owned(),
                    documentation: std::mem::take(&mut documentation),
                });
            }
        } else if trimmed.starts_with("##") {
            documentation.push(body.to_owned());
        } else {
            documentation.clear();
        }
    }
    options
}

/// One commented line per default shortcut, in the `[keybindings]` table.
fn keybinding_option_lines() -> Vec<OptionLine> {
    crate::keybindings::defaults()
        .map(|(key, action)| OptionLine {
            section: Some("keybindings".to_owned()),
            key: key.to_owned(),
            line: format!("{key:?} = {:?}", action.id()),
            documentation: Vec::new(),
        })
        .collect()
}

fn commented_options(text: &str) -> HashSet<(Option<String>, String)> {
    let mut options = HashSet::new();
    let mut section = None;
    for line in text.lines() {
        if let Some(body) = comment_body(line) {
            if let Some(header) = table_header(body) {
                section = Some(header.to_owned());
                continue;
            }
            if let Some(keys) = commented_keys(section.as_deref(), body) {
                options.extend(keys.into_iter().map(|key| (section.clone(), key)));
            }
        } else if let Some(header) = table_header(line.trim()) {
            section = Some(header.to_owned());
        }
    }
    options
}

fn commented_keys(section: Option<&str>, body: &str) -> Option<Vec<String>> {
    let source = match section {
        Some(section) => format!("[{section}]\n{body}\n"),
        None => format!("{body}\n"),
    };
    let value = toml::from_str::<toml::Value>(&source).ok()?;
    let table = match section {
        Some(section) => value.get(section)?.as_table()?,
        None => value.as_table()?,
    };
    Some(table.keys().cloned().collect())
}

/// Returns the body of a single- or multi-hash TOML comment.
fn comment_body(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let body = trimmed
        .strip_prefix('#')?
        .trim_start_matches('#')
        .trim_start();
    if body.is_empty() { None } else { Some(body) }
}

fn table_header(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.starts_with("[[") || !line.starts_with('[') {
        return None;
    }
    let end = line.find(']')?;
    let tail = line[end + 1..].trim();
    if !tail.is_empty() && !tail.starts_with('#') {
        return None;
    }
    let section = line[1..end].trim();
    if section.is_empty() {
        None
    } else {
        Some(section.trim_matches(['"', '\'']))
    }
}

/// A `config.toml` document edited key by key, keeping comments, whitespace and key
/// order: only the edited items change.
#[derive(Debug, Clone)]
pub(crate) struct ConfigEdit {
    document: DocumentMut,
    file: PathBuf,
}

impl ConfigEdit {
    /// Parses `text`, the contents of `file`. Invalid TOML is a
    /// [`ConfigErrorKind::Syntax`] error.
    pub(crate) fn parse(text: &str, file: &Path) -> Result<Self, ConfigError> {
        match text.parse::<DocumentMut>() {
            Ok(document) => Ok(Self {
                document,
                file: file.to_path_buf(),
            }),
            Err(error) => Err(ConfigError {
                file: file.to_path_buf(),
                line: error.span().map(|span| line_of(text, span.start)),
                key: None,
                kind: ConfigErrorKind::Syntax,
                detail: Box::new(ConfigMsg::Syntax(error.message().trim_end().to_owned())),
            }),
        }
    }

    /// Sets the key at `path` (`["font", "size"]`, `["keybindings", "Ctrl+X"]`) to
    /// `value`. A missing section table is created at the end of the document, after the
    /// commented defaults; an existing key keeps its comments. A segment of `path` that
    /// holds a value instead of a table is an [`ConfigErrorKind::InvalidValue`] error.
    pub(crate) fn set(
        &mut self,
        path: &[&str],
        value: impl Into<toml_edit::Value>,
    ) -> Result<(), ConfigError> {
        let (key, parents) = path.split_last().expect("a key path is never empty");
        let file = self.file.clone();
        let not_a_table = |end: usize| {
            let name = key_path(&path[..end]);
            ConfigError {
                file: file.clone(),
                line: None,
                key: Some(name.clone()),
                kind: ConfigErrorKind::InvalidValue,
                detail: Box::new(ConfigMsg::MustBeTable(name.to_owned())),
            }
        };

        if let Some(section) = parents.first()
            && !self.document.contains_key(section)
        {
            // Appended after everything, so the trailing commented defaults stay above.
            let mut table = toml_edit::Table::new();
            let trailing = self
                .document
                .trailing()
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if !trailing.is_empty() {
                table.decor_mut().set_prefix(trailing);
                self.document.set_trailing("");
            }
            self.document.insert(section, toml_edit::Item::Table(table));
        }

        let mut table: &mut dyn toml_edit::TableLike = self.document.as_table_mut();
        for (index, parent) in parents.iter().enumerate() {
            if !table.contains_key(parent) {
                table.insert(parent, toml_edit::Item::Table(toml_edit::Table::new()));
            }
            table = table
                .get_mut(parent)
                .and_then(toml_edit::Item::as_table_like_mut)
                .ok_or_else(|| not_a_table(index + 1))?;
        }

        let mut value = value.into();
        if let Some(existing) = table.get_mut(key).and_then(toml_edit::Item::as_value_mut) {
            *value.decor_mut() = existing.decor().clone();
            *existing = value;
        } else {
            table.insert(key, toml_edit::Item::Value(value));
        }
        Ok(())
    }

    /// The value of the key at `path`, `None` when it is missing or holds a table.
    pub(crate) fn value(&self, path: &[&str]) -> Option<&toml_edit::Value> {
        let (key, parents) = path.split_last()?;
        let mut table: &dyn toml_edit::TableLike = self.document.as_table();
        for parent in parents {
            table = table.get(parent)?.as_table_like()?;
        }
        table.get(key)?.as_value()
    }

    /// Removes the key at `path`, if present. An emptied `[keybindings]` table that holds
    /// no comment is removed too.
    pub(crate) fn remove(&mut self, path: &[&str]) {
        let (key, parents) = path.split_last().expect("a key path is never empty");
        let mut table: &mut dyn toml_edit::TableLike = self.document.as_table_mut();
        for parent in parents {
            match table
                .get_mut(parent)
                .and_then(toml_edit::Item::as_table_like_mut)
            {
                Some(child) => table = child,
                None => return,
            }
        }
        table.remove(key);

        if parents == ["keybindings"]
            && let Some(keybindings) = self
                .document
                .get("keybindings")
                .and_then(toml_edit::Item::as_table)
            && keybindings.is_empty()
            && !has_comment(keybindings.decor())
        {
            self.document.remove("keybindings");
        }
    }
}

impl std::fmt::Display for ConfigEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.document.fmt(f)
    }
}

/// Applies `change` to `file`: the file is read again first so hand edits made meanwhile
/// are kept (a missing file starts from `default_text`: [`default_file_text`] for
/// `config.toml`, [`default_keybindings_text`] for `keybindings.toml`), then written next
/// to it and renamed over it, which the watcher sees as one change.
pub(crate) fn write(
    file: &Path,
    default_text: fn() -> String,
    change: impl FnOnce(&mut ConfigEdit) -> Result<(), ConfigError>,
) -> Result<(), ConfigError> {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => default_text(),
        Err(error) => return Err(read_error(file, &error)),
    };
    write_text(file, &text, change)
}

/// Like [`write`], for a file that must already exist (a theme file): a missing file is a
/// [`ConfigErrorKind::Read`] error instead of a new file.
pub(crate) fn write_existing(
    file: &Path,
    change: impl FnOnce(&mut ConfigEdit) -> Result<(), ConfigError>,
) -> Result<(), ConfigError> {
    let text = fs::read_to_string(file).map_err(|error| read_error(file, &error))?;
    write_text(file, &text, change)
}

fn write_text(
    file: &Path,
    text: &str,
    change: impl FnOnce(&mut ConfigEdit) -> Result<(), ConfigError>,
) -> Result<(), ConfigError> {
    let mut edit = ConfigEdit::parse(text, file)?;
    change(&mut edit)?;
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|error| write_error(file, &error))?;
    }
    write_atomic(file, &edit.to_string()).map_err(|error| write_error(file, &error))
}

fn read_error(file: &Path, error: &io::Error) -> ConfigError {
    ConfigError::read(file.to_path_buf(), ConfigMsg::CannotRead(error.to_string()))
}

pub(crate) fn write_error(file: &Path, error: &io::Error) -> ConfigError {
    ConfigError {
        file: file.to_path_buf(),
        line: None,
        key: None,
        kind: ConfigErrorKind::Write,
        detail: Box::new(ConfigMsg::CannotWrite(error.to_string())),
    }
}

fn has_comment(decor: &toml_edit::Decor) -> bool {
    [decor.prefix(), decor.suffix()]
        .into_iter()
        .flatten()
        .any(|raw| raw.as_str().is_some_and(|text| text.contains('#')))
}

/// 1-based line of the byte `offset` in `text`.
fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    pub(super) fn temporary_config(contents: Option<&str>) -> (PathBuf, PathBuf) {
        // A clock alone is not unique: macOS has a coarse one, so parallel
        // tests can read the same value and end up sharing a directory.
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "graphene-prune-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("config.toml");
        if let Some(contents) = contents {
            fs::write(&path, contents).unwrap();
        }
        (directory, path)
    }

    pub(super) fn remove_temporary_config(directory: PathBuf) {
        fs::remove_dir_all(directory).unwrap();
    }

    fn edit(text: &str) -> ConfigEdit {
        ConfigEdit::parse(text, Path::new("config.toml")).unwrap()
    }

    fn parse_config(text: &str) -> crate::config::Config {
        let (config, errors) = crate::config::Config::parse(
            text,
            &crate::config::Config::default(),
            Path::new("config.toml"),
        );
        assert_eq!(errors, Vec::new());
        config
    }

    /// The lines of `after` that are not in `before`.
    fn added_lines(before: &str, after: &str) -> Vec<String> {
        let mut remaining: Vec<&str> = before.lines().collect();
        after
            .lines()
            .filter(|line| match remaining.iter().position(|old| old == line) {
                Some(index) => {
                    remaining.remove(index);
                    false
                }
                None => true,
            })
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn set_on_the_default_file_keeps_every_comment_and_only_adds_the_key() {
        let defaults = default_file_text();
        let mut config = edit(&defaults);
        config.set(&["font", "size"], 16.0).unwrap();
        let text = config.to_string();

        assert!(text.starts_with(&defaults), "{text}");
        assert_eq!(added_lines(&defaults, &text), ["[font]", "size = 16.0"]);
        assert_eq!(parse_config(&text).font.size, 16.0);
    }

    #[test]
    fn set_on_the_root_keeps_every_comment() {
        let defaults = default_file_text();
        let mut config = edit(&defaults);
        config.set(&["language"], "fr").unwrap();
        let text = config.to_string();

        assert_eq!(added_lines(&defaults, &text), ["language = \"fr\""]);
        assert_eq!(parse_config(&text).language, crate::config::Language::Fr);
    }

    #[test]
    fn set_on_an_existing_key_keeps_its_comments() {
        let mut config =
            edit("# Fonts.\n[font]\n# Bigger.\nsize = 12.0 # was 14\nfamily = \"Iosevka\"\n");
        config.set(&["font", "size"], 18.0).unwrap();
        assert_eq!(
            config.to_string(),
            "# Fonts.\n[font]\n# Bigger.\nsize = 18.0 # was 14\nfamily = \"Iosevka\"\n"
        );
    }

    #[test]
    fn set_adds_a_key_to_an_existing_section() {
        let mut config = edit("[font]\nfamily = \"Iosevka\"\n\n[window]\npadding = 4\n");
        config.set(&["font", "size"], 18.0).unwrap();
        assert_eq!(
            config.to_string(),
            "[font]\nfamily = \"Iosevka\"\nsize = 18.0\n\n[window]\npadding = 4\n"
        );
    }

    #[test]
    fn set_quotes_a_keybinding_key() {
        let mut config = edit("");
        config
            .set(&["keybindings", "Ctrl+Alt+T"], "new_tab")
            .unwrap();
        let text = config.to_string();
        assert_eq!(text, "[keybindings]\n\"Ctrl+Alt+T\" = \"new_tab\"\n");
        let (keybindings, errors, _) = crate::config::Config::parse_keybindings(
            &text,
            &crate::config::Config::default(),
            Path::new("keybindings.toml"),
        );
        assert_eq!(errors, Vec::new());
        assert_eq!(
            keybindings.get("Ctrl+Alt+T").map(String::as_str),
            Some("new_tab")
        );
    }

    #[test]
    fn set_under_a_value_is_an_error() {
        let mut config = edit("font = 3\n");
        let error = config.set(&["font", "size"], 18.0).unwrap_err();
        assert_eq!(error.key.as_deref(), Some("font"));
        assert_eq!(error.kind, ConfigErrorKind::InvalidValue);
        assert_eq!(config.to_string(), "font = 3\n");
    }

    #[test]
    fn remove_drops_a_key_and_an_emptied_keybindings_table() {
        let mut config = edit("[font]\nsize = 12.0\n\n[keybindings]\n\"Ctrl+X\" = \"copy\"\n");
        config.remove(&["keybindings", "Ctrl+X"]);
        assert_eq!(config.to_string(), "[font]\nsize = 12.0\n");
        config.remove(&["font", "size"]);
        assert_eq!(config.to_string(), "[font]\n");
    }

    #[test]
    fn remove_keeps_a_commented_keybindings_table() {
        let text = "# My shortcuts.\n[keybindings]\n\"Ctrl+X\" = \"copy\"\n";
        let mut config = edit(text);
        config.remove(&["keybindings", "Ctrl+X"]);
        assert_eq!(config.to_string(), "# My shortcuts.\n[keybindings]\n");
    }

    #[test]
    fn remove_of_a_missing_key_changes_nothing() {
        let text = "# Note.\n[font]\nsize = 12.0\n";
        let mut config = edit(text);
        config.remove(&["window", "padding"]);
        config.remove(&["font", "family"]);
        assert_eq!(config.to_string(), text);
    }

    #[test]
    fn parse_rejects_invalid_toml() {
        let error =
            ConfigEdit::parse("[font]\nsize = = 1\n", Path::new("config.toml")).unwrap_err();
        assert_eq!(error.kind, ConfigErrorKind::Syntax);
        assert_eq!(error.line, Some(2));
        assert_eq!(error.key, None);
    }

    #[test]
    fn write_starts_from_the_defaults_when_the_file_is_missing() {
        let (directory, path) = temporary_config(None);
        write(&path, default_file_text, |config| {
            config.set(&["font", "size"], 20.0)
        })
        .unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(&default_file_text()));
        assert_eq!(parse_config(&text).font.size, 20.0);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        remove_temporary_config(directory);
    }

    #[test]
    fn write_reads_the_file_again_and_keeps_hand_edits() {
        let (directory, path) = temporary_config(Some("# Mine.\n[font]\nsize = 12.0\n"));
        write(&path, default_file_text, |config| {
            config.set(&["window", "padding"], 4)
        })
        .unwrap();
        fs::write(
            &path,
            "# Mine.\n[font]\nsize = 13.0\n[window]\npadding = 4\n",
        )
        .unwrap();
        write(&path, default_file_text, |config| {
            config.set(&["cursor", "blinking"], true)
        })
        .unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            "# Mine.\n[font]\nsize = 13.0\n[window]\npadding = 4\n\n[cursor]\nblinking = true\n"
        );
        let config = parse_config(&text);
        assert_eq!(config.font.size, 13.0);
        assert!(config.cursor.blinking);
        remove_temporary_config(directory);
    }

    #[test]
    fn write_existing_fails_on_a_missing_file() {
        let (directory, path) = temporary_config(None);
        let error =
            write_existing(&path, |config| config.set(&["font", "size"], 20.0)).unwrap_err();
        assert_eq!(error.kind, ConfigErrorKind::Read);
        assert!(!path.exists());
        remove_temporary_config(directory);
    }

    #[test]
    fn write_existing_edits_only_the_given_key() {
        let (directory, path) = temporary_config(Some("# Mine.\n[font]\nsize = 12.0 # big\n"));
        write_existing(&path, |config| config.set(&["font", "size"], 13.0)).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# Mine.\n[font]\nsize = 13.0 # big\n"
        );
        remove_temporary_config(directory);
    }

    #[test]
    fn value_reads_a_nested_key() {
        let config = edit("[colors.normal]\nred = \"#ff0000\"\n");
        assert_eq!(
            config
                .value(&["colors", "normal", "red"])
                .and_then(toml_edit::Value::as_str),
            Some("#ff0000")
        );
        assert!(config.value(&["colors", "normal", "blue"]).is_none());
        assert!(config.value(&["colors", "normal"]).is_none());
        assert!(config.value(&["ui", "pane_border"]).is_none());
    }

    #[test]
    fn write_leaves_an_invalid_file_untouched() {
        let (directory, path) = temporary_config(Some("[font\n"));
        let error = write(&path, default_file_text, |config| {
            config.set(&["font", "size"], 20.0)
        })
        .unwrap_err();
        assert_eq!(error.kind, ConfigErrorKind::Syntax);
        assert_eq!(fs::read_to_string(&path).unwrap(), "[font\n");
        remove_temporary_config(directory);
    }

    #[test]
    fn write_keeps_the_file_when_the_change_fails() {
        let (directory, path) = temporary_config(Some("font = 3\n"));
        let error = write(&path, default_file_text, |config| {
            config.set(&["font", "size"], 20.0)
        })
        .unwrap_err();
        assert_eq!(error.kind, ConfigErrorKind::InvalidValue);
        assert_eq!(fs::read_to_string(&path).unwrap(), "font = 3\n");
        remove_temporary_config(directory);
    }
}
