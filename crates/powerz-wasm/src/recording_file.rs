//! Opening a recording file in the browser (FR AHTZW3): a CSV read with the
//! desktop app's reader ([`powerz_record::reader::CsvReader`]), a vendor `.db`
//! with its table and row rules ([`powerz_record::db`]) over the answers of
//! sql.js, and both through its row rules ([`powerz_record::opened::FileRows`]),
//! so the web edition keeps, skips and leaves out the rows the desktop app
//! does. Errors carry the desktop's words.

use powerz_record::db::{self, DbClock, DbValue};
use powerz_record::opened::{FileRows, MAX_ROWS};
use powerz_record::reader::CsvReader;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::BindError;
use crate::decoders::{QueueSample, to_js};

/// The rows a recording file opens with.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenedRows {
    /// Rows kept.
    pub rows: u64,
    /// Damaged rows skipped.
    pub skipped: u64,
    /// Readable rows past the row limit, or past the time a record holds.
    pub left_out: u64,
    /// The file's own last CHARGE, Ah, when it has one.
    pub charge_ah: Option<f64>,
    /// The file's own last ENERGY, Wh, when it has one.
    pub energy_wh: Option<f64>,
    /// The kept rows, their time the file's elapsed time.
    pub samples: Vec<QueueSample>,
}

/// A refusal of a file that is no recording.
fn refused(message: impl Into<String>) -> BindError {
    BindError::new("usage", message)
}

/// The rows of the recorder CSV `bytes`, ours or the vendor's.
pub fn read_recording_csv(bytes: &[u8]) -> Result<OpenedRows, BindError> {
    let mut reader = CsvReader::new(bytes).map_err(|e| refused(e.to_string()))?;
    let mut rules = FileRows::new(MAX_ROWS);
    let mut samples = Vec::new();
    while let Some(outcome) = reader.next_row().map_err(|e| refused(e.to_string()))? {
        if let Some(record) = rules.take(outcome) {
            samples.push(QueueSample::from(&record));
        }
    }
    opened_rows(&rules, samples)
}

/// The rows `rules` kept, refusing a file with none.
fn opened_rows(rules: &FileRows, samples: Vec<QueueSample>) -> Result<OpenedRows, BindError> {
    rules.check_any().map_err(refused)?;
    Ok(OpenedRows {
        rows: rules.rows(),
        skipped: rules.skipped(),
        left_out: rules.left_out(),
        charge_ah: rules.charge_ah(),
        energy_wh: rules.energy_wh(),
        samples,
    })
}

/// [`read_recording_csv`] for JS.
#[wasm_bindgen(js_name = readRecordingCsv)]
pub fn read_recording_csv_js(bytes: &[u8]) -> Result<JsValue, BindError> {
    to_js(&read_recording_csv(bytes)?)
}

// ---- a vendor `.db`, its SQL run by sql.js ----

/// The statement listing a database's tables as `(name, plain)` rows.
#[wasm_bindgen(js_name = dbTableNamesSql)]
pub fn db_table_names_sql() -> String {
    db::table_names_sql()
}

/// The statement listing the columns of the table bound as its one parameter.
#[wasm_bindgen(js_name = dbTableColumnsSql)]
pub fn db_table_columns_sql() -> String {
    db::table_columns_sql().to_owned()
}

/// The statement reading the data table's rows.
#[wasm_bindgen(js_name = dbSelectRowsSql)]
pub fn db_select_rows_sql(table: &str) -> String {
    db::select_rows_sql(table)
}

/// The statement answering the data table's highest rowid.
#[wasm_bindgen(js_name = dbRowTotalSql)]
pub fn db_row_total_sql(table: &str) -> String {
    db::row_total_sql(table)
}

/// The data table of a database whose tables are `tables`, by the desktop's
/// rules: `None` when it holds none. A database of too many tables is
/// refused; an error of `columns_of` refuses it as no database.
pub fn pick_db_table(
    tables: &[(String, bool)],
    columns_of: impl FnMut(&str) -> Result<Vec<String>, BindError>,
) -> Result<Option<String>, BindError> {
    db::check_table_count(tables).map_err(refused)?;
    db::pick_data_table(tables, columns_of)
}

#[wasm_bindgen]
extern "C" {
    /// A table's column names, from sql.js.
    #[wasm_bindgen(typescript_type = "(table: string) => string[]")]
    pub type DbColumnsOf;
    #[wasm_bindgen(method, catch, js_name = call)]
    fn call_table(this: &DbColumnsOf, this_arg: &JsValue, table: &str) -> Result<JsValue, JsValue>;
}

/// [`pick_db_table`] for JS: `tables` are `[name, plain]` pairs.
#[wasm_bindgen(js_name = dbPickDataTable)]
pub fn db_pick_data_table_js(
    tables: JsValue,
    columns_of: &DbColumnsOf,
) -> Result<Option<String>, BindError> {
    let tables: Vec<(String, bool)> =
        serde_wasm_bindgen::from_value(tables).map_err(|_| refused(db::NOT_A_DATABASE))?;
    pick_db_table(&tables, |table| {
        columns_of
            .call_table(&JsValue::NULL, table)
            .ok()
            .and_then(|v| serde_wasm_bindgen::from_value(v).ok())
            .ok_or_else(|| refused(db::NOT_A_DATABASE))
    })
}

/// The rows of a vendor `.db` data table, taken in one result row at a time.
#[wasm_bindgen]
#[derive(Debug)]
pub struct DbRecording {
    rules: FileRows,
    clock: DbClock,
    samples: Vec<QueueSample>,
}

impl Default for DbRecording {
    fn default() -> DbRecording {
        DbRecording::new()
    }
}

#[wasm_bindgen]
impl DbRecording {
    /// Rules that keep at most the desktop's row limit.
    #[wasm_bindgen(constructor)]
    pub fn new() -> DbRecording {
        DbRecording {
            rules: FileRows::new(MAX_ROWS),
            clock: DbClock::default(),
            samples: Vec::new(),
        }
    }

    /// Take in one result row of [`db_select_rows_sql`]: its ElapsedTime when
    /// that is text, and its ten numbers, NaN for NULL (SQLite stores no
    /// NaN). True when a progress report is due.
    pub fn push(&mut self, elapsed: Option<String>, numbers: &[f64]) -> bool {
        let mut values = Vec::with_capacity(db::DB_COLUMNS.len());
        values.push(elapsed.map_or(DbValue::Null, DbValue::Text));
        values.extend(numbers.iter().map(|&n| {
            if n.is_nan() {
                DbValue::Null
            } else {
                DbValue::Real(n)
            }
        }));
        self.take(&values)
    }

    /// Lines taken in so far.
    pub fn lines(&self) -> f64 {
        self.rules.lines() as f64
    }

    /// The rows kept, as [`OpenedRows`]; a table with none is refused.
    pub fn finish(self) -> Result<JsValue, BindError> {
        to_js(&self.into_rows()?)
    }
}

impl DbRecording {
    /// Take in one result row's values.
    pub fn take(&mut self, values: &[DbValue]) -> bool {
        let outcome = db::file_row(values, &mut self.clock);
        if let Some(record) = self.rules.take(outcome) {
            self.samples.push(QueueSample::from(&record));
        }
        self.rules.report_due()
    }

    /// The rows kept; a table with none is refused.
    pub fn into_rows(self) -> Result<OpenedRows, BindError> {
        opened_rows(&self.rules, self.samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR";

    #[test]
    fn a_csv_keeps_its_rows_and_last_charge() {
        let csv = format!(
            "{HEADER}\n0:00:00.000,1,5,1,0.6,0.01,1.68,0,25,0.001,0.005,5\nbad\n\
             0:00:01.000,2,5,2,0.6,0.01,1.68,0,25,0.002,0.01,10\n"
        );
        let r = read_recording_csv(csv.as_bytes()).expect("a recording");
        assert_eq!((r.rows, r.skipped, r.left_out), (2, 1, 0));
        assert_eq!(r.charge_ah, Some(0.002));
        assert_eq!(r.samples.len(), 2);
        assert_eq!(r.samples[1].time_ms, 1000);
    }

    #[test]
    fn no_recording_is_refused_in_the_desktops_words() {
        assert_eq!(
            read_recording_csv(b"").unwrap_err().message,
            "This file is empty."
        );
        let header_only = format!("{HEADER}\n");
        let e = read_recording_csv(header_only.as_bytes()).unwrap_err();
        assert_eq!(
            (e.kind.as_str(), e.message.as_str()),
            ("usage", "This file holds no readable row.")
        );
    }

    #[test]
    fn a_db_table_keeps_the_desktops_rows() {
        let mut opened = DbRecording::new();
        let good = |elapsed: &str| {
            let mut v = vec![DbValue::Text(elapsed.to_owned())];
            v.extend([1.0, 5.0, 1.0, 0.6, 0.01, 1.68, 0.0, 25.0, 0.001, 0.005].map(DbValue::Real));
            v
        };
        opened.take(&good("0:00:01.000"));
        opened.take(&good("0:00:00.500"));
        opened.push(None, &[f64::NAN; 10]);
        let r = opened.into_rows().expect("a recording");
        assert_eq!((r.rows, r.skipped, r.left_out), (1, 2, 0));
        assert_eq!(r.samples[0].time_ms, 1000);
    }

    #[test]
    fn a_db_table_with_no_row_is_refused() {
        let e = DbRecording::new().into_rows().unwrap_err();
        assert_eq!(e.message, "This file holds no readable row.");
    }

    #[test]
    fn too_many_tables_are_refused() {
        let tables = vec![("t".to_owned(), true); db::MAX_TABLES + 1];
        let e = pick_db_table(&tables, |_| Ok(Vec::new())).unwrap_err();
        assert_eq!(e.message, db::TOO_MANY_TABLES);
    }
}
