//! Images of one terminal and their placements, shared by the PTY thread (writes) and
//! the UI thread (reads when drawing) as `Arc<Mutex<ImageStore>>`. Lock order: `Term`,
//! then the timeline, then the store.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Byte budget of the pixels of one terminal's images.
pub const BUDGET: usize = 256 * 1024 * 1024;

/// Gap, in milliseconds, of an animation frame added without one (kitty's default).
pub const DEFAULT_GAP: u32 = 40;

/// RGBA pixels of one image frame, immutable once built. Each value has its own
/// [`Pixels::uid`], so the renderer keys its textures on it: a changed frame is a new
/// `Pixels`, and a texture is dropped once no store holds its `Pixels` any more.
pub struct Pixels {
    uid: u64,
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, rows top to bottom, straight (not premultiplied)
    /// alpha, sRGB.
    pub rgba: Vec<u8>,
}

static NEXT_PIXELS_UID: AtomicU64 = AtomicU64::new(1);

impl Pixels {
    /// `None` when `rgba` is not `width * height * 4` bytes or the image is empty.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Arc<Self>> {
        let expected = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        (expected != 0 && rgba.len() == expected).then(|| {
            Arc::new(Self {
                uid: NEXT_PIXELS_UID.fetch_add(1, Ordering::Relaxed),
                width,
                height,
                rgba,
            })
        })
    }

    /// Identifies these pixels for the whole process.
    pub fn uid(&self) -> u64 {
        self.uid
    }
}

impl PartialEq for Pixels {
    fn eq(&self, other: &Self) -> bool {
        self.uid == other.uid
    }
}

impl Eq for Pixels {}

impl fmt::Debug for Pixels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pixels")
            .field("uid", &self.uid)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// Whether an animated image plays (kitty `a=a,s=`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnimationState {
    /// The shown frame only changes when the client picks another one.
    #[default]
    Stopped,
    /// Plays, and waits on the last frame for more frames instead of looping.
    Loading,
    /// Plays and loops.
    Running,
}

/// Playback of an animated image.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Animation {
    pub state: AnimationState,
    /// Loops to play before stopping, 0 to loop forever.
    pub max_loops: u32,
    /// Loops played since the animation started.
    pub current_loop: u32,
    /// When the shown frame gives way to the next one; `None` until the next
    /// [`ImageStore::advance`] schedules it.
    next_at: Option<Instant>,
}

/// An image: its frames (one for a still image) and the one shown.
#[derive(Debug, Clone)]
pub struct StoredImage {
    pub frames: Vec<Arc<Pixels>>,
    /// Milliseconds each frame stays shown, by frame; 0 for a gapless frame, skipped
    /// when playing.
    pub gaps: Vec<u32>,
    /// Index in `frames` of the frame shown.
    pub current: usize,
    /// Changes each time the image's pixels or shown frame change.
    pub generation: u64,
    pub animation: Animation,
    /// Insertion order, for eviction.
    seq: u64,
    bytes: usize,
}

impl StoredImage {
    /// The frame shown.
    pub fn frame(&self) -> &Arc<Pixels> {
        &self.frames[self.current.min(self.frames.len() - 1)]
    }

    /// The frame that follows the shown one when playing, skipping gapless frames, and
    /// whether reaching it wraps around to the first frame. `None` when there is none
    /// yet: a loading animation waits on its last frame.
    fn next_frame(&self) -> Option<(usize, bool)> {
        let len = self.frames.len();
        let mut index = self.current;
        let mut wrapped = false;
        for _ in 0..len {
            index += 1;
            if index >= len {
                if self.animation.state == AnimationState::Loading {
                    return None;
                }
                index = 0;
                wrapped = true;
            }
            if self.gaps[index] > 0 {
                return Some((index, wrapped));
            }
        }
        None
    }

    /// Whether the image plays on its own: not stopped, several frames, and at least
    /// one frame shown for some time.
    fn plays(&self) -> bool {
        self.animation.state != AnimationState::Stopped
            && self.frames.len() > 1
            && self.gaps.iter().any(|&gap| gap > 0)
    }

    fn gap(&self) -> Duration {
        Duration::from_millis(u64::from(self.gaps[self.current]))
    }

    /// Shows the next frames due at `now`. Returns whether the shown frame changed.
    fn step(&mut self, now: Instant) -> bool {
        let Some(mut at) = self.animation.next_at else {
            self.animation.next_at = Some(now + self.gap());
            return false;
        };
        if now < at {
            return false;
        }
        // Far behind (the pane was hidden, the machine slept): resume from now rather
        // than replaying every missed frame.
        let total: u64 = self.gaps.iter().map(|&gap| u64::from(gap)).sum();
        if now.duration_since(at) >= Duration::from_millis(total) {
            at = now;
        }
        let mut changed = false;
        while at <= now {
            let Some((index, wrapped)) = self.next_frame() else {
                // Waits for more frames; `at` stays due so a new frame shows at once.
                self.animation.next_at = Some(at);
                return changed;
            };
            changed |= index != self.current;
            self.current = index;
            if wrapped {
                self.animation.current_loop += 1;
                let max = self.animation.max_loops;
                if max != 0 && self.animation.current_loop >= max {
                    self.animation = Animation {
                        max_loops: max,
                        ..Animation::default()
                    };
                    return changed;
                }
            }
            at += self.gap();
        }
        self.animation.next_at = Some(at);
        changed
    }
}

/// Part of an image shown by a placement, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl SourceRect {
    /// `rect` (the whole image when `None`) clipped to a `width` x `height` image;
    /// `None` when nothing of it is left.
    pub fn resolve(rect: Option<Self>, width: u32, height: u32) -> Option<Self> {
        let rect = rect.unwrap_or(Self {
            x: 0,
            y: 0,
            width,
            height,
        });
        let x = rect.x.min(width);
        let y = rect.y.min(height);
        let clipped = Self {
            x,
            y,
            width: rect.width.min(width - x),
            height: rect.height.min(height - y),
        };
        (clipped.width > 0 && clipped.height > 0).then_some(clipped)
    }
}

/// Who writes the placeholder cells of a placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementKind {
    /// The terminal wrote them, at the cursor (kitty direct placement, sixel).
    Internal,
    /// The client prints them itself (kitty `U=1`).
    Virtual,
}

/// Where and how an image is shown: a box of `cols` x `rows` cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub cols: u32,
    pub rows: u32,
    /// Part of the image shown, the whole image when `None`.
    pub source: Option<SourceRect>,
    pub kind: PlacementKind,
    /// Stacking order requested by the client (kitty `z`), only used to select
    /// placements to delete: images are always drawn over the cell background.
    pub z: i32,
}

/// The images of one terminal and their placements.
#[derive(Debug)]
pub struct ImageStore {
    images: HashMap<u32, StoredImage>,
    /// Placements by (image id, placement id).
    placements: BTreeMap<(u32, u32), Placement>,
    /// Image ids by insertion order.
    order: BTreeMap<u64, u32>,
    bytes: usize,
    budget: usize,
    next_seq: u64,
    next_generation: u64,
    /// Next terminal-assigned id to try, counting down from the top of the range.
    next_id: u32,
}

impl Default for ImageStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageStore {
    pub fn new() -> Self {
        Self::with_budget(BUDGET)
    }

    pub fn with_budget(budget: usize) -> Self {
        Self {
            images: HashMap::new(),
            placements: BTreeMap::new(),
            order: BTreeMap::new(),
            bytes: 0,
            budget,
            next_seq: 0,
            next_generation: 1,
            next_id: u32::MAX,
        }
    }

    /// Stores image `id` with `frames`, replacing a previous image of that id and its
    /// placements. The oldest images are evicted while the budget is exceeded. Returns
    /// the ids evicted, or `None` (nothing stored) when there is no frame or the frames
    /// alone exceed the budget.
    pub fn insert(&mut self, id: u32, frames: Vec<Arc<Pixels>>) -> Option<Vec<u32>> {
        let bytes: usize = frames.iter().map(|frame| frame.rgba.len()).sum();
        if frames.is_empty() || bytes > self.budget {
            return None;
        }
        self.remove(id);
        let mut evicted = Vec::new();
        while self.bytes + bytes > self.budget {
            let Some((_, oldest)) = self.order.first_key_value() else {
                break;
            };
            let oldest = *oldest;
            self.remove(oldest);
            evicted.push(oldest);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        let generation = self.bump_generation();
        self.order.insert(seq, id);
        self.bytes += bytes;
        let gaps = (0..frames.len())
            .map(|index| if index == 0 { 0 } else { DEFAULT_GAP })
            .collect();
        self.images.insert(
            id,
            StoredImage {
                frames,
                gaps,
                current: 0,
                generation,
                animation: Animation::default(),
                seq,
                bytes,
            },
        );
        Some(evicted)
    }

    /// Evicts the oldest images other than `keep` until `extra` more bytes fit in the
    /// budget. `false` when they still do not.
    fn make_room(&mut self, extra: usize, keep: u32) -> bool {
        while self.bytes + extra > self.budget {
            let Some(oldest) = self.order.values().copied().find(|&id| id != keep) else {
                return false;
            };
            self.remove(oldest);
        }
        true
    }

    fn bump_generation(&mut self) -> u64 {
        let generation = self.next_generation;
        self.next_generation += 1;
        generation
    }

    pub fn get(&self, id: u32) -> Option<&StoredImage> {
        self.images.get(&id)
    }

    /// Shows frame `index` of image `id`; a playing animation goes on from it. `false`
    /// when there is no such frame.
    pub fn set_current_frame(&mut self, id: u32, index: usize) -> bool {
        let generation = self.next_generation;
        match self.images.get_mut(&id) {
            Some(image) if index < image.frames.len() => {
                if image.current != index {
                    image.current = index;
                    image.generation = generation;
                    self.next_generation += 1;
                }
                image.animation.next_at = None;
                true
            }
            _ => false,
        }
    }

    /// Appends a frame of `gap` milliseconds to image `id`, evicting older images if
    /// needed. Returns its index, `None` when there is no such image or the frame does
    /// not fit in the budget.
    pub fn add_frame(&mut self, id: u32, pixels: Arc<Pixels>, gap: u32) -> Option<usize> {
        if !self.images.contains_key(&id) {
            return None;
        }
        let extra = pixels.rgba.len();
        if !self.make_room(extra, id) {
            return None;
        }
        self.bytes += extra;
        let image = self.images.get_mut(&id)?;
        image.frames.push(pixels);
        image.gaps.push(gap);
        image.bytes += extra;
        Some(image.frames.len() - 1)
    }

    /// Replaces frame `index` of image `id`, and its gap when `gap` is given. `false`
    /// when there is no such frame or the new pixels do not fit in the budget.
    pub fn replace_frame(
        &mut self,
        id: u32,
        index: usize,
        pixels: Arc<Pixels>,
        gap: Option<u32>,
    ) -> bool {
        let Some(old) = self
            .images
            .get(&id)
            .and_then(|image| image.frames.get(index))
            .map(|frame| frame.rgba.len())
        else {
            return false;
        };
        let new = pixels.rgba.len();
        if new > old && !self.make_room(new - old, id) {
            return false;
        }
        let generation = self.bump_generation();
        let Some(image) = self.images.get_mut(&id) else {
            return false;
        };
        image.frames[index] = pixels;
        if let Some(gap) = gap {
            image.gaps[index] = gap;
        }
        image.bytes = image.bytes - old + new;
        image.generation = generation;
        self.bytes = self.bytes - old + new;
        true
    }

    /// Sets the gap of frame `index` of image `id`, in milliseconds (0 for gapless).
    /// `false` when there is no such frame.
    pub fn set_gap(&mut self, id: u32, index: usize, gap: u32) -> bool {
        match self.images.get_mut(&id) {
            Some(image) if index < image.gaps.len() => {
                image.gaps[index] = gap;
                true
            }
            _ => false,
        }
    }

    /// Removes frame `index` of image `id`; the frame after it takes its place. `false`
    /// when there is no such frame or it is the only one.
    pub fn remove_frame(&mut self, id: u32, index: usize) -> bool {
        let generation = self.next_generation;
        let Some(image) = self.images.get_mut(&id) else {
            return false;
        };
        if index >= image.frames.len() || image.frames.len() == 1 {
            return false;
        }
        let bytes = image.frames.remove(index).rgba.len();
        image.gaps.remove(index);
        image.bytes -= bytes;
        if image.current > index || image.current >= image.frames.len() {
            image.current = image.current.saturating_sub(1);
        }
        image.generation = generation;
        self.next_generation += 1;
        self.bytes -= bytes;
        true
    }

    /// Changes the playback of image `id`: its state when given, and its loop count
    /// (`loops` loops before stopping, 0 forever) when given. Stopping resets the loop
    /// counter. `false` when there is no such image.
    pub fn set_animation(
        &mut self,
        id: u32,
        state: Option<AnimationState>,
        loops: Option<u32>,
    ) -> bool {
        let Some(image) = self.images.get_mut(&id) else {
            return false;
        };
        let animation = &mut image.animation;
        if let Some(loops) = loops {
            animation.max_loops = loops;
        }
        if let Some(state) = state {
            if state == AnimationState::Stopped {
                animation.current_loop = 0;
                animation.next_at = None;
            } else if animation.state == AnimationState::Stopped {
                animation.next_at = None;
            }
            animation.state = state;
        }
        true
    }

    /// Whether image `id` plays on its own and is placed somewhere.
    fn animates(&self, id: u32, image: &StoredImage) -> bool {
        image.plays() && self.placements_of(id).next().is_some()
    }

    /// Shows the frames of the playing animations due at `now`, and schedules the
    /// animations that just started. Returns whether a shown frame changed.
    pub fn advance(&mut self, now: Instant) -> bool {
        let ids: Vec<u32> = self
            .images
            .iter()
            .filter(|&(&id, image)| self.animates(id, image))
            .map(|(&id, _)| id)
            .collect();
        let mut changed = false;
        for id in ids {
            let generation = self.next_generation;
            let Some(image) = self.images.get_mut(&id) else {
                continue;
            };
            if image.step(now) {
                image.generation = generation;
                self.next_generation += 1;
                changed = true;
            }
        }
        changed
    }

    /// When the next frame of a playing animation is due: `now` for an animation not
    /// scheduled yet, `None` when nothing plays. An animation waiting for more frames
    /// does not count.
    pub fn next_frame_deadline(&self, now: Instant) -> Option<Instant> {
        self.images
            .iter()
            .filter(|&(&id, image)| self.animates(id, image))
            .filter_map(|(_, image)| match image.animation.next_at {
                None => Some(now),
                Some(at) if at <= now && image.next_frame().is_none() => None,
                Some(at) => Some(at),
            })
            .min()
    }

    /// Removes image `id` and its placements. `false` when there was none.
    pub fn remove(&mut self, id: u32) -> bool {
        let Some(image) = self.images.remove(&id) else {
            return false;
        };
        self.order.remove(&image.seq);
        self.bytes -= image.bytes;
        let keys: Vec<_> = self
            .placements
            .range((id, 0)..=(id, u32::MAX))
            .map(|(key, _)| *key)
            .collect();
        for key in keys {
            self.placements.remove(&key);
        }
        true
    }

    /// Records placement `placement` of image `id`, replacing one of the same ids.
    /// `false` (nothing recorded) when there is no such image.
    pub fn place(&mut self, id: u32, placement: u32, value: Placement) -> bool {
        if !self.images.contains_key(&id) {
            return false;
        }
        self.placements.insert((id, placement), value);
        true
    }

    /// Placement `placement` of image `id`. A placeholder naming no placement (0) shows
    /// the image's placement of lowest id when there is no placement 0.
    pub fn placement(&self, id: u32, placement: u32) -> Option<&Placement> {
        self.placements.get(&(id, placement)).or_else(|| {
            (placement == 0)
                .then(|| self.placements.range((id, 0)..=(id, u32::MAX)).next())
                .flatten()
                .map(|(_, value)| value)
        })
    }

    /// Placements of image `id`, by placement id.
    pub fn placements_of(&self, id: u32) -> impl Iterator<Item = (u32, &Placement)> {
        self.placements
            .range((id, 0)..=(id, u32::MAX))
            .map(|((_, placement), value)| (*placement, value))
    }

    /// Every placement, by (image id, placement id).
    pub fn placements(&self) -> impl Iterator<Item = ((u32, u32), &Placement)> {
        self.placements.iter().map(|(key, value)| (*key, value))
    }

    /// Ids of the images stored, in no particular order.
    pub fn ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.images.keys().copied()
    }

    /// Removes placement `placement` of image `id`; `false` when there was none.
    pub fn remove_placement(&mut self, id: u32, placement: u32) -> bool {
        self.placements.remove(&(id, placement)).is_some()
    }

    /// A free id for an image the client gave none to, from the top of the 32-bit
    /// range down, so it does not collide with the ids clients pick.
    pub fn allocate_id(&mut self) -> u32 {
        loop {
            let id = self.next_id;
            self.next_id = match self.next_id.wrapping_sub(1) {
                0 => u32::MAX,
                next => next,
            };
            if !self.images.contains_key(&id) {
                return id;
            }
        }
    }

    /// Removes every image and placement (full reset).
    pub fn clear(&mut self) {
        self.images.clear();
        self.placements.clear();
        self.order.clear();
        self.bytes = 0;
    }

    /// Bytes of pixels stored.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `width` x 1 image: `width * 4` bytes.
    fn pixels(width: u32) -> Arc<Pixels> {
        Pixels::new(width, 1, vec![0; width as usize * 4]).unwrap()
    }

    fn placement(cols: u32) -> Placement {
        Placement {
            cols,
            rows: 1,
            source: None,
            kind: PlacementKind::Internal,
            z: 0,
        }
    }

    #[test]
    fn pixels_need_the_exact_byte_count() {
        assert!(Pixels::new(2, 2, vec![0; 16]).is_some());
        assert!(Pixels::new(2, 2, vec![0; 15]).is_none());
        assert!(Pixels::new(0, 2, vec![]).is_none());
        let (a, b) = (pixels(1), pixels(1));
        assert_ne!(a.uid(), b.uid());
    }

    #[test]
    fn oldest_images_are_evicted_first_when_the_budget_is_exceeded() {
        let mut store = ImageStore::with_budget(40);
        assert_eq!(store.insert(1, vec![pixels(4)]), Some(vec![]));
        assert_eq!(store.insert(2, vec![pixels(4)]), Some(vec![]));
        assert_eq!(store.insert(3, vec![pixels(2)]), Some(vec![]));
        assert_eq!(store.bytes(), 40);
        // 12 more bytes: 1 then nothing else is needed (16 freed).
        assert_eq!(store.insert(4, vec![pixels(3)]), Some(vec![1]));
        assert_eq!(store.bytes(), 36);
        // 24 more bytes: 2 and 3 go, 4 is newer and stays.
        assert_eq!(store.insert(5, vec![pixels(6)]), Some(vec![2, 3]));
        assert_eq!(store.bytes(), 36);
        assert!(store.get(4).is_some() && store.get(5).is_some());
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn an_image_larger_than_the_budget_is_refused() {
        let mut store = ImageStore::with_budget(8);
        store.insert(1, vec![pixels(1)]);
        assert_eq!(store.insert(2, vec![pixels(3)]), None);
        assert_eq!(store.insert(3, vec![]), None);
        assert!(store.get(1).is_some());
    }

    #[test]
    fn replacing_an_image_drops_its_placements_and_frees_its_bytes() {
        let mut store = ImageStore::new();
        store.insert(1, vec![pixels(4)]);
        let first = store.get(1).unwrap().generation;
        assert!(store.place(1, 3, placement(2)));
        store.insert(1, vec![pixels(2)]);
        assert_eq!(store.bytes(), 8);
        assert!(store.placement(1, 3).is_none());
        assert!(store.get(1).unwrap().generation > first);
        assert!(!store.place(9, 1, placement(1)));
    }

    #[test]
    fn placement_zero_falls_back_to_the_lowest_placement() {
        let mut store = ImageStore::new();
        store.insert(1, vec![pixels(1)]);
        store.place(1, 7, placement(7));
        store.place(1, 4, placement(4));
        assert_eq!(store.placement(1, 0).unwrap().cols, 4);
        assert_eq!(store.placement(1, 7).unwrap().cols, 7);
        assert!(store.placement(1, 5).is_none());
        store.place(1, 0, placement(1));
        assert_eq!(store.placement(1, 0).unwrap().cols, 1);
        assert_eq!(store.placements_of(1).count(), 3);
        assert!(store.remove_placement(1, 7));
        assert!(!store.remove_placement(1, 7));
    }

    #[test]
    fn clear_removes_everything() {
        let mut store = ImageStore::new();
        store.insert(1, vec![pixels(1)]);
        store.place(1, 1, placement(1));
        store.clear();
        assert!(store.is_empty());
        assert_eq!(store.bytes(), 0);
        assert!(store.placement(1, 1).is_none());
        // Insertion still works after a clear.
        assert_eq!(store.insert(2, vec![pixels(1)]), Some(vec![]));
    }

    #[test]
    fn terminal_ids_count_down_from_the_top_and_skip_used_ones() {
        let mut store = ImageStore::new();
        store.insert(u32::MAX - 1, vec![pixels(1)]);
        assert_eq!(store.allocate_id(), u32::MAX);
        assert_eq!(store.allocate_id(), u32::MAX - 2);
    }

    #[test]
    fn showing_another_frame_changes_the_generation() {
        let mut store = ImageStore::new();
        store.insert(1, vec![pixels(1), pixels(2)]);
        let before = store.get(1).unwrap().generation;
        assert!(store.set_current_frame(1, 1));
        let image = store.get(1).unwrap();
        assert!(image.generation > before);
        assert_eq!(image.frame().width, 2);
        assert!(!store.set_current_frame(1, 2));
    }

    /// Image 1 placed once, with frames of `gaps` milliseconds (frame `i` is `i + 1`
    /// pixels wide).
    fn animated(gaps: &[u32]) -> ImageStore {
        let mut store = ImageStore::new();
        store.insert(1, vec![pixels(1)]);
        store.set_gap(1, 0, gaps[0]);
        for (index, &gap) in gaps.iter().enumerate().skip(1) {
            store.add_frame(1, pixels(index as u32 + 1), gap);
        }
        store.place(1, 1, placement(1));
        store
    }

    fn ms(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    fn shown(store: &ImageStore) -> usize {
        store.get(1).unwrap().current
    }

    #[test]
    fn frames_are_added_replaced_and_removed_with_their_bytes() {
        let mut store = ImageStore::new();
        store.insert(1, vec![pixels(1)]);
        assert_eq!(store.get(1).unwrap().gaps, [0]);
        assert_eq!(store.add_frame(1, pixels(2), DEFAULT_GAP), Some(1));
        assert_eq!(store.add_frame(1, pixels(3), 7), Some(2));
        assert_eq!(store.add_frame(9, pixels(1), 7), None);
        assert_eq!(store.bytes(), 24);
        assert_eq!(store.get(1).unwrap().gaps, [0, DEFAULT_GAP, 7]);

        let before = store.get(1).unwrap().generation;
        assert!(store.replace_frame(1, 1, pixels(4), None));
        assert!(store.replace_frame(1, 2, pixels(1), Some(9)));
        assert!(!store.replace_frame(1, 3, pixels(1), None));
        let image = store.get(1).unwrap();
        assert!(image.generation > before);
        assert_eq!(image.gaps, [0, DEFAULT_GAP, 9]);
        assert_eq!(store.bytes(), 24);

        store.set_current_frame(1, 2);
        assert!(store.remove_frame(1, 0));
        let image = store.get(1).unwrap();
        assert_eq!(image.frames.len(), 2);
        assert_eq!(image.current, 1);
        assert_eq!(image.frame().width, 1);
        assert_eq!(store.bytes(), 20);
        assert!(store.remove_frame(1, 1));
        assert_eq!(store.get(1).unwrap().current, 0);
        // The last frame stays.
        assert!(!store.remove_frame(1, 0));
    }

    #[test]
    fn a_new_frame_evicts_older_images_but_never_its_own() {
        let mut store = ImageStore::with_budget(12);
        store.insert(1, vec![pixels(1)]);
        store.insert(2, vec![pixels(1)]);
        assert_eq!(store.add_frame(2, pixels(1), 5), Some(1));
        assert!(store.get(1).is_some());
        assert_eq!(store.add_frame(2, pixels(1), 5), Some(2));
        assert!(store.get(1).is_none());
        assert_eq!(store.add_frame(2, pixels(1), 5), None);
        assert!(!store.replace_frame(2, 0, pixels(2), None));
        assert_eq!(store.bytes(), 12);
    }

    #[test]
    fn a_running_animation_shows_each_frame_for_its_gap_and_loops() {
        let mut store = animated(&[100, 50, 30]);
        let start = Instant::now();
        // Stopped: nothing plays.
        assert!(!store.advance(start));
        assert_eq!(store.next_frame_deadline(start), None);

        store.set_animation(1, Some(AnimationState::Running), None);
        assert_eq!(store.next_frame_deadline(start), Some(start));
        assert!(!store.advance(start));
        assert_eq!(store.next_frame_deadline(start), Some(ms(start, 100)));
        assert!(!store.advance(ms(start, 99)));
        assert!(store.advance(ms(start, 100)));
        assert_eq!(shown(&store), 1);
        assert_eq!(store.next_frame_deadline(start), Some(ms(start, 150)));
        assert!(store.advance(ms(start, 150)));
        assert_eq!(shown(&store), 2);
        // Back to the first frame: one loop played.
        assert!(store.advance(ms(start, 185)));
        assert_eq!(shown(&store), 0);
        assert_eq!(store.get(1).unwrap().animation.current_loop, 1);
        assert_eq!(store.next_frame_deadline(start), Some(ms(start, 280)));
    }

    #[test]
    fn several_due_frames_are_shown_at_once_and_a_long_pause_resumes_from_now() {
        let mut store = animated(&[10, 10, 10]);
        let start = Instant::now();
        store.set_animation(1, Some(AnimationState::Running), None);
        store.advance(start);
        assert!(store.advance(ms(start, 25)));
        assert_eq!(shown(&store), 2);
        assert_eq!(store.next_frame_deadline(start), Some(ms(start, 30)));
        // Far behind: one step, then the gap from now.
        assert!(store.advance(ms(start, 10_000)));
        assert_eq!(shown(&store), 0);
        assert_eq!(store.next_frame_deadline(start), Some(ms(start, 10_010)));
    }

    #[test]
    fn gapless_frames_are_skipped() {
        // The root frame has no gap by default.
        let mut store = animated(&[0, 20, 0, 20]);
        let start = Instant::now();
        store.set_animation(1, Some(AnimationState::Running), None);
        store.advance(start);
        assert_eq!(store.next_frame_deadline(start), Some(start));
        assert!(store.advance(start));
        assert_eq!(shown(&store), 1);
        assert!(store.advance(ms(start, 20)));
        assert_eq!(shown(&store), 3);
        assert!(store.advance(ms(start, 40)));
        assert_eq!(shown(&store), 1);

        // No frame shown for some time: nothing plays.
        let mut store = animated(&[0, 0]);
        store.set_animation(1, Some(AnimationState::Running), None);
        assert!(!store.advance(start));
        assert_eq!(store.next_frame_deadline(start), None);
    }

    #[test]
    fn a_loop_count_stops_the_animation_on_the_first_frame() {
        let mut store = animated(&[10, 10]);
        let start = Instant::now();
        // Two loops: `v=3`.
        store.set_animation(1, Some(AnimationState::Running), Some(2));
        store.advance(start);
        for (at, frame) in [(10, 1), (20, 0), (30, 1)] {
            store.advance(ms(start, at));
            assert_eq!(shown(&store), frame, "at {at} ms");
        }
        store.advance(ms(start, 40));
        let image = store.get(1).unwrap();
        assert_eq!(image.current, 0);
        assert_eq!(image.animation.state, AnimationState::Stopped);
        assert_eq!(image.animation.current_loop, 0);
        assert_eq!(store.next_frame_deadline(ms(start, 40)), None);
        assert!(!store.advance(ms(start, 100)));
    }

    #[test]
    fn a_loading_animation_waits_on_its_last_frame_for_more() {
        let mut store = animated(&[10, 10]);
        let start = Instant::now();
        store.set_animation(1, Some(AnimationState::Loading), None);
        store.advance(start);
        assert!(store.advance(ms(start, 10)));
        assert_eq!(shown(&store), 1);
        assert!(!store.advance(ms(start, 20)));
        assert_eq!(shown(&store), 1);
        assert_eq!(store.next_frame_deadline(ms(start, 20)), None);

        // A new frame shows as soon as it arrives.
        store.add_frame(1, pixels(3), 10);
        assert_eq!(
            store.next_frame_deadline(ms(start, 25)),
            Some(ms(start, 20))
        );
        assert!(store.advance(ms(start, 25)));
        assert_eq!(shown(&store), 2);
    }

    #[test]
    fn stopping_keeps_the_frame_and_resets_the_loop_counter() {
        let mut store = animated(&[10, 10]);
        let start = Instant::now();
        store.set_animation(1, Some(AnimationState::Running), None);
        store.advance(start);
        store.advance(ms(start, 20));
        assert_eq!(store.get(1).unwrap().animation.current_loop, 1);
        store.set_animation(1, Some(AnimationState::Stopped), None);
        assert!(!store.advance(ms(start, 1000)));
        let image = store.get(1).unwrap();
        assert_eq!((image.current, image.animation.current_loop), (0, 0));
        assert!(!store.set_animation(9, None, None));
    }

    #[test]
    fn picking_a_frame_restarts_its_gap_and_an_unplaced_image_does_not_play() {
        let mut store = animated(&[10, 30]);
        let start = Instant::now();
        store.set_animation(1, Some(AnimationState::Running), None);
        store.advance(start);
        store.set_current_frame(1, 1);
        assert_eq!(store.next_frame_deadline(ms(start, 5)), Some(ms(start, 5)));
        store.advance(ms(start, 5));
        assert_eq!(store.next_frame_deadline(ms(start, 5)), Some(ms(start, 35)));

        store.remove_placement(1, 1);
        assert_eq!(store.next_frame_deadline(start), None);
        assert!(!store.advance(ms(start, 1000)));
    }

    #[test]
    fn source_rect_is_clipped_to_the_image() {
        assert_eq!(
            SourceRect::resolve(None, 4, 3),
            Some(SourceRect {
                x: 0,
                y: 0,
                width: 4,
                height: 3
            })
        );
        let rect = SourceRect {
            x: 2,
            y: 1,
            width: 10,
            height: 1,
        };
        assert_eq!(
            SourceRect::resolve(Some(rect), 4, 3),
            Some(SourceRect {
                x: 2,
                y: 1,
                width: 2,
                height: 1
            })
        );
        let outside = SourceRect { x: 4, ..rect };
        assert_eq!(SourceRect::resolve(Some(outside), 4, 3), None);
    }
}
