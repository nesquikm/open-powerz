//! FR AHTZW2 (AC-AHTZW2.4): the desktop app's `.db` reader opens a file the
//! web edition wrote and reads every row.
//!
//! - `tests/data/web-export.db` is a session exported by the browser's writer
//!   (sql.js) over the demo meter; synthetic, no serial number in it.
//! - A file written as the browser writes it: only the shared SQL text and row
//!   values from `powerz_record::db`, executed statement by statement.

use std::path::{Path, PathBuf};

use open_powerz_desktop_lib::recording_file::{self, FileError, OpenControl, OpenedFile};
use powerz_record::db::{self, DbRows, DbSample};
use rusqlite::{Connection, params_from_iter, types::Value};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/web-export.db")
}

fn opened(result: Result<OpenedFile, FileError>) -> OpenedFile {
    match result {
        Ok(file) => file,
        Err(FileError::Refused(why)) => panic!("refused: {why}"),
        Err(FileError::Cancelled) => panic!("cancelled"),
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("open-powerz-web-db-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// The rows of the file's data table, counted with SQLite.
fn row_count(path: &Path) -> u64 {
    let db = Connection::open(path).expect("database");
    let table: String = db
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE '%_param'",
            [],
            |r| r.get(0),
        )
        .expect("data table");
    db.query_row(
        &format!("SELECT count(*) FROM \"{}\"", table.replace('"', "\"\"")),
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n as u64)
    .expect("count")
}

#[test]
fn the_browser_written_fixture_opens_with_every_row() {
    let path = fixture();
    assert!(path.exists(), "{} is missing", path.display());
    let total = row_count(&path);
    assert!(total > 0);

    let file = opened(recording_file::open_db(&path, &mut OpenControl::silent()));
    let s = file.summary();
    assert_eq!(s.name, "web-export.db");
    assert_eq!((s.rows, s.skipped, s.left_out), (total, 0, 0));
    let records = file.records();
    assert_eq!(records.len() as u64, total);
    // The demo meter draws about 5 V / 1 A.
    let last = records.iter().last().expect("a row");
    assert!((last.vbus_v() - 5.0).abs() < 1.0, "VBUS {}", last.vbus_v());
    assert!((last.ibus_a() - 1.0).abs() < 0.5, "IBUS {}", last.ibus_a());
}

#[test]
fn a_file_written_from_the_shared_sql_and_rows_opens_with_every_row() {
    let dir = scratch("shared");
    let path = dir.join("browser.db");
    let table = db::table_name((11, 9, 18, 21, 42));
    let start_ms = 1_790_000_000_250;
    let rows: Vec<_> = {
        let mut b = DbRows::new(start_ms);
        (0..500u32)
            .map(|k| {
                b.push(&DbSample {
                    time_ms: k * 20,
                    vbus_v: 5.0 + f64::from(k % 7) * 0.01,
                    ibus_a: 1.0,
                    dp_v: 0.6,
                    dm_v: 0.0,
                    cc1_v: 1.68,
                    cc2_v: 0.0,
                    temp_c: (k % 2 == 0).then_some(30.0),
                    new_run: k == 250,
                })
            })
            .collect()
    };
    {
        let conn = Connection::open(&path).expect("database");
        conn.execute_batch(&db::create_sql(&table)).expect("tables");
        conn.execute(
            &db::insert_param_sql(&table),
            params_from_iter(db::param_values(
                &table,
                start_ms,
                "2026-09-21 18:21:42",
                50,
            )),
        )
        .expect("param row");
        let mut insert = conn.prepare(&db::insert_row_sql(&table)).expect("insert");
        for r in &rows {
            let mut values = vec![Value::Text(r.elapsed.clone())];
            values.extend(
                r.numbers()
                    .into_iter()
                    .map(|n| n.map_or(Value::Null, Value::Real)),
            );
            insert.execute(params_from_iter(values)).expect("row");
        }
    }

    let file = opened(recording_file::open_db(&path, &mut OpenControl::silent()));
    let s = file.summary();
    assert_eq!((s.rows, s.skipped, s.left_out), (500, 0, 0));
    let times: Vec<u32> = file.records().iter().map(|r| r.time_ms()).collect();
    assert_eq!(times, (0..500u32).map(|k| k * 20).collect::<Vec<_>>());
    let _ = std::fs::remove_dir_all(&dir);
}
