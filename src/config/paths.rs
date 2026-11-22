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

/// Folder of the sockets and locks shared by the `graphene` processes of the user,
/// created with mode `0700` so that only the user reaches them.
pub fn runtime_dir() -> std::io::Result<PathBuf> {
    // SAFETY: `getuid` has no precondition and cannot fail.
    let uid = unsafe { libc::getuid() };
    let dir = runtime_dir_from(
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::var_os("TMPDIR"),
        uid,
    );
    create_private_dir(&dir)?;
    Ok(dir)
}

/// `$XDG_RUNTIME_DIR/graphene` on Linux, else `graphene-<uid>` in the temporary
/// directory (`$TMPDIR`, else `/tmp`), which is the case on macOS.
pub(super) fn runtime_dir_from(
    xdg_runtime_dir: Option<OsString>,
    tmpdir: Option<OsString>,
    uid: u32,
) -> PathBuf {
    // The XDG spec ignores an empty or relative `XDG_RUNTIME_DIR`.
    let xdg = xdg_runtime_dir
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && cfg!(not(target_os = "macos")));
    if let Some(xdg) = xdg {
        return xdg.join("graphene");
    }
    let tmp = tmpdir
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    tmp.join(format!("graphene-{uid}"))
}

/// Creates `dir` and its parents, and gives it mode `0700`.
pub(super) fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    // An existing folder keeps its mode with `create`.
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_dir_prefers_xdg_runtime_dir_on_linux() {
        let dir = runtime_dir_from(Some("/run/user/1000".into()), Some("/tmp".into()), 1000);
        #[cfg(target_os = "macos")]
        assert_eq!(dir, PathBuf::from("/tmp/graphene-1000"));
        #[cfg(not(target_os = "macos"))]
        assert_eq!(dir, PathBuf::from("/run/user/1000/graphene"));
    }

    #[test]
    fn runtime_dir_falls_back_to_the_temporary_directory() {
        assert_eq!(
            runtime_dir_from(None, Some("/var/folders/x".into()), 501),
            PathBuf::from("/var/folders/x/graphene-501")
        );
        assert_eq!(
            runtime_dir_from(Some("relative".into()), None, 501),
            PathBuf::from("/tmp/graphene-501")
        );
        assert_eq!(
            runtime_dir_from(Some("".into()), Some("".into()), 7),
            PathBuf::from("/tmp/graphene-7")
        );
    }

    #[test]
    fn private_dir_is_created_with_mode_0700() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("graphene-runtime-{}", std::process::id()));
        let dir = base.join("nested");
        create_private_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        create_private_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        std::fs::remove_dir_all(&base).unwrap();
    }

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
