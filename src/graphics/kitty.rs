//! Kitty graphics protocol, still images and animations:
//! <https://sw.kovidgoyal.net/kitty/graphics-protocol/>.
//!
//! [`parse`], [`load`] and [`decode`] turn a sequence into pixels. [`Kitty`] runs the
//! commands of one terminal against its grid and [`ImageStore`], and returns the reply
//! to write back to the PTY.

use std::collections::{BTreeSet, HashMap};
use std::ffi::CString;
use std::fmt;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::os::fd::FromRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;
use base64::Engine as _;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use log::debug;

use super::placeholder::{MAX_CELLS, on_screen};
use super::store::BUDGET;
use super::{
    AnimationState, CursorMove, DEFAULT_GAP, ImageStore, Pixels, Placement, PlacementKind,
    SourceRect, erase, place,
};

/// Largest decoded payload, decompressed data or file read: the store budget.
const MAX_DATA: usize = BUDGET;

/// Largest base64 payload of a chunked transfer: the base64 size of [`MAX_DATA`].
const MAX_TRANSFER: usize = MAX_DATA / 3 * 4 + 4;

/// Largest placement id: placeholder cells carry it in a 24-bit color.
pub const MAX_PLACEMENT: u32 = 0x00ff_ffff;

/// Cell size used until the PTY loop is told the real one.
pub const DEFAULT_CELL: (u32, u32) = (8, 16);

/// Base64 as clients send it: padding optional.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

/// Directories a file medium (`t=f`) may not read from.
const FORBIDDEN_DIRS: [&str; 3] = ["/proc", "/sys", "/dev"];

/// Part a temporary file medium (`t=t`) must have in its path, as the protocol requires.
const TEMP_MARKER: &str = "tty-graphics-protocol";

/// A failed command, replied as `CODE:message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
}

impl Error {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::new("EINVAL", message)
    }

    fn too_big() -> Self {
        Self::new("EFBIG", "image data too large")
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.code, self.message)
    }
}

/// The control keys of a command. Keys not given keep the protocol defaults; keys this
/// terminal does not use (relative placements) are ignored. Animation commands give
/// some keys another meaning, noted on each field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// `a`: `t` transmit, `T` transmit and put, `p` put, `q` query, `d` delete, `f`
    /// `a` `c` animation.
    pub action: u8,
    /// `q`: 1 suppresses `OK` replies, 2 suppresses errors too.
    pub quiet: u32,
    /// `f`: 24 (RGB), 32 (RGBA) or 100 (PNG).
    pub format: u32,
    /// `t`: `d` direct, `f` file, `t` temporary file, `s` shared memory.
    pub medium: u8,
    /// `o`: 0 for none, `z` for zlib.
    pub compression: u8,
    /// `m=1`: more chunks follow.
    pub more: bool,
    /// `s`, `v`: pixel size of raw data. For `a=a`: `s` is the state (1 stop, 2 run
    /// waiting for frames, 3 run and loop), `v` the loop count.
    pub width: u32,
    pub height: u32,
    /// `S`, `O`: bytes to read from a file or shared memory, and where to start.
    pub size: u32,
    pub offset: u32,
    /// `i`: image id given by the client.
    pub id: u32,
    /// `I`: image number, the terminal picks the id.
    pub number: u32,
    /// `p`: placement id.
    pub placement: u32,
    /// `x`, `y`, `w`, `h`: source rectangle, in pixels (0 for the rest of the image).
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// `c`, `r`: columns and rows of the placement (0 to compute it). For animations,
    /// 1-based frame numbers: `a=f` `c` base frame and `r` frame to edit, `a=a` `c`
    /// frame to show and `r` frame whose gap `z` sets, `a=c` `c` destination and `r`
    /// source, `d=f` `r` frame to delete.
    pub cols: u32,
    pub rows: u32,
    /// `C=1`: leave the cursor where it is. For `a=c`: overwrite instead of blending.
    pub keep_cursor: bool,
    /// `U=1`: virtual placement, the client prints the placeholders.
    pub unicode: bool,
    /// `z`: stacking order. For `a=f` and `a=a`: frame gap in milliseconds, negative
    /// for a gapless frame, 0 for unspecified.
    pub z: i32,
    /// `X`: for `a=f`, 1 overwrites instead of blending; for `a=c`, left edge of the
    /// source rectangle.
    pub big_x: u32,
    /// `Y`: for `a=f`, background color as `0xRRGGBBAA`; for `a=c`, top edge of the
    /// source rectangle.
    pub big_y: u32,
    /// `d`: what to delete.
    pub delete: u8,
}

impl Default for Command {
    fn default() -> Self {
        Self {
            action: b't',
            quiet: 0,
            format: 32,
            medium: b'd',
            compression: 0,
            more: false,
            width: 0,
            height: 0,
            size: 0,
            offset: 0,
            id: 0,
            number: 0,
            placement: 0,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            cols: 0,
            rows: 0,
            keep_cursor: false,
            unicode: false,
            z: 0,
            big_x: 0,
            big_y: 0,
            delete: b'a',
        }
    }
}

/// Splits a sequence payload (without the leading `G`) into its command and its base64
/// data.
pub fn parse(payload: &[u8]) -> Result<(Command, &[u8]), Error> {
    let (control, data) = match payload.iter().position(|&byte| byte == b';') {
        Some(index) => (&payload[..index], &payload[index + 1..]),
        None => (payload, &payload[payload.len()..]),
    };
    let mut command = Command::default();
    for pair in control.split(|&byte| byte == b',') {
        if pair.is_empty() {
            continue;
        }
        let [key, b'=', value @ ..] = pair else {
            return Err(Error::invalid(format!(
                "malformed key: {}",
                String::from_utf8_lossy(pair)
            )));
        };
        let key = *key;
        match key {
            b'a' => command.action = letter(key, value)?,
            b't' => command.medium = letter(key, value)?,
            b'o' => command.compression = letter(key, value)?,
            b'd' => command.delete = letter(key, value)?,
            b'q' => command.quiet = number(key, value)?,
            b'f' => command.format = number(key, value)?,
            b'm' => command.more = number(key, value)? == 1,
            b's' => command.width = number(key, value)?,
            b'v' => command.height = number(key, value)?,
            b'S' => command.size = number(key, value)?,
            b'O' => command.offset = number(key, value)?,
            b'i' => command.id = number(key, value)?,
            b'I' => command.number = number(key, value)?,
            b'p' => command.placement = number(key, value)?,
            b'x' => command.x = number(key, value)?,
            b'y' => command.y = number(key, value)?,
            b'w' => command.w = number(key, value)?,
            b'h' => command.h = number(key, value)?,
            b'c' => command.cols = number(key, value)?,
            b'r' => command.rows = number(key, value)?,
            b'C' => command.keep_cursor = number(key, value)? == 1,
            b'U' => command.unicode = number(key, value)? == 1,
            b'X' => command.big_x = number(key, value)?,
            b'Y' => command.big_y = number(key, value)?,
            b'z' => {
                command.z = text(value)
                    .parse()
                    .map_err(|_| Error::invalid(format!("bad value for z: {}", text(value))))?
            }
            _ => (),
        }
    }
    Ok((command, data))
}

fn text(value: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(value)
}

fn letter(key: u8, value: &[u8]) -> Result<u8, Error> {
    match value {
        [letter] => Ok(*letter),
        _ => Err(Error::invalid(format!(
            "bad value for {}: {}",
            key as char,
            text(value)
        ))),
    }
}

fn number(key: u8, value: &[u8]) -> Result<u32, Error> {
    text(value)
        .parse()
        .map_err(|_| Error::invalid(format!("bad value for {}: {}", key as char, text(value))))
}

/// The bytes of the image a command transmits: its base64 `data` decoded, or the file or
/// shared memory it names read.
pub fn load(command: &Command, data: &[u8]) -> Result<Vec<u8>, Error> {
    let decoded = BASE64
        .decode(data)
        .map_err(|err| Error::invalid(format!("bad base64 data: {err}")))?;
    if command.medium == b'd' {
        return Ok(decoded);
    }
    let name =
        String::from_utf8(decoded).map_err(|_| Error::invalid("the file name is not UTF-8"))?;
    match command.medium {
        b'f' => read_file(Path::new(&name), command),
        b't' => read_temp_file(Path::new(&name), command),
        b's' => read_shared_memory(&name, command),
        other => Err(Error::invalid(format!(
            "unknown transmission medium: {}",
            other as char
        ))),
    }
}

fn bad_file(path: &Path, err: impl fmt::Display) -> Error {
    Error::new("EBADF", format!("{}: {err}", path.display()))
}

/// Opens `path` when it is a regular file.
fn open_regular(path: &Path) -> Result<File, Error> {
    let file = File::open(path).map_err(|err| bad_file(path, err))?;
    let metadata = file.metadata().map_err(|err| bad_file(path, err))?;
    if !metadata.is_file() {
        return Err(bad_file(path, "not a regular file"));
    }
    Ok(file)
}

fn read_file(path: &Path, command: &Command) -> Result<Vec<u8>, Error> {
    let real = path.canonicalize().map_err(|err| bad_file(path, err))?;
    if FORBIDDEN_DIRS.iter().any(|dir| real.starts_with(dir)) {
        return Err(bad_file(path, "reading from this directory is not allowed"));
    }
    read_from(open_regular(&real)?, command)
}

/// Reads a temporary file and deletes it, as the protocol requires. Only files under a
/// temporary directory, with [`TEMP_MARKER`] in their path, are read or deleted.
fn read_temp_file(path: &Path, command: &Command) -> Result<Vec<u8>, Error> {
    let real = path.canonicalize().map_err(|err| bad_file(path, err))?;
    let in_temp = temp_dirs().iter().any(|dir| real.starts_with(dir));
    if !in_temp || !real.to_string_lossy().contains(TEMP_MARKER) {
        return Err(bad_file(
            path,
            "not a temporary file of the graphics protocol",
        ));
    }
    let bytes = open_regular(&real).and_then(|file| read_from(file, command));
    if let Err(err) = std::fs::remove_file(&real) {
        debug!("cannot delete {}: {err}", real.display());
    }
    bytes
}

fn temp_dirs() -> Vec<PathBuf> {
    [
        std::env::temp_dir(),
        PathBuf::from("/tmp"),
        PathBuf::from("/dev/shm"),
    ]
    .into_iter()
    .filter_map(|dir| dir.canonicalize().ok())
    .collect()
}

/// Reads a POSIX shared memory object and unlinks it, as the protocol requires.
fn read_shared_memory(name: &str, command: &Command) -> Result<Vec<u8>, Error> {
    let bad = |err: &dyn fmt::Display| Error::new("EBADF", format!("{name}: {err}"));
    let c_name = CString::new(name).map_err(|err| bad(&err))?;
    // SAFETY: `c_name` is a valid NUL-terminated string.
    let fd = unsafe { libc::shm_open(c_name.as_ptr(), libc::O_RDONLY, 0) };
    if fd < 0 {
        return Err(bad(&std::io::Error::last_os_error()));
    }
    // SAFETY: `fd` was just opened and is owned by nothing else.
    let file = unsafe { File::from_raw_fd(fd) };
    let bytes = read_from(file, command);
    // SAFETY: `c_name` is a valid NUL-terminated string.
    unsafe { libc::shm_unlink(c_name.as_ptr()) };
    bytes
}

/// Reads `command.size` bytes (all when 0) from `command.offset` on.
fn read_from(mut file: File, command: &Command) -> Result<Vec<u8>, Error> {
    let fail = |err: std::io::Error| Error::new("EBADF", err.to_string());
    if command.offset > 0 {
        file.seek(SeekFrom::Start(u64::from(command.offset)))
            .map_err(fail)?;
    }
    let limit = match command.size {
        0 => MAX_DATA as u64 + 1,
        size => u64::from(size),
    };
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes).map_err(fail)?;
    if bytes.len() > MAX_DATA {
        return Err(Error::too_big());
    }
    Ok(bytes)
}

/// Decodes the bytes of an image, as [`load`] returns them, to RGBA pixels.
pub fn decode(command: &Command, bytes: Vec<u8>) -> Result<Arc<Pixels>, Error> {
    let bytes = match command.compression {
        0 => bytes,
        b'z' => inflate(&bytes)?,
        other => {
            return Err(Error::invalid(format!(
                "unknown compression: {}",
                other as char
            )));
        }
    };
    match command.format {
        100 => decode_png(&bytes),
        24 | 32 => decode_raw(command, &bytes),
        other => Err(Error::invalid(format!("unknown format: {other}"))),
    }
}

fn inflate(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(bytes)
        .take(MAX_DATA as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|err| Error::invalid(format!("bad zlib data: {err}")))?;
    if out.len() > MAX_DATA {
        return Err(Error::too_big());
    }
    Ok(out)
}

fn decode_png(bytes: &[u8]) -> Result<Arc<Pixels>, Error> {
    let bad = |err: png::DecodingError| Error::invalid(format!("bad PNG data: {err}"));
    let mut decoder =
        png::Decoder::new_with_limits(Cursor::new(bytes), png::Limits { bytes: MAX_DATA });
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(bad)?;
    let size = reader.output_buffer_size().ok_or_else(Error::too_big)?;
    if size > MAX_DATA {
        return Err(Error::too_big());
    }
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(bad)?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => rgb_to_rgba(&buf),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[g, a]| [g, g, g, a])
            .collect(),
        png::ColorType::Indexed => return Err(Error::invalid("unexpanded PNG palette")),
    };
    Pixels::new(info.width, info.height, rgba).ok_or_else(|| Error::invalid("empty PNG image"))
}

fn rgb_to_rgba(rgb: &[u8]) -> Vec<u8> {
    rgb.as_chunks::<3>()
        .0
        .iter()
        .flat_map(|&[r, g, b]| [r, g, b, 255])
        .collect()
}

fn decode_raw(command: &Command, bytes: &[u8]) -> Result<Arc<Pixels>, Error> {
    let (width, height) = (command.width, command.height);
    if width == 0 || height == 0 {
        return Err(Error::invalid(
            "the image width and height (s, v) are required",
        ));
    }
    let channels = if command.format == 24 { 3 } else { 4 };
    let needed = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(channels))
        .filter(|&needed| needed <= MAX_DATA)
        .ok_or_else(Error::too_big)?;
    if bytes.len() < needed {
        return Err(Error::new(
            "ENODATA",
            format!(
                "insufficient image data: {} bytes, {needed} needed",
                bytes.len()
            ),
        ));
    }
    let bytes = &bytes[..needed];
    let rgba = if channels == 4 {
        bytes.to_vec()
    } else {
        rgb_to_rgba(bytes)
    };
    Pixels::new(width, height, rgba).ok_or_else(|| Error::invalid("empty image"))
}

/// A chunked transfer (`m=1`) waiting for its last chunk.
#[derive(Debug, Clone)]
struct Transfer {
    /// The command of the first chunk.
    command: Command,
    /// Base64 data of the chunks so far.
    data: Vec<u8>,
    /// The data went over [`MAX_TRANSFER`] and was dropped.
    too_big: bool,
}

/// Kitty graphics state of one terminal.
#[derive(Debug, Clone)]
pub struct Kitty {
    transfer: Option<Transfer>,
    /// Image id of the newest image of each image number (`I`).
    numbers: HashMap<u32, u32>,
    /// Next placement id to try for a direct placement without `p`.
    next_placement: u32,
    cell_width: u32,
    cell_height: u32,
}

impl Default for Kitty {
    fn default() -> Self {
        Self::new()
    }
}

impl Kitty {
    pub fn new() -> Self {
        Self {
            transfer: None,
            numbers: HashMap::new(),
            next_placement: MAX_PLACEMENT,
            cell_width: DEFAULT_CELL.0,
            cell_height: DEFAULT_CELL.1,
        }
    }

    /// Pixel size of a cell, used to size placements without `c` or `r`. Zero sizes
    /// are ignored.
    pub fn set_cell_size(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.cell_width = width;
            self.cell_height = height;
        }
    }

    /// Forgets every transfer and image number (full reset).
    pub fn reset(&mut self) {
        self.transfer = None;
        self.numbers.clear();
    }

    /// Runs the command of a sequence payload (without the leading `G`) at the cursor of
    /// `term`. Returns the reply to write to the PTY, if any.
    pub fn handle<L: EventListener>(
        &mut self,
        term: &mut Term<L>,
        store: &mut ImageStore,
        payload: &[u8],
    ) -> Option<Vec<u8>> {
        let (command, data) = match parse(payload) {
            Ok(parsed) => parsed,
            Err(err) => {
                debug!("kitty graphics command dropped: {err}");
                self.transfer = None;
                return None;
            }
        };

        // While a chunked transfer runs, every command is one of its chunks: only `m`
        // (and `q`) matter.
        if let Some(mut transfer) = self.transfer.take() {
            if !transfer.too_big {
                if transfer.data.len() + data.len() > MAX_TRANSFER {
                    transfer.too_big = true;
                    transfer.data = Vec::new();
                } else {
                    transfer.data.extend_from_slice(data);
                }
            }
            if command.quiet != 0 {
                transfer.command.quiet = command.quiet;
            }
            if command.more {
                self.transfer = Some(transfer);
                return None;
            }
            if transfer.too_big {
                return reply(&transfer.command, Err(Error::too_big()));
            }
            return self.run(term, store, &transfer.command, &transfer.data);
        }

        if command.more {
            self.transfer = Some(Transfer {
                command,
                data: data.to_vec(),
                too_big: data.len() > MAX_TRANSFER,
            });
            return None;
        }
        self.run(term, store, &command, data)
    }

    fn run<L: EventListener>(
        &mut self,
        term: &mut Term<L>,
        store: &mut ImageStore,
        command: &Command,
        data: &[u8],
    ) -> Option<Vec<u8>> {
        match command.action {
            b't' | b'T' | b'q' => reply(command, self.transmit(term, store, command, data)),
            b'p' => reply(command, self.put(term, store, command)),
            b'd' => {
                self.delete(term, store, command);
                None
            }
            b'f' => reply(command, self.frame(store, command, data)),
            b'c' => reply(command, self.compose(store, command)),
            // Like kitty, only errors are replied to an animation control.
            b'a' => match self.control(store, command) {
                Ok(_) => None,
                Err(err) => reply(command, Err(err)),
            },
            other => reply(
                command,
                Err(Error::invalid(format!("unknown action: {}", other as char))),
            ),
        }
    }

    /// `a=t`, `a=T`, `a=q`. Returns the image id.
    fn transmit<L: EventListener>(
        &mut self,
        term: &mut Term<L>,
        store: &mut ImageStore,
        command: &Command,
        data: &[u8],
    ) -> Result<u32, Error> {
        if command.id != 0 && command.number != 0 {
            return Err(Error::invalid("i and I cannot be used together"));
        }
        let pixels = decode(command, load(command, data)?)?;
        if command.action == b'q' {
            return Ok(command.id);
        }
        let id = match command.id {
            0 => store.allocate_id(),
            id => id,
        };
        // A new image replaces the one of the same id, and its placements leave the
        // screen.
        let old = internal_placements(store, id);
        if store.insert(id, vec![pixels]).is_none() {
            return Err(Error::too_big());
        }
        for placement in old {
            erase(term, id, Some(placement));
        }
        if command.number != 0 {
            self.numbers.insert(command.number, id);
        }
        if command.action == b'T' {
            self.place(term, store, command, id)?;
        }
        Ok(id)
    }

    /// `a=p`. Returns the image id.
    fn put<L: EventListener>(
        &mut self,
        term: &mut Term<L>,
        store: &mut ImageStore,
        command: &Command,
    ) -> Result<u32, Error> {
        let id = self.image_id(command)?;
        self.place(term, store, command, id)?;
        Ok(id)
    }

    /// The image a command names, by id (`i`) or number (`I`).
    fn image_id(&self, command: &Command) -> Result<u32, Error> {
        if command.id != 0 && command.number != 0 {
            return Err(Error::invalid("i and I cannot be used together"));
        }
        match (command.id, command.number) {
            (0, 0) => Err(Error::invalid("no image id (i or I)")),
            (0, number) => self
                .numbers
                .get(&number)
                .copied()
                .ok_or_else(|| Error::new("ENOENT", format!("no image with number {number}"))),
            (id, _) => Ok(id),
        }
    }

    /// `a=f`: adds a frame to an image, or edits one (`r`). The data covers a
    /// rectangle at `x`, `y` (the whole image by default), drawn over the base frame
    /// `c`, or a `Y` background. Returns the image id.
    fn frame(&self, store: &mut ImageStore, command: &Command, data: &[u8]) -> Result<u32, Error> {
        let id = self.image_id(command)?;
        let image = store.get(id).ok_or_else(|| no_image(id))?;
        let root = image.frames[0].clone();
        let frame_of = |number: u32| {
            image
                .frames
                .get(number as usize - 1)
                .cloned()
                .ok_or_else(|| no_frame(number))
        };
        let edited = (command.rows != 0)
            .then(|| frame_of(command.rows))
            .transpose()?;
        let base = (command.cols != 0)
            .then(|| frame_of(command.cols))
            .transpose()?;

        let mut format = command.clone();
        if format.width == 0 && format.height == 0 {
            format.width = root.width;
            format.height = root.height;
        }
        let rect = decode(&format, load(command, data)?)?;
        let mut canvas = match edited.or(base) {
            Some(frame) => frame.rgba.clone(),
            None => command
                .big_y
                .to_be_bytes()
                .repeat(root.width as usize * root.height as usize),
        };
        blit(
            &mut canvas,
            root.width,
            root.height,
            &rect,
            SourceRect {
                x: 0,
                y: 0,
                width: rect.width,
                height: rect.height,
            },
            (command.x, command.y),
            command.big_x == 1,
        );
        let pixels = Pixels::new(root.width, root.height, canvas)
            .ok_or_else(|| Error::invalid("empty frame"))?;
        let gap = match command.z {
            0 => None,
            z => Some(u32::try_from(z).unwrap_or(0)),
        };
        let stored = match command.rows {
            0 => store
                .add_frame(id, pixels, gap.unwrap_or(DEFAULT_GAP))
                .is_some(),
            number => store.replace_frame(id, number as usize - 1, pixels, gap),
        };
        if !stored {
            return Err(Error::too_big());
        }
        Ok(id)
    }

    /// `a=c`: copies a `w` x `h` rectangle of frame `r`, at `X`, `Y`, onto frame `c` at
    /// `x`, `y`. Returns the image id.
    fn compose(&self, store: &mut ImageStore, command: &Command) -> Result<u32, Error> {
        let id = self.image_id(command)?;
        let image = store.get(id).ok_or_else(|| no_image(id))?;
        let frame_of = |number: u32| {
            number
                .checked_sub(1)
                .and_then(|index| image.frames.get(index as usize))
                .cloned()
                .ok_or_else(|| no_frame(number))
        };
        let source = frame_of(command.rows)?;
        let target = frame_of(command.cols)?;
        let (width, height) = (source.width, source.height);
        let w = if command.w == 0 { width } else { command.w };
        let h = if command.h == 0 { height } else { command.h };
        let fits = |x: u32, y: u32| {
            x.checked_add(w).is_some_and(|end| end <= width)
                && y.checked_add(h).is_some_and(|end| end <= height)
        };
        if !fits(command.big_x, command.big_y) || !fits(command.x, command.y) {
            return Err(Error::invalid("the rectangle is outside the frame"));
        }
        let overlaps = command.big_x < command.x + w
            && command.x < command.big_x + w
            && command.big_y < command.y + h
            && command.y < command.big_y + h;
        if command.rows == command.cols && overlaps {
            return Err(Error::invalid(
                "the source and destination rectangles overlap",
            ));
        }
        let mut canvas = target.rgba.clone();
        blit(
            &mut canvas,
            width,
            height,
            &source,
            SourceRect {
                x: command.big_x,
                y: command.big_y,
                width: w,
                height: h,
            },
            (command.x, command.y),
            command.keep_cursor,
        );
        let pixels =
            Pixels::new(width, height, canvas).ok_or_else(|| Error::invalid("empty frame"))?;
        if !store.replace_frame(id, command.cols as usize - 1, pixels, None) {
            return Err(Error::too_big());
        }
        Ok(id)
    }

    /// `a=a`: sets the gap of frame `r` (`z`), shows frame `c`, sets the loop count
    /// (`v`) and the state (`s`). Returns the image id.
    fn control(&self, store: &mut ImageStore, command: &Command) -> Result<u32, Error> {
        let id = self.image_id(command)?;
        if store.get(id).is_none() {
            return Err(no_image(id));
        }
        if command.rows != 0 && command.z != 0 {
            let gap = u32::try_from(command.z).unwrap_or(0);
            if !store.set_gap(id, command.rows as usize - 1, gap) {
                return Err(no_frame(command.rows));
            }
        }
        if command.cols != 0 && !store.set_current_frame(id, command.cols as usize - 1) {
            return Err(no_frame(command.cols));
        }
        let state = match command.width {
            1 => Some(AnimationState::Stopped),
            2 => Some(AnimationState::Loading),
            3 => Some(AnimationState::Running),
            _ => None,
        };
        let loops = (command.height != 0).then(|| command.height - 1);
        store.set_animation(id, state, loops);
        Ok(id)
    }

    /// Places image `id`: a virtual placement is only recorded, a direct one also
    /// writes its placeholder cells at the cursor.
    fn place<L: EventListener>(
        &mut self,
        term: &mut Term<L>,
        store: &mut ImageStore,
        command: &Command,
        id: u32,
    ) -> Result<(), Error> {
        if command.placement > MAX_PLACEMENT {
            return Err(Error::invalid(format!(
                "placement id above {MAX_PLACEMENT}"
            )));
        }
        let frame = store
            .get(id)
            .ok_or_else(|| Error::new("ENOENT", format!("no image with id {id}")))?
            .frame()
            .clone();
        let whole = command.x == 0 && command.y == 0 && command.w == 0 && command.h == 0;
        let source = SourceRect::resolve(
            Some(SourceRect {
                x: command.x,
                y: command.y,
                width: match command.w {
                    0 => frame.width.saturating_sub(command.x),
                    w => w,
                },
                height: match command.h {
                    0 => frame.height.saturating_sub(command.y),
                    h => h,
                },
            }),
            frame.width,
            frame.height,
        )
        .ok_or_else(|| Error::invalid("the source rectangle is outside the image"))?;
        let (cols, rows) = self.cell_box(command, source.width, source.height);
        let placement = Placement {
            cols,
            rows,
            source: (!whole).then_some(source),
            kind: if command.unicode {
                PlacementKind::Virtual
            } else {
                PlacementKind::Internal
            },
            z: command.z,
        };

        if command.unicode {
            store.place(id, command.placement, placement);
            return Ok(());
        }
        let placement_id = match command.placement {
            0 => self.allocate_placement(store, id),
            given => {
                if internal_placements(store, id).contains(&given) {
                    erase(term, id, Some(given));
                }
                given
            }
        };
        store.place(id, placement_id, placement);
        let cursor = if command.keep_cursor {
            CursorMove::Keep
        } else {
            CursorMove::After
        };
        place(term, id, placement_id, cols, rows, cursor);
        Ok(())
    }

    /// Columns and rows of a placement showing `width` x `height` pixels: `c` and `r`
    /// when given, the other one keeping the aspect ratio when only one is, else the
    /// cells the pixels cover.
    fn cell_box(&self, command: &Command, width: u32, height: u32) -> (u32, u32) {
        let (cell_w, cell_h) = (u64::from(self.cell_width), u64::from(self.cell_height));
        let (w, h) = (u64::from(width), u64::from(height));
        let (cols, rows) = match (u64::from(command.cols), u64::from(command.rows)) {
            (0, 0) => (w.div_ceil(cell_w), h.div_ceil(cell_h)),
            (cols, 0) => (cols, (cols * cell_w * h).div_ceil(w * cell_h)),
            (0, rows) => ((rows * cell_h * w).div_ceil(h * cell_w), rows),
            (cols, rows) => (cols, rows),
        };
        let clamp = |cells: u64| cells.clamp(1, u64::from(MAX_CELLS)) as u32;
        (clamp(cols), clamp(rows))
    }

    /// A placement id image `id` does not use, for a direct placement without `p`,
    /// counting down from [`MAX_PLACEMENT`].
    fn allocate_placement(&mut self, store: &ImageStore, id: u32) -> u32 {
        loop {
            let placement = self.next_placement;
            self.next_placement = match placement {
                1 => MAX_PLACEMENT,
                _ => placement - 1,
            };
            if store.placements_of(id).all(|(used, _)| used != placement) {
                return placement;
            }
        }
    }

    /// `a=d`. A lowercase `d` removes placements, an uppercase one also frees the images
    /// left without placement.
    fn delete<L: EventListener>(
        &mut self,
        term: &mut Term<L>,
        store: &mut ImageStore,
        command: &Command,
    ) {
        let free = command.delete.is_ascii_uppercase();
        // Images named by the command, freed even when they have no placement.
        let mut images = BTreeSet::new();
        let cell = |x: u32, y: u32| {
            Point::new(
                Line(y.saturating_sub(1) as i32),
                Column(x.saturating_sub(1) as usize),
            )
        };
        let targets: BTreeSet<(u32, u32)> = match command.delete.to_ascii_lowercase() {
            b'a' => on_screen(term, |_| true),
            b'i' | b'n' => {
                let id = match command.delete.to_ascii_lowercase() {
                    b'i' => Some(command.id).filter(|&id| id != 0),
                    _ => self.numbers.get(&command.number).copied(),
                };
                let Some(id) = id else {
                    return;
                };
                if command.placement == 0 {
                    images.insert(id);
                    store
                        .placements_of(id)
                        .map(|(placement, _)| (id, placement))
                        .collect()
                } else {
                    BTreeSet::from([(id, command.placement)])
                }
            }
            b'r' => {
                let range = command.x..=command.y;
                images.extend(store.ids().filter(|id| range.contains(id)));
                store
                    .placements()
                    .filter(|((id, _), _)| range.contains(id))
                    .map(|(key, _)| key)
                    .collect()
            }
            b'c' => {
                let cursor = term.grid().cursor.point;
                on_screen(term, |point| point == cursor)
            }
            b'p' => {
                let target = cell(command.x, command.y);
                on_screen(term, |point| point == target)
            }
            b'q' => {
                let target = cell(command.x, command.y);
                on_screen(term, |point| point == target)
                    .into_iter()
                    .filter(|&(id, placement)| z_of(store, id, placement) == Some(command.z))
                    .collect()
            }
            b'x' => {
                let column = cell(command.x, 1).column;
                on_screen(term, |point| point.column == column)
            }
            b'y' => {
                let line = cell(1, command.y).line;
                on_screen(term, |point| point.line == line)
            }
            b'z' => store
                .placements()
                .filter(|(_, placement)| placement.z == command.z)
                .map(|(key, _)| key)
                .collect(),
            b'f' => {
                self.delete_frame(store, command, free);
                return;
            }
            other => {
                debug!("unknown kitty deletion: {}", other as char);
                return;
            }
        };

        for &(id, placement) in &targets {
            let kind = store
                .placements_of(id)
                .find(|&(used, _)| used == placement)
                .map(|(_, value)| value.kind);
            // Cells of an image already gone from the store are erased too; the client
            // prints the cells of a virtual placement, they stay.
            if kind != Some(PlacementKind::Virtual) {
                erase(term, id, Some(placement));
            }
            store.remove_placement(id, placement);
            images.insert(id);
        }
        if free {
            for id in images {
                if store.placements_of(id).next().is_none() {
                    store.remove(id);
                    self.numbers.retain(|_, image| *image != id);
                }
            }
        }
    }
}

impl Kitty {
    /// `d=f`: removes frame `r` (clamped to the last one, the first when 0). The only
    /// frame left stays; with `d=F`, an image down to it and without placement is
    /// freed.
    fn delete_frame(&mut self, store: &mut ImageStore, command: &Command, free: bool) {
        let Ok(id) = self.image_id(command) else {
            return;
        };
        let Some(count) = store.get(id).map(|image| image.frames.len()) else {
            return;
        };
        let number = (command.rows as usize).clamp(1, count);
        let removed = store.remove_frame(id, number - 1);
        if !removed && free && store.placements_of(id).next().is_none() {
            store.remove(id);
            self.numbers.retain(|_, image| *image != id);
        }
    }
}

fn no_image(id: u32) -> Error {
    Error::new("ENOENT", format!("no image with id {id}"))
}

fn no_frame(number: u32) -> Error {
    Error::new("ENOENT", format!("no frame number {number}"))
}

/// Draws `rect` of `source` onto the `width` x `height` RGBA `canvas` with its top left
/// corner at `at`, clipped to the canvas: blended over it, or replacing it when
/// `overwrite`.
fn blit(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    source: &Pixels,
    rect: SourceRect,
    at: (u32, u32),
    overwrite: bool,
) {
    let Some(rect) = SourceRect::resolve(Some(rect), source.width, source.height) else {
        return;
    };
    let columns = rect.width.min(width.saturating_sub(at.0)) as usize;
    let rows = rect.height.min(height.saturating_sub(at.1));
    for row in 0..rows {
        let from = (((rect.y + row) * source.width + rect.x) * 4) as usize;
        let to = (((at.1 + row) * width + at.0) * 4) as usize;
        let src = &source.rgba[from..from + columns * 4];
        let dst = &mut canvas[to..to + columns * 4];
        if overwrite {
            dst.copy_from_slice(src);
            continue;
        }
        for (dst, src) in dst
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(src.as_chunks::<4>().0)
        {
            blend(dst, src);
        }
    }
}

/// `src` over `dst`, straight alpha.
fn blend(dst: &mut [u8; 4], src: &[u8; 4]) {
    let (sa, da) = (u32::from(src[3]), u32::from(dst[3]));
    // Alphas scaled to 255 * 255.
    let dst_weight = da * (255 - sa);
    let out = sa * 255 + dst_weight;
    if out == 0 {
        *dst = [0; 4];
        return;
    }
    for channel in 0..3 {
        let value = u32::from(src[channel]) * sa * 255 + u32::from(dst[channel]) * dst_weight;
        dst[channel] = ((value + out / 2) / out) as u8;
    }
    dst[3] = ((out + 127) / 255) as u8;
}

/// Ids of the direct placements of image `id`.
fn internal_placements(store: &ImageStore, id: u32) -> Vec<u32> {
    store
        .placements_of(id)
        .filter(|(_, placement)| placement.kind == PlacementKind::Internal)
        .map(|(placement, _)| placement)
        .collect()
}

fn z_of(store: &ImageStore, id: u32, placement: u32) -> Option<i32> {
    store
        .placements_of(id)
        .find(|&(used, _)| used == placement)
        .map(|(_, value)| value.z)
}

/// The reply to a command: `ESC _ G i=..,I=..,p=..;OK ESC \` or the error, nothing when
/// the command names no image (`i` or `I`) or its `q` mutes it.
fn reply(command: &Command, result: Result<u32, Error>) -> Option<Vec<u8>> {
    if command.id == 0 && command.number == 0 {
        if let Err(err) = &result {
            debug!("kitty graphics command failed: {err}");
        }
        return None;
    }
    let (id, message) = match result {
        Ok(_) if command.quiet >= 1 => return None,
        Err(_) if command.quiet >= 2 => return None,
        Ok(id) => (id, "OK".to_owned()),
        Err(err) => (command.id, err.to_string()),
    };
    let mut keys = Vec::new();
    if id != 0 {
        keys.push(format!("i={id}"));
    }
    if command.number != 0 {
        keys.push(format!("I={}", command.number));
    }
    if command.placement != 0 {
        keys.push(format!("p={}", command.placement));
    }
    Some(format!("\x1b_G{};{message}\x1b\\", keys.join(",")).into_bytes())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;

    use super::*;
    use crate::graphics::placeholder::{ImageCell, row_cells};

    /// A 10x5 terminal with 10x20 pixel cells.
    struct Setup {
        term: Term<VoidListener>,
        store: ImageStore,
        kitty: Kitty,
    }

    impl Setup {
        fn new() -> Self {
            let mut kitty = Kitty::new();
            kitty.set_cell_size(10, 20);
            Self {
                term: Term::new(Config::default(), &TermSize::new(10, 5), VoidListener),
                store: ImageStore::new(),
                kitty,
            }
        }

        /// Runs `control;data`, returns the reply as text.
        fn run(&mut self, control: &str, data: &[u8]) -> Option<String> {
            let payload = format!("{control};{}", BASE64.encode(data));
            self.run_raw(&payload)
        }

        fn run_raw(&mut self, payload: &str) -> Option<String> {
            self.kitty
                .handle(&mut self.term, &mut self.store, payload.as_bytes())
                .map(|reply| String::from_utf8(reply).unwrap())
        }

        /// Image cells of the screen, by line.
        fn cells(&self) -> Vec<Vec<ImageCell>> {
            (0..5)
                .map(|line| {
                    row_cells(&self.term.grid()[Line(line)])
                        .into_iter()
                        .flatten()
                        .collect()
                })
                .collect()
        }

        fn cell_count(&self) -> usize {
            self.cells().iter().map(Vec::len).sum()
        }

        fn cursor(&self) -> (i32, usize) {
            let point = self.term.grid().cursor.point;
            (point.line.0, point.column.0)
        }

        fn goto(&mut self, line: i32, column: usize) {
            self.term.grid_mut().cursor.point = Point::new(Line(line), Column(column));
        }
    }

    fn ok(keys: &str) -> Option<String> {
        Some(format!("\x1b_G{keys};OK\x1b\\"))
    }

    /// `width` x `height` RGBA pixels of value `fill`.
    fn rgba(width: u32, height: u32, fill: u8) -> Vec<u8> {
        vec![fill; (width * height * 4) as usize]
    }

    fn png_bytes(width: u32, height: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(data).unwrap();
        writer.finish().unwrap();
        out
    }

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn command(control: &str) -> Command {
        parse(control.as_bytes()).unwrap().0
    }

    #[test]
    fn parse_reads_the_keys_and_keeps_the_defaults() {
        let (cmd, data) =
            parse(b"a=T,f=100,t=d,i=7,p=3,c=4,r=2,C=1,U=1,z=-5,q=2,m=1,o=z,X=9;QUJD").unwrap();
        assert_eq!(data, b"QUJD");
        assert_eq!(
            cmd,
            Command {
                action: b'T',
                format: 100,
                id: 7,
                placement: 3,
                cols: 4,
                rows: 2,
                keep_cursor: true,
                unicode: true,
                z: -5,
                quiet: 2,
                more: true,
                compression: b'z',
                big_x: 9,
                ..Command::default()
            }
        );

        let (cmd, data) = parse(b"").unwrap();
        assert!(data.is_empty());
        assert_eq!(
            (cmd.action, cmd.format, cmd.medium, cmd.delete),
            (b't', 32, b'd', b'a')
        );
    }

    #[test]
    fn parse_refuses_malformed_keys_and_values() {
        for payload in [&b"i=x"[..], b"a=Tt", b"z=1.5", b"i", b"i=-1"] {
            let err = parse(payload).unwrap_err();
            assert_eq!(err.code, "EINVAL", "{}", text(payload));
        }
    }

    #[test]
    fn raw_rgb_and_rgba_are_decoded() {
        let rgb = decode(&command("f=24,s=2,v=1"), vec![1, 2, 3, 4, 5, 6]).unwrap();
        assert_eq!((rgb.width, rgb.height), (2, 1));
        assert_eq!(rgb.rgba, [1, 2, 3, 255, 4, 5, 6, 255]);

        // Extra bytes are ignored.
        let rgba = decode(&command("f=32,s=1,v=2"), vec![9; 10]).unwrap();
        assert_eq!(rgba.rgba, [9; 8]);
    }

    #[test]
    fn raw_data_needs_its_size_and_enough_bytes() {
        let err = decode(&command("f=32,s=2"), vec![0; 8]).unwrap_err();
        assert_eq!(err.code, "EINVAL");
        let err = decode(&command("f=32,s=2,v=2"), vec![0; 15]).unwrap_err();
        assert_eq!(err.code, "ENODATA");
        let err = decode(&command("f=32,s=100000,v=100000"), vec![0; 4]).unwrap_err();
        assert_eq!(err.code, "EFBIG");
        let err = decode(&command("f=42,s=1,v=1"), vec![0; 4]).unwrap_err();
        assert_eq!(err.code, "EINVAL");
    }

    #[test]
    fn png_is_decoded_to_rgba_whatever_its_color_type() {
        let cases = [
            (
                png::ColorType::Rgba,
                vec![1, 2, 3, 4, 5, 6, 7, 8],
                vec![1, 2, 3, 4, 5, 6, 7, 8],
            ),
            (
                png::ColorType::Rgb,
                vec![1, 2, 3, 4, 5, 6],
                vec![1, 2, 3, 255, 4, 5, 6, 255],
            ),
            (
                png::ColorType::Grayscale,
                vec![7, 9],
                vec![7, 7, 7, 255, 9, 9, 9, 255],
            ),
            (
                png::ColorType::GrayscaleAlpha,
                vec![7, 1, 9, 2],
                vec![7, 7, 7, 1, 9, 9, 9, 2],
            ),
        ];
        for (color, data, expected) in cases {
            let pixels = decode(&command("f=100"), png_bytes(2, 1, color, &data)).unwrap();
            assert_eq!((pixels.width, pixels.height), (2, 1));
            assert_eq!(pixels.rgba, expected, "{color:?}");
        }

        let err = decode(&command("f=100"), b"not a png".to_vec()).unwrap_err();
        assert_eq!(err.code, "EINVAL");
    }

    #[test]
    fn zlib_compressed_data_is_inflated_first() {
        let raw = decode(&command("f=24,s=1,v=1,o=z"), zlib(&[1, 2, 3])).unwrap();
        assert_eq!(raw.rgba, [1, 2, 3, 255]);

        let data = png_bytes(1, 1, png::ColorType::Rgba, &[4, 5, 6, 7]);
        let png = decode(&command("f=100,o=z"), zlib(&data)).unwrap();
        assert_eq!(png.rgba, [4, 5, 6, 7]);

        let err = decode(&command("f=24,s=1,v=1,o=z"), vec![1, 2, 3]).unwrap_err();
        assert_eq!(err.code, "EINVAL");
        let err = decode(&command("f=24,s=1,v=1,o=x"), vec![1, 2, 3]).unwrap_err();
        assert_eq!(err.code, "EINVAL");
    }

    #[test]
    fn direct_data_is_base64_with_or_without_padding() {
        assert_eq!(load(&command("t=d"), b"AQI=").unwrap(), [1, 2]);
        assert_eq!(load(&command("t=d"), b"AQI").unwrap(), [1, 2]);
        assert_eq!(load(&command("t=d"), b"*").unwrap_err().code, "EINVAL");
        assert_eq!(load(&command("t=x"), b"AQI=").unwrap_err().code, "EINVAL");
    }

    /// A unique path in the temporary directory, with the protocol marker.
    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "graphene-{}-{TEMP_MARKER}-{name}",
            std::process::id()
        ))
    }

    fn encoded(path: &Path) -> Vec<u8> {
        BASE64
            .encode(path.to_str().unwrap().as_bytes())
            .into_bytes()
    }

    #[test]
    fn file_medium_reads_a_range_and_keeps_the_file() {
        let path = temp_path("file");
        std::fs::write(&path, b"0123456789").unwrap();
        assert_eq!(
            load(&command("t=f"), &encoded(&path)).unwrap(),
            b"0123456789"
        );
        assert_eq!(
            load(&command("t=f,O=2,S=3"), &encoded(&path)).unwrap(),
            b"234"
        );
        assert!(path.exists());
        std::fs::remove_file(&path).unwrap();

        let err = load(&command("t=f"), &encoded(&path)).unwrap_err();
        assert_eq!(err.code, "EBADF");
        let err = load(&command("t=f"), &encoded(Path::new("/proc/self/status"))).unwrap_err();
        assert_eq!(err.code, "EBADF");
        let err = load(&command("t=f"), &encoded(&std::env::temp_dir())).unwrap_err();
        assert_eq!(err.code, "EBADF");
    }

    #[test]
    fn temporary_file_medium_deletes_the_file_after_reading() {
        let path = temp_path("temp");
        std::fs::write(&path, b"data").unwrap();
        assert_eq!(load(&command("t=t"), &encoded(&path)).unwrap(), b"data");
        assert!(!path.exists());

        // Without the protocol marker, the file is neither read nor deleted.
        let plain = std::env::temp_dir().join(format!("graphene-{}-plain", std::process::id()));
        std::fs::write(&plain, b"data").unwrap();
        let err = load(&command("t=t"), &encoded(&plain)).unwrap_err();
        assert_eq!(err.code, "EBADF");
        assert!(plain.exists());
        std::fs::remove_file(&plain).unwrap();
    }

    #[test]
    fn shared_memory_medium_reads_and_unlinks_the_object() {
        let name = format!("/graphene-test-{}", std::process::id());
        let c_name = CString::new(name.clone()).unwrap();
        // SAFETY: valid NUL-terminated name; the fd is handed to a `File`.
        let fd = unsafe {
            libc::shm_open(
                c_name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                0o600,
            )
        };
        assert!(fd >= 0, "{}", std::io::Error::last_os_error());
        // SAFETY: `fd` was just opened.
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.write_all(b"shared").unwrap();
        drop(file);

        let data = BASE64.encode(name.as_bytes());
        assert_eq!(
            load(&command("t=s,O=1,S=3"), data.as_bytes()).unwrap(),
            b"har"
        );
        // Unlinked: a second read fails.
        let err = load(&command("t=s"), data.as_bytes()).unwrap_err();
        assert_eq!(err.code, "EBADF");
    }

    #[test]
    fn transmit_stores_the_image_and_replies_ok() {
        let mut setup = Setup::new();
        let reply = setup.run("a=t,f=32,s=2,v=1,i=5", &rgba(2, 1, 1));
        assert_eq!(reply, ok("i=5"));
        let image = setup.store.get(5).unwrap();
        assert_eq!((image.frame().width, image.frame().height), (2, 1));
        assert_eq!(setup.cell_count(), 0);
    }

    #[test]
    fn a_command_without_id_gets_no_reply_even_on_error() {
        let mut setup = Setup::new();
        assert_eq!(setup.run("a=t,f=32,s=1,v=1", &rgba(1, 1, 0)), None);
        assert_eq!(setup.store.len(), 1);
        assert_eq!(setup.run("a=t,f=32,s=1,v=9", &rgba(1, 1, 0)), None);
        assert_eq!(setup.store.len(), 1);
    }

    #[test]
    fn image_numbers_get_terminal_ids_from_the_top_of_the_range() {
        let mut setup = Setup::new();
        let reply = setup.run("a=t,f=32,s=1,v=1,I=7", &rgba(1, 1, 0));
        assert_eq!(reply, ok(&format!("i={},I=7", u32::MAX)));
        let reply = setup.run("a=t,f=32,s=1,v=1,I=7", &rgba(1, 1, 0));
        assert_eq!(reply, ok(&format!("i={},I=7", u32::MAX - 1)));

        // A put by number shows the newest image of that number.
        let reply = setup.run_raw("a=p,I=7,p=2");
        assert_eq!(reply, ok(&format!("i={},I=7,p=2", u32::MAX - 1)));
        assert!(setup.store.placement(u32::MAX - 1, 2).is_some());

        let reply = setup.run("a=t,f=32,s=1,v=1,i=1,I=2", &rgba(1, 1, 0));
        assert_eq!(
            reply,
            Some("\x1b_Gi=1,I=2;EINVAL:i and I cannot be used together\x1b\\".to_owned())
        );
    }

    #[test]
    fn quiet_modes_mute_ok_then_errors() {
        let mut setup = Setup::new();
        assert_eq!(setup.run("f=32,s=1,v=1,i=1,q=1", &rgba(1, 1, 0)), None);
        let reply = setup.run("f=32,s=1,v=2,i=1,q=1", &rgba(1, 1, 0)).unwrap();
        assert!(reply.starts_with("\x1b_Gi=1;ENODATA:"), "{reply:?}");
        assert_eq!(setup.run("f=32,s=1,v=2,i=1,q=2", &rgba(1, 1, 0)), None);
    }

    #[test]
    fn error_replies_name_the_failure() {
        let mut setup = Setup::new();
        assert_eq!(
            setup.run_raw("a=p,i=3"),
            Some("\x1b_Gi=3;ENOENT:no image with id 3\x1b\\".to_owned())
        );
        assert_eq!(
            setup.run_raw("a=p,I=3"),
            Some("\x1b_GI=3;ENOENT:no image with number 3\x1b\\".to_owned())
        );
        assert_eq!(
            setup.run_raw("a=w,i=3"),
            Some("\x1b_Gi=3;EINVAL:unknown action: w\x1b\\".to_owned())
        );
        setup.run("i=3,f=32,s=1,v=1", &rgba(1, 1, 0));
        let reply = setup.run_raw("a=p,i=3,x=5").unwrap();
        assert!(reply.contains("EINVAL:the source rectangle"), "{reply:?}");
        let reply = setup.run_raw("a=p,i=3,p=16777216").unwrap();
        assert!(reply.contains("EINVAL:placement id"), "{reply:?}");
    }

    #[test]
    fn query_checks_the_data_without_storing_it() {
        let mut setup = Setup::new();
        assert_eq!(setup.run("a=q,i=31,f=24,s=1,v=1", &[0, 0, 0]), ok("i=31"));
        assert!(setup.store.is_empty());
        let reply = setup.run("a=q,i=31,f=24,s=1,v=1", &[0]).unwrap();
        assert!(reply.contains("ENODATA"), "{reply:?}");
    }

    #[test]
    fn chunked_transfer_is_reassembled_and_replied_once() {
        let mut setup = Setup::new();
        let data = BASE64.encode(rgba(3, 1, 7));
        let (first, rest) = data.split_at(4);
        let (second, third) = rest.split_at(8);
        assert_eq!(
            setup.run_raw(&format!("a=T,f=32,s=3,v=1,i=9,m=1;{first}")),
            None
        );
        assert_eq!(setup.run_raw(&format!("m=1;{second}")), None);
        assert!(setup.store.is_empty());
        assert_eq!(setup.run_raw(&format!("m=0;{third}")), ok("i=9"));
        assert_eq!(setup.store.get(9).unwrap().frame().rgba, rgba(3, 1, 7));
        assert_eq!(setup.cell_count(), 1);

        // The `q` of a later chunk applies to the reply.
        setup.run_raw(&format!("f=32,s=3,v=1,i=9,m=1;{first}"));
        assert_eq!(setup.run_raw(&format!("q=1;{second}{third}")), None);
    }

    #[test]
    fn transmit_and_put_writes_cells_and_moves_the_cursor_after_the_image() {
        let mut setup = Setup::new();
        setup.goto(1, 2);
        // 25x30 pixels on 10x20 cells: 3 columns, 2 rows.
        let reply = setup.run("a=T,f=32,s=25,v=30,i=4", &rgba(25, 30, 0));
        assert_eq!(reply, ok("i=4"));
        let cells = setup.cells();
        assert_eq!(cells[1].len(), 3);
        assert_eq!(cells[2].len(), 3);
        let first = cells[1][0];
        assert_eq!((first.image, first.row, first.col), (4, 0, 0));
        assert_ne!(first.placement, 0);
        assert_eq!(setup.cursor(), (2, 5));
        let placement = setup.store.placement(4, first.placement).unwrap();
        assert_eq!((placement.cols, placement.rows), (3, 2));
        assert_eq!(placement.kind, PlacementKind::Internal);
    }

    #[test]
    fn put_keeps_the_cursor_with_c1_and_sizes_from_c_or_r() {
        let mut setup = Setup::new();
        setup.run("f=32,s=40,v=20,i=4", &rgba(40, 20, 0));

        // `c=2`: 20 pixels wide, so 10 pixels high: 1 row.
        assert_eq!(setup.run_raw("a=p,i=4,p=1,c=2,C=1"), ok("i=4,p=1"));
        assert_eq!(setup.cursor(), (0, 0));
        let placement = setup.store.placement(4, 1).unwrap();
        assert_eq!((placement.cols, placement.rows), (2, 1));

        // `r=3`: 60 pixels high, so 120 pixels wide: 12 columns, clipped at the edge.
        setup.goto(1, 0);
        setup.run_raw("a=p,i=4,p=2,r=3");
        let placement = setup.store.placement(4, 2).unwrap();
        assert_eq!((placement.cols, placement.rows), (12, 3));
        assert_eq!(setup.cells()[1].len(), 10);

        // Source rectangle: 10x20 pixels from (30, 0): 1 cell.
        setup.goto(4, 0);
        setup.run_raw("a=p,i=4,p=3,x=30,w=15,C=1");
        let placement = setup.store.placement(4, 3).unwrap();
        assert_eq!((placement.cols, placement.rows), (1, 1));
        assert_eq!(
            placement.source,
            Some(SourceRect {
                x: 30,
                y: 0,
                width: 10,
                height: 20
            })
        );
    }

    #[test]
    fn a_virtual_placement_writes_no_cell() {
        let mut setup = Setup::new();
        setup.run("f=32,s=10,v=20,i=4", &rgba(10, 20, 0));
        assert_eq!(setup.run_raw("a=p,i=4,U=1,c=3,r=2"), ok("i=4"));
        assert_eq!(setup.cell_count(), 0);
        assert_eq!(setup.cursor(), (0, 0));
        let placement = setup.store.placement(4, 0).unwrap();
        assert_eq!(placement.kind, PlacementKind::Virtual);
        assert_eq!((placement.cols, placement.rows), (3, 2));
    }

    #[test]
    fn replacing_a_placement_or_an_image_erases_the_old_cells() {
        let mut setup = Setup::new();
        setup.run("f=32,s=10,v=20,i=4", &rgba(10, 20, 0));
        setup.run_raw("a=p,i=4,p=1,c=3");
        setup.goto(3, 0);
        setup.run_raw("a=p,i=4,p=1,c=2");
        let cells = setup.cells();
        assert!(cells[0].is_empty());
        assert_eq!(cells[3].len(), 2);

        // Unnamed puts get their own placement ids: both stay.
        setup.goto(0, 0);
        setup.run_raw("a=p,i=4");
        setup.run_raw("a=p,i=4");
        // 2 rows of 2 cells, then 2 single cells.
        assert_eq!(setup.cell_count(), 6);
        assert_eq!(setup.store.placements_of(4).count(), 3);

        setup.run("f=32,s=10,v=20,i=4", &rgba(10, 20, 1));
        assert_eq!(setup.cell_count(), 0);
        assert_eq!(setup.store.placements_of(4).count(), 0);
    }

    /// Image 1 placed at (0, 0) 2x1 with z=1, image 2 at (1, 3) 1x2, image 3 virtual.
    fn deletion_setup() -> Setup {
        let mut setup = Setup::new();
        for id in 1..=3 {
            setup.run(&format!("f=32,s=1,v=1,i={id}"), &rgba(1, 1, 0));
        }
        setup.run_raw("a=p,i=1,p=1,c=2,r=1,z=1");
        setup.goto(1, 3);
        setup.run_raw("a=p,i=2,p=1,c=1,r=2");
        setup.run_raw("a=p,i=3,U=1,p=1,z=1");
        setup
    }

    fn placed(setup: &Setup) -> Vec<u32> {
        let mut ids: Vec<u32> = setup.store.placements().map(|((id, _), _)| id).collect();
        ids.dedup();
        ids
    }

    #[test]
    fn delete_by_id_removes_placements_and_uppercase_frees_the_image() {
        let mut setup = deletion_setup();
        assert_eq!(setup.run_raw("a=d,d=i,i=1"), None);
        assert_eq!(placed(&setup), [2, 3]);
        assert!(setup.store.get(1).is_some());
        assert_eq!(setup.cell_count(), 2);

        setup.run_raw("a=d,d=I,i=2,p=7");
        assert_eq!(placed(&setup), [2, 3]);
        setup.run_raw("a=d,d=I,i=2,p=1");
        assert_eq!(placed(&setup), [3]);
        assert!(setup.store.get(2).is_none());
        assert_eq!(setup.cell_count(), 0);

        // Uppercase frees the image even without placement.
        setup.run_raw("a=d,d=I,i=1");
        assert!(setup.store.get(1).is_none());
    }

    #[test]
    fn delete_by_number_uses_the_newest_image_of_the_number() {
        let mut setup = Setup::new();
        setup.run("a=T,f=32,s=1,v=1,I=5", &rgba(1, 1, 0));
        setup.run_raw("a=d,d=N,I=5");
        assert!(setup.store.is_empty());
        assert_eq!(setup.cell_count(), 0);
        assert_eq!(
            setup.run_raw("a=p,I=5"),
            Some("\x1b_GI=5;ENOENT:no image with number 5\x1b\\".to_owned())
        );
    }

    #[test]
    fn delete_all_on_screen_spares_virtual_placements() {
        let mut setup = deletion_setup();
        setup.run_raw("a=d");
        assert_eq!(placed(&setup), [3]);
        assert_eq!(setup.store.len(), 3);
        assert_eq!(setup.cell_count(), 0);

        let mut setup = deletion_setup();
        setup.run_raw("a=d,d=A");
        assert_eq!(placed(&setup), [3]);
        assert_eq!(setup.store.len(), 1);
        assert!(setup.store.get(3).is_some());
    }

    #[test]
    fn delete_by_position_hits_the_placements_covering_it() {
        let cases = [
            ("d=p,x=2,y=1", vec![2, 3]),
            ("d=p,x=3,y=1", vec![1, 2, 3]),
            ("d=q,x=1,y=1,z=0", vec![1, 2, 3]),
            ("d=q,x=1,y=1,z=1", vec![2, 3]),
            ("d=x,x=4", vec![1, 3]),
            ("d=y,y=3", vec![1, 3]),
            ("d=z,z=1", vec![2]),
            ("d=r,x=2,y=3", vec![1]),
        ];
        for (keys, left) in cases {
            let mut setup = deletion_setup();
            setup.run_raw(&format!("a=d,{keys}"));
            assert_eq!(placed(&setup), left, "{keys}");
            assert_eq!(setup.store.len(), 3, "{keys}");
        }

        let mut setup = deletion_setup();
        setup.run_raw("a=d,d=R,x=2,y=3");
        assert_eq!(setup.store.len(), 1);

        // At the cursor: it is right after image 2.
        let mut setup = deletion_setup();
        setup.goto(2, 3);
        setup.run_raw("a=d,d=C");
        assert_eq!(placed(&setup), [1, 3]);
        assert!(setup.store.get(2).is_none());
    }

    #[test]
    fn reset_forgets_numbers_and_pending_transfers() {
        let mut setup = Setup::new();
        setup.run("f=32,s=1,v=1,I=5", &rgba(1, 1, 0));
        setup.run_raw("f=32,s=1,v=1,i=6,m=1;AAAA");
        setup.kitty.reset();
        setup.store.clear();
        // Not taken for the end of the transfer: a command of its own.
        assert_eq!(setup.run("f=32,s=1,v=1,i=7", &rgba(1, 1, 0)), ok("i=7"));
        assert!(setup.run_raw("a=p,I=5").unwrap().contains("ENOENT"));
    }

    /// Image 1: a 2x1 RGBA root frame of red pixels, placed.
    fn animation_setup() -> Setup {
        let mut setup = Setup::new();
        setup.run(
            "a=T,f=32,s=2,v=1,i=1,q=1",
            &[255, 0, 0, 255, 255, 0, 0, 255],
        );
        setup
    }

    fn frame(setup: &Setup, number: usize) -> Vec<u8> {
        setup.store.get(1).unwrap().frames[number - 1].rgba.clone()
    }

    #[test]
    fn a_frame_covers_the_whole_image_by_default_with_its_gap() {
        let mut setup = animation_setup();
        let green = [0, 255, 0, 255, 0, 255, 0, 255];
        assert_eq!(setup.run("a=f,f=32,i=1", &green), ok("i=1"));
        assert_eq!(setup.run("a=f,f=32,i=1,z=70,q=1", &green), None);
        assert_eq!(setup.run("a=f,f=32,i=1,z=-1,q=1", &green), None);
        let image = setup.store.get(1).unwrap();
        assert_eq!(image.frames.len(), 4);
        assert_eq!(image.gaps, [0, DEFAULT_GAP, 70, 0]);
        assert_eq!(frame(&setup, 2), green);
        // The shown frame does not change.
        assert_eq!(image.current, 0);
    }

    #[test]
    fn a_partial_frame_is_drawn_over_its_base_or_background() {
        let mut setup = animation_setup();
        let blue = [0, 0, 255, 255];
        // Over a transparent background by default.
        setup.run("a=f,f=32,s=1,v=1,x=1,i=1", &blue);
        assert_eq!(frame(&setup, 2), [0, 0, 0, 0, 0, 0, 255, 255]);
        // Over a background color.
        setup.run("a=f,f=32,s=1,v=1,i=1,Y=4294967295", &blue);
        assert_eq!(frame(&setup, 3), [0, 0, 255, 255, 255, 255, 255, 255]);
        // Over frame 1, half transparent: blended.
        setup.run("a=f,f=32,s=1,v=1,x=1,c=1,i=1", &[0, 0, 255, 128]);
        assert_eq!(frame(&setup, 4), [255, 0, 0, 255, 127, 0, 128, 255]);
        // Overwriting.
        setup.run("a=f,f=32,s=1,v=1,c=1,X=1,i=1", &[0, 0, 255, 128]);
        assert_eq!(frame(&setup, 5), [0, 0, 255, 128, 255, 0, 0, 255]);
        // Clipped to the image.
        setup.run(
            "a=f,f=32,s=2,v=1,x=1,c=1,i=1",
            &[1, 1, 1, 255, 2, 2, 2, 255],
        );
        assert_eq!(frame(&setup, 6), [255, 0, 0, 255, 1, 1, 1, 255]);
    }

    #[test]
    fn a_frame_can_be_edited_in_place() {
        let mut setup = animation_setup();
        setup.run("a=f,f=32,i=1,z=50", &[0; 8]);
        setup.run("a=f,f=32,s=1,v=1,r=2,i=1", &[9, 9, 9, 255]);
        let image = setup.store.get(1).unwrap();
        assert_eq!(image.frames.len(), 2);
        assert_eq!(image.gaps[1], 50);
        assert_eq!(frame(&setup, 2), [9, 9, 9, 255, 0, 0, 0, 0]);
        // The root frame too.
        setup.run("a=f,f=32,s=1,v=1,r=1,z=30,i=1", &[1, 2, 3, 255]);
        assert_eq!(frame(&setup, 1)[..4], [1, 2, 3, 255]);
        assert_eq!(setup.store.get(1).unwrap().gaps[0], 30);
    }

    #[test]
    fn frame_errors_are_replied() {
        let mut setup = animation_setup();
        let reply = setup.run("a=f,f=32,i=2", &[0; 8]).unwrap();
        assert!(reply.contains("ENOENT"), "{reply}");
        let reply = setup.run("a=f,f=32,c=3,i=1", &[0; 8]).unwrap();
        assert!(reply.contains("ENOENT"), "{reply}");
        let reply = setup.run("a=f,f=32,r=2,i=1", &[0; 8]).unwrap();
        assert!(reply.contains("ENOENT"), "{reply}");
        assert_eq!(setup.store.get(1).unwrap().frames.len(), 1);
        // By image number.
        setup.run("a=t,f=32,s=1,v=1,I=4,q=2", &[0; 4]);
        let reply = setup.run("a=f,f=32,I=4", &[0; 4]).unwrap();
        assert!(
            reply.ends_with(";OK\x1b\\") && reply.contains("I=4"),
            "{reply}"
        );
    }

    #[test]
    fn a_chunked_frame_is_reassembled() {
        let mut setup = animation_setup();
        let data = BASE64.encode([5u8; 8]);
        let (first, rest) = data.split_at(4);
        assert_eq!(setup.run_raw(&format!("a=f,f=32,i=1,m=1;{first}")), None);
        assert_eq!(setup.run_raw(&format!("a=f,m=0;{rest}")), ok("i=1"));
        assert_eq!(frame(&setup, 2), [5; 8]);
    }

    #[test]
    fn control_sets_state_loops_gaps_and_the_shown_frame() {
        let mut setup = animation_setup();
        setup.run("a=f,f=32,i=1", &[0; 8]);
        assert_eq!(setup.run_raw("a=a,i=1,r=1,z=120"), None);
        assert_eq!(setup.run_raw("a=a,i=1,s=3,v=4"), None);
        let image = setup.store.get(1).unwrap();
        assert_eq!(image.gaps, [120, DEFAULT_GAP]);
        assert_eq!(image.animation.state, AnimationState::Running);
        assert_eq!(image.animation.max_loops, 3);

        setup.run_raw("a=a,i=1,c=2,s=1");
        let image = setup.store.get(1).unwrap();
        assert_eq!(image.current, 1);
        assert_eq!(image.animation.state, AnimationState::Stopped);
        setup.run_raw("a=a,i=1,s=2,v=1,r=2,z=-1");
        let image = setup.store.get(1).unwrap();
        assert_eq!(image.animation.state, AnimationState::Loading);
        assert_eq!(image.animation.max_loops, 0);
        assert_eq!(image.gaps[1], 0);

        // Only errors are replied.
        let reply = setup.run_raw("a=a,i=1,c=3").unwrap();
        assert!(reply.contains("ENOENT"), "{reply}");
        let reply = setup.run_raw("a=a,i=9,s=3").unwrap();
        assert!(reply.contains("ENOENT"), "{reply}");
    }

    #[test]
    fn compose_copies_a_rectangle_between_frames() {
        let mut setup = animation_setup();
        setup.run("a=f,f=32,i=1", &[0, 0, 255, 255, 0, 255, 0, 255]);
        // Pixel 2 of frame 2 onto pixel 1 of frame 1, overwriting.
        assert_eq!(setup.run_raw("a=c,i=1,r=2,c=1,X=1,w=1,h=1,C=1"), ok("i=1"));
        assert_eq!(frame(&setup, 1), [0, 255, 0, 255, 255, 0, 0, 255]);
        // The whole frame by default.
        setup.run_raw("a=c,i=1,r=1,c=2,q=1");
        assert_eq!(frame(&setup, 2), frame(&setup, 1));
        // Within one frame, rectangles must not overlap.
        assert_eq!(setup.run_raw("a=c,i=1,r=1,c=1,x=1,w=1,h=1,q=1"), None);
        assert_eq!(frame(&setup, 1)[4..], [0, 255, 0, 255]);
        let reply = setup.run_raw("a=c,i=1,r=1,c=1,w=1,h=1").unwrap();
        assert!(reply.contains("EINVAL"), "{reply}");
        let reply = setup.run_raw("a=c,i=1,r=1,c=2,x=1").unwrap();
        assert!(reply.contains("EINVAL"), "{reply}");
        let reply = setup.run_raw("a=c,i=1,r=3,c=1").unwrap();
        assert!(reply.contains("ENOENT"), "{reply}");
        let reply = setup.run_raw("a=c,i=1,c=1").unwrap();
        assert!(reply.contains("ENOENT"), "{reply}");
    }

    #[test]
    fn delete_frames_keeps_the_last_one() {
        let mut setup = animation_setup();
        setup.run("a=f,f=32,i=1,q=1", &[1; 8]);
        setup.run("a=f,f=32,i=1,q=1", &[2; 8]);
        setup.run_raw("a=d,d=f,i=1,r=9");
        assert_eq!(setup.store.get(1).unwrap().frames.len(), 2);
        assert_eq!(frame(&setup, 2), [1; 8]);
        // Frame 1 by default.
        setup.run_raw("a=d,d=f,i=1");
        assert_eq!(frame(&setup, 1), [1; 8]);
        setup.run_raw("a=d,d=F,i=1");
        assert_eq!(setup.store.get(1).unwrap().frames.len(), 1);
        // Still placed: not freed.
        setup.run_raw("a=d,d=F,i=1");
        assert!(setup.store.get(1).is_some());
        // Not placed any more: freed.
        setup.run_raw("a=d,d=i,i=1");
        setup.run("a=t,f=32,s=1,v=1,i=1,q=2", &[0; 4]);
        setup.run_raw("a=d,d=F,i=1");
        assert!(setup.store.get(1).is_none());
    }

    #[test]
    fn blending_follows_straight_alpha() {
        let mut dst = [0, 0, 0, 0];
        blend(&mut dst, &[10, 20, 30, 0]);
        assert_eq!(dst, [0, 0, 0, 0]);
        blend(&mut dst, &[10, 20, 30, 200]);
        assert_eq!(dst, [10, 20, 30, 200]);
        let mut dst = [200, 100, 0, 255];
        blend(&mut dst, &[0, 0, 0, 255]);
        assert_eq!(dst, [0, 0, 0, 255]);
    }
}
