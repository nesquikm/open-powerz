//! The `.db` export of the web edition (FR AHTZW2): the table name, the SQL
//! text, the parameter row and the row values from `powerz_record::db`,
//! which the desktop app writes too. The page runs the SQL with sql.js.

use powerz_record::db::{self, DbSample};
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::BindError;

/// The data table's name for a recording started at this local time.
#[wasm_bindgen(js_name = dbTableName)]
pub fn db_table_name(month: u32, day: u32, hour: u32, minute: u32, second: u32) -> String {
    db::table_name((month, day, hour, minute, second))
}

/// The statements that make the data table and its `_param` table.
#[wasm_bindgen(js_name = dbCreateSql)]
pub fn db_create_sql(table: &str) -> String {
    db::create_sql(table)
}

/// The insert of the `_param` row.
#[wasm_bindgen(js_name = dbInsertParamSql)]
pub fn db_insert_param_sql(table: &str) -> String {
    db::insert_param_sql(table)
}

/// The insert of one data row.
#[wasm_bindgen(js_name = dbInsertRowSql)]
pub fn db_insert_row_sql(table: &str) -> String {
    db::insert_row_sql(table)
}

/// The `_param` row's four values; `start_unix_ms` is whole milliseconds.
#[wasm_bindgen(js_name = dbParamValues)]
pub fn db_param_values(
    table: &str,
    start_unix_ms: f64,
    local_start: &str,
    rate_sps: u32,
) -> Vec<String> {
    db::param_values(table, unix_ms(start_unix_ms), local_start, rate_sps).to_vec()
}

/// Whole non-negative milliseconds of a JS time.
fn unix_ms(ms: f64) -> u64 {
    if ms.is_finite() && ms > 0.0 {
        ms.round() as u64
    } else {
        0
    }
}

/// One data row for the insert: `ElapsedTime`, then the other ten columns
/// with `null` for NULL.
#[derive(Serialize)]
struct RowView {
    elapsed: String,
    numbers: [Option<f64>; 10],
}

/// The rows of one export (`powerz_record::db::DbRows`).
#[wasm_bindgen]
pub struct DbRows(db::DbRows);

#[wasm_bindgen]
impl DbRows {
    /// Rows of a session whose first sample was at `start_unix_ms`.
    #[wasm_bindgen(constructor)]
    pub fn new(start_unix_ms: f64) -> DbRows {
        DbRows(db::DbRows::new(unix_ms(start_unix_ms)))
    }

    /// The row of the sample at `time_ms` with `readings` = VBUS, IBUS, D+,
    /// D-, CC1, CC2: `{ elapsed, numbers }`.
    pub fn push(
        &mut self,
        time_ms: u32,
        readings: &[f64],
        temp_c: Option<f64>,
        new_run: bool,
    ) -> Result<JsValue, BindError> {
        let &[vbus_v, ibus_a, dp_v, dm_v, cc1_v, cc2_v] = readings else {
            return Err(BindError::new(
                "internal",
                format!("a row takes 6 readings, not {}", readings.len()),
            ));
        };
        let row = self.0.push(&DbSample {
            time_ms,
            vbus_v,
            ibus_a,
            dp_v,
            dm_v,
            cc1_v,
            cc2_v,
            temp_c,
            new_run,
        });
        let view = RowView {
            numbers: row.numbers(),
            elapsed: row.elapsed,
        };
        crate::decoders::to_js_view(&view)
    }
}
