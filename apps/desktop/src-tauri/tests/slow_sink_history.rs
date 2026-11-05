//! A sink too slow for the live batches (AC-AXHK2M.9, AC-AXHK2M.10): the
//! worker drops batches past its queue limit, and that costs the live view
//! only. The outage gap a session sends comes out the same as the history's,
//! and the history, the export and a recording keep every sample.
//!
//! The simulator's clock runs as fast as the worker polls, so a gate that
//! holds the first batch back fills the queue at once; each test checks that
//! batches really were dropped. Simulators only; every serial is synthetic.

#![allow(dead_code)]

mod common;

use common::*;
use open_powerz_desktop_lib::recording::RecordingEvent;
use open_powerz_desktop_lib::worker::GapKind;
use std::sync::Condvar;

include!("common/two_meters.inc.rs");

/// One sample per queue poll (every 100 ms) and so one per batch: each
/// sample costs a batch now that idle heartbeats come only every 250 ms
/// (FR HR2JAG), where 50 sps packed five samples into each.
const RATE_SPS: u32 = 10;
/// Well past the worker's queue limit at one sample per batch.
const ENOUGH_SAMPLES: usize = 2_000;
/// How long the worker gets to meet the unplug, on real time.
const MEET_THE_UNPLUG: Duration = Duration::from_millis(200);

/// Holds batches back while it is shut.
#[derive(Clone, Default)]
struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    fn set_shut(&self, shut: bool) {
        *self.0.0.lock().unwrap() = shut;
        self.0.1.notify_all();
    }

    fn wait(&self) {
        let mut shut = self.0.0.lock().unwrap();
        while *shut {
            shut = self.0.1.wait(shut).unwrap();
        }
    }
}

impl Drop for Gate {
    /// A failed test must not leave the session stuck behind a shut gate:
    /// dropping the manager waits for the worker.
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.set_shut(false);
        }
    }
}

/// A sink that blocks on `gate` for every batch, as a throttled page does.
fn gated_sink(
    gate: Gate,
) -> (
    mpsc::Receiver<BackendEvent>,
    impl FnMut(BackendEvent) + Send + 'static,
) {
    let (tx, rx) = mpsc::channel();
    (rx, move |e: BackendEvent| {
        if matches!(e, BackendEvent::Batch(_)) {
            gate.wait();
        }
        let _ = tx.send(e);
    })
}

/// Wait until meter A's history holds `want` samples.
fn wait_for_history(rig: &Rig, want: usize) {
    let store = rig.m.session_store_for(METER_A).expect("the session store");
    let deadline = Instant::now() + Duration::from_secs(30);
    while store.history_len() < want {
        assert!(
            Instant::now() < deadline,
            "only {} samples of {want}",
            store.history_len()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn sample_count(events: &[BackendEvent]) -> usize {
    events
        .iter()
        .map(|e| match e {
            BackendEvent::Batch(b) => b.samples.len(),
            _ => 0,
        })
        .sum()
}

#[test]
fn the_outage_gap_is_the_history_gap_after_dropped_batches() {
    let mut rig = Rig::new();
    let gate = Gate::default();
    gate.set_shut(true);
    let (rx, sink) = gated_sink(gate.clone());
    rig.m
        .connect(METER_A, RATE_SPS, sink)
        .expect("connect meter A");
    wait_for_history(&rig, ENOUGH_SAMPLES);

    // The last batches before the unplug go the way of the others.
    rig.unplug_a();
    std::thread::sleep(MEET_THE_UNPLUG);
    gate.set_shut(false);
    let before = recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Waiting { .. }))
    });
    let store = rig.m.session_store_for(METER_A).expect("the session store");
    assert!(
        sample_count(&before) < store.history_len(),
        "no batch was dropped: {} of {} samples arrived",
        sample_count(&before),
        store.history_len()
    );

    rig.clock.advance(Duration::from_secs(5));
    assert!(rig.replug_a(), "no reconnect");
    let after = recv_until(&rx, is_samples);
    let gap = after
        .iter()
        .find_map(|e| match e {
            BackendEvent::Gap(g) if g.kind == GapKind::Outage => Some(g.clone()),
            _ => None,
        })
        .expect("no outage gap after the reconnect");
    let gaps = rig.m.history_gaps(0, u64::MAX).expect("history_gaps");
    assert_eq!(gaps.len(), 1, "one unplug, one gap: {gaps:?}");
    assert_eq!(
        (gap.from_ms, gap.to_ms),
        (gaps[0].from_ms, gaps[0].to_ms),
        "the gap sent is not the history's"
    );
    rig.m.disconnect_for(METER_A).unwrap();
}

#[test]
fn no_held_samples_arrive_after_the_event_that_follows_them() {
    let mut rig = Rig::new();
    let gate = Gate::default();
    let (rx, sink) = gated_sink(gate.clone());
    rig.m
        .connect(METER_A, RATE_SPS, sink)
        .expect("connect meter A");
    recv_until(&rx, is_samples);
    let dir = temp_dir("order");
    rig.m
        .start_recording_for(METER_A, &dir.join("order.csv"))
        .expect("start recording");
    let store = rig.m.session_store_for(METER_A).expect("the session store");
    let started = store.history_len();

    gate.set_shut(true);
    wait_for_history(&rig, started + ENOUGH_SAMPLES);
    rig.unplug_a();
    std::thread::sleep(MEET_THE_UNPLUG);
    gate.set_shut(false);

    let events = recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
    let stopped = events
        .iter()
        .position(|e| matches!(e, BackendEvent::Recording(RecordingEvent::Stopped(_))))
        .expect("the unplug stops the recording");
    assert_eq!(
        sample_count(&events[stopped..]),
        0,
        "samples came after the recording's end"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A fresh temp folder for this test.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("slow-sink-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Data rows of a CSV: every line after the header.
fn data_rows(path: &std::path::Path) -> usize {
    let text = std::fs::read_to_string(path).expect("read the CSV");
    text.lines().skip(1).filter(|l| !l.is_empty()).count()
}

#[test]
fn a_dropped_batch_costs_no_sample_in_the_history_the_export_or_a_recording() {
    let mut rig = Rig::new();
    let gate = Gate::default();
    let (rx, sink) = gated_sink(gate.clone());
    rig.m
        .connect(METER_A, RATE_SPS, sink)
        .expect("connect meter A");
    let first = recv_until(&rx, is_samples);
    let store = rig.m.session_store_for(METER_A).expect("the session store");

    let dir = temp_dir("export");
    let recording = dir.join("recording.csv");
    let before_start = store.history_len();
    rig.m
        .start_recording_for(METER_A, &recording)
        .expect("start recording");
    let after_start = store.history_len();

    gate.set_shut(true);
    wait_for_history(&rig, after_start + ENOUGH_SAMPLES);
    gate.set_shut(false);
    let before_stop = store.history_len();
    let summary = rig.m.stop_recording_for(METER_A).expect("stop recording");
    let after_stop = store.history_len();

    rig.m.disconnect_for(METER_A).unwrap();
    let events = recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });

    let history = store.history_len();
    let arrived = sample_count(&first) + sample_count(&events);
    assert!(
        arrived < history,
        "no batch was dropped: {arrived} of {history} samples arrived"
    );
    let export = dir.join("export.csv");
    rig.m
        .export_session(&export, |_| {})
        .expect("export refused")
        .join()
        .unwrap()
        .expect("export failed");
    assert_eq!(data_rows(&export), history, "the export lost samples");

    let rows = data_rows(&recording);
    assert_eq!(rows as u64, summary.rows, "the summary and the file differ");
    assert!(
        (before_stop - after_start..=after_stop - before_start).contains(&rows),
        "the recording holds {rows} rows of {}..={} samples",
        before_stop - after_start,
        after_stop - before_start
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_docs_say_what_a_dropped_batch_costs() {
    let doc = include_str!("../../../../docs/reference/desktop-backend.md");
    let text = doc.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        text.contains("slow sink") || text.contains("slow page"),
        "the reference does not mention the slow-sink drop"
    );
    assert!(
        text.contains("lost to the live view") || text.contains("only delayed"),
        "the reference does not say what the dropped samples cost the live view"
    );
    assert!(
        text.contains("history, the export and a recording"),
        "the reference does not say what keeps every sample"
    );
}
