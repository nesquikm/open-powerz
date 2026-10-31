//! AC-4PJRVM.6 on the desktop side, on the simulator: a delete that would
//! leave 22 or more entries is refused before the CSV backup is written, the
//! re-read mismatch says when the count matches but the entries differ, and a
//! delete-all on an empty list sends nothing. The `record_count` fault pads
//! the demo table with synthetic empty records (no samples).

mod common;

use std::path::{Path, PathBuf};

use common::*;
use open_powerz_desktop_lib::ui_error::from_session;
use open_powerz_desktop_lib::worker::{Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_protocol::session::Error;
use powerz_sim::SimClock;

type Log = std::sync::Arc<std::sync::Mutex<common::Log>>;

fn start_with(faults: FaultPlan) -> (Worker<Pipe, SimClock>, Log) {
    let clock = clock();
    let pipe = Pipe::sim(Scenario::Load5v1a, faults, &clock, PipeOpts::default());
    let log = std::sync::Arc::clone(&pipe.log);
    let (_events, sink) = capture(&clock);
    let worker = Worker::start(pipe, clock.clone(), WorkerConfig::new(Rate::Sps50, 1), sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    (worker, log)
}

fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("open-powerz-edges-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn files_in(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, Iterator::count)
}

fn writes(log: &Log) -> usize {
    log.lock().unwrap().of_type(0x48).len()
}

fn first_name(worker: &mut Worker<Pipe, SimClock>) -> String {
    let v = serde_json::to_value(worker.list_records().expect("list")).unwrap();
    v["records"][0]["name"].as_str().unwrap().to_string()
}

#[test]
fn a_delete_leaving_22_entries_is_refused_before_the_backup() {
    let (mut worker, log) = start_with(FaultPlan::new().record_count(23));
    let name = first_name(&mut worker);
    let dir = fresh_dir("refused");
    let err = worker
        .delete_records(Some(1), &name, &dir)
        .expect_err("22 entries do not fit one table write");
    assert!(err.message.contains("more records"), "{}", err.message);
    assert_eq!(files_in(&dir), 0, "no backup was written");
    assert_eq!(writes(&log), 0, "nothing was sent");
}

#[test]
fn a_delete_leaving_21_entries_still_goes_through() {
    let (mut worker, log) = start_with(FaultPlan::new().record_count(22));
    let name = first_name(&mut worker);
    let dir = fresh_dir("fits");
    worker
        .delete_records(Some(1), &name, &dir)
        .unwrap_or_else(|e| panic!("21 entries fit: {}", e.message));
    assert_eq!(files_in(&dir), 1);
    assert_eq!(writes(&log), 1);
}

#[test]
fn deleting_all_of_a_long_list_is_not_refused() {
    let (mut worker, log) = start_with(FaultPlan::new().record_count(23));
    let _ = first_name(&mut worker);
    let dir = fresh_dir("all");
    worker
        .delete_records(None, "all", &dir)
        .unwrap_or_else(|e| panic!("an empty table fits: {}", e.message));
    assert_eq!(files_in(&dir), 23);
    assert_eq!(writes(&log), 1);
}

#[test]
fn a_delete_all_on_an_empty_list_sends_nothing() {
    let (mut worker, log) = start_with(FaultPlan::new());
    let dir = fresh_dir("empty");
    let _ = first_name(&mut worker);
    worker
        .delete_records(None, "all", &dir)
        .expect("delete all");
    assert_eq!(writes(&log), 1);

    let again = fresh_dir("empty-again");
    let v = worker
        .delete_records(None, "all", &again)
        .unwrap_or_else(|e| panic!("an empty list is no error: {}", e.message));
    let v = serde_json::to_value(v).unwrap();
    assert_eq!(v["records"].as_array().map(Vec::len), Some(0), "{v}");
    assert_eq!(writes(&log), 1, "no empty-table write");
    assert_eq!(files_in(&again), 0);
}

#[test]
fn the_mismatch_message_says_when_the_count_matches_but_the_entries_differ() {
    let same = Error::RecordTableMismatch {
        expected: 2,
        actual: 2,
    };
    let e = from_session(&same, "macos");
    assert!(e.message.contains("entries differ"), "{}", e.message);

    let other = Error::RecordTableMismatch {
        expected: 2,
        actual: 3,
    };
    let e = from_session(&other, "macos");
    assert!(!e.message.contains("differ"), "{}", e.message);
    assert!(e.message.contains('3'), "the count it found: {}", e.message);
}
