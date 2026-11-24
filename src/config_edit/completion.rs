//! Completion of a configuration file: options missing from it are added, commented, with their documentation.

use super::*;

#[derive(Debug, Clone)]
pub(super) struct OptionLine {
    pub(super) section: Option<String>,
    pub(super) key: String,
    pub(super) line: String,
    pub(super) documentation: Vec<String>,
}

/// Completes supported options and their value documentation without changing configured
/// values. `None` means the document is already complete. Invalid TOML is never rewritten.
pub(crate) fn complete_defaults(text: &str) -> Result<Option<String>, toml_edit::TomlError> {
    complete_with(text, option_lines(), &[])
}

/// [`complete_defaults`] for `keybindings.toml`: the documentation of the shortcuts and
/// one commented line per default shortcut.
pub(crate) fn complete_keybindings_defaults(
    text: &str,
) -> Result<Option<String>, toml_edit::TomlError> {
    complete_with(text, keybinding_option_lines(), &keybinding_documentation())
}

pub(super) fn complete_with(
    text: &str,
    options: Vec<OptionLine>,
    keybinding_documentation: &[String],
) -> Result<Option<String>, toml_edit::TomlError> {
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
        let documentation = missing_keybinding_documentation(text, keybinding_documentation);
        if !documentation.is_empty() {
            insertions.push((
                section_header_end(text, block),
                render_documentation(&documentation, &newline),
            ));
        }
    }
    insertions.extend(commented_documentation_insertions(text, &options, &newline));
    insertions.extend(active_documentation_insertions(
        text,
        &options,
        keybinding_documentation,
        &newline,
    ));

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
                        keybinding_documentation,
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

pub(super) fn grouped_options(options: Vec<OptionLine>) -> Vec<(Option<String>, Vec<OptionLine>)> {
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

pub(super) fn render_options(
    options: &[OptionLine],
    position: usize,
    text: &str,
    newline: &str,
) -> String {
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

pub(super) fn render_documentation(documentation: &[String], newline: &str) -> String {
    let mut rendered = String::new();
    append_documentation(&mut rendered, documentation, newline);
    rendered
}

pub(super) fn option_documentation(option: &OptionLine) -> Vec<String> {
    option.documentation.clone()
}

pub(super) fn missing_option_documentation(
    text: &str,
    line_start: usize,
    expected: &[String],
) -> Vec<String> {
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
        // A setting (`key = value`) ends the help; help text may contain `=` too.
        if table_header(body).is_some() || commented_keys(None, body).is_some() {
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

pub(super) fn commented_documentation_insertions(
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

pub(super) fn active_documentation_insertions(
    text: &str,
    options: &[OptionLine],
    keybinding_documentation: &[String],
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
                    missing_option_documentation(text, offset, keybinding_documentation);
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

pub(super) fn active_paths(section: Option<&str>, line: &str) -> Vec<(Option<String>, String)> {
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

pub(super) fn append_documentation(output: &mut String, documentation: &[String], newline: &str) {
    for line in documentation {
        output.push_str("## ");
        output.push_str(line);
        output.push_str(newline);
    }
}

/// The `##` documentation lines of [`DEFAULT_KEYBINDINGS`] (the usage example, which holds
/// an `=`, is not repeated into existing files).
pub(super) fn keybinding_documentation() -> Vec<String> {
    DEFAULT_KEYBINDINGS
        .lines()
        .filter(|line| line.trim_start().starts_with("##"))
        .filter_map(comment_body)
        .filter(|body| !body.contains('='))
        .map(str::to_owned)
        .collect()
}

pub(super) fn missing_keybinding_documentation(
    text: &str,
    documentation: &[String],
) -> Vec<String> {
    documentation
        .iter()
        .filter(|line| !text.contains(&format!("## {line}")))
        .cloned()
        .collect()
}

pub(super) fn section_header_end(text: &str, block: &SectionBlock) -> usize {
    let header = text[block.start..block.end]
        .split_inclusive('\n')
        .next()
        .unwrap_or_default();
    block.start + header.len()
}

pub(super) fn newline(text: &str) -> String {
    if text.contains("\r\n") {
        "\r\n".to_owned()
    } else {
        "\n".to_owned()
    }
}

#[derive(Debug)]
pub(super) struct SectionBlock {
    pub(super) name: String,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) active: bool,
}

pub(super) fn section_blocks(text: &str) -> Vec<SectionBlock> {
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

pub(super) fn insertion_point(text: &str, start: usize, end: usize) -> usize {
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

pub(super) fn normalize_legacy_documentation(text: &str) -> Option<String> {
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
                "Default background opacity: 0.0 transparent to 1.0 opaque; needs a compositor. Explicit cell backgrounds, selection and cursor stay opaque; block cards are translucent.",
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

pub(super) fn is_legacy_generated_type_comment(body: &str) -> bool {
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

pub(super) fn normalize_malformed_comments(text: &str, options: &[OptionLine]) -> Option<String> {
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

/// The text of a new `config.toml`: [`DEFAULT_CONFIG`] with its documentation completed.
pub(crate) fn default_file_text() -> String {
    complete_defaults(DEFAULT_CONFIG)
        .ok()
        .flatten()
        .unwrap_or_else(|| DEFAULT_CONFIG.to_owned())
}

/// The text of a new `keybindings.toml`: [`DEFAULT_KEYBINDINGS`] with every default
/// shortcut as a commented line.
pub(crate) fn default_keybindings_text() -> String {
    complete_keybindings_defaults(DEFAULT_KEYBINDINGS)
        .ok()
        .flatten()
        .unwrap_or_else(|| DEFAULT_KEYBINDINGS.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_adds_all_documented_options() {
        let completed = complete_defaults("").unwrap().unwrap();
        for option in option_lines() {
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
        assert!(!completed.contains("keybindings"));
        assert!(commented_options(&completed).contains(&(None, "language".to_owned())));
        assert_eq!(complete_defaults(&completed).unwrap(), None);
    }

    #[test]
    fn keybindings_completion_adds_documentation_and_default_shortcuts() {
        let completed = complete_keybindings_defaults("").unwrap().unwrap();
        for documentation in keybinding_documentation() {
            assert!(completed.contains(&format!("## {documentation}\n")));
        }
        assert_eq!(completed.matches("[keybindings]").count(), 1);
        for (key, action) in crate::keybindings::defaults() {
            assert!(completed.contains(&format!("# {key:?} = {:?}\n", action.id())));
        }
        assert_eq!(complete_keybindings_defaults(&completed).unwrap(), None);
    }

    #[test]
    fn default_keybindings_text_is_complete_with_an_active_header() {
        let text = default_keybindings_text();
        assert_eq!(complete_keybindings_defaults(&text).unwrap(), None);
        assert_eq!(text.matches("\n[keybindings]\n").count(), 1);
        assert!(text.starts_with("## Each string shortcut key maps"));
        assert!(text.contains("## Example: `\"Ctrl+Alt+T\" = \"new_tab\"` adds a shortcut.\n"));
        let (keybindings, errors, unknown) = crate::config::Config::parse_keybindings(
            &text,
            &crate::config::Config::default(),
            Path::new("keybindings.toml"),
        );
        assert!(keybindings.is_empty());
        assert!(errors.is_empty());
        assert!(unknown.is_empty());
    }

    #[test]
    fn keybindings_completion_keeps_user_shortcuts_and_adds_missing_documentation() {
        let original =
            "# mine\n[keybindings]\n\"Ctrl+Alt+T\" = \"new_tab\"\n\"Ctrl+Shift+C\" = \"none\"\n";
        let updated = complete_keybindings_defaults(original).unwrap().unwrap();
        assert!(updated.starts_with("# mine\n[keybindings]\n## Each string shortcut key maps"));
        assert!(updated.contains("\"Ctrl+Alt+T\" = \"new_tab\"\n"));
        // An active shortcut is not repeated as a commented default.
        assert!(!updated.contains("# \"Ctrl+Shift+C\" = \"copy\""));
        assert!(updated.contains("# \"Ctrl+Shift+V\" = \"paste\""));
        assert_eq!(updated.matches("## Allowed actions:").count(), 1);
        assert_eq!(complete_keybindings_defaults(&updated).unwrap(), None);
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
        let updated = complete_keybindings_defaults("# [keybindings]\n")
            .unwrap()
            .unwrap();

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
            "## Default background opacity: 0.0 transparent to 1.0 opaque; needs a compositor. Explicit cell backgrounds, selection and cursor stay opaque; block cards are translucent.\nbackground_opacity = 0.9\n"
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
            "## Default background opacity: 0.0 transparent to 1.0 opaque; needs a compositor. Explicit cell backgrounds, selection and cursor stay opaque; block cards are translucent.\n# background_opacity = 1.0\n"
        ));
        assert!(updated.contains(
            "## Cursor blinking: true or false. Applications can still change this (DECSCUSR).\n# blinking = false\n"
        ));
    }

    #[test]
    fn help_containing_an_equal_sign_is_not_duplicated() {
        let commented = concat!("# [font]\n", "# features = \"\"\n");
        let updated = complete_defaults(commented).unwrap().unwrap();
        assert_eq!(
            updated.matches("## OpenType features").count(),
            1,
            "{updated}"
        );
        assert_eq!(complete_defaults(&updated).unwrap(), None);

        let active = concat!("[font]\n", "features = \"zero\"\n");
        let updated = complete_defaults(active).unwrap().unwrap();
        assert_eq!(
            updated.matches("## OpenType features").count(),
            1,
            "{updated}"
        );
        assert!(updated.contains("features = \"zero\"\n"));
        assert_eq!(complete_defaults(&updated).unwrap(), None);
    }

    #[test]
    fn inline_table_settings_get_concise_help_without_rewriting_values() {
        let original = "window = { background_opacity = 0.9 }\n";
        let updated = complete_defaults(original).unwrap().unwrap();

        assert!(updated.starts_with("## Default background opacity: 0.0 transparent to 1.0 opaque; needs a compositor. Explicit cell backgrounds, selection and cursor stay opaque; block cards are translucent.\nwindow = { background_opacity = 0.9 }\n"));
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
        assert!(updated.contains("## Type: user-specific plugin note."));
        assert!(updated.contains("language = \"fr\""));
        assert!(updated.contains("# family = \"\""));
    }

    #[test]
    fn legacy_keybinding_help_is_replaced_in_keybindings_file() {
        let original = concat!(
            "# [keybindings]\n",
            "## Type: string shortcut key mapped to a string action.\n",
            "## \"Keys\" = \"action\". The action \"none\" disables a default shortcut, so the key goes\n",
            "## to the shell. Keys: modifiers Ctrl, Shift, Alt, Super joined with \"+\", then a key\n",
            "## name (PageUp, F5, Insert, Left...) or one character. Letters ignore case; Shift is\n",
            "## ignored for other characters (\"Ctrl++\"). Alt+digit uses the digit row on any layout.\n",
            "## Actions: copy, paste, scroll_page_up, scroll_page_down, scroll_to_top,\n",
            "## command_palette. Example: \"Ctrl+Alt+T\" = \"new_tab\" adds a shortcut;\n",
            "## Type: user-specific plugin note.\n",
            "\"F7\" = \"copy\"\n",
        );

        let updated =
            complete_keybindings_defaults(&original.replace("# [keybindings]", "[keybindings]"))
                .unwrap()
                .unwrap();

        assert!(!updated.contains("## Type: string shortcut"));
        assert!(!updated.contains("## Actions:"));
        assert!(!updated.contains("\"Keys\" = \"action\""));
        assert_eq!(updated.matches("## Allowed actions:").count(), 1);
        assert!(updated.contains("## Type: user-specific plugin note."));
        assert!(updated.contains("\"F7\" = \"copy\""));
    }

    #[test]
    fn invalid_toml_is_rejected_without_a_replacement() {
        assert!(complete_defaults("[font\nsize = 12\n").is_err());
        assert!(complete_defaults("[cursor]\nshape = \"unterminated\n").is_err());
    }
}
