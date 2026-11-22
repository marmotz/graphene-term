//! macOS backend: notifications through the User Notifications framework, the counter
//! on the Dock icon, and the clicks on a notification sent back to the event loop.
//!
//! The framework only works from an application bundle (`Graphene.app`): run outside
//! of one (`cargo run`), notifications are unavailable but the Dock badge still works.
//! Every call only starts asynchronous work, so the backend runs on the UI thread,
//! which is also the main thread AppKit wants for the Dock.
//!
//! The pure parts (request identifiers, badge label, authorization gate) are compiled
//! on every system so that they are tested everywhere.

use crate::terminal::PaneId;

/// Prefix of the request identifier of a pane's notification: one identifier per pane,
/// so a new notification replaces the one the pane still shows.
const IDENTIFIER_PREFIX: &str = "pane-";

/// Identifier of the notification request of `pane`.
fn identifier(pane: PaneId) -> String {
    format!("{IDENTIFIER_PREFIX}{}", pane.0)
}

/// The pane a request identifier belongs to, if it is one of ours.
fn pane_of(identifier: &str) -> Option<PaneId> {
    let id = identifier.strip_prefix(IDENTIFIER_PREFIX)?;
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    id.parse().ok().map(PaneId)
}

/// Text of the Dock badge for `count` unseen notices; `None` hides it.
fn badge_label(count: u32) -> Option<String> {
    (count > 0).then(|| count.to_string())
}

/// A notification waiting for the user's answer to the authorization request.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    pane: PaneId,
    title: String,
    body: String,
}

/// Whether notifications may be shown. Authorization is asked at the first notification,
/// not at startup; a refusal is kept and the next notifications are dropped.
#[derive(Debug, Default, PartialEq, Eq)]
enum Gate {
    /// Never asked.
    #[default]
    Unasked,
    /// Asked, no answer yet: the latest notification of each pane waits.
    Asking(Vec<Pending>),
    /// Granted: notifications are shown.
    Granted,
    /// Refused, or the request failed: notifications are dropped.
    Denied,
}

/// What to do with a notification, decided by the [`Gate`].
#[derive(Debug, PartialEq, Eq)]
enum Shown {
    /// Ask for authorization; the notification waits for the answer.
    Ask,
    /// Authorization is being asked; the notification waits for the answer.
    Wait,
    /// Show it now.
    Post,
    /// Drop it: notifications were refused.
    Drop,
}

impl Gate {
    /// Takes a notification for `pane`.
    fn show(&mut self, pane: PaneId, title: &str, body: &str) -> Shown {
        let pending = Pending {
            pane,
            title: title.to_owned(),
            body: body.to_owned(),
        };
        match self {
            Self::Unasked => {
                *self = Self::Asking(vec![pending]);
                Shown::Ask
            }
            Self::Asking(waiting) => {
                waiting.retain(|other| other.pane != pane);
                waiting.push(pending);
                Shown::Wait
            }
            Self::Granted => Shown::Post,
            Self::Denied => Shown::Drop,
        }
    }

    /// Forgets the notification of `pane` that still waits for the answer.
    fn close(&mut self, pane: PaneId) {
        if let Self::Asking(waiting) = self {
            waiting.retain(|other| other.pane != pane);
        }
    }

    /// Records the user's answer, and returns the notifications to show now.
    fn answered(&mut self, granted: bool) -> Vec<Pending> {
        let waiting = match std::mem::replace(self, Self::Denied) {
            Self::Asking(waiting) => waiting,
            _ => Vec::new(),
        };
        if granted {
            *self = Self::Granted;
            waiting
        } else {
            Vec::new()
        }
    }
}

#[cfg(target_os = "macos")]
pub use self::native::{Backend, spawn};

#[cfg(target_os = "macos")]
mod native {
    use std::sync::{Arc, Mutex, PoisonError};

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send};
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{NSArray, NSBundle, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
        UNNotificationDefaultActionIdentifier, UNNotificationPresentationOptions,
        UNNotificationRequest, UNNotificationResponse, UNUserNotificationCenter,
        UNUserNotificationCenterDelegate,
    };
    use winit::event_loop::EventLoopProxy;

    use super::{Gate, Pending, Shown, badge_label, identifier, pane_of};
    use crate::notifications::backend::{Command, Notifier};
    use crate::terminal::{PaneId, UserEvent};

    /// Starts the macOS backend. Must run on the main thread, like the event loop.
    pub fn spawn(events: EventLoopProxy<UserEvent>) -> Notifier {
        let Some(main_thread) = MainThreadMarker::new() else {
            log::warn!("the notification backend must start on the main thread");
            return Notifier::null();
        };
        Notifier::main_thread(Backend::new(main_thread, events))
    }

    /// The notification center and its delegate, when the application runs from a
    /// bundle, and the Dock.
    pub struct Backend {
        main_thread: MainThreadMarker,
        notifications: Option<Notifications>,
    }

    /// What showing notifications needs.
    struct Notifications {
        center: Retained<UNUserNotificationCenter>,
        /// Kept alive here: the center only holds it weakly.
        _delegate: Retained<Delegate>,
        gate: Arc<Mutex<Gate>>,
    }

    impl Backend {
        fn new(main_thread: MainThreadMarker, events: EventLoopProxy<UserEvent>) -> Self {
            // The framework raises an exception outside of a bundle.
            if NSBundle::mainBundle().bundleIdentifier().is_none() {
                log::info!("desktop notifications unavailable (not run from Graphene.app)");
                return Self {
                    main_thread,
                    notifications: None,
                };
            }
            let center = UNUserNotificationCenter::currentNotificationCenter();
            let delegate = Delegate::new(events);
            center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            Self {
                main_thread,
                notifications: Some(Notifications {
                    center,
                    _delegate: delegate,
                    gate: Arc::new(Mutex::new(Gate::default())),
                }),
            }
        }

        /// Runs `command`. Every call returns at once; the system answers later.
        pub fn run(&self, command: Command) {
            match command {
                Command::Show { pane, title, body } => {
                    if let Some(notifications) = &self.notifications {
                        notifications.show(pane, &title, &body);
                    }
                }
                Command::Close { pane } => {
                    if let Some(notifications) = &self.notifications {
                        notifications.close(pane);
                    }
                }
                Command::Badge(count) => {
                    let label = badge_label(count).map(|label| NSString::from_str(&label));
                    NSApplication::sharedApplication(self.main_thread)
                        .dockTile()
                        .setBadgeLabel(label.as_deref());
                }
            }
        }
    }

    impl Notifications {
        fn show(&self, pane: PaneId, title: &str, body: &str) {
            let shown = self
                .gate
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .show(pane, title, body);
            match shown {
                Shown::Post => post(&self.center, pane, title, body),
                Shown::Ask => self.ask(),
                Shown::Wait | Shown::Drop => {}
            }
        }

        fn close(&self, pane: PaneId) {
            self.gate
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .close(pane);
            let identifier = NSString::from_str(&identifier(pane));
            self.center
                .removeDeliveredNotificationsWithIdentifiers(&NSArray::from_slice(&[&*identifier]));
        }

        /// Asks for authorization; the answer shows the notifications that waited.
        fn ask(&self) {
            let gate = Arc::clone(&self.gate);
            let answer = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
                let granted = granted.as_bool();
                if !granted {
                    log::info!("desktop notifications refused");
                }
                let waiting = gate
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .answered(granted);
                if waiting.is_empty() {
                    return;
                }
                // The center can be used from any thread, like this answer's queue.
                let center = UNUserNotificationCenter::currentNotificationCenter();
                for Pending { pane, title, body } in waiting {
                    post(&center, pane, &title, &body);
                }
            });
            self.center
                .requestAuthorizationWithOptions_completionHandler(
                    UNAuthorizationOptions::Alert | UNAuthorizationOptions::Badge,
                    &answer,
                );
        }
    }

    /// Shows a notification for `pane`, replacing the one it still shows.
    fn post(center: &UNUserNotificationCenter, pane: PaneId, title: &str, body: &str) {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&identifier(pane)),
            &content,
            None,
        );
        let done = RcBlock::new(|error: *mut NSError| {
            // SAFETY: the framework passes a valid error or null.
            if let Some(error) = unsafe { error.as_ref() } {
                log::warn!(
                    "cannot show a notification: {}",
                    error.localizedDescription()
                );
            }
        });
        center.addNotificationRequest_withCompletionHandler(&request, Some(&done));
    }

    /// What the delegate needs: where to send the clicks.
    struct Ivars {
        events: EventLoopProxy<UserEvent>,
    }

    define_class!(
        // SAFETY:
        // - NSObject has no subclassing requirements.
        // - `Delegate` does not implement `Drop`.
        #[unsafe(super(NSObject))]
        #[name = "GrapheneNotificationDelegate"]
        #[ivars = Ivars]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            /// Shows the banner while Graphene is in front too: a pane of the active
            /// window may be out of sight.
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                completion_handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                completion_handler.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List,));
            }

            /// A click on the notification shows its pane.
            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                completion_handler: &block2::DynBlock<dyn Fn()>,
            ) {
                // SAFETY: a constant string of the framework.
                let default = unsafe { UNNotificationDefaultActionIdentifier };
                if response.actionIdentifier().isEqualToString(default) {
                    let identifier = response.notification().request().identifier();
                    if let Some(pane) = pane_of(&identifier.to_string()) {
                        let _ = self
                            .ivars()
                            .events
                            .send_event(UserEvent::NotificationClicked(pane));
                    }
                }
                completion_handler.call(());
            }
        }
    );

    impl Delegate {
        fn new(events: EventLoopProxy<UserEvent>) -> Retained<Self> {
            let this = Self::alloc().set_ivars(Ivars { events });
            // SAFETY: `init` of NSObject.
            unsafe { msg_send![super(this), init] }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_pane_has_its_own_identifier() {
        assert_eq!(identifier(PaneId(7)), "pane-7");
        assert_eq!(pane_of(&identifier(PaneId(7))), Some(PaneId(7)));
        assert_eq!(
            pane_of(&identifier(PaneId(u64::MAX))),
            Some(PaneId(u64::MAX))
        );
    }

    #[test]
    fn foreign_identifiers_belong_to_no_pane() {
        for foreign in [
            "", "pane-", "pane-x", "pane--1", "pane-+1", "pane-1a", "other-1",
        ] {
            assert_eq!(pane_of(foreign), None, "{foreign:?}");
        }
    }

    #[test]
    fn the_badge_is_hidden_at_zero() {
        assert_eq!(badge_label(0), None);
        assert_eq!(badge_label(5).as_deref(), Some("5"));
        assert_eq!(badge_label(120).as_deref(), Some("120"));
    }

    fn pending(pane: u64, body: &str) -> Pending {
        Pending {
            pane: PaneId(pane),
            title: "title".into(),
            body: body.into(),
        }
    }

    #[test]
    fn authorization_is_asked_at_the_first_notification_only() {
        let mut gate = Gate::default();
        assert_eq!(gate.show(PaneId(1), "title", "a"), Shown::Ask);
        assert_eq!(gate.show(PaneId(2), "title", "b"), Shown::Wait);
        assert_eq!(gate.answered(true), vec![pending(1, "a"), pending(2, "b")]);
        assert_eq!(gate.show(PaneId(1), "title", "c"), Shown::Post);
    }

    #[test]
    fn a_waiting_notification_is_replaced_or_closed() {
        let mut gate = Gate::default();
        gate.show(PaneId(1), "title", "a");
        gate.show(PaneId(2), "title", "b");
        gate.show(PaneId(1), "title", "c");
        gate.close(PaneId(2));
        assert_eq!(gate.answered(true), vec![pending(1, "c")]);
    }

    #[test]
    fn a_refusal_drops_the_notifications_for_good() {
        let mut gate = Gate::default();
        gate.show(PaneId(1), "title", "a");
        assert_eq!(gate.answered(false), Vec::new());
        assert_eq!(gate.show(PaneId(1), "title", "b"), Shown::Drop);
        assert_eq!(gate, Gate::Denied);
    }

    #[test]
    fn closing_without_a_waiting_notification_changes_nothing() {
        let mut gate = Gate::Granted;
        gate.close(PaneId(1));
        assert_eq!(gate, Gate::Granted);
    }
}
