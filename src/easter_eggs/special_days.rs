//! Special days that decorate the window title, from the local date.

use std::time::Duration;

/// The local time now, from `libc::localtime_r`; `None` if it fails.
fn local_now() -> Option<(libc::time_t, libc::tm)> {
    // SAFETY: `time` accepts a null pointer, and `localtime_r` only writes to the `tm`
    // it is given, which lives on this stack frame.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return None;
        }
        Some((now, tm))
    }
}

/// Local (month, day) now, from `libc::localtime_r`; `None` if it fails.
pub fn local_today() -> Option<(u32, u32)> {
    let (_, tm) = local_now()?;
    Some((
        u32::try_from(tm.tm_mon + 1).ok()?,
        u32::try_from(tm.tm_mday).ok()?,
    ))
}

/// Time left until the next local midnight, daylight saving changes included.
pub fn until_midnight() -> Option<Duration> {
    let (now, mut tm) = local_now()?;
    tm.tm_mday += 1;
    tm.tm_hour = 0;
    tm.tm_min = 0;
    tm.tm_sec = 0;
    // Let `mktime` tell whether daylight saving time is in effect at midnight.
    tm.tm_isdst = -1;
    // SAFETY: `mktime` only reads and normalizes the `tm` it is given.
    let midnight = unsafe { libc::mktime(&mut tm) };
    if midnight == -1 {
        return None;
    }
    Some(Duration::from_secs(
        u64::try_from(midnight - now).ok()?.max(1),
    ))
}

/// The decoration of the special day today, `None` on an ordinary day or when the local
/// date is unknown.
pub fn today() -> Option<Decoration> {
    let (month, day) = local_today()?;
    decoration(month, day)
}

/// How a special day decorates the window title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decoration {
    /// A symbol before the title: `"🎄 <title>"`.
    Prefix(&'static str),
    /// A text after the title: `"<title> - May the fourth..."`.
    Suffix(&'static str),
}

/// Decoration of a special day, `None` on an ordinary day.
pub fn decoration(month: u32, day: u32) -> Option<Decoration> {
    match (month, day) {
        (2, 14) => Some(Decoration::Prefix("❤️")),
        (3, 14) => Some(Decoration::Prefix("π")),
        // English in every language: the pun does not translate.
        (5, 4) => Some(Decoration::Suffix("May the fourth...")),
        (10, 31) => Some(Decoration::Prefix("🎃")),
        (12, 25) => Some(Decoration::Prefix("🎄")),
        _ => None,
    }
}

/// `title` with the `decoration` of the day, or as is without one.
pub fn titled(decoration: Option<&Decoration>, title: &str) -> String {
    match decoration {
        Some(Decoration::Prefix(symbol)) => format!("{symbol} {title}"),
        Some(Decoration::Suffix(text)) => format!("{title} - {text}"),
        None => title.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_days_have_their_decoration() {
        assert_eq!(decoration(2, 14), Some(Decoration::Prefix("❤️")));
        assert_eq!(decoration(3, 14), Some(Decoration::Prefix("π")));
        assert_eq!(
            decoration(5, 4),
            Some(Decoration::Suffix("May the fourth..."))
        );
        assert_eq!(decoration(10, 31), Some(Decoration::Prefix("🎃")));
        assert_eq!(decoration(12, 25), Some(Decoration::Prefix("🎄")));
    }

    #[test]
    fn an_ordinary_day_has_no_decoration() {
        assert_eq!(decoration(6, 15), None);
        assert_eq!(decoration(4, 5), None);
    }

    #[test]
    fn a_prefix_goes_before_the_title() {
        assert_eq!(titled(Some(&Decoration::Prefix("🎄")), "vim"), "🎄 vim");
    }

    #[test]
    fn a_suffix_goes_after_the_title() {
        assert_eq!(
            titled(Some(&Decoration::Suffix("May the fourth...")), "vim"),
            "vim - May the fourth..."
        );
    }

    #[test]
    fn no_decoration_keeps_the_title() {
        assert_eq!(titled(None, "vim"), "vim");
    }

    #[test]
    fn the_local_date_and_next_midnight_are_known() {
        let (month, day) = local_today().expect("local date");
        assert!((1..=12).contains(&month));
        assert!((1..=31).contains(&day));
        let left = until_midnight().expect("next midnight");
        assert!(left > Duration::ZERO && left <= Duration::from_secs(25 * 3600));
    }
}
