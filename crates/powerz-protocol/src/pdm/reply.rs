//! Pure stream reader for the CDC byte stream: the PDM reply stream and
//! the ADCNEW push stream share one port, so one reader separates them.
//!
//! The CDC stream carries, in any order \[device\] (firmware 2.0.6):
//! - text reply lines ending in `\n` (no `\r`);
//! - binary capability blocks starting with a tag (see [`super::caps`]),
//!   usually followed by `\n`;
//! - binary protocol replies: PUT_DATA (`byte0 & 0x7F == 0x41`, framed like
//!   the CDC transport frames them), 0x44 / 0x4C replies and 4-byte control
//!   replies;
//! - ADCNEW frames (`02 att obj crc` + `obj × 4` body bytes, CRC-checked).
//!
//! [`ReplyReader`] hands out lines and blocks, keeps ADCNEW frames for
//! [`ReplyReader::take_frames`] and skips binary replies;
//! [`crate::adcnew::CdcDemux`] uses the same reader without block detection
//! and hands out frames, replies and lines.
//!
//! A `\n` right after a block is swallowed, even when it arrives in a later
//! chunk. After a malformed block the reader skips to the next `\n`. A bad
//! ADCNEW frame is dropped whole when a valid item follows it; otherwise the
//! reader drops bytes up to the next valid frame or line start, so a corrupt
//! length never swallows what comes after it.

use std::collections::VecDeque;

use super::Error;
use super::caps::{self, CapBlock};
use crate::adcnew::{Frame, decode_frame};
use crate::frame::msg;
use crate::transport::cdc::frame_complete;

/// One item read from the reply stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyItem {
    /// One text line, without its `\n`.
    Line(String),
    /// A binary capability block.
    Block(CapBlock),
}

/// One item of the shared CDC stream reader.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Token {
    /// A text line, without its `\n`.
    Line(String),
    /// A capability block (only with block detection on).
    Block(CapBlock),
    /// A decoded ADCNEW frame.
    Frame(Frame),
    /// A whole binary protocol reply.
    Reply(Vec<u8>),
}

/// Incremental reader: push bytes as they arrive, pull items.
#[derive(Debug, Clone, Default)]
pub struct ReplyReader {
    buf: Vec<u8>,
    after_block: bool,
    /// After a malformed block: drop bytes up to and including the next `\n`.
    skip_line: bool,
    /// Capability blocks are not recognised (the plain CDC demux).
    no_blocks: bool,
    /// After a bad ADCNEW frame: drop bytes up to the next valid frame or
    /// the start of a line.
    resync: bool,
    /// Bad ADCNEW frames dropped so far.
    dropped: usize,
    /// ADCNEW frames read between lines, oldest first.
    frames: VecDeque<Frame>,
}

/// ADCNEW record frame type (first byte). \[device\]
const ADCNEW: u8 = 0x02;
/// ADCNEW frame header length.
const ADCNEW_HEADER: usize = 4;

/// Binary reply types (byte 0, extend bit masked off) the reader frames as
/// replies; other non-text bytes are dropped one by one. \[static\]
const REPLY_TYPES: [u8; 11] = [
    msg::ACCEPT,
    msg::REJECT,
    msg::FINISHED,
    msg::GET_DATA,
    msg::START_QUEUE,
    msg::STOP_QUEUE,
    msg::START_ANALYZER,
    msg::STOP_ANALYZER,
    msg::PUT_DATA,
    msg::MEM_READ_CONFIRM,
    msg::AUTH_REPLY,
];

/// Reply types whose first byte is a printable letter (`A`, `D`, `L`): a
/// text line may start with them too. \[inferred\]
const LETTER_TYPES: [u8; 3] = [msg::PUT_DATA, msg::MEM_READ_CONFIRM, msg::AUTH_REPLY];

/// Shortest text line that may start with 0x41 (`A`); shorter ones are taken
/// as a PUT_DATA frame. \[inferred\]
const PUT_DATA_TEXT_MIN: usize = 4;
/// A tag (`ufcs:` etc.) starts a text line only with text after it: the tag
/// itself plus at least one character. \[inferred\]
const TAG_TEXT_MIN: usize = 6;

/// Most ADCNEW frames kept for [`ReplyReader::take_frames`]; older ones are
/// dropped so a session nobody drains stays bounded. \[inferred\]
const MAX_KEPT_FRAMES: usize = 4096;

/// See [`ReplyReader::shape`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Text,
    Binary,
    Undecided,
}

/// An ADCNEW frame at some offset: decoded, bad (with its declared length),
/// or not complete yet (`None`).
type FrameAt = Option<Result<(Frame, usize), usize>>;

/// Most bytes kept while waiting for a line end, block end or frame end.
/// Longer input is dropped up to the next `\n` with [`Error::LineTooLong`],
/// so a device that never ends a line cannot grow the buffer forever.
pub const MAX_LINE: usize = 4096;

/// Bytes a text line may hold: printable ASCII, tab and `\r`.
fn is_text(b: u8) -> bool {
    matches!(b, 0x20..=0x7e | b'\t' | b'\r')
}

impl ReplyReader {
    /// An empty reader.
    pub fn new() -> ReplyReader {
        ReplyReader::default()
    }

    /// An empty reader that does not look for capability blocks.
    pub(crate) fn without_blocks() -> ReplyReader {
        ReplyReader {
            no_blocks: true,
            ..ReplyReader::default()
        }
    }

    /// Append bytes read from the port.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// The next complete item, `None` while more bytes are needed.
    /// Binary replies are skipped; ADCNEW frames are kept for
    /// [`take_frames`](Self::take_frames). Never panics.
    pub fn next_item(&mut self) -> Option<Result<ReplyItem, Error>> {
        loop {
            match self.next_token()? {
                Ok(Token::Line(line)) => return Some(Ok(ReplyItem::Line(line))),
                Ok(Token::Block(block)) => return Some(Ok(ReplyItem::Block(block))),
                Ok(Token::Frame(frame)) => {
                    if self.frames.len() >= MAX_KEPT_FRAMES {
                        self.frames.pop_front();
                    }
                    self.frames.push_back(frame);
                }
                Ok(Token::Reply(_)) => {}
                Err(err) => return Some(Err(err)),
            }
        }
    }

    /// The ADCNEW frames read so far, oldest first; each is handed out once.
    pub fn take_frames(&mut self) -> Vec<Frame> {
        self.frames.drain(..).collect()
    }

    /// Bad ADCNEW frames dropped so far.
    pub(crate) fn dropped_frames(&self) -> usize {
        self.dropped
    }

    /// The next complete token, `None` while more bytes are needed.
    pub(crate) fn next_token(&mut self) -> Option<Result<Token, Error>> {
        loop {
            if self.skip_line {
                match self.newline() {
                    Some(pos) => {
                        self.consume(pos + 1);
                        self.skip_line = false;
                    }
                    None => {
                        self.buf.clear();
                        return None;
                    }
                }
            }
            if self.resync && !self.resync_step() {
                return self.need_more();
            }
            let &first = self.buf.first()?;
            if self.after_block {
                self.after_block = false;
                if first == b'\n' {
                    self.consume(1);
                    continue;
                }
            }
            if first == ADCNEW {
                match self.frame_at(0) {
                    None => return self.need_more(),
                    Some(Ok((frame, len))) => {
                        self.consume(len);
                        return Some(Ok(Token::Frame(frame)));
                    }
                    Some(Err(len)) => {
                        // Drop the bad frame whole when a valid item (or
                        // nothing yet) follows its declared end; otherwise
                        // its length is suspect: drop one byte and resync.
                        match self.item_at(len) {
                            None if len < self.buf.len() => return self.need_more(),
                            None | Some(true) => self.consume(len),
                            Some(false) => {
                                self.consume(1);
                                self.resync = true;
                            }
                        }
                        self.dropped += 1;
                        continue;
                    }
                }
            }
            // A printable line that merely starts with 0x41 (`Apple 2.4A`
            // in `entry list` [device]) is text, not a frame.
            let letter = first < 0x80 && LETTER_TYPES.contains(&first);
            let reply = if letter {
                match self.shape(PUT_DATA_TEXT_MIN) {
                    Shape::Undecided => return self.need_more(),
                    Shape::Binary => true,
                    Shape::Text => false,
                }
            } else {
                !is_text(first) && first != b'\n'
            };
            if reply {
                if !REPLY_TYPES.contains(&(first & 0x7F)) {
                    // Neither text nor a known frame: noise, drop it.
                    self.consume(1);
                    continue;
                }
                let Some(len) = frame_complete(&self.buf) else {
                    return self.need_more();
                };
                let len = len.max(1);
                let bytes = self.buf.get(..len).unwrap_or_default().to_vec();
                self.consume(len);
                return Some(Ok(Token::Reply(bytes)));
            }
            // `pdo:` is always a block. The other tags also start text lines
            // of `entry list` (`ufcs:63.0W,` [device]): followed by printable
            // text up to the line end they are text, while their binary
            // blocks carry a non-printable byte (the count) right after the tag.
            let tag = if self.no_blocks {
                None
            } else {
                caps::block_tag(&self.buf)
            };
            let block = match tag {
                None => false,
                Some(super::BlockTag::Pdo) => true,
                Some(_) => match self.shape(TAG_TEXT_MIN) {
                    Shape::Text => false,
                    Shape::Binary => true,
                    Shape::Undecided => return self.need_more(),
                },
            };
            if block {
                return match caps::parse(&self.buf) {
                    Ok(None) => self.need_more(),
                    Ok(Some((block, len))) => {
                        self.consume(len.max(1));
                        self.after_block = true;
                        Some(Ok(Token::Block(block)))
                    }
                    Err(err) => {
                        self.skip_line = true;
                        Some(Err(err))
                    }
                };
            }
            match self.line_at(0) {
                Shape::Undecided => return self.need_more(),
                // A non-text byte before the line end: not a line, drop one
                // byte and look again.
                Shape::Binary => {
                    self.consume(1);
                    continue;
                }
                Shape::Text => {}
            }
            let Some(pos) = self.newline() else {
                return self.need_more();
            };
            let line =
                String::from_utf8_lossy(self.buf.get(..pos).unwrap_or_default()).into_owned();
            self.consume(pos + 1);
            return Some(Ok(Token::Line(line)));
        }
    }

    /// The ADCNEW frame starting at `at`.
    fn frame_at(&self, at: usize) -> FrameAt {
        let &words = self.buf.get(at + 2)?;
        let len = ADCNEW_HEADER + usize::from(words) * 4;
        let bytes = self.buf.get(at..at + len)?;
        Some(match decode_frame(bytes) {
            Ok(frame) => Ok((frame, len)),
            Err(_) => Err(len),
        })
    }

    /// Whether a valid item (good frame, text line, binary reply) starts at
    /// `at`; `None` while more bytes are needed.
    fn item_at(&self, at: usize) -> Option<bool> {
        let &b = self.buf.get(at)?;
        if b == ADCNEW {
            return self.frame_at(at).map(|f| f.is_ok());
        }
        match self.line_at(at) {
            Shape::Text => return Some(true),
            Shape::Undecided if is_text(b) => return None,
            _ => {}
        }
        if !REPLY_TYPES.contains(&(b & 0x7F)) {
            return Some(false);
        }
        frame_complete(self.buf.get(at..).unwrap_or_default()).map(|_| true)
    }

    /// One resync step after a bad frame: drop bytes up to the next valid
    /// frame or line start. `false` while more bytes are needed.
    fn resync_step(&mut self) -> bool {
        for p in 0..self.buf.len() {
            if p > 0 && self.buf.get(p - 1) == Some(&b'\n') {
                self.consume(p);
                self.resync = false;
                return true;
            }
            if self.buf.get(p) == Some(&ADCNEW) {
                match self.frame_at(p) {
                    None => {
                        self.consume(p);
                        return false;
                    }
                    Some(Ok(_)) => {
                        self.consume(p);
                        self.resync = false;
                        return true;
                    }
                    Some(Err(_)) => {}
                }
            }
        }
        if self.buf.last() == Some(&b'\n') {
            self.resync = false;
        }
        self.buf.clear();
        false
    }

    /// More bytes are needed: `None`, unless the buffer is already past
    /// [`MAX_LINE`], in which case it is dropped up to the next `\n`.
    fn need_more(&mut self) -> Option<Result<Token, Error>> {
        if self.buf.len() <= MAX_LINE {
            return None;
        }
        let len = self.buf.len();
        self.buf.clear();
        self.skip_line = true;
        self.resync = false;
        Some(Err(Error::LineTooLong { len }))
    }

    /// Whether a text line starts at `at`: text bytes up to a `\n` (text),
    /// a non-text byte first (binary), or no line end yet (undecided).
    /// Scans at most [`MAX_LINE`] bytes.
    fn line_at(&self, at: usize) -> Shape {
        let rest = self.buf.get(at..).unwrap_or_default();
        for &b in rest.iter().take(MAX_LINE + 1) {
            match b {
                b'\n' => return Shape::Text,
                b if is_text(b) => {}
                _ => return Shape::Binary,
            }
        }
        Shape::Undecided
    }

    /// What the buffer starts with: a text line (printable ASCII up to a
    /// `\n` at index `min` or later), binary (a non-printable byte first,
    /// which frames and capability blocks carry early; a `\n` too early, like
    /// a UFCS count of 10 or a PUT_DATA id of 0x0a, counts as binary), or
    /// undecided (no line end yet). Scans at most [`MAX_LINE`] bytes.
    fn shape(&self, min: usize) -> Shape {
        for (i, &b) in self.buf.iter().enumerate().take(MAX_LINE + 1) {
            match b {
                b'\n' if i >= min => return Shape::Text,
                0x20..=0x7e => {}
                _ => return Shape::Binary,
            }
        }
        Shape::Undecided
    }

    /// Bytes buffered but not yet returned.
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    /// Position of the first `\n` in the buffer.
    fn newline(&self) -> Option<usize> {
        self.buf.iter().position(|&b| b == b'\n')
    }

    /// Drop the first `n` bytes (all of them when fewer are buffered).
    fn consume(&mut self, n: usize) {
        self.buf.drain(..n.min(self.buf.len()));
    }
}

#[cfg(test)]
mod tests {
    use super::super::BlockTag;
    use super::super::caps::fixtures::*;
    use super::*;
    use crate::decode::test_util::hex_vector;

    fn drain(r: &mut ReplyReader) -> Vec<Result<ReplyItem, Error>> {
        let mut out = Vec::new();
        while let Some(item) = r.next_item() {
            out.push(item);
        }
        out
    }

    fn line(s: &str) -> Result<ReplyItem, Error> {
        Ok(ReplyItem::Line(s.to_string()))
    }

    fn pdo_item(bytes: &[u8]) -> Result<ReplyItem, Error> {
        let (block, _) = crate::pdm::caps::parse(bytes).unwrap().unwrap();
        Ok(ReplyItem::Block(block))
    }

    /// The device's reply to `entry pd` (session 2): text, two unprompted
    /// pdo blocks, ready lines.
    fn entry_pd_reply() -> Vec<u8> {
        let mut b = b"ok\ncc1 attach\n".to_vec();
        b.extend(spr_block());
        b.extend(b"ready:5191mV,0mA\n");
        b.extend(epr_block());
        b.extend(b"ready:5162mV,0mA\n");
        b
    }

    // ---- AC-PR41FT.2: newline-terminated lines ----

    #[test]
    fn splits_lines_on_newline() {
        let mut r = ReplyReader::new();
        r.push(b"pdm mode entry\nver1.0\n");
        assert_eq!(drain(&mut r), vec![line("pdm mode entry"), line("ver1.0")]);
        assert_eq!(r.buffered(), 0);
    }

    #[test]
    fn holds_a_partial_line_until_its_newline() {
        let mut r = ReplyReader::new();
        r.push(b"req:9000m");
        assert_eq!(r.next_item(), None);
        r.push(b"V,3000mA\nrea");
        assert_eq!(drain(&mut r), vec![line("req:9000mV,3000mA")]);
        assert_eq!(r.buffered(), 3);
        r.push(b"dy:8554mV,0mA\n");
        assert_eq!(drain(&mut r), vec![line("ready:8554mV,0mA")]);
    }

    // ---- AC-PR41FT.3: blocks inside the reply stream, unprompted ----

    #[test]
    fn entry_pd_reply_yields_lines_and_both_pdo_blocks() {
        let mut r = ReplyReader::new();
        r.push(&entry_pd_reply());
        assert_eq!(
            drain(&mut r),
            vec![
                line("ok"),
                line("cc1 attach"),
                pdo_item(&spr_block()),
                line("ready:5191mV,0mA"),
                pdo_item(&epr_block()),
                line("ready:5162mV,0mA"),
            ]
        );
    }

    #[test]
    fn entry_pd_reply_survives_one_byte_chunks() {
        let mut r = ReplyReader::new();
        let mut items = Vec::new();
        for b in entry_pd_reply() {
            r.push(&[b]);
            items.extend(drain(&mut r));
        }
        assert_eq!(items.len(), 6, "{items:?}");
        assert_eq!(items[2], pdo_item(&spr_block()));
        assert_eq!(items[4], pdo_item(&epr_block()));
        assert_eq!(items[5], line("ready:5162mV,0mA"));
    }

    #[test]
    fn newline_after_a_block_in_a_later_chunk_is_swallowed() {
        let mut r = ReplyReader::new();
        let spr = spr_block();
        r.push(&spr[..spr.len() - 1]);
        let first = drain(&mut r);
        assert_eq!(first, vec![pdo_item(&spr)]);
        r.push(b"\nready:5191mV,0mA\n");
        assert_eq!(drain(&mut r), vec![line("ready:5191mV,0mA")]);
    }

    #[test]
    fn other_blocks_are_parsed_in_the_stream() {
        let mut r = ReplyReader::new();
        let mut bytes = b"ok\n".to_vec();
        bytes.extend(scp_block());
        bytes.push(b'\n');
        bytes.extend(ufcs_block());
        bytes.push(b'\n');
        bytes.extend(tfcp_block());
        bytes.push(b'\n');
        bytes.extend(vfcp_block());
        bytes.extend(b"\nready\n");
        r.push(&bytes);
        let tags: Vec<String> = drain(&mut r)
            .into_iter()
            .map(|i| match i {
                Ok(ReplyItem::Line(l)) => l,
                Ok(ReplyItem::Block(b)) => format!("{:?}", b.tag()),
                Err(e) => format!("error {e}"),
            })
            .collect();
        assert_eq!(tags, vec!["ok", "Scp", "Ufcs", "Tfcp", "Vfcp", "ready"]);
    }

    #[test]
    fn a_line_without_newline_is_capped_then_the_reader_resyncs() {
        // A device that never ends its line must not grow the buffer forever.
        let mut r = ReplyReader::new();
        r.push(&vec![b'x'; MAX_LINE + 1]);
        let first = drain(&mut r);
        assert!(
            matches!(first.as_slice(), [Err(Error::LineTooLong { .. })]),
            "{first:?}"
        );
        assert!(r.buffered() <= MAX_LINE, "buffered {}", r.buffered());
        r.push(b"more junk\nok\n");
        assert_eq!(drain(&mut r), vec![line("ok")]);
    }

    #[test]
    fn printable_list_lines_that_look_like_tags_or_frames_stay_text() {
        // `Apple 2.4A` starts with 0x41 (PUT_DATA) and `ufcs:63.0W,` with a
        // block tag; both are text lines of `entry list` [device].
        let mut r = ReplyReader::new();
        r.push(b"Apple 2.4A\nufcs:63.0W,\n Num1: 3.40 - 5.50V 3.00A\nscp:null\n");
        assert_eq!(
            drain(&mut r),
            vec![
                line("Apple 2.4A"),
                line("ufcs:63.0W,"),
                line(" Num1: 3.40 - 5.50V 3.00A"),
                line("scp:null"),
            ]
        );
    }

    #[test]
    fn a_printable_prefix_waits_for_its_line_end() {
        let mut r = ReplyReader::new();
        r.push(b"Apple 2.");
        assert_eq!(drain(&mut r), vec![]);
        r.push(b"4A\n");
        assert_eq!(drain(&mut r), vec![line("Apple 2.4A")]);
    }

    #[test]
    fn an_endless_printable_line_after_a_tag_or_0x41_is_capped() {
        for start in [&b"Apple"[..], &b"ufcs:"[..]] {
            let mut r = ReplyReader::new();
            r.push(start);
            r.push(&vec![b'x'; MAX_LINE + 1]);
            let items = drain(&mut r);
            assert!(
                matches!(items.as_slice(), [Err(Error::LineTooLong { .. })]),
                "{items:?}"
            );
        }
    }

    #[test]
    fn a_line_end_right_after_a_tag_or_0x41_is_not_text() {
        // A UFCS block with 10 entries has count 0x0a right after the tag; a
        // PUT_DATA frame can have id 0x0a. Neither is the text line it looks like.
        let mut r = ReplyReader::new();
        r.push(b"ufcs:\n");
        assert!(!matches!(drain(&mut r).first(), Some(Ok(ReplyItem::Line(l))) if l == "ufcs:"));
        let mut r = ReplyReader::new();
        r.push(b"A\n");
        assert!(!matches!(drain(&mut r).first(), Some(Ok(ReplyItem::Line(l))) if l == "A"));
    }

    #[test]
    fn entry_list_null_results_stay_text() {
        let mut r = ReplyReader::new();
        r.push(b"starting\nbc1.2:true 5203mV,0mA\nufcs:null\nvfcp:null\n");
        assert_eq!(
            drain(&mut r),
            vec![
                line("starting"),
                line("bc1.2:true 5203mV,0mA"),
                line("ufcs:null"),
                line("vfcp:null"),
            ]
        );
    }

    #[test]
    fn a_bare_vfcp_line_then_a_list_line_are_two_lines() {
        let mut r = ReplyReader::new();
        r.push(b"vfcp:\nready:5191mV,0mA\n");
        assert_eq!(drain(&mut r), vec![line("vfcp:"), line("ready:5191mV,0mA")]);
        // Split before the next line ends: the reader waits, then reads both.
        let mut r = ReplyReader::new();
        r.push(b"vfcp:\nready:51");
        assert_eq!(drain(&mut r), vec![]);
        r.push(b"91mV,0mA\n");
        assert_eq!(drain(&mut r), vec![line("vfcp:"), line("ready:5191mV,0mA")]);
        // The cut-off list's last line, nothing after it.
        let mut r = ReplyReader::new();
        r.push(b"vfcp:\n");
        assert_eq!(drain(&mut r), vec![line("vfcp:")]);
    }

    #[test]
    fn scp_and_vfcp_blocks_starting_with_a_line_end_byte_stay_blocks() {
        let mut scp = b"scp:".to_vec();
        for v in [266u16, 3400, 20000, 100, 4000, 10, 50, 11000, 5000] {
            scp.extend(v.to_le_bytes());
        }
        let mut vfcp = vfcp_block();
        vfcp[5] = 0x0A;
        let mut bytes = scp.clone();
        bytes.push(b'\n');
        bytes.extend(&vfcp);
        bytes.extend(b"\nready\n");
        let mut r = ReplyReader::new();
        r.push(&bytes);
        let items = drain(&mut r);
        assert!(
            matches!(items.first(), Some(Ok(ReplyItem::Block(CapBlock::Scp(s)))) if s.max_power_mw == 26_600),
            "{items:?}"
        );
        assert!(
            matches!(items.get(1), Some(Ok(ReplyItem::Block(CapBlock::Vfcp(_))))),
            "{items:?}"
        );
        assert_eq!(items.get(2), Some(&line("ready")));
        assert_eq!(items.len(), 3, "{items:?}");
    }

    // ---- AC-PR41FT.3: malformed blocks become typed errors, reader resyncs ----

    #[test]
    fn malformed_block_is_an_error_then_the_next_line_reads() {
        let mut r = ReplyReader::new();
        let mut bytes = b"pdo:4".to_vec();
        bytes.push(0x20); // 32 slots: more than any source caps hold
        bytes.extend(b"\nready:5191mV,0mA\n");
        r.push(&bytes);
        let items = drain(&mut r);
        assert!(
            matches!(
                items.first(),
                Some(Err(Error::Malformed {
                    tag: BlockTag::Pdo,
                    ..
                }))
            ),
            "{items:?}"
        );
        assert_eq!(items.last(), Some(&line("ready:5191mV,0mA")));
    }

    // ---- AC-PR41FT.2: interleaved binary frames are skipped ----

    #[test]
    fn skips_adcnew_frames_between_lines() {
        let mut r = ReplyReader::new();
        // ADCNEW record frame: type 0x02, att 0x12, obj 5 words, crc, 20 bytes.
        let mut adcnew = vec![0x02, 0x12, 0x05, 0x9C];
        adcnew.extend([0x61u8; 20]); // body bytes that look like text
        let mut bytes = b"ok\n".to_vec();
        bytes.extend(&adcnew);
        bytes.extend(b"cc1 attach\n");
        bytes.extend(&adcnew);
        r.push(&bytes);
        assert_eq!(drain(&mut r), vec![line("ok"), line("cc1 attach")]);
        assert_eq!(r.buffered(), 0);
    }

    #[test]
    fn skips_put_data_frames_split_over_chunks() {
        let adc = hex_vector(include_str!("../../tests/vectors/adc_52.hex"));
        assert_eq!(adc.len(), 52);
        let mut r = ReplyReader::new();
        r.push(b"req:9000mV,3000mA\n");
        r.push(&adc[..10]);
        let mut items = drain(&mut r);
        r.push(&adc[10..]);
        r.push(b"ready:8554mV,0mA\n");
        items.extend(drain(&mut r));
        assert_eq!(
            items,
            vec![line("req:9000mV,3000mA"), line("ready:8554mV,0mA")]
        );
    }
}
