//! Fixes from the first real-meter QA run (FR X2SPDW): lossless streaming
//! with a slow UI, recording gaps, one Unix anchor per session, the TEMP
//! column, error hints and a queue left running. CONNECT recovery and the
//! shutdown on quit live in `connect_recovery.rs` and `shutdown.rs`. Every
//! identifier is synthetic.

mod common;

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::slow_sink::{self, Outcome, RecordingClock};
use common::*;
use open_powerz_desktop_lib::store::SessionStore;
use open_powerz_desktop_lib::ui_error;
use open_powerz_desktop_lib::worker::{self, BackendEvent, Status, Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_protocol::transport;

fn config(rate: Rate) -> WorkerConfig {
    WorkerConfig {
        rate,
        session_id: 1,
    }
}

fn start(
    rate: Rate,
    opts: PipeOpts,
) -> (Worker<Pipe, SimClock>, SimClock, Arc<Mutex<Log>>, Events) {
    let clock = clock();
    let pipe = Pipe::sim(Scenario::Load5v1a, FaultPlan::new(), &clock, opts);
    let log = Arc::clone(&pipe.log);
    let (events, sink) = capture(&clock);
    let worker = Worker::start(pipe, clock.clone(), config(rate), sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    (worker, clock, log, events)
}

fn connected(events: &Events) -> worker::ConnectedInfo {
    statuses(events)
        .into_iter()
        .find_map(|s| match s {
            Status::Connected(info) => Some(info),
            _ => None,
        })
        .expect("no connected status")
}

fn types(log: &Arc<Mutex<Log>>) -> Vec<u8> {
    log.lock()
        .unwrap()
        .frames
        .iter()
        .map(|(_, f)| f[0] & 0x7F)
        .collect()
}

/// An in-memory file shared with the test.
#[derive(Clone, Default)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl Write for SharedBuf {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl SharedBuf {
    /// Data rows (header dropped), split into columns.
    fn rows(&self) -> Vec<Vec<String>> {
        let text = String::from_utf8(self.0.lock().unwrap().clone()).unwrap();
        text.lines()
            .skip(1)
            .map(|l| l.split(',').map(str::to_owned).collect())
            .collect()
    }
}

const UNIX: usize = 1;
const TEMP: usize = 8;

/// Export the worker's session now and return its rows.
fn export_rows(worker: &mut Worker<Pipe, SimClock>) -> Vec<Vec<String>> {
    let buf = SharedBuf::default();
    worker
        .export(Box::new(buf.clone()), |_| {})
        .join()
        .expect("export thread")
        .expect("export");
    buf.rows()
}

/// Rows of `export` that hold the same samples as `recording`: the recording
/// and the export end at the same record, so they line up from the end.
fn aligned<'a>(
    recording: &'a [Vec<String>],
    export: &'a [Vec<String>],
) -> Vec<(&'a Vec<String>, &'a Vec<String>)> {
    assert!(!recording.is_empty(), "the recording has no rows");
    assert!(
        export.len() >= recording.len(),
        "the export ({}) has fewer rows than the recording ({})",
        export.len(),
        recording.len()
    );
    let offset = export.len() - recording.len();
    let pairs: Vec<_> = recording.iter().zip(&export[offset..]).collect();
    for (r, e) in &pairs {
        // Same sample: VBUS, IBUS, DP, DM, CC1, CC2 agree.
        assert_eq!(r[2..8], e[2..8], "rows do not line up: {r:?} vs {e:?}");
    }
    pairs
}

fn unix_ms(row: &[String]) -> u64 {
    let s: f64 = row[UNIX]
        .parse()
        .unwrap_or_else(|_| panic!("bad Unix {row:?}"));
    (s * 1000.0).round() as u64
}

// ---- AC-X2SPDW.1: a slow event sink loses no record ----

#[test]
fn a_sink_that_blocks_200_ms_per_event_loses_no_record_at_1000_sps() {
    // Real time: the simulator keeps only its latest 63 records (63 ms at
    // 1000 SPS), like the meter.
    let pipe_log: Arc<Mutex<Option<Arc<Mutex<Log>>>>> = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&pipe_log);
    let open = move || {
        let dev = SimDevice::new(Persona::Bulk, Scenario::Load5v1a);
        let pipe = Pipe::new(dev, &clock(), PipeOpts::default());
        *slot.lock().unwrap() = Some(Arc::clone(&pipe.log));
        Ok(pipe)
    };
    let blocking = Arc::new(AtomicBool::new(true));
    let times: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));
    let gaps = Arc::new(Mutex::new(Vec::new()));
    let (done_tx, done_rx) = mpsc::channel();
    let (b, t, g) = (Arc::clone(&blocking), Arc::clone(&times), Arc::clone(&gaps));
    let sink = move |event: BackendEvent| {
        if b.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(200));
        }
        match event {
            BackendEvent::Batch(batch) => t
                .lock()
                .unwrap()
                .extend(batch.samples.iter().map(|s| s.t_ms)),
            BackendEvent::Gap(gap) => g.lock().unwrap().push(gap),
            BackendEvent::Status(Status::Disconnected { error }) => {
                let _ = done_tx.send(error);
            }
            _ => {}
        }
    };
    let store = SessionStore::new();
    // AC-KQF5GM.1: the system clock, with every sleep logged for the failure message.
    let clock = RecordingClock::new();
    let handle = worker::spawn_with_store(
        open,
        clock.clone(),
        config(Rate::Sps1000),
        store.clone(),
        sink,
        Arc::new(AtomicBool::new(false)),
    );
    std::thread::sleep(Duration::from_secs(10));
    blocking.store(false, Ordering::SeqCst);
    handle.disconnect();
    let error = done_rx
        .recv_timeout(Duration::from_secs(60))
        .expect("the disconnected status never reached the sink");
    assert_eq!(error, None, "the session ended with an error");

    let log = pipe_log.lock().unwrap().clone().expect("pipe never opened");
    let delivered = log.lock().unwrap().records_delivered;
    let gaps = gaps.lock().unwrap();
    let times = times.lock().unwrap();
    let holes: Vec<_> = times.windows(2).filter(|w| w[1] - w[0] != 1).collect();
    let outcome = Outcome {
        gaps: gaps.len(),
        holes: holes.len(),
        samples: times.len(),
        delivered,
        history: store.history_len(),
    };
    // AC-KQF5GM.3: without CI, zero gaps, zero holes and every delivered record.
    // AC-KQF5GM.2: on CI, only what the measured oversleep explains.
    let ci = std::env::var("CI").ok();
    let evidence = clock.evidence();
    if let Err(message) = slow_sink::check(
        &outcome,
        &slow_sink::tolerance_for(ci.as_deref(), &evidence),
        &evidence,
    ) {
        panic!(
            "{message}; first gaps {:?}, first hole {:?}",
            &gaps[..gaps.len().min(5)],
            holes.first()
        );
    }
}

// ---- AC-X2SPDW.4: the summary counts only gaps after the first row ----

#[test]
fn a_gap_that_ends_at_the_first_row_is_not_counted() {
    let (mut worker, clock, _log, events) = start(
        Rate::Sps1000,
        PipeOpts {
            stall_at_poll: Some((2, 200)),
            ..PipeOpts::default()
        },
    );
    // Poll 1 runs before the recording starts; the stall before poll 2
    // leaves a gap that ends at the recording's first row.
    worker.step();
    let file = SharedBuf::default();
    worker.start_recording(Box::new(file.clone())).unwrap();
    run_for(&mut worker, &clock, 1_000);
    let summary = worker.stop_recording().unwrap();
    let stream_gaps = events
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, e)| matches!(e, BackendEvent::Gap(_)))
        .count();
    assert!(stream_gaps >= 1, "the stall left no gap at all");
    assert!(
        summary.gaps.is_empty(),
        "a gap before the first row was counted: {:?}",
        summary.gaps
    );
    assert!(summary.rows > 0);
}

#[test]
fn a_gap_after_the_first_row_is_counted() {
    let (mut worker, clock, _log, _events) = start(
        Rate::Sps1000,
        PipeOpts {
            stall_at_poll: Some((3, 200)),
            ..PipeOpts::default()
        },
    );
    worker.step();
    let file = SharedBuf::default();
    worker.start_recording(Box::new(file.clone())).unwrap();
    run_for(&mut worker, &clock, 1_000);
    let summary = worker.stop_recording().unwrap();
    assert_eq!(summary.gaps.len(), 1, "{:?}", summary.gaps);
}

// ---- AC-X2SPDW.5: the same Unix time in a recording and an export ----

fn assert_same_unix_times(clear_first: bool) {
    let (mut worker, clock, _log, _events) = start(Rate::Sps1000, PipeOpts::default());
    run_for(&mut worker, &clock, 1_000);
    if clear_first {
        worker.clear_history();
        // The first poll after the clear finds a 40 ms backlog (no gap), so
        // the history restarts at a record well before that poll.
        clock.advance(Duration::from_millis(40));
        run_for(&mut worker, &clock, 300);
    }
    let file = SharedBuf::default();
    worker.start_recording(Box::new(file.clone())).unwrap();
    run_for(&mut worker, &clock, 2_000);
    worker.stop_recording().unwrap();
    let recording = file.rows();
    let export = export_rows(&mut worker);
    for (r, e) in aligned(&recording, &export) {
        assert_eq!(
            unix_ms(r),
            unix_ms(e),
            "one sample, two Unix times: recording {} vs export {}",
            r[UNIX],
            e[UNIX]
        );
    }
}

#[test]
fn a_sample_has_the_same_unix_time_in_a_recording_and_an_export() {
    assert_same_unix_times(false);
}

#[test]
fn a_sample_has_the_same_unix_time_after_the_history_was_cleared() {
    assert_same_unix_times(true);
}

// ---- AC-X2SPDW.7: the TEMP column ----

fn temps(rows: &[Vec<String>]) -> Vec<Option<f64>> {
    rows.iter()
        .map(|r| {
            let t = r[TEMP].trim();
            (!t.is_empty()).then(|| t.parse().unwrap_or_else(|_| panic!("bad TEMP {r:?}")))
        })
        .collect()
}

/// TEMP is empty only before the first snapshot, then always a plausible
/// value; rows from 1 s on all carry one.
fn assert_temp_filled(rows: &[Vec<String>], what: &str) {
    let t = temps(rows);
    let first = t
        .iter()
        .position(Option::is_some)
        .unwrap_or_else(|| panic!("{what}: TEMP is empty in every row"));
    assert!(
        t[first..].iter().all(Option::is_some),
        "{what}: TEMP went empty again after it was filled"
    );
    assert!(
        first < 1_000,
        "{what}: TEMP empty for the first {first} rows"
    );
    for v in t.iter().flatten() {
        assert!(
            (28.0..=32.0).contains(v),
            "{what}: TEMP {v} is not the meter's"
        );
    }
}

#[test]
fn recordings_and_exports_write_the_latest_snapshot_temperature() {
    let (mut worker, clock, _log, _events) = start(Rate::Sps1000, PipeOpts::default());
    let file = SharedBuf::default();
    worker.start_recording(Box::new(file.clone())).unwrap();
    run_for(&mut worker, &clock, 3_000);
    worker.stop_recording().unwrap();
    let recording = file.rows();
    let export = export_rows(&mut worker);
    assert_temp_filled(&recording, "recording");
    assert_temp_filled(&export, "export");
    for (r, e) in aligned(&recording, &export) {
        assert_eq!(
            r[TEMP], e[TEMP],
            "TEMP differs for one sample: {r:?} vs {e:?}"
        );
    }
}

#[test]
fn an_invalid_snapshot_keeps_the_latest_valid_temperature() {
    let (mut worker, clock, _log, _events) = start(
        Rate::Sps1000,
        PipeOpts {
            invalid_temp_from_snapshot: Some(3),
            ..PipeOpts::default()
        },
    );
    let file = SharedBuf::default();
    worker.start_recording(Box::new(file.clone())).unwrap();
    // Within 2 s of the last valid snapshot (AC-CWWKJ6.4 empties TEMP later).
    run_for(&mut worker, &clock, 2_000);
    worker.stop_recording().unwrap();
    assert_temp_filled(&file.rows(), "recording");
    assert_temp_filled(&export_rows(&mut worker), "export");
}

#[test]
fn temp_stays_empty_while_the_meter_reports_none() {
    let (mut worker, clock, _log, _events) = start(
        Rate::Sps1000,
        PipeOpts {
            invalid_temp: true,
            ..PipeOpts::default()
        },
    );
    let file = SharedBuf::default();
    worker.start_recording(Box::new(file.clone())).unwrap();
    run_for(&mut worker, &clock, 2_000);
    worker.stop_recording().unwrap();
    let recording = file.rows();
    assert!(!recording.is_empty());
    assert!(
        temps(&recording).iter().all(Option::is_none),
        "TEMP without a valid snapshot"
    );
    let export = export_rows(&mut worker);
    assert!(
        temps(&export).iter().all(Option::is_none),
        "TEMP without a valid snapshot"
    );
}

// ---- AC-CWWKJ6.4: TEMP is never older than 2 s ----

/// Simulated times of the ADC snapshot requests the pipe saw, in order.
fn snapshot_times(log: &Arc<Mutex<Log>>) -> Vec<u64> {
    use powerz_protocol::frame::{self, Attribute};
    log.lock()
        .unwrap()
        .frames
        .iter()
        .filter(|(_, f)| frame::get_data_mask(f).is_some_and(|m| m.bits() == Attribute::ADC.bits()))
        .map(|&(t, _)| t)
        .collect()
}

/// Rows up to 2 s after `last_valid_ms` (the last valid snapshot) keep its
/// temperature; rows more than 2 s after it have an empty TEMP. `slack_ms`
/// covers the queue latency between a record's time and its arrival.
fn assert_temp_ages_out(rows: &[Vec<String>], first_valid_ms: u64, last_valid_ms: u64, what: &str) {
    const SLACK_MS: u64 = 250;
    let bound = last_valid_ms + 2_000;
    let mut late = 0;
    for row in rows {
        let t = unix_ms(row);
        let temp = row[TEMP].trim();
        if t > bound + SLACK_MS {
            late += 1;
            assert!(
                temp.is_empty(),
                "{what}: TEMP {temp} at {} ms, more than 2 s after the last valid snapshot at {} ms",
                t - T0,
                last_valid_ms - T0
            );
        } else if t > first_valid_ms + SLACK_MS && t + SLACK_MS < bound {
            assert!(
                !temp.is_empty(),
                "{what}: TEMP empty at {} ms, within 2 s of the valid snapshot at {} ms",
                t - T0,
                last_valid_ms - T0
            );
        }
    }
    assert!(late > 100, "{what}: only {late} rows after the 2 s bound");
}

#[test]
fn temp_goes_empty_more_than_2_s_after_the_last_valid_snapshot() {
    let (mut worker, clock, log, _events) = start(
        Rate::Sps1000,
        PipeOpts {
            invalid_temp_from_snapshot: Some(3),
            ..PipeOpts::default()
        },
    );
    let file = SharedBuf::default();
    worker.start_recording(Box::new(file.clone())).unwrap();
    run_for(&mut worker, &clock, 6_000);
    worker.stop_recording().unwrap();
    let snaps = snapshot_times(&log);
    assert!(snaps.len() >= 3, "only {} snapshots", snaps.len());
    let (first_valid, last_valid) = (snaps[0], snaps[1]);
    assert_temp_ages_out(&file.rows(), first_valid, last_valid, "recording");
    assert_temp_ages_out(&export_rows(&mut worker), first_valid, last_valid, "export");
}

#[test]
fn temp_stays_filled_while_valid_snapshots_keep_coming() {
    // Fresh snapshots every half second: no row is ever 2 s past one.
    let (mut worker, clock, _log, _events) = start(Rate::Sps1000, PipeOpts::default());
    let file = SharedBuf::default();
    worker.start_recording(Box::new(file.clone())).unwrap();
    run_for(&mut worker, &clock, 6_000);
    worker.stop_recording().unwrap();
    assert_temp_filled(&file.rows(), "recording");
    assert_temp_filled(&export_rows(&mut worker), "export");
}

// ---- AC-X2SPDW.8: commands and paths in hints are quoted for monospace ----

#[test]
fn linux_hints_quote_their_commands_and_paths() {
    let perm = ui_error::from_transport(&transport::Error::Permission("EACCES".into()), "linux");
    let hint = perm.hint.expect("no permission hint");
    for quoted in [
        "`sudo cp packaging/linux/60-powerz.rules /etc/udev/rules.d/`",
        "`sudo udevadm control --reload-rules`",
        "`sudo udevadm trigger`",
        "`docs/linux.md`",
    ] {
        assert!(hint.contains(quoted), "{quoted} not quoted in: {hint}");
    }
    assert!(!hint.starts_with('`'), "the prose is quoted too: {hint}");

    let busy = ui_error::from_transport(
        &transport::Error::Io("Device or resource busy".into()),
        "linux",
    );
    let hint = busy.hint.expect("no busy hint");
    for quoted in ["`sudo modprobe -r powerz`", "`docs/linux.md`"] {
        assert!(hint.contains(quoted), "{quoted} not quoted in: {hint}");
    }
}

#[test]
fn hints_without_commands_have_no_quotes() {
    for os in ["macos", "windows"] {
        let e = ui_error::from_transport(&transport::Error::Permission("denied".into()), os);
        let hint = e.hint.expect("no hint");
        assert!(!hint.contains('`'), "{os}: {hint}");
    }
    let e = ui_error::from_transport(&transport::Error::Timeout, "linux");
    assert!(!e.hint.unwrap_or_default().contains('`'));
}

// ---- AC-X2SPDW.10: a queue left running by an earlier session ----

#[test]
fn a_queue_left_running_is_stopped_and_started_again_once() {
    let (mut worker, clock, log, events) = start(
        Rate::Sps1000,
        PipeOpts {
            stale_stream: true,
            ..PipeOpts::default()
        },
    );
    let info = connected(&events);
    assert!(!info.rate_locked, "the rate was locked by the stale stream");
    assert_eq!(info.rate_sps, Some(1000));
    let t = types(&log);
    let starts: Vec<usize> = t
        .iter()
        .enumerate()
        .filter(|(_, x)| **x == 0x0E)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(starts.len(), 2, "queue starts: {t:02x?}");
    assert!(
        t[starts[0]..starts[1]].contains(&0x0F),
        "no queue stop between the two starts: {t:02x?}"
    );
    run_for(&mut worker, &clock, 1_000);
    let s = samples(&events);
    assert!(s.len() >= 900, "{} samples", s.len());
    assert!(s.windows(2).all(|w| w[1] - w[0] == 1), "not 1000 SPS");
    // The rate stays selectable.
    worker
        .set_rate(Rate::Sps50)
        .expect("set_rate after the restart");
}

#[test]
fn a_queue_start_refused_twice_is_not_retried_again() {
    let (_worker, _clock, log, events) = start(
        Rate::Sps1000,
        PipeOpts {
            reject_queue_start: true,
            ..PipeOpts::default()
        },
    );
    assert!(connected(&events).rate_locked);
    let starts = types(&log).iter().filter(|&&x| x == 0x0E).count();
    assert_eq!(starts, 2, "the start is retried exactly once");
}
