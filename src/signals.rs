//! Private block signals scanner: finds Graphene's own OSC sequences, and the screen
//! clears, in the PTY byte stream.
//!
//! The shell hooks tell Graphene when a command starts and ends with a private OSC
//! carrying the session token:
//!
//! ```text
//! ESC ] 6973 ; <token> ; start ; <command line, base64> ST
//! ESC ] 6973 ; <token> ; end ; <exit status> ST
//! ```
//!
//! `vte` drops unknown OSC without calling any handler, so the signals are found here,
//! before the parser. The scanner is incremental (a sequence split across reads is
//! still found) and never consumes bytes: every byte, signals included, comes back as
//! [`Chunk::Bytes`] so the `vte` parser still sees the whole stream.

/// The private OSC number used for block signals. Checked unused in xterm's ctlseqs,
/// kitty, iTerm2, WezTerm and foot.
pub const GRAPHENE_OSC: u32 = 6973;

/// Longest decoded command line carried by a `start` signal; longer ones are reported
/// empty.
pub const MAX_COMMAND_LEN: usize = 64 * 1024;

/// OSC payload length after which a sequence that is not a signal is abandoned.
const MAX_OSC_LEN: usize = 4096;

/// OSC payload length after which a signal stops being collected: the base64 of the
/// longest command line, plus room for the number, the token and the kind.
const MAX_SIGNAL_LEN: usize = MAX_COMMAND_LEN.div_ceil(3) * 4 + 256;

/// Parameter bytes kept for a CSI sequence; longer parameters are never a clear.
const MAX_CSI_PARAMS: usize = 16;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
/// CAN and SUB abort a control sequence.
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;

/// A block signal sent by the shell hooks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// A command starts. `command` is empty when the hook could not send it (too long,
    /// not valid base64).
    Start { command: String },
    /// The command ended, with its exit status when given.
    End { status: Option<i32> },
}

/// A piece of the byte stream returned by [`Scanner::split`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chunk<'a> {
    /// Bytes to hand to the parser, in order.
    Bytes(&'a [u8]),
    /// A signal whose sequence ends with the previous `Bytes` chunk.
    Signal(Signal),
    /// `CSI 2 J` or `CSI 3 J`, ending with the previous `Bytes` chunk.
    Clear,
}

/// What a finished sequence reports.
enum Event {
    Signal(Signal),
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum State {
    #[default]
    Ground,
    /// `ESC` seen outside an OSC.
    Escape,
    /// Inside `ESC [`, collecting the parameters.
    Csi,
    /// Inside `ESC ]`, collecting the payload.
    Osc,
    /// `ESC` seen inside an OSC: `\` ends it.
    OscEscape,
    /// Overlong OSC: skipped up to its terminator.
    Abandoned,
    /// `ESC` seen inside an abandoned OSC.
    AbandonedEscape,
}

/// Incremental finder of the block signals and screen clears. Keeps its state between
/// reads.
#[derive(Debug)]
pub struct Scanner {
    /// `6973;<token>;`: the prefix of every signal of this session.
    prefix: Vec<u8>,
    state: State,
    payload: Vec<u8>,
    /// The signal payload went over [`MAX_SIGNAL_LEN`]: the rest was dropped.
    overflowed: bool,
    /// CSI parameters, or `None` once they can no longer form a clear.
    csi: Option<Vec<u8>>,
}

impl Scanner {
    /// A scanner reporting only the signals carrying `token`. An empty token matches
    /// no signal.
    pub fn new(token: &str) -> Self {
        let prefix = if token.is_empty() {
            Vec::new()
        } else {
            format!("{GRAPHENE_OSC};{token};").into_bytes()
        };
        Self {
            prefix,
            state: State::Ground,
            payload: Vec::new(),
            overflowed: false,
            csi: None,
        }
    }

    /// Splits `bytes` into byte chunks and the signals or clears found at their end.
    /// Concatenating the `Bytes` chunks gives back `bytes` unchanged.
    pub fn split<'s, 'a>(&'s mut self, bytes: &'a [u8]) -> Split<'s, 'a> {
        Split {
            scanner: self,
            bytes,
            pos: 0,
            pending: None,
        }
    }

    /// Advances the state machine by one byte; returns an event when a sequence ends.
    fn step(&mut self, byte: u8) -> Option<Event> {
        match self.state {
            State::Ground => {
                if byte == ESC {
                    self.state = State::Escape;
                }
                None
            }
            State::Escape => {
                self.escape(byte);
                None
            }
            State::Csi => self.csi(byte),
            State::Osc => {
                match byte {
                    BEL => return self.finish(),
                    ESC => self.state = State::OscEscape,
                    CAN | SUB => self.reset(),
                    _ => self.push(byte),
                }
                None
            }
            State::OscEscape => {
                if byte == b'\\' {
                    return self.finish();
                }
                // Any other byte aborts the OSC and starts a new escape sequence.
                self.payload.clear();
                self.overflowed = false;
                self.escape(byte);
                None
            }
            State::Abandoned => {
                match byte {
                    BEL | CAN | SUB => self.state = State::Ground,
                    ESC => self.state = State::AbandonedEscape,
                    _ => {}
                }
                None
            }
            State::AbandonedEscape => {
                if byte == b'\\' {
                    self.state = State::Ground;
                } else {
                    self.escape(byte);
                }
                None
            }
        }
    }

    /// Handles the byte following an `ESC`.
    fn escape(&mut self, byte: u8) {
        self.state = match byte {
            b']' => {
                self.payload.clear();
                self.overflowed = false;
                State::Osc
            }
            b'[' => {
                self.csi = Some(Vec::new());
                State::Csi
            }
            ESC => State::Escape,
            _ => State::Ground,
        };
    }

    /// Handles a byte inside a CSI sequence.
    fn csi(&mut self, byte: u8) -> Option<Event> {
        match byte {
            ESC => {
                self.csi = None;
                self.state = State::Escape;
                None
            }
            CAN | SUB => {
                self.csi = None;
                self.state = State::Ground;
                None
            }
            // Parameter bytes.
            0x30..=0x3f => {
                if let Some(params) = &mut self.csi {
                    if params.len() < MAX_CSI_PARAMS {
                        params.push(byte);
                    } else {
                        self.csi = None;
                    }
                }
                None
            }
            // Intermediate bytes: never part of an erase in display.
            0x20..=0x2f => {
                self.csi = None;
                None
            }
            // Final byte.
            0x40..=0x7e => {
                let params = self.csi.take();
                self.state = State::Ground;
                let clear = byte == b'J' && matches!(params.as_deref(), Some(b"2" | b"3"));
                clear.then_some(Event::Clear)
            }
            // C0 controls are executed inside a CSI without ending it; other bytes
            // spoil the sequence.
            0x00..=0x1f => None,
            _ => {
                self.csi = None;
                None
            }
        }
    }

    /// Adds a byte to the OSC payload, within the caps.
    fn push(&mut self, byte: u8) {
        if self.overflowed {
            return;
        }
        if self.payload.len() < MAX_OSC_LEN {
            self.payload.push(byte);
        } else if self.is_signal() {
            if self.payload.len() < MAX_SIGNAL_LEN {
                self.payload.push(byte);
            } else {
                self.overflowed = true;
            }
        } else {
            self.payload.clear();
            self.state = State::Abandoned;
        }
    }

    /// Whether the payload collected so far is a signal of this session.
    fn is_signal(&self) -> bool {
        !self.prefix.is_empty() && self.payload.starts_with(&self.prefix)
    }

    fn reset(&mut self) {
        self.payload.clear();
        self.overflowed = false;
        self.state = State::Ground;
    }

    /// Ends the current OSC and parses its payload.
    fn finish(&mut self) -> Option<Event> {
        let signal = if self.is_signal() {
            parse(&self.payload[self.prefix.len()..], self.overflowed)
        } else {
            None
        };
        self.reset();
        signal.map(Event::Signal)
    }
}

/// Iterator returned by [`Scanner::split`].
pub struct Split<'s, 'a> {
    scanner: &'s mut Scanner,
    bytes: &'a [u8],
    pos: usize,
    pending: Option<Event>,
}

impl<'a> Iterator for Split<'_, 'a> {
    type Item = Chunk<'a>;

    fn next(&mut self) -> Option<Chunk<'a>> {
        if let Some(event) = self.pending.take() {
            return Some(match event {
                Event::Signal(signal) => Chunk::Signal(signal),
                Event::Clear => Chunk::Clear,
            });
        }
        if self.pos >= self.bytes.len() {
            return None;
        }
        let start = self.pos;
        for (offset, &byte) in self.bytes[start..].iter().enumerate() {
            if let Some(event) = self.scanner.step(byte) {
                let end = start + offset + 1;
                self.pos = end;
                self.pending = Some(event);
                return Some(Chunk::Bytes(&self.bytes[start..end]));
            }
        }
        self.pos = self.bytes.len();
        Some(Chunk::Bytes(&self.bytes[start..]))
    }
}

/// Parses what follows `6973;<token>;`. `overflowed` means the payload was cut.
fn parse(rest: &[u8], overflowed: bool) -> Option<Signal> {
    let mut parts = rest.splitn(2, |&b| b == b';');
    let kind = parts.next()?;
    let value = parts.next().unwrap_or_default();
    match kind {
        b"start" => {
            let command = if overflowed {
                String::new()
            } else {
                decode_base64(value)
                    .filter(|bytes| bytes.len() <= MAX_COMMAND_LEN)
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                    .unwrap_or_default()
            };
            Some(Signal::Start { command })
        }
        b"end" if !overflowed => {
            let status = std::str::from_utf8(value)
                .ok()
                .and_then(|value| value.parse().ok());
            Some(Signal::End { status })
        }
        _ => None,
    }
}

/// Decodes standard base64, padded or not, ignoring ASCII whitespace. `None` when a
/// byte is outside the alphabet or the length is impossible.
fn decode_base64(input: &[u8]) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        Some(match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    }

    let digits: Vec<u8> = input
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    let padding = digits.iter().rev().take_while(|&&b| b == b'=').count();
    if padding > 2 || (padding > 0 && !digits.len().is_multiple_of(4)) {
        return None;
    }
    let digits = &digits[..digits.len() - padding];
    if digits.len() % 4 == 1 {
        return None;
    }

    let mut out = Vec::with_capacity(digits.len() / 4 * 3 + 2);
    for group in digits.chunks(4) {
        let mut acc = 0u32;
        for &byte in group {
            acc = (acc << 6) | value(byte)?;
        }
        acc <<= 6 * (4 - group.len() as u32);
        let bytes = acc.to_be_bytes();
        out.extend_from_slice(&bytes[1..group.len()]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    /// What a read gave, bytes left out.
    #[derive(Debug, PartialEq, Eq)]
    enum Seen {
        Signal(Signal),
        Clear,
    }

    /// Feeds `reads` through one scanner; returns the concatenated bytes and the events.
    fn scan(reads: &[&[u8]]) -> (Vec<u8>, Vec<Seen>) {
        let mut scanner = Scanner::new(TOKEN);
        let mut bytes = Vec::new();
        let mut seen = Vec::new();
        for read in reads {
            for chunk in scanner.split(read) {
                match chunk {
                    Chunk::Bytes(b) => bytes.extend_from_slice(b),
                    Chunk::Signal(s) => seen.push(Seen::Signal(s)),
                    Chunk::Clear => seen.push(Seen::Clear),
                }
            }
        }
        (bytes, seen)
    }

    fn events(reads: &[&[u8]]) -> Vec<Seen> {
        let (bytes, seen) = scan(reads);
        assert_eq!(bytes, reads.concat(), "bytes must pass through unchanged");
        seen
    }

    fn encode_base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for group in bytes.chunks(3) {
            let mut acc = 0u32;
            for (i, &b) in group.iter().enumerate() {
                acc |= (b as u32) << (16 - 8 * i);
            }
            for i in 0..4 {
                if i <= group.len() {
                    out.push(ALPHABET[((acc >> (18 - 6 * i)) & 0x3f) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    fn start(command: &str) -> Vec<u8> {
        format!(
            "\x1b]6973;{TOKEN};start;{}\x1b\\",
            encode_base64(command.as_bytes())
        )
        .into_bytes()
    }

    fn started(command: &str) -> Seen {
        Seen::Signal(Signal::Start {
            command: command.to_owned(),
        })
    }

    fn ended(status: Option<i32>) -> Seen {
        Seen::Signal(Signal::End { status })
    }

    #[test]
    fn start_and_end() {
        let mut stream = b"$ ".to_vec();
        stream.extend(start("ls -l"));
        stream.extend_from_slice(b"out\n");
        stream.extend_from_slice(format!("\x1b]6973;{TOKEN};end;0\x1b\\").as_bytes());
        assert_eq!(events(&[&stream]), [started("ls -l"), ended(Some(0))]);
    }

    #[test]
    fn both_terminators() {
        let bel = format!("\x1b]6973;{TOKEN};end;1\x07");
        let st = format!("\x1b]6973;{TOKEN};end;2\x1b\\");
        assert_eq!(
            events(&[bel.as_bytes(), st.as_bytes()]),
            [ended(Some(1)), ended(Some(2))]
        );
    }

    #[test]
    fn end_status_forms() {
        let end = |value: &str| format!("\x1b]6973;{TOKEN};end{value}\x07");
        assert_eq!(events(&[end(";-1").as_bytes()]), [ended(Some(-1))]);
        assert_eq!(events(&[end(";127").as_bytes()]), [ended(Some(127))]);
        assert_eq!(events(&[end("").as_bytes()]), [ended(None)]);
        assert_eq!(events(&[end(";").as_bytes()]), [ended(None)]);
        assert_eq!(events(&[end(";oops").as_bytes()]), [ended(None)]);
    }

    #[test]
    fn signal_follows_the_bytes_of_its_sequence() {
        let mut scanner = Scanner::new(TOKEN);
        let end = format!("ab\x1b]6973;{TOKEN};end;0\x07cd");
        let chunks: Vec<_> = scanner.split(end.as_bytes()).collect();
        assert_eq!(
            chunks,
            [
                Chunk::Bytes(&end.as_bytes()[..end.len() - 2]),
                Chunk::Signal(Signal::End { status: Some(0) }),
                Chunk::Bytes(b"cd"),
            ]
        );
    }

    #[test]
    fn wrong_or_missing_token_is_passed_through() {
        let other = "ffffffffffffffffffffffffffffffff";
        let stream = format!(
            "\x1b]6973;{other};end;0\x07\x1b]6973;;end;0\x07\x1b]6973;end;0\x07\
             \x1b]6973;{TOKEN}x;end;0\x07"
        );
        assert!(events(&[stream.as_bytes()]).is_empty());
    }

    #[test]
    fn empty_token_matches_nothing() {
        let mut scanner = Scanner::new("");
        assert!(
            scanner
                .split(b"\x1b]6973;;end;0\x07")
                .all(|chunk| matches!(chunk, Chunk::Bytes(_)))
        );
    }

    #[test]
    fn unknown_kind_is_ignored() {
        let stream = format!("\x1b]6973;{TOKEN};prompt;x\x07\x1b]6973;{TOKEN}\x07");
        assert!(events(&[stream.as_bytes()]).is_empty());
    }

    #[test]
    fn command_may_hold_control_characters() {
        let command = "printf 'a;b\x07c\x1b'";
        assert_eq!(events(&[&start(command)]), [started(command)]);
    }

    #[test]
    fn invalid_base64_gives_an_empty_command() {
        let stream = format!(
            "\x1b]6973;{TOKEN};start;not base64!\x07\x1b]6973;{TOKEN};start;QQ=\x07\
             \x1b]6973;{TOKEN};start\x07"
        );
        assert_eq!(
            events(&[stream.as_bytes()]),
            [started(""), started(""), started("")]
        );
    }

    #[test]
    fn unpadded_base64_is_accepted() {
        let stream = format!("\x1b]6973;{TOKEN};start;bHM\x07");
        assert_eq!(events(&[stream.as_bytes()]), [started("ls")]);
    }

    #[test]
    fn command_at_the_cap_is_kept() {
        let command = "x".repeat(MAX_COMMAND_LEN);
        assert_eq!(events(&[&start(&command)]), [started(&command)]);
    }

    #[test]
    fn command_over_the_cap_is_empty() {
        let command = "x".repeat(MAX_COMMAND_LEN + 1);
        assert_eq!(events(&[&start(&command)]), [started("")]);
        // Far over the cap: the payload is cut but the signal still comes through.
        let command = "x".repeat(MAX_COMMAND_LEN * 3);
        let mut stream = start(&command);
        stream.extend(start("next"));
        assert_eq!(events(&[&stream]), [started(""), started("next")]);
    }

    #[test]
    fn overlong_unrelated_osc_is_abandoned() {
        let mut long = b"\x1b]0;".to_vec();
        long.extend(std::iter::repeat_n(b'x', MAX_OSC_LEN + 10));
        long.extend_from_slice(b"\x1b\\");
        long.extend_from_slice(format!("\x1b]6973;{TOKEN};end;3\x07").as_bytes());
        assert_eq!(events(&[&long]), [ended(Some(3))]);
    }

    #[test]
    fn sequence_split_across_reads() {
        let stream = start("make test");
        for cut in 0..=stream.len() {
            assert_eq!(
                events(&[&stream[..cut], &stream[cut..]]),
                [started("make test")],
                "cut at {cut}"
            );
        }
        let end = format!("\x1b]6973;{TOKEN};end;42\x1b\\");
        let end = end.as_bytes();
        assert_eq!(
            events(&[
                &end[..1],
                &end[1..10],
                &end[10..end.len() - 1],
                &end[end.len() - 1..]
            ]),
            [ended(Some(42))]
        );
    }

    #[test]
    fn cancelled_signal_is_dropped() {
        let stream = format!("\x1b]6973;{TOKEN};end;0\x18\x07");
        assert!(events(&[stream.as_bytes()]).is_empty());
    }

    #[test]
    fn osc_133_and_8_are_no_longer_reported() {
        let stream = b"\x1b]133;A\x07$ \x1b]133;B\x07ls\n\x1b]133;C\x07out\n\
            \x1b]133;D;0\x07\x1b]8;;http://x\x07link\x1b]8;;\x07";
        assert!(events(&[stream]).is_empty());
    }

    #[test]
    fn clear_screen_and_scrollback() {
        assert_eq!(events(&[b"\x1b[H\x1b[2J"]), [Seen::Clear]);
        assert_eq!(events(&[b"\x1b[3J"]), [Seen::Clear]);
        assert_eq!(
            events(&[b"\x1b[H\x1b[2J\x1b[3J"]),
            [Seen::Clear, Seen::Clear]
        );
    }

    #[test]
    fn clear_split_across_reads() {
        assert_eq!(events(&[b"\x1b", b"[", b"2", b"J"]), [Seen::Clear]);
    }

    #[test]
    fn other_csi_are_ignored() {
        let stream = b"\x1b[J\x1b[0J\x1b[1J\x1b[22J\x1b[?2J\x1b[2;3J\x1b[2 J\x1b[2K\
            \x1b[31m\x1b[2H";
        assert!(events(&[stream]).is_empty());
    }

    #[test]
    fn interrupted_csi_is_not_a_clear() {
        assert!(events(&[b"\x1b[2\x18J\x1b[2\x1b[31m"]).is_empty());
        // ESC inside a CSI starts a new sequence, which can be a clear.
        assert_eq!(events(&[b"\x1b[2\x1b[3J"]), [Seen::Clear]);
    }

    #[test]
    fn control_inside_csi_keeps_the_sequence() {
        assert_eq!(events(&[b"\x1b[2\nJ"]), [Seen::Clear]);
    }

    #[test]
    fn esc_bracket_inside_osc_aborts_it() {
        let stream = b"\x1b]0;\x1b[2J\x07";
        // The ESC aborts the OSC and starts the CSI, as vte does.
        assert_eq!(events(&[stream]), [Seen::Clear]);
    }

    #[test]
    fn empty_input_gives_no_chunk() {
        let mut scanner = Scanner::new(TOKEN);
        assert_eq!(scanner.split(b"").count(), 0);
    }

    #[test]
    fn decode_base64_rejects_bad_lengths() {
        assert_eq!(decode_base64(b"").as_deref(), Some(&b""[..]));
        assert_eq!(decode_base64(b"QUJD").as_deref(), Some(&b"ABC"[..]));
        assert_eq!(decode_base64(b"QUI=").as_deref(), Some(&b"AB"[..]));
        assert_eq!(decode_base64(b"QQ==").as_deref(), Some(&b"A"[..]));
        assert_eq!(decode_base64(b"QUJ\nD").as_deref(), Some(&b"ABC"[..]));
        assert!(decode_base64(b"Q").is_none());
        assert!(decode_base64(b"QQ===").is_none());
        assert!(decode_base64(b"Q=Q=").is_none());
    }
}
