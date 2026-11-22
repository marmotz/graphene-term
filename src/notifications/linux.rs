//! Linux notification backend: `org.freedesktop.Notifications` over the session bus,
//! and the `com.canonical.Unity.LauncherEntry` badge of the taskbar icon.
//!
//! A thread named `notifications` reads the commands of the UI thread and runs each
//! D-Bus call in its own `zbus::block_on`; between two calls, the connection keeps
//! running on zbus's helper thread. A second thread, `notification-signals`, waits for
//! the clicks and closings the notification server reports.

use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use futures_util::StreamExt;
use winit::event_loop::EventLoopProxy;
use zbus::Variant;

use super::backend::{Command, Notifier};
use crate::terminal::{PaneId, UserEvent};

const SERVER: &str = "org.freedesktop.Notifications";
const SERVER_PATH: &str = "/org/freedesktop/Notifications";
/// Name and icon of the application, as installed by `packaging/graphene.desktop`.
const APP_NAME: &str = "Graphene";
const APP_ICON: &str = "graphene";
const DESKTOP_ENTRY: &str = "graphene";
/// Action invoked when the user clicks the notification itself.
const DEFAULT_ACTION: &str = "default";
/// The notification stays until the user dismisses it, or Graphene closes it.
const NEVER_EXPIRES: i32 = -1;

const LAUNCHER_ENTRY: &str = "com.canonical.Unity.LauncherEntry";
const LAUNCHER_PATH: &str = "/dev/marmotz/graphene/LauncherEntry";
const LAUNCHER_APP: &str = "application://graphene.desktop";

/// Arguments of `org.freedesktop.Notifications.Notify`: app name, id to replace, icon,
/// summary, body, actions, hints, timeout.
type NotifyArgs<'a> = (
    &'a str,
    u32,
    &'a str,
    &'a str,
    &'a str,
    Vec<&'a str>,
    HashMap<&'a str, Variant<'a>>,
    i32,
);

/// Arguments of the `com.canonical.Unity.LauncherEntry.Update` signal: application and
/// properties.
type BadgeArgs = (&'static str, HashMap<&'static str, Variant<'static>>);

/// The notification each pane shows, by the id the server gave it.
#[derive(Debug, Default)]
struct Shown {
    by_pane: HashMap<PaneId, u32>,
}

impl Shown {
    /// The id a new notification of `pane` replaces: the one it still shows, or 0 for
    /// none.
    fn replaces(&self, pane: PaneId) -> u32 {
        self.by_pane.get(&pane).copied().unwrap_or(0)
    }

    /// `pane` now shows the notification `id`.
    fn show(&mut self, pane: PaneId, id: u32) {
        self.by_pane.retain(|_, shown| *shown != id);
        self.by_pane.insert(pane, id);
    }

    /// Forgets the notification of `pane`, returning its id to close it.
    fn take(&mut self, pane: PaneId) -> Option<u32> {
        self.by_pane.remove(&pane)
    }

    /// The pane showing the notification `id`.
    fn pane_of(&self, id: u32) -> Option<PaneId> {
        self.by_pane
            .iter()
            .find_map(|(pane, shown)| (*shown == id).then_some(*pane))
    }

    /// The notification `id` is gone from the desktop.
    fn closed(&mut self, id: u32) {
        self.by_pane.retain(|_, shown| *shown != id);
    }

    /// The pane to show when the user invoked `action` on the notification `id`.
    fn clicked(&self, id: u32, action: &str) -> Option<PaneId> {
        (action == DEFAULT_ACTION)
            .then(|| self.pane_of(id))
            .flatten()
    }
}

fn notify_args<'a>(
    replaces: u32,
    title: &'a str,
    body: &'a str,
    action: &'a str,
) -> NotifyArgs<'a> {
    let hints = HashMap::from([("desktop-entry", Variant::from(DESKTOP_ENTRY))]);
    (
        APP_NAME,
        replaces,
        APP_ICON,
        title,
        body,
        vec![DEFAULT_ACTION, action],
        hints,
        NEVER_EXPIRES,
    )
}

fn badge_args(count: u32) -> BadgeArgs {
    let properties = HashMap::from([
        ("count", Variant::from(i64::from(count))),
        ("count-visible", Variant::from(count > 0)),
    ]);
    (LAUNCHER_APP, properties)
}

/// A signal of the notification server.
enum Signal {
    ActionInvoked(zbus::Message),
    NotificationClosed(zbus::Message),
}

/// Starts the backend threads. Without session bus, every command is dropped; without
/// notification server, only the badge is sent.
pub fn spawn(events: EventLoopProxy<UserEvent>, action: String) -> Notifier {
    let (notifier, commands) = Notifier::channel();
    let spawned = thread::Builder::new()
        .name("notifications".into())
        .spawn(move || run(&commands, events, &action));
    match spawned {
        Ok(_) => notifier,
        Err(error) => {
            log::warn!("cannot start the notification backend: {error}");
            Notifier::null()
        }
    }
}

fn run(commands: &Receiver<Command>, events: EventLoopProxy<UserEvent>, action: &str) {
    let connection = match zbus::block_on(zbus::Connection::session()) {
        Ok(connection) => connection,
        Err(error) => {
            log::info!("desktop notifications unavailable ({error})");
            // Dropping the receiver makes `Notifier::send` a no-op.
            return;
        }
    };
    let shown = Arc::new(Mutex::new(Shown::default()));
    let server = match zbus::block_on(connect_server(&connection)) {
        Ok((server, signals)) => {
            listen(signals, Arc::clone(&shown), events);
            Some(server)
        }
        Err(error) => {
            log::info!("no desktop notification server ({error}): notifications are not shown");
            None
        }
    };

    // Waits for each command outside `block_on`, so the connection keeps running.
    for command in commands {
        match command {
            Command::Show { pane, title, body } => {
                let Some(server) = &server else { continue };
                let replaces = lock(&shown).replaces(pane);
                let args = notify_args(replaces, &title, &body, action);
                match zbus::block_on(server.call::<_, _, u32>("Notify", &args)) {
                    Ok(id) => lock(&shown).show(pane, id),
                    Err(error) => log::warn!("cannot show a notification: {error}"),
                }
            }
            Command::Close { pane } => {
                let Some(server) = &server else { continue };
                let Some(id) = lock(&shown).take(pane) else {
                    continue;
                };
                if let Err(error) = zbus::block_on(server.call_method("CloseNotification", &(id,)))
                {
                    log::warn!("cannot close a notification: {error}");
                }
            }
            Command::Badge(count) => {
                let sent = zbus::block_on(connection.emit_signal(
                    None::<&str>,
                    LAUNCHER_PATH,
                    LAUNCHER_ENTRY,
                    "Update",
                    &badge_args(count),
                ));
                if let Err(error) = sent {
                    log::warn!("cannot update the taskbar badge: {error}");
                }
            }
        }
    }
}

/// Reaches the notification server and subscribes to its signals, before any
/// notification is shown so that no click is missed.
async fn connect_server(
    connection: &zbus::Connection,
) -> zbus::Result<(
    zbus::Proxy<'static>,
    impl futures_util::Stream<Item = Signal> + Send + Unpin + 'static,
)> {
    let server = zbus::Proxy::new(connection, SERVER, SERVER_PATH, SERVER).await?;
    // Fails when no server runs and none can be activated.
    let _: (String, String, String, String) = server.call("GetServerInformation", &()).await?;
    let invoked = server
        .receive_signal("ActionInvoked")
        .await?
        .map(Signal::ActionInvoked);
    let closed = server
        .receive_signal("NotificationClosed")
        .await?
        .map(Signal::NotificationClosed);
    Ok((server, futures_util::stream::select(invoked, closed)))
}

/// Starts the thread that turns a click on a notification into
/// [`UserEvent::NotificationClicked`], and forgets the notifications the desktop closed.
fn listen(
    mut signals: impl futures_util::Stream<Item = Signal> + Send + Unpin + 'static,
    shown: Arc<Mutex<Shown>>,
    events: EventLoopProxy<UserEvent>,
) {
    let spawned = thread::Builder::new()
        .name("notification-signals".into())
        .spawn(move || {
            zbus::block_on(async {
                while let Some(signal) = signals.next().await {
                    match signal {
                        Signal::ActionInvoked(message) => {
                            match message.body().deserialize::<(u32, String)>() {
                                Ok((id, action)) => {
                                    if let Some(pane) = lock(&shown).clicked(id, &action) {
                                        let _ =
                                            events.send_event(UserEvent::NotificationClicked(pane));
                                    }
                                }
                                Err(error) => log::warn!("unreadable notification click: {error}"),
                            }
                        }
                        Signal::NotificationClosed(message) => {
                            match message.body().deserialize::<(u32, u32)>() {
                                Ok((id, _reason)) => lock(&shown).closed(id),
                                Err(error) => {
                                    log::warn!("unreadable notification closing: {error}")
                                }
                            }
                        }
                    }
                }
            });
        });
    if let Err(error) = spawned {
        log::warn!("cannot watch the notification clicks: {error}");
    }
}

/// The shown notifications, even after a panic of the other thread: the map stays
/// consistent between two of its calls.
fn lock(shown: &Mutex<Shown>) -> std::sync::MutexGuard<'_, Shown> {
    shown.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_replaces_the_notification_it_still_shows() {
        let mut shown = Shown::default();
        assert_eq!(shown.replaces(PaneId(1)), 0);
        shown.show(PaneId(1), 7);
        assert_eq!(shown.replaces(PaneId(1)), 7);
        assert_eq!(shown.replaces(PaneId(2)), 0);
        shown.show(PaneId(1), 9);
        assert_eq!(shown.replaces(PaneId(1)), 9);
        assert_eq!(shown.pane_of(7), None);
    }

    #[test]
    fn an_id_belongs_to_one_pane_only() {
        let mut shown = Shown::default();
        shown.show(PaneId(1), 7);
        shown.show(PaneId(2), 7);
        assert_eq!(shown.pane_of(7), Some(PaneId(2)));
        assert_eq!(shown.replaces(PaneId(1)), 0);
    }

    #[test]
    fn closing_forgets_the_notification() {
        let mut shown = Shown::default();
        shown.show(PaneId(1), 7);
        shown.show(PaneId(2), 8);
        assert_eq!(shown.take(PaneId(1)), Some(7));
        assert_eq!(shown.take(PaneId(1)), None);
        shown.closed(8);
        assert_eq!(shown.pane_of(8), None);
        assert_eq!(shown.replaces(PaneId(2)), 0);
    }

    #[test]
    fn only_the_default_action_of_a_known_notification_is_a_click() {
        let mut shown = Shown::default();
        shown.show(PaneId(3), 5);
        assert_eq!(shown.clicked(5, "default"), Some(PaneId(3)));
        assert_eq!(shown.clicked(5, "other"), None);
        assert_eq!(shown.clicked(6, "default"), None);
    }

    #[test]
    fn notify_arguments() {
        let (app, replaces, icon, title, body, actions, hints, timeout) =
            notify_args(4, "vim", "Command finished", "Show pane");
        assert_eq!(app, "Graphene");
        assert_eq!(replaces, 4);
        assert_eq!(icon, "graphene");
        assert_eq!(title, "vim");
        assert_eq!(body, "Command finished");
        assert_eq!(actions, ["default", "Show pane"]);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints["desktop-entry"], Variant::from("graphene"));
        assert_eq!(timeout, -1);
    }

    #[test]
    fn badge_arguments_hide_a_zero_count() {
        let (app, properties) = badge_args(3);
        assert_eq!(app, "application://graphene.desktop");
        assert_eq!(properties["count"], Variant::I64(3));
        assert_eq!(properties["count-visible"], Variant::Bool(true));
        let (_, properties) = badge_args(0);
        assert_eq!(properties["count"], Variant::I64(0));
        assert_eq!(properties["count-visible"], Variant::Bool(false));
    }

    #[test]
    fn the_arguments_have_the_dbus_signatures() {
        use zbus::DynamicType;
        let notify = notify_args(0, "t", "b", "a");
        assert_eq!(notify.signature().to_string(), "(susssasa{sv}i)");
        assert_eq!(badge_args(1).signature().to_string(), "(sa{sv})");
    }

    /// Needs a session bus with a notification server:
    /// `cargo test -- --ignored notification_server`. Shows, replaces then closes a
    /// notification, and sends a badge.
    #[test]
    #[ignore = "needs a desktop notification server"]
    fn notification_server_shows_replaces_and_closes() {
        zbus::block_on(async {
            let connection = zbus::Connection::session().await.expect("no session bus");
            let (server, _signals) = connect_server(&connection)
                .await
                .expect("no notification server");
            let first: u32 = server
                .call("Notify", &notify_args(0, "Graphene", "first", "Show pane"))
                .await
                .expect("Notify failed");
            let second: u32 = server
                .call(
                    "Notify",
                    &notify_args(first, "Graphene", "second", "Show pane"),
                )
                .await
                .expect("Notify with replaces_id failed");
            eprintln!("notification ids: {first} then {second}");
            server
                .call_method("CloseNotification", &(second,))
                .await
                .expect("CloseNotification failed");
            connection
                .emit_signal(
                    None::<&str>,
                    LAUNCHER_PATH,
                    LAUNCHER_ENTRY,
                    "Update",
                    &badge_args(0),
                )
                .await
                .expect("badge signal failed");
        });
    }
}
