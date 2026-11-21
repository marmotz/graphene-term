//! X11 clipboard offers: a selection owner serving `CLIPBOARD`, with the `INCR`
//! protocol for large data.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow};
use x11rb::connection::{Connection, RequestConnection as _};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, CreateWindowAux, EventMask,
    PropMode, Property, SELECTION_NOTIFY_EVENT, SelectionNotifyEvent, SelectionRequestEvent, Time,
    Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, NONE};

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
                    if event.owner == self.window && event.selection == self.atoms.CLIPBOARD =>
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
                self.conn
                    .change_property8(PropMode::REPLACE, window, property, target, chunk)?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
