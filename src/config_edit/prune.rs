//! Removal of the unknown keys of the configuration files, after confirmation.

use super::*;

/// A file `--prune-config` cleans, which decides the parser that classifies its keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PruneKind {
    Config,
    Keybindings,
}

impl PruneKind {
    pub(super) fn default_text(self) -> anyhow::Result<String> {
        match self {
            Self::Config => Ok(complete_defaults(DEFAULT_CONFIG)
                .map_err(|error| {
                    anyhow::anyhow!("invalid embedded default configuration: {error}")
                })?
                .unwrap_or_else(|| DEFAULT_CONFIG.to_owned())),
            Self::Keybindings => Ok(default_keybindings_text()),
        }
    }

    pub(super) fn unknown_keys(
        self,
        text: &str,
        path: &Path,
    ) -> Vec<crate::config::UnknownConfigKey> {
        let previous = crate::config::Config::default();
        match self {
            Self::Config => crate::config::Config::parse_with_unknown_keys(text, &previous, path).2,
            Self::Keybindings => crate::config::Config::parse_keybindings(text, &previous, path).2,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PruneOutcome {
    NothingToPrune,
    Declined(Vec<String>),
    Pruned(Vec<String>),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PruneReport {
    /// Missing files created from their default text (never asks for confirmation).
    pub(crate) created: Vec<PathBuf>,
    pub(crate) outcome: PruneOutcome,
}

pub(super) struct PrunePlan<'a> {
    pub(super) path: &'a Path,
    pub(super) kind: PruneKind,
    pub(super) text: String,
    pub(super) unknown: Vec<crate::config::UnknownConfigKey>,
}

/// Removes only keys the file parsers do not recognize. The callback is invoked once with
/// the complete list (each key prefixed by its file name) before any file is modified.
/// A missing file is created from its default text.
pub(crate) fn prune_files(
    targets: &[(PruneKind, &Path)],
    mut confirm: impl FnMut(&[String]) -> bool,
) -> anyhow::Result<PruneReport> {
    let mut created = Vec::new();
    let mut plans = Vec::new();
    for &(kind, path) in targets {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                write_atomic(path, &kind.default_text()?)?;
                created.push(path.to_path_buf());
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        // Validate syntax before asking for confirmation or computing any edits. In
        // particular, never rewrite a file that toml_edit cannot parse.
        toml_edit::Document::parse(text.to_owned())
            .map_err(|error| anyhow::anyhow!("invalid TOML in {}: {error}", path.display()))?;
        let unknown = kind.unknown_keys(&text, path);
        if !unknown.is_empty() {
            plans.push(PrunePlan {
                path,
                kind,
                text,
                unknown,
            });
        }
    }
    if plans.is_empty() {
        return Ok(PruneReport {
            created,
            outcome: PruneOutcome::NothingToPrune,
        });
    }

    let paths: Vec<String> = plans
        .iter()
        .flat_map(|plan| {
            let file = plan
                .path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            plan.unknown
                .iter()
                .map(move |key| format!("{file}: {}", key.path))
        })
        .collect();
    if !confirm(&paths) {
        return Ok(PruneReport {
            created,
            outcome: PruneOutcome::Declined(paths),
        });
    }

    // Compute every edit before writing any file: one failure leaves all untouched.
    let mut updates = Vec::with_capacity(plans.len());
    for plan in &plans {
        updates.push((plan.path, pruned_text(plan)?));
    }
    for (path, updated) in updates {
        write_atomic(path, &updated)?;
    }
    Ok(PruneReport {
        created,
        outcome: PruneOutcome::Pruned(paths),
    })
}

pub(super) fn pruned_text(plan: &PrunePlan) -> anyhow::Result<String> {
    let PrunePlan {
        path,
        kind,
        text,
        unknown,
    } = plan;
    let document = toml_edit::Document::parse(text.to_owned())
        .map_err(|error| anyhow::anyhow!("invalid TOML in {}: {error}", path.display()))?;
    let mut ranges = Vec::with_capacity(unknown.len());
    for key in unknown {
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
            full_line_range(text, section_span)?
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
                    inline_entry_range(text, key_span.start..span.end)?
                }
                _ => full_line_range(text, span)?,
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
    let remaining = kind.unknown_keys(&updated, path);
    if !remaining.is_empty() {
        anyhow::bail!(
            "pruning did not remove all unknown configuration keys: {:?}",
            remaining.iter().map(|key| &key.path).collect::<Vec<_>>()
        );
    }
    Ok(updated)
}

pub(super) fn item_source_end(item: &toml_edit::Item) -> Option<usize> {
    let own_end = item.span()?.end;
    let nested_end = item
        .as_table()
        .into_iter()
        .flat_map(toml_edit::Table::iter)
        .filter_map(|(_, child)| item_source_end(child))
        .max();
    Some(nested_end.unwrap_or(own_end).max(own_end))
}

pub(super) fn full_line_range(text: &str, span: Range<usize>) -> anyhow::Result<Range<usize>> {
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

pub(super) fn inline_entry_range(text: &str, span: Range<usize>) -> anyhow::Result<Range<usize>> {
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
    let dir = crate::config::config_dir().ok_or_else(|| {
        anyhow::anyhow!("cannot locate the configuration directory: set HOME or XDG_CONFIG_HOME")
    })?;
    let config = dir.join(crate::config::CONFIG_FILE);
    let keybindings = dir.join(crate::config::KEYBINDINGS_FILE);
    let stdin_tty = io::stdin().is_terminal();
    let stdout_tty = io::stdout().is_terminal();
    let report = prune_files(
        &[
            (PruneKind::Config, &config),
            (PruneKind::Keybindings, &keybindings),
        ],
        |paths| {
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
        },
    )?;
    for path in &report.created {
        println!(
            "Created {} from the default configuration; nothing to prune.",
            path.display()
        );
    }
    match report.outcome {
        PruneOutcome::NothingToPrune if report.created.is_empty() => {
            println!("No unknown configuration keys to prune.")
        }
        PruneOutcome::NothingToPrune => {}
        PruneOutcome::Declined(_) => println!("Configuration left unchanged."),
        PruneOutcome::Pruned(paths) => {
            println!("Removed {} unknown configuration key(s).", paths.len())
        }
    }
    Ok(())
}

pub(super) fn active_item<'a>(
    document: &'a DocumentMut,
    option: &OptionLine,
) -> Option<&'a toml_edit::Item> {
    match &option.section {
        None => document.as_table().get(&option.key),
        Some(section) => document
            .as_table()
            .get(section)
            .and_then(|item| item.as_table_like())
            .and_then(|table| table.get(&option.key)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_edit::tests::{remove_temporary_config, temporary_config};

    fn prune_one(
        path: &Path,
        kind: PruneKind,
        confirm: impl FnMut(&[String]) -> bool,
    ) -> anyhow::Result<PruneOutcome> {
        Ok(prune_files(&[(kind, path)], confirm)?.outcome)
    }

    /// Both files of a configuration folder, with the given contents (`None` = absent).
    fn temporary_pair(
        config: Option<&str>,
        keybindings: Option<&str>,
    ) -> (PathBuf, PathBuf, PathBuf) {
        let (directory, config_path) = temporary_config(config);
        let keybindings_path = directory.join("keybindings.toml");
        if let Some(contents) = keybindings {
            fs::write(&keybindings_path, contents).unwrap();
        }
        (directory, config_path, keybindings_path)
    }

    fn prune_pair(
        config: &Path,
        keybindings: &Path,
        confirm: impl FnMut(&[String]) -> bool,
    ) -> anyhow::Result<PruneReport> {
        prune_files(
            &[
                (PruneKind::Config, config),
                (PruneKind::Keybindings, keybindings),
            ],
            confirm,
        )
    }

    #[test]
    fn prune_lists_unknown_keys_of_both_files_and_asks_once() {
        let (directory, config, keybindings) = temporary_pair(
            Some("language = \"fr\"\nfuture_root = 1\n"),
            Some("[keybindings]\n\"Ctrl+Alt+T\" = \"new_tab\"\n[future]\nvalue = 1\n"),
        );
        let mut asked = 0;
        let report = prune_pair(&config, &keybindings, |keys| {
            asked += 1;
            assert_eq!(
                keys,
                &["config.toml: future_root", "keybindings.toml: future"]
            );
            true
        })
        .unwrap();
        assert_eq!(asked, 1);
        assert_eq!(
            report.outcome,
            PruneOutcome::Pruned(vec![
                "config.toml: future_root".into(),
                "keybindings.toml: future".into()
            ])
        );
        let config_text = fs::read_to_string(&config).unwrap();
        assert!(config_text.contains("language = \"fr\"") && !config_text.contains("future_root"));
        let keybindings_text = fs::read_to_string(&keybindings).unwrap();
        assert!(keybindings_text.contains("\"Ctrl+Alt+T\" = \"new_tab\""));
        assert!(!keybindings_text.contains("future") && !keybindings_text.contains("value"));
        remove_temporary_config(directory);
    }

    #[test]
    fn prune_prunes_a_single_file_when_only_it_has_unknown_keys() {
        let (directory, config, keybindings) = temporary_pair(
            Some("language = \"fr\"\n"),
            Some("[keybindings]\n[future]\nvalue = 1\n"),
        );
        let config_before = fs::read_to_string(&config).unwrap();
        let report = prune_pair(&config, &keybindings, |keys| {
            assert_eq!(keys, &["keybindings.toml: future"]);
            true
        })
        .unwrap();
        assert!(matches!(report.outcome, PruneOutcome::Pruned(_)));
        assert_eq!(fs::read_to_string(&config).unwrap(), config_before);
        assert!(!fs::read_to_string(&keybindings).unwrap().contains("future"));
        remove_temporary_config(directory);
    }

    #[test]
    fn prune_refusal_leaves_both_files_untouched() {
        let config_text = "future_root = 1\n";
        let keybindings_text = "[keybindings]\n[future]\nvalue = 1\n";
        let (directory, config, keybindings) =
            temporary_pair(Some(config_text), Some(keybindings_text));
        let report = prune_pair(&config, &keybindings, |_| false).unwrap();
        assert!(matches!(report.outcome, PruneOutcome::Declined(keys) if keys.len() == 2));
        assert_eq!(fs::read_to_string(&config).unwrap(), config_text);
        assert_eq!(fs::read_to_string(&keybindings).unwrap(), keybindings_text);
        remove_temporary_config(directory);
    }

    #[test]
    fn prune_creates_the_missing_keybindings_file_from_its_template() {
        let (directory, config, keybindings) = temporary_pair(Some("language = \"fr\"\n"), None);
        let report = prune_pair(&config, &keybindings, |_| {
            panic!("nothing to prune must not ask")
        })
        .unwrap();
        assert_eq!(report.created, vec![keybindings.clone()]);
        assert_eq!(report.outcome, PruneOutcome::NothingToPrune);
        assert_eq!(
            fs::read_to_string(&keybindings).unwrap(),
            default_keybindings_text()
        );
        remove_temporary_config(directory);
    }

    #[test]
    fn prune_never_removes_a_moved_keybindings_section_from_config() {
        let original = "language = \"fr\"\n[keybindings]\n\"Ctrl+Alt+T\" = \"new_tab\"\n";
        let (directory, config, keybindings) =
            temporary_pair(Some(original), Some("[keybindings]\n"));
        let report = prune_pair(&config, &keybindings, |_| {
            panic!("moved section is not unknown")
        })
        .unwrap();
        assert_eq!(report.outcome, PruneOutcome::NothingToPrune);
        assert_eq!(fs::read_to_string(&config).unwrap(), original);
        remove_temporary_config(directory);
    }

    #[test]
    fn prune_does_not_touch_any_file_when_one_is_invalid_toml() {
        let config_text = "future_root = 1\n";
        let (directory, config, keybindings) =
            temporary_pair(Some(config_text), Some("[keybindings\n"));
        assert!(prune_pair(&config, &keybindings, |_| panic!("must not ask")).is_err());
        assert_eq!(fs::read_to_string(&config).unwrap(), config_text);
        remove_temporary_config(directory);
    }

    #[test]
    fn prune_removes_only_unknown_root_and_nested_keys() {
        let (directory, path) = temporary_config(Some(
            "language = \"fr\"\nfuture_root_option = \"preserve until confirmed\"\n[font]\nsize = \"invalid-but-known\"\nfuture_font_option = 12\n[plugin]\nfuture_plugin_option = true\n[window]\nwidth = 1024\n",
        ));
        let outcome = prune_one(&path, PruneKind::Config, |keys| {
            assert_eq!(
                keys,
                &[
                    "config.toml: font.future_font_option",
                    "config.toml: future_root_option",
                    "config.toml: plugin"
                ]
            );
            true
        })
        .unwrap();
        assert_eq!(
            outcome,
            PruneOutcome::Pruned(vec![
                "config.toml: font.future_font_option".to_owned(),
                "config.toml: future_root_option".to_owned(),
                "config.toml: plugin".to_owned()
            ])
        );
        let updated = fs::read_to_string(&path).unwrap();
        assert!(updated.contains("language = \"fr\""));
        assert!(updated.contains("size = \"invalid-but-known\""));
        assert!(updated.contains("width = 1024"));
        assert!(!updated.contains("config.toml: future_root_option"));
        assert!(!updated.contains("future_font_option"));
        assert!(!updated.contains("[plugin]"));
        remove_temporary_config(directory);
    }

    #[test]
    fn prune_refusal_does_not_modify_the_file() {
        let original = "[font]\nfuture_option = 1\nsize = 16\n";
        let (directory, path) = temporary_config(Some(original));
        let outcome = prune_one(&path, PruneKind::Config, |keys| {
            assert_eq!(keys, &["config.toml: font.future_option"]);
            false
        })
        .unwrap();
        assert_eq!(
            outcome,
            PruneOutcome::Declined(vec!["config.toml: font.future_option".into()])
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        remove_temporary_config(directory);
    }

    #[test]
    fn prune_confirmation_yes_removes_keys_and_preserves_inline_known_values() {
        let original = "font = { size = \"invalid-but-known\", future_option = 1 }\n";
        let (directory, path) = temporary_config(Some(original));
        let outcome = prune_one(&path, PruneKind::Config, |keys| {
            assert_eq!(keys, &["config.toml: font.future_option"]);
            true
        })
        .unwrap();
        assert_eq!(
            outcome,
            PruneOutcome::Pruned(vec!["config.toml: font.future_option".into()])
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
        let outcome = prune_one(&path, PruneKind::Config, |keys| {
            assert_eq!(keys, &["config.toml: font.future"]);
            true
        })
        .unwrap();
        assert_eq!(
            outcome,
            PruneOutcome::Pruned(vec!["config.toml: font.future".into()])
        );
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
        let report = prune_files(&[(PruneKind::Config, &path)], |_| {
            panic!("default creation must not ask")
        })
        .unwrap();
        assert_eq!(report.created, vec![path.clone()]);
        assert_eq!(report.outcome, PruneOutcome::NothingToPrune);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(!contents.contains("keybindings"));
        assert!(contents.parse::<DocumentMut>().is_ok());
        remove_temporary_config(directory);
    }

    #[test]
    fn invalid_toml_is_never_changed_or_confirmed() {
        let original = "[font\nsize = 12\n";
        let (directory, path) = temporary_config(Some(original));
        assert!(
            prune_one(&path, PruneKind::Config, |_| panic!(
                "invalid TOML must not ask"
            ))
            .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        remove_temporary_config(directory);
    }

    #[test]
    fn no_unknown_keys_does_not_ask_for_confirmation() {
        let original = "language = \"fr\"\n";
        let (directory, path) = temporary_config(Some(original));
        assert_eq!(
            prune_one(&path, PruneKind::Config, |_| panic!(
                "nothing to prune must not ask"
            ))
            .unwrap(),
            PruneOutcome::NothingToPrune
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        remove_temporary_config(directory);
    }
}
