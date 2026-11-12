//! A shell running in a PTY, with its terminal state.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener, Notify, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty;
use winit::event_loop::EventLoopProxy;

/// Events sent from the PTY thread to the UI thread.
#[derive(Debug)]
pub enum UserEvent {
    Terminal(Event),
}

/// Forwards terminal events to the winit event loop.
#[derive(Clone)]
pub struct EventProxy(EventLoopProxy<UserEvent>);

impl EventProxy {
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self(proxy)
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        // The event loop may already be closed during shutdown.
        let _ = self.0.send_event(UserEvent::Terminal(event));
    }
}

pub struct Terminal {
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    notifier: Notifier,
}

impl Terminal {
    /// Spawns the user's shell in a new PTY.
    pub fn spawn(proxy: EventProxy, size: WindowSize, config: Config) -> anyhow::Result<Self> {
        let env = HashMap::from([
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ("TERM_PROGRAM".to_owned(), "graphene".to_owned()),
        ]);
        let options = tty::Options {
            env,
            ..Default::default()
        };
        let pty = tty::new(&options, size, 0)?;

        let dims = TermSize::new(size.num_cols as usize, size.num_lines as usize);
        let term = Arc::new(FairMutex::new(Term::new(config, &dims, proxy.clone())));

        let event_loop = EventLoop::new(term.clone(), proxy, pty, false, false)?;
        let notifier = Notifier(event_loop.channel());
        event_loop.spawn();

        Ok(Self { term, notifier })
    }

    /// Sends bytes to the shell.
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        self.notifier.notify(bytes);
    }

    /// Resizes both the terminal grid and the PTY.
    pub fn resize(&self, size: WindowSize) {
        let dims = TermSize::new(size.num_cols as usize, size.num_lines as usize);
        self.term.lock().resize(dims);
        let _ = self.notifier.0.send(Msg::Resize(size));
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.notifier.0.send(Msg::Shutdown);
    }
}
