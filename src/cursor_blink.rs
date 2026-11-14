//! Blink phase of the text cursor.
//!
//! The cursor blinks when its style asks for it (`cursor.blinking` or DECSCUSR) and the
//! window has the focus. The phase is derived from the time elapsed since the last
//! reset, so a key press or a paste shows the cursor again for a full interval.

use std::time::{Duration, Instant};

/// Time between two phase toggles.
pub const BLINK_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy)]
pub struct CursorBlink {
    /// Start of the current visible phase.
    epoch: Instant,
    /// Phase last computed by [`Self::update`] or [`Self::reset`].
    visible: bool,
}

impl CursorBlink {
    pub fn new(now: Instant) -> Self {
        Self {
            epoch: now,
            visible: true,
        }
    }

    /// Whether the cursor is shown in the current phase.
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Restarts the cycle on a visible phase. Returns whether the phase changed.
    pub fn reset(&mut self, now: Instant) -> bool {
        self.epoch = now;
        let changed = !self.visible;
        self.visible = true;
        changed
    }

    /// Updates the phase at `now`. `active` is whether the cursor blinks at all: its
    /// style blinks and the window has the focus; when it does not, the cursor stays
    /// visible. Returns whether the phase changed and when the next toggle is due
    /// (`None` while the cursor does not blink).
    pub fn update(&mut self, now: Instant, active: bool) -> (bool, Option<Instant>) {
        if !active {
            // The next blink starts on a full visible phase.
            return (self.reset(now), None);
        }
        let interval = BLINK_INTERVAL.as_nanos();
        let ticks = now.saturating_duration_since(self.epoch).as_nanos() / interval;
        let visible = ticks.is_multiple_of(2);
        let changed = visible != self.visible;
        self.visible = visible;
        let next = self.epoch + BLINK_INTERVAL * (ticks as u32 + 1);
        (changed, Some(next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn starts_visible() {
        let blink = CursorBlink::new(Instant::now());
        assert!(blink.visible());
    }

    #[test]
    fn toggles_every_interval() {
        let start = Instant::now();
        let mut blink = CursorBlink::new(start);
        assert_eq!(
            blink.update(start + 100 * MS, true),
            (false, Some(start + 500 * MS))
        );
        assert!(blink.visible());
        assert_eq!(
            blink.update(start + 500 * MS, true),
            (true, Some(start + 1000 * MS))
        );
        assert!(!blink.visible());
        assert_eq!(
            blink.update(start + 999 * MS, true),
            (false, Some(start + 1000 * MS))
        );
        assert_eq!(
            blink.update(start + 1000 * MS, true),
            (true, Some(start + 1500 * MS))
        );
        assert!(blink.visible());
    }

    #[test]
    fn reset_shows_the_cursor_for_a_full_interval() {
        let start = Instant::now();
        let mut blink = CursorBlink::new(start);
        blink.update(start + 600 * MS, true);
        assert!(!blink.visible());

        let typed = start + 700 * MS;
        assert!(blink.reset(typed));
        assert!(blink.visible());
        assert_eq!(
            blink.update(start + 1100 * MS, true),
            (false, Some(typed + 500 * MS))
        );
        assert!(blink.visible());
        // Resetting while visible changes nothing.
        assert!(!blink.reset(start + 1150 * MS));
    }

    #[test]
    fn inactive_keeps_the_cursor_visible_without_deadline() {
        let start = Instant::now();
        let mut blink = CursorBlink::new(start);
        blink.update(start + 600 * MS, true);
        assert!(!blink.visible());

        assert_eq!(blink.update(start + 700 * MS, false), (true, None));
        assert!(blink.visible());
        assert_eq!(blink.update(start + 2000 * MS, false), (false, None));
        // Blinking again starts on a full visible phase.
        assert_eq!(
            blink.update(start + 2100 * MS, true),
            (false, Some(start + 2500 * MS))
        );
    }
}
