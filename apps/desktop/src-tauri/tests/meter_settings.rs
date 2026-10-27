//! Meter settings panel backend (FR 1HHT01, AC .4 and .6): the worker reads
//! the settings block on request with GET_DATA 0x008 only, and hands the UI
//! tagged rows.

mod common;

use std::time::Duration;

use common::*;
use open_powerz_desktop_lib::worker::{self, BackendEvent, Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_protocol::frame::Attribute;
use powerz_sim::SimClock;
use serde_json::Value;

fn start() -> (
    Worker<Pipe, SimClock>,
    SimClock,
    std::sync::Arc<std::sync::Mutex<Log>>,
) {
    let clock = clock();
    let pipe = Pipe::sim(
        Scenario::Load5v1a,
        FaultPlan::new(),
        &clock,
        PipeOpts::default(),
    );
    let log = std::sync::Arc::clone(&pipe.log);
    let (_events, sink) = capture(&clock);
    let worker = Worker::start(pipe, clock.clone(), WorkerConfig::new(Rate::Sps50, 1), sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    (worker, clock, log)
}

// ---- AC-1HHT01.4: the worker reads the settings for the panel ----

#[test]
fn read_settings_returns_every_field_with_a_tag() {
    let (mut worker, clock, _log) = start();
    run_for(&mut worker, &clock, 500);
    let view = worker.read_settings().expect("settings read failed");
    let v: Value = serde_json::to_value(&view).unwrap();
    let fields = v["fields"].as_array().unwrap_or_else(|| panic!("{v}"));
    let keys: Vec<&str> = fields.iter().filter_map(|f| f["key"].as_str()).collect();
    assert_eq!(
        keys,
        vec![
            "language",
            "brightness",
            "dim_timeout",
            "startup_logo",
            "theme",
            "auto_rotate",
            "rotation",
            "sample_rate",
            "voltage_cal",
            "current_cal",
            "offset_cal",
        ]
    );
    for f in fields {
        assert!(f["label"].as_str().is_some_and(|l| !l.is_empty()), "{f}");
        assert!(f["value"].as_str().is_some_and(|l| !l.is_empty()), "{f}");
        // Screen-checked (1HHT04): all but rotation and the calibration.
        let inferred = ["rotation", "voltage_cal", "current_cal", "offset_cal"];
        let want = if inferred.iter().any(|k| f["key"] == *k) {
            "inferred"
        } else {
            "confirmed"
        };
        assert_eq!(f["tag"], want, "{f}");
    }
    let lang = fields.iter().find(|f| f["key"] == "language").unwrap();
    assert_eq!(
        lang["value"], "English",
        "demo meter mirrors the 2.0.6 block"
    );
    // The session keeps streaming afterwards.
    run_for(&mut worker, &clock, 500);
    worker.stop();
}

#[test]
fn read_settings_through_the_spawned_worker_control() {
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
    rx.recv_timeout(Duration::from_secs(10))
        .expect("no samples");
    let view = handle
        .control()
        .read_settings()
        .expect("settings read failed");
    let v = serde_json::to_value(&view).unwrap();
    assert_eq!(v["fields"].as_array().map(Vec::len), Some(11), "{v}");
    handle.disconnect();
}

// ---- AC-1HHT01.6: only GET_DATA 0x008 goes out for the read ----

#[test]
fn read_settings_sends_only_get_data_settings() {
    let (mut worker, clock, log) = start();
    run_for(&mut worker, &clock, 500);
    let before = log.lock().unwrap().frames.len();
    worker.read_settings().expect("settings read failed");
    let log = log.lock().unwrap();
    let sent = &log.frames[before..];
    assert_eq!(sent.len(), 1, "frames for one settings read: {sent:02x?}");
    let f = &sent[0].1;
    assert_eq!(f[0] & 0x7F, 0x0C, "not a GET_DATA: {f:02x?}");
    let mask = u16::from_le_bytes([f[2], f[3]]) >> 1;
    assert_eq!(mask, Attribute::SETTINGS.bits(), "GET_DATA mask {mask:#x}");
    for (_, f) in log.frames.iter() {
        let ty = f[0] & 0x7F;
        assert_ne!(ty, 0x0A, "settings write sent: {f:02x?}");
        assert_ne!(ty, 0x48, "SET_DATA sent: {f:02x?}");
        assert!(SAFE_TYPES.contains(&ty), "outside the safe set: {f:02x?}");
    }
}

#[test]
fn read_settings_without_a_session_is_an_error_and_sends_nothing() {
    let (mut worker, clock, log) = start();
    run_for(&mut worker, &clock, 200);
    worker.stop();
    let before = log.lock().unwrap().frames.len();
    assert!(worker.read_settings().is_err());
    assert_eq!(log.lock().unwrap().frames.len(), before);
}

// ---- AC-1HHT01.4: the Tauri command is registered ----

#[test]
fn meter_settings_command_is_registered() {
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    let handler = lib
        .split("generate_handler![")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .expect("generate_handler! list");
    assert!(
        handler.split(',').any(|n| n.trim() == "meter_settings"),
        "meter_settings is not in the invoke handler"
    );
    assert!(
        lib.contains("async fn meter_settings("),
        "no meter_settings command"
    );
}
