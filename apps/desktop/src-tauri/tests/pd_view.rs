//! Desktop PD analyzer view, backend side (FR TGE83C): the session worker
//! runs the PD sniffer of the capture core on request, sends the captured
//! records to the UI in batches (rows with the decoded summary), answers the
//! field tree and raw bytes of one record on demand, and clears the capture.
//!
//! Wire shapes pinned here (what the UI reads, as JSON):
//!
//! - `{ "type": "pd_state", "data": { "capturing": bool } }` on start / stop.
//! - `{ "type": "pd_rows", "data": { "rows": [row, ...] } }`, batched like
//!   the measurement stream (at most 60 a second), where a row is
//!   `{ index, elapsed_ms, kind: "message" | "event", sop: "SOP" | "SOP'" | ... | null,
//!      name, message_id, direction: "source_to_sink" | "sink_to_source" | "cable" | null,
//!      objects, spec_revision, vbus_v, ibus_a, summary }`.
//! - `pd_details(index)` → `{ index, raw: [u8...], fields: [node...] }` with
//!   `raw` the PD message bytes (16-bit header little-endian, then the body)
//!   and each node `{ name, meaning, raw, bytes: [start, end), children }`.
//!   `bytes` maps the node's bit range onto `raw`: the header is bytes
//!   0..2, data object `i` (from 1) bytes `2 + 4(i-1) .. 2 + 4i`, the
//!   extended header bytes 2..4, and a child with bits `hi..=lo` of its word
//!   at `w` covers bytes `w + lo / 8 ..= w + hi / 8`.
//!
//! Device safety: the capture sends only 0x10 (mode 1), 0x11 and GET_DATA on
//! top of the session's safe set. Every value is synthetic.

mod common;

use std::sync::mpsc;
use std::time::{Duration, Instant};

use common::*;
use open_powerz_desktop_lib::devices::{AppOpener, DeviceListing, Opener, list_devices_from};
use open_powerz_desktop_lib::worker::{self, BackendEvent, GapKind, Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::session::SystemClock;
use powerz_sim::PdScenario;
use serde_json::Value;

fn config() -> WorkerConfig {
    WorkerConfig::new(Rate::Sps50, 1)
}

/// A worker on a simulator that plays the PD negotiation scenario.
fn start() -> (
    Worker<Pipe, SimClock>,
    SimClock,
    std::sync::Arc<std::sync::Mutex<Log>>,
    Events,
) {
    let clock = clock();
    let dev = SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, clock.clone())
        .with_pd_scenario(PdScenario::Negotiation);
    let pipe = Pipe::new(dev, &clock, PipeOpts::default());
    let log = std::sync::Arc::clone(&pipe.log);
    let (events, sink) = capture(&clock);
    let worker = Worker::start(pipe, clock.clone(), config(), sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    (worker, clock, log, events)
}

/// Every event as the UI receives it (JSON), with its simulated time.
fn json_events(events: &Events) -> Vec<(u64, Value)> {
    events
        .lock()
        .unwrap()
        .iter()
        .map(|(t, e)| (*t, serde_json::to_value(e).expect("event serializes")))
        .collect()
}

/// The `pd_rows` events, with their simulated time.
fn row_batches(events: &Events) -> Vec<(u64, Vec<Value>)> {
    json_events(events)
        .into_iter()
        .filter(|(_, e)| e["type"] == "pd_rows")
        .map(|(t, e)| {
            let rows = e["data"]["rows"]
                .as_array()
                .unwrap_or_else(|| panic!("pd_rows without a rows array: {e}"))
                .clone();
            (t, rows)
        })
        .collect()
}

/// Every PD row sent so far, in order.
fn rows(events: &Events) -> Vec<Value> {
    row_batches(events)
        .into_iter()
        .flat_map(|(_, r)| r)
        .collect()
}

/// The `capturing` flag of every `pd_state` event, in order.
fn pd_states(events: &Events) -> Vec<bool> {
    json_events(events)
        .into_iter()
        .filter(|(_, e)| e["type"] == "pd_state")
        .map(|(_, e)| {
            e["data"]["capturing"]
                .as_bool()
                .unwrap_or_else(|| panic!("pd_state without capturing: {e}"))
        })
        .collect()
}

/// Rows named `name` (exact message name).
fn named<'a>(rows: &'a [Value], name: &str) -> Vec<&'a Value> {
    rows.iter().filter(|r| r["name"] == name).collect()
}

/// The PD scenario plays for ~3.4 s from the sniffer start.
const SCENARIO_MS: u64 = 3_800;

/// Number of records in the negotiation scenario: attach, SPR negotiation
/// (4), EPR_Mode (3), EPR_Source_Capabilities chunks (3), EPR_Request,
/// Accept, PS_RDY, keep-alives (6), Hard Reset, SPR negotiation (4).
const SCENARIO_RECORDS: usize = 25;

fn captured() -> (
    Worker<Pipe, SimClock>,
    SimClock,
    std::sync::Arc<std::sync::Mutex<Log>>,
    Events,
) {
    let (mut worker, clock, log, events) = start();
    run_for(&mut worker, &clock, 200);
    worker.start_pd().expect("PD capture starts");
    run_for(&mut worker, &clock, SCENARIO_MS);
    (worker, clock, log, events)
}

// ---- AC-TGE83C.1: Start and Stop on the connected meter ----

#[test]
fn start_sends_the_sniffer_start_and_reports_capturing() {
    let (mut worker, clock, log, events) = start();
    run_for(&mut worker, &clock, 200);
    assert!(
        log.lock().unwrap().of_type(0x10).is_empty(),
        "the sniffer started before Start"
    );
    worker.start_pd().expect("PD capture starts");
    run_for(&mut worker, &clock, 100);
    let starts = log.lock().unwrap().of_type(0x10);
    assert_eq!(starts.len(), 1, "one sniffer start: {starts:02x?}");
    assert_eq!(
        starts[0].1[2], 0x02,
        "start in PD sniffer mode 1: {starts:02x?}"
    );
    assert_eq!(pd_states(&events), vec![true]);
}

#[test]
fn stop_sends_the_sniffer_stop_and_polls_no_more_pd_packets() {
    let (mut worker, clock, log, events) = start();
    worker.start_pd().expect("PD capture starts");
    run_for(&mut worker, &clock, 500);
    worker.stop_pd().expect("PD capture stops");
    let after_stop = log.lock().unwrap().frames.len();
    run_for(&mut worker, &clock, 500);
    let log = log.lock().unwrap();
    assert_eq!(log.of_type(0x11).len(), 1, "one sniffer stop");
    let pd_polls_after: Vec<_> = log.frames[after_stop..]
        .iter()
        .filter(|(_, f)| {
            frame::get_data_mask(f).is_some_and(|m| m.bits() & Attribute::PD_PACKET.bits() != 0)
        })
        .collect();
    assert!(
        pd_polls_after.is_empty(),
        "PD_PACKET still polled after Stop: {pd_polls_after:02x?}"
    );
    assert_eq!(pd_states(&events), vec![true, false]);
}

#[test]
fn the_capture_sends_only_the_safe_set_plus_sniffer_start_and_stop() {
    let (mut worker, clock, log, _events) = captured();
    worker.stop_pd().expect("PD capture stops");
    run_for(&mut worker, &clock, 100);
    worker.stop();
    let log = log.lock().unwrap();
    for (_, f) in &log.frames {
        let ty = f[0] & 0x7F;
        assert!(
            SAFE_TYPES.contains(&ty) || ty == 0x10 || ty == 0x11,
            "frame type {ty:#04x} outside the PD capture's safe set: {f:02x?}"
        );
        if ty == 0x10 {
            assert_eq!(f[2], 0x02, "0x10 only in PD sniffer mode 1: {f:02x?}");
        }
    }
}

#[test]
fn disconnecting_while_capturing_stops_the_sniffer_first() {
    let (mut worker, clock, log, _events) = start();
    worker.start_pd().expect("PD capture starts");
    run_for(&mut worker, &clock, 500);
    worker.stop();
    let log = log.lock().unwrap();
    assert_eq!(
        log.of_type(0x11).len(),
        1,
        "the sniffer was left running on disconnect"
    );
    assert!(log.closed, "the pipe stayed open");
}

#[test]
fn the_demo_device_plays_the_simulator_pd_scenario() {
    let demo: DeviceListing = list_devices_from(Ok(vec![]))
        .into_iter()
        .find(|d| d.id == "demo:load-5v-1a")
        .expect("the demo device is listed");
    let transport = AppOpener.open(&demo).expect("the demo device opens");
    let (tx, rx) = mpsc::channel::<Value>();
    let sink = move |e: BackendEvent| {
        let _ = tx.send(serde_json::to_value(&e).expect("event serializes"));
    };
    let mut worker =
        Worker::start(transport, SystemClock, config(), sink).expect("demo worker starts");
    worker
        .start_pd()
        .expect("PD capture starts on the demo device");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut names = Vec::new();
    while Instant::now() < deadline && !names.iter().any(|n| n == "Source_Capabilities") {
        worker.step();
        for e in rx.try_iter() {
            if e["type"] == "pd_rows" {
                for r in e["data"]["rows"].as_array().cloned().unwrap_or_default() {
                    names.push(r["name"].as_str().unwrap_or_default().to_owned());
                }
            }
        }
    }
    worker.stop();
    assert!(
        names.iter().any(|n| n == "Source_Capabilities"),
        "the demo device captured no PD negotiation: {names:?}"
    );
}

// ---- AC-TGE83C.2: row contents, records reach the UI in batches ----

#[test]
fn every_scenario_record_arrives_as_one_row_with_a_running_index() {
    let (_worker, _clock, _log, events) = captured();
    let rows = rows(&events);
    assert_eq!(rows.len(), SCENARIO_RECORDS, "{rows:#?}");
    for (i, r) in rows.iter().enumerate() {
        assert_eq!(r["index"], i as u64, "row {i}: {r}");
    }
    let times: Vec<u64> = rows
        .iter()
        .map(|r| r["elapsed_ms"].as_u64().expect("elapsed_ms"))
        .collect();
    assert!(
        times.windows(2).all(|w| w[0] <= w[1]),
        "elapsed_ms goes backwards: {times:?}"
    );
    let attach = &rows[0];
    let caps = named(&rows, "Source_Capabilities")[0];
    assert_eq!(
        caps["elapsed_ms"].as_u64().unwrap() - attach["elapsed_ms"].as_u64().unwrap(),
        280,
        "time since capture start follows the meter's record times"
    );
}

#[test]
fn rows_go_out_in_batches_at_most_60_a_second() {
    let (_worker, _clock, _log, events) = captured();
    let batches = row_batches(&events);
    assert!(
        batches.iter().all(|(_, r)| !r.is_empty()),
        "an empty pd_rows batch"
    );
    assert!(
        batches.len() < SCENARIO_RECORDS,
        "one event per record: {} batches for {SCENARIO_RECORDS} records",
        batches.len()
    );
    assert!(
        batches.iter().any(|(_, r)| r.len() >= 2),
        "records 2 ms apart never shared a batch"
    );
    for w in batches.windows(2) {
        assert!(
            w[1].0 - w[0].0 >= 16,
            "pd_rows batches {} ms apart (more than 60 a second)",
            w[1].0 - w[0].0
        );
    }
}

#[test]
fn a_message_row_has_sop_name_id_direction_objects_revision_and_summary() {
    let (_worker, _clock, _log, events) = captured();
    let rows = rows(&events);
    let caps = named(&rows, "Source_Capabilities")[0];
    assert_eq!(caps["kind"], "message");
    assert_eq!(caps["sop"], "SOP");
    assert_eq!(caps["message_id"], 0);
    assert_eq!(caps["direction"], "source_to_sink");
    assert_eq!(caps["objects"], 4);
    assert!(
        caps["spec_revision"]
            .as_str()
            .is_some_and(|s| s.contains('3')),
        "spec revision 3.x: {caps}"
    );
    let summary = caps["summary"].as_str().expect("summary");
    assert!(
        summary.starts_with("Source_Capabilities") && summary.contains("5.00 V"),
        "summary is the decoder's line: {summary}"
    );

    let request = named(&rows, "Request")[0];
    assert_eq!(request["direction"], "sink_to_source");
    assert_eq!(request["message_id"], 0);
    assert_eq!(request["objects"], 1);

    let accept = named(&rows, "Accept")[0];
    assert_eq!(accept["objects"], 0);
    assert_eq!(accept["message_id"], 1, "the source's second message");
}

#[test]
fn rows_carry_vbus_and_ibus_at_their_time() {
    let (_worker, _clock, _log, events) = captured();
    let rows = rows(&events);
    let caps = named(&rows, "Source_Capabilities")[0];
    let vbus = caps["vbus_v"].as_f64().expect("vbus_v");
    assert!(
        (vbus - 5.0).abs() < 0.01,
        "VBUS at the SPR negotiation: {vbus}"
    );
    assert!(caps["ibus_a"].as_f64().is_some(), "ibus_a: {caps}");
    // Keep-alives run inside the 28 V EPR contract.
    let keepalive = named(&rows, "Extended_Control")[0];
    let vbus = keepalive["vbus_v"].as_f64().expect("vbus_v");
    assert!(
        (vbus - 28.0).abs() < 0.01,
        "VBUS in the EPR contract: {vbus}"
    );
}

#[test]
fn event_rows_are_kind_event_without_message_fields() {
    let (_worker, _clock, _log, events) = captured();
    let rows = rows(&events);
    let attach = &rows[0];
    assert_eq!(attach["kind"], "event", "{attach}");
    assert!(
        attach["name"]
            .as_str()
            .unwrap_or_default()
            .contains("Attach"),
        "{attach}"
    );
    assert!(
        attach["sop"].is_null() && attach["message_id"].is_null(),
        "{attach}"
    );
    assert!(
        attach["objects"].is_null() && attach["direction"].is_null(),
        "{attach}"
    );
    let reset: Vec<&Value> = rows.iter().filter(|r| r["kind"] == "event").collect();
    assert_eq!(reset.len(), 2, "attach and hard reset: {reset:#?}");
    let hard = reset[1]["name"].as_str().unwrap_or_default().to_lowercase();
    assert!(hard.contains("hard") && hard.contains("reset"), "{hard}");
    assert!(
        !reset[1]["summary"].as_str().unwrap_or_default().is_empty(),
        "an event row has a summary line"
    );
}

// ---- AC-TGE83C.4: the field tree and raw bytes on demand ----

fn details(worker: &mut Worker<Pipe, SimClock>, index: u64) -> Value {
    let d = worker
        .pd_details(index)
        .unwrap_or_else(|e| panic!("no details for row {index}: {e:?}"));
    serde_json::to_value(&d).expect("details serialize")
}

fn node<'a>(nodes: &'a Value, name: &str) -> &'a Value {
    nodes
        .as_array()
        .and_then(|a| a.iter().find(|n| n["name"] == name))
        .unwrap_or_else(|| panic!("no field {name:?} in {nodes:#}"))
}

fn bytes(n: &Value) -> (u64, u64) {
    let b = n["bytes"]
        .as_array()
        .unwrap_or_else(|| panic!("no bytes: {n}"));
    assert_eq!(b.len(), 2, "bytes is [start, end): {n}");
    (b[0].as_u64().unwrap(), b[1].as_u64().unwrap())
}

#[test]
fn details_give_the_raw_message_bytes_and_the_decoded_field_tree() {
    let (mut worker, _clock, _log, events) = captured();
    let rows = rows(&events);
    let caps = named(&rows, "Source_Capabilities")[0];
    let index = caps["index"].as_u64().unwrap();
    let d = details(&mut worker, index);
    assert_eq!(d["index"], index);
    let raw: Vec<u8> = d["raw"]
        .as_array()
        .expect("raw is a byte array")
        .iter()
        .map(|b| b.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(raw.len(), 2 + 4 * 4, "header plus four PDOs: {raw:02x?}");
    let header = u16::from_le_bytes([raw[0], raw[1]]);
    assert_eq!(header & 0x1F, 1, "Source_Capabilities header first");
    assert_eq!((header >> 12) & 7, 4);
    let fields = &d["fields"];
    let h = node(fields, "Header");
    assert!(
        node(&h["children"], "Message Type")["meaning"]
            .as_str()
            .is_some_and(|m| m.contains("Source_Capabilities")),
        "{h:#}"
    );
    let pdo1 = node(fields, "PDO 1");
    assert!(
        node(&pdo1["children"], "Voltage")["meaning"]
            .as_str()
            .is_some_and(|m| m.contains("5.00 V")),
        "{pdo1:#}"
    );
}

#[test]
fn field_byte_ranges_map_bit_ranges_onto_the_raw_bytes() {
    let (mut worker, _clock, _log, events) = captured();
    let rows = rows(&events);
    let caps = named(&rows, "Source_Capabilities")[0]["index"]
        .as_u64()
        .unwrap();
    let d = details(&mut worker, caps);
    let fields = &d["fields"];
    let h = node(fields, "Header");
    assert_eq!(bytes(h), (0, 2), "header: bytes 0..2");
    assert_eq!(
        bytes(node(&h["children"], "Message Type")),
        (0, 1),
        "bits 4..0"
    );
    assert_eq!(
        bytes(node(&h["children"], "Number of Data Objects")),
        (1, 2),
        "bits 14..12"
    );
    assert_eq!(
        bytes(node(&h["children"], "MessageID")),
        (1, 2),
        "bits 11..9"
    );
    assert_eq!(bytes(node(fields, "PDO 1")), (2, 6));
    assert_eq!(bytes(node(fields, "PDO 4")), (14, 18));
    let pdo1 = node(fields, "PDO 1");
    assert_eq!(
        bytes(node(&pdo1["children"], "Voltage")),
        (3, 5),
        "bits 19..10 of the word at byte 2"
    );
    assert_eq!(
        bytes(node(&pdo1["children"], "Maximum Current")),
        (2, 4),
        "bits 9..0 of the word at byte 2"
    );

    // An extended message: the extended header sits at bytes 2..4.
    let ka = named(&rows, "Extended_Control")[0]["index"]
        .as_u64()
        .unwrap();
    let d = details(&mut worker, ka);
    let ext = d["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| {
            n["name"]
                .as_str()
                .is_some_and(|s| s.contains("Extended Header"))
        })
        .unwrap_or_else(|| panic!("no extended header node: {:#}", d["fields"]));
    assert_eq!(bytes(ext), (2, 4));
}

/// Every node's byte range lies inside `raw` and inside its parent's range.
fn check_ranges(nodes: &Value, parent: (u64, u64), len: u64, path: &str) {
    for n in nodes.as_array().into_iter().flatten() {
        let (s, e) = bytes(n);
        let here = format!("{path}/{}", n["name"]);
        assert!(s <= e && e <= len, "{here}: {s}..{e} outside 0..{len}");
        if s < e {
            assert!(
                s >= parent.0 && e <= parent.1,
                "{here}: {s}..{e} outside its parent {parent:?}"
            );
            check_ranges(&n["children"], (s, e), len, &here);
        }
    }
}

#[test]
fn every_row_has_details_whose_ranges_stay_inside_its_bytes() {
    let (mut worker, _clock, _log, events) = captured();
    for r in rows(&events) {
        let index = r["index"].as_u64().unwrap();
        let d = details(&mut worker, index);
        let len = d["raw"].as_array().map_or(0, Vec::len) as u64;
        assert!(len > 0, "row {index} has no raw bytes");
        if r["kind"] == "message" {
            assert!(
                d["fields"].as_array().is_some_and(|f| !f.is_empty()),
                "message row {index} has no field tree"
            );
        }
        check_ranges(&d["fields"], (0, len), len, &format!("row {index}"));
    }
}

#[test]
fn details_of_an_unknown_row_are_an_error() {
    let (mut worker, _clock, _log, _events) = captured();
    assert!(worker.pd_details(10_000).is_err());
}

// ---- AC-TGE83C.7: Clear empties the capture ----

#[test]
fn clear_drops_the_captured_records() {
    let (mut worker, _clock, _log, events) = captured();
    assert!(worker.pd_details(0).is_ok());
    worker.clear_pd().expect("clear");
    assert!(
        worker.pd_details(0).is_err(),
        "a cleared record still answers details"
    );
    assert_eq!(rows(&events).len(), SCENARIO_RECORDS, "clear sends no rows");
}

// ---- The same through the worker's control (what the Tauri commands use) ----

#[test]
fn the_worker_control_starts_answers_clears_and_stops_the_capture() {
    let clock = clock();
    let c = clock.clone();
    let log_slot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let l = std::sync::Arc::clone(&log_slot);
    let open = move || {
        let dev = SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, c.clone())
            .with_pd_scenario(PdScenario::Negotiation);
        let pipe = Pipe::new(dev, &c, PipeOpts::default());
        *l.lock().unwrap() = Some(std::sync::Arc::clone(&pipe.log));
        Ok(pipe)
    };
    let (tx, rx) = mpsc::channel::<Value>();
    let sink = move |e: BackendEvent| {
        let _ = tx.send(serde_json::to_value(&e).expect("event serializes"));
    };
    let handle = worker::spawn(open, clock, config(), sink);
    let control = handle.control();
    let deadline = Instant::now() + Duration::from_secs(10);
    // Wait for the session, then start the capture.
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(e) if e["type"] == "status" && e["data"]["state"] == "connected" => break,
            _ => {}
        }
    }
    control.start_pd().expect("start through the control");
    let mut got = 0usize;
    while Instant::now() < deadline && got < 5 {
        if let Ok(e) = rx.recv_timeout(Duration::from_millis(100))
            && e["type"] == "pd_rows"
        {
            got += e["data"]["rows"].as_array().map_or(0, Vec::len);
        }
    }
    assert!(got >= 5, "no rows through the spawned worker");
    let d = serde_json::to_value(control.pd_details(0).expect("details of row 0")).unwrap();
    assert_eq!(d["index"], 0);
    control.stop_pd().expect("stop through the control");
    control.clear_pd().expect("clear through the control");
    assert!(
        control.pd_details(0).is_err(),
        "a cleared record still answers details"
    );
    handle.disconnect();
    let log = log_slot.lock().unwrap().clone().expect("pipe opened");
    let log = log.lock().unwrap();
    assert_eq!(log.of_type(0x10).len(), 1);
    assert_eq!(log.of_type(0x11).len(), 1);
}

#[test]
fn records_after_a_clear_keep_counting_so_late_rows_cannot_collide() {
    let (mut worker, clock, _log, events) = captured();
    let next = worker.clear_pd().expect("clear");
    assert_eq!(
        next, SCENARIO_RECORDS as u64,
        "clear returns the next index"
    );
    worker.stop_pd().expect("stop");
    worker.start_pd().expect("start again");
    run_for(&mut worker, &clock, SCENARIO_MS);
    let after: Vec<u64> = rows(&events)
        .iter()
        .skip(SCENARIO_RECORDS)
        .filter_map(|r| r["index"].as_u64())
        .collect();
    assert!(!after.is_empty(), "the second capture sent no rows");
    assert!(after.iter().all(|&i| i >= next), "{after:?}");
    assert!(worker.pd_details(next).is_ok());
    assert!(worker.pd_details(0).is_err());
}

#[test]
fn a_second_capture_without_a_clear_keeps_the_tables_time_going() {
    let (mut worker, clock, _log, events) = captured();
    worker.stop_pd().expect("stop");
    let first = rows(&events);
    let last = first
        .last()
        .and_then(|r| r["elapsed_ms"].as_u64())
        .expect("the first capture sent rows");
    run_for(&mut worker, &clock, 300);
    worker.start_pd().expect("start again");
    run_for(&mut worker, &clock, SCENARIO_MS);
    let second: Vec<u64> = rows(&events)
        .iter()
        .skip(first.len())
        .filter_map(|r| r["elapsed_ms"].as_u64())
        .collect();
    assert!(!second.is_empty(), "the second capture sent no rows");
    assert!(
        second.iter().all(|&t| t >= last),
        "elapsed_ms restarted in the same table: last of the first run {last}, second run {second:?}"
    );
}

// ---- The PD poll shares the worker thread with the measurement queue ----

#[test]
fn a_running_pd_capture_does_not_starve_the_1000_sps_queue() {
    let clock = clock();
    let dev = SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, clock.clone())
        .with_pd_scenario(PdScenario::Negotiation);
    // A slow PD round trip: the PD poll (every 5 ms) takes most of the thread.
    let opts = PipeOpts {
        pd_poll_cost_ms: Some(4),
        ..PipeOpts::default()
    };
    let pipe = Pipe::new(dev, &clock, opts);
    let (events, sink) = capture(&clock);
    let config = WorkerConfig::new(Rate::Sps1000, 1);
    let mut worker = Worker::start(pipe, clock.clone(), config, sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    run_for(&mut worker, &clock, 500);
    let before = samples(&events).len();
    worker.start_pd().expect("PD capture starts");
    const RUN_MS: u64 = 4_000;
    run_for(&mut worker, &clock, RUN_MS);
    assert!(
        !rows(&events).is_empty(),
        "the PD capture captured nothing, so it proves nothing"
    );
    let gaps: Vec<String> = events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(t, e)| match e {
            BackendEvent::Gap(g) if g.kind == GapKind::Stream => Some(format!("{t}: {g:?}")),
            _ => None,
        })
        .collect();
    assert!(gaps.is_empty(), "stream gaps while capturing PD: {gaps:?}");
    let times = samples(&events);
    let during = &times[before..];
    let holes: Vec<_> = during.windows(2).filter(|w| w[1] - w[0] != 1).collect();
    assert!(
        holes.is_empty(),
        "samples missing: {:?}",
        &holes[..holes.len().min(5)]
    );
    // 1000 SPS for RUN_MS, less what is still queued or batched at the end.
    assert!(
        during.len() as u64 >= RUN_MS - 200,
        "{} samples in {RUN_MS} ms at 1000 SPS",
        during.len()
    );
}
