//! A shell running in a PTY, with its terminal state.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty;
use winit::event_loop::EventLoopProxy;

use crate::graphics::ImageStore;
use crate::process;
use crate::pty::{self, Msg, PtyLoop};
use crate::shell_integration;
use crate::timeline::Timeline;

/// Identifies a pane, and so the terminal running in it, for the whole session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaneId(pub u64);

/// Events sent from the PTY, update check and configuration watcher threads to the UI
/// thread.
#[derive(Debug)]
pub enum UserEvent {
    /// An event from the terminal of a pane.
    Terminal(PaneId, Event),
    /// Release to show in the update banner, `None` to hide it.
    Update(Option<crate::update::Release>),
    /// `config.toml` or a theme file changed on disk.
    ConfigChanged,
    /// The system light/dark appearance, at startup and on each change.
    Appearance(crate::appearance::Appearance),
}

/// Forwards the terminal events of one pane to the winit event loop.
#[derive(Clone)]
pub struct EventProxy {
    proxy: EventLoopProxy<UserEvent>,
    pane: PaneId,
}

impl EventProxy {
    pub fn new(proxy: EventLoopProxy<UserEvent>, pane: PaneId) -> Self {
        Self { proxy, pane }
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        // The event loop may already be closed during shutdown.
        let _ = self.proxy.send_event(UserEvent::Terminal(self.pane, event));
    }
}

pub struct Terminal {
    /// The live terminal: its grid holds the current block.
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    /// The command blocks. Lock order: `term` first, then `timeline`.
    pub timeline: Arc<Mutex<Timeline>>,
    /// The images shown by the grids. Lock order: `term`, `timeline`, then `images`.
    pub images: Arc<Mutex<ImageStore>>,
    sender: pty::Sender,
    /// Process id of the shell.
    shell_pid: u32,
}

impl Terminal {
    /// Spawns the user's shell in a new PTY, in `cwd` when given (else the current
    /// directory of this process).
    pub fn spawn(
        proxy: EventProxy,
        size: WindowSize,
        config: Config,
        cwd: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let mut env = HashMap::from([
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ("TERM_PROGRAM".to_owned(), "graphene".to_owned()),
        ]);
        // The hooks put this token in their block signals: a signal printed by a program
        // carries another one.
        let token = session_token();
        // zsh, bash and fish get the shell integration; other shells start as before.
        let shell = shell_integration::prepare(&token).map(|injection| {
            env.extend(injection.env);
            tty::Shell::new(injection.program, injection.args)
        });
        let options = tty::Options {
            shell,
            env,
            working_directory: cwd,
            ..Default::default()
        };
        let pty = tty::new(&options, size, 0)?;
        let shell_pid = pty.child().id();

        let dims = TermSize::new(size.num_cols as usize, size.num_lines as usize);
        let timeline = Arc::new(Mutex::new(Timeline::new(config.scrolling_history)));
        let term = Arc::new(FairMutex::new(Term::new(config, &dims, proxy.clone())));

        let images = Arc::new(Mutex::new(ImageStore::new()));
        let pty_loop = PtyLoop::new(
            term.clone(),
            timeline.clone(),
            token,
            images.clone(),
            size,
            proxy,
            pty,
        )?;
        let sender = pty_loop.sender();
        pty_loop.spawn();

        Ok(Self {
            term,
            timeline,
            images,
            sender,
            shell_pid,
        })
    }

    /// The shell's current directory, `None` when it cannot be read.
    pub fn current_dir(&self) -> Option<PathBuf> {
        process::current_dir(self.shell_pid)
    }

    /// Name of the shell program, `None` when it cannot be read.
    pub fn shell_name(&self) -> Option<String> {
        process::program_name(self.shell_pid)
    }

    /// The program running in the foreground of this terminal, `None` when it is the
    /// shell itself or it cannot be read.
    pub fn foreground_program(&self) -> Option<String> {
        process::foreground_program(self.shell_pid)
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

/// A new session token: 32 random hex characters, so a signal printed by a program (a
/// file shown by `cat`, a remote shell) is not taken for one of this session's hooks.
fn session_token() -> String {
    let mut bytes = [0u8; 16];
    if let Err(error) = File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)) {
        // Unlikely on Linux; the clock and the process id still make it unguessable
        // enough for its purpose, which is avoiding accidental matches.
        log::warn!("no random source for the block token: {error}");
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let seed = nanos ^ (u128::from(std::process::id()) << 64);
        bytes = seed.to_le_bytes();
    }
    hex(&bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.sender.send(Msg::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_tokens_are_32_hex_characters_and_differ() {
        let (a, b) = (session_token(), session_token());
        for token in [&a, &b] {
            assert_eq!(token.len(), 32);
            assert!(token.bytes().all(|c| c.is_ascii_hexdigit()));
        }
        assert_ne!(a, b);
    }

    #[test]
    fn hex_is_lowercase_and_padded() {
        assert_eq!(hex(&[0x00, 0x0f, 0xab]), "000fab");
    }
}
