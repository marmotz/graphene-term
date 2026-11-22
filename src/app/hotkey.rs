//! The global hotkey of the quick terminal process, and the hidden start of that
//! process by an ordinary window when a hotkey is configured.

use super::*;
use crate::hotkey::{Hotkey, HotkeyError};
use crate::quick_terminal::{QuickCommand, control};

/// Whether an ordinary window starts the quick terminal hidden for `hotkey`, the quick
/// terminal being `supported` by the session.
pub(super) fn wants_hidden_quick(hotkey: &str, supported: bool) -> bool {
    supported && !hotkey.is_empty()
}

/// Whether a reload from the hotkey `old` to `new` asks for the hidden start again.
pub(super) fn hotkey_turned_on(old: &str, new: &str) -> bool {
    old.is_empty() && !new.is_empty()
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

    /// From an ordinary window, starts the quick terminal hidden when a hotkey is
    /// configured and none listens, so that the hotkey works before any
    /// `--quick-toggle`. The single instance lock makes simultaneous starts harmless.
    pub(super) fn start_hidden_quick(&self) {
        if self.mode != WindowMode::Normal
            || !wants_hidden_quick(&self.config.quick_terminal.hotkey, self.quick_supported)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_hidden_start_without_a_hotkey_or_where_unsupported() {
        assert!(!wants_hidden_quick("", true));
        assert!(!wants_hidden_quick("Ctrl+`", false));
        assert!(wants_hidden_quick("Ctrl+`", true));
    }

    #[test]
    fn hidden_start_again_only_when_a_reload_sets_the_hotkey() {
        assert!(hotkey_turned_on("", "Ctrl+`"));
        assert!(!hotkey_turned_on("Ctrl+`", "Ctrl+F12"));
        assert!(!hotkey_turned_on("Ctrl+`", ""));
        assert!(!hotkey_turned_on("", ""));
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
