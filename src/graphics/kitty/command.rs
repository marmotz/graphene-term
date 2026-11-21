//! Parsing of the control keys of a graphics command, and the errors it replies.

use super::*;

/// A failed command, replied as `CODE:message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
}

impl Error {
    pub(super) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub(super) fn invalid(message: impl Into<String>) -> Self {
        Self::new("EINVAL", message)
    }

    pub(super) fn too_big() -> Self {
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

pub(super) fn text(value: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(value)
}

pub(super) fn letter(key: u8, value: &[u8]) -> Result<u8, Error> {
    match value {
        [letter] => Ok(*letter),
        _ => Err(Error::invalid(format!(
            "bad value for {}: {}",
            key as char,
            text(value)
        ))),
    }
}

pub(super) fn number(key: u8, value: &[u8]) -> Result<u32, Error> {
    text(value)
        .parse()
        .map_err(|_| Error::invalid(format!("bad value for {}: {}", key as char, text(value))))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
