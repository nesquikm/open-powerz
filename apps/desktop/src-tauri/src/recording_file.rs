//! Opening a recording file for a read-only view (FR AXHK2S).
//!
//! A file becomes an [`OpenedFile`]: its rows in a [`History`], so the chart
//! reads buckets as it does for a session, and its statistics. It lives
//! outside every connection, so it opens with no meter and touches none.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_record::reader::{CsvReader, FileRow, ReadError, RowOutcome, parse_elapsed};
use powerz_record::{Integrator, format_elapsed};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, ErrorCode, Row, params};
use serde::Serialize;

use crate::history::{
    History, RangeStats, Records, RunProgress, RunProgressQuery, Series, SeriesBuckets,
};
use crate::pd_import;
use crate::recording::{ExportProgress, create_temp_beside, io_error};
use crate::stats::{Stats, StatsSnapshot};
use crate::ui_error::UiError;

/// The most rows a file opens with; [`FileSummary::left_out`] says how many
/// more it held.
pub const MAX_ROWS: u64 = 10_000_000;

/// Rows between two progress reports, which is also how soon Cancel is seen.
const REPORT_EVERY: u64 = 8_192;

/// Input size of a read in bytes.
const READ_BUFFER: usize = 1 << 16;

/// Highest CC or D± value the 0.1 mV scale holds, V.
const FINE_MAX_V: f64 = 6.5535;

/// The columns of a vendor data table, in the order [`select_row`] reads them.
const DB_COLUMNS: [&str; 11] = [
    "ElapsedTime",
    "Unix",
    "VBUS",
    "IBUS",
    "DP",
    "DM",
    "CC1",
    "CC2",
    "TEMP",
    "CHARGE",
    "ENERGY",
];

/// Rows in one transaction of an export, which is also how often it reports.
const EXPORT_BATCH: u64 = 10_000;

/// Longest ElapsedTime taken as one, bytes; a real one is a dozen.
const MAX_ELAPSED_BYTES: usize = 64;

/// Most tables a database may hold; a recording has two.
const MAX_TABLES: usize = 10_000;

/// Longest table name taken as one, bytes; a real one is a few dozen.
const MAX_NAME_BYTES: usize = 1_024;

/// How far an open has got, in a unit of the reader's own. A `total` of 0
/// means the end is not known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OpenProgress {
    pub done: u64,
    pub total: u64,
}

/// What steers an open from outside: raising `cancel` ends it, `progress`
/// hears how far it got.
pub struct OpenControl {
    pub cancel: Arc<AtomicBool>,
    pub progress: Box<dyn FnMut(OpenProgress) + Send>,
}

impl OpenControl {
    /// A control that nothing cancels and nothing listens to.
    pub fn silent() -> OpenControl {
        OpenControl {
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Box::new(|_| {}),
        }
    }

    /// Stop when Cancel was pressed.
    fn check(&self) -> Result<(), FileError> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(FileError::Cancelled);
        }
        Ok(())
    }

    /// Report how far the open got, then answer whether to stop.
    fn report(&mut self, done: u64, total: u64) -> Result<(), FileError> {
        (self.progress)(OpenProgress { done, total });
        self.check()
    }
}

/// Why no file came of an open.
#[derive(Debug)]
pub enum FileError {
    /// The file is no recording, in words for the user.
    Refused(String),
    /// The user pressed Cancel.
    Cancelled,
}

impl From<ReadError> for FileError {
    fn from(e: ReadError) -> FileError {
        FileError::Refused(e.to_string())
    }
}

/// What the view shows of a file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FileSummary {
    /// The file's name, without its folder.
    pub name: String,
    /// Rows opened.
    pub rows: u64,
    /// Damaged rows skipped.
    pub skipped: u64,
    /// Readable rows past the row limit, or past the time a record holds.
    pub left_out: u64,
    pub stats: StatsSnapshot,
}

/// A recording file, open for reading.
#[derive(Debug)]
pub struct OpenedFile {
    history: History,
    summary: FileSummary,
}

impl OpenedFile {
    /// What the view shows of the file.
    pub fn summary(&self) -> FileSummary {
        self.summary.clone()
    }

    /// The rows, as records with the file's elapsed time as their time.
    pub fn records(&self) -> Records {
        self.history.snapshot()
    }

    /// Min / max buckets of `series` over `t0..=t1`, at most `buckets` each.
    pub fn range(&self, series: &[Series], t0: u64, t1: u64, buckets: usize) -> Vec<SeriesBuckets> {
        self.history.range(series, t0, t1, buckets)
    }

    /// The ruler's stats over the rows with `t0 <= time <= t1` (FR AXHK2T).
    pub fn range_stats(&self, t0: u64, t1: u64) -> Option<RangeStats> {
        self.history.range_stats(t0, t1)
    }

    /// Segment marks and E.D. of the file's current run (FR AXHK2T).
    pub fn run_progress(&self, q: &RunProgressQuery) -> Option<RunProgress> {
        self.history.run_progress(q)
    }

    /// [`range`](Self::range) widened to the rows just outside each end, so a
    /// zoom between two rows of a sparse file still draws.
    pub fn range_to_edges(
        &self,
        series: &[Series],
        t0: u64,
        t1: u64,
        buckets: usize,
    ) -> Vec<SeriesBuckets> {
        let (t0, t1) = self.history.widen_to_neighbours(t0, t1);
        self.history.range(series, t0, t1, buckets)
    }
}

/// Collects a file's rows into an [`OpenedFile`].
struct FileBuilder {
    history: History,
    stats: Stats,
    /// The last CHARGE and ENERGY of the rows taken in.
    charge_ah: Option<f64>,
    energy_wh: Option<f64>,
    rows: u64,
    skipped: u64,
    left_out: u64,
    /// Blank lines, which only count towards the next report.
    blank: u64,
    max_rows: u64,
    /// A row has needed the coarse line scale: the rest of the file keeps it.
    coarse: bool,
}

impl FileBuilder {
    fn new(max_rows: u64) -> FileBuilder {
        FileBuilder {
            history: History::new(),
            stats: Stats::new(),
            charge_ah: None,
            energy_wh: None,
            rows: 0,
            skipped: 0,
            left_out: 0,
            blank: 0,
            max_rows,
            coarse: false,
        }
    }

    /// Take in one line of the file.
    fn take(&mut self, outcome: RowOutcome) {
        match outcome {
            RowOutcome::Row(row) => self.push(&row),
            RowOutcome::Damaged => self.skipped += 1,
            RowOutcome::Blank => self.blank += 1,
        }
    }

    /// Lines taken in so far: each one lands in exactly one of these counts.
    fn lines(&self) -> u64 {
        self.rows + self.skipped + self.left_out + self.blank
    }

    /// Whether the line just taken in is one to report after.
    fn report_due(&self) -> bool {
        self.lines().is_multiple_of(REPORT_EVERY)
    }

    fn push(&mut self, row: &FileRow) {
        // A time past a record's reach is readable, and so are all after it.
        let Some(time_ms) = u32::try_from(row.elapsed_ms)
            .ok()
            .filter(|_| self.rows < self.max_rows)
        else {
            self.left_out += 1;
            return;
        };
        self.coarse |= [row.cc1_v, row.cc2_v, row.dp_v, row.dm_v]
            .iter()
            .any(|&v| v > FINE_MAX_V);
        let record = record_of(row, time_ms, self.coarse);
        self.history.push(&record);
        self.stats.push(&record);
        self.charge_ah = row.charge_ah.or(self.charge_ah);
        self.energy_wh = row.energy_wh.or(self.energy_wh);
        self.rows += 1;
    }

    fn finish(self, path: &Path) -> Result<OpenedFile, FileError> {
        if self.rows == 0 {
            return Err(FileError::Refused(
                "This file holds no readable row.".to_owned(),
            ));
        }
        // A recording counts no charge across a pause or a break, which the
        // rows alone cannot show: its own last values win over a sum of them.
        let mut stats = self.stats.snapshot();
        stats.charge_ah = self.charge_ah.unwrap_or(stats.charge_ah);
        stats.energy_wh = self.energy_wh.unwrap_or(stats.energy_wh);
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        Ok(OpenedFile {
            history: self.history,
            summary: FileSummary {
                name,
                rows: self.rows,
                skipped: self.skipped,
                left_out: self.left_out,
                stats,
            },
        })
    }
}

/// `row` as a record at `time_ms`.
///
/// A record's rate sets the scale of its CC and D± codes, and a file names no
/// rate. A file starts on the fine scale and takes the coarse one at the first
/// row that needs it, so a 0.1 mV digit from 2 SPS and 7.5 V from 1000 SPS both
/// survive, and the history keeps at most one rate change however rows mix.
fn record_of(row: &FileRow, time_ms: u32, coarse: bool) -> QueueRecord {
    let lines = [row.cc1_v, row.cc2_v, row.dp_v, row.dm_v];
    let (per_volt, rate) = if coarse {
        (1_000.0, Rate::Sps1000)
    } else {
        (10_000.0, Rate::Sps2)
    };
    QueueRecord::from_raw(
        time_ms,
        (row.vbus_v * 1e6).round() as i32,
        (row.ibus_a * 1e6).round() as i32,
        lines.map(|v| (v * per_volt).round() as u16),
        rate,
    )
}

/// Open the recording at `path`, picking the reader by its extension. The
/// first report comes before anything is read: Cancel works from there on.
pub fn open(path: &Path, control: &mut OpenControl) -> Result<OpenedFile, FileError> {
    control.report(0, 0)?;
    let is_db = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("db"));
    if is_db {
        open_db(path, control)
    } else {
        open_csv(path, control)
    }
}

/// Open the CSV recording at `path`.
pub fn open_csv(path: &Path, control: &mut OpenControl) -> Result<OpenedFile, FileError> {
    open_csv_limited(path, control, MAX_ROWS)
}

/// [`open_csv`] with `max_rows` in place of [`MAX_ROWS`]. Progress counts the
/// file's bytes.
pub fn open_csv_limited(
    path: &Path,
    control: &mut OpenControl,
    max_rows: u64,
) -> Result<OpenedFile, FileError> {
    control.check()?;
    let file = File::open(path).map_err(|_| unreadable())?;
    let total = file.metadata().map_err(|_| unreadable())?.len();
    let mut reader = CsvReader::new(BufReader::with_capacity(READ_BUFFER, file))?;
    let mut opened = FileBuilder::new(max_rows);
    while let Some(outcome) = reader.next_row()? {
        opened.take(outcome);
        if opened.report_due() {
            control.report(reader.bytes_read().min(total), total)?;
        }
    }
    control.report(total, total)?;
    opened.finish(path)
}

fn unreadable() -> FileError {
    FileError::Refused("The file could not be opened.".to_owned())
}

/// Open the vendor `.db` recording at `path`.
pub fn open_db(path: &Path, control: &mut OpenControl) -> Result<OpenedFile, FileError> {
    open_db_limited(path, control, MAX_ROWS)
}

/// [`open_db`] with `max_rows` in place of [`MAX_ROWS`]. Progress counts the
/// table's rows, up to its highest rowid; a total of 0 means it has none.
///
/// The file is untrusted. It is opened immutable, so it writes nothing beside
/// the file, the only name taken from it goes into a statement quoted, and a
/// statement reads a value only once SQL has bounded it.
pub fn open_db_limited(
    path: &Path,
    control: &mut OpenControl,
    max_rows: u64,
) -> Result<OpenedFile, FileError> {
    control.check()?;
    refuse_sidecars(path)?;
    let db = pd_import::open_immutable(path).map_err(|_| no_database())?;
    let table = data_table(&db, control)?
        .ok_or_else(|| FileError::Refused("This database is not a recording.".to_owned()))?;
    let table = quoted(&table);
    control.check()?;

    let total = row_total(&db, &table);
    control.report(0, total)?;

    let mut rows = db
        .prepare(&format!("SELECT {} FROM {table}", select_row()))
        .map_err(|_| unreadable_db())?;
    let mut rows = rows.query([]).map_err(|_| unreadable_db())?;
    let mut opened = FileBuilder::new(max_rows);
    let mut clock = DbClock::default();
    while let Some(row) = rows.next().map_err(|_| unreadable_db())? {
        opened.take(file_row(row, &mut clock).map_err(|_| unreadable_db())?);
        if opened.report_due() {
            control.report(done_of(opened.lines(), total), total)?;
        }
    }
    control.report(done_of(total.max(opened.lines()), total), total)?;
    opened.finish(path)
}

/// Rows read as a report: with a total, never past it.
fn done_of(lines: u64, total: u64) -> u64 {
    if total == 0 { lines } else { lines.min(total) }
}

/// Refuse a database something else may still be writing: immutable reads
/// only the main file, which would not hold all its rows. An empty WAL is
/// what a clean close of a reader can leave.
fn refuse_sidecars(path: &Path) -> Result<(), FileError> {
    let beside = |suffix: &str| {
        let mut name = OsString::from(path);
        name.push(suffix);
        PathBuf::from(name)
    };
    let wal = std::fs::metadata(beside("-wal")).is_ok_and(|m| m.len() > 0);
    if wal || beside("-journal").exists() {
        return Err(FileError::Refused(
            "This file is still being written by another program. Close that program first."
                .to_owned(),
        ));
    }
    Ok(())
}

/// A cheap total for progress: the table's highest rowid, which SQLite finds
/// at the end of the tree. 0, for unknown, on a table with no rowids.
fn row_total(db: &Connection, table: &str) -> u64 {
    db.query_row(&format!("SELECT max(rowid) FROM {table}"), [], |r| {
        r.get::<_, Option<i64>>(0)
    })
    .ok()
    .flatten()
    .and_then(|top| u64::try_from(top).ok())
    .unwrap_or(0)
}

fn no_database() -> FileError {
    FileError::Refused("This file is not a database that can be opened.".to_owned())
}

fn unreadable_db() -> FileError {
    FileError::Refused("The database could not be read.".to_owned())
}

/// `name` as an SQL identifier.
fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// The first table by name that has a `_param` table beside it and holds the
/// vendor's columns. Views and virtual tables do not count: SQLite stores a
/// real table's statement with exactly this start. A `_param` table is matched
/// as SQLite matches names, ignoring ASCII case.
fn data_table(db: &Connection, control: &OpenControl) -> Result<Option<String>, FileError> {
    let mut tables = table_names(db).map_err(|_| no_database())?;
    if tables.len() > MAX_TABLES {
        return Err(FileError::Refused(
            "This database holds too many tables to be a recording.".to_owned(),
        ));
    }
    tables.sort();
    let names: HashSet<String> = tables
        .iter()
        .map(|(name, _)| name.to_ascii_lowercase())
        .collect();
    for (name, _) in tables.iter().filter(|(_, plain)| *plain) {
        control.check()?;
        if names.contains(&format!("{}_param", name.to_ascii_lowercase()))
            && has_vendor_columns(db, name).map_err(|_| no_database())?
        {
            return Ok(Some(name.clone()));
        }
    }
    Ok(None)
}

/// The file's tables, one past [`MAX_TABLES`] at most, each with whether it is
/// a plain one. A name past [`MAX_NAME_BYTES`] is no table of a recording.
fn table_names(db: &Connection) -> rusqlite::Result<Vec<(String, bool)>> {
    let mut tables = db.prepare(&format!(
        "SELECT name, coalesce(sql LIKE 'CREATE TABLE%', 0) FROM sqlite_master \
         WHERE type = 'table' AND typeof(name) = 'text' \
         AND octet_length(name) <= {MAX_NAME_BYTES} LIMIT {}",
        MAX_TABLES + 1
    ))?;
    tables
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

/// Whether `table` holds every vendor column. A generated column is not
/// listed, so a file cannot compute one.
fn has_vendor_columns(db: &Connection, table: &str) -> rusqlite::Result<bool> {
    let mut missing = DB_COLUMNS.to_vec();
    let mut columns = db.prepare("SELECT name FROM pragma_table_info(?1)")?;
    let mut columns = columns.query([table])?;
    while let Some(column) = columns.next()? {
        let name: String = column.get(0)?;
        missing.retain(|c| !c.eq_ignore_ascii_case(&name));
    }
    Ok(missing.is_empty())
}

/// The result columns of a data row, in [`DB_COLUMNS`] order. A value is
/// handed over only when it is short: anything but a number reads as NULL
/// here, and SQLite answers `typeof` and `octet_length` without loading it.
fn select_row() -> String {
    let [elapsed, numbers @ ..] = DB_COLUMNS.map(quoted);
    let mut columns = vec![format!(
        "CASE WHEN typeof({elapsed}) = 'text' AND octet_length({elapsed}) <= {MAX_ELAPSED_BYTES} \
         THEN {elapsed} END"
    )];
    columns.extend(
        numbers.map(|c| format!("CASE WHEN typeof({c}) IN ('real', 'integer') THEN {c} END")),
    );
    columns.join(", ")
}

/// The time of the rows read so far.
#[derive(Default)]
struct DbClock {
    /// Time of the last good row: a row before it goes backwards.
    last_ms: u64,
    /// The last good row's time and its Unix column.
    anchor: Option<(u64, f64)>,
}

impl DbClock {
    /// The time of a row: its ElapsedTime, or else the distance of its Unix
    /// value from the last good row's. `None` when it has neither, or when it
    /// goes backwards.
    fn time_of(&self, elapsed: Option<&str>, unix: Option<f64>) -> Option<u64> {
        let ms = elapsed
            .and_then(parse_elapsed)
            .or_else(|| self.since_anchor(unix?))?;
        (ms >= self.last_ms).then_some(ms)
    }

    fn since_anchor(&self, unix: f64) -> Option<u64> {
        let Some((at_ms, at_unix)) = self.anchor else {
            return Some(0);
        };
        let since_ms = ((unix - at_unix) * 1000.0).round();
        // `as` saturates, and a time past a record's reach is then damage.
        (since_ms >= 0.0).then(|| at_ms.saturating_add(since_ms as u64))
    }

    fn advance(&mut self, row: &FileRow) {
        self.last_ms = row.elapsed_ms;
        self.anchor = row.unix.map(|unix| (row.elapsed_ms, unix));
    }
}

/// One data row of [`select_row`] as a [`FileRow`], unless it is damaged.
fn file_row(row: &Row<'_>, clock: &mut DbClock) -> rusqlite::Result<RowOutcome> {
    let number = |i| pd_import::real(row, i);
    let elapsed = match row.get_ref(0)? {
        ValueRef::Text(text) => std::str::from_utf8(text).ok(),
        _ => None,
    };
    let unix = number(1)?;
    let (Some(vbus_v), Some(ibus_a)) = (number(2)?, number(3)?) else {
        return Ok(RowOutcome::Damaged);
    };
    // As in a CSV: all four empty is a meter-memory row, some of them damage.
    let lines = [number(4)?, number(5)?, number(6)?, number(7)?];
    let [dp_v, dm_v, cc1_v, cc2_v] = if lines.iter().all(Option::is_none) {
        [0.0; 4]
    } else {
        lines.map(|v| v.unwrap_or(f64::NAN))
    };
    let Some(elapsed_ms) = clock.time_of(elapsed, unix) else {
        return Ok(RowOutcome::Damaged);
    };
    let row = FileRow {
        elapsed_ms,
        unix,
        vbus_v,
        ibus_a,
        dp_v,
        dm_v,
        cc1_v,
        cc2_v,
        temp_c: number(8)?,
        charge_ah: number(9)?,
        energy_wh: number(10)?,
    };
    if !row.in_range() {
        return Ok(RowOutcome::Damaged);
    }
    clock.advance(&row);
    Ok(RowOutcome::Row(row))
}

/// One row of a session to export: its record, whether it starts a new
/// measuring run, and its `TEMP`.
pub type ExportRow = (QueueRecord, bool, Option<f64>);

/// The data table's name for a recording started at local `(month, day, hour,
/// minute, second)`.
pub fn db_table_name((month, day, hour, minute, second): (u32, u32, u32, u32, u32)) -> String {
    format!("powerz_{month:02}_{day:02}_{hour:02}_{minute:02}_{second:02}")
}

/// Write `rows` to `path` in the vendor's table layout; returns the rows
/// written.
pub fn export_db(
    rows: impl IntoIterator<Item = ExportRow, IntoIter: ExactSizeIterator>,
    path: &Path,
    start_unix_ms: u64,
    rate_sps: u32,
    progress: impl FnMut(ExportProgress),
) -> Result<u64, UiError> {
    let rows = rows.into_iter();
    export_db_iter(
        rows.len() as u64,
        rows,
        path,
        start_unix_ms,
        rate_sps,
        progress,
    )
}

/// [`export_db`] over `total` rows read from an iterator. The file is written
/// under a temporary name beside `path` and renamed at the end, so a failed
/// export leaves what was at `path` alone.
pub fn export_db_iter(
    total: u64,
    rows: impl IntoIterator<Item = ExportRow>,
    path: &Path,
    start_unix_ms: u64,
    rate_sps: u32,
    progress: impl FnMut(ExportProgress),
) -> Result<u64, UiError> {
    let (tmp, file) = create_temp_beside(path).map_err(|e| io_error(&e))?;
    let written = write_db(&tmp, total, rows, start_unix_ms, rate_sps, progress)
        .map_err(|e| db_write_error(&e))
        .and_then(|rows| file.sync_all().map(|()| rows).map_err(|e| io_error(&e)));
    drop(file);
    written
        .and_then(|rows| {
            std::fs::rename(&tmp, path)
                .map(|()| rows)
                .map_err(|e| io_error(&e))
        })
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
}

fn db_write_error(e: &rusqlite::Error) -> UiError {
    let kind = match e.sqlite_error_code() {
        Some(ErrorCode::DiskFull) => io::ErrorKind::StorageFull,
        _ => io::ErrorKind::Other,
    };
    io_error(&kind.into())
}

/// The data table's name and the local time of `start_unix_ms`, as SQLite
/// tells it: `YYYY-MM-DD HH:MM:SS`.
fn local_start(db: &Connection, start_unix_ms: u64) -> rusqlite::Result<(String, String)> {
    let local: String = db.query_row(
        "SELECT datetime(?1, 'unixepoch', 'localtime')",
        [(start_unix_ms / 1000) as i64],
        |r| r.get(0),
    )?;
    let part = |at| {
        local
            .get(at)
            .and_then(|s: &str| s.parse().ok())
            .unwrap_or(0)
    };
    let table = db_table_name((
        part(5..7),
        part(8..10),
        part(11..13),
        part(14..16),
        part(17..19),
    ));
    Ok((table, local))
}

/// Write the file at `path`, which holds nothing yet, and report progress per
/// batch. Each batch is a transaction of its own. A file that fails is thrown
/// away, so it keeps no journal to roll back with.
fn write_db(
    path: &Path,
    total: u64,
    rows: impl IntoIterator<Item = ExportRow>,
    start_unix_ms: u64,
    rate_sps: u32,
    mut progress: impl FnMut(ExportProgress),
) -> rusqlite::Result<u64> {
    let db = Connection::open(path)?;
    db.execute_batch("PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;")?;
    let (table, local) = local_start(&db, start_unix_ms)?;
    let param = quoted(&format!("{table}_param"));
    let data = quoted(&table);
    db.execute_batch(&format!(
        "CREATE TABLE {data} (ElapsedTime text, Unix real primary key, VBUS real, IBUS real, \
         DP real, DM real, CC1 real, CC2 real, TEMP real, CHARGE real, ENERGY real); \
         CREATE TABLE {param} (name text, Unix text, StartTime text, SamplingRate text);"
    ))?;
    db.execute(
        &format!("INSERT INTO {param} VALUES (?1, ?2, ?3, ?4)"),
        params![
            table,
            format!("{:.3}", start_unix_ms as f64 / 1000.0),
            local,
            rate_sps.to_string()
        ],
    )?;

    let mut written = 0u64;
    {
        let mut insert = db.prepare(&format!(
            "INSERT INTO {data} VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
        ))?;
        let mut integrator = Integrator::new();
        let mut first_ms = None;
        let mut last_unix_us = 0u64;
        db.execute_batch("BEGIN")?;
        for (r, new_run, temp_c) in rows {
            if new_run {
                integrator.break_run();
            }
            let t = r.time_ms();
            let elapsed_ms = u64::from(t.saturating_sub(*first_ms.get_or_insert(t)));
            let (charge_ah, energy_wh) = integrator.push(t, r.vbus_v(), r.ibus_a());
            // `Unix` is the primary key: rows with one value move apart.
            last_unix_us = ((start_unix_ms + elapsed_ms) * 1000).max(last_unix_us + 1);
            insert.execute(params![
                format_elapsed(elapsed_ms),
                last_unix_us as f64 / 1e6,
                r.vbus_v(),
                r.ibus_a(),
                r.dp_v(),
                r.dm_v(),
                r.cc1_v(),
                r.cc2_v(),
                temp_c,
                charge_ah,
                energy_wh,
            ])?;
            written += 1;
            if written.is_multiple_of(EXPORT_BATCH) {
                db.execute_batch("COMMIT; BEGIN")?;
                progress(ExportProgress {
                    done: written,
                    total,
                });
            }
        }
        db.execute_batch("COMMIT")?;
    }
    if !written.is_multiple_of(EXPORT_BATCH) || written == 0 {
        progress(ExportProgress {
            done: written,
            total,
        });
    }
    db.close().map_err(|(_, e)| e)?;
    Ok(written)
}
