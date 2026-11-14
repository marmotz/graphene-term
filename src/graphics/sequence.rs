//! Incremental splitter of the image sequences in the PTY stream (pure).
//!
//! Cuts kitty graphics commands (`ESC _ G ... ESC \`) and sixel images (DCS with final
//! byte `q` and no intermediate, `ESC P <params> q ... ESC \`) out of the raw PTY bytes.
//! Every other byte, other APC and DCS strings included, is passed through untouched so
//! `vte` keeps its behaviour. A full reset (`ESC c`) in the passed bytes is flagged right
//! after them. The state survives across reads.

use std::borrow::Cow;

use log::warn;

/// Max payload bytes of one image sequence (the image store budget). A longer sequence
/// is dropped up to its terminator.
pub const PAYLOAD_LIMIT: usize = 256 << 20;

/// Max DCS parameter bytes before `q`; longer DCS strings are passed through.
const MAX_DCS_PARAMS: usize = 64;

const ESC: u8 = 0x1b;
/// `CAN` and `SUB` abort a control string.
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;

/// A piece of the PTY stream, in stream order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chunk<'a> {
    /// Bytes for the VT parser. Borrowed from the read, or owned when they are the start
    /// of an escape sequence carried over from the previous read.
    Vt(Cow<'a, [u8]>),
    /// Payload of `ESC _ G ... ESC \`, without the leading `G`.
    Kitty(Vec<u8>),
    /// Body of `ESC P <params> q ... ESC \`: params, `q` and data.
    Sixel(Vec<u8>),
    /// The `Vt` chunk just before ends with a full reset (`ESC c`).
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Kitty,
    Sixel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Plain bytes.
    Ground,
    /// After `ESC`.
    Esc,
    /// After `ESC _`.
    Apc,
    /// After `ESC P` and parameter bytes.
    Dcs,
    /// Inside an image sequence; `esc` when the last byte was `ESC`.
    Payload { kind: Kind, esc: bool },
}

/// Splits the PTY stream into VT bytes and image sequences.
#[derive(Debug)]
pub struct Splitter {
    state: State,
    /// Bytes of the escape sequence being recognised (`Esc`, `Apc`, `Dcs` states).
    prefix: Vec<u8>,
    /// Payload of the image sequence being read.
    payload: Vec<u8>,
    /// The current payload went over `limit` and is being dropped.
    overflow: bool,
    limit: usize,
}

impl Default for Splitter {
    fn default() -> Self {
        Self::new()
    }
}

impl Splitter {
    pub fn new() -> Self {
        Self::with_limit(PAYLOAD_LIMIT)
    }

    /// A splitter dropping payloads longer than `limit` bytes.
    pub fn with_limit(limit: usize) -> Self {
        Self {
            state: State::Ground,
            prefix: Vec::new(),
            payload: Vec::new(),
            overflow: false,
            limit,
        }
    }

    /// Splits one read. Bytes of a sequence not finished at the end of `bytes` are kept
    /// for the next call.
    pub fn feed<'a>(&mut self, bytes: &'a [u8]) -> Vec<Chunk<'a>> {
        let mut out = Vec::new();
        // The escape sequence being recognised started in an earlier read: its bytes in
        // `prefix` were not emitted yet.
        let mut carried = matches!(self.state, State::Esc | State::Apc | State::Dcs);
        // Length of `prefix` coming from earlier reads.
        let mut carried_len = self.prefix.len();
        // Index in `bytes` of the `ESC` being recognised, when not carried.
        let mut prefix_start = 0;
        // Start of the VT bytes not emitted yet; `None` inside a payload.
        let mut run = match self.state {
            State::Payload { .. } => None,
            _ => Some(0),
        };

        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            match self.state {
                State::Ground => {
                    if b == ESC {
                        self.prefix.clear();
                        self.prefix.push(ESC);
                        self.state = State::Esc;
                        prefix_start = i;
                        carried = false;
                    } else {
                        i = next(bytes, i, |b| b == ESC);
                        continue;
                    }
                }
                State::Esc => match b {
                    b'_' => {
                        self.prefix.push(b);
                        self.state = State::Apc;
                    }
                    b'P' => {
                        self.prefix.push(b);
                        self.state = State::Dcs;
                    }
                    b'c' => {
                        self.pass(&mut out, &mut carried, carried_len);
                        let start = run.unwrap_or(i);
                        out.push(Chunk::Vt(Cow::Borrowed(&bytes[start..=i])));
                        out.push(Chunk::Reset);
                        run = Some(i + 1);
                    }
                    _ => {
                        // Handle `b` again in ground.
                        self.pass(&mut out, &mut carried, carried_len);
                        continue;
                    }
                },
                State::Apc => {
                    if b == b'G' {
                        self.start(
                            Kind::Kitty,
                            &mut out,
                            bytes,
                            &mut run,
                            carried,
                            prefix_start,
                        );
                    } else {
                        self.pass(&mut out, &mut carried, carried_len);
                        continue;
                    }
                }
                State::Dcs => {
                    if b == b'q' {
                        self.start(
                            Kind::Sixel,
                            &mut out,
                            bytes,
                            &mut run,
                            carried,
                            prefix_start,
                        );
                    } else if (0x30..=0x3f).contains(&b) && self.prefix.len() < 2 + MAX_DCS_PARAMS {
                        self.prefix.push(b);
                    } else {
                        self.pass(&mut out, &mut carried, carried_len);
                        continue;
                    }
                }
                State::Payload { kind, esc: true } => {
                    if b == b'\\' {
                        self.finish(kind, &mut out);
                        run = Some(i + 1);
                    } else {
                        // The `ESC` starts a new escape sequence: the payload is dropped,
                        // as `vte` would do.
                        self.abort();
                        self.prefix.clear();
                        self.prefix.push(ESC);
                        self.state = State::Esc;
                        if i == 0 {
                            carried = true;
                            carried_len = 1;
                            run = Some(0);
                        } else {
                            carried = false;
                            prefix_start = i - 1;
                            run = Some(i - 1);
                        }
                        continue;
                    }
                }
                State::Payload { kind, esc: false } => {
                    if b == ESC {
                        self.state = State::Payload { kind, esc: true };
                    } else if b == CAN || b == SUB {
                        // The string is cancelled; the byte itself goes to the parser.
                        self.abort();
                        self.state = State::Ground;
                        run = Some(i);
                    } else {
                        let end = next(bytes, i, |b| b == ESC || b == CAN || b == SUB);
                        self.push_payload(&bytes[i..end]);
                        i = end;
                        continue;
                    }
                }
            }
            i += 1;
        }

        let end = match self.state {
            State::Ground => Some(bytes.len()),
            State::Esc | State::Apc | State::Dcs if !carried => Some(prefix_start),
            _ => None,
        };
        if let (Some(start), Some(end)) = (run, end)
            && start < end
        {
            out.push(Chunk::Vt(Cow::Borrowed(&bytes[start..end])));
        }
        out
    }

    /// The escape sequence being recognised is not an image: back to ground, emitting
    /// its bytes carried from an earlier read.
    fn pass<'a>(&mut self, out: &mut Vec<Chunk<'a>>, carried: &mut bool, carried_len: usize) {
        if *carried {
            out.push(Chunk::Vt(Cow::Owned(self.prefix[..carried_len].to_vec())));
            *carried = false;
        }
        self.prefix.clear();
        self.state = State::Ground;
    }

    /// An image sequence starts: emits the VT bytes before its `ESC`.
    fn start<'a>(
        &mut self,
        kind: Kind,
        out: &mut Vec<Chunk<'a>>,
        bytes: &'a [u8],
        run: &mut Option<usize>,
        carried: bool,
        prefix_start: usize,
    ) {
        if let Some(start) = *run
            && !carried
            && start < prefix_start
        {
            out.push(Chunk::Vt(Cow::Borrowed(&bytes[start..prefix_start])));
        }
        *run = None;
        self.payload.clear();
        self.overflow = false;
        if kind == Kind::Sixel {
            let params = self.prefix[2..].to_vec();
            self.push_payload(&params);
            self.push_payload(b"q");
        }
        self.prefix.clear();
        self.state = State::Payload { kind, esc: false };
    }

    fn push_payload(&mut self, bytes: &[u8]) {
        if self.overflow {
            return;
        }
        if self.payload.len() + bytes.len() > self.limit {
            warn!("image sequence over {} bytes dropped", self.limit);
            self.overflow = true;
            self.payload = Vec::new();
            return;
        }
        self.payload.extend_from_slice(bytes);
    }

    /// The image sequence ends with `ESC \`.
    fn finish<'a>(&mut self, kind: Kind, out: &mut Vec<Chunk<'a>>) {
        let payload = std::mem::take(&mut self.payload);
        if !self.overflow {
            out.push(match kind {
                Kind::Kitty => Chunk::Kitty(payload),
                Kind::Sixel => Chunk::Sixel(payload),
            });
        }
        self.overflow = false;
        self.state = State::Ground;
    }

    /// The image sequence is cut short: drops its payload.
    fn abort(&mut self) {
        self.payload = Vec::new();
        self.overflow = false;
    }
}

/// Index of the first byte from `from` matching `stop`, or `bytes.len()`.
fn next(bytes: &[u8], from: usize, stop: impl Fn(u8) -> bool) -> usize {
    bytes[from..]
        .iter()
        .position(|&b| stop(b))
        .map_or(bytes.len(), |p| from + p)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Owned view of the chunks, adjacent VT bytes merged, so different read splits of
    /// the same stream compare equal.
    #[derive(Debug, PartialEq, Eq)]
    enum Out {
        Vt(Vec<u8>),
        Kitty(Vec<u8>),
        Sixel(Vec<u8>),
        Reset,
    }

    fn split(splitter: &mut Splitter, reads: &[&[u8]]) -> Vec<Out> {
        let mut out: Vec<Out> = Vec::new();
        for read in reads {
            for chunk in splitter.feed(read) {
                match chunk {
                    Chunk::Vt(bytes) => match out.last_mut() {
                        Some(Out::Vt(last)) => last.extend_from_slice(&bytes),
                        _ => out.push(Out::Vt(bytes.into_owned())),
                    },
                    Chunk::Kitty(payload) => out.push(Out::Kitty(payload)),
                    Chunk::Sixel(body) => out.push(Out::Sixel(body)),
                    Chunk::Reset => out.push(Out::Reset),
                }
            }
        }
        out
    }

    fn whole(bytes: &[u8]) -> Vec<Out> {
        split(&mut Splitter::new(), &[bytes])
    }

    /// Asserts that every two-read split and the byte-by-byte split of `bytes` give
    /// `expected`.
    fn assert_every_split(bytes: &[u8], expected: &[Out]) {
        assert_eq!(whole(bytes), expected, "whole");
        for at in 0..=bytes.len() {
            let (a, b) = bytes.split_at(at);
            assert_eq!(
                split(&mut Splitter::new(), &[a, b]),
                expected,
                "split at {at}"
            );
        }
        let bytes: Vec<&[u8]> = bytes.chunks(1).collect();
        assert_eq!(
            split(&mut Splitter::new(), &bytes),
            expected,
            "byte by byte"
        );
    }

    fn vt(bytes: &[u8]) -> Out {
        Out::Vt(bytes.to_vec())
    }

    #[test]
    fn plain_bytes_are_borrowed_in_one_chunk() {
        let bytes = b"hello\r\n\x1b[31mred\x1b]0;t\x07";
        let chunks = Splitter::new().feed(bytes);
        assert_eq!(chunks, [Chunk::Vt(Cow::Borrowed(&bytes[..]))]);
        assert!(matches!(&chunks[0], Chunk::Vt(Cow::Borrowed(_))));
    }

    #[test]
    fn extracts_a_kitty_command() {
        assert_every_split(
            b"ab\x1b_Ga=T,f=100;AAAA\x1b\\cd",
            &[vt(b"ab"), Out::Kitty(b"a=T,f=100;AAAA".to_vec()), vt(b"cd")],
        );
    }

    #[test]
    fn extracts_a_sixel_with_its_params() {
        assert_every_split(
            b"x\x1bP0;1;0q\"1;1;2;2#0~~\x1b\\y",
            &[
                vt(b"x"),
                Out::Sixel(b"0;1;0q\"1;1;2;2#0~~".to_vec()),
                vt(b"y"),
            ],
        );
        assert_every_split(b"\x1bPq#0~\x1b\\", &[Out::Sixel(b"q#0~".to_vec())]);
    }

    #[test]
    fn extracts_consecutive_sequences() {
        assert_every_split(
            b"\x1b_Gi=1;A\x1b\\\x1b_Gi=2;B\x1b\\\x1bPq-\x1b\\",
            &[
                Out::Kitty(b"i=1;A".to_vec()),
                Out::Kitty(b"i=2;B".to_vec()),
                Out::Sixel(b"q-".to_vec()),
            ],
        );
    }

    #[test]
    fn passes_other_apc_and_dcs_byte_for_byte() {
        let bytes: &[u8] = b"a\x1b_Xfoo\x1b\\b\x1bP$qm\x1b\\c\x1bP1$r0m\x1b\\d\x1bP+q544e\x1b\\\
            e\x1b_\x1b\\\x1bP\x1b[1mf\x1b\x1b[0m";
        assert_every_split(bytes, &[vt(bytes)]);
    }

    #[test]
    fn passes_dcs_with_too_many_params() {
        let mut bytes = b"\x1bP".to_vec();
        bytes.extend(std::iter::repeat_n(b'1', MAX_DCS_PARAMS + 1));
        bytes.extend_from_slice(b"q#0~\x1b\\");
        assert_every_split(&bytes, &[vt(&bytes)]);
    }

    #[test]
    fn drops_an_oversize_payload_and_recovers() {
        let mut splitter = Splitter::with_limit(4);
        let out = split(
            &mut splitter,
            &[b"a\x1b_Gabc", b"defgh\x1b", b"\\b\x1b_Gok\x1b\\c"],
        );
        assert_eq!(out, [vt(b"ab"), Out::Kitty(b"ok".to_vec()), vt(b"c")]);
    }

    #[test]
    fn keeps_a_payload_at_the_limit() {
        let mut splitter = Splitter::with_limit(4);
        let out = split(&mut splitter, &[b"\x1b_Gabcd\x1b\\"]);
        assert_eq!(out, [Out::Kitty(b"abcd".to_vec())]);
    }

    #[test]
    fn flags_a_full_reset() {
        assert_every_split(
            b"a\x1bcb\x1b\x1bc",
            &[vt(b"a\x1bc"), Out::Reset, vt(b"b\x1b\x1bc"), Out::Reset],
        );
    }

    #[test]
    fn an_escape_inside_a_payload_aborts_it() {
        assert_every_split(
            b"\x1b_Gabc\x1b[31mX\x1bPq#0\x1bc",
            &[vt(b"\x1b[31mX\x1bc"), Out::Reset],
        );
    }

    #[test]
    fn can_and_sub_abort_a_payload() {
        assert_every_split(b"\x1b_Gabc\x18d\x1bPq#\x1ae", &[vt(b"\x18d\x1ae")]);
    }

    #[test]
    fn a_prefix_at_the_end_of_a_read_is_held() {
        let mut splitter = Splitter::new();
        assert_eq!(
            splitter.feed(b"ab\x1b_"),
            [Chunk::Vt(Cow::Borrowed(&b"ab"[..]))]
        );
        assert_eq!(
            splitter.feed(b"x"),
            [
                Chunk::Vt(Cow::Owned(b"\x1b_".to_vec())),
                Chunk::Vt(Cow::Borrowed(&b"x"[..])),
            ]
        );
    }
}
