//! The vendor `.db` layout shared by the desktop app and the web edition
//! (FR AHTZW2): the data table's name, the SQL text, the parameter row and
//! the row values. No SQLite here: the desktop runs the SQL with `rusqlite`,
//! the browser with sql.js, so both write the same file.

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
