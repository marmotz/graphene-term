//! Progress reported by programs with the ConEmu OSC 9;4 sequence:
//!
//! ```text
//! ESC ] 9 ; 4 ; <state> [; <value>] ST
//! ```
//!
//! | State | Meaning       | Value                                          |
//! | ----- | ------------- | ---------------------------------------------- |
//! | `0`   | clear         | ignored                                        |
//! | `1`   | percentage    | 0 to 100, missing = 0                          |
//! | `2`   | error         | optional: missing keeps the previous value     |
//! | `3`   | indeterminate | ignored                                        |
//! | `4`   | paused        | optional: missing keeps the previous value     |
//!
//! The value is capped at 100, a value that is not a number counts as missing, and a
//! sequence with an unknown or missing state is ignored. The sequence is found in the
//! PTY stream by [`crate::signals::Scanner`]; the resulting state lives in the
//! pane's [`crate::timeline::Timeline`].

use std::time::{Duration, Instant};

/// A progress with no new sequence for this long is dropped.
pub const PROGRESS_TIMEOUT: Duration = Duration::from_secs(15);

/// Time between two frames of the indeterminate animation (about 30 fps).
pub const INDETERMINATE_FRAME: Duration = Duration::from_millis(33);

/// Time the indeterminate segment takes to cross the bar once.
pub const INDETERMINATE_PERIOD: Duration = Duration::from_millis(1500);

/// How a progress is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressState {
    /// A percentage.
    Normal,
    /// The task failed.
    Error,
    /// The task runs, its progress is unknown.
    Indeterminate,
    /// The task is paused.
    Paused,
}

/// One OSC 9;4 sequence, as parsed by [`parse_progress`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressReport {
    /// State `0`: no progress anymore.
    Clear,
    /// Any other known state, with its value when one was given (capped at 100).
    Set {
        state: ProgressState,
        value: Option<u8>,
    },
}

/// The progress currently shown for a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub state: ProgressState,
    /// 0 to 100; meaningless for [`ProgressState::Indeterminate`].
    pub value: u8,
    /// When the last sequence was received, for the expiry.
    pub updated: Instant,
}

impl Progress {
    /// The progress after `report`, received at `now`, given the `current` one.
    pub fn apply(current: Option<Progress>, report: ProgressReport, now: Instant) -> Option<Self> {
        let ProgressReport::Set { state, value } = report else {
            return None;
        };
        let previous = current.map_or(0, |progress| progress.value);
        let value = match state {
            ProgressState::Normal => value.unwrap_or(0),
            ProgressState::Error | ProgressState::Paused => value.unwrap_or(previous),
            // The value is ignored; the previous one is kept for a later error or pause
            // without a value.
            ProgressState::Indeterminate => previous,
        };
        Some(Progress {
            state,
            value,
            updated: now,
        })
    }

    /// Whether the progress is still to be shown at `now`.
    pub fn live(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.updated) < PROGRESS_TIMEOUT
    }

    /// When the progress stops being [`live`](Self::live) without a new sequence.
    pub fn expires_at(&self) -> Instant {
        self.updated + PROGRESS_TIMEOUT
    }
}

/// Drops the progress `seen` by the window, and the `timeline`'s if it is the same,
/// once it expired at `now`; whether it did. A newer progress the timeline received
/// meanwhile is kept for the next read.
pub fn expire(seen: &mut Option<Progress>, timeline: &mut Option<Progress>, now: Instant) -> bool {
    let Some(progress) = seen.filter(|progress| !progress.live(now)) else {
        return false;
    };
    *seen = None;
    if *timeline == Some(progress) {
        *timeline = None;
    }
    true
}

/// A progress as drawn in one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressView {
    pub state: ProgressState,
    /// 0 to 100; meaningless for [`ProgressState::Indeterminate`].
    pub value: u8,
    /// Start of the animated segment along the bar, from 0 to 1; only meaningful for
    /// [`ProgressState::Indeterminate`] (see [`indeterminate_offset`]).
    pub offset: f32,
}

impl ProgressView {
    /// `progress` drawn `elapsed` after the animation's origin.
    pub fn new(progress: Progress, elapsed: Duration) -> Self {
        Self {
            state: progress.state,
            value: progress.value,
            offset: indeterminate_offset(elapsed),
        }
    }
}

/// Start of the indeterminate segment, from 0 to 1, `elapsed` after the animation's
/// origin: it crosses the bar once per [`INDETERMINATE_PERIOD`], then starts over.
pub fn indeterminate_offset(elapsed: Duration) -> f32 {
    let period = INDETERMINATE_PERIOD.as_nanos();
    let offset = (elapsed.as_nanos() % period) as f64 / period as f64;
    // Just before the end of a period, the rounding to `f32` would reach 1.
    (offset as f32).min(1.0 - f32::EPSILON)
}

/// Parses an OSC payload (what follows `ESC ]`, terminator excluded) as a progress
/// report. `None` when it is not `9;4`, or when its state is unknown or missing.
pub fn parse_progress(payload: &[u8]) -> Option<ProgressReport> {
    let rest = payload.strip_prefix(b"9;4")?;
    let rest = rest.strip_prefix(b";")?;
    let mut parts = rest.split(|&b| b == b';');
    let state = match parts.next()? {
        b"0" => return Some(ProgressReport::Clear),
        b"1" => ProgressState::Normal,
        b"2" => ProgressState::Error,
        b"3" => ProgressState::Indeterminate,
        b"4" => ProgressState::Paused,
        _ => return None,
    };
    let value = parts.next().and_then(parse_value);
    Some(ProgressReport::Set { state, value })
}

/// A decimal value capped at 100; `None` when it is not a number.
fn parse_value(bytes: &[u8]) -> Option<u8> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    // Digits only: a parse failure is an overflow, so over the cap.
    let value = std::str::from_utf8(bytes)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(u64::MAX);
    Some(value.min(100) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(state: ProgressState, value: Option<u8>) -> Option<ProgressReport> {
        Some(ProgressReport::Set { state, value })
    }

    #[test]
    fn parses_each_state() {
        assert_eq!(parse_progress(b"9;4;0"), Some(ProgressReport::Clear));
        assert_eq!(parse_progress(b"9;4;0;50"), Some(ProgressReport::Clear));
        assert_eq!(
            parse_progress(b"9;4;1;42"),
            set(ProgressState::Normal, Some(42))
        );
        assert_eq!(
            parse_progress(b"9;4;2;7"),
            set(ProgressState::Error, Some(7))
        );
        assert_eq!(
            parse_progress(b"9;4;3"),
            set(ProgressState::Indeterminate, None)
        );
        assert_eq!(parse_progress(b"9;4;4"), set(ProgressState::Paused, None));
    }

    #[test]
    fn value_is_capped_and_non_numbers_are_missing() {
        assert_eq!(
            parse_progress(b"9;4;1;100"),
            set(ProgressState::Normal, Some(100))
        );
        assert_eq!(
            parse_progress(b"9;4;1;101"),
            set(ProgressState::Normal, Some(100))
        );
        assert_eq!(
            parse_progress(b"9;4;1;99999999999999999999999"),
            set(ProgressState::Normal, Some(100))
        );
        assert_eq!(
            parse_progress(b"9;4;1;0"),
            set(ProgressState::Normal, Some(0))
        );
        for value in [
            &b"9;4;1;"[..],
            b"9;4;1;x",
            b"9;4;1;-5",
            b"9;4;1;5%",
            b"9;4;1",
        ] {
            assert_eq!(
                parse_progress(value),
                set(ProgressState::Normal, None),
                "{}",
                String::from_utf8_lossy(value)
            );
        }
        assert_eq!(
            parse_progress(b"9;4;1;30;extra"),
            set(ProgressState::Normal, Some(30))
        );
    }

    #[test]
    fn unknown_or_missing_state_is_ignored() {
        for payload in [
            &b"9;4"[..],
            b"9;4;",
            b"9;4;5",
            b"9;4;x;1",
            b"9;4;11",
            b"9;44;1",
        ] {
            assert_eq!(
                parse_progress(payload),
                None,
                "{}",
                String::from_utf8_lossy(payload)
            );
        }
    }

    #[test]
    fn other_osc_are_not_progress() {
        for payload in [&b"9;hello"[..], b"9;9;/home", b"0;title", b"777;notify;a;b"] {
            assert_eq!(parse_progress(payload), None);
        }
    }

    fn report(state: ProgressState, value: Option<u8>) -> ProgressReport {
        ProgressReport::Set { state, value }
    }

    #[test]
    fn apply_sets_and_clears() {
        let now = Instant::now();
        let progress = Progress::apply(None, report(ProgressState::Normal, Some(40)), now).unwrap();
        assert_eq!(
            progress,
            Progress {
                state: ProgressState::Normal,
                value: 40,
                updated: now
            }
        );
        assert_eq!(
            Progress::apply(Some(progress), ProgressReport::Clear, now),
            None
        );
        assert_eq!(Progress::apply(None, ProgressReport::Clear, now), None);
    }

    #[test]
    fn normal_without_value_is_zero() {
        let now = Instant::now();
        let current = Progress::apply(None, report(ProgressState::Normal, Some(70)), now);
        let progress = Progress::apply(current, report(ProgressState::Normal, None), now);
        assert_eq!(progress.unwrap().value, 0);
    }

    #[test]
    fn error_and_pause_without_value_keep_the_previous_one() {
        let now = Instant::now();
        let current = Progress::apply(None, report(ProgressState::Normal, Some(60)), now);
        for state in [ProgressState::Error, ProgressState::Paused] {
            let progress = Progress::apply(current, report(state, None), now).unwrap();
            assert_eq!((progress.state, progress.value), (state, 60));
            let progress = Progress::apply(current, report(state, Some(10)), now).unwrap();
            assert_eq!((progress.state, progress.value), (state, 10));
            let progress = Progress::apply(None, report(state, None), now).unwrap();
            assert_eq!(progress.value, 0);
        }
    }

    #[test]
    fn indeterminate_ignores_its_value() {
        let now = Instant::now();
        let current = Progress::apply(None, report(ProgressState::Normal, Some(25)), now);
        let progress =
            Progress::apply(current, report(ProgressState::Indeterminate, Some(90)), now).unwrap();
        assert_eq!(progress.state, ProgressState::Indeterminate);
        assert_eq!(progress.value, 25);
    }

    #[test]
    fn apply_refreshes_the_time() {
        let start = Instant::now();
        let later = start + Duration::from_secs(5);
        let current = Progress::apply(None, report(ProgressState::Normal, Some(1)), start);
        let progress =
            Progress::apply(current, report(ProgressState::Normal, Some(2)), later).unwrap();
        assert_eq!(progress.updated, later);
    }

    #[test]
    fn expires_after_the_timeout() {
        let start = Instant::now();
        let progress =
            Progress::apply(None, report(ProgressState::Normal, Some(1)), start).unwrap();
        assert!(progress.live(start));
        assert!(progress.live(start + PROGRESS_TIMEOUT - Duration::from_millis(1)));
        assert!(!progress.live(start + PROGRESS_TIMEOUT));
        // A clock earlier than the update keeps it live.
        assert!(progress.live(start.checked_sub(Duration::from_secs(1)).unwrap_or(start)));
        assert_eq!(progress.expires_at(), start + PROGRESS_TIMEOUT);
        assert!(!progress.live(progress.expires_at()));
    }

    #[test]
    fn indeterminate_offset_loops_over_the_period() {
        assert_eq!(indeterminate_offset(Duration::ZERO), 0.0);
        assert_eq!(indeterminate_offset(INDETERMINATE_PERIOD / 2), 0.5);
        assert_eq!(indeterminate_offset(INDETERMINATE_PERIOD), 0.0);
        let later = indeterminate_offset(INDETERMINATE_PERIOD * 3 + INDETERMINATE_PERIOD / 4);
        assert_eq!(later, 0.25);
        let offset = indeterminate_offset(INDETERMINATE_PERIOD - Duration::from_nanos(1));
        assert!((0.0..1.0).contains(&offset));
    }

    #[test]
    fn view_keeps_the_state_and_value_and_adds_the_offset() {
        let progress = Progress {
            state: ProgressState::Paused,
            value: 40,
            updated: Instant::now(),
        };
        let view = ProgressView::new(progress, INDETERMINATE_PERIOD / 2);
        assert_eq!(
            view,
            ProgressView {
                state: ProgressState::Paused,
                value: 40,
                offset: 0.5,
            }
        );
    }

    #[test]
    fn expire_drops_an_expired_progress_from_both_sides() {
        let start = Instant::now();
        let progress =
            Progress::apply(None, report(ProgressState::Normal, Some(5)), start).unwrap();
        let (mut seen, mut timeline) = (Some(progress), Some(progress));
        assert!(!expire(&mut seen, &mut timeline, start));
        assert_eq!((seen, timeline), (Some(progress), Some(progress)));
        assert!(expire(&mut seen, &mut timeline, progress.expires_at()));
        assert_eq!((seen, timeline), (None, None));
        // Nothing left to expire.
        assert!(!expire(&mut seen, &mut timeline, progress.expires_at()));
    }

    #[test]
    fn expire_keeps_a_newer_timeline_progress() {
        let start = Instant::now();
        let old = Progress::apply(None, report(ProgressState::Normal, Some(5)), start).unwrap();
        let newer = Progress::apply(
            Some(old),
            report(ProgressState::Normal, Some(6)),
            old.expires_at(),
        )
        .unwrap();
        let (mut seen, mut timeline) = (Some(old), Some(newer));
        assert!(expire(&mut seen, &mut timeline, old.expires_at()));
        assert_eq!((seen, timeline), (None, Some(newer)));
    }
}
