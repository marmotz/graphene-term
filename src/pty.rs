//! PTY thread: reads the shell output into `Term` and writes the user input.
//!
//! Replaces `alacritty_terminal::event_loop::EventLoop` with the same structure (one
//! `polling::Poller`, a write queue, `Msg::{Input, Resize, Shutdown}`, child exit and
//! synchronized updates), so the parse step is ours and the byte stream can be split.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{self, ErrorKind, Read, Write};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::JoinHandle;
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::thread;
use alacritty_terminal::tty::{self, EventedPty};
use alacritty_terminal::vte::ansi::Processor;
use log::error;
use polling::{Event as PollingEvent, Events, PollMode, Poller};

/// Max bytes read from the PTY before forcing the `Term` lock.
const READ_BUFFER_SIZE: usize = 0x10_0000;

/// Max bytes parsed while holding the `Term` lock.
const MAX_LOCKED_READ: usize = u16::MAX as usize;

/// Poller keys registered by alacritty's unix `Pty` (`pub(crate)` there).
const PTY_READ_WRITE_TOKEN: usize = 0;
const PTY_CHILD_EVENT_TOKEN: usize = 1;

/// Messages sent to the PTY thread.
#[derive(Debug)]
pub enum Msg {
    /// Bytes to write to the PTY.
    Input(Cow<'static, [u8]>),
    /// New PTY size.
    Resize(WindowSize),
    /// Stop the thread.
    Shutdown,
}

/// Sends messages to the PTY thread and wakes its poller.
#[derive(Clone)]
pub struct Sender {
    sender: mpsc::Sender<Msg>,
    poller: Arc<Poller>,
}

impl Sender {
    pub fn send(&self, msg: Msg) {
        // The thread may already be gone (shell exited): nothing left to do.
        if self.sender.send(msg).is_ok() {
            let _ = self.poller.notify();
        }
    }
}

/// Parses `bytes` into `term`. Single parse step of the read path, so the stream can be
/// split around it.
pub fn feed<L: EventListener>(processor: &mut Processor, term: &mut Term<L>, bytes: &[u8]) {
    processor.advance(term, bytes);
}

/// Pending writes to the PTY, in order.
#[derive(Default)]
pub struct WriteQueue {
    pending: VecDeque<Cow<'static, [u8]>>,
    /// Bytes of the front buffer already written.
    written: usize,
}

impl WriteQueue {
    pub fn push(&mut self, bytes: Cow<'static, [u8]>) {
        if !bytes.is_empty() {
            self.pending.push_back(bytes);
        }
    }

    pub fn needs_write(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Writes as much as `writer` accepts. Stops without error when it would block or
    /// accepts nothing; the rest stays queued.
    pub fn flush(&mut self, writer: &mut impl Write) -> io::Result<()> {
        while let Some(front) = self.pending.front() {
            match writer.write(&front[self.written..]) {
                Ok(0) => break,
                Ok(n) => {
                    self.written += n;
                    if self.written >= front.len() {
                        self.pending.pop_front();
                        self.written = 0;
                    }
                }
                Err(err) => match err.kind() {
                    ErrorKind::Interrupted | ErrorKind::WouldBlock => break,
                    _ => return Err(err),
                },
            }
        }
        Ok(())
    }
}

/// The PTY thread before it is spawned.
pub struct PtyLoop<T, L: EventListener> {
    poller: Arc<Poller>,
    pty: T,
    rx: Receiver<Msg>,
    tx: mpsc::Sender<Msg>,
    term: Arc<FairMutex<Term<L>>>,
    event_proxy: L,
}

/// Mutable state of the running thread.
struct State {
    writes: WriteQueue,
    processor: Processor,
}

impl<T, L> PtyLoop<T, L>
where
    T: EventedPty + OnResize + Send + 'static,
    L: EventListener + Send + 'static,
{
    pub fn new(term: Arc<FairMutex<Term<L>>>, event_proxy: L, pty: T) -> io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        Ok(Self {
            poller: Arc::new(Poller::new()?),
            pty,
            rx,
            tx,
            term,
            event_proxy,
        })
    }

    pub fn sender(&self) -> Sender {
        Sender {
            sender: self.tx.clone(),
            poller: self.poller.clone(),
        }
    }

    pub fn spawn(mut self) -> JoinHandle<()> {
        thread::spawn_named("PTY reader", move || self.run())
    }

    fn run(&mut self) {
        let mut state = State {
            writes: WriteQueue::default(),
            processor: Processor::new(),
        };
        let mut buf = vec![0u8; READ_BUFFER_SIZE];
        let mode = PollMode::Level;
        let mut interest = PollingEvent::readable(0);

        // SAFETY: the PTY lives in `self` and is deregistered before `run` returns.
        if let Err(err) = unsafe { self.pty.register(&self.poller, interest, mode) } {
            error!("PTY registration error: {err}");
            return;
        }

        let mut events = Events::with_capacity(NonZeroUsize::new(1024).unwrap());
        let mut peeked: Option<Msg> = None;

        'event_loop: loop {
            let timeout = state
                .processor
                .sync_timeout()
                .sync_timeout()
                .map(|deadline| deadline.saturating_duration_since(Instant::now()));

            events.clear();
            if let Err(err) = self.poller.wait(&mut events, timeout) {
                if err.kind() == ErrorKind::Interrupted {
                    continue;
                }
                error!("PTY polling error: {err}");
                break;
            }

            // Synchronized update timeout: flush what the update held back.
            if events.is_empty() {
                if peeked.is_none() {
                    peeked = self.rx.try_recv().ok();
                }
                if peeked.is_none() {
                    state.processor.stop_sync(&mut *self.term.lock());
                    self.event_proxy.send_event(Event::Wakeup);
                    continue;
                }
            }

            if !self.drain_channel(&mut state, peeked.take()) {
                break;
            }

            for event in events.iter() {
                match event.key {
                    PTY_CHILD_EVENT_TOKEN => {
                        if let Some(tty::ChildEvent::Exited(status)) = self.pty.next_child_event() {
                            if let Some(status) = status {
                                self.event_proxy.send_event(Event::ChildExit(status));
                            }
                            self.term.lock().exit();
                            self.event_proxy.send_event(Event::Wakeup);
                            break 'event_loop;
                        }
                    }
                    PTY_READ_WRITE_TOKEN => {
                        if event.is_interrupt() {
                            continue;
                        }
                        if event.readable
                            && let Err(err) = self.read(&mut state, &mut buf)
                        {
                            // On Linux, reading the master side fails with EIO once the
                            // child hangs up: wait for the child exit event instead.
                            if err.raw_os_error() == Some(libc::EIO) {
                                continue;
                            }
                            error!("PTY read error: {err}");
                            break 'event_loop;
                        }
                        if event.writable
                            && let Err(err) = state.writes.flush(self.pty.writer())
                        {
                            error!("PTY write error: {err}");
                            break 'event_loop;
                        }
                    }
                    _ => (),
                }
            }

            let needs_write = state.writes.needs_write();
            if needs_write != interest.writable {
                interest.writable = needs_write;
                if let Err(err) = self.pty.reregister(&self.poller, interest, mode) {
                    error!("PTY reregistration error: {err}");
                    break;
                }
            }
        }

        let _ = self.pty.deregister(&self.poller);
    }

    /// Handles queued messages. Returns `false` on `Shutdown`.
    fn drain_channel(&mut self, state: &mut State, first: Option<Msg>) -> bool {
        let mut next = first;
        loop {
            let msg = match next.take() {
                Some(msg) => msg,
                None => match self.rx.try_recv() {
                    Ok(msg) => msg,
                    Err(TryRecvError::Empty) => return true,
                    // Every `Sender` is gone: the terminal was dropped.
                    Err(TryRecvError::Disconnected) => return false,
                },
            };
            match msg {
                Msg::Input(bytes) => state.writes.push(bytes),
                Msg::Resize(size) => self.pty.on_resize(size),
                Msg::Shutdown => return false,
            }
        }
    }

    /// Reads available PTY output and parses it into `Term`, without holding the lock
    /// for more than `MAX_LOCKED_READ` bytes.
    fn read(&mut self, state: &mut State, buf: &mut [u8]) -> io::Result<()> {
        let mut unprocessed = 0;
        let mut processed = 0;

        // Reserve the next lock for the PTY thread.
        let _lease = self.term.lease();
        let mut term = None;

        loop {
            match self.pty.reader().read(&mut buf[unprocessed..]) {
                Ok(0) if unprocessed == 0 => break,
                Ok(got) => unprocessed += got,
                Err(err) => match err.kind() {
                    ErrorKind::Interrupted | ErrorKind::WouldBlock => {
                        if unprocessed == 0 {
                            break;
                        }
                    }
                    _ => return Err(err),
                },
            }

            let term = match &mut term {
                Some(term) => term,
                None => term.insert(match self.term.try_lock_unfair() {
                    None if unprocessed >= READ_BUFFER_SIZE => self.term.lock_unfair(),
                    None => continue,
                    Some(term) => term,
                }),
            };

            feed(&mut state.processor, term, &buf[..unprocessed]);
            processed += unprocessed;
            unprocessed = 0;

            if processed >= MAX_LOCKED_READ {
                break;
            }
        }

        // Redraw unless every parsed byte is held by a synchronized update.
        if processed > 0 && state.processor.sync_bytes_count() < processed {
            self.event_proxy.send_event(Event::Wakeup);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::index::{Column, Line, Point};
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;

    fn term() -> Term<VoidListener> {
        Term::new(Config::default(), &TermSize::new(20, 5), VoidListener)
    }

    fn screen(term: &Term<VoidListener>) -> String {
        let mut text = String::new();
        for line in 0..5 {
            for column in 0..20 {
                text.push(term.grid()[Point::new(Line(line), Column(column))].c);
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn feed_matches_direct_advance() {
        let bytes = b"hello\r\n\x1b[31mred\x1b[0m\x1b[2;3Hx\x1b]0;title\x07end";

        let mut fed = term();
        let mut processor = Processor::new();
        feed(&mut processor, &mut fed, bytes);

        let mut direct = term();
        Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::new()
            .advance(&mut direct, bytes);

        assert_eq!(screen(&fed), screen(&direct));
        assert_eq!(fed.grid().cursor.point, direct.grid().cursor.point);
    }

    #[test]
    fn feed_handles_a_sequence_split_across_calls() {
        let mut split = term();
        let mut processor = Processor::new();
        feed(&mut processor, &mut split, b"ab\x1b[");
        feed(&mut processor, &mut split, b"3;5Hz");

        let mut whole = term();
        Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::new()
            .advance(&mut whole, b"ab\x1b[3;5Hz");

        assert_eq!(screen(&split), screen(&whole));
    }

    /// Accepts at most `chunk` bytes per call, then blocks after `budget` bytes.
    struct Partial {
        out: Vec<u8>,
        chunk: usize,
        budget: usize,
    }

    impl Write for Partial {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.budget == 0 {
                return Err(ErrorKind::WouldBlock.into());
            }
            let n = buf.len().min(self.chunk).min(self.budget);
            self.out.extend_from_slice(&buf[..n]);
            self.budget -= n;
            Ok(n)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn write_queue_resumes_partial_writes_in_order() {
        let mut queue = WriteQueue::default();
        queue.push(Cow::Borrowed(b"hello "));
        queue.push(Cow::Owned(b"world".to_vec()));
        let mut writer = Partial {
            out: Vec::new(),
            chunk: 4,
            budget: 7,
        };

        queue.flush(&mut writer).unwrap();
        assert_eq!(writer.out, b"hello w");
        assert!(queue.needs_write());

        writer.budget = 100;
        queue.flush(&mut writer).unwrap();
        assert_eq!(writer.out, b"hello world");
        assert!(!queue.needs_write());
    }

    #[test]
    fn write_queue_ignores_empty_input() {
        let mut queue = WriteQueue::default();
        queue.push(Cow::Borrowed(b""));
        assert!(!queue.needs_write());
    }

    #[test]
    fn write_queue_stops_when_the_writer_accepts_nothing() {
        struct Full;
        impl Write for Full {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Ok(0)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut queue = WriteQueue::default();
        queue.push(Cow::Borrowed(b"x"));
        queue.flush(&mut Full).unwrap();
        assert!(queue.needs_write());
    }

    #[test]
    fn write_queue_returns_real_errors() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("broken"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut queue = WriteQueue::default();
        queue.push(Cow::Borrowed(b"x"));
        assert!(queue.flush(&mut Broken).is_err());
        assert!(queue.needs_write());
    }

    /// Forwards events of the real PTY thread to the test.
    #[derive(Clone)]
    struct Events(mpsc::Sender<Event>);

    impl EventListener for Events {
        fn send_event(&self, event: Event) {
            let _ = self.0.send(event);
        }
    }

    #[test]
    fn real_pty_round_trip_until_child_exit() {
        use std::time::Duration;

        let options = tty::Options {
            shell: Some(tty::Shell::new(
                "sh".to_owned(),
                vec!["-c".to_owned(), "read l; printf 'got:%s' \"$l\"".to_owned()],
            )),
            ..Default::default()
        };
        let size = WindowSize {
            num_lines: 5,
            num_cols: 20,
            cell_width: 8,
            cell_height: 16,
        };
        let pty = tty::new(&options, size, 0).unwrap();
        let (tx, rx) = mpsc::channel();
        let term = Arc::new(FairMutex::new(Term::new(
            Config::default(),
            &TermSize::new(20, 5),
            Events(tx.clone()),
        )));
        let pty_loop = PtyLoop::new(term.clone(), Events(tx), pty).unwrap();
        let sender = pty_loop.sender();
        let handle = pty_loop.spawn();

        sender.send(Msg::Input(Cow::Borrowed(b"abc\n")));

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left).expect("child exit") {
                Event::ChildExit(_) => break,
                _ => continue,
            }
        }
        handle.join().unwrap();

        let term = term.lock();
        let text: String = (0..5)
            .flat_map(|line| (0..20).map(move |column| (line, column)))
            .map(|(line, column)| term.grid()[Point::new(Line(line), Column(column))].c)
            .collect();
        assert!(text.contains("got:abc"), "{text:?}");
    }
}
