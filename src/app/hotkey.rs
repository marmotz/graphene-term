//! The global hotkey of the quick terminal process, the hidden start of that process
//! by an ordinary window when a hotkey is configured, and its session autostart entry.

use super::*;
use crate::config::QuickTerminalConfig;
use crate::hotkey::{Hotkey, HotkeyError};
use crate::quick_terminal::{QuickCommand, autostart, control};

/// Whether an ordinary window starts the quick terminal hidden for `quick`: the quick
/// terminal is enabled, has a hotkey, and is `supported` by the session.
pub(super) fn wants_hidden_quick(quick: &QuickTerminalConfig, supported: bool) -> bool {
    supported && quick.enabled && !quick.hotkey.is_empty()
}

/// Whether a reload from `old` to `new` asks for the hidden start again: enabling the
/// quick terminal with a hotkey set, or setting the hotkey of an enabled one.
pub(super) fn hidden_start_turned_on(old: &QuickTerminalConfig, new: &QuickTerminalConfig) -> bool {
    !wants_hidden_quick(old, true) && wants_hidden_quick(new, true)
}

/// The effective autostart setting of `quick`: the entry exists only for an enabled
/// quick terminal.
pub(super) fn effective_autostart(quick: &QuickTerminalConfig) -> bool {
    quick.enabled && quick.autostart
}

/// Whether the session autostart entry is synchronised with the setting `current`, the
/// quick terminal being `supported` by the session: at start (`previous` is `None`) to
/// catch up with outside changes, then on a reload only when the setting changed.
pub(super) fn wants_autostart_sync(previous: Option<bool>, current: bool, supported: bool) -> bool {
    supported && previous != Some(current)
}

/// The configuration error the banner shows for `error`.
pub(super) fn hotkey_config_error(error: &HotkeyError) -> ConfigError {
    ConfigError {
        file: config::config_file().unwrap_or_else(|| PathBuf::from("config.toml")),
        line: None,
        key: Some("quick_terminal.hotkey".to_owned()),
        kind: config::ConfigErrorKind::InvalidValue,
        message: error.to_string(),
    }
}

/// The configuration banner: the errors of the files, then the hotkey's.
pub(super) fn config_banner(load: &LoadErrors, hotkey: Option<&ConfigError>) -> Option<Banner> {
    let all: Vec<ConfigError> = load
        .config
        .iter()
        .chain(&load.theme)
        .chain(hotkey)
        .cloned()
        .collect();
    Banner::config_errors(&all)
}

impl App {
    /// Registers the configured hotkey in the quick terminal process, and returns
    /// whether its error changed. A failure is logged and kept for the configuration
    /// banner; the process goes on without a hotkey.
    pub(super) fn register_hotkey(&mut self) -> bool {
        if self.quick_state().is_none() {
            return false;
        }
        let proxy = self.proxy.clone();
        let hotkey = self.hotkey.get_or_insert_with(|| {
            Hotkey::new(Arc::new(move || {
                // The event loop may already be closed during shutdown.
                let _ = proxy.send_event(UserEvent::Quick(QuickCommand::Toggle));
            }))
        });
        let error = match hotkey.set(&self.config.quick_terminal.hotkey) {
            Ok(()) => None,
            Err(error) => {
                log::warn!("{error}");
                Some(hotkey_config_error(&error))
            }
        };
        let changed = error != self.hotkey_error;
        self.hotkey_error = error;
        changed
    }

    /// Shows the configuration banner of the current errors, the hotkey's included: the
    /// hidden quick terminal shows it at its next appearance.
    pub(super) fn refresh_config_banner(&mut self) {
        let banner = config_banner(&self.load_errors, self.hotkey_error.as_ref());
        self.config_banner = banner.clone();
        if let Some(session) = self.session.as_mut() {
            session.config_banner = banner;
            session.relayout();
        }
    }

    /// From an ordinary window, starts the quick terminal hidden when it is enabled, a
    /// hotkey is configured and none listens, so that the hotkey works before any
    /// `--quick-toggle`. The single instance lock makes simultaneous starts harmless.
    pub(super) fn start_hidden_quick(&self) {
        if self.mode != WindowMode::Normal
            || !wants_hidden_quick(&self.config.quick_terminal, self.quick_supported)
        {
            return;
        }
        let spawned = std::thread::Builder::new()
            .name("quick-terminal-start".into())
            .spawn(|| {
                let started = config::runtime_dir().and_then(|dir| control::start_hidden(&dir));
                if let Err(error) = started {
                    log::warn!("cannot start the quick terminal: {error}");
                }
            });
        if let Err(error) = spawned {
            log::warn!("cannot start the quick terminal: {error}");
        }
    }

    /// Makes the session autostart entry follow the effective setting
    /// ([`effective_autostart`]), `previous` being its effective value before a reload
    /// (`None` at start). Nothing is created or removed where the quick
    /// terminal is unsupported; a failure is logged.
    pub(super) fn sync_autostart(&self, previous: Option<bool>) {
        let enabled = effective_autostart(&self.config.quick_terminal);
        if !wants_autostart_sync(previous, enabled, self.quick_supported) {
            return;
        }
        let spawned = std::thread::Builder::new()
            .name("quick-terminal-autostart".into())
            .spawn(move || {
                if let Err(error) = autostart::sync(enabled) {
                    log::warn!("cannot update the session autostart entry: {error}");
                }
            });
        if let Err(error) = spawned {
            log::warn!("cannot update the session autostart entry: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quick terminal configuration, `enabled` or not, with `hotkey` and `autostart`.
    fn quick(enabled: bool, hotkey: &str, autostart: bool) -> QuickTerminalConfig {
        QuickTerminalConfig {
            enabled,
            hotkey: hotkey.to_owned(),
            autostart,
            ..Config::default().quick_terminal
        }
    }

    #[test]
    fn no_hidden_start_unless_enabled_with_a_hotkey_where_supported() {
        assert!(!wants_hidden_quick(&quick(true, "", false), true));
        assert!(!wants_hidden_quick(&quick(true, "Ctrl+`", false), false));
        assert!(!wants_hidden_quick(&quick(false, "Ctrl+`", false), true));
        assert!(wants_hidden_quick(&quick(true, "Ctrl+`", false), true));
    }

    #[test]
    fn hidden_start_again_when_a_reload_sets_the_hotkey_of_an_enabled_quick_terminal() {
        let on = |old: (bool, &str), new: (bool, &str)| {
            hidden_start_turned_on(&quick(old.0, old.1, false), &quick(new.0, new.1, false))
        };
        assert!(on((true, ""), (true, "Ctrl+`")));
        assert!(!on((true, "Ctrl+`"), (true, "Ctrl+F12")));
        assert!(!on((true, "Ctrl+`"), (true, "")));
        assert!(!on((true, ""), (true, "")));
        assert!(!on((false, ""), (false, "Ctrl+`")));
    }

    #[test]
    fn hidden_start_again_when_a_reload_enables_a_quick_terminal_with_a_hotkey() {
        let on = |old: (bool, &str), new: (bool, &str)| {
            hidden_start_turned_on(&quick(old.0, old.1, false), &quick(new.0, new.1, false))
        };
        assert!(on((false, "Ctrl+`"), (true, "Ctrl+`")));
        assert!(on((false, ""), (true, "Ctrl+`")));
        assert!(!on((false, ""), (true, "")));
        assert!(!on((true, "Ctrl+`"), (false, "Ctrl+`")));
    }

    #[test]
    fn autostart_is_effective_only_for_an_enabled_quick_terminal() {
        assert!(effective_autostart(&quick(true, "", true)));
        assert!(!effective_autostart(&quick(false, "", true)));
        assert!(!effective_autostart(&quick(true, "", false)));
        assert!(!effective_autostart(&quick(false, "", false)));
    }

    #[test]
    fn autostart_syncs_at_start_and_when_a_reload_changes_it() {
        assert!(wants_autostart_sync(None, true, true));
        assert!(wants_autostart_sync(None, false, true));
        assert!(wants_autostart_sync(Some(false), true, true));
        assert!(wants_autostart_sync(Some(true), false, true));
        assert!(!wants_autostart_sync(Some(true), true, true));
        assert!(!wants_autostart_sync(Some(false), false, true));
    }

    #[test]
    fn no_autostart_sync_where_unsupported() {
        assert!(!wants_autostart_sync(None, true, false));
        assert!(!wants_autostart_sync(None, false, false));
        assert!(!wants_autostart_sync(Some(false), true, false));
    }

    #[test]
    fn hotkey_error_is_reported_on_its_key() {
        let error = hotkey_config_error(&HotkeyError::Taken("Ctrl+`".into()));
        assert_eq!(error.key.as_deref(), Some("quick_terminal.hotkey"));
        assert_eq!(error.kind, config::ConfigErrorKind::InvalidValue);
        assert!(
            error
                .to_string()
                .ends_with("the global hotkey \"Ctrl+`\" is already used by another application")
        );
    }

    #[test]
    fn banner_shows_the_file_errors_first_then_the_hotkey() {
        let hotkey = hotkey_config_error(&HotkeyError::Taken("Ctrl+`".into()));
        let none = LoadErrors::default();
        assert_eq!(config_banner(&none, None), None);
        let alone = config_banner(&none, Some(&hotkey)).unwrap();
        assert_eq!(alone.text, hotkey.to_string());

        let file = ConfigError::read(PathBuf::from("config.toml"), "unreadable".into());
        let load = LoadErrors {
            config: vec![file.clone()],
            theme: Vec::new(),
        };
        let both = config_banner(&load, Some(&hotkey)).unwrap();
        assert_eq!(both.text, format!("{file} (+1 more)"));
    }
}
