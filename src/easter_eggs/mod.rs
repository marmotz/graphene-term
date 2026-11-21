//! Hidden features: secret command palette entries, the ASCII drawing they open, and
//! the hidden theme.

use std::time::{Duration, Instant};

pub mod art;
pub mod secrets;

/// The hidden theme, unlocked from the settings.
pub const HIDDEN_THEME: &str = "phosphor-crt";

/// Whether the theme `name` covers the panes with scanlines: only the hidden one.
pub fn shows_scanlines(name: &str) -> bool {
    name == HIDDEN_THEME
}

/// How many clicks on the version unlock the hidden theme.
const UNLOCK_CLICKS: usize = 7;

/// The time within which those clicks must all land.
const UNLOCK_WINDOW: Duration = Duration::from_secs(2);

/// Keeps the `clicks` of the last 2 seconds before `now`; `true` once at least 7 are
/// left.
pub fn rapid_clicks(clicks: &mut Vec<Instant>, now: Instant) -> bool {
    clicks.retain(|click| now.saturating_duration_since(*click) < UNLOCK_WINDOW);
    clicks.len() >= UNLOCK_CLICKS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_hidden_theme_shows_scanlines() {
        assert!(shows_scanlines(HIDDEN_THEME));
        for (name, _) in crate::theme::BUILT_IN {
            assert!(!shows_scanlines(name), "{name}");
        }
    }

    #[test]
    fn the_hidden_theme_is_a_hidden_built_in_theme() {
        assert!(crate::theme::is_hidden(HIDDEN_THEME));
        assert!(crate::theme::built_in(HIDDEN_THEME).is_some());
    }

    /// Clicks at `start + offset` milliseconds, fed one by one; the last result.
    fn clicked(start: Instant, offsets: &[u64]) -> (bool, usize) {
        let mut clicks = Vec::new();
        let mut unlocked = false;
        for offset in offsets {
            let now = start + Duration::from_millis(*offset);
            clicks.push(now);
            unlocked = rapid_clicks(&mut clicks, now);
        }
        (unlocked, clicks.len())
    }

    #[test]
    fn six_quick_clicks_unlock_nothing() {
        let (unlocked, _) = clicked(Instant::now(), &[0, 100, 200, 300, 400, 500]);
        assert!(!unlocked);
    }

    #[test]
    fn seven_clicks_within_two_seconds_unlock() {
        let (unlocked, _) = clicked(Instant::now(), &[0, 100, 200, 300, 400, 500, 1900]);
        assert!(unlocked);
    }

    #[test]
    fn seven_spread_clicks_unlock_nothing_and_old_ones_are_dropped() {
        let (unlocked, left) = clicked(Instant::now(), &[0, 400, 800, 1200, 1600, 2000, 2400]);
        assert!(!unlocked);
        assert_eq!(left, 5);
    }
}
