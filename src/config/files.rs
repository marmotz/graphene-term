//! Loading and reloading of  and : creation, completion and migration of the files.

use super::*;

/// Loads `config.toml` and `keybindings.toml` from the config directory. A missing file
/// is created from its default (a write failure is only logged) and yields the default.
pub fn load() -> (Config, Vec<ConfigError>) {
    match config_dir() {
        Some(dir) => load_from(&dir),
        None => {
            log::warn!("neither XDG_CONFIG_HOME nor HOME is set: using the default configuration");
            (Config::default(), Vec::new())
        }
    }
}

/// Reads both files again after one changed on disk: an invalid value keeps the one of
/// `previous`, a syntax error keeps the previous contents of that file. A missing file
/// yields its default and is not written back (an editor may be replacing it).
pub fn reload(previous: &Config) -> (Config, Vec<ConfigError>) {
    match config_dir() {
        Some(dir) => reload_from(&dir, previous),
        None => (previous.clone(), Vec::new()),
    }
}

pub(crate) fn reload_from(dir: &Path, previous: &Config) -> (Config, Vec<ConfigError>) {
    let (mut config, mut errors) = reload_config_file(dir, previous);
    let (keybindings, keybinding_errors) = reload_keybindings_file(dir, previous);
    config.keybindings = keybindings;
    errors.extend(keybinding_errors);
    (config, errors)
}

pub(super) fn reload_config_file(dir: &Path, previous: &Config) -> (Config, Vec<ConfigError>) {
    let file = dir.join(CONFIG_FILE);
    match std::fs::read_to_string(&file) {
        Ok(text) => Config::parse(&text, previous, &file),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (Config::default(), Vec::new())
        }
        Err(error) => (
            previous.clone(),
            vec![ConfigError::read(
                file,
                ConfigMsg::CannotRead(error.to_string()),
            )],
        ),
    }
}

pub(super) fn reload_keybindings_file(
    dir: &Path,
    previous: &Config,
) -> (BTreeMap<String, String>, Vec<ConfigError>) {
    let file = dir.join(KEYBINDINGS_FILE);
    match std::fs::read_to_string(&file) {
        Ok(text) => {
            let (keybindings, errors, _) = Config::parse_keybindings(&text, previous, &file);
            (keybindings, errors)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (BTreeMap::new(), Vec::new()),
        Err(error) => (
            previous.keybindings.clone(),
            vec![ConfigError::read(
                file,
                ConfigMsg::CannotRead(error.to_string()),
            )],
        ),
    }
}

pub(super) fn load_from(dir: &Path) -> (Config, Vec<ConfigError>) {
    let (merged_keybindings, mut errors) = migrate_keybindings_section(dir);
    let (mut config, config_errors) = load_config_file(dir);
    errors.extend(config_errors);
    let (keybindings, keybinding_errors) = load_keybindings_file(dir, merged_keybindings);
    config.keybindings = keybindings;
    errors.extend(keybinding_errors);
    (config, errors)
}

/// Moves the `[keybindings]` section of `config.toml` into `keybindings.toml`:
/// `keybindings.toml` is written first, so a crash between the two writes leaves the
/// shortcuts in both files, never in neither. Returns the merged text of
/// `keybindings.toml` (kept even when a write fails) and the errors to report.
pub(super) fn migrate_keybindings_section(dir: &Path) -> (Option<String>, Vec<ConfigError>) {
    let config_file = dir.join(CONFIG_FILE);
    let keybindings_file = dir.join(KEYBINDINGS_FILE);
    let Ok(config_text) = std::fs::read_to_string(&config_file) else {
        return (None, Vec::new());
    };
    let keybindings_text = match std::fs::read_to_string(&keybindings_file) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return (None, Vec::new()),
    };
    let Some(migration) = crate::config_edit::migrate_keybindings(
        &config_text,
        keybindings_text.as_deref(),
        &keybindings_file,
    ) else {
        return (None, Vec::new());
    };

    if let Some(text) = &migration.keybindings
        && let Err(error) = crate::atomic_file::write_atomic(&keybindings_file, text)
    {
        log::warn!("cannot write {}: {error}", keybindings_file.display());
        return (
            migration.keybindings,
            vec![crate::config_edit::write_error(&keybindings_file, &error)],
        );
    }
    if let Err(error) = crate::atomic_file::write_atomic(&config_file, &migration.config) {
        log::warn!("cannot write {}: {error}", config_file.display());
        return (
            migration.keybindings,
            vec![crate::config_edit::write_error(&config_file, &error)],
        );
    }

    let errors = if migration.moved {
        vec![ConfigError {
            file: config_file,
            line: None,
            key: Some(key_path(&["keybindings"])),
            kind: ConfigErrorKind::Moved,
            detail: Box::new(ConfigMsg::KeybindingsMoved(KEYBINDINGS_FILE.to_owned())),
        }]
    } else {
        Vec::new()
    };
    (migration.keybindings, errors)
}

pub(super) fn load_config_file(dir: &Path) -> (Config, Vec<ConfigError>) {
    let file = dir.join(CONFIG_FILE);
    match std::fs::read_to_string(&file) {
        Ok(text) => {
            let text = match crate::config_edit::complete_defaults(&text) {
                Ok(Some(updated)) => match crate::atomic_file::write_atomic(&file, &updated) {
                    Ok(()) => updated,
                    Err(error) => {
                        log::warn!("cannot update {}: {error}", file.display());
                        text
                    }
                },
                Ok(None) | Err(_) => text,
            };
            Config::parse(&text, &Config::default(), &file)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Err(error) = std::fs::create_dir_all(dir) {
                log::warn!("cannot create {}: {error}", dir.display());
                return (Config::default(), Vec::new());
            }
            let defaults = crate::config_edit::default_file_text();
            if let Err(error) = crate::atomic_file::write_atomic(&file, &defaults) {
                log::warn!("cannot write {}: {error}", file.display());
            }
            (Config::default(), Vec::new())
        }
        Err(error) => (
            Config::default(),
            vec![ConfigError::read(
                file,
                ConfigMsg::CannotRead(error.to_string()),
            )],
        ),
    }
}

/// `merged` is the text of the file after a migration, used instead of reading it.
pub(super) fn load_keybindings_file(
    dir: &Path,
    merged: Option<String>,
) -> (BTreeMap<String, String>, Vec<ConfigError>) {
    let file = dir.join(KEYBINDINGS_FILE);
    match merged.map_or_else(|| std::fs::read_to_string(&file), Ok) {
        Ok(text) => {
            let text = match crate::config_edit::complete_keybindings_defaults(&text) {
                Ok(Some(updated)) => match crate::atomic_file::write_atomic(&file, &updated) {
                    Ok(()) => updated,
                    Err(error) => {
                        log::warn!("cannot update {}: {error}", file.display());
                        text
                    }
                },
                Ok(None) | Err(_) => text,
            };
            let (keybindings, errors, _) =
                Config::parse_keybindings(&text, &Config::default(), &file);
            (keybindings, errors)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Err(error) = std::fs::create_dir_all(dir) {
                log::warn!("cannot create {}: {error}", dir.display());
                return (BTreeMap::new(), Vec::new());
            }
            let defaults = crate::config_edit::default_keybindings_text();
            if let Err(error) = crate::atomic_file::write_atomic(&file, &defaults) {
                log::warn!("cannot write {}: {error}", file.display());
            }
            (BTreeMap::new(), Vec::new())
        }
        Err(error) => (
            BTreeMap::new(),
            vec![ConfigError::read(
                file,
                ConfigMsg::CannotRead(error.to_string()),
            )],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::previous_with_shortcut;

    #[test]
    fn missing_file_is_created_from_the_default() {
        let dir = std::env::temp_dir().join(format!("graphene-config-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let nested = dir.join("graphene");

        assert_eq!(load_from(&nested), (Config::default(), Vec::new()));
        let written = std::fs::read_to_string(nested.join(CONFIG_FILE)).unwrap();
        assert!(written.starts_with("## Graphene Term configuration.\n"));
        assert!(!written.contains("keybindings"));
        let shortcuts = std::fs::read_to_string(nested.join(KEYBINDINGS_FILE)).unwrap();
        assert!(shortcuts.starts_with("## Each string shortcut key"));
        assert_eq!(shortcuts.matches("\n[keybindings]\n").count(), 1);
        assert_eq!(shortcuts.matches("# [keybindings]").count(), 0);
        for (key, action) in keybindings::defaults() {
            assert!(shortcuts.contains(&format!("# {key:?} = {:?}\n", action.id())));
        }
        assert_eq!(load_from(&nested), (Config::default(), Vec::new()));
        assert_eq!(
            std::fs::read_to_string(nested.join(CONFIG_FILE)).unwrap(),
            written
        );
        assert_eq!(
            std::fs::read_to_string(nested.join(KEYBINDINGS_FILE)).unwrap(),
            shortcuts
        );

        std::fs::write(nested.join(CONFIG_FILE), "[font]\nsize = 9\n").unwrap();
        assert_eq!(load_from(&nested).0.font.size, 9.0);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn initial_load_completes_existing_sections_without_duplicate_headers() {
        let dir = std::env::temp_dir().join(format!(
            "graphene-config-complete-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CONFIG_FILE);
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
        std::fs::write(&path, original).unwrap();

        let (config, errors) = load_from(&dir);
        let updated = std::fs::read_to_string(&path).unwrap();

        assert!(errors.is_empty());
        assert_eq!(config.window.background_opacity, 0.9);
        assert!(config.cursor.blinking);
        assert_eq!(updated.matches("[window]").count(), 1);
        assert_eq!(updated.matches("# [window]").count(), 0);
        assert_eq!(updated.matches("# startup_mode = \"default\"").count(), 1);
        assert_eq!(updated.matches("[cursor]").count(), 1);
        assert_eq!(updated.matches("# shape = \"block\"").count(), 1);
        assert!(updated.contains(
            "## Default background opacity: 0.0 transparent to 1.0 opaque; needs a compositor. Explicit cell backgrounds, selection and cursor stay opaque; block cards are translucent.\nbackground_opacity = 0.9\n"
        ));
        assert!(updated.contains(
            "## Cursor blinking: true or false. Applications can still change this (DECSCUSR).\nblinking = true\n"
        ));
        assert!(updated.contains("background_opacity = 0.9"));
        assert!(updated.contains("blinking = true"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn initial_load_does_not_rewrite_invalid_toml() {
        let dir =
            std::env::temp_dir().join(format!("graphene-config-invalid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CONFIG_FILE);
        let invalid =
            "# keep this\n## Type: finite number in the range 6..=72.\n[font\nsize = 12\n";
        std::fs::write(&path, invalid).unwrap();

        let (_, errors) = load_from(&dir);
        assert_eq!(errors.len(), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reload_keeps_the_previous_value_of_an_invalid_key() {
        let dir =
            std::env::temp_dir().join(format!("graphene-config-reload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut previous = Config::default();
        previous.font.size = 11.0;
        previous.scrollback_lines = 500;

        std::fs::write(dir.join(CONFIG_FILE), "[font]\nsize = \"big\"\n").unwrap();
        let (config, errors) = reload_from(&dir, &previous);
        assert_eq!(config.font.size, 11.0);
        // A key absent from the file takes its default, not the previous value.
        assert_eq!(config.scrollback_lines, Config::default().scrollback_lines);
        assert_eq!(errors.len(), 1);

        std::fs::write(dir.join(CONFIG_FILE), "[font\n").unwrap();
        let (config, errors) = reload_from(&dir, &previous);
        assert_eq!(config, previous);
        assert_eq!(errors.len(), 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "graphene-config-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn load_reads_shortcuts_from_keybindings_file_and_completes_it() {
        let dir = temp_dir("load-keybindings");
        std::fs::write(dir.join(CONFIG_FILE), "[font]\nsize = 9\n").unwrap();
        std::fs::write(
            dir.join(KEYBINDINGS_FILE),
            "# mine\n[keybindings]\n\"Ctrl+Alt+T\" = \"new_tab\"\n",
        )
        .unwrap();

        let (config, errors) = load_from(&dir);
        assert_eq!(errors, Vec::new());
        assert_eq!(config.font.size, 9.0);
        assert_eq!(
            config.keybindings,
            BTreeMap::from([("Ctrl+Alt+T".to_owned(), "new_tab".to_owned())])
        );
        let text = std::fs::read_to_string(dir.join(KEYBINDINGS_FILE)).unwrap();
        assert!(text.starts_with("# mine\n"), "{text}");
        assert!(text.contains("\"Ctrl+Alt+T\" = \"new_tab\"\n"), "{text}");
        assert!(text.contains("## Allowed actions:"), "{text}");
        assert!(text.contains("# \"Ctrl+Shift+C\" = \"copy\""), "{text}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_reports_errors_of_both_files_config_first() {
        let dir = temp_dir("load-both-errors");
        std::fs::write(dir.join(CONFIG_FILE), "[font]\nsize = \"big\"\n").unwrap();
        std::fs::write(
            dir.join(KEYBINDINGS_FILE),
            "[keybindings]\n\"F7\" = \"nope\"\n",
        )
        .unwrap();

        let (_, errors) = load_from(&dir);
        let files: Vec<_> = errors
            .iter()
            .map(|e| e.file.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files, [CONFIG_FILE, KEYBINDINGS_FILE]);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn read(dir: &Path, name: &str) -> String {
        std::fs::read_to_string(dir.join(name)).unwrap()
    }

    fn generated_keybindings_block() -> String {
        let mut block = String::from("# [keybindings]\n");
        for line in DEFAULT_KEYBINDINGS.lines().filter(|l| l.starts_with("##")) {
            block.push_str(line);
            block.push('\n');
        }
        block.push_str("# \"Ctrl+Shift+C\" = \"copy\"\n");
        block
    }

    #[test]
    fn load_migrates_the_keybindings_section_to_the_new_file() {
        let dir = temp_dir("migrate-simple");
        std::fs::write(
            dir.join(CONFIG_FILE),
            "[font]\nsize = 9\n\n[keybindings]\n# my tab key\n\"Ctrl+Alt+T\" = \"new_tab\" # inline\n\n[updates]\ncheck = false\n",
        )
        .unwrap();

        let (config, errors) = load_from(&dir);
        assert_eq!(
            keys(&errors),
            vec![(Some("keybindings"), ConfigErrorKind::Moved)]
        );
        assert_eq!(errors[0].file, dir.join(CONFIG_FILE));
        assert_eq!(
            errors[0].message(Lang::EnUs),
            "keyboard shortcuts moved to keybindings.toml"
        );
        assert_eq!(config.font.size, 9.0);
        assert_eq!(
            config.keybindings,
            BTreeMap::from([("Ctrl+Alt+T".to_owned(), "new_tab".to_owned())])
        );
        let moved = read(&dir, KEYBINDINGS_FILE);
        assert!(
            moved.contains("# my tab key\n\"Ctrl+Alt+T\" = \"new_tab\" # inline\n"),
            "{moved}"
        );
        let remaining = read(&dir, CONFIG_FILE);
        assert!(!remaining.contains("keybindings"), "{remaining}");
        assert!(!remaining.contains("Ctrl+Alt+T"), "{remaining}");
        assert!(remaining.contains("[font]\n"), "{remaining}");
        assert!(remaining.contains("size = 9\n"), "{remaining}");
        assert!(remaining.contains("check = false\n"), "{remaining}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn migration_keeps_the_shortcut_of_keybindings_file_on_conflict() {
        let dir = temp_dir("migrate-conflict");
        std::fs::write(
            dir.join(CONFIG_FILE),
            "[keybindings]\n\"F7\" = \"copy\"\n\"F8\" = \"paste\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(KEYBINDINGS_FILE),
            "[keybindings]\n\"F7\" = \"quit\"\n",
        )
        .unwrap();

        let (config, errors) = load_from(&dir);
        assert_eq!(
            keys(&errors),
            vec![(Some("keybindings"), ConfigErrorKind::Moved)]
        );
        assert_eq!(
            config.keybindings,
            BTreeMap::from([
                ("F7".to_owned(), "quit".to_owned()),
                ("F8".to_owned(), "paste".to_owned()),
            ])
        );
        assert!(!read(&dir, CONFIG_FILE).contains("keybindings"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn migration_removes_a_generated_commented_block_silently() {
        let dir = temp_dir("migrate-commented");
        let config = format!(
            "[font]\nsize = 9\n\n{}\n# [updates]\n# check = true\n",
            generated_keybindings_block()
        );
        std::fs::write(dir.join(CONFIG_FILE), &config).unwrap();

        let (loaded, errors) = load_from(&dir);
        assert_eq!(errors, Vec::new());
        assert!(loaded.keybindings.is_empty());
        let remaining = read(&dir, CONFIG_FILE);
        assert!(!remaining.contains("keybindings"), "{remaining}");
        assert!(!remaining.contains("Each string shortcut"), "{remaining}");
        assert!(!remaining.contains("Ctrl+Shift+C"), "{remaining}");
        assert!(remaining.contains("size = 9\n"), "{remaining}");
        assert!(remaining.contains("# [updates]\n"), "{remaining}");
        assert!(read(&dir, KEYBINDINGS_FILE).contains("[keybindings]"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn migration_keeps_unrecognized_commented_lines_in_config_file() {
        let dir = temp_dir("migrate-user-comments");
        std::fs::write(
            dir.join(CONFIG_FILE),
            "# [keybindings]\n# remember to bind F9\n# \"F9\" = \"copy\"\n# \"Ctrl+Shift+C\" = \"copy\"\n",
        )
        .unwrap();

        let (_, errors) = load_from(&dir);
        assert_eq!(errors, Vec::new());
        let remaining = read(&dir, CONFIG_FILE);
        assert!(
            remaining.starts_with("# remember to bind F9\n# \"F9\" = \"copy\"\n"),
            "{remaining}"
        );
        assert!(!remaining.contains("[keybindings]"), "{remaining}");
        assert!(!remaining.contains("Ctrl+Shift+C"), "{remaining}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn migration_drops_generated_lines_but_moves_user_ones_of_an_active_section() {
        let dir = temp_dir("migrate-active-generated");
        let doc: String = DEFAULT_KEYBINDINGS
            .lines()
            .filter(|l| l.starts_with("##"))
            .map(|l| format!("{l}\n"))
            .collect();
        std::fs::write(
            dir.join(CONFIG_FILE),
            format!(
                "[keybindings]\n{doc}# \"Ctrl+Shift+C\" = \"copy\"\n# mine\n\"F7\" = \"copy\"\n"
            ),
        )
        .unwrap();

        let (config, _) = load_from(&dir);
        assert_eq!(config.keybindings.len(), 1);
        let remaining = read(&dir, CONFIG_FILE);
        assert!(!remaining.contains("keybindings"), "{remaining}");
        assert!(!remaining.contains("Ctrl+Shift+C"), "{remaining}");
        assert!(!remaining.contains("Each string shortcut"), "{remaining}");
        let moved = read(&dir, KEYBINDINGS_FILE);
        assert!(moved.contains("# mine\nF7 = \"copy\"\n"), "{moved}");
        assert_eq!(
            moved.matches("## Each string shortcut").count(),
            1,
            "{moved}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn migration_is_idempotent() {
        let dir = temp_dir("migrate-idempotent");
        std::fs::write(
            dir.join(CONFIG_FILE),
            "[font]\nsize = 9\n\n[keybindings]\n\"F7\" = \"copy\"\n",
        )
        .unwrap();

        let _ = load_from(&dir);
        let config_text = read(&dir, CONFIG_FILE);
        let keybindings_text = read(&dir, KEYBINDINGS_FILE);
        let (config, errors) = load_from(&dir);
        assert_eq!(errors, Vec::new());
        assert_eq!(config.keybindings.len(), 1);
        assert_eq!(read(&dir, CONFIG_FILE), config_text);
        assert_eq!(read(&dir, KEYBINDINGS_FILE), keybindings_text);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn migration_resumes_after_a_crash_between_the_two_writes() {
        let dir = temp_dir("migrate-crash");
        std::fs::write(dir.join(CONFIG_FILE), "[keybindings]\n\"F7\" = \"copy\"\n").unwrap();
        std::fs::write(
            dir.join(KEYBINDINGS_FILE),
            "[keybindings]\n\"F7\" = \"copy\"\n",
        )
        .unwrap();

        let (config, errors) = load_from(&dir);
        assert_eq!(
            keys(&errors),
            vec![(Some("keybindings"), ConfigErrorKind::Moved)]
        );
        assert_eq!(config.keybindings.len(), 1);
        assert!(!read(&dir, CONFIG_FILE).contains("keybindings"));
        assert_eq!(read(&dir, KEYBINDINGS_FILE).matches("\"F7\"").count(), 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn migration_does_not_touch_files_when_keybindings_file_is_invalid() {
        let dir = temp_dir("migrate-invalid-destination");
        let config_text = "[keybindings]\n\"F7\" = \"copy\"\n";
        std::fs::write(dir.join(CONFIG_FILE), config_text).unwrap();
        std::fs::write(dir.join(KEYBINDINGS_FILE), "[keybindings\n").unwrap();

        let (_, errors) = load_from(&dir);
        assert!(errors.iter().any(|e| e.kind == ConfigErrorKind::Syntax));
        assert!(read(&dir, CONFIG_FILE).contains(config_text));
        assert_eq!(read(&dir, KEYBINDINGS_FILE), "[keybindings\n");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn migration_keeps_merged_shortcuts_in_memory_when_writing_fails() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("migrate-write-failure");
        let config_text = "[keybindings]\n\"F7\" = \"copy\"\n";
        std::fs::write(dir.join(CONFIG_FILE), config_text).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let writable = std::fs::File::create(dir.join("probe")).is_ok();

        let (config, errors) = load_from(&dir);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        if !writable {
            assert!(errors.iter().any(|e| e.kind == ConfigErrorKind::Write));
            assert_eq!(
                config.keybindings,
                BTreeMap::from([("F7".to_owned(), "copy".to_owned())])
            );
            assert_eq!(read(&dir, CONFIG_FILE), config_text);
            assert!(!dir.join(KEYBINDINGS_FILE).exists());
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reload_does_not_migrate_the_keybindings_section() {
        let dir = temp_dir("reload-no-migration");
        let config_text = "[keybindings]\n\"F7\" = \"copy\"\n";
        std::fs::write(dir.join(CONFIG_FILE), config_text).unwrap();

        let (config, errors) = reload_from(&dir, &Config::default());
        assert!(config.keybindings.is_empty());
        assert_eq!(
            keys(&errors),
            vec![(Some("keybindings"), ConfigErrorKind::Moved)]
        );
        assert_eq!(read(&dir, CONFIG_FILE), config_text);
        assert!(!dir.join(KEYBINDINGS_FILE).exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reload_reads_both_files_and_keeps_shortcuts_on_a_syntax_error() {
        let dir = temp_dir("reload-both");
        let previous = previous_with_shortcut();
        std::fs::write(dir.join(CONFIG_FILE), "[font]\nsize = 9\n").unwrap();
        std::fs::write(
            dir.join(KEYBINDINGS_FILE),
            "[keybindings]\n\"F7\" = \"copy\"\n",
        )
        .unwrap();

        let (config, errors) = reload_from(&dir, &previous);
        assert_eq!(errors, Vec::new());
        assert_eq!(config.font.size, 9.0);
        assert_eq!(
            config.keybindings,
            BTreeMap::from([("F7".to_owned(), "copy".to_owned())])
        );

        std::fs::write(dir.join(KEYBINDINGS_FILE), "[keybindings\n").unwrap();
        let (config, errors) = reload_from(&dir, &previous);
        assert_eq!(config.font.size, 9.0);
        assert_eq!(config.keybindings, previous.keybindings);
        assert_eq!(errors.len(), 1);

        // A missing file yields no shortcuts and is not written back.
        std::fs::remove_file(dir.join(KEYBINDINGS_FILE)).unwrap();
        let (config, errors) = reload_from(&dir, &previous);
        assert!(config.keybindings.is_empty());
        assert_eq!(errors, Vec::new());
        assert!(!dir.join(KEYBINDINGS_FILE).exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reload_does_not_write_keybindings_file() {
        let dir = temp_dir("reload-keybindings-read-only");
        let original = "# keep me\n[keybindings]\n\"F7\" = \"copy\"\n";
        std::fs::write(dir.join(KEYBINDINGS_FILE), original).unwrap();

        let _ = reload_from(&dir, &Config::default());
        assert_eq!(
            std::fs::read_to_string(dir.join(KEYBINDINGS_FILE)).unwrap(),
            original
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reload_does_not_complete_or_write_the_file() {
        let dir = std::env::temp_dir().join(format!(
            "graphene-config-reload-read-only-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CONFIG_FILE);
        let original = "# keep me\n[font]\nsize = 9\n";
        std::fs::write(&path, original).unwrap();

        let _ = reload_from(&dir, &Config::default());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reload_of_a_missing_file_is_the_default_without_writing_it() {
        let dir = std::env::temp_dir().join(format!("graphene-config-gone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut previous = Config::default();
        previous.font.size = 11.0;

        assert_eq!(
            reload_from(&dir, &previous),
            (Config::default(), Vec::new())
        );
        assert!(!dir.join(CONFIG_FILE).exists());
    }

    fn keys(errors: &[ConfigError]) -> Vec<(Option<&str>, ConfigErrorKind)> {
        errors
            .iter()
            .map(|error| (error.key.as_deref(), error.kind))
            .collect()
    }
}
