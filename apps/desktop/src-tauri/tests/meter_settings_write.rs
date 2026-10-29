//! Meter settings panel writes (FR JKWT3W, AC .3, .4, .5 and .6): the
//! worker takes the panel's changes by field key, sends one type-0x0A
//! settings write through the guard, reads the block again and hands the UI
//! the rows as read back. Unknown or read-only keys and values out of range
//! are usage errors that send no write; a re-read that does not match is an
//! error naming the field and is not retried.

mod common;

use std::collections::BTreeMap;

use common::*;
use open_powerz_desktop_lib::worker::{Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_sim::SimClock;
use serde_json::Value;

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

fn changes(pairs: &[(&str, i32)]) -> BTreeMap<String, i32> {
    pairs.iter().map(|&(k, v)| (k.to_string(), v)).collect()
}

fn settings_writes(log: &Log) -> Vec<Vec<u8>> {
    log.lock()
        .unwrap()
        .of_type(0x0A)
        .into_iter()
        .map(|(_, f)| f)
        .collect()
}

fn value_of(view: &Value, key: &str) -> String {
    view["fields"]
        .as_array()
        .unwrap_or_else(|| panic!("{view}"))
        .iter()
        .find(|f| f["key"] == key)
        .and_then(|f| f["value"].as_str())
        .unwrap_or_else(|| panic!("no {key} in {view}"))
        .to_string()
}

/// `(key, value)` entries of a type-0x0A frame.
fn entries(frame: &[u8]) -> Vec<(u16, i32)> {
    frame[4..]
        .chunks(8)
        .map(|e| {
            let key = u32::from_le_bytes([e[0], e[1], e[2], e[3]]) & 0x7FFF;
            (key as u16, i32::from_le_bytes([e[4], e[5], e[6], e[7]]))
        })
        .collect()
}

#[test]
fn write_settings_sends_one_write_and_returns_the_rows_read_back() {
    let (mut worker, clock, log) = start_with(FaultPlan::new());
    run_for(&mut worker, &clock, 500);
    let view = worker
        .write_settings(&changes(&[("brightness", 80), ("theme", 2)]))
        .expect("write failed");
    let v = serde_json::to_value(&view).unwrap();
    assert_eq!(value_of(&v, "brightness"), "80%");
    assert_eq!(value_of(&v, "theme"), "pink");
    assert_eq!(value_of(&v, "language"), "English", "unchanged");
    let writes = settings_writes(&log);
    assert_eq!(writes.len(), 1, "{writes:02x?}");
    let e = entries(&writes[0]);
    assert_eq!(
        e.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
        vec![0x01, 0x03, 0x04, 0x06, 0x07, 0x14, 0x15, 0x7FFF]
    );
    assert!(e.contains(&(0x03, 80)) && e.contains(&(0x07, 2)), "{e:?}");
    // A later plain read shows the change too, and the stream goes on.
    let again = serde_json::to_value(worker.read_settings().unwrap()).unwrap();
    assert_eq!(value_of(&again, "brightness"), "80%");
    run_for(&mut worker, &clock, 500);
    worker.stop();
}

#[test]
fn write_settings_through_the_spawned_worker_control() {
    use open_powerz_desktop_lib::worker::{self, BackendEvent};
    let clock = clock();
    let c = clock.clone();
    let open = move || {
        Ok(Pipe::sim(
            Scenario::Load5v1a,
            FaultPlan::new(),
            &c,
            PipeOpts::default(),
        ))
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let sink = move |e: BackendEvent| {
        if let BackendEvent::Batch(b) = &e
            && !b.samples.is_empty()
        {
            let _ = tx.send(());
        }
    };
    let handle = worker::spawn(open, clock, WorkerConfig::new(Rate::Sps50, 1), sink);
    rx.recv_timeout(std::time::Duration::from_secs(10))
        .expect("no samples");
    let view = handle
        .control()
        .write_settings(&changes(&[("rotation", 3)]))
        .expect("write failed");
    let v = serde_json::to_value(&view).unwrap();
    assert_eq!(value_of(&v, "rotation"), "270°");
    handle.disconnect();
}

#[test]
fn keys_that_are_not_writable_are_usage_errors_and_send_nothing() {
    let (mut worker, clock, log) = start_with(FaultPlan::new());
    run_for(&mut worker, &clock, 500);
    for key in [
        "voltage_cal",
        "current_cal",
        "offset_cal",
        "auto_rotate",
        "startup_logo",
        "otp",
        "factory_reset",
        "nonsense",
    ] {
        let err = worker
            .write_settings(&changes(&[("brightness", 80), (key, 1)]))
            .expect_err(key);
        let e = serde_json::to_value(&err).unwrap();
        assert_eq!(e["kind"], "usage", "{key}: {e}");
        assert!(
            e["message"].as_str().unwrap_or("").contains(key),
            "{key}: {e}"
        );
    }
    assert!(settings_writes(&log).is_empty());
}

#[test]
fn values_out_of_range_are_usage_errors_and_send_nothing() {
    let (mut worker, clock, log) = start_with(FaultPlan::new());
    run_for(&mut worker, &clock, 500);
    for (key, bad) in [
        ("brightness", 0),
        ("brightness", 101),
        ("language", 2),
        ("dim_timeout", 4),
        ("theme", 7),
        ("rotation", -1),
        ("sample_rate", 4),
    ] {
        let err = worker
            .write_settings(&changes(&[(key, bad)]))
            .expect_err(key);
        let e = serde_json::to_value(&err).unwrap();
        assert_eq!(e["kind"], "usage", "{key}={bad}: {e}");
        assert!(
            e["message"].as_str().unwrap_or("").contains(key),
            "{key}: {e}"
        );
    }
    assert!(settings_writes(&log).is_empty());
}

#[test]
fn an_empty_change_is_a_usage_error_and_sends_nothing() {
    let (mut worker, clock, log) = start_with(FaultPlan::new());
    run_for(&mut worker, &clock, 500);
    let err = worker.write_settings(&BTreeMap::new()).expect_err("empty");
    assert_eq!(serde_json::to_value(&err).unwrap()["kind"], "usage");
    assert!(settings_writes(&log).is_empty());
}

#[test]
fn a_mismatching_re_read_is_an_error_naming_the_field_and_is_not_retried() {
    let (mut worker, clock, log) = start_with(FaultPlan::new().ignore_settings_write());
    run_for(&mut worker, &clock, 500);
    let err = worker
        .write_settings(&changes(&[("brightness", 80)]))
        .expect_err("mismatch");
    let e = serde_json::to_value(&err).unwrap();
    assert!(
        e["message"].as_str().unwrap_or("").contains("brightness"),
        "{e}"
    );
    assert_eq!(settings_writes(&log).len(), 1, "the write is not retried");
    run_for(&mut worker, &clock, 500);
    assert_eq!(settings_writes(&log).len(), 1, "nor later");
}

#[test]
fn write_settings_without_a_session_is_an_error_and_sends_nothing() {
    let (mut worker, clock, log) = start_with(FaultPlan::new());
    run_for(&mut worker, &clock, 200);
    worker.stop();
    let before = log.lock().unwrap().frames.len();
    assert!(
        worker
            .write_settings(&changes(&[("brightness", 80)]))
            .is_err()
    );
    assert_eq!(log.lock().unwrap().frames.len(), before);
}

#[test]
fn meter_settings_write_command_is_registered() {
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    let handler = lib
        .split("generate_handler![")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .expect("generate_handler! list");
    assert!(
        handler
            .split(',')
            .any(|n| n.trim() == "meter_settings_write"),
        "meter_settings_write is not in the invoke handler"
    );
    assert!(
        lib.contains("async fn meter_settings_write("),
        "no meter_settings_write command"
    );
}
