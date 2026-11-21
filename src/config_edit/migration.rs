//! Migration of the shortcuts from  to their own file.

use super::*;

/// What moving the `[keybindings]` section out of `config.toml` changes.
#[derive(Debug)]
pub(crate) struct KeybindingsMigration {
    /// The new text of `config.toml`: no `[keybindings]` section nor generated block.
    pub(crate) config: String,
    /// The new text of `keybindings.toml` when at least one shortcut was merged into it.
    pub(crate) keybindings: Option<String>,
    /// An active section holding shortcuts was found (a commented block alone is not).
    pub(crate) moved: bool,
}

/// Moves the shortcuts of an active `[keybindings]` section of `config.toml` into
/// `keybindings_text` (the text of `keybindings.toml`, `None` when the file is missing) and
/// removes that section and the generated commented block from `config.toml`. A shortcut
/// already active in `keybindings.toml` is not replaced. `None` means there is nothing to
/// do, or that a file is not valid TOML (neither is ever rewritten then).
pub(crate) fn migrate_keybindings(
    config_text: &str,
    keybindings_text: Option<&str>,
    keybindings_file: &Path,
) -> Option<KeybindingsMigration> {
    let legacy = normalize_legacy_documentation(config_text);
    let text = legacy.as_deref().unwrap_or(config_text);
    text.parse::<DocumentMut>().ok()?;
    let blocks: Vec<SectionBlock> = section_blocks(text)
        .into_iter()
        .filter(|block| block.name == "keybindings")
        .collect();
    if blocks.is_empty() {
        return None;
    }

    let generated = GeneratedKeybindingLines::new();
    let mut entries: Vec<(String, toml_edit::Value, String)> = Vec::new();
    let mut config = String::with_capacity(text.len());
    let mut cursor = 0;
    for block in &blocks {
        config.push_str(&text[cursor..block.start]);
        cursor = block.end;
        let body = &text[block.start..block.end];
        let leftover = if block.active {
            let document = body.parse::<DocumentMut>().ok()?;
            let table = document.get("keybindings")?.as_table()?;
            for (key, item) in table {
                let value = item.as_value()?;
                let (key_node, _) = table.get_key_value(key)?;
                let prefix = key_node
                    .leaf_decor()
                    .prefix()
                    .and_then(toml_edit::RawString::as_str)
                    .unwrap_or_default();
                entries.push((key.to_owned(), value.clone(), generated.strip(prefix)));
            }
            generated.strip(document.trailing().as_str().unwrap_or_default())
        } else {
            let header = body.split_inclusive('\n').next().unwrap_or_default();
            generated.strip(&body[header.len()..])
        };
        if !leftover.trim().is_empty() {
            config.push_str(&leftover);
        }
    }
    config.push_str(&text[cursor..]);

    let mut merged = None;
    if !entries.is_empty() {
        let destination = keybindings_text.map_or_else(default_keybindings_text, str::to_owned);
        let mut edit = ConfigEdit::parse(&destination, keybindings_file).ok()?;
        let mut added = false;
        for (key, value, prefix) in &entries {
            let present = edit
                .document
                .get("keybindings")
                .and_then(toml_edit::Item::as_table_like)
                .is_some_and(|table| table.contains_key(key));
            if present {
                continue;
            }
            edit.set(&["keybindings", key], value.clone()).ok()?;
            if prefix.contains('#')
                && let Some(mut node) = edit
                    .document
                    .get_mut("keybindings")
                    .and_then(toml_edit::Item::as_table_mut)
                    .and_then(|table| table.key_mut(key))
            {
                node.leaf_decor_mut().set_prefix(prefix.clone());
            }
            added = true;
        }
        if added {
            merged = Some(edit.to_string());
        }
    }

    Some(KeybindingsMigration {
        config,
        keybindings: merged,
        moved: !entries.is_empty(),
    })
}

/// The lines Graphene generates in `config.toml` for the shortcuts: the `##`
/// documentation and one commented line per default shortcut.
pub(super) struct GeneratedKeybindingLines {
    pub(super) documentation: HashSet<String>,
    pub(super) shortcuts: HashSet<String>,
}

impl GeneratedKeybindingLines {
    pub(super) fn new() -> Self {
        Self {
            documentation: DEFAULT_KEYBINDINGS
                .lines()
                .filter(|line| line.trim_start().starts_with("##"))
                .filter_map(comment_body)
                .map(str::to_owned)
                .collect(),
            shortcuts: keybinding_option_lines()
                .into_iter()
                .map(|option| option.line)
                .collect(),
        }
    }

    pub(super) fn is_generated(&self, line: &str) -> bool {
        let trimmed = line.trim();
        let Some(body) = comment_body(trimmed) else {
            return false;
        };
        (trimmed.starts_with("##") && self.documentation.contains(body))
            || self.shortcuts.contains(body)
    }

    /// `text` without its generated lines.
    pub(super) fn strip(&self, text: &str) -> String {
        text.split_inclusive('\n')
            .filter(|line| !self.is_generated(line))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn migrate(config: &str, keybindings: Option<&str>) -> Option<KeybindingsMigration> {
        migrate_keybindings(config, keybindings, Path::new("keybindings.toml"))
    }

    #[test]
    fn migration_cuts_the_section_and_keeps_the_neighbours() {
        let migration = migrate(
            "[font]\nsize = 9\n\n[keybindings]\n# why\n\"F7\" = \"copy\"\n\n[updates]\ncheck = false\n",
            Some("[keybindings]\n"),
        )
        .unwrap();
        assert_eq!(
            migration.config,
            "[font]\nsize = 9\n\n[updates]\ncheck = false\n"
        );
        assert_eq!(
            migration.keybindings.as_deref(),
            Some("[keybindings]\n# why\nF7 = \"copy\"\n")
        );
        assert!(migration.moved);
    }

    #[test]
    fn migration_of_a_commented_block_alone_changes_nothing_else() {
        let block: String = DEFAULT_KEYBINDINGS
            .lines()
            .filter(|line| line.starts_with("##"))
            .map(|line| format!("{line}\n"))
            .collect();
        let migration = migrate(
            &format!("[font]\nsize = 9\n\n# [keybindings]\n{block}\n# [updates]\n"),
            None,
        )
        .unwrap();
        assert_eq!(migration.config, "[font]\nsize = 9\n\n# [updates]\n");
        assert_eq!(migration.keybindings, None);
        assert!(!migration.moved);
    }

    #[test]
    fn migration_needs_a_keybindings_block() {
        assert!(migrate("[font]\nsize = 9\n", None).is_none());
    }

    #[test]
    fn migration_skips_invalid_toml() {
        assert!(migrate("[keybindings\n", None).is_none());
        assert!(migrate("[keybindings]\n\"F7\" = \"copy\"\n", Some("[oops")).is_none());
    }
}
