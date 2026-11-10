//! FR AHTZW3: the rules that read a vendor `.db` recording, shared by the
//! desktop app (rusqlite) and the web edition (sql.js), so both keep and
//! skip the same rows (AC-AHTZW3.3). No SQLite here: the rules take the
//! answers of SQL the caller runs.
//!
//! Every value here is synthetic.
//!
//! API pinned here, in `powerz_record::db`:
//!
//! - `DB_COLUMNS: [&str; 11]`, the vendor data table's columns in the order
//!   [`db::select_rows_sql`] returns them;
//! - `table_names_sql() -> String`: `(name, is_plain_table)` rows of
//!   `sqlite_master`, the desktop's statement word for word;
//! - `table_columns_sql() -> &str`: one `name` row per column of the table
//!   bound as its one parameter;
//! - `has_vendor_columns(&[&str]) -> bool`, matching names as SQLite does
//!   (ASCII case ignored);
//! - `pick_data_table(&[(String, bool)], columns_of) -> Result<Option<String>, E>`:
//!   the first plain table by name with a `_param` table beside it and the
//!   vendor columns; `columns_of(name)` answers a table's column names, and
//!   its error ends the pick;
//! - `select_rows_sql(table) -> String`: the desktop's guarded `SELECT`;
//! - `DbValue::{Null, Integer(i64), Real(f64), Text(String), Blob(Vec<u8>)}`,
//!   one value of a result row (`Debug`, `Clone`), and `file_row(&[DbValue], &mut DbClock) ->
//!   RowOutcome` with `DbClock::default()`.

use powerz_record::db::{self, DB_COLUMNS, DbClock, DbValue};
use powerz_record::reader::{FileRow, RowOutcome};

const T: &str = "powerz_11_09_18_21_42";

fn tables(list: &[(&str, bool)]) -> Vec<(String, bool)> {
    list.iter()
        .map(|&(n, plain)| (n.to_owned(), plain))
        .collect()
}

fn vendor_columns() -> Vec<String> {
    DB_COLUMNS.iter().map(|&c| c.to_owned()).collect()
}

fn pick(list: &[(&str, bool)], columns: impl Fn(&str) -> Vec<String>) -> Option<String> {
    let r: Result<Option<String>, ()> = db::pick_data_table(&tables(list), |n| Ok(columns(n)));
    r.expect("no error from the column lookup")
}

fn real(x: f64) -> DbValue {
    DbValue::Real(x)
}

fn text(s: &str) -> DbValue {
    DbValue::Text(s.to_owned())
}

/// A data row in `DB_COLUMNS` order with the given ElapsedTime and Unix.
fn values(elapsed: DbValue, unix: DbValue, vbus: DbValue, ibus: DbValue) -> Vec<DbValue> {
    vec![
        elapsed,
        unix,
        vbus,
        ibus,
        real(0.6),
        real(0.01),
        real(1.68),
        real(0.002),
        real(25.5),
        real(0.001),
        real(0.005),
    ]
}

fn good(elapsed: &str, unix: f64) -> Vec<DbValue> {
    values(text(elapsed), real(unix), real(5.0), real(1.25))
}

fn row_of(outcome: RowOutcome) -> FileRow {
    match outcome {
        RowOutcome::Row(row) => row,
        other => panic!("expected a row, got {other:?}"),
    }
}

// ---- the data table ----

#[test]
fn the_columns_are_the_vendor_layout_in_order() {
    assert_eq!(
        DB_COLUMNS,
        [
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
            "ENERGY"
        ]
    );
}

#[test]
fn table_names_sql_is_the_desktops_statement() {
    assert_eq!(
        db::table_names_sql(),
        "SELECT name, coalesce(sql LIKE 'CREATE TABLE%', 0) FROM sqlite_master \
         WHERE type = 'table' AND typeof(name) = 'text' \
         AND octet_length(name) <= 1024 LIMIT 10001"
    );
}

#[test]
fn table_columns_sql_lists_the_bound_tables_columns() {
    let sql = db::table_columns_sql();
    assert!(sql.contains("pragma_table_info("), "{sql}");
    assert!(sql.starts_with("SELECT name FROM"), "{sql}");
}

#[test]
fn vendor_columns_match_ignoring_ascii_case_and_extra_columns() {
    assert!(db::has_vendor_columns(&DB_COLUMNS));
    let lower: Vec<String> = DB_COLUMNS.iter().map(|c| c.to_ascii_lowercase()).collect();
    let lower: Vec<&str> = lower.iter().map(String::as_str).collect();
    assert!(db::has_vendor_columns(&lower));
    let mut extra = DB_COLUMNS.to_vec();
    extra.push("PWR");
    assert!(db::has_vendor_columns(&extra));
}

#[test]
fn a_missing_column_is_no_vendor_table() {
    for missing in DB_COLUMNS {
        let rest: Vec<&str> = DB_COLUMNS.into_iter().filter(|&c| c != missing).collect();
        assert!(!db::has_vendor_columns(&rest), "without {missing}");
    }
    assert!(!db::has_vendor_columns(&[]));
}

#[test]
fn the_pick_is_the_first_table_by_name_with_a_param_table_and_the_columns() {
    let picked = pick(
        &[
            ("zzz_later", true),
            ("zzz_later_param", true),
            (T, true),
            (&format!("{T}_param"), true),
        ],
        |_| vendor_columns(),
    );
    assert_eq!(picked.as_deref(), Some(T));
}

#[test]
fn a_table_without_a_param_table_is_passed_over() {
    let picked = pick(
        &[("aaa", true), (T, true), (&format!("{T}_param"), true)],
        |_| vendor_columns(),
    );
    assert_eq!(picked.as_deref(), Some(T));
}

#[test]
fn the_param_table_matches_ignoring_ascii_case() {
    let picked = pick(&[(T, true), (&format!("{T}_PARAM"), true)], |_| {
        vendor_columns()
    });
    assert_eq!(picked.as_deref(), Some(T));
}

#[test]
fn a_table_missing_a_vendor_column_is_passed_over() {
    let picked = pick(
        &[
            ("aab", true),
            ("aab_param", true),
            (T, true),
            (&format!("{T}_param"), true),
        ],
        |name| {
            if name == "aab" {
                vec!["ElapsedTime".to_owned(), "VBUS".to_owned()]
            } else {
                vendor_columns()
            }
        },
    );
    assert_eq!(picked.as_deref(), Some(T));
}

#[test]
fn a_table_not_made_by_create_table_is_never_picked() {
    // A virtual table: in sqlite_master as a table, its statement no CREATE TABLE.
    let picked = pick(&[("aaa", false), ("aaa_param", true)], |_| vendor_columns());
    assert_eq!(picked, None);
}

#[test]
fn no_table_of_the_vendor_layout_is_none() {
    assert_eq!(pick(&[], |_| vendor_columns()), None);
    assert_eq!(
        pick(&[("notes", true), ("notes_param", true)], |_| vec![
            "a".to_owned()
        ]),
        None
    );
}

#[test]
fn an_error_of_the_column_lookup_ends_the_pick() {
    let r: Result<Option<String>, &str> =
        db::pick_data_table(&tables(&[(T, true), (&format!("{T}_param"), true)]), |_| {
            Err("cancelled")
        });
    assert_eq!(r, Err("cancelled"));
}

#[test]
fn select_rows_sql_is_the_desktops_guarded_select() {
    let numbers = DB_COLUMNS[1..]
        .iter()
        .map(|c| format!("CASE WHEN typeof(\"{c}\") IN ('real', 'integer') THEN \"{c}\" END"))
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(
        db::select_rows_sql(T),
        format!(
            "SELECT CASE WHEN typeof(\"ElapsedTime\") = 'text' AND \
             octet_length(\"ElapsedTime\") <= 64 THEN \"ElapsedTime\" END, {numbers} \
             FROM \"{T}\""
        )
    );
}

#[test]
fn select_rows_sql_quotes_the_table_name() {
    assert!(db::select_rows_sql("a\"b").ends_with(" FROM \"a\"\"b\""));
}

// ---- the rows ----

#[test]
fn a_whole_row_reads_as_its_values() {
    let mut clock = DbClock::default();
    let row = row_of(db::file_row(
        &good("0:00:01.250", 1_790_000_001.25),
        &mut clock,
    ));
    assert_eq!(row.elapsed_ms, 1_250);
    assert_eq!(row.unix, Some(1_790_000_001.25));
    assert_eq!((row.vbus_v, row.ibus_a), (5.0, 1.25));
    assert_eq!(
        (row.dp_v, row.dm_v, row.cc1_v, row.cc2_v),
        (0.6, 0.01, 1.68, 0.002)
    );
    assert_eq!(row.temp_c, Some(25.5));
    assert_eq!((row.charge_ah, row.energy_wh), (Some(0.001), Some(0.005)));
}

#[test]
fn an_integer_counts_as_a_number() {
    let mut clock = DbClock::default();
    let v = values(
        text("0:00:00.000"),
        DbValue::Integer(1_790_000_000),
        DbValue::Integer(5),
        DbValue::Integer(1),
    );
    let row = row_of(db::file_row(&v, &mut clock));
    assert_eq!((row.vbus_v, row.ibus_a), (5.0, 1.0));
    assert_eq!(row.unix, Some(1_790_000_000.0));
}

#[test]
fn a_row_without_vbus_or_ibus_is_damaged() {
    for (vbus, ibus) in [
        (DbValue::Null, real(1.0)),
        (real(5.0), DbValue::Null),
        (text("5.0"), real(1.0)),
        (DbValue::Blob(vec![1, 2]), real(1.0)),
    ] {
        let mut clock = DbClock::default();
        let v = values(text("0:00:00.000"), real(1.0), vbus.clone(), ibus.clone());
        assert_eq!(
            db::file_row(&v, &mut clock),
            RowOutcome::Damaged,
            "{vbus:?} / {ibus:?}"
        );
    }
}

#[test]
fn no_line_voltages_at_all_is_a_meter_memory_row() {
    let mut clock = DbClock::default();
    let mut v = good("0:00:00.000", 1.0);
    for value in &mut v[4..8] {
        *value = DbValue::Null;
    }
    let row = row_of(db::file_row(&v, &mut clock));
    assert_eq!(
        (row.dp_v, row.dm_v, row.cc1_v, row.cc2_v),
        (0.0, 0.0, 0.0, 0.0)
    );
}

#[test]
fn some_line_voltages_missing_is_damage() {
    let mut clock = DbClock::default();
    let mut v = good("0:00:00.000", 1.0);
    v[5] = DbValue::Null;
    assert_eq!(db::file_row(&v, &mut clock), RowOutcome::Damaged);
}

#[test]
fn a_value_out_of_what_the_meter_measures_is_damage() {
    let mut clock = DbClock::default();
    let v = values(text("0:00:00.000"), real(1.0), real(99_999.0), real(1.0));
    assert_eq!(db::file_row(&v, &mut clock), RowOutcome::Damaged);
}

#[test]
fn a_row_without_elapsed_time_takes_its_time_from_unix() {
    let mut clock = DbClock::default();
    row_of(db::file_row(
        &good("0:00:00.250", 1_790_000_000.25),
        &mut clock,
    ));
    let v = values(DbValue::Null, real(1_790_000_000.75), real(5.0), real(1.0));
    let row = row_of(db::file_row(&v, &mut clock));
    assert_eq!(row.elapsed_ms, 750);
}

#[test]
fn the_first_row_without_elapsed_time_is_at_zero() {
    let mut clock = DbClock::default();
    let v = values(DbValue::Null, real(1_790_000_000.75), real(5.0), real(1.0));
    assert_eq!(row_of(db::file_row(&v, &mut clock)).elapsed_ms, 0);
}

#[test]
fn a_row_with_neither_time_is_damaged() {
    let mut clock = DbClock::default();
    let v = values(DbValue::Null, DbValue::Null, real(5.0), real(1.0));
    assert_eq!(db::file_row(&v, &mut clock), RowOutcome::Damaged);
}

#[test]
fn a_row_going_backwards_is_damaged_and_moves_no_clock() {
    let mut clock = DbClock::default();
    row_of(db::file_row(
        &good("0:00:01.000", 1_790_000_001.0),
        &mut clock,
    ));
    assert_eq!(
        db::file_row(&good("0:00:00.500", 1_790_000_000.5), &mut clock),
        RowOutcome::Damaged
    );
    let row = row_of(db::file_row(
        &good("0:00:01.000", 1_790_000_001.0),
        &mut clock,
    ));
    assert_eq!(row.elapsed_ms, 1_000, "the same time is no step back");
}

#[test]
fn a_damaged_row_moves_no_clock() {
    let mut clock = DbClock::default();
    row_of(db::file_row(
        &good("0:00:01.000", 1_790_000_001.0),
        &mut clock,
    ));
    // Damaged (no VBUS) at a later time: the next row may still be earlier than it.
    let v = values(
        text("0:00:05.000"),
        real(1_790_000_005.0),
        DbValue::Null,
        real(1.0),
    );
    assert_eq!(db::file_row(&v, &mut clock), RowOutcome::Damaged);
    let row = row_of(db::file_row(
        &good("0:00:02.000", 1_790_000_002.0),
        &mut clock,
    ));
    assert_eq!(row.elapsed_ms, 2_000);
}

#[test]
fn an_unreadable_elapsed_time_falls_back_to_unix() {
    let mut clock = DbClock::default();
    row_of(db::file_row(
        &good("0:00:01.000", 1_790_000_001.0),
        &mut clock,
    ));
    let v = values(text("garbage"), real(1_790_000_001.5), real(5.0), real(1.0));
    assert_eq!(row_of(db::file_row(&v, &mut clock)).elapsed_ms, 1_500);
}

#[test]
fn a_short_row_is_damaged() {
    let mut clock = DbClock::default();
    let v = good("0:00:00.000", 1.0);
    assert_eq!(db::file_row(&v[..5], &mut clock), RowOutcome::Damaged);
}
