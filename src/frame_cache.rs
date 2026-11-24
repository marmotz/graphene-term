//! The frame of each pane drawn last, kept to be drawn again while the pane is
//! unchanged: a redraw caused by one pane does not capture the others again.

use std::collections::HashMap;
use std::hash::Hash;

use crate::frame::Frame;

/// Frames by pane, each with the key of what it was captured from.
pub struct FrameCache<Id, Key> {
    entries: HashMap<Id, (Option<Key>, Frame)>,
}

impl<Id, Key> Default for FrameCache<Id, Key> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

impl<Id: Copy + Eq + Hash, Key: PartialEq> FrameCache<Id, Key> {
    /// Keeps the frame of `id` when it was captured with `key`, otherwise replaces it
    /// with `capture()`. A `None` key always captures: the frame depends on something
    /// the key cannot hold (an animation). Returns whether it captured.
    pub fn update(&mut self, id: Id, key: Option<Key>, capture: impl FnOnce() -> Frame) -> bool {
        if let Some((Some(cached), _)) = self.entries.get(&id)
            && key.as_ref() == Some(cached)
        {
            return false;
        }
        self.entries.insert(id, (key, capture()));
        true
    }

    /// The frame of `id` set by the last [`Self::update`].
    pub fn get(&self, id: Id) -> Option<&Frame> {
        self.entries.get(&id).map(|(_, frame)| frame)
    }

    /// Drops the frames of the panes `keep` rejects.
    pub fn retain(&mut self, mut keep: impl FnMut(Id) -> bool) {
        self.entries.retain(|id, _| keep(*id));
    }
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::vte::ansi::Rgb;

    use super::*;
    use crate::frame::BlockColors;
    use crate::i18n::Lang;

    fn frame(background: u8) -> Frame {
        let color = Rgb {
            r: background,
            g: 0,
            b: 0,
        };
        Frame {
            rows: Vec::new(),
            decorations: Vec::new(),
            cards: Vec::new(),
            row_tops: Vec::new(),
            block_colors: BlockColors {
                success: color,
                failure: color,
                running: color,
                dim: color,
                card_tint: color,
            },
            cursor: None,
            ligature_break: None,
            background: color,
            banner: None,
            lang: Lang::En,
            images: Vec::new(),
            progress_colors: crate::progress::ProgressColors {
                normal: color,
                error: color,
                paused: color,
                track: color,
            },
            progress_thickness: 0.0,
        }
    }

    #[test]
    fn a_frame_is_captured_again_only_when_its_key_changes() {
        let mut cache = FrameCache::default();
        assert!(cache.update(1, Some(10), || frame(1)));
        assert!(!cache.update(1, Some(10), || frame(2)));
        assert_eq!(cache.get(1).unwrap().background.r, 1);
        assert!(cache.update(1, Some(11), || frame(3)));
        assert_eq!(cache.get(1).unwrap().background.r, 3);
        // Each pane has its own frame.
        assert!(cache.update(2, Some(11), || frame(4)));
        assert_eq!(cache.get(1).unwrap().background.r, 3);
    }

    #[test]
    fn a_frame_without_key_is_captured_every_time() {
        let mut cache = FrameCache::default();
        assert!(cache.update(1, None, || frame(1)));
        assert!(cache.update(1, None, || frame(2)));
        assert!(cache.update(1, Some(5), || frame(3)));
        assert!(!cache.update(1, Some(5), || frame(4)));
    }

    #[test]
    fn retain_drops_the_rejected_panes() {
        let mut cache = FrameCache::default();
        cache.update(1, Some(()), || frame(1));
        cache.update(2, Some(()), || frame(2));
        cache.retain(|id| id == 2);
        assert!(cache.get(1).is_none());
        assert!(cache.get(2).is_some());
    }
}
