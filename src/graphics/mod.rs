//! Terminal graphics: kitty graphics protocol and sixel.
//!
//! `vte` 0.15 drops APC strings and `alacritty_terminal` only logs DCS, so the image
//! sequences are cut out of the PTY stream by [`sequence::Splitter`] before parsing and
//! handed to a [`Handler`] with the `Term` lock held.

pub mod sequence;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::Term;
use log::debug;

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

/// Handler used until images are drawn: logs and drops every sequence.
#[derive(Debug, Default)]
pub struct LogHandler;

impl Handler for LogHandler {
    fn image<L: EventListener>(&mut self, _term: &mut Term<L>, image: Image) {
        match image {
            Image::Kitty(payload) => {
                debug!("kitty graphics sequence ignored ({} bytes)", payload.len())
            }
            Image::Sixel(body) => debug!("sixel sequence ignored ({} bytes)", body.len()),
        }
    }

    fn reset<L: EventListener>(&mut self, _term: &mut Term<L>) {
        debug!("full reset");
    }
}
