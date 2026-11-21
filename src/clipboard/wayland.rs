//! Wayland clipboard offers, through the data control protocol of `wl-clipboard-rs`.

use anyhow::Result;
use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};

use super::PNG_MIME;

/// Offers the sources from a background thread of `wl-clipboard-rs`, until another
/// client sets the clipboard.
pub fn set(text: Option<String>, png: Vec<u8>) -> Result<()> {
    let mut sources = Vec::with_capacity(2);
    if let Some(text) = text {
        sources.push(MimeSource {
            source: Source::Bytes(text.into_bytes().into()),
            mime_type: MimeType::Text,
        });
    }
    sources.push(MimeSource {
        source: Source::Bytes(png.into()),
        mime_type: MimeType::Specific(PNG_MIME.to_owned()),
    });
    Options::new().copy_multi(sources)?;
    Ok(())
}
