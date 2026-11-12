//! A shell running in a PTY, with its terminal state.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty;
use winit::event_loop::EventLoopProxy;

use crate::blocks::ShellState;
use crate::pty::{self, Msg, PtyLoop};
use crate::shell_integration;

/// Events sent from the PTY and update check threads to the UI thread.
#[derive(Debug)]
pub enum UserEvent {
    Terminal(Event),
    /// Release to show in the update banner, `None` to hide it.
    Update(Option<crate::update::Release>),
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
    /// Shell integration state. Lock order: `term` first, then `shell`.
    pub shell: Arc<Mutex<ShellState>>,
    sender: pty::Sender,
}

impl Terminal {
    /// Spawns the user's shell in a new PTY.
    pub fn spawn(proxy: EventProxy, size: WindowSize, config: Config) -> anyhow::Result<Self> {
        let mut env = HashMap::from([
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ("TERM_PROGRAM".to_owned(), "graphene".to_owned()),
        ]);
        // zsh, bash and fish get the shell integration; other shells start as before.
        let shell = shell_integration::prepare().map(|injection| {
            env.extend(injection.env);
            tty::Shell::new(injection.program, injection.args)
        });
        let options = tty::Options {
            shell,
            env,
            ..Default::default()
        };
        let pty = tty::new(&options, size, 0)?;

        let dims = TermSize::new(size.num_cols as usize, size.num_lines as usize);
        let term = Arc::new(FairMutex::new(Term::new(config, &dims, proxy.clone())));

        let shell = Arc::new(Mutex::new(ShellState::new()));

        let pty_loop = PtyLoop::new(term.clone(), shell.clone(), proxy, pty)?;
        let sender = pty_loop.sender();
        pty_loop.spawn();

        Ok(Self {
            term,
            shell,
            sender,
        })
    }

    /// Sends bytes to the shell.
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let bytes = bytes.into();
        if !bytes.is_empty() {
            self.sender.send(Msg::Input(bytes));
        }
    }

    /// Resizes both the terminal grid and the PTY.
    pub fn resize(&self, size: WindowSize) {
        let dims = TermSize::new(size.num_cols as usize, size.num_lines as usize);
        self.term.lock().resize(dims);
        self.sender.send(Msg::Resize(size));
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.sender.send(Msg::Shutdown);
    }
}
