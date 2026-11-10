//! The vendor `.db` layout shared by the desktop app and the web edition
//! (FR AHTZW2): the data table's name, the SQL text, the parameter row and
//! the row values. No SQLite here: the desktop runs the SQL with `rusqlite`,
//! the browser with sql.js, so both write the same file, and read one back
//! by the same rules (FR AHTZW3).

use crate::reader::{FileRow, RowOutcome, parse_elapsed};
use crate::{Integrator, format_elapsed};

/// The data table's name for a recording started at local `(month, day,
/// hour, minute, second)`: `powerz_MM_DD_HH_MM_SS`.
pub fn table_name((month, day, hour, minute, second): (u32, u32, u32, u32, u32)) -> String {
    format!("powerz_{month:02}_{day:02}_{hour:02}_{minute:02}_{second:02}")
}

/// `name` as an SQL identifier in double quotes.
pub fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn param_table(table: &str) -> String {
    quoted(&format!("{table}_param"))
}

/// The statements that make the data table `table` and its `_param` table.
pub fn create_sql(table: &str) -> String {
    format!(
        "CREATE TABLE {} (ElapsedTime text, Unix real primary key, VBUS real, IBUS real, \
         DP real, DM real, CC1 real, CC2 real, TEMP real, CHARGE real, ENERGY real); \
         CREATE TABLE {} (name text, Unix text, StartTime text, SamplingRate text);",
        quoted(table),
        param_table(table)
    )
}

/// The insert of the one `_param` row, its values from [`param_values`].
pub fn insert_param_sql(table: &str) -> String {
    format!("INSERT INTO {} VALUES (?1, ?2, ?3, ?4)", param_table(table))
}

/// The insert of one data row: [`DbRow::elapsed`], then [`DbRow::numbers`].
pub fn insert_row_sql(table: &str) -> String {
    format!(
        "INSERT INTO {} VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        quoted(table)
    )
}

/// The `_param` row: the table's name, the start as Unix seconds with three
/// decimals, the local start time (`YYYY-MM-DD HH:MM:SS`) and the rate.
pub fn param_values(
    table: &str,
    start_unix_ms: u64,
    local_start: &str,
    rate_sps: u32,
) -> [String; 4] {
    [
        table.to_owned(),
        format!("{}.{:03}", start_unix_ms / 1000, start_unix_ms % 1000),
        local_start.to_owned(),
        rate_sps.to_string(),
    ]
}

/// One sample to export: device time, the readings, `TEMP`, and whether it
/// starts a new measuring run (nothing is counted for the interval before).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DbSample {
    pub time_ms: u32,
    pub vbus_v: f64,
    pub ibus_a: f64,
    pub dp_v: f64,
    pub dm_v: f64,
    pub cc1_v: f64,
    pub cc2_v: f64,
    pub temp_c: Option<f64>,
    pub new_run: bool,
}

/// One data row's values, in column order.
#[derive(Debug, Clone, PartialEq)]
pub struct DbRow {
    /// `h:mm:ss.mmm` since the first row.
    pub elapsed: String,
    /// Unix seconds; rows at one time are a microsecond apart (primary key).
    pub unix: f64,
    pub vbus_v: f64,
    pub ibus_a: f64,
    pub dp_v: f64,
    pub dm_v: f64,
    pub cc1_v: f64,
    pub cc2_v: f64,
    pub temp_c: Option<f64>,
    pub charge_ah: f64,
    pub energy_wh: f64,
}

impl DbRow {
    /// The values after `ElapsedTime`, in column order; `None` is NULL.
    pub fn numbers(&self) -> [Option<f64>; 10] {
        [
            Some(self.unix),
            Some(self.vbus_v),
            Some(self.ibus_a),
            Some(self.dp_v),
            Some(self.dm_v),
            Some(self.cc1_v),
            Some(self.cc2_v),
            self.temp_c,
            Some(self.charge_ah),
            Some(self.energy_wh),
        ]
    }
}

/// Turns samples into [`DbRow`]s: elapsed time and Unix time from the first
/// sample, charge and energy integrated per measuring run.
#[derive(Debug, Clone)]
pub struct DbRows {
    start_unix_ms: u64,
    integrator: Integrator,
    first_ms: Option<u32>,
    last_unix_us: Option<u64>,
}

impl DbRows {
    /// Rows of a session whose first sample was at `start_unix_ms`.
    pub fn new(start_unix_ms: u64) -> DbRows {
        DbRows {
            start_unix_ms,
            integrator: Integrator::new(),
            first_ms: None,
            last_unix_us: None,
        }
    }

    /// The row of `s`.
    pub fn push(&mut self, s: &DbSample) -> DbRow {
        if s.new_run {
            self.integrator.break_run();
        }
        let t = s.time_ms;
        let elapsed_ms = u64::from(t.saturating_sub(*self.first_ms.get_or_insert(t)));
        let (charge_ah, energy_wh) = self.integrator.push(t, s.vbus_v, s.ibus_a);
        // `Unix` is the primary key: rows with one value move apart.
        let clock_us = (self.start_unix_ms + elapsed_ms) * 1000;
        let unix_us = self
            .last_unix_us
            .map_or(clock_us, |last| clock_us.max(last + 1));
        self.last_unix_us = Some(unix_us);
        DbRow {
            elapsed: format_elapsed(elapsed_ms),
            unix: unix_us as f64 / 1e6,
            vbus_v: s.vbus_v,
            ibus_a: s.ibus_a,
            dp_v: s.dp_v,
            dm_v: s.dm_v,
            cc1_v: s.cc1_v,
            cc2_v: s.cc2_v,
            temp_c: s.temp_c,
            charge_ah,
            energy_wh,
        }
    }
}

// ---- reading a vendor `.db` recording (FR AHTZW3) ----
//
// The desktop app runs these statements with `rusqlite`, the web edition with
// sql.js; both hand the answers to the rules below, so they pick the same
// table and keep and skip the same rows. The file is untrusted: the only name
// taken from it goes into a statement quoted, and a statement reads a value
// only once SQL has bounded it.

/// The columns of a vendor data table, in the order [`select_rows_sql`]
/// returns them.
pub const DB_COLUMNS: [&str; 11] = [
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

/// Most tables a database may hold; a recording has two.
pub const MAX_TABLES: usize = 10_000;

/// Longest table name taken as one, bytes.
pub const MAX_NAME_BYTES: usize = 1_024;

/// Longest ElapsedTime taken as one, bytes; a real one is a dozen.
pub const MAX_ELAPSED_BYTES: usize = 64;

/// The refusal of a file SQLite cannot open or list.
pub const NOT_A_DATABASE: &str = "This file is not a database that can be opened.";
/// The refusal of a database with no vendor data table.
pub const NOT_A_RECORDING: &str = "This database is not a recording.";
/// The refusal of a database with more than [`MAX_TABLES`] tables.
pub const TOO_MANY_TABLES: &str = "This database holds too many tables to be a recording.";
/// The refusal of a data table whose rows cannot be read.
pub const UNREADABLE_DB: &str = "The database could not be read.";

/// The file's tables, one past [`MAX_TABLES`] at most, as `(name, plain)`
/// rows: `plain` is whether `CREATE TABLE` made it, so views and virtual
/// tables do not count. A name past [`MAX_NAME_BYTES`] is no table of a
/// recording.
pub fn table_names_sql() -> String {
    format!(
        "SELECT name, coalesce(sql LIKE 'CREATE TABLE%', 0) FROM sqlite_master \
         WHERE type = 'table' AND typeof(name) = 'text' \
         AND octet_length(name) <= {MAX_NAME_BYTES} LIMIT {}",
        MAX_TABLES + 1
    )
}

/// One `name` row per column of the table bound as the one parameter. A
/// generated column is not listed, so a file cannot compute one.
pub fn table_columns_sql() -> &'static str {
    "SELECT name FROM pragma_table_info(?1)"
}

/// Whether `columns` hold every vendor column, matched as SQLite matches
/// names (ASCII case ignored).
pub fn has_vendor_columns(columns: &[&str]) -> bool {
    DB_COLUMNS
        .iter()
        .all(|c| columns.iter().any(|name| c.eq_ignore_ascii_case(name)))
}

/// Refuse a database listing more than [`MAX_TABLES`] tables, in words for
/// the user: [`table_names_sql`] lists one past the limit to tell.
pub fn check_table_count(tables: &[(String, bool)]) -> Result<(), &'static str> {
    if tables.len() > MAX_TABLES {
        Err(TOO_MANY_TABLES)
    } else {
        Ok(())
    }
}

/// The first plain table by name with a `_param` table beside it (matched
/// ignoring ASCII case) and the vendor columns. `columns_of(name)` answers a
/// table's column names; its error ends the pick.
pub fn pick_data_table<E>(
    tables: &[(String, bool)],
    mut columns_of: impl FnMut(&str) -> Result<Vec<String>, E>,
) -> Result<Option<String>, E> {
    let mut sorted: Vec<&(String, bool)> = tables.iter().collect();
    sorted.sort();
    let names: std::collections::HashSet<String> = tables
        .iter()
        .map(|(name, _)| name.to_ascii_lowercase())
        .collect();
    for (name, _) in sorted.into_iter().filter(|(_, plain)| *plain) {
        if !names.contains(&format!("{}_param", name.to_ascii_lowercase())) {
            continue;
        }
        let columns = columns_of(name)?;
        let columns: Vec<&str> = columns.iter().map(String::as_str).collect();
        if has_vendor_columns(&columns) {
            return Ok(Some(name.clone()));
        }
    }
    Ok(None)
}

/// The rows of the data table `table`, in [`DB_COLUMNS`] order. A value is
/// handed over only when it is short: anything but a number reads as NULL,
/// and SQLite answers `typeof` and `octet_length` without loading it.
pub fn select_rows_sql(table: &str) -> String {
    let [elapsed, numbers @ ..] = DB_COLUMNS.map(quoted);
    let mut columns = vec![format!(
        "CASE WHEN typeof({elapsed}) = 'text' AND octet_length({elapsed}) <= {MAX_ELAPSED_BYTES} \
         THEN {elapsed} END"
    )];
    columns.extend(
        numbers.map(|c| format!("CASE WHEN typeof({c}) IN ('real', 'integer') THEN {c} END")),
    );
    format!("SELECT {} FROM {}", columns.join(", "), quoted(table))
}

/// A cheap total for progress: the table's highest rowid, which SQLite finds
/// at the end of the tree.
pub fn row_total_sql(table: &str) -> String {
    format!("SELECT max(rowid) FROM {}", quoted(table))
}

/// One value of a result row.
#[derive(Debug, Clone, PartialEq)]
pub enum DbValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl DbValue {
    /// The value as a number: only an integer or a real is one.
    fn number(&self) -> Option<f64> {
        match *self {
            DbValue::Integer(n) => Some(n as f64),
            DbValue::Real(f) => Some(f),
            _ => None,
        }
    }
}

/// The time of the rows read so far.
#[derive(Debug, Clone, Default)]
pub struct DbClock {
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

/// One result row of [`select_rows_sql`] as a [`FileRow`], unless it is
/// damaged.
pub fn file_row(values: &[DbValue], clock: &mut DbClock) -> RowOutcome {
    let Some(values) = values.get(..DB_COLUMNS.len()) else {
        return RowOutcome::Damaged;
    };
    let number = |i: usize| values[i].number();
    let elapsed = match &values[0] {
        DbValue::Text(text) => Some(text.as_str()),
        _ => None,
    };
    let unix = number(1);
    let (Some(vbus_v), Some(ibus_a)) = (number(2), number(3)) else {
        return RowOutcome::Damaged;
    };
    // As in a CSV: all four empty is a meter-memory row, some of them damage.
    let lines = [number(4), number(5), number(6), number(7)];
    let [dp_v, dm_v, cc1_v, cc2_v] = if lines.iter().all(Option::is_none) {
        [0.0; 4]
    } else {
        lines.map(|v| v.unwrap_or(f64::NAN))
    };
    let Some(elapsed_ms) = clock.time_of(elapsed, unix) else {
        return RowOutcome::Damaged;
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
        temp_c: number(8),
        charge_ah: number(9),
        energy_wh: number(10),
    };
    if !row.in_range() {
        return RowOutcome::Damaged;
    }
    clock.advance(&row);
    RowOutcome::Row(row)
}
