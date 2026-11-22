//! What the UI thread asks of the desktop: the commands every notification backend
//! takes, and the handle that hands them over without blocking.
//!
//! On Linux, the commands go to the desktop over D-Bus ([`super::linux`]). Elsewhere,
//! or when the desktop has no notification server, they are dropped: the counters drawn
//! by Graphene itself work the same without a backend.

use std::sync::mpsc;

use winit::event_loop::EventLoopProxy;

use crate::terminal::{PaneId, UserEvent};

/// A request to the desktop notification backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Shows a notification for `pane`, replacing the one it still shows.
    Show {
        pane: PaneId,
        title: String,
        body: String,
    },
    /// Removes the notification `pane` still shows, if any.
    Close { pane: PaneId },
    /// Number of unseen notices of the window, shown on its taskbar or Dock icon; 0
    /// hides it.
    Badge(u32),
}

/// Hands commands to the backend. [`Notifier::send`] never blocks: a backend runs on
/// its own thread, or drops the commands when there is none.
#[derive(Debug, Default)]
pub struct Notifier {
    /// Where the backend reads its commands; `None` without backend.
    sender: Option<mpsc::Sender<Command>>,
}

impl Notifier {
    /// A handle dropping every command.
    pub fn null() -> Self {
        Self::default()
    }

    /// A handle whose commands are read from the returned receiver, by the thread of a
    /// backend.
    pub fn channel() -> (Self, mpsc::Receiver<Command>) {
        let (sender, receiver) = mpsc::channel();
        (
            Self {
                sender: Some(sender),
            },
            receiver,
        )
    }

    /// Hands `command` to the backend. A backend that stopped is ignored.
    pub fn send(&self, command: Command) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(command);
        }
    }
}

/// Starts the notification backend of this system. Clicks on a notification come back
/// through `proxy` as [`UserEvent::NotificationClicked`]. `action` labels the action of
/// a notification that shows its pane, on desktops that display it.
pub fn spawn(proxy: EventLoopProxy<UserEvent>, action: String) -> Notifier {
    #[cfg(target_os = "linux")]
    {
        super::linux::spawn(proxy, action)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (proxy, action);
        Notifier::null()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_channel_hands_over_the_commands_in_order() {
        let (notifier, receiver) = Notifier::channel();
        notifier.send(Command::Badge(2));
        notifier.send(Command::Close { pane: PaneId(1) });
        assert_eq!(receiver.try_recv(), Ok(Command::Badge(2)));
        assert_eq!(receiver.try_recv(), Ok(Command::Close { pane: PaneId(1) }));
    }

    #[test]
    fn a_stopped_backend_or_none_is_ignored() {
        Notifier::null().send(Command::Badge(1));
        let (notifier, receiver) = Notifier::channel();
        drop(receiver);
        notifier.send(Command::Badge(1));
    }
}
