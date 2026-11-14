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
    documentation: Vec<String>,
}

/// Completes supported options and their value documentation without changing configured
/// values. `None` means the document is already complete. Invalid TOML is never rewritten.
pub(super) fn complete_defaults(text: &str) -> Result<Option<String>, toml_edit::TomlError> {
    let options = option_lines();
    let legacy = normalize_legacy_documentation(text);
    let source = legacy.as_deref().unwrap_or(text);
    let malformed = normalize_malformed_comments(source, &options);
    let text = malformed.as_deref().unwrap_or(source);
    let normalized = (legacy.is_some() || malformed.is_some()).then(|| text.to_owned());
    let document = text.parse::<DocumentMut>()?;
    let commented = commented_options(text);
    let newline = newline(text);
    let blocks = section_blocks(text);
    let mut missing = Vec::new();
    let mut insertions = Vec::new();

    if let Some(block) = blocks
        .iter()
        .filter(|block| block.name == "keybindings" && block.active)
        .chain(
            blocks
                .iter()
                .filter(|block| block.name == "keybindings" && !block.active),
        )
        .next()
    {
        let documentation = missing_keybinding_documentation(text, block);
        if !documentation.is_empty() {
            insertions.push((
                section_header_end(text, block),
                render_documentation(&documentation, &newline),
            ));
        }
    }
    insertions.extend(commented_documentation_insertions(text, &options, &newline));
    insertions.extend(active_documentation_insertions(text, &options, &newline));

    for option in &options {
        if active_item(&document, option).is_none()
            && !commented.contains(&(option.section.clone(), option.key.clone()))
        {
            missing.push(option.clone());
        }
    }

    if missing.is_empty() && insertions.is_empty() {
        return Ok(normalized);
    }

    let groups = grouped_options(missing);
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
                if section == "keybindings" {
                    append_documentation(
                        &mut appended_sections,
                        &keybinding_documentation(),
                        &newline,
                    );
                }
                for option in options {
                    append_documentation(&mut appended_sections, &option.documentation, &newline);
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
        append_documentation(&mut rendered, &option.documentation, newline);
        rendered.push_str("# ");
        rendered.push_str(&option.line);
        rendered.push_str(newline);
    }
    rendered
}

fn render_documentation(documentation: &[String], newline: &str) -> String {
    let mut rendered = String::new();
    append_documentation(&mut rendered, documentation, newline);
    rendered
}

fn option_documentation(option: &OptionLine) -> Vec<String> {
    option.documentation.clone()
}

fn missing_option_documentation(text: &str, line_start: usize, expected: &[String]) -> Vec<String> {
    if expected.is_empty() {
        return Vec::new();
    }
    let mut has_existing_help = false;
    for line in text[..line_start].lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if !line.starts_with('#') {
            break;
        }
        let Some(body) = comment_body(line) else {
            continue;
        };
        if table_header(body).is_some() || body.contains('=') {
            break;
        }
        has_existing_help = true;
    }
    if has_existing_help {
        Vec::new()
    } else {
        expected.to_vec()
    }
}

fn commented_documentation_insertions(
    text: &str,
    options: &[OptionLine],
    newline: &str,
) -> Vec<(usize, String)> {
    let mut insertions = Vec::new();
    let mut section = None;
    let mut offset = 0;
    for raw_line in text.split_inclusive('\n') {
        let trimmed = raw_line.trim_start();
        if let Some(body) = comment_body(trimmed) {
            if let Some(header) = table_header(body) {
                section = Some(header.to_owned());
            } else if let Some(keys) = commented_keys(section.as_deref(), body) {
                for key in keys {
                    if let Some(option) = options
                        .iter()
                        .find(|option| option.section == section && option.key == key)
                    {
                        let documentation = missing_option_documentation(
                            text,
                            offset,
                            &option_documentation(option),
                        );
                        if !documentation.is_empty() {
                            insertions
                                .push((offset, render_documentation(&documentation, newline)));
                        }
                    }
                }
            }
        } else if let Some(header) = table_header(trimmed) {
            section = Some(header.to_owned());
        }
        offset += raw_line.len();
    }
    insertions
}

fn active_documentation_insertions(
    text: &str,
    options: &[OptionLine],
    newline: &str,
) -> Vec<(usize, String)> {
    let mut insertions = Vec::new();
    let mut section = None;
    let mut offset = 0;
    for raw_line in text.split_inclusive('\n') {
        let trimmed = raw_line.trim_start();
        if let Some(body) = comment_body(trimmed) {
            if let Some(header) = table_header(body) {
                section = Some(header.to_owned());
            }
        } else if let Some(header) = table_header(trimmed) {
            section = Some(header.to_owned());
        } else {
            let paths = active_paths(section.as_deref(), trimmed);
            if section.is_none() && paths.contains(&(None, "keybindings".to_owned())) {
                let documentation =
                    missing_option_documentation(text, offset, &keybinding_documentation());
                if !documentation.is_empty() {
                    insertions.push((offset, render_documentation(&documentation, newline)));
                }
            }
            for (option_section, key) in paths {
                if let Some(option) = options
                    .iter()
                    .find(|option| option.section == option_section && option.key == key)
                {
                    let documentation =
                        missing_option_documentation(text, offset, &option_documentation(option));
                    if !documentation.is_empty() {
                        insertions.push((offset, render_documentation(&documentation, newline)));
                    }
                }
            }
        }
        offset += raw_line.len();
    }
    insertions
}

fn active_paths(section: Option<&str>, line: &str) -> Vec<(Option<String>, String)> {
    if let Some(section) = section {
        return commented_keys(Some(section), line)
            .unwrap_or_default()
            .into_iter()
            .map(|key| (Some(section.to_owned()), key))
            .collect();
    }

    let Ok(value) = toml::from_str::<toml::Value>(line) else {
        return Vec::new();
    };
    let Some(table) = value.as_table() else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for (key, value) in table {
        paths.push((None, key.clone()));
        if let Some(section) = value.as_table() {
            paths.extend(
                section
                    .keys()
                    .cloned()
                    .map(|sub_key| (Some(key.clone()), sub_key)),
            );
        }
    }
    paths
}

fn append_documentation(output: &mut String, documentation: &[String], newline: &str) {
    for line in documentation {
        output.push_str("## ");
        output.push_str(line);
        output.push_str(newline);
    }
}

fn keybinding_documentation() -> Vec<String> {
    let mut in_keybindings = false;
    let mut documentation = Vec::new();
    for line in DEFAULT_CONFIG.lines() {
        let Some(body) = comment_body(line) else {
            continue;
        };
        if let Some(header) = table_header(body) {
            in_keybindings = header == "keybindings";
            continue;
        }
        if in_keybindings && line.trim_start().starts_with("##") && !body.contains('=') {
            documentation.push(body.to_owned());
        }
    }
    documentation
}

fn missing_keybinding_documentation(text: &str, block: &SectionBlock) -> Vec<String> {
    keybinding_documentation()
        .into_iter()
        .filter(|line| !text[block.start..block.end].contains(&format!("## {line}")))
        .collect()
}

fn section_header_end(text: &str, block: &SectionBlock) -> usize {
    let header = text[block.start..block.end]
        .split_inclusive('\n')
        .next()
        .unwrap_or_default();
    block.start + header.len()
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

fn normalize_legacy_documentation(text: &str) -> Option<String> {
    let mut normalized = String::with_capacity(text.len());
    let mut section = None;
    let mut skipping_actions = false;
    let mut skipping_key_syntax = false;
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

        if skipping_actions {
            changed = true;
            if body.starts_with("command_palette. Example:") {
                skipping_actions = false;
            }
            continue;
        }
        if skipping_key_syntax {
            changed = true;
            if body.starts_with("ignored for other characters") {
                skipping_key_syntax = false;
            }
            continue;
        }
        if section.as_deref() == Some("keybindings") && body.starts_with("\"Keys\" = \"action\".") {
            skipping_key_syntax = true;
            changed = true;
            continue;
        }
        if section.as_deref() == Some("keybindings")
            && body.starts_with(
                "Actions: copy, paste, scroll_page_up, scroll_page_down, scroll_to_top",
            )
        {
            skipping_actions = true;
            changed = true;
            continue;
        }
        if is_legacy_generated_type_comment(body) {
            changed = true;
            continue;
        }

        let replacement = match body {
            "Interface language: \"auto\" (follows LANG), \"en\", \"fr\"." => {
                Some("Allowed values: \"auto\", \"en\", \"fr\".")
            }
            "Font family name. Empty: the system monospace font." | "Font family name." => Some(
                "Font family name. Run `fc-list : family` to list installed fonts; empty uses system monospace.",
            ),
            "Font size in points." => Some("Size in points, from 6 to 72."),
            "Startup mode for new windows." => Some(
                "Startup mode: \"default\" (configured size), \"maximized\", or \"remember\" (last closed window size/state).",
            ),
            "Size of a new window: \"default\" (width and height below), \"maximized\", or" => {
                Some(
                    "Startup mode: \"default\" (configured size), \"maximized\", or \"remember\" (last closed window size/state).",
                )
            }
            "or \"remember\" (the size and maximized state of the last window closed)." => {
                changed = true;
                continue;
            }
            "Initial size when startup_mode is \"default\", or \"remember\" with nothing remembered"
            | "Initial window width in logical pixels when startup_mode is \"default\", or \"remember\""
            | "Initial window height in logical pixels when startup_mode is \"default\", or \"remember\"" => {
                Some("Initial window size, in logical pixels.")
            }
            "yet, in logical pixels." | "with nothing remembered yet." => {
                changed = true;
                continue;
            }
            "Minimum space around the grid, in logical pixels." => {
                Some("Minimum space around the grid, in logical pixels (zero or greater).")
            }
            "Opacity of the default background, from 0.0 (transparent) to 1.0 (opaque)." => Some(
                "Default background opacity: 0.0 transparent to 1.0 opaque; needs a compositor. Explicit cell backgrounds, selection and cursor stay opaque.",
            ),
            "Explicit cell backgrounds, the selection and the cursor stay opaque. Needs a"
            | "compositor; ignored when the GPU surface does not support transparency." => {
                changed = true;
                continue;
            }
            "Applications can still change both (DECSCUSR)." => Some(
                "Cursor blinking: true or false. Applications can still change this (DECSCUSR).",
            ),
            "Characters ending a word on double-click." => {
                Some("Characters that end a word on double-click.")
            }
            "Check GitHub for a newer release and show a banner. false: no network call at all." => {
                Some(
                    "Check GitHub for a newer release and show a banner (true or false); false makes no network call.",
                )
            }
            "Check GitHub for a newer release and show a banner." => Some(
                "Check GitHub for a newer release and show a banner (true or false); false makes no network call.",
            ),
            "Time between two checks: \"auto\", \"6h\", \"12h\", \"24h\", \"2d\", \"3d\", \"1w\"." => {
                Some(
                    "Check interval: \"auto\", \"6h\", \"12h\", \"24h\", \"2d\", \"3d\" or \"1w\"; \"auto\" is 6h for dev builds, 24h for stable builds.",
                )
            }
            "auto: 6h for dev builds, 24h for stable builds." => {
                changed = true;
                continue;
            }
            _ => None,
        };

        if let Some(body) = replacement {
            let indentation = line.len() - trimmed.len();
            normalized.push_str(&line[..indentation]);
            normalized.push_str("## ");
            normalized.push_str(body);
            normalized.push_str(&raw_line[line.len()..]);
            changed = true;
        } else {
            normalized.push_str(raw_line);
        }
    }

    changed.then_some(normalized)
}

fn is_legacy_generated_type_comment(body: &str) -> bool {
    matches!(
        body,
        "Type: string. Allowed values: \"auto\" (follows LANG), \"en\", \"fr\"."
            | "Type: string. Empty uses the system monospace font."
            | "Type: finite number in the range 6..=72."
            | "Type: number in the range 6..=72."
            | "Type: string. Allowed values: \"default\" (configured width and height below), \"maximized\","
            | "Type: positive 32-bit integer (1..=4294967295)."
            | "Type: positive integer (> 0)."
            | "Type: finite non-negative number (>= 0)."
            | "Type: non-negative number (>= 0)."
            | "Type: finite number in the range 0.0..=1.0."
            | "Type: number in the range 0.0..=1.0."
            | "Type: non-negative integer in the range 0..=100000."
            | "Type: string. Allowed values: \"block\", \"beam\", \"underline\"."
            | "Type: boolean (true or false)."
            | "Type: string containing the word-separator characters."
            | "Type: string. Allowed values: \"top\", \"bottom\", \"left\", \"right\"."
            | "Type: non-empty theme name string; built-in names or a custom theme file name."
            | "Type: string shortcut key mapped to a string action."
            | "Type: string shortcut key mapped to a string action; see allowed actions above."
            | "Type: string shortcut key mapped to a string action; see allowed actions in the keybindings section."
            | "Type: string shortcut key mapped to an action string; see allowed actions above."
            | "Type: string. Allowed values: \"auto\", \"6h\", \"12h\", \"24h\", \"2d\", \"3d\", \"1w\"."
    )
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

fn active_item<'a>(document: &'a DocumentMut, option: &OptionLine) -> Option<&'a toml_edit::Item> {
    match &option.section {
        None => document.as_table().get(&option.key),
        Some(section) => document
            .as_table()
            .get(section)
            .and_then(|item| item.as_table_like())
            .and_then(|table| table.get(&option.key)),
    }
}

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
        if section.as_deref() == Some("keybindings") {
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

    for (key, action) in crate::keybindings::defaults() {
        let line = format!("{key:?} = {:?}", action.id());
        options.push(OptionLine {
            section: Some("keybindings".to_owned()),
            key: key.to_owned(),
            line,
            documentation: Vec::new(),
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
            if option.section.as_deref() != Some("keybindings") {
                assert!(
                    !option.documentation.is_empty(),
                    "missing documentation for option: {:?}",
                    option
                );
                assert_eq!(
                    option.documentation.len(),
                    1,
                    "duplicate help lines for option: {:?}",
                    option
                );
            }
            assert!(
                completed.contains(&format!("# {}\n", option.line)),
                "missing generated option: {:?}",
                option
            );
            for documentation in &option.documentation {
                assert!(
                    completed.contains(&format!("## {documentation}\n")),
                    "missing documentation for generated option: {:?}",
                    option
                );
            }
        }
        for documentation in keybinding_documentation() {
            assert!(completed.contains(&format!("## {documentation}\n")));
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
        assert!(updated.starts_with("# user's note\n"));
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
        assert!(updated.contains("# \"Ctrl+Shift+C\" = \"copy\"\n"));
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
    fn existing_keybinding_sections_get_one_shared_help_block() {
        let updated = complete_defaults("# [keybindings]\n").unwrap().unwrap();

        assert_eq!(updated.matches("# [keybindings]").count(), 1);
        assert!(updated.contains("## Each string shortcut key maps to one string action name."));
        assert!(updated.contains("## Allowed actions: \"none\", \"copy\", \"paste\""));
        assert!(!updated.contains("## Type: string."));
        assert_eq!(
            updated.matches("## Each string shortcut key maps").count(),
            1
        );
        assert_eq!(updated.matches("## Allowed actions:").count(), 1);
    }

    #[test]
    fn existing_active_and_commented_settings_get_concise_help() {
        let active = concat!(
            "[window]\n",
            "background_opacity = 0.9\n",
            "[cursor]\n",
            "blinking = true\n",
        );
        let updated = complete_defaults(active).unwrap().unwrap();
        assert!(updated.contains(
            "## Default background opacity: 0.0 transparent to 1.0 opaque; needs a compositor. Explicit cell backgrounds, selection and cursor stay opaque.\nbackground_opacity = 0.9\n"
        ));
        assert!(updated.contains(
            "## Cursor blinking: true or false. Applications can still change this (DECSCUSR).\nblinking = true\n"
        ));
        assert!(updated.contains("background_opacity = 0.9"));
        assert!(updated.contains("blinking = true"));
        assert_eq!(complete_defaults(&updated).unwrap(), None);

        let commented = concat!(
            "# [window]\n",
            "# background_opacity = 1.0\n",
            "# [cursor]\n",
            "# blinking = false\n",
        );
        let updated = complete_defaults(commented).unwrap().unwrap();
        assert!(updated.contains(
            "## Default background opacity: 0.0 transparent to 1.0 opaque; needs a compositor. Explicit cell backgrounds, selection and cursor stay opaque.\n# background_opacity = 1.0\n"
        ));
        assert!(updated.contains(
            "## Cursor blinking: true or false. Applications can still change this (DECSCUSR).\n# blinking = false\n"
        ));
    }

    #[test]
    fn inline_table_settings_get_concise_help_without_rewriting_values() {
        let original = "window = { background_opacity = 0.9 }\n";
        let updated = complete_defaults(original).unwrap().unwrap();

        assert!(updated.starts_with("## Default background opacity: 0.0 transparent to 1.0 opaque; needs a compositor. Explicit cell backgrounds, selection and cursor stay opaque.\nwindow = { background_opacity = 0.9 }\n"));
        assert_eq!(updated.matches("background_opacity = 0.9").count(), 1);
        assert!(updated.parse::<DocumentMut>().is_ok());
    }

    #[test]
    fn template_documents_ranges_and_finite_values_concisely() {
        let options = option_lines();
        for (section, key, expected) in [
            (
                None,
                "language",
                "Allowed values: \"auto\", \"en\", \"fr\".",
            ),
            (
                Some("window"),
                "startup_mode",
                "Startup mode: \"default\" (configured size), \"maximized\", or \"remember\" (last closed window size/state).",
            ),
            (
                Some("cursor"),
                "shape",
                "Cursor shape: \"block\", \"beam\" or \"underline\".",
            ),
            (
                Some("tabs"),
                "position",
                "Tab bar position: \"top\", \"bottom\", \"left\", \"right\".",
            ),
            (
                Some("updates"),
                "interval",
                "Check interval: \"auto\", \"6h\", \"12h\", \"24h\", \"2d\", \"3d\" or \"1w\"; \"auto\" is 6h for dev builds, 24h for stable builds.",
            ),
        ] {
            let option = options
                .iter()
                .find(|option| option.section.as_deref() == section && option.key == key)
                .unwrap();
            assert!(
                option
                    .documentation
                    .iter()
                    .any(|line| line.contains(expected)),
                "missing finite values for {section:?}.{key}: {:?}",
                option.documentation
            );
        }
    }

    #[test]
    fn legacy_duplicate_help_is_replaced_with_concise_comments() {
        let original = concat!(
            "## Interface language: \"auto\" (follows LANG), \"en\", \"fr\".\n",
            "## Type: string. Allowed values: \"auto\" (follows LANG), \"en\", \"fr\".\n",
            "language = \"fr\"\n",
            "# [font]\n",
            "## Font family name. Empty: the system monospace font.\n",
            "## Type: string. Empty uses the system monospace font.\n",
            "# family = \"\"\n",
            "# [keybindings]\n",
            "## Type: string shortcut key mapped to a string action.\n",
            "## \"Keys\" = \"action\". The action \"none\" disables a default shortcut, so the key goes\n",
            "## to the shell. Keys: modifiers Ctrl, Shift, Alt, Super joined with \"+\", then a key\n",
            "## name (PageUp, F5, Insert, Left...) or one character. Letters ignore case; Shift is\n",
            "## ignored for other characters (\"Ctrl++\"). Alt+digit uses the digit row on any layout.\n",
            "## Actions: copy, paste, scroll_page_up, scroll_page_down, scroll_to_top,\n",
            "## command_palette. Example: \"Ctrl+Alt+T\" = \"new_tab\" adds a shortcut;\n",
            "## Type: user-specific plugin note.\n",
            "[plugin]\n",
            "value = 42\n",
        );

        let updated = complete_defaults(original).unwrap().unwrap();

        assert!(!updated.contains("follows LANG"));
        assert_eq!(
            updated
                .matches("## Allowed values: \"auto\", \"en\", \"fr\".")
                .count(),
            1
        );
        assert_eq!(updated.matches("fc-list : family").count(), 1);
        assert!(!updated.contains("## Type: string."));
        assert!(!updated.contains("## Actions:"));
        assert!(!updated.contains("\"Keys\" = \"action\""));
        assert_eq!(updated.matches("## Allowed actions:").count(), 1);
        assert!(updated.contains("## Type: user-specific plugin note."));
        assert!(updated.contains("language = \"fr\""));
        assert!(updated.contains("# family = \"\""));
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
