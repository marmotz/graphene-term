//! Clipboard offers holding an image: `arboard` sets one format at a time, so a copy
//! with an image offers its PNG and its text itself, through `wl-clipboard-rs` on
//! Wayland and an `x11rb` selection owner on X11. Text-only copies stay on `arboard`.

use std::sync::Arc;

use anyhow::Result;

use crate::graphics::{Pixels, SourceRect};

/// MIME type of the image offer.
pub const PNG_MIME: &str = "image/png";

/// What a copy puts in the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// Text only, through `arboard`.
    Text(String),
    /// A PNG, with the text when the selection holds any.
    Image { text: Option<String>, png: Vec<u8> },
}

/// Chooses what a copy offers: exactly one image in `images` → its PNG from `encode`,
/// with the non-empty `text`; no image, several images, or an image that cannot be
/// encoded → the text only. `None` when there is nothing to copy.
pub fn content(
    text: Option<String>,
    images: &[(u32, u32)],
    encode: impl FnOnce(u32, u32) -> Option<Vec<u8>>,
) -> Option<Content> {
    let text = text.filter(|text| !text.is_empty());
    if let [(image, placement)] = images
        && let Some(png) = encode(*image, *placement)
    {
        return Some(Content::Image { text, png });
    }
    text.map(Content::Text)
}

/// PNG of the part `source` (the whole image when `None`) of `pixels`. `None` when the
/// part is empty or the encoder fails.
pub fn encode_png(pixels: &Pixels, source: Option<SourceRect>) -> Option<Vec<u8>> {
    let rect = SourceRect::resolve(source, pixels.width, pixels.height)?;
    let stride = pixels.width as usize * 4;
    let mut rgba = Vec::with_capacity(rect.width as usize * rect.height as usize * 4);
    for row in rect.y..rect.y + rect.height {
        let start = row as usize * stride + rect.x as usize * 4;
        rgba.extend_from_slice(&pixels.rgba[start..start + rect.width as usize * 4]);
    }
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, rect.width, rect.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let result = encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&rgba).and(writer.finish()));
    if let Err(error) = result {
        log::warn!("PNG encoding failed: {error}");
        return None;
    }
    Some(png)
}

/// Offers `png` as `image/png` in the clipboard, with `text` as UTF-8 text when given.
/// Wayland is tried first when `WAYLAND_DISPLAY` is set (compositors without the data
/// control protocol fall back to X11 through XWayland).
pub fn set_text_and_image(text: Option<String>, png: Vec<u8>) -> Result<()> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        match wayland::set(text.clone(), png.clone()) {
            Ok(()) => return Ok(()),
            Err(error) => log::debug!("Wayland clipboard unavailable, trying X11: {error}"),
        }
    }
    x11::set(text, png)
}

mod wayland {
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
}

/// Format the X11 owner offers a selection in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Utf8String,
    TextPlainUtf8,
    Png,
}

/// Targets offered for a copy, `TARGETS` aside: the text ones only when there is text.
fn targets(has_text: bool) -> Vec<Target> {
    let mut targets = Vec::with_capacity(3);
    if has_text {
        targets.extend([Target::Utf8String, Target::TextPlainUtf8]);
    }
    targets.push(Target::Png);
    targets
}

/// Data larger than this is sent with the `INCR` protocol, in chunks of this size (or of
/// the server's maximum request size, when smaller).
const INCR_CHUNK: usize = 256 * 1024;

/// One `INCR` transfer in progress: the chunks of `data`, then an empty one that ends
/// the transfer.
#[derive(Debug)]
struct Transfer {
    /// Atom of the requested target, the type of every chunk.
    target: u32,
    data: Arc<[u8]>,
    offset: usize,
    done: bool,
}

impl Transfer {
    fn new(target: u32, data: Arc<[u8]>) -> Self {
        Self {
            target,
            data,
            offset: 0,
            done: false,
        }
    }

    /// The next chunk, at most `max` bytes; an empty chunk once the data is sent, then
    /// `None`.
    fn next_chunk(&mut self, max: usize) -> Option<&[u8]> {
        if self.done {
            return None;
        }
        let end = (self.offset + max).min(self.data.len());
        let chunk = &self.data[self.offset..end];
        self.done = chunk.is_empty();
        self.offset = end;
        Some(chunk)
    }
}

mod x11 {
    use std::collections::HashMap;
    use std::sync::Arc;

    use anyhow::{Context as _, Result, anyhow};
    use x11rb::connection::{Connection, RequestConnection as _};
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{
        Atom, AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, CreateWindowAux, EventMask,
        PropMode, Property, SELECTION_NOTIFY_EVENT, SelectionNotifyEvent, SelectionRequestEvent,
        Time, Window, WindowClass,
    };
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;
    use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, NONE};

    use super::{INCR_CHUNK, Target, Transfer, targets};

    x11rb::atom_manager! {
        Atoms: AtomsCookie {
            CLIPBOARD,
            TARGETS,
            INCR,
            UTF8_STRING,
            TEXT_PLAIN_UTF8: b"text/plain;charset=utf-8",
            IMAGE_PNG: b"image/png",
        }
    }

    /// Takes the `CLIPBOARD` selection with a window of a new connection, then serves it
    /// from a dedicated thread until another client takes it.
    pub fn set(text: Option<String>, png: Vec<u8>) -> Result<()> {
        let (conn, screen) = RustConnection::connect(None).context("X11 connection")?;
        let atoms = Atoms::new(&conn)?.reply()?;
        let root = conn
            .setup()
            .roots
            .get(screen)
            .ok_or_else(|| anyhow!("no X11 screen {screen}"))?
            .root;
        let window = conn.generate_id()?;
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            COPY_FROM_PARENT,
            &CreateWindowAux::new(),
        )?;
        conn.set_selection_owner(window, atoms.CLIPBOARD, Time::CURRENT_TIME)?;
        if conn.get_selection_owner(atoms.CLIPBOARD)?.reply()?.owner != window {
            return Err(anyhow!("X11 clipboard ownership refused"));
        }
        let owner = Owner::new(conn, atoms, window, text, png);
        std::thread::Builder::new()
            .name("clipboard-owner".into())
            .spawn(move || {
                if let Err(error) = owner.run() {
                    log::warn!("X11 clipboard owner stopped: {error}");
                }
            })?;
        Ok(())
    }

    /// Owner of the `CLIPBOARD` selection.
    struct Owner {
        conn: RustConnection,
        atoms: Atoms,
        window: Window,
        targets: Vec<Target>,
        text: Arc<[u8]>,
        png: Arc<[u8]>,
        /// `INCR` transfers in progress, by requestor window and property.
        transfers: HashMap<(Window, Atom), Transfer>,
        /// Largest chunk sent in one request.
        chunk: usize,
    }

    impl Owner {
        fn new(
            conn: RustConnection,
            atoms: Atoms,
            window: Window,
            text: Option<String>,
            png: Vec<u8>,
        ) -> Self {
            let chunk = conn
                .maximum_request_bytes()
                .saturating_sub(1024)
                .clamp(1024, INCR_CHUNK);
            Self {
                targets: targets(text.is_some()),
                text: text.unwrap_or_default().into_bytes().into(),
                png: png.into(),
                conn,
                atoms,
                window,
                transfers: HashMap::new(),
                chunk,
            }
        }

        fn atom(&self, target: Target) -> Atom {
            match target {
                Target::Utf8String => self.atoms.UTF8_STRING,
                Target::TextPlainUtf8 => self.atoms.TEXT_PLAIN_UTF8,
                Target::Png => self.atoms.IMAGE_PNG,
            }
        }

        /// Serves requests until the selection is lost and the transfers in progress
        /// are over.
        fn run(mut self) -> Result<()> {
            let mut owned = true;
            while owned || !self.transfers.is_empty() {
                match self.conn.wait_for_event()? {
                    Event::SelectionClear(event)
                        if event.owner == self.window
                            && event.selection == self.atoms.CLIPBOARD =>
                    {
                        owned = false;
                    }
                    Event::SelectionRequest(event) => {
                        let served = owned && self.serve(event)?;
                        self.notify(event, served)?;
                    }
                    Event::PropertyNotify(event) if event.state == Property::DELETE => {
                        self.continue_transfer(event.window, event.atom)?;
                    }
                    // A requestor gone in the middle of a transfer.
                    Event::Error(error) => log::debug!("X11 clipboard error: {error:?}"),
                    _ => {}
                }
            }
            Ok(())
        }

        /// Writes the requested target to the requestor's property. `false` when the
        /// target is not offered.
        fn serve(&mut self, event: SelectionRequestEvent) -> Result<bool> {
            let property = property_of(&event);
            if event.target == self.atoms.TARGETS {
                let mut atoms: Vec<Atom> = self.targets.iter().map(|&t| self.atom(t)).collect();
                atoms.push(self.atoms.TARGETS);
                self.conn.change_property32(
                    PropMode::REPLACE,
                    event.requestor,
                    property,
                    AtomEnum::ATOM,
                    &atoms,
                )?;
                return Ok(true);
            }
            let Some(target) = self
                .targets
                .iter()
                .copied()
                .find(|&t| self.atom(t) == event.target)
            else {
                return Ok(false);
            };
            let data = match target {
                Target::Png => self.png.clone(),
                Target::Utf8String | Target::TextPlainUtf8 => self.text.clone(),
            };
            if data.len() <= self.chunk {
                self.conn.change_property8(
                    PropMode::REPLACE,
                    event.requestor,
                    property,
                    event.target,
                    &data,
                )?;
                return Ok(true);
            }
            // The requestor deletes the property to ask for each next chunk.
            self.conn.change_window_attributes(
                event.requestor,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )?;
            self.conn.change_property32(
                PropMode::REPLACE,
                event.requestor,
                property,
                self.atoms.INCR,
                &[u32::try_from(data.len()).unwrap_or(u32::MAX)],
            )?;
            self.transfers.insert(
                (event.requestor, property),
                Transfer::new(event.target, data),
            );
            Ok(true)
        }

        /// Writes the next chunk of the transfer to `property` of `window`, if one is in
        /// progress there.
        fn continue_transfer(&mut self, window: Window, property: Atom) -> Result<()> {
            let key = (window, property);
            let Some(transfer) = self.transfers.get_mut(&key) else {
                return Ok(());
            };
            let target = transfer.target;
            match transfer.next_chunk(self.chunk) {
                Some(chunk) => {
                    let last = chunk.is_empty();
                    self.conn.change_property8(
                        PropMode::REPLACE,
                        window,
                        property,
                        target,
                        chunk,
                    )?;
                    if last {
                        self.transfers.remove(&key);
                        self.conn.change_window_attributes(
                            window,
                            &ChangeWindowAttributesAux::new().event_mask(EventMask::NO_EVENT),
                        )?;
                    }
                }
                None => {
                    self.transfers.remove(&key);
                }
            }
            self.conn.flush()?;
            Ok(())
        }

        /// Tells the requestor its request is answered (`served`) or refused.
        fn notify(&self, event: SelectionRequestEvent, served: bool) -> Result<()> {
            let property = if served { property_of(&event) } else { NONE };
            self.conn.send_event(
                false,
                event.requestor,
                EventMask::NO_EVENT,
                SelectionNotifyEvent {
                    response_type: SELECTION_NOTIFY_EVENT,
                    sequence: 0,
                    time: event.time,
                    requestor: event.requestor,
                    selection: event.selection,
                    target: event.target,
                    property,
                },
            )?;
            self.conn.flush()?;
            Ok(())
        }
    }

    /// Property the answer goes to: obsolete clients name none and get the target.
    fn property_of(event: &SelectionRequestEvent) -> Atom {
        if event.property == NONE {
            event.target
        } else {
            event.property
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(width: u32, height: u32) -> Arc<Pixels> {
        let rgba = (0..width * height)
            .flat_map(|i| [i as u8, 0, 0, 255])
            .collect();
        Pixels::new(width, height, rgba).unwrap()
    }

    fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buffer).unwrap();
        buffer.truncate(info.buffer_size());
        (info.width, info.height, buffer)
    }

    #[test]
    fn text_alone_takes_the_text_path() {
        let content = content(Some("hello".into()), &[], |_, _| panic!("no image"));
        assert_eq!(content, Some(Content::Text("hello".into())));
    }

    #[test]
    fn one_image_is_offered_with_the_text() {
        let content = content(Some("hello".into()), &[(7, 3)], |image, placement| {
            assert_eq!((image, placement), (7, 3));
            Some(vec![1, 2])
        });
        assert_eq!(
            content,
            Some(Content::Image {
                text: Some("hello".into()),
                png: vec![1, 2]
            })
        );
    }

    #[test]
    fn an_image_alone_offers_no_text() {
        let content = content(Some(String::new()), &[(7, 3)], |_, _| Some(vec![1]));
        assert_eq!(
            content,
            Some(Content::Image {
                text: None,
                png: vec![1]
            })
        );
    }

    #[test]
    fn several_images_copy_the_text_only() {
        let content = content(Some("hi".into()), &[(7, 3), (9, 1)], |_, _| {
            panic!("no image")
        });
        assert_eq!(content, Some(Content::Text("hi".into())));
    }

    #[test]
    fn an_image_that_cannot_be_encoded_falls_back_to_the_text() {
        let content = content(Some("hi".into()), &[(7, 3)], |_, _| None);
        assert_eq!(content, Some(Content::Text("hi".into())));
    }

    #[test]
    fn nothing_to_copy_is_none() {
        assert_eq!(content(Some(String::new()), &[], |_, _| None), None);
        assert_eq!(content(None, &[(7, 3), (9, 1)], |_, _| None), None);
    }

    #[test]
    fn png_holds_the_whole_frame() {
        let png = encode_png(&pixels(3, 2), None).unwrap();
        let (width, height, rgba) = decode(&png);
        assert_eq!((width, height), (3, 2));
        assert_eq!(rgba, pixels(3, 2).rgba);
    }

    #[test]
    fn png_holds_only_the_shown_part() {
        let source = SourceRect {
            x: 1,
            y: 1,
            width: 2,
            height: 1,
        };
        let png = encode_png(&pixels(3, 2), Some(source)).unwrap();
        let (width, height, rgba) = decode(&png);
        assert_eq!((width, height), (2, 1));
        assert_eq!(rgba, [4, 0, 0, 255, 5, 0, 0, 255]);
    }

    #[test]
    fn png_of_an_empty_part_is_none() {
        let source = SourceRect {
            x: 5,
            y: 0,
            width: 2,
            height: 1,
        };
        assert_eq!(encode_png(&pixels(3, 2), Some(source)), None);
    }

    #[test]
    fn targets_offer_text_only_with_text() {
        assert_eq!(
            targets(true),
            [Target::Utf8String, Target::TextPlainUtf8, Target::Png]
        );
        assert_eq!(targets(false), [Target::Png]);
    }

    #[test]
    fn incr_transfer_sends_chunks_then_an_empty_one() {
        let mut transfer = Transfer::new(0, Arc::from(&[1u8, 2, 3, 4, 5][..]));
        assert_eq!(transfer.next_chunk(2), Some(&[1, 2][..]));
        assert_eq!(transfer.next_chunk(2), Some(&[3, 4][..]));
        assert_eq!(transfer.next_chunk(2), Some(&[5][..]));
        assert_eq!(transfer.next_chunk(2), Some(&[][..]));
        assert_eq!(transfer.next_chunk(2), None);
    }
}
