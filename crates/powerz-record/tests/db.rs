//! FR AHTZW2: the vendor `.db` layout shared by the desktop app and the web
//! edition: the data table's name, the SQL text, the parameter row and the
//! row values, so both write the same file.
//!
//! Every value here is synthetic.

use powerz_record::db::{self, DbRow, DbRows, DbSample};
use powerz_record::{Integrator, format_elapsed};

/// Wall-clock start used by the tests, ms since the Unix epoch.
const START: u64 = 1_790_000_000_250;

fn sample(time_ms: u32, vbus_v: f64, ibus_a: f64) -> DbSample {
    DbSample {
        time_ms,
        vbus_v,
        ibus_a,
        dp_v: 0.6,
        dm_v: 0.0,
        cc1_v: 1.68,
        cc2_v: 0.0,
        temp_c: None,
        new_run: false,
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 + 1e-9 * b.abs()
}

// ---- AC-AHTZW2.2: the desktop's layout ----

#[test]
fn table_name_is_powerz_and_the_local_start_to_the_second() {
    assert_eq!(db::table_name((11, 9, 18, 21, 42)), "powerz_11_09_18_21_42");
    assert_eq!(db::table_name((1, 2, 3, 4, 5)), "powerz_01_02_03_04_05");
    assert_eq!(
        db::table_name((12, 31, 23, 59, 59)),
        "powerz_12_31_23_59_59"
    );
}

#[test]
fn create_sql_makes_the_data_table_and_its_param_table() {
    assert_eq!(
        db::create_sql("powerz_11_09_18_21_42"),
        "CREATE TABLE \"powerz_11_09_18_21_42\" (ElapsedTime text, Unix real primary key, \
         VBUS real, IBUS real, DP real, DM real, CC1 real, CC2 real, TEMP real, CHARGE real, \
         ENERGY real); \
         CREATE TABLE \"powerz_11_09_18_21_42_param\" (name text, Unix text, StartTime text, \
         SamplingRate text);"
    );
}

#[test]
fn create_sql_quotes_the_table_name() {
    let sql = db::create_sql("a\"b");
    assert!(sql.contains("CREATE TABLE \"a\"\"b\" ("), "{sql}");
    assert!(sql.contains("CREATE TABLE \"a\"\"b_param\" ("), "{sql}");
}

#[test]
fn insert_sql_has_one_placeholder_per_column() {
    assert_eq!(
        db::insert_param_sql("powerz_11_09_18_21_42"),
        "INSERT INTO \"powerz_11_09_18_21_42_param\" VALUES (?1, ?2, ?3, ?4)"
    );
    assert_eq!(
        db::insert_row_sql("powerz_11_09_18_21_42"),
        "INSERT INTO \"powerz_11_09_18_21_42\" VALUES \
         (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
    );
}

#[test]
fn param_row_is_name_unix_start_time_and_rate() {
    assert_eq!(
        db::param_values("powerz_11_09_18_21_42", START, "2026-11-09 18:21:42", 50),
        [
            "powerz_11_09_18_21_42".to_owned(),
            "1790000000.250".to_owned(),
            "2026-11-09 18:21:42".to_owned(),
            "50".to_owned(),
        ]
    );
    assert_eq!(
        db::param_values("t", 1_000, "1970-01-01 00:00:01", 1000)[1],
        "1.000"
    );
}

// ---- AC-AHTZW2.3: the rows the desktop writes ----

#[test]
fn rows_carry_elapsed_time_unix_time_and_the_readings() {
    let mut rows = DbRows::new(START);
    let mut s = sample(5_000, 5.0, 1.0);
    s.temp_c = Some(31.5);
    let first = rows.push(&s);
    assert_eq!(
        first,
        DbRow {
            elapsed: "0:00:00.000".to_owned(),
            unix: 1_790_000_000.25,
            vbus_v: 5.0,
            ibus_a: 1.0,
            dp_v: 0.6,
            dm_v: 0.0,
            cc1_v: 1.68,
            cc2_v: 0.0,
            temp_c: Some(31.5),
            charge_ah: 0.0,
            energy_wh: 0.0,
        }
    );
    let second = rows.push(&sample(5_000 + 3_723_456, 5.0, 1.0));
    assert_eq!(second.elapsed, format_elapsed(3_723_456));
    assert_eq!(second.elapsed, "1:02:03.456");
    assert!(close(second.unix, (START + 3_723_456) as f64 / 1000.0));
    assert_eq!(second.temp_c, None);
}

#[test]
fn rows_at_one_time_get_unix_times_a_microsecond_apart() {
    let mut rows = DbRows::new(START);
    let a = rows.push(&sample(0, 5.0, 1.0));
    let b = rows.push(&sample(0, 5.0, 1.0));
    let c = rows.push(&sample(0, 5.0, 1.0));
    let us = |r: &DbRow| (r.unix * 1e6).round() as u64;
    assert_eq!(us(&a), START * 1000);
    assert_eq!(us(&b), START * 1000 + 1);
    assert_eq!(us(&c), START * 1000 + 2);
    // The next row on a later millisecond is back on the clock.
    let d = rows.push(&sample(1, 5.0, 1.0));
    assert_eq!(us(&d), (START + 1) * 1000);
}

#[test]
fn charge_and_energy_are_integrated_like_the_recorder() {
    let samples = [
        sample(0, 5.0, 1.0),
        sample(100, 5.5, 2.0),
        sample(250, 9.0, -0.5),
    ];
    let mut rows = DbRows::new(START);
    let mut want = Integrator::new();
    for s in &samples {
        let got = rows.push(s);
        let (c, e) = want.push(s.time_ms, s.vbus_v, s.ibus_a);
        assert!(close(got.charge_ah, c), "{} vs {c}", got.charge_ah);
        assert!(close(got.energy_wh, e), "{} vs {e}", got.energy_wh);
    }
}

#[test]
fn a_new_measuring_run_adds_nothing_for_the_interval_before_it() {
    let mut rows = DbRows::new(START);
    rows.push(&sample(0, 5.0, 1.0));
    let before = rows.push(&sample(3_600_000, 5.0, 1.0));
    assert!(close(before.charge_ah, 1.0));
    assert!(close(before.energy_wh, 5.0));

    // An outage: the next run starts an hour later, which counts for nothing.
    let mut run = sample(7_200_000, 5.0, 1.0);
    run.new_run = true;
    let first_of_run = rows.push(&run);
    assert!(close(first_of_run.charge_ah, 1.0));
    assert!(close(first_of_run.energy_wh, 5.0));
    // Elapsed time and Unix time still count from the session's first row.
    assert_eq!(first_of_run.elapsed, "2:00:00.000");
    assert!(close(
        first_of_run.unix,
        (START + 7_200_000) as f64 / 1000.0
    ));

    let after = rows.push(&sample(10_800_000, 5.0, 1.0));
    assert!(close(after.charge_ah, 2.0));
    assert!(close(after.energy_wh, 10.0));
}

#[test]
fn values_are_in_column_order_for_the_insert() {
    let mut rows = DbRows::new(START);
    let mut s = sample(0, 5.0, 1.0);
    s.temp_c = Some(30.0);
    let r = rows.push(&s);
    assert_eq!(
        r.numbers(),
        [
            Some(1_790_000_000.25),
            Some(5.0),
            Some(1.0),
            Some(0.6),
            Some(0.0),
            Some(1.68),
            Some(0.0),
            Some(30.0),
            Some(0.0),
            Some(0.0),
        ]
    );
    let r = rows.push(&sample(10, 5.0, 1.0));
    assert_eq!(r.numbers()[7], None, "no temperature is a NULL TEMP");
}
