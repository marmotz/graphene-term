//! Files replaced atomically: written next to their target, then renamed over it, so
//! a reader (another instance, the configuration watcher) never sees a partial file.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Writes a complete replacement next to the target and atomically renames it into place.
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let metadata = fs::metadata(path).ok();
    let mut temp_path = None;
    let mut temp_file = None;

    for _ in 0..100 {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), suffix));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temp_path = Some(candidate);
                temp_file = Some(file);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    let temp_path = temp_path.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a temporary configuration file",
        )
    })?;
    let result = (|| {
        let mut temp_file = temp_file.expect("temporary path and file are set together");
        if let Some(metadata) = metadata {
            temp_file.set_permissions(metadata.permissions())?;
        }
        temp_file.write_all(contents.as_bytes())?;
        temp_file.sync_all()?;
        drop(temp_file);
        fs::rename(&temp_path, path)?;
        // Persist the rename where the platform supports syncing directories.
        if let Ok(directory) = OpenOptions::new().read(true).open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

/// Reads the JSON `file`. A missing file gives `None` silently; an unreadable or
/// corrupt one gives `None` and is logged.
pub fn read_json<T: DeserializeOwned>(file: &Path) -> Option<T> {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(error) => {
            log::warn!("cannot read {}: {error}", file.display());
            return None;
        }
    };
    serde_json::from_str(&text)
        .map_err(|error| log::warn!("ignoring corrupt {}: {error}", file.display()))
        .ok()
}

/// Writes `value` as JSON to `file`, creating its folder, with [`write_atomic`].
pub fn write_json<T: Serialize>(file: &Path, value: &T) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    write_atomic(file, &text)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "graphene-atomic-file-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn atomic_writer_replaces_the_file_contents() {
        let dir = temp_dir("replace");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        fs::write(&path, "old").unwrap();
        write_atomic(&path, "new\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn json_round_trips_through_a_new_folder() {
        let dir = temp_dir("json");
        let file = dir.join("nested").join("state.json");
        write_json(&file, &vec![1, 2, 3]).unwrap();
        assert_eq!(read_json::<Vec<u32>>(&file), Some(vec![1, 2, 3]));
        assert_eq!(fs::read_dir(file.parent().unwrap()).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_or_corrupt_json_is_none() {
        let dir = temp_dir("corrupt");
        let file = dir.join("state.json");
        assert_eq!(read_json::<Vec<u32>>(&file), None);
        fs::create_dir_all(&dir).unwrap();
        fs::write(&file, "{not json").unwrap();
        assert_eq!(read_json::<Vec<u32>>(&file), None);
        fs::remove_dir_all(dir).unwrap();
    }
}
