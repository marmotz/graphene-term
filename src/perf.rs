//! Timing logs, silent by default: `RUST_LOG=graphene::perf=debug` prints the time of
//! each startup step since the start of `main`, and the mean time of the redraws.

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

/// Redraws averaged in one [`RedrawStats`] line.
const REDRAWS_PER_LINE: u32 = 120;

/// Time spent in the redraws of a window, logged every [`REDRAWS_PER_LINE`] redraws.
#[derive(Debug, Default)]
pub struct RedrawStats {
    count: u32,
    total: Duration,
    longest: Duration,
}

impl RedrawStats {
    /// Records a redraw that took `took`. Returns the line to log when it completes a
    /// series, and starts the next one.
    pub fn record(&mut self, took: Duration) -> Option<String> {
        self.count += 1;
        self.total += took;
        self.longest = self.longest.max(took);
        if self.count < REDRAWS_PER_LINE {
            return None;
        }
        let line = format!(
            "{} redraws: mean {:.2} ms, longest {:.2} ms",
            self.count,
            self.total.as_secs_f64() * 1000.0 / f64::from(self.count),
            self.longest.as_secs_f64() * 1000.0
        );
        *self = Self::default();
        Some(line)
    }

    /// Records a redraw started at `start`, and logs the series it completes.
    pub fn redraw_done(&mut self, start: Instant) {
        if log::log_enabled!(target: TARGET, log::Level::Debug)
            && let Some(line) = self.record(start.elapsed())
        {
            log::debug!(target: TARGET, "{line}");
        }
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
    fn redraw_stats_report_each_series_then_start_over() {
        let mut stats = RedrawStats::default();
        for _ in 1..REDRAWS_PER_LINE - 1 {
            assert_eq!(stats.record(Duration::from_millis(1)), None);
        }
        assert_eq!(stats.record(Duration::from_millis(3)), None);
        assert_eq!(
            stats.record(Duration::from_millis(1)).as_deref(),
            Some("120 redraws: mean 1.02 ms, longest 3.00 ms")
        );
        assert_eq!(stats.record(Duration::from_millis(1)), None);
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
