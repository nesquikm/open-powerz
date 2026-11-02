//! Recording the stream to CSV and exporting the session (FR 0P7HSJ).
//!
//! Rows are written by [`powerz_record::CsvRecorder`], the same writer
//! `powerz stream` uses, so both files are byte-for-byte alike. The worker
//! thread owns the open recording: file I/O never runs on the UI thread.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use powerz_protocol::decode::QueueRecord;
use powerz_record::CsvRecorder;
use serde::{Deserialize, Serialize};

use crate::ui_error::{ErrorKind, UiError};
use crate::worker::{Gap, GapKind};

/// The row limit of a recording unless the user sets another (FR AXHK2R).
pub const DEFAULT_MAX_ROWS: u64 = 10_000_000;

/// What ends a recording on its own (FR AXHK2R).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// End once the current's magnitude stays below this, A; `None` for never.
    pub below_a: Option<f64>,
    /// How long the current must stay low, ms.
    pub for_ms: u64,
    /// End at this many rows.
    pub max_rows: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            below_a: None,
            for_ms: 0,
            max_rows: DEFAULT_MAX_ROWS,
        }
    }
}

impl Limits {
    /// Whether these limits can hold a recording: a current to stop below is
    /// above 0 A and comes with a time, and a recording can reach one row.
    pub fn check(&self) -> Result<(), UiError> {
        if let Some(below_a) = self.below_a {
            if !below_a.is_finite() || below_a <= 0.0 {
                return Err(UiError::usage("Auto-stop needs a current above 0 A."));
            }
            if self.for_ms == 0 {
                return Err(UiError::usage("Auto-stop needs a time above 0 seconds."));
            }
        }
        if self.max_rows == 0 {
            return Err(UiError::usage("The row limit must be at least 1 row."));
        }
        Ok(())
    }
}

/// What the page starts a recording with (FR AXHK2R); absent fields keep the
/// defaults.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RecordingOptions {
    /// The folder to record into.
    pub folder: Option<PathBuf>,
    /// End once the current's magnitude stays below this, A.
    pub below_a: Option<f64>,
    /// How long the current must stay low, ms.
    pub for_ms: Option<u64>,
    /// End at this many rows.
    pub max_rows: Option<u64>,
}

impl RecordingOptions {
    /// The limits these options set. An auto-stop with no time gets 0 ms,
    /// which [`Limits::check`] refuses.
    pub fn limits(&self) -> Limits {
        Limits {
            below_a: self.below_a,
            for_ms: self.for_ms.unwrap_or(0),
            max_rows: self.max_rows.unwrap_or(DEFAULT_MAX_ROWS),
        }
    }
}

/// Why a recording ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    /// The user stopped it, or the session was disconnected on request.
    Stopped,
    /// The meter went away.
    Disconnected,
    /// The current stayed below the set value for the set time.
    LowCurrent,
    /// It reached the row limit.
    RowLimit,
}

/// What a finished recording wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordingSummary {
    /// Data rows written (the header not counted).
    pub rows: u64,
    /// Stream gaps seen while recording, and pauses.
    pub gaps: Vec<Gap>,
    /// The recording ended because the meter went away.
    pub disconnected: bool,
    /// Why it ended.
    pub ended: EndReason,
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
        /// Rows are held back by a pause.
        paused: bool,
    },
    /// The recording ended without being asked to: the meter went away, or a
    /// stop rule fired ([`RecordingSummary::ended`] says which).
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

/// Claim a new file in `dir` (created when missing) for a recording that has
/// no place chosen yet: `name` when that is free, else `<stem>-2.csv` and so
/// on. Creating the file is what claims it, so two meters starting in the
/// same second never share one and an existing file is never touched.
pub fn default_recording_path(dir: &Path, name: &str) -> Result<PathBuf, UiError> {
    fs::create_dir_all(dir).map_err(|e| io_error(&e))?;
    claim_recording_file(dir, name)
}

/// Create a new file in `dir` for `name`, or `<stem>-2.csv` and so on when
/// that is taken.
fn claim_recording_file(dir: &Path, name: &str) -> Result<PathBuf, UiError> {
    let stem = Path::new(name)
        .file_stem()
        .map_or_else(|| "powerz".into(), |s| s.to_string_lossy());
    let mut attempt = 1u32;
    loop {
        let file = if attempt == 1 {
            format!("{stem}.csv")
        } else {
            format!("{stem}-{attempt}.csv")
        };
        let path = dir.join(file);
        match File::options().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => attempt += 1,
            Err(e) => return Err(io_error(&e)),
        }
    }
}

/// Whether `name` can be a recording's file name on every OS: no path
/// separator, nothing Windows forbids or mangles, no hidden file, and
/// something before the `.csv`. That also keeps it inside its folder.
pub fn recording_name_ok(name: &str) -> bool {
    let stem = name.strip_suffix(".csv").unwrap_or(name);
    !stem.trim().is_empty()
        && !stem.starts_with('.')
        && !stem.ends_with(['.', ' '])
        && !is_windows_device(stem)
        && !name.chars().any(|c| c < ' ' || r#"/\:*?"<>|"#.contains(c))
}

/// Windows keeps these names for devices, whatever follows the first dot.
fn is_windows_device(stem: &str) -> bool {
    let base = stem.split('.').next().unwrap_or(stem).to_ascii_lowercase();
    match base.as_str() {
        "con" | "prn" | "aux" | "nul" => true,
        other => other
            .strip_prefix("com")
            .or_else(|| other.strip_prefix("lpt"))
            .is_some_and(|n| matches!(n.as_bytes(), [b'1'..=b'9'])),
    }
}

/// The answer to a name [`recording_name_ok`] refuses.
pub fn bad_recording_name() -> UiError {
    UiError::usage("That is not a file name the app can use.")
}

/// Claim a new file in the folder the user set, as
/// [`default_recording_path`] does, except that a folder that is missing is
/// refused and not created: it may be a drive that is gone.
pub fn folder_recording_path(dir: &Path, name: &str) -> Result<PathBuf, UiError> {
    if !recording_name_ok(name) {
        return Err(bad_recording_name());
    }
    if !fs::metadata(dir).map_err(|e| io_error(&e))?.is_dir() {
        return Err(UiError::usage("The recording folder is not a folder."));
    }
    claim_recording_file(dir, name)
}

/// Run `start` on the file claimed at `path`, and clear up after a refusal. A
/// timed-out answer says nothing about the start: the worker may still apply
/// it and write to the file, so that file stays. Any other error is a definite
/// refusal, which leaves an empty placeholder nobody will use.
pub fn start_at(
    path: &Path,
    start: impl FnOnce(&Path) -> Result<(), UiError>,
) -> Result<(), UiError> {
    let answer = start(path);
    if let Err(refused) = &answer
        && refused.kind != ErrorKind::Timeout
    {
        let _ = fs::remove_file(path);
    }
    answer
}

/// Move the finished recording `from` to `to`, replacing a file there. On a
/// failure the recording stays at `from`.
pub fn save_recording_as(from: &Path, to: &Path) -> Result<(), UiError> {
    match fs::rename(from, to) {
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => move_across_volumes(from, to),
        moved => moved,
    }
    .map_err(|e| io_error(&e))
}

/// Copy beside `to` first, so a failed copy never damages a file already there.
fn move_across_volumes(from: &Path, to: &Path) -> io::Result<()> {
    let part = claim_part(to)?;
    let copied = fs::copy(from, &part).and_then(|_| fs::rename(&part, to));
    if copied.is_err() {
        let _ = fs::remove_file(&part);
    }
    copied?;
    // The recording is safe at `to`: a copy left behind is no failed move.
    let _ = fs::remove_file(from);
    Ok(())
}

/// A new file beside `to` to copy into; one that exists is never touched.
fn claim_part(to: &Path) -> io::Result<PathBuf> {
    let mut attempt = 1u32;
    loop {
        let part = to.with_extension(format!("open-powerz-part-{attempt}"));
        match File::options().write(true).create_new(true).open(&part) {
            Ok(_) => return Ok(part),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => attempt += 1,
            Err(e) => return Err(e),
        }
    }
}

/// Whether `path` is a file directly in the recordings folder `dir`: the only
/// files Save As may move.
pub fn is_recording_in(dir: &Path, path: &Path) -> bool {
    let resolved = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_owned());
    path.file_name().is_some() && path.parent().map(resolved) == Some(resolved(dir))
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
        sessions: Vec::new(),
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

/// How far a recording is along what ends it on its own.
struct StopRules {
    limits: Limits,
    /// Session time of the first row of the current low-current stretch.
    low_since_ms: Option<u64>,
    /// A limit was reached: the worker ends the recording after this poll.
    reached: Option<EndReason>,
}

impl StopRules {
    fn new(limits: Limits) -> StopRules {
        StopRules {
            limits,
            low_since_ms: None,
            reached: None,
        }
    }

    /// Take in a row just written at `time_ms`, the recording's `rows`th.
    fn row_written(&mut self, ibus_a: f64, time_ms: u64, rows: u64) {
        self.watch_current(ibus_a, time_ms);
        if self.reached.is_none() && rows >= self.limits.max_rows {
            self.reached = Some(EndReason::RowLimit);
        }
    }

    /// The auto-stop rule, on a row just written at `time_ms`.
    fn watch_current(&mut self, ibus_a: f64, time_ms: u64) {
        let Some(below_a) = self.limits.below_a else {
            return;
        };
        if ibus_a.abs() >= below_a {
            self.low_since_ms = None;
            return;
        }
        let since_ms = *self.low_since_ms.get_or_insert(time_ms);
        if time_ms.saturating_sub(since_ms) >= self.limits.for_ms {
            self.reached = Some(EndReason::LowCurrent);
        }
    }

    /// The rows broke off: nothing shows the current stayed low across that.
    fn rows_broke_off(&mut self) {
        self.low_since_ms = None;
    }
}

/// An open recording, owned by the worker thread.
pub(crate) struct Recorder {
    csv: CsvRecorder<RowBuffer>,
    start_ms: u64,
    last_flush_ms: u64,
    last_progress_ms: u64,
    gaps: Vec<Gap>,
    /// Rows are held back: the records that arrive are not written.
    paused: bool,
    /// Session time of the last row written.
    last_row_ms: Option<u64>,
    /// Where the pause gap the next row closes began. Outlives a resume, so a
    /// pause and resume with no row between them stay one gap.
    pause_from_ms: Option<u64>,
    /// What ends this recording on its own.
    stop_rules: StopRules,
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
    pub(crate) fn start(
        out: Box<dyn Write + Send>,
        now_ms: u64,
        limits: Limits,
    ) -> io::Result<Recorder> {
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
            paused: false,
            last_row_ms: None,
            pause_from_ms: None,
            stop_rules: StopRules::new(limits),
        })
    }

    /// Rows that are in the file for sure: flushed in full, header not counted.
    pub(crate) fn kept_rows(&self) -> u64 {
        self.csv.get_ref().committed_lines.saturating_sub(1)
    }

    /// Add one row, unless a pause holds it back or a limit was reached;
    /// `now_unix_ms` fixes the `Unix` origin on the first row. `temp_c` fills
    /// the `TEMP` column.
    pub(crate) fn push(
        &mut self,
        r: &QueueRecord,
        now_unix_ms: u64,
        temp_c: Option<f64>,
    ) -> io::Result<()> {
        if self.paused || self.stop_rules.reached.is_some() {
            return Ok(());
        }
        self.csv.push_with_temp(r, now_unix_ms, temp_c)?;
        let time_ms = u64::from(r.time_ms());
        self.last_row_ms = Some(time_ms);
        if let Some(from_ms) = self.pause_from_ms.take() {
            self.gaps.push(Gap {
                kind: GapKind::Pause,
                from_ms,
                to_ms: time_ms,
            });
        }
        self.stop_rules
            .row_written(r.ibus_a(), time_ms, self.csv.rows());
        Ok(())
    }

    /// The limit this recording has reached, if any.
    pub(crate) fn limit_reached(&self) -> Option<EndReason> {
        self.stop_rules.reached
    }

    /// Hold the rows back from here on. The file stays open; the gap opens at
    /// the last row, so a pause before the first row adds none.
    pub(crate) fn pause(&mut self) {
        if !self.paused {
            self.paused = true;
            self.break_run();
            self.pause_from_ms = self.pause_from_ms.or(self.last_row_ms);
        }
    }

    /// Write rows again, in the same file.
    pub(crate) fn resume(&mut self) {
        self.paused = false;
    }

    /// A pause holds the rows back.
    pub(crate) fn is_paused(&self) -> bool {
        self.paused
    }

    /// Nothing was measured up to the next row: it adds no charge or energy,
    /// and nothing shows the current stayed low across it.
    pub(crate) fn break_run(&mut self) {
        self.csv.break_run();
        self.stop_rules.rows_broke_off();
    }

    /// Remember a stream gap seen while recording. A gap that ends at the
    /// first row lies before the recording and is not counted, and one inside
    /// a pause is that pause's gap already.
    pub(crate) fn gap(&mut self, gap: Gap) {
        self.stop_rules.rows_broke_off();
        if self.csv.rows() > 0 && !self.paused && self.pause_from_ms.is_none() {
            self.gaps.push(gap);
        }
    }

    /// Rows written so far and time since the start, at `now_ms`.
    pub(crate) fn progress(&self, now_ms: u64) -> (u64, u64) {
        (self.csv.rows(), now_ms.saturating_sub(self.start_ms))
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
                paused: self.paused,
            }));
        }
        Ok(None)
    }

    /// Flush and close the file.
    pub(crate) fn finish(self, ended: EndReason) -> Result<RecordingSummary, (u64, UiError)> {
        let rows = self.csv.rows();
        // If the last flush fails, only what reached the file before it counts.
        let kept = self.kept_rows();
        match self.csv.finish() {
            Ok(_) => Ok(RecordingSummary {
                rows,
                gaps: self.gaps,
                disconnected: ended == EndReason::Disconnected,
                ended,
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
    progress: impl FnMut(ExportProgress),
) -> Result<u64, UiError> {
    export_iter(
        records.len() as u64,
        records.iter().map(|&r| (r, false, None)),
        out,
        start_unix_ms,
        progress,
    )
}

/// [`export_records`] over `total` records read from an iterator, each with
/// whether it starts a new measuring run (charge and energy then skip the
/// interval before it, as the session statistics do) and its `TEMP`.
pub fn export_iter<W: Write>(
    total: u64,
    records: impl IntoIterator<Item = (QueueRecord, bool, Option<f64>)>,
    out: W,
    start_unix_ms: u64,
    mut progress: impl FnMut(ExportProgress),
) -> Result<u64, UiError> {
    let fail = |e: io::Error| io_error(&e);
    let every = EXPORT_PROGRESS_ROWS as u64;
    let mut csv = CsvRecorder::new(BufWriter::with_capacity(1 << 16, out)).map_err(fail)?;
    for (r, new_run, temp_c) in records {
        if new_run {
            csv.break_run();
        }
        csv.push_with_temp(&r, start_unix_ms, temp_c)
            .map_err(fail)?;
        if csv.rows() % every == 0 {
            progress(ExportProgress {
                done: csv.rows(),
                total,
            });
        }
    }
    let rows = csv.rows();
    if rows % every != 0 {
        progress(ExportProgress { done: rows, total });
    }
    let buffered = csv.finish().map_err(fail)?;
    buffered.into_inner().map_err(|e| io_error(e.error()))?;
    if rows == 0 {
        progress(ExportProgress { done: 0, total: 0 });
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_move_across_volumes_leaves_a_user_file_with_the_part_name_alone() {
        let dir = std::env::temp_dir().join(format!("powerz-part-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir");
        let from = dir.join("from.csv");
        let to = dir.join("to.csv");
        let theirs = to.with_extension("open-powerz-part-1");
        fs::write(&from, "rows").expect("recording");
        fs::write(&theirs, "theirs").expect("their file");

        move_across_volumes(&from, &to).expect("moved");

        assert_eq!(fs::read_to_string(&to).expect("moved file"), "rows");
        assert_eq!(fs::read_to_string(&theirs).expect("their file"), "theirs");
        assert!(!from.exists(), "the source stays behind");
        assert!(!to.with_extension("open-powerz-part-2").exists());
        fs::remove_dir_all(&dir).expect("clean up");
    }
}
