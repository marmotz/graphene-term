//! Startup timing logs, silent by default: `RUST_LOG=graphene::perf=debug` prints the
//! time of each startup step since the start of `main`.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// `log` target of the timing lines.
pub const TARGET: &str = "graphene::perf";

static START: OnceLock<Instant> = OnceLock::new();

/// Starts the clock. Later calls keep the first instant.
pub fn start() {
    START.get_or_init(Instant::now);
}

/// Logs `step` with the time elapsed since [`start`].
pub fn mark(step: &str) {
    if log::log_enabled!(target: TARGET, log::Level::Debug) {
        let elapsed = START.get_or_init(Instant::now).elapsed();
        log::debug!(target: TARGET, "{}", format_mark(step, elapsed));
    }
}

fn format_mark(step: &str, elapsed: Duration) -> String {
    format!("{step}: {:.1} ms", elapsed.as_secs_f64() * 1000.0)
}

/// Tracks the first frame drawn after the first PTY output, so it is logged once.
#[derive(Debug, Default)]
pub struct FirstFrame {
    state: FirstFrameState,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum FirstFrameState {
    #[default]
    WaitingOutput,
    WaitingFrame,
    Done,
}

impl FirstFrame {
    /// Records PTY output. Returns `true` for the first one.
    pub fn output(&mut self) -> bool {
        let first = self.state == FirstFrameState::WaitingOutput;
        if first {
            self.state = FirstFrameState::WaitingFrame;
        }
        first
    }

    /// Records a drawn frame. Returns `true` for the first one after the first output.
    pub fn frame_drawn(&mut self) -> bool {
        let first = self.state == FirstFrameState::WaitingFrame;
        if first {
            self.state = FirstFrameState::Done;
        }
        first
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_format_is_milliseconds_with_one_decimal() {
        assert_eq!(
            format_mark("shell spawned", Duration::from_micros(337_420)),
            "shell spawned: 337.4 ms"
        );
    }

    #[test]
    fn frames_before_the_first_output_are_ignored() {
        let mut first = FirstFrame::default();
        assert!(!first.frame_drawn());
        assert!(first.output());
        assert!(first.frame_drawn());
    }

    #[test]
    fn first_output_and_frame_are_reported_once() {
        let mut first = FirstFrame::default();
        assert!(first.output());
        assert!(!first.output());
        assert!(first.frame_drawn());
        assert!(!first.frame_drawn());
        assert!(!first.output());
    }
}
