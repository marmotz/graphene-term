//! Errors found in the configuration files, with the place and key path of each.

use super::*;

/// An error found in a configuration file.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigError {
    pub file: PathBuf,
    /// 1-based line of the faulty key or value, when known.
    pub line: Option<usize>,
    /// Dotted TOML path of the faulty key (`font.size`, `keybindings."Ctrl+X"`), `None`
    /// for an error not tied to a key (syntax or read error).
    pub key: Option<String>,
    pub kind: ConfigErrorKind,
    pub message: String,
}

/// What a [`ConfigError`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigErrorKind {
    /// The file cannot be read.
    Read,
    /// The file is not valid TOML.
    Syntax,
    /// A key the application does not know.
    UnknownKey,
    /// A known key with a rejected, clamped or otherwise unusable value.
    InvalidValue,
    /// The file cannot be written.
    Write,
    /// A section that now lives in another file.
    Moved,
}

impl ConfigError {
    /// An error reading `file`, tied to no line nor key.
    pub fn read(file: PathBuf, message: String) -> Self {
        Self {
            file,
            line: None,
            key: None,
            kind: ConfigErrorKind::Read,
            message,
        }
    }
}

/// Joins key segments into a dotted TOML path, quoting each segment that is not a bare
/// key (`keybindings."Ctrl+X"`).
pub fn key_path(segments: &[&str]) -> String {
    segments
        .iter()
        .map(|segment| {
            let bare = !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
            if bare {
                (*segment).to_owned()
            } else {
                let escaped = segment.replace('\\', "\\\\").replace('"', "\\\"");
                format!("\"{escaped}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.file.file_name().map_or_else(
            || self.file.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        match self.line {
            Some(line) => write!(f, "{name}:{line}: {}", self.message),
            None => write!(f, "{name}: {}", self.message),
        }
    }
}
