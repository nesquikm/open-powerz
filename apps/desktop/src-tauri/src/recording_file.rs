//! Opening a recording file for a read-only view (FR AXHK2S).
//!
//! A file becomes an [`OpenedFile`]: its rows in a [`History`], so the chart
//! reads buckets as it does for a session, and its statistics. It lives
//! outside every connection, so it opens with no meter and touches none.

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use powerz_protocol::decode::QueueRecord;
use powerz_record::db::{self, DbClock, DbRows, DbSample, DbValue};
use powerz_record::opened::FileRows;
use powerz_record::reader::{CsvReader, ReadError, RowOutcome};
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, ErrorCode, Row, params_from_iter};
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
pub use powerz_record::opened::MAX_ROWS;

/// Input size of a read in bytes.
const READ_BUFFER: usize = 1 << 16;

/// Rows in one transaction of an export, which is also how often it reports.
const EXPORT_BATCH: u64 = 10_000;

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

/// Collects a file's rows into an [`OpenedFile`], by the shared row rules.
struct FileBuilder {
    history: History,
    stats: Stats,
    rules: FileRows,
}

impl FileBuilder {
    fn new(max_rows: u64) -> FileBuilder {
        FileBuilder {
            history: History::new(),
            stats: Stats::new(),
            rules: FileRows::new(max_rows),
        }
    }

    /// Take in one line of the file.
    fn take(&mut self, outcome: RowOutcome) {
        if let Some(record) = self.rules.take(outcome) {
            self.history.push(&record);
            self.stats.push(&record);
        }
    }

    /// Lines taken in so far: each one lands in exactly one of the counts.
    fn lines(&self) -> u64 {
        self.rules.lines()
    }

    /// Whether the line just taken in is one to report after.
    fn report_due(&self) -> bool {
        self.rules.report_due()
    }

    fn finish(self, path: &Path) -> Result<OpenedFile, FileError> {
        self.rules
            .check_any()
            .map_err(|m| FileError::Refused(m.to_owned()))?;
        // A recording counts no charge across a pause or a break, which the
        // rows alone cannot show: its own last values win over a sum of them.
        let mut stats = self.stats.snapshot();
        stats.charge_ah = self.rules.charge_ah().unwrap_or(stats.charge_ah);
        stats.energy_wh = self.rules.energy_wh().unwrap_or(stats.energy_wh);
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        Ok(OpenedFile {
            history: self.history,
            summary: FileSummary {
                name,
                rows: self.rules.rows(),
                skipped: self.rules.skipped(),
                left_out: self.rules.left_out(),
                stats,
            },
        })
    }
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
        .ok_or_else(|| FileError::Refused(db::NOT_A_RECORDING.to_owned()))?;
    control.check()?;

    let total = row_total(&db, &table);
    control.report(0, total)?;

    let mut rows = db
        .prepare(&db::select_rows_sql(&table))
        .map_err(|_| unreadable_db())?;
    let mut rows = rows.query([]).map_err(|_| unreadable_db())?;
    let mut opened = FileBuilder::new(max_rows);
    let mut clock = DbClock::default();
    while let Some(row) = rows.next().map_err(|_| unreadable_db())? {
        let values = db_values(row).map_err(|_| unreadable_db())?;
        opened.take(db::file_row(&values, &mut clock));
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

/// A cheap total for progress: the table's highest rowid. 0, for unknown, on
/// a table with no rowids.
fn row_total(db: &Connection, table: &str) -> u64 {
    db.query_row(&db::row_total_sql(table), [], |r| {
        r.get::<_, Option<i64>>(0)
    })
    .ok()
    .flatten()
    .and_then(|top| u64::try_from(top).ok())
    .unwrap_or(0)
}

fn no_database() -> FileError {
    FileError::Refused(db::NOT_A_DATABASE.to_owned())
}

fn unreadable_db() -> FileError {
    FileError::Refused(db::UNREADABLE_DB.to_owned())
}

/// The data table, picked by the shared rules ([`db::pick_data_table`]).
fn data_table(conn: &Connection, control: &OpenControl) -> Result<Option<String>, FileError> {
    let tables = table_names(conn).map_err(|_| no_database())?;
    db::check_table_count(&tables).map_err(|m| FileError::Refused(m.to_owned()))?;
    db::pick_data_table(&tables, |name| {
        control.check()?;
        table_columns(conn, name).map_err(|_| no_database())
    })
}

/// The file's tables, each with whether it is a plain one.
fn table_names(conn: &Connection) -> rusqlite::Result<Vec<(String, bool)>> {
    let mut tables = conn.prepare(&db::table_names_sql())?;
    tables
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

/// The column names of `table`.
fn table_columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    let mut columns = conn.prepare(db::table_columns_sql())?;
    columns.query_map([table], |r| r.get(0))?.collect()
}

/// One result row of [`db::select_rows_sql`] as the shared rules' values.
fn db_values(row: &Row<'_>) -> rusqlite::Result<Vec<DbValue>> {
    (0..db::DB_COLUMNS.len())
        .map(|i| {
            Ok(match row.get_ref(i)? {
                ValueRef::Null => DbValue::Null,
                ValueRef::Integer(n) => DbValue::Integer(n),
                ValueRef::Real(f) => DbValue::Real(f),
                ValueRef::Text(t) => match std::str::from_utf8(t) {
                    Ok(t) => DbValue::Text(t.to_owned()),
                    Err(_) => DbValue::Blob(t.to_vec()),
                },
                ValueRef::Blob(b) => DbValue::Blob(b.to_vec()),
            })
        })
        .collect()
}

/// One row of a session to export: its record, whether it starts a new
/// measuring run, and its `TEMP`.
pub type ExportRow = (QueueRecord, bool, Option<f64>);

/// The data table's name for a recording started at local `(month, day, hour,
/// minute, second)`.
pub fn db_table_name(local: (u32, u32, u32, u32, u32)) -> String {
    db::table_name(local)
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
    db.execute_batch(&db::create_sql(&table))?;
    db.execute(
        &db::insert_param_sql(&table),
        params_from_iter(db::param_values(&table, start_unix_ms, &local, rate_sps)),
    )?;

    let mut written = 0u64;
    {
        let mut insert = db.prepare(&db::insert_row_sql(&table))?;
        let mut builder = DbRows::new(start_unix_ms);
        db.execute_batch("BEGIN")?;
        for (r, new_run, temp_c) in rows {
            let row = builder.push(&DbSample {
                time_ms: r.time_ms(),
                vbus_v: r.vbus_v(),
                ibus_a: r.ibus_a(),
                dp_v: r.dp_v(),
                dm_v: r.dm_v(),
                cc1_v: r.cc1_v(),
                cc2_v: r.cc2_v(),
                temp_c,
                new_run,
            });
            let numbers = row.numbers();
            insert.execute(params_from_iter(
                std::iter::once(Value::Text(row.elapsed)).chain(
                    numbers
                        .into_iter()
                        .map(|n| n.map_or(Value::Null, Value::Real)),
                ),
            ))?;
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
