//! Reading a recorder CSV back (FR AXHK2S): our own files and the vendor's,
//! which share [`CSV_HEADER`].
//!
//! [`CsvReader`] streams one row at a time, so a file of any size takes the
//! memory of one row. A row that cannot be trusted is [`RowOutcome::Damaged`],
//! not an error: only a file that is no recording, or a failed read, is. A
//! blank line is neither.

use std::io::{self, BufRead};

use thiserror::Error;

use crate::CSV_HEADER;

/// Longest row taken as a row; anything longer is damage.
const MAX_ROW_BYTES: usize = 4096;
/// Bytes of a line kept in memory: a row at the limit and its CR, and one
/// more to tell it from a longer line.
const LINE_CAP: usize = MAX_ROW_BYTES + 2;
/// Fields of a row: the columns of [`CSV_HEADER`].
const FIELDS: usize = 12;
/// Largest VBUS (V) or IBUS (A) magnitude: what the meter's microunit integers
/// hold.
const MAX_BUS: f64 = 2147.0;
/// Largest CC or D± reading, V: what the meter's 1 mV code holds.
const MAX_LINE_V: f64 = 65.535;

/// A UTF-8 byte order mark, as spreadsheets write it.
const BOM: &[u8] = b"\xEF\xBB\xBF";

/// One readable row of a recording.
#[derive(Debug, Clone, PartialEq)]
pub struct FileRow {
    /// Time since the recording started, ms.
    pub elapsed_ms: u64,
    /// Wall clock, s since the Unix epoch.
    pub unix: Option<f64>,
    pub vbus_v: f64,
    pub ibus_a: f64,
    pub dp_v: f64,
    pub dm_v: f64,
    pub cc1_v: f64,
    pub cc2_v: f64,
    pub temp_c: Option<f64>,
    pub charge_ah: Option<f64>,
    pub energy_wh: Option<f64>,
}

impl FileRow {
    /// Whether every value is one a meter could have written: NaN is in no
    /// range. A reader of another format checks its rows with this, as
    /// [`CsvReader`] does field by field.
    pub fn in_range(&self) -> bool {
        is_bus(self.vbus_v)
            && is_bus(self.ibus_a)
            && [self.dp_v, self.dm_v, self.cc1_v, self.cc2_v]
                .iter()
                .all(|&v| is_line_v(v))
            && [self.unix, self.temp_c, self.charge_ah, self.energy_wh]
                .iter()
                .flatten()
                .all(|v| v.is_finite())
    }
}

/// What one line of a recording turned out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum RowOutcome {
    /// A row with every value it needs.
    Row(FileRow),
    /// A row that cannot be trusted: to be skipped and counted.
    Damaged,
    /// A blank line, which is no row at all. It still comes back, so a file
    /// of nothing but blank lines stays cancellable between two of them.
    Blank,
}

/// Why a recording could not be read. Each one reads as a plain sentence.
#[derive(Debug, Error)]
pub enum ReadError {
    /// The file holds nothing at all.
    #[error("This file is empty.")]
    Empty,
    /// The first line is not the recorder header.
    #[error("This file's header is not a recorder header.")]
    BadHeader,
    /// The file is binary.
    #[error("This file is not a text file, so it is not a recording.")]
    NotText,
    /// The file could not be read to its end.
    #[error("The file could not be read.")]
    Io(#[from] io::Error),
}

/// What [`CsvReader::read_line`] found.
enum Line {
    /// The end of the file: no line.
    End,
    /// A line, whole, in the reader's buffer.
    Whole,
    /// A line longer than the buffer, the start of it in the buffer.
    Overlong,
}

/// Streams the rows of a recorder CSV from `R`.
#[derive(Debug)]
pub struct CsvReader<R> {
    input: R,
    /// The line last read, without its line feed.
    line: Vec<u8>,
    /// Bytes taken from `input` so far.
    consumed: u64,
    /// Time of the last good row: a row before it goes backwards.
    last_elapsed_ms: u64,
}

impl<R: BufRead> CsvReader<R> {
    /// A reader past the header of `input`; a file that does not start with
    /// [`CSV_HEADER`] is refused. A UTF-8 byte order mark and CRLF line ends
    /// are accepted.
    pub fn new(input: R) -> Result<CsvReader<R>, ReadError> {
        let mut reader = CsvReader {
            input,
            line: Vec::with_capacity(LINE_CAP),
            consumed: 0,
            last_elapsed_ms: 0,
        };
        match reader.read_line()? {
            Line::End => Err(ReadError::Empty),
            Line::Whole | Line::Overlong => check_header(&reader.line).map(|()| reader),
        }
    }

    /// The next row; `None` at the end of the file, and from then on.
    pub fn next_row(&mut self) -> Result<Option<RowOutcome>, ReadError> {
        match self.read_line()? {
            Line::End => Ok(None),
            Line::Overlong => {
                self.skip_line()?;
                Ok(Some(RowOutcome::Damaged))
            }
            Line::Whole => Ok(Some(self.judge_line())),
        }
    }

    /// Bytes read so far, for a progress report.
    pub fn bytes_read(&self) -> u64 {
        self.consumed
    }

    /// The whole line in the buffer as a row.
    fn judge_line(&mut self) -> RowOutcome {
        let bytes = without_cr(&self.line);
        if bytes.is_empty() {
            return RowOutcome::Blank;
        }
        let row = (bytes.len() <= MAX_ROW_BYTES)
            .then(|| std::str::from_utf8(bytes).ok())
            .flatten()
            .and_then(parse_row)
            .filter(|row| row.elapsed_ms >= self.last_elapsed_ms);
        match row {
            Some(row) => {
                self.last_elapsed_ms = row.elapsed_ms;
                RowOutcome::Row(row)
            }
            None => RowOutcome::Damaged,
        }
    }

    /// Read the next line into the buffer, up to [`LINE_CAP`] bytes, so a
    /// file of one huge line never lands in memory.
    fn read_line(&mut self) -> io::Result<Line> {
        self.line.clear();
        let mut any = false;
        loop {
            let buf = fill(&mut self.input)?;
            if buf.is_empty() {
                return Ok(if any { Line::Whole } else { Line::End });
            }
            any = true;
            let newline = buf.iter().position(|&b| b == b'\n');
            let content = newline.unwrap_or(buf.len());
            let kept = content.min(LINE_CAP - self.line.len());
            self.line.extend_from_slice(&buf[..kept]);
            let whole = kept == content;
            let used = match newline {
                Some(at) if whole => at + 1,
                _ => kept,
            };
            self.input.consume(used);
            self.consumed += used as u64;
            if !whole {
                return Ok(Line::Overlong);
            }
            if newline.is_some() {
                return Ok(Line::Whole);
            }
        }
    }

    /// Pass over the rest of the current line.
    fn skip_line(&mut self) -> io::Result<()> {
        loop {
            let buf = fill(&mut self.input)?;
            if buf.is_empty() {
                return Ok(());
            }
            let newline = buf.iter().position(|&b| b == b'\n');
            let used = newline.map_or(buf.len(), |at| at + 1);
            self.input.consume(used);
            self.consumed += used as u64;
            if newline.is_some() {
                return Ok(());
            }
        }
    }
}

/// The buffered bytes of `input`, refilled when empty.
fn fill<R: BufRead>(input: &mut R) -> io::Result<&[u8]> {
    loop {
        match input.fill_buf() {
            // Borrowing the answer across the loop is what the borrow checker
            // refuses, so the slice is taken again once it is known to be good.
            Ok(_) => return input.fill_buf(),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

fn without_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// Whether the first line, or the start of an overlong one, is the header.
fn check_header(line: &[u8]) -> Result<(), ReadError> {
    let head = without_cr(line.strip_prefix(BOM).unwrap_or(line));
    match std::str::from_utf8(head) {
        Ok(text) if text.contains('\0') => Err(ReadError::NotText),
        Ok(text) if text == CSV_HEADER => Ok(()),
        // Only an overlong line ends inside a character: that alone is no binary.
        Err(e) if e.error_len().is_some() => Err(ReadError::NotText),
        _ => Err(ReadError::BadHeader),
    }
}

/// `text` as a row; `None` when it is damaged.
fn parse_row(text: &str) -> Option<FileRow> {
    let mut parts = text.split(',');
    let mut fields = [""; FIELDS];
    for field in &mut fields {
        *field = parts.next()?;
    }
    if parts.next().is_some() {
        return None;
    }
    let [
        elapsed,
        unix,
        vbus,
        ibus,
        dp,
        dm,
        cc1,
        cc2,
        temp,
        charge,
        energy,
        _power,
    ] = fields;
    // `powerz records get` writes no line voltages: such a row is whole.
    let [dp_v, dm_v, cc1_v, cc2_v] = if [dp, dm, cc1, cc2].iter().all(|f| f.is_empty()) {
        [0.0; 4]
    } else {
        [line_v(dp)?, line_v(dm)?, line_v(cc1)?, line_v(cc2)?]
    };
    Some(FileRow {
        elapsed_ms: parse_elapsed(elapsed)?,
        unix: optional(unix)?,
        vbus_v: bus(vbus)?,
        ibus_a: bus(ibus)?,
        dp_v,
        dm_v,
        cc1_v,
        cc2_v,
        temp_c: optional(temp)?,
        charge_ah: optional(charge)?,
        energy_wh: optional(energy)?,
    })
}

/// A VBUS or IBUS value within what the meter measures. NaN is in no range.
fn bus(field: &str) -> Option<f64> {
    let value: f64 = field.parse().ok()?;
    is_bus(value).then_some(value)
}

fn is_bus(value: f64) -> bool {
    value.abs() <= MAX_BUS
}

/// A CC or D± value within what the meter measures. NaN is in no range.
fn line_v(field: &str) -> Option<f64> {
    let value: f64 = field.parse().ok()?;
    is_line_v(value).then_some(value)
}

fn is_line_v(value: f64) -> bool {
    (0.0..=MAX_LINE_V).contains(&value)
}

/// A column that may be missing: `Some(None)` when it is empty or unreadable,
/// `None` (damage) when it holds a value that is no measurement.
fn optional(field: &str) -> Option<Option<f64>> {
    match field.parse::<f64>() {
        Ok(value) if !value.is_finite() => None,
        Ok(value) => Some(Some(value)),
        Err(_) => Some(None),
    }
}

/// `h:mm:ss.mmm`, hours unpadded and unwrapped, in ms.
pub fn parse_elapsed(text: &str) -> Option<u64> {
    let (hours, rest) = text.split_once(':')?;
    let (minutes, rest) = rest.split_once(':')?;
    let (seconds, fraction) = rest.split_once('.').unwrap_or((rest, ""));
    let (hours, minutes, seconds) = (digits(hours)?, digits(minutes)?, digits(seconds)?);
    if minutes >= 60 || seconds >= 60 {
        return None;
    }
    hours
        .checked_mul(3_600_000)?
        .checked_add(minutes * 60_000 + seconds * 1_000 + millis(fraction)?)
}

/// A number of plain digits, so no sign or space passes.
fn digits(text: &str) -> Option<u64> {
    let plain = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    if plain { text.parse().ok() } else { None }
}

/// The digits after the seconds' point, in ms; digits past the third are
/// finer than the column.
fn millis(fraction: &str) -> Option<u64> {
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut places = fraction.bytes();
    let mut ms = 0;
    for _ in 0..3 {
        ms = ms * 10 + places.next().map_or(0, |b| u64::from(b - b'0'));
    }
    Some(ms)
}
