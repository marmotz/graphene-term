//! The saved session: what each window shows (tabs, split trees, current directories),
//! written to `<state_dir>/sessions/<instance>.json` while it runs, so that a later
//! launch can reopen it (`window.restore_session`).
//!
//! Each participating instance also holds an exclusive `flock` on
//! `sessions/<instance>.lock` for its whole life: an instance is alive while its lock
//! cannot be taken. The kernel releases it when the process dies, crash included.
//! Errors are only logged: the session is a convenience, never a reason to fail.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::window_state::WindowState;
use crate::workspace::tree::Axis;
use crate::{atomic_file, config};

/// Version of the format written; a file of another version is ignored and removed.
pub const SESSION_VERSION: u32 = 1;
/// Folder of the session files inside the state directory.
pub const SESSIONS_DIR: &str = "sessions";
/// Shortest time between two checks of the window for changes to save.
pub const SESSION_SAVE_INTERVAL: Duration = Duration::from_secs(2);
/// Longest time the file of a live instance goes without its modification date being
/// updated, so that the date tells when an instance that crashed died.
pub const HEARTBEAT: Duration = Duration::from_secs(10);

/// Kind of window a snapshot belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WindowKind {
    /// An ordinary window.
    #[default]
    Normal,
    /// The quick terminal of `graphene --quick`.
    Quick,
}

/// Everything saved about one window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowSnapshot {
    /// [`SESSION_VERSION`].
    pub version: u32,
    pub kind: WindowKind,
    pub window: WindowState,
    /// Outer position in physical pixels and name of the screen; `None` where the
    /// platform does not tell (Wayland).
    pub placement: Option<Placement>,
    /// Tabs in order. Empty when the window closed because its last shell exited:
    /// nothing is left to reopen.
    pub tabs: Vec<TabSnapshot>,
    /// Index of the shown tab in `tabs`.
    pub active_tab: usize,
    /// Unix seconds of the clean exit; `None` while the window runs, or after a crash.
    pub closed_at: Option<u64>,
}

/// Where a window was on screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub monitor: Option<String>,
}

/// A tab: its split tree and the pane with the focus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabSnapshot {
    /// Title given by the user; `None` for the automatic title.
    pub title: Option<String>,
    pub tree: NodeSnapshot,
    /// Rank of the active pane among the leaves of `tree`, depth first.
    pub active: usize,
    pub zoomed: bool,
}

/// A node of a split tree. Pane ids are not saved: they are allocated again when the
/// tree is rebuilt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeSnapshot {
    Pane {
        cwd: Option<PathBuf>,
    },
    Split {
        axis: Axis,
        ratio: f32,
        first: Box<NodeSnapshot>,
        second: Box<NodeSnapshot>,
    },
}

/// The leading field of a session file, read before the rest to tell a known format.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "read at launch by the restoration, issue #214")
)]
#[derive(Deserialize)]
struct Versioned {
    version: u32,
}

impl WindowSnapshot {
    /// Reads `file`. A missing file gives `None`; an unreadable, corrupt or unknown
    /// version one gives `None`, is logged and removed.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read at launch by the restoration, issue #214")
    )]
    pub fn load(file: &Path) -> Option<WindowSnapshot> {
        let text = match fs::read_to_string(file) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(error) => {
                log::warn!("cannot read {}: {error}", file.display());
                remove(file);
                return None;
            }
        };
        let snapshot = match serde_json::from_str::<Versioned>(&text) {
            Ok(Versioned {
                version: SESSION_VERSION,
            }) => serde_json::from_str(&text)
                .map_err(|error| log::warn!("ignoring corrupt {}: {error}", file.display()))
                .ok(),
            Ok(Versioned { version }) => {
                log::warn!(
                    "ignoring {}: unknown session version {version}",
                    file.display()
                );
                None
            }
            Err(error) => {
                log::warn!("ignoring corrupt {}: {error}", file.display());
                None
            }
        };
        if snapshot.is_none() {
            remove(file);
        }
        snapshot
    }
}

/// Removes `file`, logging a failure.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "read at launch by the restoration, issue #214")
)]
fn remove(file: &Path) {
    if let Err(error) = fs::remove_file(file)
        && error.kind() != io::ErrorKind::NotFound
    {
        log::warn!("cannot remove {}: {error}", file.display());
    }
}

/// `sessions` in the state directory ([`config::state_dir`]).
pub fn sessions_dir() -> Option<PathBuf> {
    config::state_dir().map(|dir| dir.join(SESSIONS_DIR))
}

/// Whether a launch takes part in the session (saves its window): only when
/// `restore_session` is on, for an ordinary window that neither runs a program (`-e`),
/// nor is sized in cells (`--columns`, `--lines`), nor was started with `--no-restore`.
pub fn participates(
    restore_session: bool,
    normal: bool,
    program: bool,
    sized_by_grid: bool,
    no_restore: bool,
) -> bool {
    restore_session && normal && !program && !sized_by_grid && !no_restore
}

/// Whether the instance whose lock file is `lock` is alive: its lock is held by an
/// open file. A missing lock file is a dead instance.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "read at launch by the restoration, issue #214")
)]
pub fn is_alive(lock: &Path) -> bool {
    if !lock.exists() {
        return false;
    }
    match crate::quick_terminal::control::try_lock(lock) {
        Ok(Some(_released_on_drop)) => false,
        Ok(None) => true,
        Err(error) => {
            log::warn!("cannot check {}: {error}", lock.display());
            false
        }
    }
}

/// Seconds since the Unix epoch of `time`, 0 before it.
pub fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// This process in the session folder: its name, and its lock, held as long as the
/// value lives.
#[derive(Debug)]
pub struct Instance {
    /// `<sessions>/<pid>-<start in ns>.json`.
    file: PathBuf,
    _lock: File,
}

impl Instance {
    /// Creates the folder `dir` when missing and takes the lock of a new instance in it,
    /// named after the process id and the current time, so that a reused pid never
    /// takes the name of a dead instance.
    pub fn claim(dir: &Path) -> io::Result<Instance> {
        fs::create_dir_all(dir)?;
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let name = format!("{}-{started}", std::process::id());
        let lock = crate::quick_terminal::control::try_lock(&dir.join(format!("{name}.lock")))?
            .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "instance lock is held"))?;
        Ok(Instance {
            file: dir.join(format!("{name}.json")),
            _lock: lock,
        })
    }

    /// The session file of this instance.
    pub fn file(&self) -> &Path {
        &self.file
    }
}

/// Saves the snapshot of a window while it runs: at most every
/// [`SESSION_SAVE_INTERVAL`], only when it changed, and touches the file at least every
/// [`HEARTBEAT`] otherwise.
#[derive(Debug)]
pub struct SessionSaver {
    instance: Instance,
    /// JSON last written, compared with the next snapshot.
    written: Option<String>,
    /// When the window is next checked for changes.
    next_check: Instant,
    /// When the file was last written or touched.
    touched: Instant,
}

impl SessionSaver {
    /// A saver for `instance`, whose first check is due at `now`.
    pub fn new(instance: Instance, now: Instant) -> Self {
        Self {
            instance,
            written: None,
            next_check: now,
            touched: now,
        }
    }

    /// The session file written.
    pub fn file(&self) -> &Path {
        self.instance.file()
    }

    /// Checks the window when due: writes `snapshot()` when it changed since the last
    /// write, else touches the file once [`HEARTBEAT`] passed. Returns when to call
    /// again.
    pub fn tick(&mut self, now: Instant, snapshot: impl FnOnce() -> WindowSnapshot) -> Instant {
        if now < self.next_check {
            return self.next_check;
        }
        self.next_check = now + SESSION_SAVE_INTERVAL;
        match serde_json::to_string_pretty(&snapshot()) {
            Ok(json) if self.written.as_ref() != Some(&json) => {
                self.write(json);
                self.touched = now;
            }
            Ok(_) if now.saturating_duration_since(self.touched) >= HEARTBEAT => {
                if let Err(error) = touch(self.file()) {
                    log::warn!("cannot touch {}: {error}", self.file().display());
                }
                self.touched = now;
            }
            Ok(_) => {}
            Err(error) => log::warn!("cannot serialize the session: {error}"),
        }
        self.next_check
    }

    /// Writes the final `snapshot`, at the clean exit.
    pub fn finish(&mut self, snapshot: &WindowSnapshot) {
        match serde_json::to_string_pretty(snapshot) {
            Ok(json) => self.write(json),
            Err(error) => log::warn!("cannot serialize the session: {error}"),
        }
    }

    fn write(&mut self, json: String) {
        match atomic_file::write_atomic(self.instance.file(), &json) {
            Ok(()) => self.written = Some(json),
            Err(error) => log::warn!("cannot write {}: {error}", self.file().display()),
        }
    }
}

/// Sets the modification date of `file` to now, without changing its contents.
fn touch(file: &Path) -> io::Result<()> {
    File::options()
        .write(true)
        .open(file)?
        .set_modified(SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("graphene-session-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn snapshot() -> WindowSnapshot {
        WindowSnapshot {
            version: SESSION_VERSION,
            kind: WindowKind::Normal,
            window: WindowState {
                width: 1000,
                height: 700,
                maximized: false,
            },
            placement: Some(Placement {
                x: 20,
                y: -5,
                monitor: Some("HDMI-1".to_owned()),
            }),
            tabs: vec![
                TabSnapshot {
                    title: Some("build".to_owned()),
                    tree: NodeSnapshot::Split {
                        axis: Axis::Vertical,
                        ratio: 0.25,
                        first: Box::new(NodeSnapshot::Pane {
                            cwd: Some(PathBuf::from("/tmp")),
                        }),
                        second: Box::new(NodeSnapshot::Pane { cwd: None }),
                    },
                    active: 1,
                    zoomed: true,
                },
                TabSnapshot {
                    title: None,
                    tree: NodeSnapshot::Pane {
                        cwd: Some(PathBuf::from("/")),
                    },
                    active: 0,
                    zoomed: false,
                },
            ],
            active_tab: 1,
            closed_at: Some(1_700_000_000),
        }
    }

    #[test]
    fn snapshot_round_trips_through_json() {
        let dir = TempDir::new("round-trip");
        let file = dir.0.join("1-2.json");
        atomic_file::write_json(&file, &snapshot()).unwrap();
        assert_eq!(WindowSnapshot::load(&file), Some(snapshot()));
        assert!(file.exists());
    }

    #[test]
    fn missing_file_is_no_snapshot() {
        let dir = TempDir::new("missing");
        assert_eq!(WindowSnapshot::load(&dir.0.join("none.json")), None);
    }

    #[test]
    fn unknown_version_is_ignored_and_removed() {
        let dir = TempDir::new("version");
        let file = dir.0.join("1-2.json");
        let mut newer = serde_json::to_value(snapshot()).unwrap();
        newer["version"] = 2.into();
        newer["tabs"] = "a format to come".into();
        atomic_file::write_json(&file, &newer).unwrap();
        assert_eq!(WindowSnapshot::load(&file), None);
        assert!(!file.exists());
    }

    #[test]
    fn corrupt_file_is_ignored_and_removed() {
        let dir = TempDir::new("corrupt");
        fs::create_dir_all(&dir.0).unwrap();
        for text in ["{ \"version\": ", "{ \"version\": 1, \"tabs\": 3 }", "[]"] {
            let file = dir.0.join("1-2.json");
            fs::write(&file, text).unwrap();
            assert_eq!(WindowSnapshot::load(&file), None, "{text}");
            assert!(!file.exists(), "{text}");
        }
    }

    #[test]
    fn only_ordinary_windows_launched_plainly_take_part() {
        assert!(participates(true, true, false, false, false));
        assert!(!participates(false, true, false, false, false));
        // The quick terminal, `-e`, `--columns`/`--lines`, `--no-restore`.
        assert!(!participates(true, false, false, false, false));
        assert!(!participates(true, true, true, false, false));
        assert!(!participates(true, true, false, true, false));
        assert!(!participates(true, true, false, false, true));
    }

    #[test]
    fn an_instance_is_alive_while_it_holds_its_lock() {
        let dir = TempDir::new("alive");
        let instance = Instance::claim(&dir.0).unwrap();
        let lock = instance.file().with_extension("lock");
        assert!(lock.exists());
        assert!(!instance.file().exists());
        assert!(is_alive(&lock));
        // Checking does not take the lock away from the instance.
        assert!(is_alive(&lock));
        drop(instance);
        assert!(!is_alive(&lock));
        assert!(!is_alive(&dir.0.join("never.lock")));
    }

    #[test]
    fn two_instances_get_distinct_files() {
        let dir = TempDir::new("distinct");
        let first = Instance::claim(&dir.0).unwrap();
        let second = Instance::claim(&dir.0).unwrap();
        assert_ne!(first.file(), second.file());
        let name = first.file().file_stem().unwrap().to_string_lossy();
        let (pid, _) = name.split_once('-').unwrap();
        assert_eq!(pid, std::process::id().to_string());
    }

    fn modified(file: &Path) -> SystemTime {
        fs::metadata(file).unwrap().modified().unwrap()
    }

    #[test]
    fn saver_writes_changes_at_most_every_interval() {
        let dir = TempDir::new("saver");
        let now = Instant::now();
        let mut saver = SessionSaver::new(Instance::claim(&dir.0).unwrap(), now);
        let mut current = snapshot();
        current.closed_at = None;

        assert_eq!(
            saver.tick(now, || current.clone()),
            now + SESSION_SAVE_INTERVAL
        );
        assert_eq!(WindowSnapshot::load(saver.file()), Some(current.clone()));

        // Not due yet: the window is not even looked at.
        current.active_tab = 0;
        let soon = now + SESSION_SAVE_INTERVAL / 2;
        let deadline = saver.tick(soon, || panic!("checked before the interval"));
        assert_eq!(deadline, now + SESSION_SAVE_INTERVAL);

        let later = now + SESSION_SAVE_INTERVAL;
        saver.tick(later, || current.clone());
        assert_eq!(WindowSnapshot::load(saver.file()).unwrap().active_tab, 0);
    }

    #[test]
    fn saver_touches_an_unchanged_file_after_the_heartbeat() {
        let dir = TempDir::new("heartbeat");
        let now = Instant::now();
        let mut saver = SessionSaver::new(Instance::claim(&dir.0).unwrap(), now);
        saver.tick(now, snapshot);
        let old = SystemTime::now() - Duration::from_secs(3600);
        File::options()
            .write(true)
            .open(saver.file())
            .unwrap()
            .set_modified(old)
            .unwrap();

        // Unchanged, before the heartbeat: left alone.
        saver.tick(now + SESSION_SAVE_INTERVAL, snapshot);
        assert_eq!(modified(saver.file()), old);

        saver.tick(now + HEARTBEAT, snapshot);
        assert!(modified(saver.file()) > old);
        assert_eq!(WindowSnapshot::load(saver.file()), Some(snapshot()));
    }

    #[test]
    fn finish_writes_the_final_snapshot() {
        let dir = TempDir::new("finish");
        let now = Instant::now();
        let mut saver = SessionSaver::new(Instance::claim(&dir.0).unwrap(), now);
        let mut running = snapshot();
        running.closed_at = None;
        saver.tick(now, || running.clone());
        saver.finish(&snapshot());
        assert_eq!(
            WindowSnapshot::load(saver.file()).unwrap().closed_at,
            Some(1_700_000_000)
        );
    }

    #[test]
    fn unix_seconds_counts_from_the_epoch() {
        assert_eq!(unix_seconds(UNIX_EPOCH + Duration::from_secs(42)), 42);
        assert_eq!(unix_seconds(UNIX_EPOCH - Duration::from_secs(1)), 0);
    }
}
