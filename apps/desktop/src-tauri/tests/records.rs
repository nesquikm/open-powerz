//! Records view backend (FR 1HHT00, AC .5, .6 and .7): the worker lists the
//! demo meter's offline records and downloads one at a time with
//! GET_DATA 0x200 and one memory read of the record's own window; the live
//! session goes on afterwards.

mod common;

use std::time::Duration;

use common::*;
use open_powerz_desktop_lib::worker::{self, BackendEvent, Step, Worker, WorkerConfig};
use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::decode::Rate;
use powerz_sim::SimClock;
use serde_json::Value;

/// The RecordList GET_DATA mask.
const RECORD_LIST: u16 = 0x200;

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

fn list(worker: &mut Worker<Pipe, SimClock>) -> Vec<Value> {
    let view = worker.list_records().expect("record list failed");
    let v = serde_json::to_value(&view).unwrap();
    v["records"]
        .as_array()
        .unwrap_or_else(|| panic!("{v}"))
        .clone()
}

/// `(addr, len)` of a 0x44 request.
fn memread_window(f: &[u8]) -> (u32, u32) {
    let plain = crypto::decrypt(&KEYS[0], &f[4..36]);
    (
        u32::from_le_bytes(plain[0..4].try_into().unwrap()),
        u32::from_le_bytes(plain[4..8].try_into().unwrap()),
    )
}

fn get_data_mask(f: &[u8]) -> u16 {
    u16::from_le_bytes([f[2], f[3]]) >> 1
}

// ---- AC-1HHT00.5 / .6: the list with the decoded columns ----

#[test]
fn list_records_returns_the_demo_meters_records_with_every_column() {
    let (mut worker, clock, _log) = start();
    run_for(&mut worker, &clock, 500);
    let recs = list(&mut worker);
    assert!(recs.len() >= 3, "{recs:?}");
    for (i, r) in recs.iter().enumerate() {
        assert_eq!(r["index"], i + 1, "1-based index: {r}");
        assert!(r["name"].as_str().is_some_and(|n| !n.is_empty()), "{r}");
        for k in ["interval_s", "points", "duration_s", "offset"] {
            assert!(r[k].as_u64().is_some(), "{k}: {r}");
        }
        for k in ["charge_ah", "energy_wh"] {
            assert!(r[k].as_f64().is_some(), "{k}: {r}");
        }
    }
    // The session keeps streaming afterwards.
    assert_eq!(run_for(&mut worker, &clock, 500), Step::Continue);
    worker.stop();
}

// ---- AC-1HHT00.5 / .6: download one record ----

#[test]
fn download_record_returns_every_sample_with_its_time() {
    let (mut worker, clock, _log) = start();
    run_for(&mut worker, &clock, 500);
    let recs = list(&mut worker);
    let r = recs
        .iter()
        .find(|r| r["points"].as_u64().unwrap() > 0)
        .unwrap();
    let index = r["index"].as_u64().unwrap() as usize;
    let rec = worker.download_record(index).expect("download failed");
    let v = serde_json::to_value(&rec).unwrap();
    assert_eq!(v["index"], index);
    assert_eq!(v["name"], r["name"]);
    assert_eq!(v["interval_s"], r["interval_s"]);
    let samples = v["samples"].as_array().unwrap_or_else(|| panic!("{v}"));
    assert_eq!(samples.len() as u64, r["points"].as_u64().unwrap());
    let interval = r["interval_s"].as_f64().unwrap();
    for (k, s) in samples.iter().enumerate() {
        assert_eq!(s["t_s"].as_f64().unwrap(), interval * k as f64, "{s}");
        for f in ["vbus_v", "ibus_a", "charge_ah", "energy_wh"] {
            assert!(s[f].as_f64().is_some(), "{f}: {s}");
        }
    }
    let last = samples.last().unwrap();
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert!(near(
        last["charge_ah"].as_f64().unwrap(),
        r["charge_ah"].as_f64().unwrap()
    ));
    assert!(near(
        last["energy_wh"].as_f64().unwrap(),
        r["energy_wh"].as_f64().unwrap()
    ));
    assert_eq!(run_for(&mut worker, &clock, 500), Step::Continue);
    worker.stop();
}

#[test]
fn download_of_an_unknown_index_is_an_error_and_reads_no_samples() {
    let (mut worker, clock, log) = start();
    run_for(&mut worker, &clock, 300);
    let n = list(&mut worker).len();
    let before = log.lock().unwrap().frames.len();
    for index in [0, n + 1, 999] {
        assert!(worker.download_record(index).is_err(), "{index}");
    }
    let log = log.lock().unwrap();
    for (_, f) in &log.frames[before..] {
        if f[0] & 0x7F == 0x44 {
            let (addr, _) = memread_window(f);
            assert!(addr < 0x9800_0000, "a record was read: {addr:#x}");
        }
    }
}

#[test]
fn list_and_download_without_a_session_are_errors_and_send_nothing() {
    let (mut worker, clock, log) = start();
    run_for(&mut worker, &clock, 200);
    worker.stop();
    let before = log.lock().unwrap().frames.len();
    assert!(worker.list_records().is_err());
    assert!(worker.download_record(1).is_err());
    assert_eq!(log.lock().unwrap().frames.len(), before);
}

// ---- AC-1HHT00.7: exactly these commands, nothing else ----

#[test]
fn list_sends_one_get_data_record_list_only() {
    let (mut worker, clock, log) = start();
    run_for(&mut worker, &clock, 500);
    let before = log.lock().unwrap().frames.len();
    list(&mut worker);
    let log = log.lock().unwrap();
    let sent: Vec<&Vec<u8>> = log.frames[before..].iter().map(|(_, f)| f).collect();
    assert_eq!(sent.len(), 1, "frames for one list: {sent:02x?}");
    assert_eq!(sent[0][0] & 0x7F, 0x0C);
    assert_eq!(get_data_mask(sent[0]), RECORD_LIST);
}

#[test]
fn download_sends_one_memory_read_of_exactly_the_listed_window() {
    let (mut worker, clock, log) = start();
    run_for(&mut worker, &clock, 500);
    let recs = list(&mut worker);
    for r in recs.iter().filter(|r| r["points"].as_u64().unwrap() > 0) {
        let index = r["index"].as_u64().unwrap() as usize;
        let want = (
            0x9810_0000 + r["offset"].as_u64().unwrap() as u32,
            r["points"].as_u64().unwrap() as u32 * 16,
        );
        let before = log.lock().unwrap().frames.len();
        worker.download_record(index).expect("download failed");
        let log = log.lock().unwrap();
        let sent: Vec<&Vec<u8>> = log.frames[before..].iter().map(|(_, f)| f).collect();
        let reads: Vec<&&Vec<u8>> = sent.iter().filter(|f| f[0] & 0x7F == 0x44).collect();
        assert_eq!(reads.len(), 1, "record {index}: {sent:02x?}");
        assert_eq!(memread_window(reads[0]), want, "record {index}");
        assert_eq!(
            sent.last().map(|f| f[0] & 0x7F),
            Some(0x44),
            "the memory read is the last frame"
        );
        for f in &sent {
            let ty = f[0] & 0x7F;
            assert!(
                ty == 0x44 || (ty == 0x0C && get_data_mask(f) == RECORD_LIST),
                "record {index}: unexpected frame {f:02x?}"
            );
        }
    }
    worker.stop();
}

#[test]
fn a_full_records_session_stays_inside_the_safe_command_set() {
    let (mut worker, clock, log) = start();
    run_for(&mut worker, &clock, 500);
    let recs = list(&mut worker);
    let windows: Vec<(u32, u32)> = recs
        .iter()
        .filter(|r| r["points"].as_u64().unwrap() > 0)
        .map(|r| {
            (
                0x9810_0000 + r["offset"].as_u64().unwrap() as u32,
                r["points"].as_u64().unwrap() as u32 * 16,
            )
        })
        .collect();
    for r in &recs {
        let index = r["index"].as_u64().unwrap() as usize;
        let _ = worker.download_record(index);
        run_for(&mut worker, &clock, 100);
    }
    worker.stop();
    let allowed_mask = 0x001 | 0x002 | 0x008 | RECORD_LIST;
    let log = log.lock().unwrap();
    for (_, f) in &log.frames {
        let ty = f[0] & 0x7F;
        assert!(SAFE_TYPES.contains(&ty), "outside the safe set: {f:02x?}");
        match ty {
            0x0C => assert_eq!(
                get_data_mask(f) & !allowed_mask,
                0,
                "GET_DATA mask {:#x}",
                get_data_mask(f)
            ),
            0x44 => {
                let w = memread_window(f);
                assert!(
                    ALLOWED_READS.contains(&w) || windows.contains(&w),
                    "memory read {:#x}+{}",
                    w.0,
                    w.1
                );
            }
            _ => {}
        }
    }
}

// ---- AC-1HHT00.5: through the spawned worker and the Tauri commands ----

#[test]
fn records_through_the_spawned_worker_control() {
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
    let view = handle.control().list_records().expect("list failed");
    let v = serde_json::to_value(&view).unwrap();
    let n = v["records"].as_array().map(Vec::len).unwrap_or(0);
    assert!(n >= 3, "{v}");
    let rec = handle
        .control()
        .download_record(1)
        .expect("download failed");
    let v = serde_json::to_value(&rec).unwrap();
    assert!(v["samples"].as_array().is_some(), "{v}");
    handle.disconnect();
}

#[test]
fn records_commands_are_registered() {
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    let handler = lib
        .split("generate_handler![")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .expect("generate_handler! list");
    for name in ["records_list", "record_download"] {
        assert!(
            handler.split(',').any(|n| n.trim() == name),
            "{name} is not in the invoke handler"
        );
        assert!(
            lib.contains(&format!("async fn {name}(")),
            "no {name} command"
        );
    }
}
