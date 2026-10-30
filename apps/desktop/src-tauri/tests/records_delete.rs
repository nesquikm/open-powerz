//! Records view delete (FR JKWT3V, AC .2, .4, .5 and .6): the worker checks
//! the typed confirmation, backs the affected records up to CSV files in
//! the given folder (the existing download), sends one SET_DATA 0x48 with
//! attribute 0x28 through the guard, reads the list again and compares it
//! with the expected table. A failed backup sends no write; a mismatching
//! re-read is an error naming the backup folder and is not retried.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use open_powerz_desktop_lib::worker::{Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_sim::SimClock;

type Log = std::sync::Arc<std::sync::Mutex<common::Log>>;

fn start_with(faults: FaultPlan) -> (Worker<Pipe, SimClock>, SimClock, Log) {
    let clock = clock();
    let pipe = Pipe::sim(Scenario::Load5v1a, faults, &clock, PipeOpts::default());
    let log = std::sync::Arc::clone(&pipe.log);
    let (_events, sink) = capture(&clock);
    let worker = Worker::start(pipe, clock.clone(), WorkerConfig::new(Rate::Sps50, 1), sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    (worker, clock, log)
}

fn names(worker: &mut Worker<Pipe, SimClock>) -> Vec<String> {
    let v = serde_json::to_value(worker.list_records().expect("list")).unwrap();
    v["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect()
}

fn fresh_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("open-powerz-delete-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|it| {
            it.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn set_data_frames(log: &Log) -> Vec<Vec<u8>> {
    log.lock()
        .unwrap()
        .of_type(0x48)
        .into_iter()
        .map(|(_, f)| f)
        .collect()
}

/// Attribute of the extended section header of a SET_DATA frame.
fn set_data_att(f: &[u8]) -> u16 {
    (u32::from_le_bytes([f[4], f[5], f[6], f[7]]) & 0x7FFF) as u16
}

#[test]
fn delete_one_backs_it_up_rewrites_the_table_and_returns_the_new_list() {
    let (mut worker, clock, log) = start_with(FaultPlan::new());
    run_for(&mut worker, &clock, 500);
    let before = names(&mut worker);
    assert!(before.len() >= 3, "{before:?}");
    let dir = fresh_dir("one");

    let view = worker
        .delete_records(Some(2), &before[1], &dir)
        .expect("delete failed");
    let v = serde_json::to_value(&view).unwrap();
    let after: Vec<String> = v["records"]
        .as_array()
        .unwrap_or_else(|| panic!("{v}"))
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect();
    let mut want = before.clone();
    want.remove(1);
    assert_eq!(after, want);
    assert_eq!(
        v["backup_dir"].as_str(),
        Some(dir.display().to_string().as_str())
    );
    let backups = v["backups"].as_array().unwrap_or_else(|| panic!("{v}"));
    assert_eq!(backups.len(), 1, "{v}");
    let csv = std::fs::read_to_string(backups[0].as_str().unwrap()).unwrap();
    assert!(csv.starts_with("ElapsedTime,"), "{csv}");
    assert_eq!(files_in(&dir).len(), 1);

    // One write, attribute 0x28, through the guard.
    let writes = set_data_frames(&log);
    assert_eq!(writes.len(), 1);
    assert_eq!(set_data_att(&writes[0]), 0x28);
    // The meter keeps the new table and the stream goes on.
    assert_eq!(names(&mut worker), want);
    assert_eq!(run_for(&mut worker, &clock, 500), worker_continue());
    worker.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

fn worker_continue() -> open_powerz_desktop_lib::worker::Step {
    open_powerz_desktop_lib::worker::Step::Continue
}

#[test]
fn delete_all_needs_all_typed_and_leaves_an_empty_list() {
    let (mut worker, clock, log) = start_with(FaultPlan::new());
    run_for(&mut worker, &clock, 300);
    let before = names(&mut worker);
    let dir = fresh_dir("all");
    let view = worker
        .delete_records(None, "all", &dir)
        .expect("delete all failed");
    let v = serde_json::to_value(&view).unwrap();
    assert_eq!(v["records"].as_array().map(Vec::len), Some(0), "{v}");
    assert_eq!(
        v["backups"].as_array().map(Vec::len),
        Some(before.len()),
        "{v}"
    );
    assert_eq!(files_in(&dir).len(), before.len());
    assert!(names(&mut worker).is_empty());
    assert_eq!(set_data_frames(&log).len(), 1);
    worker.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_wrong_confirmation_deletes_nothing_and_writes_no_backup() {
    let (mut worker, clock, log) = start_with(FaultPlan::new());
    run_for(&mut worker, &clock, 300);
    let before = names(&mut worker);
    let dir = fresh_dir("wrong");
    for (target, typed) in [
        (Some(1), before[1].as_str()),
        (Some(1), ""),
        (Some(1), "all"),
        (None, before[0].as_str()),
        (None, "ALL "),
    ] {
        assert!(
            worker.delete_records(target, typed, &dir).is_err(),
            "{target:?} {typed:?}"
        );
    }
    assert!(set_data_frames(&log).is_empty());
    assert!(files_in(&dir).is_empty());
    assert_eq!(names(&mut worker), before);
    worker.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_of_an_unknown_index_is_an_error_and_sends_no_write() {
    let (mut worker, clock, log) = start_with(FaultPlan::new());
    run_for(&mut worker, &clock, 300);
    let n = names(&mut worker).len();
    let dir = fresh_dir("range");
    for index in [0, n + 1] {
        assert!(worker.delete_records(Some(index), "X", &dir).is_err());
    }
    assert!(set_data_frames(&log).is_empty());
    worker.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_backup_sends_no_write() {
    let (mut worker, clock, log) = start_with(FaultPlan::new().reject_record_reads());
    run_for(&mut worker, &clock, 300);
    let before = names(&mut worker);
    let dir = fresh_dir("backupfail");
    let err = worker
        .delete_records(Some(1), &before[0], &dir)
        .expect_err("backup failed");
    let e = serde_json::to_value(&err).unwrap();
    assert!(
        e["message"]
            .as_str()
            .is_some_and(|m| m.to_lowercase().contains("backup")),
        "{e}"
    );
    assert!(set_data_frames(&log).is_empty());
    assert_eq!(names(&mut worker), before);
    worker.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_backup_folder_that_cannot_be_written_sends_no_write() {
    let (mut worker, clock, log) = start_with(FaultPlan::new());
    run_for(&mut worker, &clock, 300);
    let before = names(&mut worker);
    let dir = fresh_dir("nodir");
    // A file where the folder should be.
    let blocked = dir.join("not-a-dir");
    std::fs::write(&blocked, b"x").unwrap();
    assert!(
        worker
            .delete_records(Some(1), &before[0], &blocked)
            .is_err()
    );
    assert!(set_data_frames(&log).is_empty());
    worker.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_mismatching_re_read_is_an_error_naming_the_backup_and_is_not_retried() {
    let (mut worker, clock, log) = start_with(FaultPlan::new().ignore_table_write());
    run_for(&mut worker, &clock, 300);
    let before = names(&mut worker);
    let dir = fresh_dir("mismatch");
    let err = worker
        .delete_records(Some(1), &before[0], &dir)
        .expect_err("mismatch");
    let e = serde_json::to_value(&err).unwrap();
    let text = format!(
        "{} {}",
        e["message"].as_str().unwrap_or(""),
        e["hint"].as_str().unwrap_or("")
    );
    assert!(
        text.contains(&dir.display().to_string()),
        "names the backup: {e}"
    );
    assert_eq!(set_data_frames(&log).len(), 1, "not retried");
    assert_eq!(files_in(&dir).len(), 1, "the backup is kept");
    worker.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_delete_session_stays_inside_the_safe_set_plus_the_one_table_write() {
    let (mut worker, clock, log) = start_with(FaultPlan::new());
    run_for(&mut worker, &clock, 300);
    let before = names(&mut worker);
    let dir = fresh_dir("safe");
    worker
        .delete_records(Some(1), &before[0], &dir)
        .expect("delete");
    run_for(&mut worker, &clock, 200);
    worker.stop();
    let log = log.lock().unwrap();
    for (_, f) in &log.frames {
        let ty = f[0] & 0x7F;
        if ty == 0x48 {
            assert_eq!(set_data_att(f), 0x28, "{f:02x?}");
        } else {
            assert!(SAFE_TYPES.contains(&ty), "outside the safe set: {f:02x?}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_without_a_session_is_an_error() {
    let (mut worker, clock, log) = start_with(FaultPlan::new());
    run_for(&mut worker, &clock, 200);
    worker.stop();
    let dir = fresh_dir("nosession");
    assert!(worker.delete_records(Some(1), "PHONE_5V", &dir).is_err());
    assert!(set_data_frames(&log).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn records_delete_command_is_registered() {
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    let handler = lib
        .split("generate_handler![")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .expect("generate_handler! list");
    assert!(
        handler.split(',').any(|n| n.trim() == "records_delete"),
        "records_delete is not in the invoke handler"
    );
    assert!(
        lib.contains("async fn records_delete("),
        "no records_delete command"
    );
}
