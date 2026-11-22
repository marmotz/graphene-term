//! Linux taskbar backend: the `progress` of the `com.canonical.Unity.LauncherEntry` API,
//! understood by KDE Plasma, Ubuntu Dock / Dash to Dock and Plank, and ignored without
//! error elsewhere.
//!
//! A thread named `taskbar` reads the progress the UI thread hands over and emits one
//! `Update` signal for each. Without session bus, it stops and the taskbar is inert.
//! The Unity API has no states: an error or a pause shows its value, an indeterminate
//! progress hides the bar.

use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::thread;

use zbus::Variant;

use super::{Shown, Taskbar};
use crate::progress::ProgressState;

/// Interface of the launcher entry signals.
pub(crate) const LAUNCHER_ENTRY: &str = "com.canonical.Unity.LauncherEntry";
/// Object the launcher entry signals are sent from; any path works.
pub(crate) const LAUNCHER_PATH: &str = "/dev/marmotz/graphene/LauncherEntry";
/// The application whose icon is updated, as installed by `packaging/graphene.desktop`.
pub(crate) const LAUNCHER_APP: &str = "application://graphene.desktop";

/// Arguments of the `com.canonical.Unity.LauncherEntry.Update` signal: application and
/// properties.
type UpdateArgs = (&'static str, HashMap<&'static str, Variant<'static>>);

/// The `progress` (0.0 to 1.0) and `progress-visible` launcher properties of `shown`.
fn progress_properties(shown: Option<Shown>) -> (f64, bool) {
    match shown {
        None
        | Some(Shown {
            state: ProgressState::Indeterminate,
            ..
        }) => (0.0, false),
        Some(Shown { value, .. }) => (f64::from(value) / 100.0, true),
    }
}

/// The `Update` arguments showing `shown`.
fn update_args(shown: Option<Shown>) -> UpdateArgs {
    let (progress, visible) = progress_properties(shown);
    let properties = HashMap::from([
        ("progress", Variant::from(progress)),
        ("progress-visible", Variant::from(visible)),
    ]);
    (LAUNCHER_APP, properties)
}

/// Starts the backend thread. Without session bus, every progress is dropped.
pub fn spawn() -> Taskbar {
    let (taskbar, progress) = Taskbar::channel();
    let spawned = thread::Builder::new()
        .name("taskbar".into())
        .spawn(move || run(&progress));
    match spawned {
        Ok(_) => taskbar,
        Err(error) => {
            log::warn!("cannot start the taskbar backend: {error}");
            Taskbar::null()
        }
    }
}

fn run(progress: &Receiver<Option<Shown>>) {
    let connection = match zbus::block_on(zbus::Connection::session()) {
        Ok(connection) => connection,
        Err(error) => {
            log::debug!("taskbar progress unavailable ({error})");
            // Dropping the receiver makes `Taskbar::set_progress` a no-op.
            return;
        }
    };
    // Waits for each value outside `block_on`, so the connection keeps running.
    for shown in progress {
        let sent = zbus::block_on(connection.emit_signal(
            None::<&str>,
            LAUNCHER_PATH,
            LAUNCHER_ENTRY,
            "Update",
            &update_args(shown),
        ));
        if let Err(error) = sent {
            log::debug!("cannot update the taskbar progress: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(state: ProgressState, value: u8) -> Option<Shown> {
        Some(Shown { state, value })
    }

    #[test]
    fn a_percentage_shows_its_fraction() {
        assert_eq!(
            progress_properties(shown(ProgressState::Normal, 0)),
            (0.0, true)
        );
        assert_eq!(
            progress_properties(shown(ProgressState::Normal, 25)),
            (0.25, true)
        );
        assert_eq!(
            progress_properties(shown(ProgressState::Normal, 100)),
            (1.0, true)
        );
    }

    #[test]
    fn an_error_or_a_pause_shows_its_value() {
        assert_eq!(
            progress_properties(shown(ProgressState::Error, 60)),
            (0.6, true)
        );
        assert_eq!(
            progress_properties(shown(ProgressState::Paused, 30)),
            (0.3, true)
        );
    }

    #[test]
    fn an_indeterminate_progress_or_none_hides_the_bar() {
        assert_eq!(
            progress_properties(shown(ProgressState::Indeterminate, 0)),
            (0.0, false)
        );
        assert_eq!(progress_properties(None), (0.0, false));
    }

    #[test]
    fn the_update_targets_the_desktop_entry() {
        let (app, properties) = update_args(shown(ProgressState::Normal, 50));
        assert_eq!(app, "application://graphene.desktop");
        assert_eq!(properties.len(), 2);
        assert_eq!(properties["progress"], Variant::from(0.5));
        assert_eq!(properties["progress-visible"], Variant::from(true));
    }

    #[test]
    fn the_update_leaves_the_badge_alone() {
        let (_, properties) = update_args(None);
        assert!(!properties.contains_key("count"));
        assert!(!properties.contains_key("count-visible"));
        assert!(!properties.contains_key("urgent"));
    }
}
