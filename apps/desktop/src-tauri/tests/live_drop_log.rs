//! AC-AXHK2W.8: app.log tells whether live batches drop at the worker's queue
//! limit in a real run. When the forwarder starts dropping live batches it
//! writes one line; when it delivers again it writes one line with the
//! spell's totals, and so does a session that ends while still dropping. A
//! run whose page keeps up writes neither. No line names a serial.
//!
//! The simulator's clock runs as fast as the worker polls, so a gate that
//! holds the first batch back fills the queue at once (as in
//! `slow_sink_history.rs`). Simulators only; every serial is synthetic.

#![allow(dead_code)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Condvar;

use common::*;
use open_powerz_desktop_lib::app_log::AppLog;

include!("common/two_meters.inc.rs");

/// Well past the worker's queue limit at one sample per batch.
const ENOUGH_SAMPLES: usize = 2_000;

/// Real time per pipe call in the keep-up run: 512 batches behind then
/// needs the forwarder starved for well over half a second.
const KEEP_UP_PACE: Duration = Duration::from_millis(1);

/// Enough samples for the keep-up run to deliver well over the queue
/// limit in batches (about five samples to a batch).
const KEEP_UP_SAMPLES: usize = 5_000;

/// The worker's queue limit for live batches (`MAX_QUEUED_BATCHES`).
const QUEUE_LIMIT: usize = 512;

const DROPPED: &str = "live batches dropped";
const RESUMED: &str = "live batches resumed";
const LOST: &str = "live batches lost at session end";

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
    /// A failed test must not leave the session stuck behind a shut gate.
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

fn log_path(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("open-powerz-livedrop-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("app.log")
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|s| s.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

fn lines_with(path: &Path, word: &str) -> Vec<String> {
    lines(path)
        .into_iter()
        .filter(|l| l.contains(word))
        .collect()
}

/// Waits for the first line with `word`.
fn wait_line(path: &Path, word: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(line) = lines_with(path, word).into_iter().next() {
            return line;
        }
        assert!(
            Instant::now() < deadline,
            "no {word:?} line in app.log: {:#?}",
            lines(path)
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn rig_logging_to(path: &Path) -> Rig {
    let mut rig = Rig::new();
    rig.m.set_app_log(AppLog::open(path).expect("open app.log"));
    rig
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

/// The numbers in `line` after its `word`, separators removed.
fn numbers_after(line: &str, word: &str) -> Vec<u64> {
    let tail = &line[line.find(word).expect("the word") + word.len()..];
    tail.split(|c: char| !(c.is_ascii_digit() || c == ','))
        .map(|s| s.replace(',', ""))
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().expect("a number"))
        .collect()
}

fn assert_no_serial(path: &Path) {
    for line in lines(path) {
        assert!(
            !line.contains(SERIAL_A) && !line.contains(SERIAL_B),
            "a line names a serial: {line}"
        );
    }
}

#[test]
fn ac_axhk2w_8_a_stalled_page_logs_one_drop_line_and_one_totals_line() {
    let path = log_path("stall");
    let mut rig = rig_logging_to(&path);
    let gate = Gate::default();
    gate.set_shut(true);
    let (rx, sink) = gated_sink(gate.clone());
    rig.m.connect(METER_A, 50, sink).expect("connect meter A");
    wait_for_history(&rig, ENOUGH_SAMPLES);
    let dropped = wait_line(&path, DROPPED);
    assert!(
        dropped.contains("512"),
        "the drop line names the queue limit: {dropped}"
    );

    gate.set_shut(false);
    let resumed = wait_line(&path, RESUMED);
    // A few more live batches after the totals: the page keeps up again.
    recv_until(&rx, is_samples);
    rig.m.disconnect_for(METER_A).unwrap();

    assert_eq!(lines_with(&path, DROPPED).len(), 1, "{:#?}", lines(&path));
    assert_eq!(lines_with(&path, RESUMED).len(), 1, "{:#?}", lines(&path));
    assert!(lines_with(&path, LOST).is_empty(), "{:#?}", lines(&path));
    let numbers = numbers_after(&resumed, RESUMED);
    assert!(
        numbers.len() >= 2 && numbers[0] > 0 && numbers[1] > 0,
        "the totals line counts batches and samples: {resumed}"
    );
    assert!(
        numbers[1] >= numbers[0],
        "every dropped batch held a sample: {resumed}"
    );
    assert_no_serial(&path);
}

#[test]
fn ac_axhk2w_8_a_session_that_ends_while_dropping_logs_its_totals() {
    let path = log_path("end");
    let mut rig = rig_logging_to(&path);
    let gate = Gate::default();
    gate.set_shut(true);
    let (_rx, sink) = gated_sink(gate.clone());
    rig.m.connect(METER_A, 50, sink).expect("connect meter A");
    wait_for_history(&rig, ENOUGH_SAMPLES);
    wait_line(&path, DROPPED);

    // The session ends with the page still stalled.
    rig.unplug_a();
    let lost = wait_line(&path, LOST);
    gate.set_shut(false);

    assert_eq!(lines_with(&path, DROPPED).len(), 1, "{:#?}", lines(&path));
    assert!(lines_with(&path, RESUMED).is_empty(), "{:#?}", lines(&path));
    assert_eq!(lines_with(&path, LOST).len(), 1, "{:#?}", lines(&path));
    let numbers = numbers_after(&lost, LOST);
    assert!(
        numbers.len() >= 2 && numbers[0] > 0 && numbers[1] > 0,
        "the totals line counts batches and samples: {lost}"
    );
    assert_no_serial(&path);
}

#[test]
fn ac_axhk2w_8_a_page_that_keeps_up_logs_no_drop_line() {
    let path = log_path("keeps-up");
    let mut rig = rig_logging_to(&path);
    // A page keeps up with a meter, not with a simulator that produces as
    // fast as it polls: unpaced, a starved forwarder thread on a loaded
    // runner falls the queue limit behind although the page never blocks.
    rig.pace_reads(KEEP_UP_PACE);
    let (rx, sink) = channel_sink();
    rig.m.connect(METER_A, 50, sink).expect("connect meter A");
    wait_for_history(&rig, KEEP_UP_SAMPLES);
    let mut delivered = recv_until(&rx, is_samples);
    rig.m.disconnect_for(METER_A).unwrap();
    delivered.extend(rx.try_iter());
    let batches = delivered.iter().filter(|e| is_samples(e)).count();
    assert!(
        batches > QUEUE_LIMIT,
        "only {batches} batches: the run must pass the queue limit to mean anything"
    );

    let live: Vec<String> = lines_with(&path, "live batches");
    assert!(live.is_empty(), "a page that kept up: {live:#?}");
}
