//! Detection of the Konami code: up, up, down, down, left, right, left, right, B, A.

use winit::keyboard::{Key, NamedKey};

/// One key of the sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Named(NamedKey),
    Letter(char),
}

const SEQUENCE: [Step; 10] = [
    Step::Named(NamedKey::ArrowUp),
    Step::Named(NamedKey::ArrowUp),
    Step::Named(NamedKey::ArrowDown),
    Step::Named(NamedKey::ArrowDown),
    Step::Named(NamedKey::ArrowLeft),
    Step::Named(NamedKey::ArrowRight),
    Step::Named(NamedKey::ArrowLeft),
    Step::Named(NamedKey::ArrowRight),
    Step::Letter('b'),
    Step::Letter('a'),
];

/// Whether `key` is the key of `step`: letters compared ignoring case.
fn matches(step: Step, key: &Key) -> bool {
    match (step, key) {
        (Step::Named(named), Key::Named(key)) => named == *key,
        (Step::Letter(letter), Key::Character(text)) => {
            let mut chars = text.chars();
            chars
                .next()
                .is_some_and(|c| c.eq_ignore_ascii_case(&letter))
                && chars.next().is_none()
        }
        _ => false,
    }
}

/// Follows the keys typed towards the Konami code.
#[derive(Debug, Default)]
pub struct KonamiDetector {
    /// How many keys of the sequence were typed in a row.
    progress: usize,
}

impl KonamiDetector {
    /// Takes the next key typed; `true` when it completes the sequence, which then
    /// starts over. A key out of the sequence starts it over, and counts as its first
    /// key when it is one: an up after the two ups that start it leaves the last two
    /// keys typed as that start, so `↑↑↑↓↓…` still triggers.
    pub fn feed(&mut self, key: &Key) -> bool {
        if matches(SEQUENCE[self.progress], key) {
            self.progress += 1;
        } else if matches(SEQUENCE[0], key) {
            self.progress = if self.progress == 2 { 2 } else { 1 };
        } else {
            self.progress = 0;
        }
        if self.progress == SEQUENCE.len() {
            self.progress = 0;
            return true;
        }
        false
    }

    /// Starts the sequence over.
    pub fn reset(&mut self) {
        self.progress = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(key: NamedKey) -> Key {
        Key::Named(key)
    }

    fn letter(text: &str) -> Key {
        Key::Character(text.into())
    }

    fn arrows() -> Vec<Key> {
        use NamedKey::*;
        [
            ArrowUp, ArrowUp, ArrowDown, ArrowDown, ArrowLeft, ArrowRight, ArrowLeft, ArrowRight,
        ]
        .into_iter()
        .map(named)
        .collect()
    }

    /// Feeds `keys`, returns which of them completed the sequence.
    fn feed_all(detector: &mut KonamiDetector, keys: &[Key]) -> Vec<bool> {
        keys.iter().map(|key| detector.feed(key)).collect()
    }

    #[test]
    fn the_full_sequence_triggers_on_its_last_key_only() {
        let mut keys = arrows();
        keys.extend([letter("b"), letter("a")]);
        let mut detector = KonamiDetector::default();
        let fired = feed_all(&mut detector, &keys);
        assert_eq!(fired.iter().filter(|&&f| f).count(), 1);
        assert_eq!(fired.last(), Some(&true));
        // Starts over after triggering.
        assert_eq!(feed_all(&mut detector, &keys).last(), Some(&true));
    }

    #[test]
    fn an_interrupted_sequence_does_not_trigger() {
        let mut keys = arrows();
        keys.extend([letter("x"), letter("b"), letter("a")]);
        assert!(!feed_all(&mut KonamiDetector::default(), &keys).contains(&true));

        let mut keys = arrows();
        keys.insert(4, named(NamedKey::Enter));
        keys.extend([letter("b"), letter("a")]);
        assert!(!feed_all(&mut KonamiDetector::default(), &keys).contains(&true));
    }

    #[test]
    fn an_extra_up_counts_as_the_first_key() {
        for extra in 1..=3 {
            let mut keys = vec![named(NamedKey::ArrowUp); extra];
            keys.extend(arrows());
            keys.extend([letter("b"), letter("a")]);
            let fired = feed_all(&mut KonamiDetector::default(), &keys);
            assert_eq!(fired.last(), Some(&true), "{extra} extra ups");
        }
        // An up later in the sequence counts as its first key only.
        let mut keys = arrows()[..5].to_vec();
        keys.extend(arrows());
        keys.extend([letter("b"), letter("a")]);
        assert_eq!(
            feed_all(&mut KonamiDetector::default(), &keys).last(),
            Some(&true)
        );
    }

    #[test]
    fn letters_ignore_case() {
        let mut keys = arrows();
        keys.extend([letter("B"), letter("A")]);
        assert_eq!(
            feed_all(&mut KonamiDetector::default(), &keys).last(),
            Some(&true)
        );

        let mut keys = arrows();
        keys.push(letter("ba"));
        assert!(!feed_all(&mut KonamiDetector::default(), &keys).contains(&true));
    }

    #[test]
    fn reset_starts_the_sequence_over() {
        let mut detector = KonamiDetector::default();
        feed_all(&mut detector, &arrows());
        detector.reset();
        assert!(!feed_all(&mut detector, &[letter("b"), letter("a")]).contains(&true));
    }
}
