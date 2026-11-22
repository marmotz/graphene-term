//! Animation of the fold chevrons. When the fold of a shown block changes, whatever
//! changed it (a chevron click, a shortcut, a menu), its chevron goes from the old fold
//! to the new one over [`DURATION`]: a V pointing down straightens into a line, then
//! bends into a V pointing up, or back.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::frame::{Card, Fold};
use crate::timeline::BlockId;
use crate::view::ViewRow;

/// How long a chevron takes to go from one fold to the other.
pub const DURATION: Duration = Duration::from_millis(220);

/// Time between two frames of a running animation.
const FRAME: Duration = Duration::from_millis(16);

/// Fold shown by a chevron, and when it last changed while shown.
#[derive(Debug, Clone, Copy)]
struct Shown {
    fold: Fold,
    changed: Option<Instant>,
}

/// Fold chevrons of one pane's last frame.
#[derive(Debug, Default)]
pub struct FoldAnimations {
    shown: HashMap<BlockId, Shown>,
}

impl FoldAnimations {
    /// Sets the fold progress of the chevrons of `cards`, over the rows `view`, at
    /// `now`: a chevron whose fold changed since the last frame starts its animation.
    /// A chevron shown for the first time, or again after leaving the view, does not
    /// animate. Forgets the chevrons not shown.
    pub fn apply(&mut self, cards: &mut [Card], view: &[ViewRow], now: Instant) {
        let mut shown = HashMap::with_capacity(cards.len());
        for card in cards {
            let (Some(fold), Some(id)) = (card.chevron, view[card.rows.start].block) else {
                continue;
            };
            let changed = match self.shown.get(&id) {
                Some(last) if last.fold != fold => Some(now),
                Some(last) => last.changed.filter(|&at| now - at < DURATION),
                None => None,
            };
            card.fold_progress = changed.map_or(1.0, |at| {
                ((now - at).as_secs_f32() / DURATION.as_secs_f32()).min(1.0)
            });
            shown.insert(id, Shown { fold, changed });
        }
        self.shown = shown;
    }

    /// When the next frame of a running animation is due, `None` when none runs.
    pub fn deadline(&self, now: Instant) -> Option<Instant> {
        self.shown
            .values()
            .filter_map(|shown| shown.changed)
            .any(|at| now - at < DURATION)
            .then(|| now + FRAME)
    }
}

/// Bend of a chevron showing `fold` at `progress` (see [`Card::fold_progress`]): 1 for
/// a V pointing down (unfolded), -1 for a V pointing up (folded), 0 for the straight
/// line halfway, eased in and out.
pub fn bend(fold: Fold, progress: f32) -> f32 {
    let t = progress.clamp(0.0, 1.0);
    let eased = t * t * (3.0 - 2.0 * t);
    let target = match fold {
        Fold::Unfolded => 1.0,
        Fold::Folded => -1.0,
    };
    target * (2.0 * eased - 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::index::Line;

    use crate::view::RowSource;

    fn view() -> Vec<ViewRow> {
        (0..2)
            .map(|i| ViewRow {
                source: RowSource::Live,
                block: Some(BlockId(i)),
                line: Line(0),
                hidden: 0,
                top: 2 * i as u32,
                gap_before: false,
            })
            .collect()
    }

    fn cards(first: Fold, second: Fold) -> Vec<Card> {
        [first, second]
            .into_iter()
            .enumerate()
            .map(|(i, fold)| Card {
                rows: i..i + 1,
                clipped_top: false,
                status: None,
                chevron: Some(fold),
                fold_progress: 1.0,
                selected: false,
                scroll: None,
                progress: None,
            })
            .collect()
    }

    fn progress(cards: &[Card]) -> Vec<f32> {
        cards.iter().map(|card| card.fold_progress).collect()
    }

    #[test]
    fn a_fold_change_animates_its_chevron_only() {
        let mut animations = FoldAnimations::default();
        let start = Instant::now();
        let mut shown = cards(Fold::Unfolded, Fold::Unfolded);
        animations.apply(&mut shown, &view(), start);
        // Shown for the first time: settled, nothing runs.
        assert_eq!(progress(&shown), [1.0, 1.0]);
        assert_eq!(animations.deadline(start), None);

        let mut shown = cards(Fold::Folded, Fold::Unfolded);
        animations.apply(&mut shown, &view(), start);
        assert_eq!(progress(&shown), [0.0, 1.0]);
        assert_eq!(animations.deadline(start), Some(start + FRAME));

        let half = start + DURATION / 2;
        let mut shown = cards(Fold::Folded, Fold::Unfolded);
        animations.apply(&mut shown, &view(), half);
        assert!((shown[0].fold_progress - 0.5).abs() < 1e-3);
        assert_eq!(animations.deadline(half), Some(half + FRAME));

        let end = start + DURATION;
        let mut shown = cards(Fold::Folded, Fold::Unfolded);
        animations.apply(&mut shown, &view(), end);
        assert_eq!(progress(&shown), [1.0, 1.0]);
        assert_eq!(animations.deadline(end), None);
    }

    #[test]
    fn a_chevron_back_in_the_view_does_not_animate() {
        let mut animations = FoldAnimations::default();
        let now = Instant::now();
        animations.apply(&mut cards(Fold::Unfolded, Fold::Unfolded), &view(), now);
        // Block 0 leaves the view, is folded meanwhile, then comes back.
        let mut only_second = cards(Fold::Unfolded, Fold::Unfolded);
        only_second[0].chevron = None;
        animations.apply(&mut only_second, &view(), now);
        let mut shown = cards(Fold::Folded, Fold::Unfolded);
        animations.apply(&mut shown, &view(), now);
        assert_eq!(progress(&shown), [1.0, 1.0]);
    }

    #[test]
    fn bend_goes_through_a_straight_line() {
        assert_eq!(bend(Fold::Unfolded, 1.0), 1.0);
        assert_eq!(bend(Fold::Folded, 1.0), -1.0);
        // Just changed: still the other fold's V.
        assert_eq!(bend(Fold::Folded, 0.0), 1.0);
        assert_eq!(bend(Fold::Unfolded, 0.0), -1.0);
        assert_eq!(bend(Fold::Folded, 0.5), 0.0);
        assert_eq!(bend(Fold::Unfolded, 0.5), 0.0);
        assert!(bend(Fold::Folded, 0.25) > 0.0);
        assert!(bend(Fold::Folded, 0.75) < 0.0);
    }
}
