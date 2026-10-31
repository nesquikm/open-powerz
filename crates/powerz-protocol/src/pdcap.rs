//! The pdcap capture file format (FR TGE83E): a PD capture as JSON Lines.
//!
//! Line 1 is the header (format, format version, app version, meter model
//! and firmware version); every further line is one status sample or one
//! record, with its session time and its raw bytes in lowercase hex. The
//! meter's serial number has no field anywhere. See `docs/reference/pdcap.md`.
//!
//! Pure: [`Writer`] works on any [`Write`], [`Reader`] and [`load`] on any
//! [`BufRead`]; no file is opened here.

use std::io::{self, BufRead, Read, Write};

use serde::{Deserialize, Serialize};

use crate::decode::pd_packet::{PdChunk, PdRecord, PdStatus, STATUS_LEN};

/// The format version this library writes and the newest it reads.
pub const FORMAT_VERSION: u32 = 1;

/// The `format` value of a pdcap header line.
const FORMAT_NAME: &str = "pdcap";

/// The longest line a reader takes, in bytes: a record line is a few hundred
/// bytes, so a longer one is damage and is not read into memory whole.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// The header line of a pdcap file. There is no serial number field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Format version of the file ([`FORMAT_VERSION`] when written here).
    pub format_version: u32,
    /// Version of the app that wrote the file.
    pub app_version: String,
    /// Meter model, e.g. `KM003C`.
    pub model: String,
    /// Meter firmware version.
    pub firmware_version: String,
}

impl Header {
    /// The header this library writes: [`FORMAT_VERSION`], the writing
    /// app's version and the meter's model and firmware version.
    pub fn new(app_version: &str, model: String, firmware_version: String) -> Header {
        Header {
            format_version: FORMAT_VERSION,
            app_version: app_version.to_owned(),
            model,
            firmware_version,
        }
    }
}

/// One line after the header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A PD status block, at session time `session_ms`.
    Status {
        /// Session time, ms.
        session_ms: u64,
        /// The status block.
        status: PdStatus,
    },
    /// A captured PD record.
    Record {
        /// Session time, ms.
        session_ms: u64,
        /// Ms since the capture start.
        elapsed_ms: u64,
        /// The record, decoded from its raw bytes.
        record: PdRecord,
    },
}

/// Why reading stopped.
#[derive(Debug, thiserror::Error)]
pub enum ErrorKind {
    /// The file could not be read.
    #[error("read failed: {0}")]
    Io(#[from] io::Error),
    /// The file has no header line.
    #[error("the file is empty, a pdcap header was expected")]
    MissingHeader,
    /// The line is not the JSON object it should be.
    #[error("{}", json_reason(.0))]
    Json(#[from] serde_json::Error),
    /// The line is not valid UTF-8.
    #[error("the line is not UTF-8 text")]
    NotUtf8,
    /// The line is longer than [`MAX_LINE_BYTES`].
    #[error("the line is too long (over {MAX_LINE_BYTES} bytes)")]
    LineTooLong,
    /// The header names another format.
    #[error("not a pdcap file (format {0:?})")]
    NotPdcap(String),
    /// The header names a newer format version than this reader knows.
    #[error("format version {0} is newer than {FORMAT_VERSION}, which this app reads")]
    NewerVersion(u32),
    /// `raw` is not an even number of hex digits.
    #[error("`raw` is not hex bytes")]
    BadHex,
    /// A status line's bytes are not one status block.
    #[error("`raw` is {0} bytes, a status block is {STATUS_LEN}")]
    BadStatus(usize),
    /// A record line's bytes are not one whole record.
    #[error("`raw` is not one whole PD record")]
    BadRecord,
}

/// `e` without its own line number: it parsed one line of the file, so its
/// "line 1" would contradict the file line [`Error`] names.
fn json_reason(e: &serde_json::Error) -> String {
    let text = e.to_string();
    let own_place = format!(" at line {} column {}", e.line(), e.column());
    match text.strip_suffix(&own_place) {
        Some(reason) => format!("{reason} at column {}", e.column()),
        None => text,
    }
}

/// A pdcap file could not be read past line [`Error::line`].
#[derive(Debug, thiserror::Error)]
#[error("pdcap line {line}: {kind}")]
pub struct Error {
    line: usize,
    kind: ErrorKind,
}

impl Error {
    fn at(line: usize, kind: impl Into<ErrorKind>) -> Error {
        Error {
            line,
            kind: kind.into(),
        }
    }

    /// The 1-based line where reading stopped (the header is line 1).
    pub fn line(&self) -> usize {
        self.line
    }

    /// Why reading stopped there.
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }
}

#[derive(Serialize, Deserialize)]
struct HeaderLine {
    format: String,
    format_version: u32,
    app_version: String,
    model: String,
    firmware_version: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum EntryLine {
    Status {
        session_ms: u64,
        raw: String,
    },
    Record {
        session_ms: u64,
        elapsed_ms: u64,
        raw: String,
    },
}

fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(DIGITS[usize::from(b >> 4)]));
        s.push(char::from(DIGITS[usize::from(b & 0x0F)]));
    }
    s
}

fn from_hex(s: &str) -> Result<Vec<u8>, ErrorKind> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ErrorKind::BadHex);
    }
    s.as_bytes()
        .chunks(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|p| u8::from_str_radix(p, 16).ok())
                .ok_or(ErrorKind::BadHex)
        })
        .collect()
}

/// The 12 status bytes as the meter sends them (little-endian).
fn status_bytes(s: &PdStatus) -> Vec<u8> {
    let mut b = Vec::with_capacity(STATUS_LEN);
    b.extend(s.time_ms.to_le_bytes());
    b.extend(s.vbus_mv.to_le_bytes());
    b.extend(s.ibus_ma.to_le_bytes());
    b.extend(s.cc1_mv.to_le_bytes());
    b.extend(s.cc2_mv.to_le_bytes());
    b
}

/// One whole record from exactly its own bytes.
fn record_from(raw: &[u8]) -> Result<PdRecord, ErrorKind> {
    let mut chunk = vec![0; STATUS_LEN];
    chunk.extend_from_slice(raw);
    let parsed = PdChunk::parse(&chunk).map_err(|_| ErrorKind::BadRecord)?;
    let mut records = parsed.records;
    match (records.pop(), records.is_empty(), parsed.skipped) {
        (Some(r), true, 0) if r.raw == raw => Ok(r),
        _ => Err(ErrorKind::BadRecord),
    }
}

/// Writes a pdcap file: the header line on creation, one line per entry.
#[derive(Debug)]
pub struct Writer<W: Write> {
    out: W,
}

impl<W: Write> Writer<W> {
    /// Start a file on `out` with its `header` line.
    pub fn new(mut out: W, header: &Header) -> io::Result<Writer<W>> {
        let line = HeaderLine {
            format: FORMAT_NAME.to_owned(),
            format_version: header.format_version,
            app_version: header.app_version.clone(),
            model: header.model.clone(),
            firmware_version: header.firmware_version.clone(),
        };
        write_line(&mut out, &line)?;
        Ok(Writer { out })
    }

    /// Write one entry as one line.
    pub fn write(&mut self, entry: &Entry) -> io::Result<()> {
        let line = match entry {
            Entry::Status { session_ms, status } => EntryLine::Status {
                session_ms: *session_ms,
                raw: to_hex(&status_bytes(status)),
            },
            Entry::Record {
                session_ms,
                elapsed_ms,
                record,
            } => EntryLine::Record {
                session_ms: *session_ms,
                elapsed_ms: *elapsed_ms,
                raw: to_hex(&record.raw),
            },
        };
        write_line(&mut self.out, &line)
    }

    /// Flush what was written so far to `out`.
    pub fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }

    /// Flush and hand `out` back.
    pub fn finish(mut self) -> io::Result<W> {
        self.out.flush()?;
        Ok(self.out)
    }
}

fn write_line(out: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer(&mut *out, value).map_err(io::Error::other)?;
    out.write_all(b"\n")
}

/// Reads the next non-blank line of `input`; `None` at the end. A line over
/// [`MAX_LINE_BYTES`] is damage; at most one byte past the cap is read.
fn next_line(input: &mut impl BufRead, line_no: &mut usize) -> Option<Result<String, Error>> {
    // The cap plus the newline, plus one byte to tell an over-long line.
    let limit = MAX_LINE_BYTES as u64 + 2;
    loop {
        let mut buf = Vec::new();
        *line_no += 1;
        match input.by_ref().take(limit).read_until(b'\n', &mut buf) {
            Ok(0) => return None,
            Ok(_) => {}
            Err(e) => return Some(Err(Error::at(*line_no, e))),
        }
        let body = buf.strip_suffix(b"\n").unwrap_or(&buf);
        let body = body.strip_suffix(b"\r").unwrap_or(body);
        if body.len() > MAX_LINE_BYTES {
            return Some(Err(Error::at(*line_no, ErrorKind::LineTooLong)));
        }
        let text = match String::from_utf8(buf) {
            Ok(t) => t,
            Err(_) => return Some(Err(Error::at(*line_no, ErrorKind::NotUtf8))),
        };
        if !text.trim().is_empty() {
            return Some(Ok(text));
        }
    }
}

fn parse_header(text: &str) -> Result<Header, ErrorKind> {
    let h: HeaderLine = serde_json::from_str(text)?;
    if h.format != FORMAT_NAME {
        return Err(ErrorKind::NotPdcap(h.format));
    }
    if h.format_version > FORMAT_VERSION {
        return Err(ErrorKind::NewerVersion(h.format_version));
    }
    Ok(Header {
        format_version: h.format_version,
        app_version: h.app_version,
        model: h.model,
        firmware_version: h.firmware_version,
    })
}

fn parse_entry(text: &str) -> Result<Entry, ErrorKind> {
    Ok(match serde_json::from_str::<EntryLine>(text)? {
        EntryLine::Status { session_ms, raw } => {
            let bytes = from_hex(&raw)?;
            if bytes.len() != STATUS_LEN {
                return Err(ErrorKind::BadStatus(bytes.len()));
            }
            let status = PdStatus::decode(&bytes).map_err(|_| ErrorKind::BadStatus(bytes.len()))?;
            Entry::Status { session_ms, status }
        }
        EntryLine::Record {
            session_ms,
            elapsed_ms,
            raw,
        } => Entry::Record {
            session_ms,
            elapsed_ms,
            record: record_from(&from_hex(&raw)?)?,
        },
    })
}

/// Streams a pdcap file: the header on creation, then one entry per line.
/// After the first damaged line it yields that error and stops.
#[derive(Debug)]
pub struct Reader<R: BufRead> {
    input: R,
    header: Header,
    line: usize,
    done: bool,
}

impl<R: BufRead> Reader<R> {
    /// Read the header line of `input`.
    pub fn new(mut input: R) -> Result<Reader<R>, Error> {
        let mut line = 0;
        let text = next_line(&mut input, &mut line)
            .unwrap_or_else(|| Err(Error::at(line, ErrorKind::MissingHeader)))?;
        let header = parse_header(&text).map_err(|k| Error::at(line, k))?;
        Ok(Reader {
            input,
            header,
            line,
            done: false,
        })
    }

    /// The file's header.
    pub fn header(&self) -> &Header {
        &self.header
    }
}

impl<R: BufRead> Iterator for Reader<R> {
    type Item = Result<Entry, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let item = next_line(&mut self.input, &mut self.line)
            .map(|text| text.and_then(|t| parse_entry(&t).map_err(|k| Error::at(self.line, k))));
        if !matches!(item, Some(Ok(_))) {
            self.done = true;
        }
        item
    }
}

/// A whole pdcap file as far as it could be read.
#[derive(Debug)]
pub struct Loaded {
    /// The header, when line 1 is one.
    pub header: Option<Header>,
    /// Every entry before the first damaged line.
    pub entries: Vec<Entry>,
    /// The damage, if any.
    pub error: Option<Error>,
}

/// Read every valid line of `input` up to the first damaged one.
pub fn load(input: impl BufRead) -> Loaded {
    let reader = match Reader::new(input) {
        Ok(r) => r,
        Err(e) => {
            return Loaded {
                header: None,
                entries: Vec::new(),
                error: Some(e),
            };
        }
    };
    let header = Some(reader.header().clone());
    let mut entries = Vec::new();
    let mut error = None;
    for item in reader {
        match item {
            Ok(e) => entries.push(e),
            Err(e) => error = Some(e),
        }
    }
    Loaded {
        header,
        entries,
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_refuses_bad_digits() {
        assert_eq!(to_hex(&[0x00, 0xAB, 0x7f]), "00ab7f");
        assert_eq!(from_hex("00ab7F").ok(), Some(vec![0x00, 0xAB, 0x7F]));
        assert!(from_hex("0").is_err());
        assert!(from_hex("zz").is_err());
        assert!(from_hex("+1").is_err());
    }

    #[test]
    fn a_record_with_trailing_bytes_is_not_one_record() {
        // An event record (6 bytes) followed by a stray byte.
        assert!(record_from(&[0x05, 0, 0, 0, 0, 0x11]).is_ok());
        assert!(record_from(&[0x05, 0, 0, 0, 0, 0x11, 0x00]).is_err());
        assert!(record_from(&[]).is_err());
    }
}
