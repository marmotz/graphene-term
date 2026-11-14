//! Text-preserving completion of missing configuration options.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::ops::Range;
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
    let options = option_lines();
    let normalized = normalize_malformed_comments(text, &options);
    let text = normalized.as_deref().unwrap_or(text);
    let document = text.parse::<DocumentMut>()?;
    let commented = commented_options(text);
    let missing: Vec<_> = options
        .into_iter()
        .filter(|option| {
            !active_contains(&document, option)
                && !commented.contains(&(option.section.clone(), option.key.clone()))
        })
        .collect();

    if missing.is_empty() {
        return Ok(normalized);
    }

    let groups = grouped_options(missing);
    let blocks = section_blocks(text);
    let newline = newline(text);
    let mut insertions = Vec::new();
    let mut appended_sections = String::new();

    for (section, options) in groups {
        if let Some(section) = section {
            if let Some(block) = blocks
                .iter()
                .filter(|block| block.name == section && block.active)
                .chain(
                    blocks
                        .iter()
                        .filter(|block| block.name == section && !block.active),
                )
                .next()
            {
                let position = insertion_point(text, block.start, block.end);
                insertions.push((position, render_options(&options, position, text, &newline)));
            } else {
                if appended_sections.is_empty() {
                    if !text.is_empty() && !text.ends_with(&format!("{newline}{newline}")) {
                        appended_sections.push_str(&newline);
                        if !text.ends_with(&newline) {
                            appended_sections.push_str(&newline);
                        }
                    }
                } else if !appended_sections.ends_with(&format!("{newline}{newline}")) {
                    appended_sections.push_str(&newline);
                }
                appended_sections.push_str("# [");
                appended_sections.push_str(&section);
                appended_sections.push(']');
                appended_sections.push_str(&newline);
                for option in options {
                    appended_sections.push_str("# ");
                    appended_sections.push_str(&option.line);
                    appended_sections.push_str(&newline);
                }
            }
        } else {
            let root_end = blocks.first().map_or(text.len(), |block| block.start);
            let position = insertion_point(text, 0, root_end);
            insertions.push((position, render_options(&options, position, text, &newline)));
        }
    }

    if !appended_sections.is_empty() {
        insertions.push((text.len(), appended_sections));
    }

    let mut merged_insertions: Vec<(usize, String)> = Vec::new();
    for (position, insertion) in insertions {
        if let Some((last_position, last_insertion)) = merged_insertions.last_mut()
            && *last_position == position
        {
            last_insertion.push_str(&insertion);
        } else {
            merged_insertions.push((position, insertion));
        }
    }
    merged_insertions.sort_by_key(|(position, _)| std::cmp::Reverse(*position));
    let mut updated = text.to_owned();
    for (position, insertion) in merged_insertions {
        updated.insert_str(position, &insertion);
    }

    updated.parse::<DocumentMut>()?;
    Ok(Some(updated))
}

fn grouped_options(options: Vec<OptionLine>) -> Vec<(Option<String>, Vec<OptionLine>)> {
    let mut groups: Vec<(Option<String>, Vec<OptionLine>)> = Vec::new();
    for option in options {
        if let Some((section, group)) = groups.last_mut()
            && *section == option.section
        {
            group.push(option);
        } else {
            groups.push((option.section.clone(), vec![option]));
        }
    }
    groups
}

fn render_options(options: &[OptionLine], position: usize, text: &str, newline: &str) -> String {
    let mut rendered = String::new();
    if position > 0 && !text[..position].ends_with('\n') {
        rendered.push_str(newline);
    }
    for option in options {
        rendered.push_str("# ");
        rendered.push_str(&option.line);
        rendered.push_str(newline);
    }
    rendered
}

fn newline(text: &str) -> String {
    if text.contains("\r\n") {
        "\r\n".to_owned()
    } else {
        "\n".to_owned()
    }
}

#[derive(Debug)]
struct SectionBlock {
    name: String,
    start: usize,
    end: usize,
    active: bool,
}

fn section_blocks(text: &str) -> Vec<SectionBlock> {
    let mut blocks: Vec<SectionBlock> = Vec::new();
    let mut offset = 0;
    for raw_line in text.split_inclusive('\n') {
        let line = raw_line.trim_end_matches(['\r', '\n']);
        let trimmed = line.trim_start();
        let (header, active) = if trimmed.starts_with('#') {
            (comment_body(trimmed).and_then(table_header), false)
        } else {
            (table_header(trimmed), true)
        };
        if let Some(name) = header {
            if let Some(previous) = blocks.last_mut() {
                previous.end = offset;
            }
            blocks.push(SectionBlock {
                name: name.to_owned(),
                start: offset,
                end: text.len(),
                active,
            });
        }
        offset += raw_line.len();
    }
    blocks
}

fn insertion_point(text: &str, start: usize, end: usize) -> usize {
    let mut point = start;
    let mut offset = start;
    for raw_line in text[start..end].split_inclusive('\n') {
        if !raw_line.trim().is_empty() {
            point = offset + raw_line.len();
        }
        offset += raw_line.len();
    }
    point
}

fn normalize_malformed_comments(text: &str, options: &[OptionLine]) -> Option<String> {
    let mut normalized = String::with_capacity(text.len());
    let mut section = None;
    let mut changed = false;

    for raw_line in text.split_inclusive('\n') {
        let line = raw_line.trim_end_matches(['\r', '\n']);
        let trimmed = line.trim_start();
        let Some(body) = comment_body(trimmed) else {
            if let Some(header) = table_header(trimmed) {
                section = Some(header.to_owned());
            }
            normalized.push_str(raw_line);
            continue;
        };
        if let Some(header) = table_header(body) {
            section = Some(header.to_owned());
            normalized.push_str(raw_line);
            continue;
        }

        let malformed_option = body.split_once('=').and_then(|(key, _)| {
            let key = key.trim().trim_matches(['"', '\'']).trim();
            options
                .iter()
                .find(|option| option.section == section && option.key == key)
                .filter(|_| commented_keys(section.as_deref(), body).is_none())
        });
        if let Some(option) = malformed_option {
            let indentation = line.len() - trimmed.len();
            normalized.push_str(&line[..indentation]);
            normalized.push_str("# ");
            normalized.push_str(&option.line);
            normalized.push_str(&raw_line[line.len()..]);
            changed = true;
        } else {
            normalized.push_str(raw_line);
        }
    }

    changed.then_some(normalized)
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

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PruneOutcome {
    CreatedDefault,
    NothingToPrune,
    Declined(Vec<String>),
    Pruned(Vec<String>),
}

/// Removes only keys the configuration parser does not recognize. The callback is
/// invoked with the complete list before any file is modified.
pub(crate) fn prune_file(
    path: &Path,
    mut confirm: impl FnMut(&[String]) -> bool,
) -> anyhow::Result<PruneOutcome> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let defaults = complete_defaults(DEFAULT_CONFIG)
                .map_err(|error| {
                    anyhow::anyhow!("invalid embedded default configuration: {error}")
                })?
                .unwrap_or_else(|| DEFAULT_CONFIG.to_owned());
            write_atomic(path, &defaults)?;
            return Ok(PruneOutcome::CreatedDefault);
        }
        Err(error) => return Err(error.into()),
    };

    // Validate syntax before asking for confirmation or computing any edits. In
    // particular, never rewrite a file that toml_edit cannot parse.
    let document = toml_edit::Document::parse(text.to_owned())
        .map_err(|error| anyhow::anyhow!("invalid TOML in {}: {error}", path.display()))?;
    let (_, _, unknown) = crate::config::Config::parse_with_unknown_keys(
        &text,
        &crate::config::Config::default(),
        path,
    );
    if unknown.is_empty() {
        return Ok(PruneOutcome::NothingToPrune);
    }

    let paths: Vec<_> = unknown.iter().map(|key| key.path.clone()).collect();
    if !confirm(&paths) {
        return Ok(PruneOutcome::Declined(paths));
    }

    let mut ranges = Vec::with_capacity(unknown.len());
    for key in &unknown {
        let item = match &key.section {
            None => document.as_table().get(&key.key),
            Some(section) => document
                .as_table()
                .get(section)
                .and_then(toml_edit::Item::as_table_like)
                .and_then(|table| table.get(&key.key)),
        }
        .ok_or_else(|| anyhow::anyhow!("cannot locate unknown configuration key `{}`", key.path))?;
        let span = item.span().ok_or_else(|| {
            anyhow::anyhow!("cannot locate source for unknown key `{}`", key.path)
        })?;
        let range = if key.whole_section && item.as_table().is_some() {
            let section_span = span.start..item_source_end(item).unwrap_or(span.end);
            full_line_range(&text, section_span)?
        } else {
            match &key.section {
                Some(section)
                    if document
                        .as_table()
                        .get(section)
                        .is_some_and(toml_edit::Item::is_value) =>
                {
                    let table = document
                        .as_table()
                        .get(section)
                        .and_then(toml_edit::Item::as_table_like)
                        .expect("inline table is table-like");
                    let key_span = table
                        .key(&key.key)
                        .and_then(toml_edit::Key::span)
                        .ok_or_else(|| anyhow::anyhow!("cannot locate key `{}`", key.path))?;
                    inline_entry_range(&text, key_span.start..span.end)?
                }
                _ => full_line_range(&text, span)?,
            }
        };
        ranges.push(range);
    }

    // A whole unknown section owns a range that contains its nested entries; avoid
    // overlapping edits when the parser reports any additional paths within it.
    ranges.sort_by_key(|range| range.start);
    ranges.dedup();
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }

    let mut updated = text.clone();
    for range in merged.into_iter().rev() {
        updated.replace_range(range, "");
    }
    updated
        .parse::<DocumentMut>()
        .map_err(|error| anyhow::anyhow!("pruning would produce invalid TOML: {error}"))?;
    let (_, _, remaining) = crate::config::Config::parse_with_unknown_keys(
        &updated,
        &crate::config::Config::default(),
        path,
    );
    if !remaining.is_empty() {
        anyhow::bail!(
            "pruning did not remove all unknown configuration keys: {:?}",
            remaining.iter().map(|key| &key.path).collect::<Vec<_>>()
        );
    }
    write_atomic(path, &updated)?;
    Ok(PruneOutcome::Pruned(paths))
}

fn item_source_end(item: &toml_edit::Item) -> Option<usize> {
    let own_end = item.span()?.end;
    let nested_end = item
        .as_table()
        .into_iter()
        .flat_map(toml_edit::Table::iter)
        .filter_map(|(_, child)| item_source_end(child))
        .max();
    Some(nested_end.unwrap_or(own_end).max(own_end))
}

fn full_line_range(text: &str, span: Range<usize>) -> anyhow::Result<Range<usize>> {
    if span.start > span.end
        || span.end > text.len()
        || !text.is_char_boundary(span.start)
        || !text.is_char_boundary(span.end)
    {
        anyhow::bail!("configuration parser returned an invalid source span");
    }
    let start = text[..span.start].rfind('\n').map_or(0, |index| index + 1);
    let end = text[span.end..]
        .find('\n')
        .map_or(text.len(), |index| span.end + index + 1);
    Ok(start..end)
}

fn inline_entry_range(text: &str, span: Range<usize>) -> anyhow::Result<Range<usize>> {
    if span.start > span.end
        || span.end > text.len()
        || !text.is_char_boundary(span.start)
        || !text.is_char_boundary(span.end)
    {
        anyhow::bail!("configuration parser returned an invalid source span");
    }
    let bytes = text.as_bytes();
    let mut start = span.start;
    let mut end = span.end;
    while start > 0 && bytes[start - 1].is_ascii_whitespace() {
        start -= 1;
    }
    while end < bytes.len() && bytes[end].is_ascii_whitespace() {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b',' {
        end += 1;
        while end < bytes.len() && bytes[end].is_ascii_whitespace() {
            end += 1;
        }
    } else if start > 0 && bytes[start - 1] == b',' {
        start -= 1;
    }
    if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        anyhow::bail!("configuration parser returned a non-character source span");
    }
    Ok(start..end)
}

/// Runs the explicit cleanup command using the standard XDG configuration location.
pub(crate) fn prune_config(yes: bool) -> anyhow::Result<()> {
    let path = crate::config::config_file().ok_or_else(|| {
        anyhow::anyhow!("cannot locate the configuration directory: set HOME or XDG_CONFIG_HOME")
    })?;
    let stdin_tty = io::stdin().is_terminal();
    let stdout_tty = io::stdout().is_terminal();
    let outcome = prune_file(&path, |paths| {
        println!("Unknown configuration keys:");
        for key in paths {
            println!("  - {key}");
        }
        if yes {
            return true;
        }
        if !stdin_tty || !stdout_tty {
            eprintln!("Not pruning configuration: confirmation requires a terminal or --yes.");
            return false;
        }
        print!("Remove these keys? [y/N] ");
        let _ = io::stdout().flush();
        let mut answer = String::new();
        io::stdin().read_line(&mut answer).is_ok()
            && matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    })?;
    match outcome {
        PruneOutcome::CreatedDefault => {
            println!(
                "Created {} from the default configuration; nothing to prune.",
                path.display()
            );
        }
        PruneOutcome::NothingToPrune => println!("No unknown configuration keys to prune."),
        PruneOutcome::Declined(_) => println!("Configuration left unchanged."),
        PruneOutcome::Pruned(paths) => {
            println!("Removed {} unknown configuration key(s).", paths.len())
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temporary_config(contents: Option<&str>) -> (PathBuf, PathBuf) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
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

    fn remove_temporary_config(directory: PathBuf) {
        fs::remove_dir_all(directory).unwrap();
    }

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
        assert!(commented_options(&completed).contains(&(None, "language".to_owned())));
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
        assert_eq!(updated.matches("# [keybindings]").count(), 1);
        assert!(updated.contains(original));
    }

    #[test]
    fn missing_options_are_inserted_into_existing_sections_without_duplicate_headers() {
        let original = concat!(
            "[window]\n",
            "# width = 960\n",
            "background_opacity = 0.9\n",
            "# [scrollback]\n",
            "# lines = 10000\n",
            "[cursor]\n",
            "# \"shape = \"block\"\n",
            "blinking = true\n",
        );

        let updated = complete_defaults(original).unwrap().unwrap();

        assert_eq!(updated.matches("[window]").count(), 1);
        assert_eq!(updated.matches("# [window]").count(), 0);
        assert_eq!(updated.matches("# startup_mode = \"default\"").count(), 1);
        assert!(
            updated.find("# startup_mode = \"default\"").unwrap()
                < updated.find("# [scrollback]").unwrap()
        );
        assert_eq!(updated.matches("[cursor]").count(), 1);
        assert_eq!(updated.matches("# [cursor]").count(), 0);
        assert_eq!(updated.matches("# shape = \"block\"").count(), 1);
        assert!(updated.contains("background_opacity = 0.9"));
        assert!(updated.contains("blinking = true"));
        assert!(updated.parse::<DocumentMut>().is_ok());
        assert_eq!(complete_defaults(&updated).unwrap(), None);
    }

    #[test]
    fn root_options_are_inserted_before_the_first_section() {
        let original = "[window]\nwidth = 900\n";
        let updated = complete_defaults(original).unwrap().unwrap();

        assert!(updated.find("# language = \"auto\"").unwrap() < updated.find("[window]").unwrap());
        assert_eq!(updated.matches("[window]").count(), 1);
        assert!(updated.contains("width = 900"));
    }

    #[test]
    fn absent_sections_get_one_commented_header() {
        let original = "[window]\nwidth = 900\n";
        let updated = complete_defaults(original).unwrap().unwrap();

        assert_eq!(updated.matches("# [font]").count(), 1);
        assert_eq!(updated.matches("# [updates]").count(), 1);
        assert!(updated.contains("# family = \"\""));
        assert!(updated.contains("# check = true"));
    }

    #[test]
    fn invalid_toml_is_rejected_without_a_replacement() {
        assert!(complete_defaults("[font\nsize = 12\n").is_err());
        assert!(complete_defaults("[cursor]\nshape = \"unterminated\n").is_err());
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

    #[test]
    fn prune_removes_only_unknown_root_and_nested_keys() {
        let (directory, path) = temporary_config(Some(
            "language = \"fr\"\nfuture_root_option = \"preserve until confirmed\"\n[font]\nsize = \"invalid-but-known\"\nfuture_font_option = 12\n[plugin]\nfuture_plugin_option = true\n[window]\nwidth = 1024\n",
        ));
        let outcome = prune_file(&path, |keys| {
            assert_eq!(
                keys,
                &["font.future_font_option", "future_root_option", "plugin"]
            );
            true
        })
        .unwrap();
        assert_eq!(
            outcome,
            PruneOutcome::Pruned(vec![
                "font.future_font_option".to_owned(),
                "future_root_option".to_owned(),
                "plugin".to_owned()
            ])
        );
        let updated = fs::read_to_string(&path).unwrap();
        assert!(updated.contains("language = \"fr\""));
        assert!(updated.contains("size = \"invalid-but-known\""));
        assert!(updated.contains("width = 1024"));
        assert!(!updated.contains("future_root_option"));
        assert!(!updated.contains("future_font_option"));
        assert!(!updated.contains("[plugin]"));
        remove_temporary_config(directory);
    }

    #[test]
    fn prune_refusal_does_not_modify_the_file() {
        let original = "[font]\nfuture_option = 1\nsize = 16\n";
        let (directory, path) = temporary_config(Some(original));
        let outcome = prune_file(&path, |keys| {
            assert_eq!(keys, &["font.future_option"]);
            false
        })
        .unwrap();
        assert_eq!(
            outcome,
            PruneOutcome::Declined(vec!["font.future_option".into()])
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        remove_temporary_config(directory);
    }

    #[test]
    fn prune_confirmation_yes_removes_keys_and_preserves_inline_known_values() {
        let original = "font = { size = \"invalid-but-known\", future_option = 1 }\n";
        let (directory, path) = temporary_config(Some(original));
        let outcome = prune_file(&path, |keys| {
            assert_eq!(keys, &["font.future_option"]);
            true
        })
        .unwrap();
        assert_eq!(
            outcome,
            PruneOutcome::Pruned(vec!["font.future_option".into()])
        );
        let updated = fs::read_to_string(&path).unwrap();
        assert!(updated.contains("size = \"invalid-but-known\""));
        assert!(!updated.contains("future_option"));
        updated.parse::<DocumentMut>().unwrap();
        remove_temporary_config(directory);
    }

    #[test]
    fn prune_removes_unknown_nested_table_contents_without_touching_parent_settings() {
        let original = "[font]\nsize = 17\n[font.future]\nvalue = 1\n[window]\nwidth = 900\n";
        let (directory, path) = temporary_config(Some(original));
        let outcome = prune_file(&path, |keys| {
            assert_eq!(keys, &["font.future"]);
            true
        })
        .unwrap();
        assert_eq!(outcome, PruneOutcome::Pruned(vec!["font.future".into()]));
        let updated = fs::read_to_string(&path).unwrap();
        assert!(updated.contains("size = 17"));
        assert!(updated.contains("width = 900"));
        assert!(!updated.contains("[font.future]"));
        assert!(!updated.contains("value = 1"));
        updated.parse::<DocumentMut>().unwrap();
        remove_temporary_config(directory);
    }

    #[test]
    fn prune_creates_completed_default_file_when_missing_without_confirmation() {
        let (directory, path) = temporary_config(None);
        let outcome = prune_file(&path, |_| panic!("default creation must not ask")).unwrap();
        assert_eq!(outcome, PruneOutcome::CreatedDefault);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("# \"Ctrl+Shift+T\" = \"new_tab\""));
        assert!(contents.parse::<DocumentMut>().is_ok());
        remove_temporary_config(directory);
    }

    #[test]
    fn invalid_toml_is_never_changed_or_confirmed() {
        let original = "[font\nsize = 12\n";
        let (directory, path) = temporary_config(Some(original));
        assert!(prune_file(&path, |_| panic!("invalid TOML must not ask")).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        remove_temporary_config(directory);
    }

    #[test]
    fn no_unknown_keys_does_not_ask_for_confirmation() {
        let original = "language = \"fr\"\n";
        let (directory, path) = temporary_config(Some(original));
        assert_eq!(
            prune_file(&path, |_| panic!("nothing to prune must not ask")).unwrap(),
            PruneOutcome::NothingToPrune
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        remove_temporary_config(directory);
    }
}
