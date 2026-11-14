//! The last window closed: `window.json` in the state directory, read when
//! `window.startup_mode` is `remember`. Errors are only logged: a missing or unreadable
//! file means "nothing remembered".

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config;

/// Name of the file inside the state directory.
pub const WINDOW_FILE: &str = "window.json";

/// Size and state of a window, in logical pixels. The size is the last one seen while
/// the window was not maximized, so that unmaximizing a restored window gives it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

/// `window.json` in the state directory ([`config::state_dir`]).
pub fn path() -> Option<PathBuf> {
    config::state_dir().map(|dir| dir.join(WINDOW_FILE))
}

impl WindowState {
    /// Reads `file`. A missing, unreadable, corrupt or zero-size file gives `None`; only
    /// the last three are logged.
    pub fn load(file: &Path) -> Option<WindowState> {
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(error) => {
                log::warn!("cannot read {}: {error}", file.display());
                return None;
            }
        };
        let state: WindowState = serde_json::from_str(&text)
            .map_err(|error| log::warn!("ignoring corrupt {}: {error}", file.display()))
            .ok()?;
        if state.width == 0 || state.height == 0 {
            log::warn!("ignoring {}: zero window size", file.display());
            return None;
        }
        Some(state)
    }

    /// Writes `file` through a temporary file renamed over it, so that two instances
    /// closing together never leave a partial file: the last one wins.
    pub fn save(&self, file: &Path) -> io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut temp = file.as_os_str().to_owned();
        temp.push(format!(".{}.tmp", std::process::id()));
        let temp = PathBuf::from(temp);
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let result = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, file));
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "graphene-window-state-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            TempDir(dir)
        }

        fn file(&self) -> PathBuf {
            self.0.join("graphene").join(WINDOW_FILE)
        }

        fn write(&self, text: &str) -> PathBuf {
            let file = self.file();
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, text).unwrap();
            file
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn round_trip() {
        let dir = TempDir::new("round-trip");
        let file = dir.file();
        let state = WindowState {
            width: 1280,
            height: 720,
            maximized: true,
        };
        state.save(&file).unwrap();
        assert_eq!(WindowState::load(&file), Some(state));

        // Saving again replaces the file and leaves no temporary file behind.
        let state = WindowState {
            width: 800,
            height: 500,
            maximized: false,
        };
        state.save(&file).unwrap();
        assert_eq!(WindowState::load(&file), Some(state));
        let entries: Vec<_> = std::fs::read_dir(file.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![OsString::from(WINDOW_FILE)]);
    }

    #[test]
    fn missing_file_is_nothing_remembered() {
        let dir = TempDir::new("missing");
        assert_eq!(WindowState::load(&dir.file()), None);
    }

    #[test]
    fn corrupt_file_is_nothing_remembered() {
        let dir = TempDir::new("corrupt");
        let file = dir.write("{ \"width\": ");
        assert_eq!(WindowState::load(&file), None);
        let file = dir.write("{ \"width\": \"wide\", \"height\": 600, \"maximized\": false }");
        assert_eq!(WindowState::load(&file), None);
        let file = dir.write("{ \"width\": 800 }");
        assert_eq!(WindowState::load(&file), None);
    }

    #[test]
    fn zero_size_is_nothing_remembered() {
        let dir = TempDir::new("zero");
        let file = dir.write("{ \"width\": 0, \"height\": 600, \"maximized\": false }");
        assert_eq!(WindowState::load(&file), None);
        let file = dir.write("{ \"width\": 800, \"height\": 0, \"maximized\": true }");
        assert_eq!(WindowState::load(&file), None);
    }
}
