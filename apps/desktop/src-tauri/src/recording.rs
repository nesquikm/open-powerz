//! Recording the stream to CSV and exporting the session (FR 0P7HSJ).
//!
//! Rows are written by [`powerz_record::CsvRecorder`], the same writer
//! `powerz stream` uses, so both files are byte-for-byte alike. The worker
//! thread owns the open recording: file I/O never runs on the UI thread.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use powerz_protocol::decode::QueueRecord;
use powerz_record::CsvRecorder;
use serde::Serialize;

use crate::ui_error::{ErrorKind, UiError};
use crate::worker::Gap;

/// What a finished recording wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordingSummary {
    /// Data rows written (the header not counted).
    pub rows: u64,
    /// Stream gaps seen while recording.
    pub gaps: Vec<Gap>,
    /// The recording ended because the meter went away.
    pub disconnected: bool,
}

/// Recording state changes sent to the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RecordingEvent {
    /// The file is open and the header written.
    Started,
    /// Rows written and time recorded so far.
    Progress {
        /// Data rows written.
        rows: u64,
        /// Time since the recording started, ms.
        elapsed_ms: u64,
    },
    /// The recording ended without being asked to (the meter went away).
    Stopped(RecordingSummary),
    /// A write failed: the recording stopped, the rows before it are kept.
    Failed {
        /// Data rows written before the failure.
        rows: u64,
        /// What went wrong, in plain language.
        error: UiError,
    },
}

/// How far an export has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExportProgress {
    /// Rows written.
    pub done: u64,
    /// Rows to write.
    pub total: u64,
}

/// A file error in plain language (no raw OS error text).
pub fn io_error(err: &io::Error) -> UiError {
    let (kind, message) = match err.kind() {
        io::ErrorKind::NotFound => (ErrorKind::Usage, "The folder for the file does not exist."),
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => (
            ErrorKind::PermissionDenied,
            "The app is not allowed to write the file there.",
        ),
        io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => {
            (ErrorKind::Internal, "The disk is full.")
        }
        io::ErrorKind::IsADirectory | io::ErrorKind::InvalidFilename => {
            (ErrorKind::Usage, "That is not a file name the app can use.")
        }
        _ => (ErrorKind::Internal, "The file could not be written."),
    };
    UiError {
        kind,
        message: message.to_owned(),
        hint: Some("Pick another place to save the file.".to_owned()),
    }
}

/// Create (or truncate) the file at `path`, with a plain-language error.
pub(crate) fn create_file(path: &Path) -> Result<File, UiError> {
    File::create(path).map_err(|e| io_error(&e))
}

/// Collects rows in memory and hands them to the file only on `flush`, so
/// a healthy file always ends on a whole row. A write that fails partway
/// (disk full) can leave a partial last row, which a plain `Write` cannot
/// take back; `committed_lines` counts only rows written in full.
struct RowBuffer {
    pending: Vec<u8>,
    out: Box<dyn Write + Send>,
    /// Lines (header included) that reached `out` in full.
    committed_lines: u64,
}

impl Write for RowBuffer {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            self.out.write_all(&self.pending)?;
            self.committed_lines += self.pending.iter().filter(|&&b| b == b'\n').count() as u64;
            self.pending.clear();
        }
        self.out.flush()
    }
}

/// How often an open recording is flushed to the file.
const FLUSH_PERIOD_MS: u64 = 250;
/// How often progress goes to the UI.
const PROGRESS_PERIOD_MS: u64 = 500;

/// An open recording, owned by the worker thread.
pub(crate) struct Recorder {
    csv: CsvRecorder<RowBuffer>,
    start_ms: u64,
    last_flush_ms: u64,
    last_progress_ms: u64,
    gaps: Vec<Gap>,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder")
            .field("rows", &self.csv.rows())
            .finish_non_exhaustive()
    }
}

impl Recorder {
    /// Write the header to `out` and flush it, at wall-clock time `now_ms`.
    pub(crate) fn start(out: Box<dyn Write + Send>, now_ms: u64) -> io::Result<Recorder> {
        let mut csv = CsvRecorder::new(RowBuffer {
            pending: Vec::new(),
            out,
            committed_lines: 0,
        })?;
        csv.flush()?;
        Ok(Recorder {
            csv,
            start_ms: now_ms,
            last_flush_ms: now_ms,
            last_progress_ms: now_ms,
            gaps: Vec::new(),
        })
    }

    /// Rows that are in the file for sure: flushed in full, header not counted.
    pub(crate) fn kept_rows(&self) -> u64 {
        self.csv.get_ref().committed_lines.saturating_sub(1)
    }

    /// Add one row; `now_unix_ms` fixes the `Unix` origin on the first row.
    pub(crate) fn push(&mut self, r: &QueueRecord, now_unix_ms: u64) -> io::Result<()> {
        self.csv.push(r, now_unix_ms)
    }

    /// Remember a stream gap seen while recording.
    pub(crate) fn gap(&mut self, gap: Gap) {
        self.gaps.push(gap);
    }

    /// Flush when due; returns a progress event when one is due.
    pub(crate) fn tick(&mut self, now_ms: u64) -> io::Result<Option<RecordingEvent>> {
        if now_ms.saturating_sub(self.last_flush_ms) >= FLUSH_PERIOD_MS {
            self.last_flush_ms = now_ms;
            self.csv.flush()?;
        }
        if now_ms.saturating_sub(self.last_progress_ms) >= PROGRESS_PERIOD_MS {
            self.last_progress_ms = now_ms;
            return Ok(Some(RecordingEvent::Progress {
                rows: self.csv.rows(),
                elapsed_ms: now_ms.saturating_sub(self.start_ms),
            }));
        }
        Ok(None)
    }

    /// Flush and close the file.
    pub(crate) fn finish(self, disconnected: bool) -> Result<RecordingSummary, (u64, UiError)> {
        let rows = self.csv.rows();
        // If the last flush fails, only what reached the file before it counts.
        let kept = self.kept_rows();
        match self.csv.finish() {
            Ok(_) => Ok(RecordingSummary {
                rows,
                gaps: self.gaps,
                disconnected,
            }),
            Err(e) => Err((kept, io_error(&e))),
        }
    }
}

/// How many rows an export writes between two progress reports.
const EXPORT_PROGRESS_ROWS: usize = 10_000;

/// Write `records` as a recording would have (header, then one row each,
/// `Unix` counted from `start_unix_ms` at the first record) to `out`,
/// reporting progress along the way. Returns the rows written; `out` is
/// flushed and closed.
pub fn export_records<W: Write>(
    records: &[QueueRecord],
    out: W,
    start_unix_ms: u64,
    mut progress: impl FnMut(ExportProgress),
) -> Result<u64, UiError> {
    let total = records.len() as u64;
    let fail = |e: io::Error| io_error(&e);
    let mut csv = CsvRecorder::new(BufWriter::with_capacity(1 << 16, out)).map_err(fail)?;
    for chunk in records.chunks(EXPORT_PROGRESS_ROWS) {
        for r in chunk {
            csv.push(r, start_unix_ms).map_err(fail)?;
        }
        progress(ExportProgress {
            done: csv.rows(),
            total,
        });
    }
    let rows = csv.rows();
    let buffered = csv.finish().map_err(fail)?;
    buffered.into_inner().map_err(|e| io_error(e.error()))?;
    if records.is_empty() {
        progress(ExportProgress { done: 0, total: 0 });
    }
    Ok(rows)
}
