//! OSC 133 stream scanner: finds the shell integration marks in the PTY byte stream.
//!
//! `vte` drops OSC 133 without calling any handler, so the marks are found here, before
//! the parser. The scanner is incremental (a sequence split across reads is still found)
//! and never consumes bytes: every byte, marks included, comes back as
//! [`Chunk::Bytes`] so the `vte` parser still sees the whole stream.

/// OSC payload length after which the sequence is abandoned.
const MAX_OSC_LEN: usize = 4096;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
/// CAN and SUB abort a control sequence.
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;

/// A shell integration mark, or a hyperlink change worth knowing about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// `133;A`: the prompt starts.
    PromptStart,
    /// `133;P;k=r`: the right prompt starts.
    RightPrompt,
    /// `133;B`: the prompt ends, the user input starts.
    InputStart,
    /// `133;C`: the command runs, its output starts.
    OutputStart,
    /// `133;D[;code]`: the command ended, with its exit code when given.
    CommandEnd(Option<i32>),
    /// `8;…`: an OSC 8 hyperlink was opened or closed.
    Hyperlink,
}

/// A piece of the byte stream returned by [`Scanner::split`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chunk<'a> {
    /// Bytes to hand to the parser, in order.
    Bytes(&'a [u8]),
    /// A mark whose sequence ends with the previous `Bytes` chunk.
    Mark(Mark),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum State {
    #[default]
    Ground,
    /// `ESC` seen outside an OSC.
    Escape,
    /// Inside `ESC ]`, collecting the payload.
    Osc,
    /// `ESC` seen inside an OSC: `\` ends it.
    OscEscape,
    /// Overlong OSC: skipped up to its terminator.
    Abandoned,
    /// `ESC` seen inside an abandoned OSC.
    AbandonedEscape,
}

/// Incremental OSC 133 / OSC 8 finder. Keeps its state between reads.
#[derive(Debug, Default)]
pub struct Scanner {
    state: State,
    payload: Vec<u8>,
}

impl Scanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Splits `bytes` into byte chunks and the marks found at their end. Concatenating
    /// the `Bytes` chunks gives back `bytes` unchanged.
    pub fn split<'s, 'a>(&'s mut self, bytes: &'a [u8]) -> Split<'s, 'a> {
        Split {
            scanner: self,
            bytes,
            pos: 0,
            pending: None,
        }
    }

    /// Advances the state machine by one byte; returns a mark when a sequence ends.
    fn step(&mut self, byte: u8) -> Option<Mark> {
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
            State::Osc => {
                match byte {
                    BEL => return self.finish(),
                    ESC => self.state = State::OscEscape,
                    CAN | SUB => self.reset(),
                    _ if self.payload.len() >= MAX_OSC_LEN => {
                        self.payload.clear();
                        self.state = State::Abandoned;
                    }
                    _ => self.payload.push(byte),
                }
                None
            }
            State::OscEscape => {
                if byte == b'\\' {
                    return self.finish();
                }
                // Any other byte aborts the OSC and starts a new escape sequence.
                self.payload.clear();
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
                State::Osc
            }
            ESC => State::Escape,
            _ => State::Ground,
        };
    }

    fn reset(&mut self) {
        self.payload.clear();
        self.state = State::Ground;
    }

    /// Ends the current OSC and parses its payload.
    fn finish(&mut self) -> Option<Mark> {
        let mark = parse(&self.payload);
        self.reset();
        mark
    }
}

/// Iterator returned by [`Scanner::split`].
pub struct Split<'s, 'a> {
    scanner: &'s mut Scanner,
    bytes: &'a [u8],
    pos: usize,
    pending: Option<Mark>,
}

impl<'a> Iterator for Split<'_, 'a> {
    type Item = Chunk<'a>;

    fn next(&mut self) -> Option<Chunk<'a>> {
        if let Some(mark) = self.pending.take() {
            return Some(Chunk::Mark(mark));
        }
        if self.pos >= self.bytes.len() {
            return None;
        }
        let start = self.pos;
        for (offset, &byte) in self.bytes[start..].iter().enumerate() {
            if let Some(mark) = self.scanner.step(byte) {
                let end = start + offset + 1;
                self.pos = end;
                self.pending = Some(mark);
                return Some(Chunk::Bytes(&self.bytes[start..end]));
            }
        }
        self.pos = self.bytes.len();
        Some(Chunk::Bytes(&self.bytes[start..]))
    }
}

/// Parses an OSC payload (`133;…` or `8;…`); other OSC give no mark.
fn parse(payload: &[u8]) -> Option<Mark> {
    let mut params = payload.split(|&b| b == b';');
    match params.next()? {
        b"8" => Some(Mark::Hyperlink),
        b"133" => {
            let kind = params.next()?;
            match kind {
                b"A" => Some(Mark::PromptStart),
                b"P" => params
                    .any(|option| option == b"k=r")
                    .then_some(Mark::RightPrompt),
                b"B" => Some(Mark::InputStart),
                b"C" => Some(Mark::OutputStart),
                b"D" => {
                    let code = params
                        .next()
                        .filter(|param| !param.contains(&b'='))
                        .and_then(|param| std::str::from_utf8(param).ok())
                        .and_then(|param| param.parse().ok());
                    Some(Mark::CommandEnd(code))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds `reads` through one scanner; returns the concatenated bytes and the marks.
    fn scan(reads: &[&[u8]]) -> (Vec<u8>, Vec<Mark>) {
        let mut scanner = Scanner::new();
        let mut bytes = Vec::new();
        let mut marks = Vec::new();
        for read in reads {
            for chunk in scanner.split(read) {
                match chunk {
                    Chunk::Bytes(b) => bytes.extend_from_slice(b),
                    Chunk::Mark(m) => marks.push(m),
                }
            }
        }
        (bytes, marks)
    }

    fn marks(reads: &[&[u8]]) -> Vec<Mark> {
        let (bytes, marks) = scan(reads);
        assert_eq!(bytes, reads.concat(), "bytes must pass through unchanged");
        marks
    }

    #[test]
    fn bel_and_st_terminators() {
        assert_eq!(marks(&[b"\x1b]133;A\x07"]), [Mark::PromptStart]);
        assert_eq!(marks(&[b"\x1b]133;B\x1b\\"]), [Mark::InputStart]);
    }

    #[test]
    fn all_mark_kinds() {
        let stream = b"\x1b]133;A\x07$ \x1b]133;P;k=r\x07 right\x1b]133;B\x07ls\n\
            \x1b]133;C\x07out\n\x1b]133;D;0\x07\x1b]8;;http://x\x07link\x1b]8;;\x07";
        assert_eq!(
            marks(&[stream]),
            [
                Mark::PromptStart,
                Mark::RightPrompt,
                Mark::InputStart,
                Mark::OutputStart,
                Mark::CommandEnd(Some(0)),
                Mark::Hyperlink,
                Mark::Hyperlink,
            ]
        );
    }

    #[test]
    fn mark_follows_the_bytes_of_its_sequence() {
        let mut scanner = Scanner::new();
        let chunks: Vec<_> = scanner.split(b"ab\x1b]133;C\x07cd").collect();
        assert_eq!(
            chunks,
            [
                Chunk::Bytes(b"ab\x1b]133;C\x07"),
                Chunk::Mark(Mark::OutputStart),
                Chunk::Bytes(b"cd"),
            ]
        );
    }

    #[test]
    fn sequence_split_over_two_reads() {
        assert_eq!(
            marks(&[b"x\x1b]13", b"3;D;1\x07y"]),
            [Mark::CommandEnd(Some(1))]
        );
        // Split right between ESC and backslash.
        assert_eq!(marks(&[b"\x1b]133;A\x1b", b"\\"]), [Mark::PromptStart]);
        // Split right after the leading ESC.
        assert_eq!(marks(&[b"\x1b", b"]133;B\x07"]), [Mark::InputStart]);
    }

    #[test]
    fn sequence_split_over_three_reads() {
        assert_eq!(
            marks(&[b"\x1b", b"]133;D;", b"127\x1b\\"]),
            [Mark::CommandEnd(Some(127))]
        );
    }

    #[test]
    fn options_are_ignored() {
        assert_eq!(
            marks(&[b"\x1b]133;A;click_events=1\x07\x1b]133;A;aid=42\x07"]),
            [Mark::PromptStart, Mark::PromptStart]
        );
        assert_eq!(
            marks(&[b"\x1b]133;C;cmdline_url=ls%20-l\x07"]),
            [Mark::OutputStart]
        );
        assert_eq!(
            marks(&[b"\x1b]133;D;2;aid=42\x07"]),
            [Mark::CommandEnd(Some(2))]
        );
    }

    #[test]
    fn command_end_with_and_without_status() {
        assert_eq!(marks(&[b"\x1b]133;D\x07"]), [Mark::CommandEnd(None)]);
        assert_eq!(marks(&[b"\x1b]133;D;0\x07"]), [Mark::CommandEnd(Some(0))]);
        assert_eq!(marks(&[b"\x1b]133;D;-1\x07"]), [Mark::CommandEnd(Some(-1))]);
        assert_eq!(marks(&[b"\x1b]133;D;aid=1\x07"]), [Mark::CommandEnd(None)]);
        assert_eq!(marks(&[b"\x1b]133;D;oops\x07"]), [Mark::CommandEnd(None)]);
    }

    #[test]
    fn other_prompt_kinds_and_unknown_marks_are_skipped() {
        assert!(marks(&[b"\x1b]133;P;k=i\x07\x1b]133;Z\x07\x1b]133\x07"]).is_empty());
    }

    #[test]
    fn unrelated_osc_untouched() {
        let stream = b"\x1b]0;title\x07\x1b]52;c;aGk=\x1b\\\x1b]1337;x\x07\x1b]88;y\x07";
        assert!(marks(&[stream]).is_empty());
    }

    #[test]
    fn plain_escape_sequences_are_not_osc() {
        assert_eq!(
            marks(&[b"\x1b[31m133;A\x07\x1b\x1b]133;A\x07"]),
            [Mark::PromptStart]
        );
    }

    #[test]
    fn overlong_osc_is_abandoned() {
        let mut long = b"\x1b]133;A;".to_vec();
        long.extend(std::iter::repeat_n(b'x', MAX_OSC_LEN));
        long.push(BEL);
        assert!(marks(&[&long]).is_empty());
        // The scanner recovers for the next sequence, even split after the long one.
        long.extend_from_slice(b"\x1b]133;B\x07");
        assert_eq!(marks(&[&long[..10], &long[10..]]), [Mark::InputStart]);
    }

    #[test]
    fn abandoned_osc_ended_by_st_then_new_sequence() {
        let mut long = b"\x1b]0;".to_vec();
        long.extend(std::iter::repeat_n(b'x', MAX_OSC_LEN + 10));
        long.extend_from_slice(b"\x1b\\\x1b]133;C\x07");
        assert_eq!(marks(&[&long]), [Mark::OutputStart]);
    }

    #[test]
    fn cancelled_osc_gives_no_mark() {
        assert!(marks(&[b"\x1b]133;A\x18\x07"]).is_empty());
    }

    #[test]
    fn esc_inside_osc_restarts_a_sequence() {
        assert_eq!(marks(&[b"\x1b]0;t\x1b\x1b]133;A\x07"]), [Mark::PromptStart]);
    }

    #[test]
    fn empty_input_gives_no_chunk() {
        let mut scanner = Scanner::new();
        assert_eq!(scanner.split(b"").count(), 0);
    }
}
