//! Desktop notifications: which notices of a pane raise one, what it says, and the
//! counters of unseen notices. The UI thread decides with these pure functions and
//! hands the result to a [`backend`].

pub mod backend;

use std::time::Duration;

use crate::config::NotificationsConfig;
use crate::i18n::{self, Lang, Msg};
use crate::signals;
use crate::terminal::PaneId;
use crate::timeline::{PaneNotice, Status};

use self::backend::Command;

/// Title of a notification when the pane has none.
const APP_NAME: &str = "Graphene";

/// Something in a pane that may raise a notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// Queued by the pane's timeline: a program's request or a finished command.
    Pane(PaneNotice),
    /// The terminal bell rang.
    Bell,
}

/// Whether the configuration asks for a notification of `notice`: its source is
/// switched on and, for a finished command, it ran at least `command_threshold`
/// seconds.
pub fn wanted(notice: &Notice, config: &NotificationsConfig) -> bool {
    match notice {
        Notice::Pane(PaneNotice::Program(_)) => config.program,
        Notice::Pane(PaneNotice::CommandFinished { duration, .. }) => {
            config.command_finished && *duration >= Duration::from_secs(config.command_threshold)
        }
        Notice::Bell => config.bell,
    }
}

/// Whether the user is looking at `pane`: the window has the focus and `pane` is its
/// active pane. A watched pane gets neither a notification nor a count.
pub fn watched(focused: bool, active_pane: Option<PaneId>, pane: PaneId) -> bool {
    focused && active_pane == Some(pane)
}

/// Title and body of the notification of `notice`, in `lang`. `pane_title` is the
/// title of the pane the notice comes from, if it has one.
pub fn content(notice: &Notice, pane_title: Option<&str>, lang: Lang) -> (String, String) {
    let pane_title = || pane_title.unwrap_or(APP_NAME).to_owned();
    match notice {
        Notice::Pane(PaneNotice::Program(signals::Notice::Program { title, body })) => {
            (title.clone().unwrap_or_else(pane_title), body.clone())
        }
        Notice::Pane(PaneNotice::CommandFinished {
            command,
            status,
            duration,
        }) => {
            let title = match status {
                Status::Failure(code) => Msg::CommandFailed(*code),
                _ => Msg::CommandFinished,
            };
            let duration = format_duration(*duration);
            let body = if command.is_empty() {
                duration
            } else {
                format!("{command} · {duration}")
            };
            (i18n::text(lang, &title).into_owned(), body)
        }
        Notice::Bell => (pane_title(), i18n::text(lang, &Msg::Bell).into_owned()),
    }
}

/// `duration` in whole units: `42 s` below a minute, `2 min 13 s` below an hour, else
/// `1 h 5 min`. A zero last unit is left out (`2 min`, `1 h`).
pub fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (big, small, big_unit, small_unit) = match seconds {
        0..60 => return format!("{seconds} s"),
        60..3600 => (seconds / 60, seconds % 60, "min", "s"),
        _ => (seconds / 3600, seconds % 3600 / 60, "h", "min"),
    };
    if small == 0 {
        format!("{big} {big_unit}")
    } else {
        format!("{big} {big_unit} {small} {small_unit}")
    }
}

/// Resets the `unseen` count of `pane` once it is watched: the command closing the
/// notification it still shows, `None` when it had nothing unseen.
pub fn clear(unseen: &mut u32, pane: PaneId) -> Option<Command> {
    if *unseen == 0 {
        return None;
    }
    *unseen = 0;
    Some(Command::Close { pane })
}

/// The count last shown on the window's taskbar or Dock icon.
#[derive(Debug, Default)]
pub struct Badge {
    shown: u32,
}

impl Badge {
    /// The command showing `total`, the unseen notices of every pane, when it differs
    /// from the count shown.
    pub fn sync(&mut self, total: u32) -> Option<Command> {
        if total == self.shown {
            return None;
        }
        self.shown = total;
        Some(Command::Badge(total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::LANGS;

    fn config() -> NotificationsConfig {
        NotificationsConfig {
            program: true,
            command_finished: true,
            command_threshold: 10,
            bell: false,
        }
    }

    fn program(title: Option<&str>, body: &str) -> Notice {
        Notice::Pane(PaneNotice::Program(signals::Notice::Program {
            title: title.map(str::to_owned),
            body: body.to_owned(),
        }))
    }

    fn finished(command: &str, status: Status, seconds: u64) -> Notice {
        Notice::Pane(PaneNotice::CommandFinished {
            command: command.to_owned(),
            status,
            duration: Duration::from_secs(seconds),
        })
    }

    #[test]
    fn each_source_follows_its_switch() {
        let on = config();
        let off = NotificationsConfig {
            program: false,
            command_finished: false,
            bell: true,
            ..config()
        };
        let program = program(None, "done");
        let command = finished("make", Status::Success, 60);
        assert!(wanted(&program, &on));
        assert!(!wanted(&program, &off));
        assert!(wanted(&command, &on));
        assert!(!wanted(&command, &off));
        assert!(!wanted(&Notice::Bell, &on));
        assert!(wanted(&Notice::Bell, &off));
    }

    #[test]
    fn a_command_is_notified_from_the_threshold_on() {
        let config = config();
        assert!(!wanted(&finished("ls", Status::Success, 9), &config));
        assert!(wanted(&finished("make", Status::Success, 10), &config));
        assert!(wanted(&finished("make", Status::Failure(1), 3600), &config));
        let just_below = Notice::Pane(PaneNotice::CommandFinished {
            command: "make".into(),
            status: Status::Success,
            duration: Duration::from_millis(9_999),
        });
        assert!(!wanted(&just_below, &config));
    }

    #[test]
    fn only_the_active_pane_of_a_focused_window_is_watched() {
        let (pane, other) = (PaneId(1), PaneId(2));
        assert!(watched(true, Some(pane), pane));
        assert!(!watched(false, Some(pane), pane));
        assert!(!watched(true, Some(other), pane));
        assert!(!watched(true, None, pane));
    }

    #[test]
    fn a_program_notification_falls_back_to_the_pane_title_then_the_app_name() {
        let osc9 = program(None, "build done");
        assert_eq!(
            content(&osc9, Some("~/src"), Lang::En),
            ("~/src".into(), "build done".into())
        );
        assert_eq!(
            content(&osc9, None, Lang::En),
            ("Graphene".into(), "build done".into())
        );
        let osc777 = program(Some("CI"), "green");
        assert_eq!(
            content(&osc777, Some("~/src"), Lang::Fr),
            ("CI".into(), "green".into())
        );
    }

    #[test]
    fn a_finished_command_tells_its_status_command_and_duration() {
        assert_eq!(
            content(
                &finished("cargo build", Status::Success, 133),
                None,
                Lang::En
            ),
            ("Command finished".into(), "cargo build · 2 min 13 s".into())
        );
        assert_eq!(
            content(&finished("make", Status::Failure(2), 12), None, Lang::Fr),
            ("Commande échouée (code 2)".into(), "make · 12 s".into())
        );
        assert_eq!(
            content(&finished("make", Status::Unknown, 12), None, Lang::Fr),
            ("Commande terminée".into(), "make · 12 s".into())
        );
    }

    #[test]
    fn a_finished_command_without_command_line_shows_only_its_duration() {
        assert_eq!(
            content(&finished("", Status::Success, 75), Some("zsh"), Lang::En).1,
            "1 min 15 s"
        );
    }

    #[test]
    fn a_bell_is_titled_by_its_pane() {
        assert_eq!(
            content(&Notice::Bell, Some("vim"), Lang::Fr),
            ("vim".into(), "Sonnerie".into())
        );
        for lang in LANGS {
            assert_eq!(content(&Notice::Bell, None, lang).0, "Graphene");
        }
    }

    #[test]
    fn durations_are_written_in_their_two_largest_units() {
        let format = |seconds| format_duration(Duration::from_secs(seconds));
        assert_eq!(format(0), "0 s");
        assert_eq!(format(59), "59 s");
        assert_eq!(format(60), "1 min");
        assert_eq!(format(133), "2 min 13 s");
        assert_eq!(format(3599), "59 min 59 s");
        assert_eq!(format(3600), "1 h");
        assert_eq!(format(3600 + 5 * 60 + 59), "1 h 5 min");
        assert_eq!(format(26 * 3600), "26 h");
        assert_eq!(format_duration(Duration::from_millis(12_900)), "12 s");
    }

    #[test]
    fn clearing_a_count_closes_its_notification_once() {
        let pane = PaneId(3);
        let mut unseen = 2;
        assert_eq!(clear(&mut unseen, pane), Some(Command::Close { pane }));
        assert_eq!(unseen, 0);
        assert_eq!(clear(&mut unseen, pane), None);
    }

    #[test]
    fn the_badge_is_sent_only_when_the_total_changes() {
        let mut badge = Badge::default();
        assert_eq!(badge.sync(0), None);
        assert_eq!(badge.sync(1), Some(Command::Badge(1)));
        assert_eq!(badge.sync(1), None);
        assert_eq!(badge.sync(3), Some(Command::Badge(3)));
        assert_eq!(badge.sync(0), Some(Command::Badge(0)));
        assert_eq!(badge.sync(0), None);
    }
}
