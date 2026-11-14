//! Images of one terminal and their placements, shared by the PTY thread (writes) and
//! the UI thread (reads when drawing) as `Arc<Mutex<ImageStore>>`. Lock order: `Term`,
//! then the timeline, then the store.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Byte budget of the pixels of one terminal's images.
pub const BUDGET: usize = 256 * 1024 * 1024;

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

/// An image: its frames (one for a still image) and the one shown.
#[derive(Debug, Clone)]
pub struct StoredImage {
    pub frames: Vec<Arc<Pixels>>,
    /// Index in `frames` of the frame shown.
    pub current: usize,
    /// Changes each time the image's pixels or shown frame change.
    pub generation: u64,
    /// Insertion order, for eviction.
    seq: u64,
    bytes: usize,
}

impl StoredImage {
    /// The frame shown.
    pub fn frame(&self) -> &Arc<Pixels> {
        &self.frames[self.current.min(self.frames.len() - 1)]
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
        self.images.insert(
            id,
            StoredImage {
                frames,
                current: 0,
                generation,
                seq,
                bytes,
            },
        );
        Some(evicted)
    }

    fn bump_generation(&mut self) -> u64 {
        let generation = self.next_generation;
        self.next_generation += 1;
        generation
    }

    pub fn get(&self, id: u32) -> Option<&StoredImage> {
        self.images.get(&id)
    }

    /// Shows frame `index` of image `id`. `false` when there is no such frame.
    pub fn set_current_frame(&mut self, id: u32, index: usize) -> bool {
        let generation = self.next_generation;
        match self.images.get_mut(&id) {
            Some(image) if index < image.frames.len() => {
                if image.current != index {
                    image.current = index;
                    image.generation = generation;
                    self.next_generation += 1;
                }
                true
            }
            _ => false,
        }
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
