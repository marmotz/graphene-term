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
pub mod store;

use std::sync::{Arc, Mutex};

use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::Term;
use log::debug;

pub use placeholder::{CursorMove, ImageCell, erase, place};
pub use store::{ImageStore, Pixels, Placement, PlacementKind, SourceRect};

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
/// [`ImageStore`] and keeps their replies for the PTY. Sixel sequences are logged and
/// dropped until the sixel decoder lands.
#[derive(Debug, Clone)]
pub struct StoreHandler {
    store: Arc<Mutex<ImageStore>>,
    kitty: kitty::Kitty,
    /// Replies to write to the PTY, in order.
    replies: Vec<u8>,
}

impl StoreHandler {
    pub fn new(store: Arc<Mutex<ImageStore>>) -> Self {
        Self {
            store,
            kitty: kitty::Kitty::new(),
            replies: Vec::new(),
        }
    }

    /// Pixel size of a cell, to size the images placed without a cell count.
    pub fn set_cell_size(&mut self, width: u16, height: u16) {
        self.kitty
            .set_cell_size(u32::from(width), u32::from(height));
    }

    /// The replies to write to the PTY since the last call, `None` when there is none.
    pub fn take_replies(&mut self) -> Option<Vec<u8>> {
        (!self.replies.is_empty()).then(|| std::mem::take(&mut self.replies))
    }
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
            Image::Sixel(body) => debug!("sixel sequence ignored ({} bytes)", body.len()),
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
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;

    use super::*;

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
