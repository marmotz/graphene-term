//! Update check state, shared by every Graphene instance: `update.json` in the state
//! directory. Errors are only logged: a missing or unreadable file means "never checked".

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::Release;
use crate::config;

/// Name of the state file inside the state directory.
pub const STATE_FILE: &str = "update.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// Unix time of the last successful check, `0` when never checked.
    #[serde(default)]
    pub last_check: u64,
    /// Newest release found by the last check.
    #[serde(default)]
    pub latest: Option<Release>,
    /// Tag the user chose to ignore.
    #[serde(default)]
    pub ignored: Option<String>,
}

/// `update.json` in the state directory ([`config::state_dir`]).
pub fn path() -> Option<PathBuf> {
    config::state_dir().map(|dir| dir.join(STATE_FILE))
}

impl State {
    /// Reads the state file. A missing, unreadable or corrupt file gives the default
    /// state; only the last two are logged.
    pub fn load(file: &Path) -> State {
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return State::default(),
            Err(error) => {
                log::warn!("cannot read {}: {error}", file.display());
                return State::default();
            }
        };
        serde_json::from_str(&text).unwrap_or_else(|error| {
            log::warn!("ignoring corrupt {}: {error}", file.display());
            State::default()
        })
    }

    /// Writes the state file through a temporary file renamed over it, so that another
    /// instance never reads a partial file.
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

/// Records `tag` as the ignored version in the state `file`, keeping the other fields.
pub fn ignore(file: &Path, tag: &str) -> io::Result<()> {
    let mut state = State::load(file);
    state.ignored = Some(tag.to_owned());
    state.save(file)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "graphene-update-state-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            TempDir(dir)
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
        let file = dir.0.join("graphene").join(STATE_FILE);
        let state = State {
            last_check: 1_794_501_169,
            latest: Some(Release {
                tag: "v0.2.0".to_owned(),
                url: "https://github.com/marmotz/graphene-term/releases/tag/v0.2.0".to_owned(),
            }),
            ignored: Some("v0.1.1".to_owned()),
        };
        state.save(&file).unwrap();
        assert_eq!(State::load(&file), state);

        // Saving again replaces the file and leaves no temporary file behind.
        let state = State {
            last_check: 1_794_600_000,
            ..State::default()
        };
        state.save(&file).unwrap();
        assert_eq!(State::load(&file), state);
        let entries: Vec<_> = std::fs::read_dir(file.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![OsString::from(STATE_FILE)]);
    }

    #[test]
    fn missing_file_is_never_checked() {
        let dir = TempDir::new("missing");
        assert_eq!(State::load(&dir.0.join(STATE_FILE)), State::default());
    }

    #[test]
    fn corrupt_file_is_never_checked() {
        let dir = TempDir::new("corrupt");
        std::fs::create_dir_all(&dir.0).unwrap();
        let file = dir.0.join(STATE_FILE);
        std::fs::write(&file, "{ \"last_check\": ").unwrap();
        assert_eq!(State::load(&file), State::default());
        std::fs::write(&file, "{ \"last_check\": \"yesterday\" }").unwrap();
        assert_eq!(State::load(&file), State::default());
    }

    #[test]
    fn ignore_keeps_the_other_fields() {
        let dir = TempDir::new("ignore");
        let file = dir.0.join("graphene").join(STATE_FILE);
        let latest = Some(Release {
            tag: "v0.2.0".to_owned(),
            url: "https://github.com/marmotz/graphene-term/releases/tag/v0.2.0".to_owned(),
        });
        State {
            last_check: 1_794_501_169,
            latest: latest.clone(),
            ignored: Some("v0.1.1".to_owned()),
        }
        .save(&file)
        .unwrap();

        ignore(&file, "v0.2.0").unwrap();
        assert_eq!(
            State::load(&file),
            State {
                last_check: 1_794_501_169,
                latest,
                ignored: Some("v0.2.0".to_owned()),
            }
        );
    }

    #[test]
    fn ignore_creates_a_missing_file() {
        let dir = TempDir::new("ignore-missing");
        let file = dir.0.join("graphene").join(STATE_FILE);
        ignore(&file, "v0.2.0").unwrap();
        assert_eq!(
            State::load(&file),
            State {
                ignored: Some("v0.2.0".to_owned()),
                ..State::default()
            }
        );
    }

    #[test]
    fn missing_fields_take_their_default() {
        let dir = TempDir::new("partial");
        std::fs::create_dir_all(&dir.0).unwrap();
        let file = dir.0.join(STATE_FILE);
        std::fs::write(&file, "{ \"ignored\": \"v0.3.0\" }").unwrap();
        assert_eq!(
            State::load(&file),
            State {
                ignored: Some("v0.3.0".to_owned()),
                ..State::default()
            }
        );
    }
}
