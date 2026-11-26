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
/// Longest time between the end of a window and the end of the last one for both to
/// be reopened: windows closed together (logout, shutdown) come back, a window closed
/// while others stayed open does not.
pub const GRACE: Duration = Duration::from_secs(15);
/// Lock held while a launch picks the snapshot it reopens, so that two launches at
/// once never reopen the same window.
pub const RESTORE_LOCK: &str = ".restore.lock";
/// Prefix of the snapshots kept for another process to reopen; never picked here.
const PENDING_PREFIX: &str = "pending-";

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
#[derive(Deserialize)]
struct Versioned {
    version: u32,
}

impl WindowSnapshot {
    /// Reads `file`. A missing file gives `None`; an unreadable, corrupt or unknown
    /// version one gives `None`, is logged and removed.
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

/// The `kind` field of a session file, read without the rest.
#[derive(Deserialize)]
struct Kinded {
    #[serde(default)]
    kind: WindowKind,
}

/// Kind of the live instance whose session file is `file`: [`WindowKind::Normal`] when
/// the file is missing or unreadable, as at the very start of an instance, before its
/// first write. Never removes the file: it belongs to a running instance.
fn live_kind(file: &Path) -> WindowKind {
    fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str::<Kinded>(&text).ok())
        .map_or(WindowKind::Normal, |kinded| kinded.kind)
}

/// When the window of `snapshot`, saved in a file last modified at `modified`, ended:
/// its clean exit, else the last write or heartbeat (a crash).
fn ended_at(snapshot: &WindowSnapshot, modified: SystemTime) -> SystemTime {
    snapshot.closed_at.map_or(modified, |seconds| {
        UNIX_EPOCH + Duration::from_secs(seconds)
    })
}

/// Splits `candidates`, each with its end, into the ones ended less than [`GRACE`]
/// before the last end, latest first, and the others.
fn select<T>(mut candidates: Vec<(T, SystemTime)>) -> (Vec<T>, Vec<T>) {
    candidates.sort_by_key(|&(_, end)| std::cmp::Reverse(end));
    let Some(&(_, latest)) = candidates.first() else {
        return (Vec::new(), Vec::new());
    };
    let (kept, dropped): (Vec<_>, Vec<_>) = candidates
        .into_iter()
        .partition(|(_, end)| latest.duration_since(*end).unwrap_or_default() < GRACE);
    (
        kept.into_iter().map(|(item, _)| item).collect(),
        dropped.into_iter().map(|(item, _)| item).collect(),
    )
}

/// Takes the place of this process in the session folder `dir`, as a window of
/// `kind`, and picks the snapshot it reopens:
///
/// - none while an instance of the same kind runs: the window opens fresh;
/// - else the latest of the snapshots of `kind` ended within [`GRACE`] of each other,
///   renamed to the file of this instance. The older ones are removed. Each other kept
///   one is renamed `pending-<n>.json` and handed to `reopen`, which starts the process
///   that reopens it (see [`adopt`]); a file `reopen` fails on is removed. Unreadable
///   files are removed.
///
/// There is a single quick terminal: for [`WindowKind::Quick`], the other kept
/// snapshots are removed, never handed to `reopen`. Snapshots of another kind are
/// always left alone.
///
/// Holds [`RESTORE_LOCK`] meanwhile, until the instance is claimed and the other
/// windows are handed over: a launch that follows sees it alive.
pub fn start(
    dir: &Path,
    kind: WindowKind,
    mut reopen: impl FnMut(&Path) -> io::Result<()>,
) -> io::Result<(Instance, Option<WindowSnapshot>)> {
    fs::create_dir_all(dir)?;
    let _restoring = lock(&dir.join(RESTORE_LOCK))?;
    let mut live = Vec::new();
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name == RESTORE_LOCK || name.starts_with(PENDING_PREFIX) {
            continue;
        }
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("lock") if is_alive(&path) => live.push(live_kind(&path.with_extension("json"))),
            // The lock of an instance that died: its snapshot, if any, is read below.
            Some("lock") => remove(&path),
            Some("json") => files.push(path),
            _ => {}
        }
    }
    if live.contains(&kind) {
        // An instance of the same kind runs: this window opens fresh and leaves the
        // snapshots alone.
        files.clear();
    }
    let mut candidates = Vec::new();
    for file in files {
        if is_alive(&file.with_extension("lock")) {
            continue;
        }
        let Ok(modified) = fs::metadata(&file).and_then(|metadata| metadata.modified()) else {
            continue;
        };
        let Some(snapshot) = WindowSnapshot::load(&file) else {
            continue;
        };
        if snapshot.kind == kind {
            let end = ended_at(&snapshot, modified);
            candidates.push(((file, snapshot), end));
        }
    }
    let (kept, dropped) = select(candidates);
    for (file, _) in &dropped {
        remove(file);
    }
    let instance = Instance::claim(dir)?;
    let mut kept = kept.into_iter();
    let restored = match kept.next() {
        Some((file, snapshot)) => match fs::rename(&file, instance.file()) {
            Ok(()) => Some(snapshot),
            Err(error) => {
                log::warn!("cannot reopen {}: {error}", file.display());
                None
            }
        },
        None => None,
    };
    if kind == WindowKind::Quick {
        for (file, _) in kept {
            remove(&file);
        }
        return Ok((instance, restored));
    }
    for file in park(dir, kept.map(|(file, _)| file)) {
        if let Err(error) = reopen(&file) {
            log::warn!("cannot reopen {}: {error}", file.display());
            remove(&file);
        }
    }
    Ok((instance, restored))
}

/// Renames each of `files` to a free `pending-<n>.json` of `dir`, for another process
/// to reopen, and returns the new names. A file that cannot be renamed is left alone.
/// Called under [`RESTORE_LOCK`], so that no other launch takes a name meanwhile.
fn park(dir: &Path, files: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut parked = Vec::new();
    let mut next = 0_usize;
    for file in files {
        let pending = loop {
            let pending = dir.join(format!("{PENDING_PREFIX}{next}.json"));
            next += 1;
            if !pending.exists() {
                break pending;
            }
        };
        match fs::rename(&file, &pending) {
            Ok(()) => parked.push(pending),
            Err(error) => log::warn!("cannot keep {} to reopen: {error}", file.display()),
        }
    }
    parked
}

/// Takes the place of this process in the session folder `dir` and reopens `file`, a
/// window handed over by [`start`] (`--restore-session`): renamed to the file of this
/// instance, without looking for live instances nor sorting snapshots. `None`, a fresh
/// window, when `file` is missing, unreadable or not a `pending-<n>.json` of `dir`.
///
/// Holds [`RESTORE_LOCK`] meanwhile, as [`start`] does.
pub fn adopt(dir: &Path, file: &Path) -> io::Result<(Instance, Option<WindowSnapshot>)> {
    fs::create_dir_all(dir)?;
    let _restoring = lock(&dir.join(RESTORE_LOCK))?;
    let instance = Instance::claim(dir)?;
    if !is_pending(dir, file) {
        log::warn!("{} is not a window to reopen", file.display());
        return Ok((instance, None));
    }
    let restored = match fs::rename(file, instance.file()) {
        Ok(()) => WindowSnapshot::load(instance.file()),
        Err(error) => {
            log::warn!("cannot reopen {}: {error}", file.display());
            None
        }
    };
    Ok((instance, restored))
}

/// Whether `file` is a `pending-<n>.json` of `dir`: [`adopt`] never takes another file.
fn is_pending(dir: &Path, file: &Path) -> bool {
    file.parent() == Some(dir)
        && file
            .extension()
            .is_some_and(|extension| extension == "json")
        && file
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(PENDING_PREFIX))
}

/// Opens `path` and waits for an exclusive `flock` on it, held as long as the file is
/// open.
fn lock(path: &Path) -> io::Result<File> {
    use std::os::fd::AsRawFd;
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    loop {
        // SAFETY: the descriptor belongs to `file`, open for the whole call.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
            return Ok(file);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// The directory a restored shell starts in: `path` when it still exists, else its
/// nearest parent that does, else `home`.
pub fn nearest_existing_dir(path: &Path, home: Option<PathBuf>) -> Option<PathBuf> {
    path.ancestors()
        .find(|dir| !dir.as_os_str().is_empty() && dir.is_dir())
        .map(Path::to_path_buf)
        .or(home)
}

/// Where a restored window is put: at its saved place, only when the screen it was on
/// is among `monitors` (the names of the screens plugged in). Elsewhere, or without a
/// saved place (Wayland), the window manager places it.
pub fn restored_position<'a>(
    placement: Option<&Placement>,
    mut monitors: impl Iterator<Item = &'a str>,
) -> Option<(i32, i32)> {
    let placement = placement?;
    let monitor = placement.monitor.as_deref()?;
    monitors
        .any(|name| name == monitor)
        .then_some((placement.x, placement.y))
}

/// Removes `file`, logging a failure.
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
/// `restore_session` is on, for a window, ordinary or the quick terminal, that neither
/// runs a program (`-e`), nor is sized in cells (`--columns`, `--lines`), nor was
/// started with `--no-restore`.
pub fn participates(
    restore_session: bool,
    program: bool,
    sized_by_grid: bool,
    no_restore: bool,
) -> bool {
    restore_session && !program && !sized_by_grid && !no_restore
}

/// Whether the instance whose lock file is `lock` is alive: its lock is held by an
/// open file. A missing lock file is a dead instance.
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
        self.save(snapshot);
    }

    /// Writes `snapshot` now, without waiting for the next check: the quick terminal
    /// starting over with a fresh tab.
    pub fn save(&mut self, snapshot: &WindowSnapshot) {
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
    fn only_windows_launched_plainly_take_part() {
        // Ordinary windows and the quick terminal alike.
        assert!(participates(true, false, false, false));
        assert!(!participates(false, false, false, false));
        // `-e`, `--columns`/`--lines`, `--no-restore`.
        assert!(!participates(true, true, false, false));
        assert!(!participates(true, false, true, false));
        assert!(!participates(true, false, false, true));
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
    fn save_writes_at_once_between_two_checks() {
        let dir = TempDir::new("save");
        let now = Instant::now();
        let mut saver = SessionSaver::new(Instance::claim(&dir.0).unwrap(), now);
        saver.tick(now, snapshot);
        let mut reset = snapshot();
        reset.tabs.truncate(1);
        reset.active_tab = 0;
        saver.save(&reset);
        assert_eq!(WindowSnapshot::load(saver.file()), Some(reset.clone()));
        // The next check sees it written: nothing new to write.
        saver.tick(now + SESSION_SAVE_INTERVAL, || reset.clone());
        assert_eq!(WindowSnapshot::load(saver.file()), Some(reset));
    }

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn select_keeps_the_ends_within_grace_of_the_last_latest_first() {
        let (kept, dropped) = select(vec![
            ("early", at(1_000)),
            ("last", at(2_000)),
            ("close", at(2_000 - 14)),
            ("edge", at(2_000 - 15)),
        ]);
        assert_eq!(kept, vec!["last", "close"]);
        assert_eq!(dropped, vec!["edge", "early"]);
        assert_eq!(select::<()>(Vec::new()), (Vec::new(), Vec::new()));
    }

    #[test]
    fn a_window_ends_at_its_clean_exit_else_at_its_last_write() {
        let mut saved = snapshot();
        assert_eq!(ended_at(&saved, at(5)), at(1_700_000_000));
        saved.closed_at = None;
        assert_eq!(ended_at(&saved, at(5)), at(5));
    }

    /// Writes in `dir` the snapshot `name` of `kind`, closed at `closed_at` or, without
    /// it, last modified at `modified`.
    fn saved(
        dir: &Path,
        name: &str,
        kind: WindowKind,
        closed_at: Option<u64>,
        modified: SystemTime,
    ) -> PathBuf {
        let file = dir.join(format!("{name}.json"));
        let mut snapshot = snapshot();
        snapshot.kind = kind;
        snapshot.closed_at = closed_at;
        snapshot.active_tab = name.len();
        atomic_file::write_json(&file, &snapshot).unwrap();
        File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        file
    }

    fn now_seconds() -> u64 {
        unix_seconds(SystemTime::now())
    }

    /// `reopen` of a launch that has no other window to hand over.
    fn no_reopen(file: &Path) -> io::Result<()> {
        panic!("no window to hand over, got {}", file.display())
    }

    #[test]
    fn start_reopens_the_last_window_and_drops_the_ones_closed_long_before() {
        let dir = TempDir::new("start");
        fs::create_dir_all(&dir.0).unwrap();
        let now = now_seconds();
        let old = SystemTime::now() - Duration::from_secs(3600);
        let last = saved(&dir.0, "1-1", WindowKind::Normal, Some(now), old);
        let close = saved(&dir.0, "2-22", WindowKind::Normal, Some(now - 5), old);
        let early = saved(&dir.0, "3-333", WindowKind::Normal, Some(now - 60), old);
        fs::write(dir.0.join("4-4.json"), "{ corrupt").unwrap();
        // The lock of an instance that died.
        fs::write(dir.0.join("1-1.lock"), "").unwrap();

        let mut handed = Vec::new();
        let (instance, restored) = start(&dir.0, WindowKind::Normal, |file| {
            handed.push(file.to_path_buf());
            Ok(())
        })
        .unwrap();
        let restored = restored.expect("a window to reopen");
        assert_eq!(restored.active_tab, 3);
        // Adopted: the file now belongs to this instance.
        assert!(!last.exists());
        assert_eq!(WindowSnapshot::load(instance.file()), Some(restored));
        // Kept for another window, handed over under a pending name; the others removed.
        let pending = dir.0.join("pending-0.json");
        assert_eq!(handed, std::slice::from_ref(&pending));
        assert!(!close.exists());
        assert_eq!(WindowSnapshot::load(&pending).unwrap().active_tab, 4);
        assert!(!early.exists());
        assert!(!dir.0.join("4-4.json").exists());
        assert!(!dir.0.join("1-1.lock").exists());
        assert!(dir.0.join(RESTORE_LOCK).exists());
    }

    #[test]
    fn kept_windows_go_to_free_pending_names_and_a_failed_hand_over_removes_them() {
        let dir = TempDir::new("park");
        fs::create_dir_all(&dir.0).unwrap();
        let now = now_seconds();
        let old = SystemTime::now() - Duration::from_secs(3600);
        saved(&dir.0, "1-1", WindowKind::Normal, Some(now), old);
        saved(&dir.0, "2-22", WindowKind::Normal, Some(now - 1), old);
        saved(&dir.0, "3-333", WindowKind::Normal, Some(now - 2), old);
        // Left by a launch whose window has not started yet: neither picked nor reused.
        fs::write(dir.0.join("pending-0.json"), "waiting").unwrap();

        let mut handed = Vec::new();
        let (_instance, restored) = start(&dir.0, WindowKind::Normal, |file| {
            handed.push(file.to_path_buf());
            if handed.len() == 2 {
                return Err(io::Error::other("cannot start"));
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(restored.unwrap().active_tab, 3);
        let first = dir.0.join("pending-1.json");
        let second = dir.0.join("pending-2.json");
        assert_eq!(handed, [first.clone(), second.clone()]);
        assert!(first.exists());
        assert!(!second.exists());
        assert_eq!(
            fs::read_to_string(dir.0.join("pending-0.json")).unwrap(),
            "waiting"
        );
    }

    #[test]
    fn a_handed_over_window_is_adopted_without_any_selection() {
        let dir = TempDir::new("adopt");
        fs::create_dir_all(&dir.0).unwrap();
        // A live normal window and an old end would both stop `start`.
        let _live = Instance::claim(&dir.0).unwrap();
        let old = SystemTime::now() - Duration::from_secs(3600);
        saved(&dir.0, "pending-3", WindowKind::Normal, Some(0), old);
        let pending = dir.0.join("pending-3.json");

        let (instance, restored) = adopt(&dir.0, &pending).unwrap();
        let restored = restored.expect("the window handed over");
        assert_eq!(restored.active_tab, "pending-3".len());
        assert!(!pending.exists());
        assert_eq!(WindowSnapshot::load(instance.file()), Some(restored));
    }

    #[test]
    fn adopting_a_missing_or_foreign_file_opens_a_fresh_window() {
        let dir = TempDir::new("adopt-missing");
        fs::create_dir_all(&dir.0).unwrap();
        let (instance, restored) = adopt(&dir.0, &dir.0.join("pending-0.json")).unwrap();
        assert_eq!(restored, None);
        assert!(!instance.file().exists());

        fs::write(dir.0.join("pending-1.json"), "{ corrupt").unwrap();
        let (instance, restored) = adopt(&dir.0, &dir.0.join("pending-1.json")).unwrap();
        assert_eq!(restored, None);
        assert!(!instance.file().exists());

        // Only a pending file of the folder: never another file of the user.
        let foreign = saved(&dir.0, "1-1", WindowKind::Normal, None, SystemTime::now());
        let (_instance, restored) = adopt(&dir.0, &foreign).unwrap();
        assert_eq!(restored, None);
        assert!(foreign.exists());
    }

    #[test]
    fn a_crashed_window_ends_at_its_last_write() {
        let dir = TempDir::new("crash");
        fs::create_dir_all(&dir.0).unwrap();
        let now = now_seconds();
        let closed = saved(
            &dir.0,
            "1-1",
            WindowKind::Normal,
            Some(now - 60),
            SystemTime::now(),
        );
        saved(&dir.0, "2-22", WindowKind::Normal, None, SystemTime::now());

        let (_instance, restored) = start(&dir.0, WindowKind::Normal, no_reopen).unwrap();
        assert_eq!(restored.unwrap().active_tab, 4);
        assert!(!closed.exists());
    }

    #[test]
    fn a_launch_only_reopens_a_window_of_its_kind() {
        let dir = TempDir::new("kind");
        fs::create_dir_all(&dir.0).unwrap();
        let quick = saved(
            &dir.0,
            "1-1",
            WindowKind::Quick,
            Some(now_seconds()),
            SystemTime::now(),
        );
        saved(
            &dir.0,
            "2-22",
            WindowKind::Normal,
            Some(now_seconds() - 60),
            SystemTime::now(),
        );

        let (_instance, restored) = start(&dir.0, WindowKind::Normal, no_reopen).unwrap();
        // The quick terminal, closed later, neither wins nor drops the normal window.
        assert_eq!(restored.unwrap().active_tab, 4);
        assert!(quick.exists());

        let (_instance, restored) = start(&dir.0, WindowKind::Quick, no_reopen).unwrap();
        assert_eq!(restored.unwrap().kind, WindowKind::Quick);
    }

    #[test]
    fn the_quick_terminal_never_takes_nor_starts_an_ordinary_window() {
        let dir = TempDir::new("quick-only");
        fs::create_dir_all(&dir.0).unwrap();
        let now = now_seconds();
        let normal = saved(
            &dir.0,
            "1-1",
            WindowKind::Normal,
            Some(now),
            SystemTime::now(),
        );
        let other = saved(
            &dir.0,
            "2-2",
            WindowKind::Normal,
            Some(now),
            SystemTime::now(),
        );

        let (instance, restored) = start(&dir.0, WindowKind::Quick, no_reopen).unwrap();
        assert_eq!(restored, None);
        assert!(normal.exists());
        assert!(other.exists());
        assert!(!instance.file().exists());
    }

    #[test]
    fn the_quick_terminal_reopens_its_last_snapshot_and_drops_the_others() {
        let dir = TempDir::new("quick-last");
        fs::create_dir_all(&dir.0).unwrap();
        let now = now_seconds();
        let old = SystemTime::now() - Duration::from_secs(3600);
        let last = saved(&dir.0, "1-1", WindowKind::Quick, Some(now), old);
        let close = saved(&dir.0, "2-22", WindowKind::Quick, Some(now - 1), old);
        let normal = saved(&dir.0, "3-333", WindowKind::Normal, Some(now), old);

        let (instance, restored) = start(&dir.0, WindowKind::Quick, no_reopen).unwrap();
        let restored = restored.expect("the quick terminal to reopen");
        assert_eq!((restored.kind, restored.active_tab), (WindowKind::Quick, 3));
        assert_eq!(WindowSnapshot::load(instance.file()), Some(restored));
        assert!(!last.exists());
        // A single quick terminal: the other one is neither kept nor handed over.
        assert!(!close.exists());
        assert!(!dir.0.join("pending-0.json").exists());
        assert!(normal.exists());
    }

    #[test]
    fn a_live_instance_of_the_same_kind_stops_the_restoration() {
        let dir = TempDir::new("live");
        fs::create_dir_all(&dir.0).unwrap();
        let file = saved(
            &dir.0,
            "1-1",
            WindowKind::Normal,
            Some(now_seconds()),
            SystemTime::now(),
        );

        // A live quick terminal does not stop it.
        let quick = Instance::claim(&dir.0).unwrap();
        let mut running = snapshot();
        running.kind = WindowKind::Quick;
        atomic_file::write_json(quick.file(), &running).unwrap();
        // Restored, then put back for the next launches.
        let (first, restored) = start(&dir.0, WindowKind::Normal, no_reopen).unwrap();
        assert!(restored.is_some());
        fs::rename(first.file(), &file).unwrap();

        // A live normal window does, and leaves the snapshot alone.
        let (second, restored) = start(&dir.0, WindowKind::Normal, no_reopen).unwrap();
        assert_eq!(restored, None);
        assert!(file.exists());
        assert!(!second.file().exists());

        // A live instance without a file yet counts as a normal window.
        drop((first, second));
        let _starting = Instance::claim(&dir.0).unwrap();
        let (_third, restored) = start(&dir.0, WindowKind::Normal, no_reopen).unwrap();
        assert_eq!(restored, None);
        assert!(file.exists());
        drop(quick);
    }

    #[test]
    fn nearest_existing_dir_climbs_to_a_parent_still_there() {
        let dir = TempDir::new("nearest");
        let inner = dir.0.join("a");
        fs::create_dir_all(&inner).unwrap();
        let home = Some(PathBuf::from("/home/someone"));
        assert_eq!(
            nearest_existing_dir(&inner, home.clone()),
            Some(inner.clone())
        );
        assert_eq!(
            nearest_existing_dir(&inner.join("gone/deeper"), home.clone()),
            Some(inner)
        );
        assert_eq!(
            nearest_existing_dir(Path::new("gone/relative"), home.clone()),
            home
        );
    }

    #[test]
    fn a_window_goes_back_to_its_place_only_on_a_screen_still_there() {
        let placement = Placement {
            x: 20,
            y: -5,
            monitor: Some("HDMI-1".to_owned()),
        };
        let screens = || ["eDP-1", "HDMI-1"].into_iter();
        assert_eq!(
            restored_position(Some(&placement), screens()),
            Some((20, -5))
        );
        assert_eq!(
            restored_position(Some(&placement), ["eDP-1"].into_iter()),
            None
        );
        assert_eq!(restored_position(None, screens()), None);
        let unnamed = Placement {
            monitor: None,
            ..placement
        };
        assert_eq!(restored_position(Some(&unnamed), screens()), None);
    }

    #[test]
    fn unix_seconds_counts_from_the_epoch() {
        assert_eq!(unix_seconds(UNIX_EPOCH + Duration::from_secs(42)), 42);
        assert_eq!(unix_seconds(UNIX_EPOCH - Duration::from_secs(1)), 0);
    }
}
