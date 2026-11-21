//! Folders of the configuration and of the state files.

use super::*;

/// `$XDG_CONFIG_HOME/graphene`, else the platform's default application config directory.
pub fn config_dir() -> Option<PathBuf> {
    config_dir_from(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

/// Path of the user's configuration file, if a config directory can be resolved.
pub(crate) fn config_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(CONFIG_FILE))
}

pub(super) fn config_dir_from(
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    // The XDG spec ignores an empty or relative `XDG_CONFIG_HOME`.
    let xdg = xdg_config_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let base = xdg.or_else(|| {
        home.filter(|home| !home.is_empty())
            .map(default_config_base)
    })?;
    Some(base.join("graphene"))
}

pub(super) fn default_config_base(home: OsString) -> PathBuf {
    #[cfg(target_os = "macos")]
    let base = PathBuf::from(home)
        .join("Library")
        .join("Application Support");
    #[cfg(not(target_os = "macos"))]
    let base = PathBuf::from(home).join(".config");
    base
}

/// `$XDG_STATE_HOME/graphene`, else the platform's default application state directory.
pub fn state_dir() -> Option<PathBuf> {
    state_dir_from(std::env::var_os("XDG_STATE_HOME"), std::env::var_os("HOME"))
}

pub(super) fn state_dir_from(
    xdg_state_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    // The XDG spec ignores an empty or relative `XDG_STATE_HOME`.
    let xdg = xdg_state_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let base = xdg.or_else(|| home.filter(|home| !home.is_empty()).map(default_state_base))?;
    Some(base.join("graphene"))
}

pub(super) fn default_state_base(home: OsString) -> PathBuf {
    #[cfg(target_os = "macos")]
    let base = PathBuf::from(home)
        .join("Library")
        .join("Application Support");
    #[cfg(not(target_os = "macos"))]
    let base = PathBuf::from(home).join(".local").join("state");
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_prefers_xdg_config_home() {
        assert_eq!(
            config_dir_from(Some("/xdg".into()), Some("/home/u".into())),
            Some(PathBuf::from("/xdg/graphene"))
        );
    }

    #[test]
    fn config_dir_falls_back_to_home() {
        #[cfg(target_os = "macos")]
        let expected = Some(PathBuf::from(
            "/home/u/Library/Application Support/graphene",
        ));
        #[cfg(not(target_os = "macos"))]
        let expected = Some(PathBuf::from("/home/u/.config/graphene"));
        assert_eq!(config_dir_from(None, Some("/home/u".into())), expected);
        assert_eq!(
            config_dir_from(Some("".into()), Some("/home/u".into())),
            expected
        );
        assert_eq!(
            config_dir_from(Some("relative".into()), Some("/home/u".into())),
            expected
        );
        assert_eq!(config_dir_from(None, None), None);
        assert_eq!(config_dir_from(None, Some("".into())), None);
    }

    #[test]
    fn state_dir_prefers_xdg_state_home() {
        assert_eq!(
            state_dir_from(Some("/xdg".into()), Some("/home/u".into())),
            Some(PathBuf::from("/xdg/graphene"))
        );
    }

    #[test]
    fn state_dir_falls_back_to_home() {
        #[cfg(target_os = "macos")]
        let expected = Some(PathBuf::from(
            "/home/u/Library/Application Support/graphene",
        ));
        #[cfg(not(target_os = "macos"))]
        let expected = Some(PathBuf::from("/home/u/.local/state/graphene"));
        assert_eq!(state_dir_from(None, Some("/home/u".into())), expected);
        assert_eq!(
            state_dir_from(Some("".into()), Some("/home/u".into())),
            expected
        );
        assert_eq!(
            state_dir_from(Some("relative".into()), Some("/home/u".into())),
            expected
        );
        assert_eq!(state_dir_from(None, None), None);
        assert_eq!(state_dir_from(None, Some("".into())), None);
    }
}
