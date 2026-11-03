//! The backend's PD status log is capped (FR AXHK2W, AC .2 and .3): a long
//! capture keeps at most `MAX_STATUS_SAMPLES` status blocks, drops the oldest
//! first and counts them, and that count reaches the page: on every
//! `pd_status` batch and in the attach snapshot of a page that comes later.
//!
//! API pinned here:
//!
//! - `pd_capture::MAX_STATUS_SAMPLES`: status blocks a capture keeps
//!   (100,000, the same as `MAX_RECORDS`).
//! - `PdCapture::status_dropped(&self) -> u64`: status blocks dropped past
//!   the limit since the last clear.
//! - `WorkerConfig::with_pd_status_limit(self, n) -> WorkerConfig`: a worker
//!   whose capture keeps at most `n` status blocks (tests use a small one).
//! - `Worker::pd_status_dropped(&self) -> u64`: its capture's count.
//! - `{ "type": "pd_status", "data": { "samples": [...], "dropped": n } }`:
//!   `dropped` is the backend's count since the last clear.
//! - `AttachedSession::pd_status_dropped: u64` (JSON `pd_status_dropped`).
//!
//! Every value is synthetic (the simulator's PD scenario, demo meters only).

mod common;

use std::sync::mpsc;
use std::time::Duration;

use common::*;
use open_powerz_desktop_lib::devices::{AppOpener, DeviceManager};
use open_powerz_desktop_lib::pd_capture::{MAX_RECORDS, MAX_STATUS_SAMPLES, PdCapture};
use open_powerz_desktop_lib::worker::{BackendEvent, Status, Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_protocol::decode::pd_packet::{PdMessage, PdRecord, PdRecordKind, PdStatus};
use powerz_protocol::pdcap::Entry;
use powerz_protocol::session::{CapturedRecord, SystemClock};
use powerz_sim::PdScenario;
use serde_json::Value;

/// A status block at meter time `time_ms`.
fn status(time_ms: u32) -> PdStatus {
    PdStatus {
        time_ms,
        vbus_mv: 5_000,
        ibus_ma: 100,
        cc1_mv: 1_600,
        cc2_mv: 0,
    }
}

/// An Accept message (rev 3, source) at `elapsed_ms`.
fn accept(elapsed_ms: u64) -> CapturedRecord {
    CapturedRecord {
        elapsed_ms,
        record: PdRecord {
            time_ms: elapsed_ms as u32,
            from_meter: false,
            raw: Vec::new(),
            kind: PdRecordKind::Message(PdMessage {
                sop: 0,
                header: 0x01A3,
                body: Vec::new(),
            }),
        },
    }
}

/// Session times of the status blocks the capture keeps (what a save writes).
fn kept_status(pd: &PdCapture) -> Vec<u64> {
    pd.to_pdcap()
        .iter()
        .filter_map(|e| match e {
            Entry::Status { session_ms, .. } => Some(*session_ms),
            Entry::Record { .. } => None,
        })
        .collect()
}

/// A capture fed `n` status blocks (no records), each batch taken as the
/// worker would take it.
fn fed(n: u32) -> PdCapture {
    let mut pd = PdCapture::new();
    for t in 0..n {
        pd.push(&[], Some(status(t)));
        if t % 1_000 == 0 {
            let _ = pd.take_all_status();
        }
    }
    let _ = pd.take_all_status();
    pd
}

// ---- AC-AXHK2W.2: the backend status log is capped ----

#[test]
fn ac_axhk2w_2_the_status_cap_is_100k_like_the_record_cap() {
    assert_eq!(MAX_STATUS_SAMPLES, 100_000);
    assert_eq!(MAX_STATUS_SAMPLES, MAX_RECORDS);
}

#[test]
fn ac_axhk2w_2_within_the_cap_nothing_is_dropped() {
    let pd = fed(1_000);
    assert_eq!(pd.status_dropped(), 0);
    assert_eq!(kept_status(&pd).len(), 1_000);
}

#[test]
fn ac_axhk2w_2_past_the_cap_the_oldest_status_blocks_go_first_and_are_counted() {
    let extra = 7u32;
    let pd = fed(100_000 + extra);
    let kept = kept_status(&pd);
    assert_eq!(kept.len(), MAX_STATUS_SAMPLES, "the log holds the cap");
    assert_eq!(pd.status_dropped(), u64::from(extra));
    // Oldest first: the kept ones are the newest, still in time order.
    assert_eq!(kept.first().copied(), Some(u64::from(extra)));
    assert_eq!(kept.last().copied(), Some(u64::from(100_000 + extra - 1)));
    assert!(
        kept.windows(2).all(|w| w[0] < w[1]),
        "the kept times stay in order"
    );
}

#[test]
fn ac_axhk2w_2_dropping_status_blocks_keeps_every_record() {
    let mut pd = PdCapture::new();
    pd.push(&[accept(0)], Some(status(0)));
    for t in 1..=100_010u32 {
        pd.push(&[], Some(status(t)));
    }
    let _ = pd.take_all_status();
    assert_eq!(kept_status(&pd).len(), MAX_STATUS_SAMPLES);
    assert_eq!(pd.dropped(), 0, "no record was dropped");
    assert!(pd.details(0).is_some(), "the record keeps its details");
    let records = pd
        .to_pdcap()
        .iter()
        .filter(|e| matches!(e, Entry::Record { .. }))
        .count();
    assert_eq!(records, 1, "the save still has the record");
}

#[test]
fn ac_axhk2w_2_status_blocks_dropped_with_a_record_are_counted() {
    let mut pd = PdCapture::with_limit(2);
    for t in 0..5u32 {
        pd.push(&[accept(u64::from(t) * 10)], Some(status(t * 10)));
        let _ = pd.take_all_status();
    }
    assert_eq!(pd.dropped(), 3, "three records went past the cap of two");
    let lost = 5 - kept_status(&pd).len() as u64;
    assert!(
        lost > 0,
        "status blocks before the oldest kept record went with it"
    );
    assert_eq!(
        pd.status_dropped(),
        lost,
        "every status block lost is counted"
    );
}

#[test]
fn ac_axhk2w_2_a_clear_resets_the_status_drop_count() {
    let mut pd = fed(100_003);
    assert_eq!(pd.status_dropped(), 3);
    pd.clear();
    assert_eq!(pd.status_dropped(), 0);
    assert!(kept_status(&pd).is_empty());
}

// ---- AC-AXHK2W.2 / .3: the count reaches the page ----

/// The PD scenario plays for ~3.4 s from the sniffer start.
const SCENARIO_MS: u64 = 3_800;

/// Status blocks the small worker capture keeps.
const SMALL_LIMIT: usize = 3;

/// A worker on the simulator's PD scenario whose capture keeps at most
/// `SMALL_LIMIT` status blocks, captured for the whole scenario.
fn captured_small() -> (Worker<Pipe, SimClock>, Events) {
    let clock = clock();
    let dev = SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, clock.clone())
        .with_pd_scenario(PdScenario::Negotiation);
    let pipe = Pipe::new(dev, &clock, PipeOpts::default());
    let (events, sink) = capture(&clock);
    let config = WorkerConfig::new(Rate::Sps50, 1).with_pd_status_limit(SMALL_LIMIT);
    let mut worker = Worker::start(pipe, clock.clone(), config, sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    run_for(&mut worker, &clock, 500);
    worker.start_pd().expect("PD capture starts");
    run_for(&mut worker, &clock, SCENARIO_MS);
    (worker, events)
}

/// The `pd_status` events as the UI receives them (JSON).
fn status_events(events: &Events) -> Vec<Value> {
    events
        .lock()
        .unwrap()
        .iter()
        .map(|(_, e)| serde_json::to_value(e).expect("event serializes"))
        .filter(|e| e["type"] == "pd_status")
        .collect()
}

#[test]
fn ac_axhk2w_2_every_status_batch_says_how_many_the_backend_dropped() {
    let (worker, events) = captured_small();
    let batches = status_events(&events);
    let sent: usize = batches
        .iter()
        .map(|e| e["data"]["samples"].as_array().map_or(0, Vec::len))
        .sum();
    assert!(
        sent > SMALL_LIMIT,
        "the scenario sends more status blocks ({sent}) than the small limit"
    );
    let counts: Vec<u64> = batches
        .iter()
        .map(|e| {
            e["data"]["dropped"]
                .as_u64()
                .unwrap_or_else(|| panic!("pd_status without a dropped count: {e}"))
        })
        .collect();
    assert!(
        counts.windows(2).all(|w| w[0] <= w[1]),
        "the count only grows: {counts:?}"
    );
    let expected = (sent - SMALL_LIMIT) as u64;
    assert_eq!(counts.last().copied(), Some(expected));
    assert_eq!(worker.pd_status_dropped(), expected);
}

// ---- AC-AXHK2W.3: a page that attaches later learns of the drop ----

type Manager = DeviceManager<AppOpener, SystemClock>;

#[test]
fn ac_axhk2w_3_the_attach_snapshot_carries_the_status_drop_count() {
    let mut m: Manager = DeviceManager::new(|| Ok(Vec::new()), AppOpener, SystemClock);
    let id = "demo:pd-9v-2a";
    let (tx, rx) = mpsc::channel();
    m.connect(id, 50, move |event: BackendEvent| {
        if matches!(event, BackendEvent::Status(Status::Connected(_))) {
            let _ = tx.send(());
        }
    })
    .expect("the demo meter connects");
    rx.recv_timeout(Duration::from_secs(10))
        .expect("the demo meter reports connected");
    let snapshot = m
        .attacher_for(id)
        .expect("a running session")
        .attach(|_: BackendEvent| {})
        .expect("a snapshot");
    assert_eq!(
        snapshot.pd_status_dropped, 0,
        "a fresh capture dropped nothing"
    );
    let json = serde_json::to_value(&snapshot).expect("the snapshot serializes");
    assert_eq!(
        json["pd_status_dropped"], 0,
        "the page reads it as JSON: {json}"
    );
    for id in m.connected_ids() {
        let _ = m.disconnect_for(&id);
    }
}
