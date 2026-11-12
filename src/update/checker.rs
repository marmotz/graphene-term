//! Background update check: the `update-check` thread wakes up when a check is due,
//! refreshes the shared state file, and reports the release to show to the UI thread.
//! The loop body is the pure [`tick`] function; the thread itself is a thin loop around it.

use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use semver::Version;

use super::state::{self, State};
use super::{Channel, Release};
use crate::config::UpdatesConfig;

/// Delay before retrying after a failed check, capped by the interval.
const RETRY_DELAY: Duration = Duration::from_secs(3_600);

/// What one wake-up of the thread decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tick {
    /// The state was refreshed by a successful check and must be saved.
    pub state_changed: bool,
    /// Release to show, `None` when nothing is newer (or it is ignored).
    pub notice: Option<Release>,
    /// Unix time of the next wake-up.
    pub wake_at: u64,
}

/// One wake-up at `now`: checks GitHub with `fetch` when the last check is older than
/// `interval`, otherwise reuses `latest` from `state` (the cache lasts one interval and
/// is shared by every window). A failed check is retried after [`RETRY_DELAY`].
pub fn tick(
    state: &mut State,
    now: u64,
    interval: Duration,
    current: &Version,
    fetch: impl FnOnce() -> anyhow::Result<Option<Release>>,
) -> Tick {
    let state_changed =
        now >= super::due(state.last_check, interval) && super::refresh(state, now, fetch);
    let due = super::due(state.last_check, interval);
    let wake_at = if due > now {
        due
    } else {
        now.saturating_add(RETRY_DELAY.min(interval).as_secs())
    };
    Tick {
        state_changed,
        notice: super::notice(current, state.latest.as_ref(), state.ignored.as_deref()),
        wake_at,
    }
}

/// Messages sent to the thread.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// New `[updates]` settings (config hot reload).
    Settings(UpdatesConfig),
}

/// Owns the `update-check` thread: starts it when `check` is on, ends it when it is off.
pub struct Checker {
    sender: Option<Sender<Command>>,
    notify: Notify,
}

type Notify = std::sync::Arc<dyn Fn(Option<Release>) + Send + Sync>;

impl Checker {
    /// `notify` receives the release to show each time it changes, from the thread.
    pub fn new(notify: impl Fn(Option<Release>) + Send + Sync + 'static) -> Self {
        Self {
            sender: None,
            notify: std::sync::Arc::new(notify),
        }
    }

    /// Applies `[updates]` settings: `check = false` ends the thread and clears the
    /// notice, `true` starts it (again) or forwards the new interval to it.
    pub fn apply(&mut self, settings: &UpdatesConfig) {
        if !settings.check {
            if self.sender.take().is_some() {
                (self.notify)(None);
            }
            return;
        }
        if let Some(sender) = &self.sender
            && sender.send(Command::Settings(settings.clone())).is_ok()
        {
            return;
        }
        self.sender = spawn(settings.clone(), self.notify.clone());
    }
}

/// Starts the thread. `None` (logged) when the thread cannot be created.
fn spawn(settings: UpdatesConfig, notify: Notify) -> Option<Sender<Command>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("update-check".to_owned())
        .spawn(move || run(settings, &receiver, &*notify))
        .map_err(|error| log::warn!("cannot start the update check thread: {error}"))
        .ok()?;
    Some(sender)
}

fn run(
    mut settings: UpdatesConfig,
    receiver: &mpsc::Receiver<Command>,
    notify: &dyn Fn(Option<Release>),
) {
    let channel = Channel::current();
    let current = super::current_version();
    let path: Option<PathBuf> = state::path();
    let mut shown: Option<Option<Release>> = None;
    loop {
        // Reloaded on each wake-up: other instances share the file.
        let mut state = path.as_deref().map(State::load).unwrap_or_default();
        let interval = super::interval(settings.interval, channel);
        let tick = tick(&mut state, unix_now(), interval, &current, || {
            super::fetch_latest(channel)
        });
        if tick.state_changed
            && let Some(path) = &path
            && let Err(error) = state.save(path)
        {
            log::warn!("cannot write {}: {error}", path.display());
        }
        if shown.as_ref() != Some(&tick.notice) {
            notify(tick.notice.clone());
            shown = Some(tick.notice);
        }
        let wait = Duration::from_secs(tick.wake_at.saturating_sub(unix_now()));
        match receiver.recv_timeout(wait) {
            Ok(Command::Settings(new)) if new.check => settings = new,
            Ok(Command::Settings(_)) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: u64 = 3_600;
    /// A realistic Unix time: a state never checked (`last_check = 0`) is long overdue.
    const NOW: u64 = 1_800_000_000;
    const SIX_HOURS: Duration = Duration::from_secs(6 * HOUR);

    fn release(tag: &str) -> Release {
        Release {
            tag: tag.to_owned(),
            url: format!("https://github.com/marmotz/graphene-term/releases/tag/{tag}"),
        }
    }

    fn current() -> Version {
        Version::parse("0.1.0").unwrap()
    }

    fn unreachable_fetch() -> anyhow::Result<Option<Release>> {
        panic!("no check expected")
    }

    #[test]
    fn never_checked_checks_at_once() {
        let mut state = State::default();
        let tick = tick(&mut state, NOW, SIX_HOURS, &current(), || {
            Ok(Some(release("v0.2.0")))
        });
        assert_eq!(
            tick,
            Tick {
                state_changed: true,
                notice: Some(release("v0.2.0")),
                wake_at: NOW + 6 * HOUR,
            }
        );
        assert_eq!(state.last_check, NOW);
    }

    #[test]
    fn fresh_cache_is_reused_without_a_check() {
        let mut state = State {
            last_check: 10_000,
            latest: Some(release("v0.2.0")),
            ignored: None,
        };
        let tick = tick(
            &mut state,
            10_000 + HOUR,
            SIX_HOURS,
            &current(),
            unreachable_fetch,
        );
        assert_eq!(
            tick,
            Tick {
                state_changed: false,
                notice: Some(release("v0.2.0")),
                wake_at: 10_000 + 6 * HOUR,
            }
        );
    }

    #[test]
    fn due_check_refreshes_the_cache() {
        let mut state = State {
            last_check: 10_000,
            latest: Some(release("v0.2.0")),
            ignored: None,
        };
        let now = 10_000 + 6 * HOUR;
        let tick = tick(&mut state, now, SIX_HOURS, &current(), || {
            Ok(Some(release("v0.3.0")))
        });
        assert!(tick.state_changed);
        assert_eq!(tick.notice, Some(release("v0.3.0")));
        assert_eq!(tick.wake_at, now + 6 * HOUR);
    }

    #[test]
    fn failed_check_keeps_the_cache_and_retries_later() {
        let mut state = State {
            last_check: 10_000,
            latest: Some(release("v0.2.0")),
            ignored: None,
        };
        let now = 10_000 + 7 * HOUR;
        let tick = tick(&mut state, now, SIX_HOURS, &current(), || {
            anyhow::bail!("offline")
        });
        assert_eq!(
            tick,
            Tick {
                state_changed: false,
                notice: Some(release("v0.2.0")),
                wake_at: now + HOUR,
            }
        );
    }

    #[test]
    fn retry_delay_is_capped_by_the_interval() {
        let mut state = State::default();
        let interval = Duration::from_secs(60);
        let tick = tick(&mut state, 10_000, interval, &current(), || {
            anyhow::bail!("offline")
        });
        assert_eq!(tick.wake_at, 10_060);
    }

    #[test]
    fn ignored_or_older_release_gives_no_notice() {
        let mut state = State {
            last_check: 10_000,
            latest: Some(release("v0.2.0")),
            ignored: Some("v0.2.0".to_owned()),
        };
        let tick_ignored = tick(&mut state, 10_000, SIX_HOURS, &current(), unreachable_fetch);
        assert_eq!(tick_ignored.notice, None);

        let mut state = State {
            last_check: 10_000,
            latest: Some(release("v0.0.9")),
            ignored: None,
        };
        let tick_older = tick(&mut state, 10_000, SIX_HOURS, &current(), unreachable_fetch);
        assert_eq!(tick_older.notice, None);
    }

    #[test]
    fn longer_interval_postpones_the_next_wake_up() {
        let mut state = State {
            last_check: 10_000,
            latest: None,
            ignored: None,
        };
        let week = Duration::from_secs(7 * 24 * HOUR);
        let tick = tick(
            &mut state,
            10_000 + 6 * HOUR,
            week,
            &current(),
            unreachable_fetch,
        );
        assert!(!tick.state_changed);
        assert_eq!(tick.wake_at, 10_000 + 7 * 24 * HOUR);
    }

    #[test]
    fn disabled_checker_starts_no_thread() {
        let notified = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = notified.clone();
        let mut checker = Checker::new(move |release| sink.lock().unwrap().push(release));
        checker.apply(&UpdatesConfig {
            check: false,
            interval: crate::config::UpdateInterval::Auto,
        });
        assert!(checker.sender.is_none());
        // Nothing was running: no notice to clear.
        assert!(notified.lock().unwrap().is_empty());
    }
}
