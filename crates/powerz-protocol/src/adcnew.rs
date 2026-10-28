//! ADCNEW push stream on the CDC port: request encoding, the frame decoder
//! (4-byte header, body length in words, CRC-8 over the body, 20-byte
//! records) and a CDC stream reader that reassembles frames and resyncs
//! after a bad one. See "CDC and the ADCNEW stream" in `docs/protocol.md`.

use crate::decode::{QueueRecord, Rate};
use crate::pdm::reply::{ReplyReader, Token};

/// How many raw (non-record) ADCNEW frames a session keeps until they are
/// taken; older ones are dropped first.
pub const MAX_RAW_FRAMES: usize = 1024;

/// Frame type of every ADCNEW stream frame and of the start request. \[device\]
const TYPE_START: u8 = 0x02;
/// Keep-alive request type. \[doc\] \[device\]
const TYPE_KEEP_ALIVE: u8 = 0x01;
/// Stop request type. \[doc\] \[device\]
const TYPE_STOP: u8 = 0x03;
/// How often a running stream is refreshed with the keep-alive: the meter
/// stops pushing an hour after the last `02` or `01`. \[doc\]
pub const KEEP_ALIVE_PERIOD: std::time::Duration = std::time::Duration::from_secs(30 * 60);
/// Length of the frame header.
const HEADER_LEN: usize = 4;
/// Length of one record in a record frame (same as the ADC queue). \[device\]
const RECORD_LEN: usize = 20;
/// Record frames carry att `0x10 | rate code`. \[device\] (firmware 2.0.6)
const RECORD_ATT_BASE: u8 = 0x10;

/// CRC-8 used by ADCNEW frames: poly 0x29, init 0, no reflection, xorout 0,
/// computed over the body only. \[device\] (firmware 2.0.6)
pub fn crc8(body: &[u8]) -> u8 {
    body.iter().fold(0u8, |mut c, &b| {
        c ^= b;
        for _ in 0..8 {
            c = if c & 0x80 != 0 {
                (c << 1) ^ 0x29
            } else {
                c << 1
            };
        }
        c
    })
}

/// Frames lost on the host side of an ADCNEW stream, counted by the CDC
/// transport since it was opened.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AdcnewCounters {
    /// Frames dropped for a bad CRC or length.
    pub crc_errors: u64,
    /// Good frames dropped because nobody polled them before the hold cap.
    pub dropped_frames: u64,
    /// Start requests the meter echoed back instead of starting the stream.
    pub start_echoes: u64,
}

/// Host silence before a start is sent again after the meter echoed one.
/// Right after a bulk session the meter echoes a start and streams nothing,
/// and any request within about 6–10 s keeps it so; a start after a quiet
/// spell streams. \[device\] (firmware 2.0.6, 2026-10-28)
pub const START_ECHO_QUIET: std::time::Duration = std::time::Duration::from_secs(12);

/// Errors from decoding an ADCNEW frame.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdcnewError {
    /// Fewer than the 4 header bytes.
    #[error("ADCNEW frame: {got} bytes is shorter than the 4-byte header")]
    Short {
        /// Bytes supplied.
        got: usize,
    },
    /// The header's type byte is not 0x02.
    #[error("ADCNEW frame: type {got:#04x} is not a stream frame")]
    NotAFrame {
        /// Type byte found.
        got: u8,
    },
    /// The frame length does not match `4 + obj × 4`, or the body is empty.
    #[error("ADCNEW frame: expected {expected} bytes, got {got}")]
    Length {
        /// Length the header declares.
        expected: usize,
        /// Length supplied.
        got: usize,
    },
    /// The body CRC does not match the header.
    #[error("ADCNEW frame: CRC {got:#04x}, header says {expected:#04x}")]
    Crc {
        /// CRC in the header.
        expected: u8,
        /// CRC computed over the body.
        got: u8,
    },
    /// A record frame whose body is not whole 20-byte records.
    #[error("ADCNEW record frame: body of {got} bytes is not whole 20-byte records")]
    Records {
        /// Body length.
        got: usize,
    },
    /// The text side of the stream failed (a line that never ended).
    #[error("CDC stream: {0}")]
    Text(crate::pdm::Error),
}

/// One decoded ADCNEW frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    /// A record frame (att `0x10 | rate`): 20-byte records at `rate`.
    Records {
        /// Rate from the att low bits.
        rate: Rate,
        /// The records, in stream order.
        records: Vec<QueueRecord>,
    },
    /// Any other frame (the att-0x03 first frame, PD / UFCS content), body kept as is.
    Raw {
        /// The header att byte.
        att: u8,
        /// The body after the header.
        body: Vec<u8>,
    },
}

/// Decode one whole frame (header plus exactly `obj × 4` body bytes).
pub fn decode_frame(bytes: &[u8]) -> Result<Frame, AdcnewError> {
    if bytes.len() < HEADER_LEN {
        return Err(AdcnewError::Short { got: bytes.len() });
    }
    if bytes[0] != TYPE_START {
        return Err(AdcnewError::NotAFrame { got: bytes[0] });
    }
    let att = bytes[1];
    let expected = HEADER_LEN + 4 * usize::from(bytes[2]);
    // A header-only frame never occurs; refusing it keeps resync from
    // locking onto `02 xx 00 00` (CRC of nothing is 0). [inferred]
    if bytes.len() != expected || expected == HEADER_LEN {
        return Err(AdcnewError::Length {
            expected,
            got: bytes.len(),
        });
    }
    let body = &bytes[HEADER_LEN..];
    let got = crc8(body);
    if got != bytes[3] {
        return Err(AdcnewError::Crc {
            expected: bytes[3],
            got,
        });
    }
    let rate = (att & 0xF0 == RECORD_ATT_BASE)
        .then(|| Rate::from_code(att & 0x0F))
        .flatten();
    let Some(rate) = rate else {
        return Ok(Frame::Raw {
            att,
            body: body.to_vec(),
        });
    };
    let (chunks, rest) = body.as_chunks::<RECORD_LEN>();
    if !rest.is_empty() {
        return Err(AdcnewError::Records { got: body.len() });
    }
    let records = chunks
        .iter()
        .map(|c| QueueRecord::decode(c, rate))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| AdcnewError::Records { got: body.len() })?;
    Ok(Frame::Records { rate, records })
}

/// What the stream carries besides ADC records (request att high nibble). \[doc\]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Content {
    /// ADC records only.
    Adc,
    /// ADC plus PD content.
    AdcPd,
    /// ADC plus UFCS content.
    AdcUfcs,
    /// ADC plus PD and UFCS content.
    AdcPdUfcs,
}

impl Content {
    /// The att high nibble (0..3).
    fn code(self) -> u8 {
        match self {
            Content::Adc => 0,
            Content::AdcPd => 1,
            Content::AdcUfcs => 2,
            Content::AdcPdUfcs => 3,
        }
    }
}

/// An ADCNEW request sent on the CDC port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Bare `02`: stream at the meter's own rate. \[device\]
    StartDefault,
    /// `02 <att>`: stream `content` at `rate`. \[doc\] \[device\]
    Start {
        /// What to stream.
        content: Content,
        /// Record rate.
        rate: Rate,
    },
    /// `03`: stop the stream. \[doc\] \[device\]
    Stop,
    /// `01`: keep-alive. \[doc\] \[device\]
    KeepAlive,
}

impl Request {
    /// The bytes written to the port.
    pub fn encode(self) -> Vec<u8> {
        match self {
            Request::StartDefault => vec![TYPE_START],
            Request::Start { content, rate } => {
                vec![TYPE_START, (content.code() << 4) | rate.code()]
            }
            Request::Stop => vec![TYPE_STOP],
            Request::KeepAlive => vec![TYPE_KEEP_ALIVE],
        }
    }
}

/// One item read off the CDC byte stream.
#[derive(Debug, Clone, PartialEq)]
pub enum CdcItem {
    /// A decoded ADCNEW frame.
    Frame(Frame),
    /// A PDM text line, without its line ending.
    Line(String),
    /// A binary protocol reply, whole.
    Reply(Vec<u8>),
}

/// Reader for the CDC byte stream: push bytes as they arrive, take items out.
///
/// ADCNEW frames, binary replies and PDM text lines are told apart on one
/// byte stream (the reader is shared with [`crate::pdm::ReplyReader`]). A
/// frame that fails its length or CRC check is dropped; when what follows
/// its declared end is not a valid item, the reader resyncs one byte later,
/// so a corrupt length never swallows the frames, lines or replies after it.
#[derive(Debug)]
pub struct CdcDemux {
    reader: ReplyReader,
}

impl Default for CdcDemux {
    fn default() -> CdcDemux {
        CdcDemux::new()
    }
}

impl CdcDemux {
    /// An empty reader.
    pub fn new() -> CdcDemux {
        CdcDemux {
            reader: ReplyReader::without_blocks(),
        }
    }

    /// Append bytes read from the port.
    pub fn push(&mut self, bytes: &[u8]) {
        self.reader.push(bytes);
    }

    /// Frames dropped for a bad CRC or length so far.
    pub fn dropped_frames(&self) -> usize {
        self.reader.dropped_frames()
    }

    /// Bytes held back waiting for more input.
    pub fn buffered(&self) -> usize {
        self.reader.buffered()
    }

    /// The next complete item, or `None` until more bytes arrive.
    pub fn next_item(&mut self) -> Option<Result<CdcItem, AdcnewError>> {
        loop {
            let item = match self.reader.next_token()? {
                Ok(Token::Frame(frame)) => Ok(CdcItem::Frame(frame)),
                Ok(Token::Line(line)) => Ok(CdcItem::Line(line)),
                Ok(Token::Reply(bytes)) => Ok(CdcItem::Reply(bytes)),
                // Block detection is off in this reader.
                Ok(Token::Block(_)) => continue,
                Err(err) => Err(AdcnewError::Text(err)),
            };
            return Some(item);
        }
    }
}
