//! FR AHTZW3: what the desktop app reports for the committed recording
//! fixtures the web edition opens too (`tests/data/recording-*.{csv,db}`).
//!
//! The web test (`src/lib/web/recordingFile.web.test.ts`) asserts the same
//! numbers on the same files, so the two editions keep and skip the same
//! rows (AC-AHTZW3.2, AC-AHTZW3.3). Every file is synthetic, written in the
//! vendor layout from the research notes `[static]`.
//!
//! - `recording-ours.csv`: our recorder's layout; one sentinel row, one blank
//!   line, and one last row past the time a record holds (left out).
//! - `recording-vendor.csv`: the vendor's (a UTF-8 BOM, CRLF), with a row
//!   going backwards and a sentinel row.
//! - `recording-vendor.db`: the vendor's table pair among decoys (a table with
//!   no `_param`, one missing a column, a view, a valid table later by name);
//!   its rows include one timed by Unix only, a backwards one, a NULL and a
//!   text VBUS, a row of no line voltages, a row of some, and a VBUS out of
//!   range.

use std::path::PathBuf;

use open_powerz_desktop_lib::recording_file::{self, FileError, FileSummary, OpenControl};
use open_powerz_desktop_lib::stats::MinMaxAvg;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

fn summary(name: &str) -> FileSummary {
    match recording_file::open(&fixture(name), &mut OpenControl::silent()) {
        Ok(file) => file.summary(),
        Err(FileError::Refused(why)) => panic!("{name} refused: {why}"),
        Err(FileError::Cancelled) => panic!("{name} cancelled"),
    }
}

fn near(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() <= 1e-9, "{what}: {a} against {b}");
}

fn mma(a: Option<MinMaxAvg>, (min, max, avg): (f64, f64, f64), what: &str) {
    let a = a.unwrap_or_else(|| panic!("{what}: no value"));
    near(a.min, min, &format!("{what} min"));
    near(a.max, max, &format!("{what} max"));
    near(a.avg, avg, &format!("{what} avg"));
}

#[test]
fn our_csv_fixture_reports_its_rows_skipped_left_out_and_stats() {
    let s = summary("recording-ours.csv");
    assert_eq!(s.name, "recording-ours.csv");
    assert_eq!((s.rows, s.skipped, s.left_out), (5, 1, 1));
    assert_eq!(s.stats.samples, 5);
    assert_eq!(s.stats.elapsed_ms, 2_000);
    near(s.stats.charge_ah, 0.0004, "charge");
    near(s.stats.energy_wh, 0.002, "energy");
    mma(s.stats.vbus, (4.9, 5.2, 5.04), "vbus");
    mma(s.stats.ibus, (0.8, 1.2, 1.0), "ibus");
    mma(s.stats.power, (4.16, 6.12, 5.036), "power");
}

#[test]
fn vendor_csv_fixture_reports_its_rows_skipped_and_stats() {
    let s = summary("recording-vendor.csv");
    assert_eq!((s.rows, s.skipped, s.left_out), (4, 2, 0));
    assert_eq!(s.stats.elapsed_ms, 3_000);
    near(s.stats.charge_ah, 0.0013, "charge");
    near(s.stats.energy_wh, 0.0115, "energy");
    mma(s.stats.vbus, (8.9, 9.1, 9.0), "vbus");
    mma(s.stats.ibus, (0.0, 2.5, 1.625), "ibus");
    mma(s.stats.power, (0.0, 22.5, 14.675), "power");
}

#[test]
fn vendor_db_fixture_reports_its_rows_skipped_and_stats() {
    let s = summary("recording-vendor.db");
    assert_eq!(s.name, "recording-vendor.db");
    assert_eq!((s.rows, s.skipped, s.left_out), (5, 5, 0));
    assert_eq!(s.stats.elapsed_ms, 3_000);
    near(s.stats.charge_ah, 0.0009, "charge");
    near(s.stats.energy_wh, 0.0105, "energy");
    mma(s.stats.vbus, (11.9, 12.1, 12.0), "vbus");
    mma(s.stats.ibus, (1.0, 2.0, 1.48), "ibus");
    mma(s.stats.power, (12.0, 23.8, 17.748), "power");
}
