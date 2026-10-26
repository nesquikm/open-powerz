//! Recording and session export in the desktop backend (FR 0P7HSJ), against
//! the simulator on its fake clock. Every identifier is synthetic.

mod common;

use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener};
use open_powerz_desktop_lib::recording::{self, ExportProgress, RecordingEvent, RecordingSummary};
use open_powerz_desktop_lib::ui_error::UiError;
use open_powerz_desktop_lib::worker::{BackendEvent, GapKind, Status, Step, Worker, WorkerConfig};
use powerz_cli::StreamOptions;
use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::session::{Clock, Session};
use powerz_protocol::transport::guard::GuardedTransport;
use powerz_protocol::transport::{self, Transport, discovery::DeviceEntry, discovery::Model};
use powerz_record::{CSV_HEADER, CsvRecorder};

fn config(rate: Rate) -> WorkerConfig {
    WorkerConfig::new(rate, 1)
}

// ---- a fake file the recorder writes to ----

#[derive(Debug, Default)]
struct FileState {
    bytes: Vec<u8>,
    /// Bytes on "disk" at the last flush.
    flushed_len: usize,
    /// Simulated time of every flush.
    flushes: Vec<u64>,
    /// The writer was dropped (the file closed).
    closed: bool,
    /// Every write that would grow the file past this fails.
    fail_after: Option<usize>,
}

/// A file shared between the test and the writer handed to the recorder.
#[derive(Clone)]
struct FakeFile {
    state: Arc<Mutex<FileState>>,
    clock: SimClock,
}

/// The writer end: dropping it closes the file.
struct FakeWriter(FakeFile);

impl Write for FakeWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let mut s = self.0.state.lock().unwrap();
        if s.fail_after
            .is_some_and(|max| s.bytes.len() + data.len() > max)
        {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "No space left on device (os error 28)",
            ));
        }
        s.bytes.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let now = self.0.clock.now_ms();
        let mut s = self.0.state.lock().unwrap();
        s.flushed_len = s.bytes.len();
        s.flushes.push(now);
        Ok(())
    }
}

impl Drop for FakeWriter {
    fn drop(&mut self) {
        if let Ok(mut s) = self.0.state.lock() {
            s.closed = true;
        }
    }
}

impl FakeFile {
    fn new(clock: &SimClock) -> FakeFile {
        FakeFile {
            state: Arc::new(Mutex::new(FileState::default())),
            clock: clock.clone(),
        }
    }

    fn failing_after(clock: &SimClock, bytes: usize) -> FakeFile {
        let f = FakeFile::new(clock);
        f.state.lock().unwrap().fail_after = Some(bytes);
        f
    }

    fn writer(&self) -> Box<dyn Write + Send> {
        Box::new(FakeWriter(self.clone()))
    }

    fn bytes(&self) -> Vec<u8> {
        self.state.lock().unwrap().bytes.clone()
    }

    fn lines(&self) -> Vec<String> {
        String::from_utf8(self.bytes())
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn closed(&self) -> bool {
        self.state.lock().unwrap().closed
    }
}

fn start(
    rate: Rate,
    scenario: Scenario,
    faults: FaultPlan,
    opts: PipeOpts,
) -> (Worker<Pipe, SimClock>, SimClock, Arc<Mutex<Log>>, Events) {
    let clock = clock();
    let pipe = Pipe::sim(scenario, faults, &clock, opts);
    let log = Arc::clone(&pipe.log);
    let (events, sink) = capture(&clock);
    let worker = Worker::start(pipe, clock.clone(), config(rate), sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    (worker, clock, log, events)
}

fn delivered(log: &Arc<Mutex<Log>>) -> usize {
    log.lock().unwrap().records_delivered
}

/// Every recording event with the simulated time it was emitted at.
fn recording_events(events: &Events) -> Vec<(u64, RecordingEvent)> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(t, e)| match e {
            BackendEvent::Recording(r) => Some((*t, r.clone())),
            _ => None,
        })
        .collect()
}

/// A plain-language message: no raw OS error codes or Rust debug output.
fn assert_plain(err: &UiError) {
    assert!(!err.message.trim().is_empty(), "{err:?}");
    for raw in ["os error", "Os {", "Kind(", "StorageFull", "Error {"] {
        assert!(!err.message.contains(raw), "raw error text: {err:?}");
    }
}

/// Data rows that have every vendor column.
fn assert_rows_complete(lines: &[String]) {
    assert_eq!(lines.first().map(String::as_str), Some(CSV_HEADER));
    for l in &lines[1..] {
        assert_eq!(l.split(',').count(), 12, "incomplete row {l:?}");
    }
}

// ---- AC-0P7HSJ.3: record to file, flush, stop with rows and gaps ----

#[test]
fn recording_writes_the_header_then_every_record_received_after_start() {
    let (mut worker, clock, log, _events) = start(
        Rate::Sps1000,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    run_for(&mut worker, &clock, 1_000);
    let before = delivered(&log);
    let file = FakeFile::new(&clock);
    worker.start_recording(file.writer()).expect("start");
    assert!(worker.is_recording());
    assert_eq!(file.lines(), vec![CSV_HEADER.to_owned()], "header first");

    run_for(&mut worker, &clock, 2_000);
    let summary = worker.stop_recording().expect("stop");
    let after = delivered(&log);
    assert!(!worker.is_recording());
    assert!(file.closed(), "stop must close the file");

    let lines = file.lines();
    assert_rows_complete(&lines);
    assert_eq!(summary.rows, (after - before) as u64, "rows != records");
    assert_eq!(lines.len() as u64, summary.rows + 1);
    assert!(summary.rows >= 1_900, "only {} rows in 2 s", summary.rows);
    assert!(lines[1].starts_with("0:00:00.000,"), "{}", lines[1]);
    assert!(summary.gaps.is_empty(), "{:?}", summary.gaps);
    assert!(!summary.disconnected);

    // The session goes on after the recording.
    assert_eq!(run_for(&mut worker, &clock, 500), Step::Continue);
    assert!(delivered(&log) > after);
}

#[test]
fn a_recording_is_flushed_at_least_once_per_second() {
    let (mut worker, clock, _log, _events) = start(
        Rate::Sps50,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    let file = FakeFile::new(&clock);
    let t_start = clock.now_ms();
    worker.start_recording(file.writer()).unwrap();
    run_for(&mut worker, &clock, 5_000);
    let flushes = file.state.lock().unwrap().flushes.clone();
    let mut times = vec![t_start];
    times.extend(flushes.iter().copied().filter(|&t| t <= t_start + 5_000));
    assert!(times.len() >= 5, "flushes: {flushes:?}");
    for w in times.windows(2) {
        assert!(w[1] - w[0] <= 1_000, "{} ms without a flush", w[1] - w[0]);
    }
    assert!(t_start + 5_000 - times.last().unwrap() <= 1_000);
    // What was flushed is whole rows.
    let s = file.state.lock().unwrap();
    assert!(s.flushed_len > CSV_HEADER.len());
    assert_eq!(s.bytes[..s.flushed_len].last(), Some(&b'\n'));
}

#[test]
fn stop_reports_the_gaps_seen_while_recording() {
    let opts = PipeOpts {
        stall_at_poll: Some((80, 500)),
        ..PipeOpts::default()
    };
    let (mut worker, clock, _log, _events) =
        start(Rate::Sps1000, Scenario::Load5v1a, FaultPlan::new(), opts);
    run_for(&mut worker, &clock, 1_000); // ~50 polls: the stall is ahead
    let file = FakeFile::new(&clock);
    worker.start_recording(file.writer()).unwrap();
    run_for(&mut worker, &clock, 3_000);
    let summary = worker.stop_recording().unwrap();
    assert!(!summary.gaps.is_empty(), "the stall left no gap");
    for g in &summary.gaps {
        assert_eq!(g.kind, GapKind::Stream);
        assert!(g.to_ms > g.from_ms);
    }
    assert_eq!(file.lines().len() as u64, summary.rows + 1);
}

#[test]
fn a_second_start_or_a_stop_without_a_recording_is_refused() {
    let (mut worker, clock, _log, _events) = start(
        Rate::Sps10,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    let err = worker.stop_recording().expect_err("nothing to stop");
    assert_plain(&err);
    let file = FakeFile::new(&clock);
    worker.start_recording(file.writer()).unwrap();
    let other = FakeFile::new(&clock);
    let err = worker
        .start_recording(other.writer())
        .expect_err("already recording");
    assert_plain(&err);
    run_for(&mut worker, &clock, 500);
    assert!(file.lines().len() > 1, "the first recording stopped");
    assert!(other.lines().is_empty(), "the refused file was written to");
}

// ---- AC-0P7HSJ.4: byte-for-byte what `powerz stream` writes ----

fn is_queue_poll(req: &[u8]) -> bool {
    frame::get_data_mask(req).is_some_and(|m| m.bits() & Attribute::ADC_QUEUE.bits() != 0)
}

fn is_snapshot(req: &[u8]) -> bool {
    frame::get_data_mask(req).is_some_and(|m| m.bits() == Attribute::ADC.bits())
}

/// The replies the desktop worker received, each kind in request order.
#[derive(Default)]
struct Captured {
    /// Queue-poll replies.
    polls: Vec<Vec<u8>>,
    /// Simulated time right after each queue-poll reply, ms.
    poll_times: Vec<u64>,
    /// ADC snapshot replies (they carry the TEMP column's values).
    snapshots: Vec<Vec<u8>>,
}

/// Passes everything through and keeps every queue-poll and ADC snapshot
/// reply.
struct Tee {
    inner: Pipe,
    clock: SimClock,
    replies: Arc<Mutex<Captured>>,
}

impl Transport for Tee {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        let reply = self.inner.transact(request, timeout);
        if let Ok(r) = &reply {
            let mut c = self.replies.lock().unwrap();
            if is_queue_poll(request) {
                c.polls.push(r.clone());
                c.poll_times.push(self.clock.now_ms());
            } else if is_snapshot(request) {
                c.snapshots.push(r.clone());
            }
        }
        reply
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.inner.read(timeout)
    }
}

/// Answers the CLI's queue polls and ADC snapshots with the replies the
/// desktop received, in request order (the same queue records and the same
/// temperatures), everything else from a fresh simulator. Raises `stop`
/// once the last queue reply is served.
///
/// It also replays the desktop's poll timing on the CLI's clock
/// ([`ReplayClock`]: a stall that held a desktop poll for 500 ms holds the
/// CLI's too), so the CLI reads its snapshots after the same polls as the
/// desktop did.
struct Replay {
    sim: SimDevice,
    replies: VecDeque<Vec<u8>>,
    /// Desktop time right after each reply in `replies`, ms.
    poll_times: VecDeque<u64>,
    snapshots: VecDeque<Vec<u8>>,
    /// Shared with the CLI session's [`ReplayClock`].
    poll_time: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

/// The CLI session's clock: a [`SimClock`] until the first queue poll, then
/// the desktop's time right after the latest replayed poll.
struct ReplayClock {
    inner: SimClock,
    /// Desktop time of the latest replayed poll, ms; `0` = none yet.
    poll_time: Arc<AtomicU64>,
}

impl Clock for ReplayClock {
    fn now_ms(&mut self) -> u64 {
        match self.poll_time.load(Ordering::SeqCst) {
            0 => Clock::now_ms(&mut self.inner),
            t => t,
        }
    }
    fn random_u64(&mut self) -> u64 {
        self.inner.random_u64()
    }
    fn sleep(&mut self, d: Duration) {
        Clock::sleep(&mut self.inner, d);
    }
}

impl Transport for Replay {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        let queue = if is_queue_poll(request) {
            if let Some(t) = self.poll_times.pop_front() {
                self.poll_time.store(t, Ordering::SeqCst);
            }
            &mut self.replies
        } else if is_snapshot(request) {
            &mut self.snapshots
        } else {
            return self.sim.transact(request, timeout);
        };
        let mut reply = queue.pop_front().ok_or(transport::Error::Timeout)?;
        reply[1] = request[1];
        if self.replies.is_empty() {
            self.stop.store(true, Ordering::SeqCst);
        }
        Ok(reply)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.sim.read(timeout)
    }
}

/// Queue records in a captured queue-poll reply.
fn records_in(reply: &[u8]) -> usize {
    frame::parse_put_data(reply)
        .expect("a PUT_DATA queue reply")
        .iter()
        .filter(|s| s.att == Attribute::ADC_QUEUE)
        .map(|s| s.payload.len() / 20)
        .sum()
}

/// `line` with its TEMP cell (column 8) emptied.
fn without_temp(line: &str) -> String {
    let mut c: Vec<&str> = line.split(',').collect();
    assert_eq!(c.len(), 12, "row with {} columns: {line}", c.len());
    c[8] = "";
    c.join(",")
}

/// Record `record_ms` after `warmup_ms` of streaming in the desktop worker,
/// then feed the same queue replies and ADC snapshots to `powerz stream`
/// and compare files.
fn assert_matches_cli(
    rate: Rate,
    scenario: Scenario,
    opts: PipeOpts,
    warmup_ms: u64,
    record_ms: u64,
) -> RecordingSummary {
    let clock = clock();
    let replies = Arc::new(Mutex::new(Captured::default()));
    let tee = Tee {
        inner: Pipe::sim(scenario, FaultPlan::new(), &clock, opts),
        clock: clock.clone(),
        replies: Arc::clone(&replies),
    };
    let (_events, sink) = capture(&clock);
    let mut worker = Worker::start(tee, clock.clone(), config(rate), sink).unwrap();
    run_for(&mut worker, &clock, warmup_ms);
    *replies.lock().unwrap() = Captured::default();
    let file = FakeFile::new(&clock);
    worker.start_recording(file.writer()).unwrap();
    run_for(&mut worker, &clock, record_ms);
    let summary = worker.stop_recording().unwrap();
    let captured = std::mem::take(&mut *replies.lock().unwrap());
    // The CLI reads its first snapshot after its first queue poll, so the
    // rows of that poll carry no TEMP.
    let first_poll_rows = captured.polls.first().map_or(0, |r| records_in(r));
    assert!(summary.rows > 0);

    let desktop = file.bytes();
    let lines = file.lines();
    let unix0: f64 = lines[1].split(',').nth(1).unwrap().parse().unwrap();
    let start_unix_ms = (unix0 * 1000.0).round() as u64;
    assert!(
        start_unix_ms >= T0,
        "Unix column {unix0} before the session"
    );

    let stop = Arc::new(AtomicBool::new(false));
    let poll_time = Arc::new(AtomicU64::new(0));
    let replay = Replay {
        sim: SimDevice::with_clock(Persona::Bulk, scenario, SimClock::new(T0)),
        replies: captured.polls.into(),
        poll_times: captured.poll_times.into(),
        snapshots: captured.snapshots.into(),
        poll_time: Arc::clone(&poll_time),
        stop: Arc::clone(&stop),
    };
    let cli_clock = ReplayClock {
        inner: SimClock::new(T0),
        poll_time,
    };
    let mut session = Session::with_clock(GuardedTransport::new(replay), cli_clock);
    session.connect().expect("CONNECT");
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let opts = StreamOptions {
        rate,
        duration: None,
        start_unix_ms,
    };
    let cli = powerz_cli::stream(&mut session, &opts, &stop, &mut csv, &mut err)
        .expect("powerz stream over the replayed records");
    assert_eq!(cli.samples, summary.rows, "row counts differ");
    let (desktop, cli_csv) = (
        String::from_utf8(desktop).unwrap(),
        String::from_utf8(csv).unwrap(),
    );
    if warmup_ms > 0 {
        // A recording started mid-session carries the warm-up snapshot's
        // temperature in its first rows, which a fresh `powerz stream`
        // cannot know before its first snapshot (after its first poll).
        // Every other column of every row must still match byte for byte,
        // and the TEMP column too from the CLI's first snapshot onward.
        let (d, c): (Vec<&str>, Vec<&str>) = (desktop.lines().collect(), cli_csv.lines().collect());
        assert_eq!(d.len(), c.len(), "line counts differ");
        assert_eq!(d[0], c[0], "headers differ");
        for (k, (dl, cl)) in d.iter().zip(&c).enumerate().skip(1) {
            if k <= first_poll_rows {
                assert_eq!(without_temp(dl), without_temp(cl), "row {k} differs");
                assert_eq!(
                    cl.split(',').nth(8),
                    Some(""),
                    "CLI row {k} TEMP before any snapshot"
                );
            } else {
                assert_eq!(dl, cl, "row {k} differs (TEMP included)");
            }
        }
        return summary;
    }
    assert!(
        desktop == cli_csv,
        "the desktop recording differs from `powerz stream` ({} vs {} lines); first differing line: {:?}",
        desktop.lines().count(),
        cli_csv.lines().count(),
        desktop
            .lines()
            .zip(cli_csv.lines())
            .enumerate()
            .find(|(_, (a, b))| a != b)
    );
    summary
}

#[test]
fn a_recording_from_the_start_is_what_powerz_stream_writes() {
    assert_matches_cli(
        Rate::Sps1000,
        Scenario::Load5v1a,
        PipeOpts::default(),
        0,
        2_000,
    );
}

#[test]
fn a_recording_started_mid_session_counts_time_and_charge_from_its_start() {
    let summary = assert_matches_cli(
        Rate::Sps10,
        Scenario::NoisyNegative,
        PipeOpts::default(),
        1_500,
        3_000,
    );
    assert!(summary.rows >= 25, "{} rows", summary.rows);
}

#[test]
fn a_recording_across_a_stream_gap_is_what_powerz_stream_writes() {
    let opts = PipeOpts {
        stall_at_poll: Some((80, 500)),
        ..PipeOpts::default()
    };
    let summary = assert_matches_cli(Rate::Sps1000, Scenario::Pd9v2a, opts, 1_000, 3_000);
    assert!(!summary.gaps.is_empty(), "the stall left no gap");
}

// ---- AC-0P7HSJ.5: export the session without stopping the stream ----

/// A 1000-SPS record at `time_ms`: 5 V, 1 A, CC1 1.68 V, D+ 0.6 V.
fn synthetic(time_ms: u32) -> QueueRecord {
    let mut b = Vec::with_capacity(20);
    b.extend_from_slice(&time_ms.to_le_bytes());
    b.extend_from_slice(&(5_000_000 + (time_ms % 997) as i32).to_le_bytes());
    b.extend_from_slice(&(1_000_000 - (time_ms % 13) as i32 * 1_000).to_le_bytes());
    for v in [1680u16, 0, 600, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    QueueRecord::decode(&b, Rate::Sps1000).unwrap()
}

#[test]
fn export_records_writes_what_a_recorder_writes_and_reports_progress() {
    let records: Vec<QueueRecord> = (0..10_000u32).map(|k| synthetic(50_000 + k)).collect();
    let mut expected = CsvRecorder::new(Vec::new()).unwrap();
    for r in &records {
        expected.push(r, T0).unwrap();
    }
    let expected = expected.finish().unwrap();

    let mut progress: Vec<ExportProgress> = Vec::new();
    let mut out = Vec::new();
    let rows = recording::export_records(&records, &mut out, T0, |p| progress.push(p)).unwrap();
    assert_eq!(rows, 10_000);
    assert_eq!(out, expected, "export layout differs from the recorder");
    assert!(!progress.is_empty(), "no progress reported");
    assert!(
        progress
            .iter()
            .all(|p| p.total == 10_000 && p.done <= p.total)
    );
    assert!(progress.windows(2).all(|w| w[1].done >= w[0].done));
    assert_eq!(
        progress.last().map(|p| (p.done, p.total)),
        Some((10_000, 10_000))
    );
}

#[test]
fn export_of_an_empty_session_is_the_header_only() {
    let mut out = Vec::new();
    let rows = recording::export_records(&[], &mut out, T0, |_| {}).unwrap();
    assert_eq!(rows, 0);
    assert_eq!(String::from_utf8(out).unwrap(), format!("{CSV_HEADER}\n"));
}

#[test]
fn export_runs_on_its_own_thread_while_the_stream_goes_on() {
    let (mut worker, clock, log, _events) = start(
        Rate::Sps1000,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    // A recording of the whole session, to compare the export with.
    let recorded = FakeFile::new(&clock);
    worker.start_recording(recorded.writer()).unwrap();
    run_for(&mut worker, &clock, 2_000);
    let summary = worker.stop_recording().unwrap();
    let so_far = delivered(&log) as u64;
    assert_eq!(summary.rows, so_far);

    let exported = FakeFile::new(&clock);
    let progress: Arc<Mutex<Vec<(std::thread::ThreadId, ExportProgress)>>> = Arc::default();
    let seen = Arc::clone(&progress);
    let job = worker.export(exported.writer(), move |p| {
        seen.lock().unwrap().push((std::thread::current().id(), p));
    });
    // The worker keeps polling while the export runs.
    assert_eq!(run_for(&mut worker, &clock, 1_000), Step::Continue);
    let rows = job.join().expect("export thread panicked").expect("export");
    assert_eq!(rows, so_far, "export must hold the whole session so far");
    assert!(delivered(&log) as u64 > so_far, "the stream stopped");
    assert!(
        log.lock().unwrap().of_type(0x0F).is_empty(),
        "export stopped the queue"
    );

    assert_eq!(
        exported.bytes(),
        recorded.bytes(),
        "export differs from a recording of the same session"
    );
    assert!(exported.closed(), "export left the file open");
    let progress = progress.lock().unwrap();
    let me = std::thread::current().id();
    assert!(!progress.is_empty(), "no progress events");
    assert!(progress.iter().all(|(id, _)| *id != me), "export ran here");
    let last = progress.last().unwrap().1;
    assert_eq!((last.done, last.total), (rows, rows));
}

/// Release builds only: the 5 s bound is meaningless in a debug build. CI
/// runs it with `cargo test --release -p open-powerz-desktop --locked --test recording`
/// (in a release build the test is not ignored).
#[test]
#[cfg_attr(debug_assertions, ignore = "timing bound; run in release mode")]
fn export_of_one_million_samples_takes_at_most_5_s() {
    let records: Vec<QueueRecord> = (0..1_000_000u32).map(|k| synthetic(k + 1)).collect();
    let dir = temp_dir("export-1m");
    let path = dir.join("powerz-export.csv");
    let file = std::fs::File::create(&path).unwrap();
    let t = Instant::now();
    let rows = recording::export_records(&records, file, T0, |_| {}).unwrap();
    let took = t.elapsed();
    assert_eq!(rows, 1_000_000);
    assert!(took <= Duration::from_secs(5), "export took {took:?}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 1_000_001);
    let _ = std::fs::remove_dir_all(dir);
}

// ---- AC-0P7HSJ.6: unplug and write errors ----

#[test]
fn unplugging_while_recording_closes_the_file_with_every_row_and_tells_the_ui() {
    let (mut worker, clock, log, events) = start(
        Rate::Sps1000,
        Scenario::Load5v1a,
        FaultPlan::new().disconnect_after_polls(100),
        PipeOpts::default(),
    );
    let file = FakeFile::new(&clock);
    worker.start_recording(file.writer()).unwrap();
    assert_eq!(run_for(&mut worker, &clock, 30_000), Step::Ended);
    let rows = delivered(&log) as u64;
    assert!(rows > 0);

    assert!(file.closed(), "the file is still open after the unplug");
    let lines = file.lines();
    assert_rows_complete(&lines);
    assert_eq!(lines.len() as u64, rows + 1, "rows lost at unplug");
    let s = file.state.lock().unwrap();
    assert_eq!(s.flushed_len, s.bytes.len(), "unflushed rows at unplug");
    drop(s);

    let stopped: Vec<RecordingSummary> = recording_events(&events)
        .into_iter()
        .filter_map(|(_, e)| match e {
            RecordingEvent::Stopped(summary) => Some(summary),
            _ => None,
        })
        .collect();
    assert_eq!(stopped.len(), 1, "one notice for the unplug");
    assert!(stopped[0].disconnected, "{:?}", stopped[0]);
    assert_eq!(stopped[0].rows, rows);
    assert!(!worker.is_recording());
}

#[test]
fn a_write_error_stops_the_recording_keeps_the_rows_and_the_session() {
    let (mut worker, clock, log, events) = start(
        Rate::Sps1000,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    // Room for the header and roughly two seconds of rows.
    let file = FakeFile::failing_after(&clock, 200_000);
    worker.start_recording(file.writer()).unwrap();
    assert_eq!(run_for(&mut worker, &clock, 6_000), Step::Continue);

    let failed: Vec<(u64, UiError)> = recording_events(&events)
        .into_iter()
        .filter_map(|(_, e)| match e {
            RecordingEvent::Failed { rows, error } => Some((rows, error)),
            _ => None,
        })
        .collect();
    assert_eq!(failed.len(), 1, "one error for the failed write");
    let (rows, error) = &failed[0];
    assert_plain(error);
    assert!(!worker.is_recording(), "the recording kept going");
    assert!(file.closed(), "the failed file is still open");

    let lines = file.lines();
    assert_rows_complete(&lines);
    assert!(lines.len() > 1, "the rows already written were lost");
    assert!((lines.len() - 1) as u64 <= *rows);

    // The stream is unaffected.
    let before = delivered(&log);
    assert_eq!(run_for(&mut worker, &clock, 1_000), Step::Continue);
    assert!(delivered(&log) > before);
    assert!(
        !statuses(&events)
            .iter()
            .any(|s| matches!(s, Status::Disconnected { .. })),
        "a write error ended the session"
    );
}

// ---- AC-0P7HSJ.7: elapsed time and row count while recording ----

#[test]
fn progress_events_carry_elapsed_time_and_rows_at_least_once_per_second() {
    let (mut worker, clock, _log, events) = start(
        Rate::Sps50,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    run_for(&mut worker, &clock, 500);
    let t_start = clock.now_ms();
    worker
        .start_recording(FakeFile::new(&clock).writer())
        .unwrap();
    run_for(&mut worker, &clock, 4_000);
    let summary = worker.stop_recording().unwrap();

    let evs = recording_events(&events);
    assert!(
        matches!(evs.first(), Some((_, RecordingEvent::Started))),
        "{evs:?}"
    );
    let progress: Vec<(u64, u64, u64)> = evs
        .iter()
        .filter_map(|(t, e)| match e {
            RecordingEvent::Progress { rows, elapsed_ms } => Some((*t, *rows, *elapsed_ms)),
            _ => None,
        })
        .collect();
    assert!(progress.len() >= 4, "{progress:?}");
    let mut last_t = t_start;
    let mut last_rows = 0;
    for &(t, rows, elapsed_ms) in &progress {
        assert!(t - last_t <= 1_000, "{} ms without progress", t - last_t);
        assert!(rows >= last_rows, "row count went down");
        assert!(rows <= summary.rows);
        let actual = t - t_start;
        assert!(
            elapsed_ms <= actual && actual - elapsed_ms <= 1_000,
            "elapsed {elapsed_ms} ms at {actual} ms"
        );
        (last_t, last_rows) = (t, rows);
    }
    // 50 SPS for 4 s: the last count is close to the final one.
    assert!(
        last_rows + 60 >= summary.rows,
        "{last_rows} vs {}",
        summary.rows
    );
}

#[test]
fn recording_events_serialize_for_the_ui() {
    let json = |e: RecordingEvent| serde_json::to_value(BackendEvent::Recording(e)).unwrap();
    assert_eq!(
        json(RecordingEvent::Started),
        serde_json::json!({"type": "recording", "data": {"state": "started"}})
    );
    assert_eq!(
        json(RecordingEvent::Progress {
            rows: 3,
            elapsed_ms: 1_500
        }),
        serde_json::json!({"type": "recording", "data": {"state": "progress", "rows": 3, "elapsed_ms": 1500}})
    );
    let v = json(RecordingEvent::Stopped(RecordingSummary {
        rows: 7,
        gaps: vec![],
        disconnected: true,
    }));
    assert_eq!(
        v,
        serde_json::json!({"type": "recording", "data": {"state": "stopped", "rows": 7, "gaps": [], "disconnected": true}})
    );
    let v = json(RecordingEvent::Failed {
        rows: 2,
        error: UiError::usage("The disk is full."),
    });
    assert_eq!(v["data"]["state"], "failed");
    assert_eq!(v["data"]["rows"], 2);
    assert_eq!(v["data"]["error"]["message"], "The disk is full.");
    let p = serde_json::to_value(ExportProgress { done: 5, total: 10 }).unwrap();
    assert_eq!(p, serde_json::json!({"done": 5, "total": 10}));
}

// ---- AC-0P7HSJ.3 / .5: real files through the device manager ----

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "open-powerz-test-{}-{name}-{}",
        std::process::id(),
        T0
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct SimOpener {
    clock: SimClock,
}

impl Opener for SimOpener {
    type Transport = SimDevice;
    fn open(&self, _device: &DeviceListing) -> Result<SimDevice, transport::Error> {
        Ok(SimDevice::with_clock(
            Persona::Bulk,
            Scenario::Load5v1a,
            self.clock.clone(),
        ))
    }
}

fn manager() -> DeviceManager<SimOpener, SimClock> {
    let clock = clock();
    DeviceManager::new(
        || {
            Ok(vec![DeviceEntry {
                model: Model::Km003c,
                pid: 0x0063,
                serial: Some("SERIAL-0001".to_owned()),
                product: Some("POWER-Z KM003C".to_owned()),
                interface_string: Some("APP".to_owned()),
                bus: "1".to_owned(),
                address: 7,
            }])
        },
        SimOpener {
            clock: clock.clone(),
        },
        clock,
    )
}

/// Wait (real time) until `n` more samples have arrived.
fn wait_samples(rx: &mpsc::Receiver<BackendEvent>, n: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut got = 0;
    while got < n {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(BackendEvent::Batch(b)) => got += b.samples.len(),
            Ok(_) => {}
            Err(e) => panic!("samples never arrived ({e:?}); got {got}"),
        }
    }
}

fn connect(m: &mut DeviceManager<SimOpener, SimClock>) -> mpsc::Receiver<BackendEvent> {
    let (tx, rx) = mpsc::channel();
    m.connect("demo:load-5v-1a", 50, move |e| {
        let _ = tx.send(e);
    })
    .expect("connect");
    wait_samples(&rx, 1);
    rx
}

#[test]
fn the_manager_records_to_a_file_on_disk() {
    let dir = temp_dir("record");
    let path = dir.join("powerz-20261021-120000.csv");
    let mut m = manager();
    let rx = connect(&mut m);
    m.start_recording(&path).expect("start recording");
    // Batches queued before the recording began carry unrecorded samples; skip them,
    // so a slow runner's backlog cannot satisfy the wait with zero rows written.
    while rx.try_recv().is_ok() {}
    wait_samples(&rx, 200);
    let summary = m.stop_recording().expect("stop recording");
    m.disconnect().unwrap();

    let text = std::fs::read_to_string(&path).expect("the file exists");
    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
    assert_rows_complete(&lines);
    assert!(summary.rows >= 1);
    assert_eq!(lines.len() as u64, summary.rows + 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_file_that_cannot_be_created_is_a_plain_language_error() {
    let dir = temp_dir("bad-path");
    let path = dir.join("no-such-folder").join("powerz.csv");
    let mut m = manager();
    let _rx = connect(&mut m);
    let err = m.start_recording(&path).expect_err("no folder to write in");
    assert_plain(&err);
    m.disconnect().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn recording_without_a_meter_is_refused() {
    let dir = temp_dir("no-meter");
    let m = manager();
    let err = m
        .start_recording(&dir.join("powerz.csv"))
        .expect_err("not connected");
    assert_plain(&err);
    assert!(
        !dir.join("powerz.csv").exists(),
        "a file was created anyway"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_manager_exports_the_session_while_it_streams() {
    let dir = temp_dir("export");
    let path = dir.join("powerz-export.csv");
    let mut m = manager();
    let rx = connect(&mut m);
    wait_samples(&rx, 100);
    let last: Arc<Mutex<Option<ExportProgress>>> = Arc::default();
    let seen = Arc::clone(&last);
    let job = m
        .export_session(Path::new(&path), move |p| {
            *seen.lock().unwrap() = Some(p);
        })
        .expect("export started");
    let rows = job.join().expect("export thread").expect("export");
    // Still streaming afterwards.
    wait_samples(&rx, 10);
    m.disconnect().unwrap();

    assert!(rows >= 100, "{rows} rows");
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
    assert_rows_complete(&lines);
    assert_eq!(lines.len() as u64, rows + 1);
    assert!(lines[1].starts_with("0:00:00.000,"));
    let last = last.lock().unwrap().expect("no progress");
    assert_eq!((last.done, last.total), (rows, rows));
    let _ = std::fs::remove_dir_all(dir);
}

// ---- Review round 1: honest counts, honest reasons, no rate changes mid-file ----

#[test]
fn a_write_error_reports_exactly_the_rows_that_reached_the_file() {
    let (mut worker, clock, _log, events) = start(
        Rate::Sps1000,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    let file = FakeFile::failing_after(&clock, 200_000);
    worker.start_recording(file.writer()).unwrap();
    run_for(&mut worker, &clock, 6_000);
    let rows = recording_events(&events)
        .into_iter()
        .find_map(|(_, e)| match e {
            RecordingEvent::Failed { rows, .. } => Some(rows),
            _ => None,
        })
        .expect("no Failed event");
    let lines = file.lines();
    assert_eq!(
        rows,
        (lines.len() - 1) as u64,
        "the reported count is not what the file holds"
    );
}

#[test]
fn a_disconnect_you_ask_for_does_not_call_the_recording_unplugged() {
    let (mut worker, clock, _log, events) = start(
        Rate::Sps50,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    let file = FakeFile::new(&clock);
    worker.start_recording(file.writer()).unwrap();
    run_for(&mut worker, &clock, 1_000);
    worker.stop();
    let stopped: Vec<_> = recording_events(&events)
        .into_iter()
        .filter_map(|(_, e)| match e {
            RecordingEvent::Stopped(summary) => Some(summary),
            _ => None,
        })
        .collect();
    assert_eq!(stopped.len(), 1);
    assert!(
        !stopped[0].disconnected,
        "a requested disconnect was reported as an unplug"
    );
}

#[test]
fn the_rate_cannot_change_while_a_recording_is_open() {
    let (mut worker, clock, log, _events) = start(
        Rate::Sps50,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    let file = FakeFile::new(&clock);
    worker.start_recording(file.writer()).unwrap();
    let frames_before = log.lock().unwrap().frames.len();
    let err = worker
        .set_rate(Rate::Sps10)
        .expect_err("rate changed mid-recording");
    assert_eq!(
        err.kind,
        open_powerz_desktop_lib::ui_error::ErrorKind::Usage
    );
    assert_eq!(
        log.lock().unwrap().frames.len(),
        frames_before,
        "a frame was sent"
    );
}

// ---- Review round 2: the chosen file is never truncated or deleted by a refused start ----

#[test]
fn a_second_start_leaves_the_file_being_recorded_alone() {
    let dir = temp_dir("second-start");
    let path = dir.join("powerz.csv");
    let mut m = manager();
    let rx = connect(&mut m);
    m.start_recording(&path).expect("start recording");
    wait_samples(&rx, 100);
    let err = m.start_recording(&path).expect_err("second start accepted");
    assert_plain(&err);
    wait_samples(&rx, 50);
    let summary = m
        .stop_recording()
        .expect("the first recording is still running");
    m.disconnect().unwrap();
    let text = std::fs::read_to_string(&path).expect("the file was deleted");
    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
    assert_rows_complete(&lines);
    assert_eq!(
        lines.len() as u64,
        summary.rows + 1,
        "the live file was truncated"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_refused_start_never_deletes_a_file_that_was_already_there() {
    let dir = temp_dir("keep-existing");
    let path = dir.join("keep.csv");
    std::fs::write(&path, b"precious\n").unwrap();
    let mut m = manager();
    let rx = connect(&mut m);
    let other = dir.join("other.csv");
    m.start_recording(&other).expect("start recording");
    wait_samples(&rx, 20);
    m.start_recording(&path).expect_err("second start accepted");
    m.stop_recording().unwrap();
    m.disconnect().unwrap();
    assert_eq!(
        std::fs::read(&path).expect("the existing file was deleted"),
        b"precious\n"
    );
    let _ = std::fs::remove_dir_all(dir);
}
