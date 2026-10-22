//! PD timeline linked to the power chart, backend side (FR TGE83D): PD rows
//! and the PD status blocks reach the UI on the chart's session time base.
//!
//! Wire shapes pinned here (what the UI reads, as JSON):
//!
//! - Every `pd_rows` row carries `session_ms`: the record's time on the same
//!   base as the measurement samples' `t_ms` (session time), so
//!   PD markers and samples share the chart's x axis. `elapsed_ms` (time
//!   since the capture start) stays for the table.
//! - `{ "type": "pd_status", "data": { "samples": [sample, ...] } }`, batched
//!   like the rows (at most 60 a second), while the capture runs, where a
//!   sample is one PD status block:
//!   `{ session_ms, vbus_v, ibus_a, cc1_v, cc2_v }` (volts, amperes signed).
//!
//! Every value is synthetic (the simulator's PD negotiation scenario).

mod common;

use common::*;
use open_powerz_desktop_lib::worker::{Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_sim::PdScenario;
use serde_json::Value;

fn config() -> WorkerConfig {
    WorkerConfig {
        rate: Rate::Sps50,
        session_id: 1,
    }
}

/// The PD scenario plays for ~3.4 s from the sniffer start.
const SCENARIO_MS: u64 = 3_800;

/// A worker on the simulator's PD negotiation scenario, captured for the
/// whole scenario after `lead_ms` of plain measuring.
fn captured(lead_ms: u64) -> (Worker<Pipe, SimClock>, SimClock, Events) {
    let clock = clock();
    let dev = SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, clock.clone())
        .with_pd_scenario(PdScenario::Negotiation);
    let pipe = Pipe::new(dev, &clock, PipeOpts::default());
    let (events, sink) = capture(&clock);
    let mut worker = Worker::start(pipe, clock.clone(), config(), sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    run_for(&mut worker, &clock, lead_ms);
    worker.start_pd().expect("PD capture starts");
    run_for(&mut worker, &clock, SCENARIO_MS);
    (worker, clock, events)
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

/// Every PD row sent so far, in order.
fn rows(events: &Events) -> Vec<Value> {
    json_events(events)
        .into_iter()
        .filter(|(_, e)| e["type"] == "pd_rows")
        .flat_map(|(_, e)| e["data"]["rows"].as_array().cloned().unwrap_or_default())
        .collect()
}

/// Every measurement sample time (`t_ms`) sent so far.
fn sample_times(events: &Events) -> Vec<u64> {
    json_events(events)
        .into_iter()
        .filter(|(_, e)| e["type"] == "batch")
        .flat_map(|(_, e)| e["data"]["samples"].as_array().cloned().unwrap_or_default())
        .filter_map(|s| s["t_ms"].as_u64())
        .collect()
}

/// The `pd_status` events, with their simulated time.
fn status_batches(events: &Events) -> Vec<(u64, Vec<Value>)> {
    json_events(events)
        .into_iter()
        .filter(|(_, e)| e["type"] == "pd_status")
        .map(|(t, e)| {
            let samples = e["data"]["samples"]
                .as_array()
                .unwrap_or_else(|| panic!("pd_status without a samples array: {e}"))
                .clone();
            (t, samples)
        })
        .collect()
}

fn status_samples(events: &Events) -> Vec<Value> {
    status_batches(events)
        .into_iter()
        .flat_map(|(_, s)| s)
        .collect()
}

fn session_ms(v: &Value) -> u64 {
    v["session_ms"]
        .as_u64()
        .unwrap_or_else(|| panic!("no session_ms: {v}"))
}

fn named<'a>(rows: &'a [Value], name: &str) -> &'a Value {
    rows.iter()
        .find(|r| r["name"] == name)
        .unwrap_or_else(|| panic!("no {name} row in {rows:#?}"))
}

// ---- AC-TGE83D.1: PD rows on the chart's session time ----

#[test]
fn pd_rows_carry_their_time_on_the_measurement_samples_time_base() {
    let (_worker, _clock, events) = captured(1_000);
    let rows = rows(&events);
    assert!(!rows.is_empty(), "no PD rows");
    let times = sample_times(&events);
    let (first, last) = (
        *times.iter().min().expect("samples were sent"),
        *times.iter().max().expect("samples were sent"),
    );
    for r in &rows {
        let t = session_ms(r);
        assert!(
            t >= first && t <= last + 100,
            "row {} at session {t} ms lies outside the samples' {first}..={last} ms: {r}",
            r["index"]
        );
    }
    // The capture started after 1 s of measuring: session time is not the
    // time since the capture start.
    let attach = &rows[0];
    assert!(
        session_ms(attach) >= first + 900,
        "the attach sits at the session start, not 1 s into it: {attach} (first sample {first})"
    );
}

#[test]
fn session_time_keeps_the_meters_spacing_and_the_capture_offset() {
    let (_worker, _clock, events) = captured(500);
    let rows = rows(&events);
    let attach = &rows[0];
    let caps = named(&rows, "Source_Capabilities");
    assert_eq!(
        session_ms(caps) - session_ms(attach),
        280,
        "session time follows the meter's record times"
    );
    let offsets: Vec<i64> = rows
        .iter()
        .map(|r| session_ms(r) as i64 - r["elapsed_ms"].as_i64().expect("elapsed_ms"))
        .collect();
    assert!(
        offsets.windows(2).all(|w| w[0] == w[1]),
        "session_ms − elapsed_ms changes within one capture run: {offsets:?}"
    );
}

// ---- AC-TGE83D.4: the PD status blocks reach the UI ----

#[test]
fn pd_status_samples_reach_the_ui_on_the_session_time_base() {
    let (_worker, _clock, events) = captured(500);
    let samples = status_samples(&events);
    assert!(
        samples.len() > 10,
        "too few PD status samples: {}",
        samples.len()
    );
    let times = sample_times(&events);
    let (first, last) = (
        *times.iter().min().expect("samples"),
        *times.iter().max().expect("samples"),
    );
    let t: Vec<u64> = samples.iter().map(session_ms).collect();
    assert!(
        t.windows(2).all(|w| w[0] <= w[1]),
        "status times go backwards"
    );
    for (s, &ms) in samples.iter().zip(&t) {
        assert!(
            ms >= first && ms <= last + 100,
            "status at {ms} ms outside the samples' {first}..={last} ms: {s}"
        );
        for key in ["vbus_v", "ibus_a", "cc1_v", "cc2_v"] {
            assert!(
                s[key].is_f64() || s[key].is_i64() || s[key].is_u64(),
                "{key}: {s}"
            );
        }
    }
}

#[test]
fn pd_status_samples_carry_vbus_ibus_cc1_and_cc2_in_si_units() {
    let (_worker, _clock, events) = captured(500);
    let samples = status_samples(&events);
    let rows = rows(&events);
    let attach_ms = session_ms(&rows[0]);
    let after_attach: Vec<&Value> = samples
        .iter()
        .filter(|s| session_ms(s) > attach_ms + 10)
        .collect();
    assert!(!after_attach.is_empty(), "no status after the attach");
    // The sink's Rd pulls CC1 to ~1.528 V once attached; CC2 stays open.
    for s in &after_attach {
        let cc1 = s["cc1_v"].as_f64().expect("cc1_v");
        assert!((cc1 - 1.528).abs() < 0.002, "CC1 after attach: {s}");
        let cc2 = s["cc2_v"].as_f64().expect("cc2_v");
        assert!(cc2.abs() < 0.002, "CC2 after attach: {s}");
        assert!(s["ibus_a"].as_f64().is_some(), "ibus_a: {s}");
    }
    let vbus: Vec<f64> = samples
        .iter()
        .map(|s| s["vbus_v"].as_f64().expect("vbus_v"))
        .collect();
    assert!(
        vbus.iter().any(|v| (v - 5.0).abs() < 0.05),
        "no 5 V status during the SPR contract: {vbus:?}"
    );
    assert!(
        vbus.iter().any(|v| (v - 28.0).abs() < 0.05),
        "no 28 V status during the EPR contract: {vbus:?}"
    );
}

#[test]
fn pd_status_goes_out_in_batches_at_most_60_a_second_and_stops_with_the_capture() {
    let (mut worker, clock, events) = captured(200);
    let batches = status_batches(&events);
    assert!(
        batches.iter().all(|(_, s)| !s.is_empty()),
        "an empty pd_status batch"
    );
    assert!(
        batches.iter().any(|(_, s)| s.len() >= 2),
        "status blocks polled every 5 ms never shared a batch"
    );
    for w in batches.windows(2) {
        assert!(
            w[1].0 - w[0].0 >= 16,
            "pd_status batches {} ms apart (more than 60 a second)",
            w[1].0 - w[0].0
        );
    }
    worker.stop_pd().expect("PD capture stops");
    let after_stop = status_batches(&events).len();
    run_for(&mut worker, &clock, 500);
    assert_eq!(
        status_batches(&events).len(),
        after_stop,
        "PD status still sent after Stop"
    );
}

// ---- PD times follow the session timeline's offset ----

/// After a reconnect the meter's clock starts again at 0 while the session
/// timeline goes on, so the samples carry a non-zero offset: PD rows and
/// status blocks must carry the same offset, or the markers land far away
/// from the samples they belong to.
#[test]
fn pd_times_follow_the_session_offset_after_a_reconnect() {
    use open_powerz_desktop_lib::store::SessionStore;
    let clock = clock();
    let store = SessionStore::new();
    // The first connection measures for 1.5 s, then the meter is unplugged.
    {
        let pipe = Pipe::sim(
            Scenario::Load5v1a,
            FaultPlan::new(),
            &clock,
            PipeOpts::default(),
        );
        let (_events, sink) = capture(&clock);
        let mut worker =
            Worker::start_with_store(pipe, clock.clone(), config(), store.clone(), sink)
                .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
        run_for(&mut worker, &clock, 1_500);
        worker.stop();
    }
    clock.advance(std::time::Duration::from_secs(30));
    // A fresh simulator: its device clock starts again at zero.
    let dev = SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, clock.clone())
        .with_pd_scenario(PdScenario::Negotiation);
    let pipe = Pipe::new(dev, &clock, PipeOpts::default());
    let (events, sink) = capture(&clock);
    let mut worker = Worker::start_with_store(pipe, clock.clone(), config(), store, sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    run_for(&mut worker, &clock, 500);
    worker.start_pd().expect("PD capture starts");
    run_for(&mut worker, &clock, SCENARIO_MS);

    let times = sample_times(&events);
    let (first, last) = (
        *times.iter().min().expect("samples were sent"),
        *times.iter().max().expect("samples were sent"),
    );
    assert!(first > 30_000, "the reconnect left no offset: {first}");
    let rows = rows(&events);
    assert!(!rows.is_empty(), "no PD rows");
    for r in &rows {
        let t = session_ms(r);
        assert!(
            t >= first + 400 && t <= last + 100,
            "row {} at session {t} ms lies outside the samples' {first}..={last} ms: {r}",
            r["index"]
        );
    }
    let status = status_samples(&events);
    assert!(!status.is_empty(), "no PD status");
    for s in &status {
        let t = session_ms(s);
        assert!(
            t >= first + 400 && t <= last + 100,
            "status at {t} ms outside the samples' {first}..={last} ms: {s}"
        );
    }
}
