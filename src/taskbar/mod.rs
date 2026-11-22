//! Progress of the active tab on the window's taskbar or Dock icon.
//!
//! The UI thread hands the progress of the active tab to a [`Taskbar`] after each round
//! of events; [`Taskbar::set_progress`] only reaches the desktop when what the icon
//! shows changes. Each platform has its own backend:
//!
//! - Linux: the `com.canonical.Unity.LauncherEntry` signal, sent by a thread of its own
//!   ([`linux`]).
//! - Elsewhere: nothing, every progress is dropped.
//!
//! A backend for another platform adds a variant to [`Sink`], built by [`spawn`], and
//! receives the deduplicated [`Shown`] values in [`Taskbar::set_progress`].

#[cfg(target_os = "linux")]
pub(crate) mod linux;

#[cfg(any(target_os = "linux", test))]
use std::sync::mpsc;

use crate::progress::{Progress, ProgressState};

/// What the icon shows of a progress: its state and value, without its timestamp, so a
/// repeated sequence does not reach the desktop again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown {
    pub state: ProgressState,
    /// 0 to 100; always 0 for [`ProgressState::Indeterminate`].
    pub value: u8,
}

impl From<Progress> for Shown {
    fn from(progress: Progress) -> Self {
        let value = match progress.state {
            ProgressState::Indeterminate => 0,
            _ => progress.value,
        };
        Self {
            state: progress.state,
            value,
        }
    }
}

/// Where the progress of a [`Taskbar`] goes.
#[derive(Default)]
enum Sink {
    /// No backend: every progress is dropped.
    #[default]
    None,
    /// Read by the thread of a backend.
    #[cfg(any(target_os = "linux", test))]
    Channel(mpsc::Sender<Option<Shown>>),
}

/// Shows the progress of the active tab on the window's icon.
#[derive(Default)]
pub struct Taskbar {
    sink: Sink,
    /// What the icon shows, `None` for no progress.
    shown: Option<Shown>,
}

impl std::fmt::Debug for Taskbar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sink = match self.sink {
            Sink::None => "none",
            #[cfg(any(target_os = "linux", test))]
            Sink::Channel(_) => "channel",
        };
        f.debug_struct("Taskbar")
            .field("sink", &sink)
            .field("shown", &self.shown)
            .finish()
    }
}

impl Taskbar {
    /// A taskbar dropping every progress.
    pub fn null() -> Self {
        Self::default()
    }

    /// A taskbar whose progress is read from the returned receiver, by the thread of a
    /// backend.
    #[cfg(any(target_os = "linux", test))]
    pub fn channel() -> (Self, mpsc::Receiver<Option<Shown>>) {
        let (sender, receiver) = mpsc::channel();
        let taskbar = Self {
            sink: Sink::Channel(sender),
            shown: None,
        };
        (taskbar, receiver)
    }

    /// Shows `progress` on the icon, `None` hiding it. Only a change reaches the
    /// backend; a backend that stopped is ignored.
    pub fn set_progress(&mut self, progress: Option<Progress>) {
        let shown = progress.map(Shown::from);
        if shown == self.shown {
            return;
        }
        self.shown = shown;
        match &self.sink {
            Sink::None => {}
            #[cfg(any(target_os = "linux", test))]
            Sink::Channel(sender) => {
                let _ = sender.send(shown);
            }
        }
    }
}

/// Starts the taskbar backend of this system.
pub fn spawn() -> Taskbar {
    #[cfg(target_os = "linux")]
    {
        linux::spawn()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Taskbar::null()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn progress(state: ProgressState, value: u8, updated: Instant) -> Progress {
        Progress {
            state,
            value,
            updated,
        }
    }

    #[test]
    fn shown_drops_the_timestamp_and_the_indeterminate_value() {
        let now = Instant::now();
        assert_eq!(
            Shown::from(progress(ProgressState::Paused, 40, now)),
            Shown {
                state: ProgressState::Paused,
                value: 40
            }
        );
        assert_eq!(
            Shown::from(progress(ProgressState::Indeterminate, 40, now)),
            Shown {
                state: ProgressState::Indeterminate,
                value: 0
            }
        );
    }

    #[test]
    fn only_a_change_reaches_the_backend() {
        let (mut taskbar, receiver) = Taskbar::channel();
        let now = Instant::now();
        let later = now + Duration::from_secs(1);

        // Nothing shown yet: no progress is no change.
        taskbar.set_progress(None);
        assert!(receiver.try_recv().is_err());

        taskbar.set_progress(Some(progress(ProgressState::Normal, 10, now)));
        // The same state and value received again.
        taskbar.set_progress(Some(progress(ProgressState::Normal, 10, later)));
        taskbar.set_progress(Some(progress(ProgressState::Normal, 20, later)));
        // An indeterminate value is meaningless: still no change.
        taskbar.set_progress(Some(progress(ProgressState::Indeterminate, 20, later)));
        taskbar.set_progress(Some(progress(ProgressState::Indeterminate, 30, later)));
        taskbar.set_progress(None);
        taskbar.set_progress(None);

        let normal = |value| {
            Some(Shown {
                state: ProgressState::Normal,
                value,
            })
        };
        let indeterminate = Some(Shown {
            state: ProgressState::Indeterminate,
            value: 0,
        });
        let sent: Vec<_> = receiver.try_iter().collect();
        assert_eq!(sent, vec![normal(10), normal(20), indeterminate, None]);
    }

    #[test]
    fn a_stopped_backend_or_none_is_ignored() {
        let now = Instant::now();
        Taskbar::null().set_progress(Some(progress(ProgressState::Normal, 1, now)));
        let (mut taskbar, receiver) = Taskbar::channel();
        drop(receiver);
        taskbar.set_progress(Some(progress(ProgressState::Normal, 1, now)));
    }
}
