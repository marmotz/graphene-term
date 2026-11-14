//! Terminal graphics: kitty graphics protocol and sixel.
//!
//! `vte` 0.15 drops APC strings and `alacritty_terminal` only logs DCS, so the image
//! sequences are cut out of the PTY stream by [`sequence::Splitter`] before parsing and
//! handed to a [`Handler`] with the `Term` lock held.
//!
//! Images live in an [`ImageStore`] per terminal and are anchored in the grid by
//! placeholder cells ([`place`]), which the renderer draws the images over.

mod diacritics;
pub mod kitty;
pub mod placeholder;
pub mod sequence;
pub mod sixel;
pub mod store;

use std::sync::{Arc, Mutex};

use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::Term;
use log::debug;

use self::kitty::DEFAULT_CELL;

pub use placeholder::{CursorMove, ImageCell, erase, place};
pub use store::{
    AnimationState, DEFAULT_GAP, ImageStore, Pixels, Placement, PlacementKind, SourceRect,
};

/// An image sequence cut out of the PTY stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Image {
    /// Payload of `ESC _ G ... ESC \`, without the leading `G`.
    Kitty(Vec<u8>),
    /// Body of a sixel DCS `ESC P <params> q ... ESC \`: params, `q` and data.
    Sixel(Vec<u8>),
}

/// Receives the image sequences and full resets of the PTY stream, in stream order,
/// with the `Term` lock held and the bytes before them already applied to `term`.
pub trait Handler {
    /// An image sequence, at the current cursor position of `term`.
    fn image<L: EventListener>(&mut self, term: &mut Term<L>, image: Image);

    /// A full reset (`ESC c`) was applied to `term`.
    fn reset<L: EventListener>(&mut self, term: &mut Term<L>);
}

/// The handler of a terminal's PTY loop: runs the kitty commands against its
/// [`ImageStore`] and keeps their replies for the PTY, and places the sixel images.
#[derive(Debug, Clone)]
pub struct StoreHandler {
    store: Arc<Mutex<ImageStore>>,
    kitty: kitty::Kitty,
    /// Replies to write to the PTY, in order.
    replies: Vec<u8>,
    /// Pixel size of a cell, to size the sixel images in cells.
    cell_width: u32,
    cell_height: u32,
}

/// Placement id of every sixel image: each sixel is its own image with one placement.
pub const SIXEL_PLACEMENT: u32 = 1;

impl StoreHandler {
    pub fn new(store: Arc<Mutex<ImageStore>>) -> Self {
        Self {
            store,
            kitty: kitty::Kitty::new(),
            replies: Vec::new(),
            cell_width: DEFAULT_CELL.0,
            cell_height: DEFAULT_CELL.1,
        }
    }

    /// Pixel size of a cell, to size the images placed without a cell count. Zero sizes
    /// are ignored.
    pub fn set_cell_size(&mut self, width: u16, height: u16) {
        let (width, height) = (u32::from(width), u32::from(height));
        self.kitty.set_cell_size(width, height);
        if width > 0 && height > 0 {
            self.cell_width = width;
            self.cell_height = height;
        }
    }

    /// Decodes a sixel image and places it at the cursor of `term`, which ends on the
    /// line below the image. The pixels are padded with transparent ones to whole cells,
    /// so the image keeps its pixel size, anchored at the top-left of its cells.
    fn sixel<L: EventListener>(&mut self, term: &mut Term<L>, body: &[u8]) {
        let Some(bitmap) = sixel::decode(body) else {
            debug!("sixel sequence without image ({} bytes)", body.len());
            return;
        };
        let cols = bitmap
            .width
            .div_ceil(self.cell_width)
            .min(placeholder::MAX_CELLS);
        let rows = bitmap
            .height
            .div_ceil(self.cell_height)
            .min(placeholder::MAX_CELLS);
        let Some(pixels) = pad(bitmap, cols * self.cell_width, rows * self.cell_height) else {
            return;
        };
        let mut store = lock(&self.store);
        let id = store.allocate_id();
        if store.insert(id, vec![pixels]).is_none() {
            debug!("sixel image of {cols}x{rows} cells refused by the store");
            return;
        }
        store.place(
            id,
            SIXEL_PLACEMENT,
            Placement {
                cols,
                rows,
                source: None,
                kind: PlacementKind::Internal,
                z: 0,
            },
        );
        place(term, id, SIXEL_PLACEMENT, cols, rows, CursorMove::Below);
    }

    /// The replies to write to the PTY since the last call, `None` when there is none.
    pub fn take_replies(&mut self) -> Option<Vec<u8>> {
        (!self.replies.is_empty()).then(|| std::mem::take(&mut self.replies))
    }
}

/// `bitmap` in the top-left corner of a `width` x `height` transparent image, cropped
/// when larger.
fn pad(bitmap: sixel::Bitmap, width: u32, height: u32) -> Option<Arc<Pixels>> {
    if (width, height) == (bitmap.width, bitmap.height) {
        return Pixels::new(width, height, bitmap.rgba);
    }
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    let copy = bitmap.width.min(width) as usize * 4;
    for row in 0..bitmap.height.min(height) as usize {
        let from = row * bitmap.width as usize * 4;
        let to = row * width as usize * 4;
        rgba[to..to + copy].copy_from_slice(&bitmap.rgba[from..from + copy]);
    }
    Pixels::new(width, height, rgba)
}

/// Locks `store`, recovering it when a thread panicked while holding it: the store
/// stays consistent between calls.
pub fn lock(store: &Mutex<ImageStore>) -> std::sync::MutexGuard<'_, ImageStore> {
    store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Handler for StoreHandler {
    fn image<L: EventListener>(&mut self, term: &mut Term<L>, image: Image) {
        match image {
            Image::Kitty(payload) => {
                let mut store = lock(&self.store);
                if let Some(reply) = self.kitty.handle(term, &mut store, &payload) {
                    self.replies.extend_from_slice(&reply);
                }
            }
            Image::Sixel(body) => self.sixel(term, &body),
        }
    }

    /// A full reset cleared the grid: no cell shows an image any more.
    fn reset<L: EventListener>(&mut self, _term: &mut Term<L>) {
        lock(&self.store).clear();
        self.kitty.reset();
    }
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::index::{Column, Line, Point};
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;

    use super::*;

    fn setup(
        columns: usize,
        lines: usize,
    ) -> (Arc<Mutex<ImageStore>>, StoreHandler, Term<VoidListener>) {
        let store = Arc::new(Mutex::new(ImageStore::new()));
        let mut handler = StoreHandler::new(store.clone());
        handler.set_cell_size(10, 20);
        let term = Term::new(
            Config::default(),
            &TermSize::new(columns, lines),
            VoidListener,
        );
        (store, handler, term)
    }

    fn image_cells(term: &Term<VoidListener>, line: i32) -> Vec<Option<ImageCell>> {
        placeholder::row_cells(&term.grid()[Line(line)][..])
    }

    #[test]
    fn sixel_is_stored_padded_to_whole_cells_and_placed_at_the_cursor() {
        let (store, mut handler, mut term) = setup(10, 5);
        term.grid_mut().cursor.point = Point::new(Line(1), Column(3));
        // 15 x 30 px raster, transparent background, top-left pixel red.
        handler.image(
            &mut term,
            Image::Sixel(b"0;1q\"1;1;15;30#1;2;100;0;0@".to_vec()),
        );

        let store = lock(&store);
        let id = store.ids().next().expect("one image");
        let frame = store.get(id).unwrap().frame().clone();
        assert_eq!((frame.width, frame.height), (20, 40));
        assert_eq!(&frame.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&frame.rgba[frame.rgba.len() - 4..], &[0, 0, 0, 0]);
        let placement = store.placement(id, SIXEL_PLACEMENT).unwrap();
        assert_eq!((placement.cols, placement.rows), (2, 2));
        assert_eq!(placement.kind, PlacementKind::Internal);

        let cell = |row, col| {
            Some(ImageCell {
                image: id,
                placement: SIXEL_PLACEMENT,
                row,
                col,
            })
        };
        assert_eq!(image_cells(&term, 1)[3..6], [cell(0, 0), cell(0, 1), None]);
        assert_eq!(image_cells(&term, 2)[3..6], [cell(1, 0), cell(1, 1), None]);
        assert_eq!(image_cells(&term, 1)[2], None);
        // The cursor ends on the line below the image, at its first column.
        assert_eq!(term.grid().cursor.point, Point::new(Line(3), Column(3)));
    }

    #[test]
    fn sixel_at_the_bottom_scrolls_the_screen() {
        let (_, mut handler, mut term) = setup(10, 3);
        term.grid_mut().cursor.point = Point::new(Line(2), Column(0));
        // 10 x 30 px: one column, two rows.
        handler.image(&mut term, Image::Sixel(b"q\"1;1;10;30#1@".to_vec()));
        assert!(image_cells(&term, 0)[0].is_some());
        assert!(image_cells(&term, 1)[0].is_some());
        assert!(image_cells(&term, -1)[0].is_none());
        assert_eq!(term.grid().cursor.point, Point::new(Line(2), Column(0)));
    }

    #[test]
    fn sixels_get_distinct_images() {
        let (store, mut handler, mut term) = setup(10, 5);
        handler.image(&mut term, Image::Sixel(b"q#1@".to_vec()));
        handler.image(&mut term, Image::Sixel(b"q#1@".to_vec()));
        assert_eq!(lock(&store).len(), 2);
        assert_eq!(term.grid().cursor.point, Point::new(Line(2), Column(0)));
    }

    #[test]
    fn sixel_without_pixels_changes_nothing() {
        let (store, mut handler, mut term) = setup(10, 5);
        handler.image(&mut term, Image::Sixel(b"0;1q".to_vec()));
        assert!(lock(&store).is_empty());
        assert_eq!(term.grid().cursor.point, Point::new(Line(0), Column(0)));
        assert!(handler.take_replies().is_none());
    }

    #[test]
    fn reset_clears_the_store() {
        let store = Arc::new(Mutex::new(ImageStore::new()));
        lock(&store).insert(1, vec![Pixels::new(1, 1, vec![0; 4]).unwrap()]);
        let mut term = Term::new(Config::default(), &TermSize::new(4, 2), VoidListener);
        let mut handler = StoreHandler::new(store.clone());
        handler.reset(&mut term);
        assert!(lock(&store).is_empty());
    }
}
