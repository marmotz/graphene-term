//! Text-preserving completion of missing configuration options.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use toml_edit::DocumentMut;

use crate::config::DEFAULT_CONFIG;

#[derive(Debug, Clone)]
struct OptionLine {
    section: Option<String>,
    key: String,
    line: String,
}

/// Returns commented entries for supported options not already present in the document.
/// `None` means the document is already complete. Invalid TOML is never rewritten.
pub(super) fn complete_defaults(text: &str) -> Result<Option<String>, toml_edit::TomlError> {
    let document = text.parse::<DocumentMut>()?;
    let options = option_lines();
    let commented = commented_options(text);
    let missing: Vec<_> = options
        .into_iter()
        .filter(|option| {
            !active_contains(&document, option)
                && !commented.contains(&(option.section.clone(), option.key.clone()))
        })
        .collect();

    if missing.is_empty() {
        return Ok(None);
    }

    let mut updated = String::with_capacity(text.len() + missing.len() * 32);
    updated.push_str(text);
    if !text.is_empty() && !text.ends_with('\n') {
        updated.push('\n');
    }
    if !text.is_empty() && !text.ends_with("\n\n") {
        updated.push('\n');
    }
    updated.push_str("## Added automatically because these supported options were missing.\n");

    let mut current_section: Option<String> = None;
    for option in missing {
        if current_section != option.section {
            if current_section.is_some() || option.section.is_some() {
                updated.push('\n');
            }
            if let Some(section) = &option.section {
                updated.push_str("# [");
                updated.push_str(section);
                updated.push_str("]\n");
            }
            current_section.clone_from(&option.section);
        }
        updated.push_str("# ");
        updated.push_str(&option.line);
        updated.push('\n');
    }
    Ok(Some(updated))
}

/// Writes a complete replacement next to the target and atomically renames it into place.
pub(super) fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let metadata = fs::metadata(path).ok();
    let mut temp_path = None;
    let mut temp_file = None;

    for _ in 0..100 {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), suffix));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temp_path = Some(candidate);
                temp_file = Some(file);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    let temp_path = temp_path.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a temporary configuration file",
        )
    })?;
    let result = (|| {
        let mut temp_file = temp_file.expect("temporary path and file are set together");
        if let Some(metadata) = metadata {
            temp_file.set_permissions(metadata.permissions())?;
        }
        temp_file.write_all(contents.as_bytes())?;
        temp_file.sync_all()?;
        drop(temp_file);
        fs::rename(&temp_path, path)?;
        // Persist the rename where the platform supports syncing directories.
        if let Ok(directory) = OpenOptions::new().read(true).open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn active_contains(document: &DocumentMut, option: &OptionLine) -> bool {
    match &option.section {
        None => document.as_table().contains_key(&option.key),
        Some(section) => document
            .as_table()
            .get(section)
            .and_then(|item| item.as_table_like())
            .is_some_and(|table| table.contains_key(&option.key)),
    }
}

fn option_lines() -> Vec<OptionLine> {
    let mut options = Vec::new();
    let mut section = None;
    for line in DEFAULT_CONFIG.lines() {
        let Some(body) = comment_body(line) else {
            continue;
        };
        if let Some(header) = table_header(body) {
            section = Some(header.to_owned());
            continue;
        }
        if section.as_deref() == Some("keybindings") {
            continue;
        }
        let Some((key, _)) = body.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        options.push(OptionLine {
            section: section.clone(),
            key: key.to_owned(),
            line: body.to_owned(),
        });
    }

    for (key, action) in crate::keybindings::defaults() {
        let line = format!("{key:?} = {:?}", action.id());
        options.push(OptionLine {
            section: Some("keybindings".to_owned()),
            key: key.to_owned(),
            line,
        });
    }
    options
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
            let Some((key, _)) = body.split_once('=') else {
                continue;
            };
            let key = key.trim();
            if section.as_deref() == Some("keybindings") {
                // TOML parses quoted shortcut keys (including escaped characters) for us.
                let snippet = format!("[keybindings]\n{body}\n");
                if let Ok(value) = toml::from_str::<toml::Value>(&snippet)
                    && let Some(table) = value.get("keybindings").and_then(toml::Value::as_table)
                {
                    options.extend(table.keys().map(|key| (section.clone(), key.clone())));
                }
            } else if !key.is_empty() {
                options.insert((section.clone(), key.to_owned()));
            }
        } else if let Some(header) = table_header(line.trim()) {
            section = Some(header.to_owned());
        }
    }
    options
}

/// Returns the body of a single- or multi-hash TOML comment.
fn comment_body(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let body = trimmed.trim_start_matches('#').trim_start();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_adds_all_documented_options_and_default_keybindings() {
        let completed = complete_defaults("").unwrap().unwrap();
        for option in option_lines() {
            assert!(
                completed.contains(&format!("# {}\n", option.line)),
                "missing generated option: {:?}",
                option
            );
        }
        for (key, action) in crate::keybindings::defaults() {
            assert!(completed.contains(&format!("# {key:?} = {:?}\n", action.id())));
        }
        assert_eq!(complete_defaults(&completed).unwrap(), None);
    }

    #[test]
    fn active_and_commented_options_custom_values_and_unknown_keys_are_preserved() {
        let original = "# user's note\nlanguage = \"fr\"\n[font]\nsize = 18\n# family = \"Custom Font\"\n[plugin]\nvalue = 42\n";
        let updated = complete_defaults(original).unwrap().unwrap();
        assert!(updated.starts_with(original));
        assert_eq!(updated.matches("# family = \"Custom Font\"").count(), 1);
        assert_eq!(updated.matches("language = \"fr\"").count(), 1);
        assert_eq!(updated.matches("size = 18").count(), 1);
        assert_eq!(updated.matches("[plugin]").count(), 1);
        assert!(updated.contains("value = 42"));
        assert!(!updated.contains("# size = 14.0"));
    }

    #[test]
    fn commented_binding_is_not_duplicated_or_replaced() {
        let original = "# [keybindings]\n# \"Ctrl+Shift+C\" = \"copy\"\n";
        let updated = complete_defaults(original).unwrap().unwrap();
        assert_eq!(updated.matches("\"Ctrl+Shift+C\" = \"copy\"").count(), 1);
        assert!(updated.starts_with(original));
    }

    #[test]
    fn invalid_toml_is_rejected_without_a_replacement() {
        assert!(complete_defaults("[font\nsize = 12\n").is_err());
    }

    #[test]
    fn atomic_writer_replaces_the_file_contents() {
        let dir = std::env::temp_dir().join(format!(
            "graphene-config-edit-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        fs::write(&path, "old").unwrap();
        write_atomic(&path, "new\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
}
