//! Processes of the quick terminal: the single resident instance, its command socket,
//! and the `graphene --quick-toggle` client that talks to it or starts it.
//!
//! Both files live in the runtime folder (`config::runtime_dir`), whose `0700` mode is
//! the only protection of the socket:
//!
//! - `quick.lock`, locked with `flock` by the resident process for its whole life, so
//!   that two simultaneous starts never open two quick terminals;
//! - `quick.sock`, where each connection sends one command line ([`QuickCommand`]).

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::QuickCommand;

/// Exit code of a `--quick` process that cannot run because the session is Wayland.
pub const WAYLAND_EXIT_CODE: i32 = 3;
/// Time `--quick-toggle` waits for a new quick terminal process to answer, and a second
/// `--quick` process for the first to listen.
const STARTUP_WAIT: Duration = Duration::from_secs(5);
/// Interval between two checks of a process starting.
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// Time the socket thread waits for the command line of a connection.
const READ_TIMEOUT: Duration = Duration::from_secs(1);

/// Lock file of the resident process in the runtime folder `dir`.
pub fn lock_path(dir: &Path) -> PathBuf {
    dir.join("quick.lock")
}

/// Command socket of the resident process in the runtime folder `dir`.
pub fn socket_path(dir: &Path) -> PathBuf {
    dir.join("quick.sock")
}

/// Takes the single instance lock of the runtime folder `dir`. `None` when another
/// process holds it: that process is asked to toggle its window, unless `hidden`, and
/// this one must exit. The lock lasts as long as the returned file is open.
pub fn claim(dir: &Path, hidden: bool) -> io::Result<Option<File>> {
    if let Some(lock) = try_lock(&lock_path(dir))? {
        return Ok(Some(lock));
    }
    if !hidden {
        send_retrying(&socket_path(dir), QuickCommand::Toggle, STARTUP_WAIT)?;
    }
    Ok(None)
}

/// Opens and locks `path` exclusively, `None` when another open file holds the lock.
/// The lock lasts as long as the returned file is open.
pub(crate) fn try_lock(path: &Path) -> io::Result<Option<File>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    // SAFETY: the descriptor belongs to `file`, open for the whole call.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(Some(file));
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::EWOULDBLOCK) => Ok(None),
        _ => Err(error),
    }
}

/// Listens on the socket of the runtime folder `dir`. Only the lock holder calls it,
/// so a socket file left there belongs to a process that died and is replaced.
pub fn bind(dir: &Path) -> io::Result<UnixListener> {
    let socket = socket_path(dir);
    match std::fs::remove_file(&socket) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    UnixListener::bind(socket)
}

/// Reads the commands of `listener` in a thread of its own and hands each to
/// `on_command`, until it returns `false` (the event loop is closed). A connection
/// sends one line; an unknown line is logged and dropped.
pub fn serve(
    listener: UnixListener,
    mut on_command: impl FnMut(QuickCommand) -> bool + Send + 'static,
) -> io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("quick-terminal-socket".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let stream = match stream {
                    Ok(stream) => stream,
                    Err(error) => {
                        log::warn!("quick terminal socket: {error}");
                        continue;
                    }
                };
                match read_command(stream) {
                    Ok(Some(command)) => {
                        if !on_command(command) {
                            return;
                        }
                    }
                    Ok(None) => {}
                    Err(error) => log::debug!("quick terminal socket: {error}"),
                }
            }
        })
}

/// The result of setting the read timeout of a connection. macOS refuses it with
/// `EINVAL` once the peer has closed its end, which [`send`] does right after writing:
/// the line is then already there and reading it cannot block.
fn timeout_set(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.raw_os_error() == Some(libc::EINVAL) => Ok(()),
        result => result,
    }
}

/// The command line of one connection; `None` for an empty or unknown one.
fn read_command(stream: UnixStream) -> io::Result<Option<QuickCommand>> {
    timeout_set(stream.set_read_timeout(Some(READ_TIMEOUT)))?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    let command = QuickCommand::parse(&line);
    if command.is_none() && !line.trim().is_empty() {
        log::warn!("quick terminal socket: unknown command {:?}", line.trim());
    }
    Ok(command)
}

/// Sends `command` to the process listening on `socket`.
pub fn send(socket: &Path, command: QuickCommand) -> io::Result<()> {
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(format!("{}\n", command.as_str()).as_bytes())
}

/// Sends `command`, retrying for `wait` while the process holding the lock does not
/// listen yet.
fn send_retrying(socket: &Path, command: QuickCommand, wait: Duration) -> io::Result<()> {
    let deadline = Instant::now() + wait;
    loop {
        match send(socket, command) {
            Ok(()) => return Ok(()),
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => std::thread::sleep(POLL_INTERVAL),
        }
    }
}

/// Why `graphene --quick-toggle` failed.
#[derive(Debug, PartialEq, Eq)]
pub enum ToggleError {
    /// The session is Wayland, where the quick terminal is unavailable.
    Wayland,
    /// Any other failure, described.
    Failed(String),
}

impl fmt::Display for ToggleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wayland => f.write_str("the quick terminal is not available under Wayland"),
            Self::Failed(reason) => write!(f, "cannot toggle the quick terminal: {reason}"),
        }
    }
}

/// `graphene --quick-toggle`: toggles the resident quick terminal of the runtime folder
/// `dir`, or starts it, shown, when there is none.
pub fn toggle(dir: &Path) -> Result<(), ToggleError> {
    toggle_with(&socket_path(dir), || spawn_quick(false), STARTUP_WAIT)
}

/// Starts the quick terminal of the runtime folder `dir` hidden, for its global hotkey,
/// unless one already listens. Returns whether it started one; the call waits for that
/// process to end, so it runs in a thread of its own.
pub fn start_hidden(dir: &Path) -> io::Result<bool> {
    start_hidden_with(&socket_path(dir), || spawn_quick(true))
}

/// Starts `spawn` unless a process listens on `socket`, and waits for it to end: a
/// second process exits at once, the resident one at the end of the session.
fn start_hidden_with(socket: &Path, spawn: impl FnOnce() -> io::Result<Child>) -> io::Result<bool> {
    // A connection without a command line is dropped by the socket thread.
    if UnixStream::connect(socket).is_ok() {
        return Ok(false);
    }
    let mut child = spawn()?;
    child.wait()?;
    Ok(true)
}

/// Sends `toggle` on `socket`; when nothing listens, starts the quick terminal with
/// `spawn` and waits up to `wait` for it to listen or exit.
fn toggle_with(
    socket: &Path,
    spawn: impl FnOnce() -> io::Result<Child>,
    wait: Duration,
) -> Result<(), ToggleError> {
    if send(socket, QuickCommand::Toggle).is_ok() {
        return Ok(());
    }
    let mut child = spawn().map_err(|error| ToggleError::Failed(error.to_string()))?;
    let deadline = Instant::now() + wait;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // A second process that found the lock taken exits with 0 once it
                // sent `toggle` to the first.
                return match status.code() {
                    Some(0) => Ok(()),
                    Some(WAYLAND_EXIT_CODE) => Err(ToggleError::Wayland),
                    _ => Err(ToggleError::Failed(format!(
                        "the quick terminal process stopped ({status})"
                    ))),
                };
            }
            Ok(None) => {}
            Err(error) => return Err(ToggleError::Failed(error.to_string())),
        }
        // The new process starts shown: it only has to listen.
        if UnixStream::connect(socket).is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(ToggleError::Failed(format!(
                "the quick terminal did not start within {} s",
                wait.as_secs()
            )));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Starts `graphene --quick`, with `--hidden` when `hidden`, in a session of its own,
/// detached from the terminal of the caller, so that it outlives it.
fn spawn_quick(hidden: bool) -> io::Result<Child> {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new(std::env::current_exe()?);
    command.arg("--quick");
    if hidden {
        command.arg("--hidden");
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: `setsid` is async-signal-safe, as `pre_exec` requires.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// A fresh runtime folder for one test.
    fn runtime(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("graphene-quick-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Serves the socket of `dir` and returns the commands it receives.
    fn server(dir: &Path) -> mpsc::Receiver<QuickCommand> {
        let (sender, receiver) = mpsc::channel();
        serve(bind(dir).unwrap(), move |command| {
            sender.send(command).is_ok()
        })
        .unwrap();
        receiver
    }

    /// Leaves a file nothing listens on at the socket path of `dir`.
    fn leave_dead_socket(dir: &Path) {
        std::fs::write(socket_path(dir), b"").unwrap();
        assert!(UnixStream::connect(socket_path(dir)).is_err());
    }

    fn shell(script: &str) -> io::Result<Child> {
        Command::new("sh").args(["-c", script]).spawn()
    }

    #[test]
    fn toggle_reaches_the_listening_process() {
        let dir = runtime("toggle");
        let received = server(&dir);
        let spawned = || -> io::Result<Child> { panic!("nothing to start") };
        assert_eq!(
            toggle_with(&socket_path(&dir), spawned, STARTUP_WAIT),
            Ok(())
        );
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)),
            Ok(QuickCommand::Toggle)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn toggle_starts_the_quick_terminal_when_nothing_listens() {
        let dir = runtime("start");
        // A socket file left by a process that died. A plain file stands for it: a
        // socket bound here could live on in a child another test forks meanwhile,
        // where the close-on-exec flag is set after the socket is created (macOS).
        leave_dead_socket(&dir);
        let socket = socket_path(&dir);
        let mut started = None;
        let spawn = || {
            // The started process listens.
            started = Some(bind(&dir).unwrap());
            shell("sleep 10")
        };
        assert_eq!(toggle_with(&socket, spawn, STARTUP_WAIT), Ok(()));
        assert!(started.is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn toggle_reports_wayland_from_the_exit_code() {
        let dir = runtime("wayland");
        let socket = socket_path(&dir);
        assert_eq!(
            toggle_with(&socket, || shell("exit 3"), STARTUP_WAIT),
            Err(ToggleError::Wayland)
        );
        assert_eq!(
            ToggleError::Wayland.to_string(),
            "the quick terminal is not available under Wayland"
        );
        assert!(matches!(
            toggle_with(&socket, || shell("exit 1"), STARTUP_WAIT),
            Err(ToggleError::Failed(_))
        ));
        // A second process that handed `toggle` to the first.
        assert_eq!(
            toggle_with(&socket, || shell("exit 0"), STARTUP_WAIT),
            Ok(())
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn toggle_gives_up_when_the_process_never_listens() {
        let dir = runtime("timeout");
        let mut child = None;
        let result = toggle_with(
            &socket_path(&dir),
            || {
                let spawned = shell("sleep 10")?;
                child = Some(spawned.id());
                Ok(spawned)
            },
            Duration::from_millis(200),
        );
        assert!(matches!(result, Err(ToggleError::Failed(_))));
        if let Some(pid) = child {
            // SAFETY: plain signal to the test's own child.
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn second_instance_toggles_the_first_and_exits() {
        let dir = runtime("single");
        let first = claim(&dir, false).unwrap();
        assert!(first.is_some());
        let received = server(&dir);

        assert!(claim(&dir, false).unwrap().is_none());
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)),
            Ok(QuickCommand::Toggle)
        );
        // Started hidden, it leaves the first one alone.
        assert!(claim(&dir, true).unwrap().is_none());
        assert!(received.recv_timeout(Duration::from_millis(200)).is_err());

        // The lock is free once the first process is gone. A child another test forks
        // meanwhile holds the lock file until it runs its program, which closes it:
        // the claim is tried again for a while.
        drop(first);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut lock = claim(&dir, true).unwrap();
        while lock.is_none() && Instant::now() < deadline {
            std::thread::sleep(POLL_INTERVAL);
            lock = claim(&dir, true).unwrap();
        }
        assert!(lock.is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hidden_start_is_skipped_when_the_quick_terminal_listens() {
        let dir = runtime("hidden-running");
        let received = server(&dir);
        let spawned = || -> io::Result<Child> { panic!("nothing to start") };
        assert!(!start_hidden_with(&socket_path(&dir), spawned).unwrap());
        // The probe sends no command: the window stays as it is.
        assert!(received.recv_timeout(Duration::from_millis(200)).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hidden_start_spawns_and_waits_when_nothing_listens() {
        let dir = runtime("hidden-start");
        // A socket file left by a process that died. A plain file stands for it: a
        // socket bound here could live on in a child another test forks meanwhile,
        // where the close-on-exec flag is set after the socket is created (macOS).
        leave_dead_socket(&dir);
        assert!(start_hidden_with(&socket_path(&dir), || shell("exit 0")).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_timeout_refused_after_the_peer_closed_is_ignored() {
        assert!(timeout_set(Ok(())).is_ok());
        assert!(timeout_set(Err(io::Error::from_raw_os_error(libc::EINVAL))).is_ok());
        let refused = timeout_set(Err(io::Error::from_raw_os_error(libc::EBADF)));
        assert_eq!(refused.unwrap_err().raw_os_error(), Some(libc::EBADF));
    }

    #[test]
    fn a_command_sent_and_closed_before_it_is_read_still_arrives() {
        let dir = runtime("closed");
        let listener = bind(&dir).unwrap();
        // Sent and closed before the server accepts it.
        send(&socket_path(&dir), QuickCommand::Show).unwrap();
        let (sender, received) = mpsc::channel();
        serve(listener, move |command| sender.send(command).is_ok()).unwrap();
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)),
            Ok(QuickCommand::Show)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unknown_lines_are_dropped() {
        let dir = runtime("unknown");
        let received = server(&dir);
        let mut stream = UnixStream::connect(socket_path(&dir)).unwrap();
        stream.write_all(b"quit\n").unwrap();
        drop(stream);
        send(&socket_path(&dir), QuickCommand::Hide).unwrap();
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)),
            Ok(QuickCommand::Hide)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
