//! Hot reload: watches the configuration directory and reports changes to
//! `config.toml` and the user themes.
//!
//! The directory is watched rather than the file: editors save through a rename, which
//! replaces the watched inode. Bursts of events (an editor writes a temporary file,
//! renames it, changes its mode) are debounced into a single change.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use notify::event::{Event, EventKind};
use notify::{RecursiveMode, Watcher};

use crate::config::CONFIG_FILE;
use crate::theme::THEMES_DIR;

/// Quiet time after the last event before the change is reported.
pub const DEBOUNCE: Duration = Duration::from_millis(100);

/// Starts the watcher thread on `dir`, the configuration directory. `on_change` is
/// called once per burst of changes. A directory that does not exist, or a watcher
/// that cannot start, is logged and leaves hot reload off.
pub fn spawn(dir: PathBuf, on_change: impl Fn() + Send + 'static) {
    if !dir.is_dir() {
        log::warn!(
            "{} does not exist: configuration hot reload is off",
            dir.display()
        );
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("config-watcher".into())
        .spawn(move || {
            if let Err(error) = watch(&dir, on_change) {
                log::warn!(
                    "cannot watch {}: {error}: configuration hot reload is off",
                    dir.display()
                );
            }
        });
    if let Err(error) = spawned {
        log::warn!("cannot start the configuration watcher: {error}");
    }
}

/// Runs the watcher until its channel closes.
fn watch(dir: &Path, on_change: impl Fn()) -> notify::Result<()> {
    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(sender)?;
    watcher.watch(dir, RecursiveMode::NonRecursive)?;
    let themes = dir.join(THEMES_DIR);
    let mut themes_watched = false;
    let mut debounce = Debounce::new(DEBOUNCE);
    loop {
        // `themes/` may be created after startup: watch it as soon as it exists.
        if !themes_watched && themes.is_dir() {
            match watcher.watch(&themes, RecursiveMode::NonRecursive) {
                Ok(()) => themes_watched = true,
                Err(error) => log::warn!("cannot watch {}: {error}", themes.display()),
            }
        }
        let received = match debounce.wait(Instant::now()) {
            Some(timeout) => receiver.recv_timeout(timeout),
            None => receiver.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match received {
            Ok(Ok(event)) => {
                if is_relevant(&event, dir) {
                    debounce.touch(Instant::now());
                }
            }
            Ok(Err(error)) => log::warn!("configuration watcher: {error}"),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
        if debounce.fire(Instant::now()) {
            on_change();
        }
    }
}

/// Whether `event` may change the configuration in `dir`: a creation, a change or a
/// removal of `config.toml` or of a `.toml` file of `themes/`. Reads are not: the
/// reload reading the files would otherwise trigger the next one.
fn is_relevant(event: &Event, dir: &Path) -> bool {
    let changes = matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
    );
    changes && event.paths.iter().any(|path| watched_file(path, dir))
}

/// Whether `path` is `config.toml` or a theme file of `dir`.
fn watched_file(path: &Path, dir: &Path) -> bool {
    if path == dir.join(CONFIG_FILE) {
        return true;
    }
    path.parent() == Some(&dir.join(THEMES_DIR))
        && path
            .extension()
            .is_some_and(|extension| extension == "toml")
}

/// Merges a burst of events into one: fires `delay` after the last one.
#[derive(Debug)]
struct Debounce {
    delay: Duration,
    deadline: Option<Instant>,
}

impl Debounce {
    fn new(delay: Duration) -> Self {
        Self {
            delay,
            deadline: None,
        }
    }

    /// Records an event at `now`, pushing the deadline back.
    fn touch(&mut self, now: Instant) {
        self.deadline = Some(now + self.delay);
    }

    /// How long to wait for the next event before the deadline, `None` (forever)
    /// without pending event.
    fn wait(&self, now: Instant) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(now))
    }

    /// Whether the deadline has passed; clears it when it has.
    fn fire(&mut self, now: Instant) -> bool {
        match self.deadline {
            Some(deadline) if deadline <= now => {
                self.deadline = None;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use notify::event::{AccessKind, AccessMode, CreateKind, ModifyKind, RemoveKind, RenameMode};

    use super::*;

    const DIR: &str = "/home/user/.config/graphene";

    fn event(kind: EventKind, path: &str) -> Event {
        Event::new(kind).add_path(PathBuf::from(path))
    }

    #[test]
    fn config_and_theme_files_are_watched() {
        let dir = Path::new(DIR);
        assert!(watched_file(&dir.join("config.toml"), dir));
        assert!(watched_file(&dir.join("themes/mine.toml"), dir));
    }

    #[test]
    fn other_files_are_not_watched() {
        let dir = Path::new(DIR);
        for path in [
            "config.toml~",
            ".config.toml.swp",
            "4913",
            "themes",
            "themes/notes.txt",
            "themes/old/mine.toml",
            "other.toml",
        ] {
            assert!(!watched_file(&dir.join(path), dir), "{path}");
        }
        assert!(!watched_file(Path::new("/tmp/config.toml"), dir));
    }

    #[test]
    fn writes_renames_and_removals_are_relevant() {
        let dir = Path::new(DIR);
        let config = format!("{DIR}/config.toml");
        for kind in [
            EventKind::Create(CreateKind::File),
            EventKind::Modify(ModifyKind::Any),
            EventKind::Modify(ModifyKind::Name(RenameMode::To)),
            EventKind::Remove(RemoveKind::File),
        ] {
            assert!(is_relevant(&event(kind, &config), dir), "{kind:?}");
        }
    }

    #[test]
    fn a_rename_onto_the_config_file_is_relevant() {
        // `sed -i` and vim write a temporary file, then rename it over `config.toml`.
        let dir = Path::new(DIR);
        let rename = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(PathBuf::from(format!("{DIR}/sedAb12Cd")))
            .add_path(PathBuf::from(format!("{DIR}/config.toml")));
        assert!(is_relevant(&rename, dir));
    }

    #[test]
    fn reads_are_not_relevant() {
        let dir = Path::new(DIR);
        let config = format!("{DIR}/config.toml");
        for kind in [
            EventKind::Access(AccessKind::Open(AccessMode::Any)),
            EventKind::Access(AccessKind::Close(AccessMode::Read)),
        ] {
            assert!(!is_relevant(&event(kind, &config), dir), "{kind:?}");
        }
    }

    #[test]
    fn debounce_fires_once_after_the_last_event() {
        let start = Instant::now();
        let mut debounce = Debounce::new(DEBOUNCE);
        assert_eq!(debounce.wait(start), None);
        assert!(!debounce.fire(start));

        debounce.touch(start);
        debounce.touch(start + Duration::from_millis(60));
        assert_eq!(
            debounce.wait(start + Duration::from_millis(100)),
            Some(Duration::from_millis(60))
        );
        assert!(!debounce.fire(start + Duration::from_millis(150)));

        assert!(debounce.fire(start + Duration::from_millis(160)));
        assert!(!debounce.fire(start + Duration::from_millis(500)));
        assert_eq!(debounce.wait(start + Duration::from_millis(500)), None);
    }

    #[test]
    fn past_deadline_waits_zero() {
        let start = Instant::now();
        let mut debounce = Debounce::new(DEBOUNCE);
        debounce.touch(start);
        assert_eq!(
            debounce.wait(start + Duration::from_secs(1)),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn a_rename_over_the_config_file_is_reported_once_and_reads_are_not() {
        let dir =
            std::env::temp_dir().join(format!("graphene-watcher-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join(CONFIG_FILE);
        std::fs::write(&config, "").unwrap();

        let (sender, receiver) = mpsc::channel();
        spawn(dir.clone(), move || sender.send(()).unwrap());
        // Let the watcher start before touching the files.
        std::thread::sleep(Duration::from_millis(200));

        // What `sed -i` does: a temporary file renamed over the original.
        let temporary = dir.join("sedXyZ");
        std::fs::write(&temporary, "[font]\nsize = 12\n").unwrap();
        std::fs::rename(&temporary, &config).unwrap();
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            receiver.recv_timeout(DEBOUNCE * 3).is_err(),
            "one burst, one change"
        );

        std::fs::read_to_string(&config).unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        assert!(
            receiver.recv_timeout(DEBOUNCE * 3).is_err(),
            "reads and other files are ignored"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
