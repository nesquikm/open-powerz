//! Exporting a session whose history has folded (FR AXHK2W, AC-AXHK2W.14).
//! Every identifier is synthetic: the simulator's made-up serial and UID.
//!
//! Once the history folds its oldest part (AC-AXHK2W.10), an export can hold
//! only the full-resolution part. Each row must keep its own record's
//! temperature, the row count (and the progress total) must match the rows
//! written, and the app must say how much was folded and left out and the
//! time the file starts at. The file keeps the plain recorder layout.
//!
//! API pinned here, beyond the FR's Technical Design:
//!
//! - `SessionStore::with_full_limit(n) -> SessionStore`: an empty session
//!   whose history is `History::with_full_limit(n)` (tests fold quickly);
//! - `worker::ExportSummary { rows: u64, folded: Option<ExportFolded> }`
//!   (Debug, Clone, Copy, PartialEq): what an export job joins to;
//!   `worker::ExportJob = JoinHandle<Result<ExportSummary, UiError>>`, so
//!   `Worker::export`, `Worker::export_to`, `Worker::export_db_to` and the
//!   `Exporter` methods all yield it;
//! - `worker::ExportFolded { samples: u64, ms: u64, start_unix_ms: u64 }`
//!   (Debug, Clone, Copy, PartialEq, `serde::Serialize`, fields as named):
//!   the folded records left out, the session time they span (from the
//!   history's first record to the first exported one), and the wall-clock
//!   time of the file's first row. `folded` is `None` when nothing was left
//!   out;
//! - the Tauri `ExportResult` (crate root) gains `folded: Option<ExportFolded>`,
//!   serialized as `folded` (`null` when nothing was left out).

mod common;

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use common::*;
use open_powerz_desktop_lib::ExportResult;
use open_powerz_desktop_lib::recording::ExportProgress;
use open_powerz_desktop_lib::recording_file::{self, OpenControl};
use open_powerz_desktop_lib::store::SessionStore;
use open_powerz_desktop_lib::worker::{ExportFolded, ExportSummary, Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_record::CSV_HEADER;

/// The history's full-resolution limit in these tests.
const FULL: usize = 2_000;
/// Samples per history chunk (the unit that folds).
const CHUNK: usize = 65_536;
/// Long enough at 1000 SPS to fold at least one whole chunk.
const RUN_MS: u64 = 80_000;

struct ChannelWriter(mpsc::Sender<Vec<u8>>);

impl std::io::Write for ChannelWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let _ = self.0.send(data.to_vec());
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The lines of a CSV written through a [`ChannelWriter`].
fn lines(rx: &mpsc::Receiver<Vec<u8>>) -> Vec<String> {
    let bytes: Vec<u8> = rx.try_iter().flatten().collect();
    String::from_utf8(bytes)
        .expect("utf-8 CSV")
        .lines()
        .map(str::to_owned)
        .collect()
}

fn cols(line: &str) -> Vec<&str> {
    line.split(',').collect()
}

/// The `Unix` column of a row, in ms.
fn unix_ms(line: &str) -> u64 {
    let s: f64 = cols(line)[1].parse().expect("a Unix time");
    (s * 1000.0).round() as u64
}

/// A session of `run_ms` at 1000 SPS on a store with full limit `full`,
/// recorded whole; the worker is left with the stream stopped right after
/// the recording ends, so the history holds exactly the recorded rows.
fn session(
    full: usize,
    run_ms: u64,
) -> (Worker<Pipe, SimClock>, SimClock, SessionStore, Vec<String>) {
    let clock = clock();
    let pipe = Pipe::sim(
        Scenario::Load5v1a,
        FaultPlan::new(),
        &clock,
        PipeOpts::default(),
    );
    let store = SessionStore::with_full_limit(full);
    let (_events, sink) = capture(&clock);
    let mut worker = Worker::start_with_store(
        pipe,
        clock.clone(),
        WorkerConfig::new(Rate::Sps1000, 1),
        store.clone(),
        sink,
    )
    .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    let (tx, rx) = mpsc::channel();
    worker
        .start_recording(Box::new(ChannelWriter(tx)))
        .expect("recording starts");
    run_for(&mut worker, &clock, run_ms);
    worker.stop_recording().expect("recording stops");
    let recorded = lines(&rx);
    (worker, clock, store, recorded)
}

type Progress = Arc<Mutex<Vec<ExportProgress>>>;

fn progress_sink() -> (Progress, impl FnMut(ExportProgress) + Send + 'static) {
    let seen: Progress = Arc::default();
    let s = Arc::clone(&seen);
    (seen, move |p| s.lock().unwrap().push(p))
}

#[test]
fn an_export_of_a_folded_session_writes_its_full_resolution_part_with_each_rows_own_temperature() {
    let (mut worker, _clock, store, recorded) = session(FULL, RUN_MS);
    assert_eq!(recorded[0], CSV_HEADER, "the recording's header");
    let rec_rows = &recorded[1..];
    assert!(
        rec_rows.len() > FULL + CHUNK,
        "precondition: {} records cannot fold a chunk",
        rec_rows.len()
    );
    assert_eq!(store.history_len(), rec_rows.len(), "history vs recording");
    assert!(
        store.history_fold_boundary().is_some(),
        "precondition: the history did not fold"
    );
    assert!(
        rec_rows.iter().any(|r| !cols(r)[8].is_empty()),
        "precondition: the recording holds no temperatures"
    );

    let (tx, rx) = mpsc::channel();
    let (progress, on_progress) = progress_sink();
    let summary: ExportSummary = worker
        .export(Box::new(ChannelWriter(tx)), on_progress)
        .join()
        .expect("export thread")
        .expect("export");
    let exported = lines(&rx);

    // The plain recorder layout.
    assert_eq!(exported[0], CSV_HEADER, "the export's header");
    let ex_rows = &exported[1..];
    assert!(
        ex_rows
            .iter()
            .all(|r| cols(r).len() == cols(CSV_HEADER).len()),
        "a row does not have the recorder's columns"
    );
    assert_eq!(
        cols(&ex_rows[0])[0],
        cols(&rec_rows[0])[0],
        "the file's elapsed time does not start at zero"
    );

    // Only the full-resolution part, and the count matches what was written.
    assert_eq!(summary.rows, ex_rows.len() as u64, "rows vs rows written");
    assert!(
        ex_rows.len() >= FULL && ex_rows.len() < rec_rows.len(),
        "exported {} of {} rows (full limit {FULL})",
        ex_rows.len(),
        rec_rows.len()
    );
    let last = *progress.lock().unwrap().last().expect("progress events");
    assert_eq!(
        (last.done, last.total),
        (summary.rows, summary.rows),
        "the progress total counts rows that were never written"
    );

    // The notice: what was folded and left out, and where the file starts.
    let folded: ExportFolded = summary.folded.expect("the export says what was folded");
    let skip = folded.samples as usize;
    assert_eq!(
        skip + ex_rows.len(),
        rec_rows.len(),
        "folded + written != the session's records"
    );
    assert_eq!(
        folded.ms,
        unix_ms(&rec_rows[skip]) - unix_ms(&rec_rows[0]),
        "the folded span"
    );
    assert_eq!(
        folded.start_unix_ms,
        unix_ms(&ex_rows[0]),
        "the file's start time"
    );

    // Each row is the same record as the recording's, with its own temperature.
    for (k, (ex, rec)) in ex_rows.iter().zip(&rec_rows[skip..]).enumerate() {
        let (e, r) = (cols(ex), cols(rec));
        assert_eq!(e[1], r[1], "row {k}: Unix time");
        assert_eq!(e[2..8], r[2..8], "row {k}: V, I, D+, D-, CC1, CC2");
        assert_eq!(e[8], r[8], "row {k}: temperature");
        assert_eq!(e[11], r[11], "row {k}: power");
    }
}

#[test]
fn a_db_export_of_a_folded_session_counts_and_reports_the_same() {
    let (mut worker, _clock, _store, recorded) = session(FULL, RUN_MS);
    let rec_rows = recorded.len() - 1;
    let dir = std::env::temp_dir().join(format!("powerz-export-fold-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("powerz-export-fold.db");
    let _ = std::fs::remove_file(&path);

    let (progress, on_progress) = progress_sink();
    let summary = worker
        .export_db_to(&path, Box::new(on_progress))
        .expect("the export starts")
        .join()
        .expect("export thread")
        .expect("export");
    let folded = summary.folded.expect("the .db export says what was folded");
    assert_eq!(summary.rows + folded.samples, rec_rows as u64);
    assert!(summary.rows >= FULL as u64 && (summary.rows as usize) < rec_rows);
    let last = *progress.lock().unwrap().last().expect("progress events");
    assert_eq!((last.done, last.total), (summary.rows, summary.rows));

    let opened = recording_file::open_db(&path, &mut OpenControl::silent()).expect("opens");
    assert_eq!(opened.summary().rows, summary.rows, "rows in the file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_export_of_an_unfolded_session_reports_nothing_folded() {
    let (mut worker, _clock, store, recorded) = session(FULL * 100, 3_000);
    assert_eq!(store.history_fold_boundary(), None);
    let (tx, rx) = mpsc::channel();
    let summary = worker
        .export(Box::new(ChannelWriter(tx)), |_| {})
        .join()
        .expect("export thread")
        .expect("export");
    assert_eq!(summary.folded, None);
    assert_eq!(summary.rows, (recorded.len() - 1) as u64);
    assert_eq!(lines(&rx), recorded, "an unfolded export is the recording");
}

#[test]
fn the_export_result_carries_the_folded_notice_to_the_page() {
    let folded = ExportFolded {
        samples: 65_536,
        ms: 65_536,
        start_unix_ms: T0 + 65_536,
    };
    let with = ExportResult {
        path: "/synthetic/powerz-export.csv".into(),
        rows: 2_500,
        folded: Some(folded),
    };
    assert_eq!(
        serde_json::to_value(&with).unwrap(),
        serde_json::json!({
            "path": "/synthetic/powerz-export.csv",
            "rows": 2_500,
            "folded": { "samples": 65_536, "ms": 65_536, "start_unix_ms": T0 + 65_536 },
        })
    );
    let without = ExportResult {
        path: "/synthetic/powerz-export.csv".into(),
        rows: 10,
        folded: None,
    };
    assert_eq!(
        serde_json::to_value(&without).unwrap()["folded"],
        serde_json::Value::Null
    );
}
