//! FR AXHK2R: pause, auto-stop, the row limit and the recording's folder.
//!
//! The simulator's stream runs through [`Script`], which overwrites the
//! current of every queue record, so a test decides what the meter measures
//! at each moment. The worker is stepped on a simulated clock.

mod common;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use common::*;
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener};
use open_powerz_desktop_lib::recording::{
    DEFAULT_MAX_ROWS, EndReason, Limits, RecordingEvent, RecordingSummary, folder_recording_path,
    recording_name_ok,
};
use open_powerz_desktop_lib::ui_error::{ErrorKind, UiError};
use open_powerz_desktop_lib::worker::{BackendEvent, GapKind, Step, Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::transport::{self, Transport, discovery::DeviceEntry, discovery::Model};
use powerz_record::CSV_HEADER;

const ROW_LEN: usize = 20;
const LOW_A: f64 = 0.05;
const BELOW_A: f64 = 0.1;
const WAIT_MS: u64 = 1_000;

// ---- the scripted meter ----

/// What the next queue records carry.
struct Knobs {
    /// Current of every record, µA.
    ibus_ua: AtomicI32,
    /// The next queue poll comes this late, which leaves a stream gap.
    stall_ms: AtomicU64,
    /// Device time steps back by this much at the next poll: a new run.
    rewind_ms: AtomicU32,
}

struct Script {
    inner: Pipe,
    clock: SimClock,
    knobs: Arc<Knobs>,
    rewound_ms: u32,
}

fn is_queue_poll(req: &[u8]) -> bool {
    frame::get_data_mask(req).is_some_and(|m| m.bits() & Attribute::ADC_QUEUE.bits() != 0)
}

/// Where the queue records of `reply` lie: offset and length of each section.
fn queue_sections(reply: &[u8]) -> Vec<(usize, usize)> {
    let base = reply.as_ptr() as usize;
    frame::parse_put_data(reply).map_or_else(
        |_| Vec::new(),
        |sections| {
            sections
                .iter()
                .filter(|s| s.att == Attribute::ADC_QUEUE)
                .map(|s| (s.payload.as_ptr() as usize - base, s.payload.len()))
                .collect()
        },
    )
}

impl Transport for Script {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        let queue = is_queue_poll(request);
        if queue {
            let stall = self.knobs.stall_ms.swap(0, Ordering::SeqCst);
            self.clock.advance(Duration::from_millis(stall));
            self.rewound_ms += self.knobs.rewind_ms.swap(0, Ordering::SeqCst);
        }
        let mut reply = self.inner.transact(request, timeout)?;
        if queue {
            let ua = self.knobs.ibus_ua.load(Ordering::SeqCst).to_le_bytes();
            for (start, len) in queue_sections(&reply) {
                for rec in reply[start..start + len].as_chunks_mut::<ROW_LEN>().0 {
                    let bytes: [u8; 4] = rec[..4].try_into().unwrap();
                    let time = u32::from_le_bytes(bytes).wrapping_sub(self.rewound_ms);
                    rec[..4].copy_from_slice(&time.to_le_bytes());
                    rec[8..12].copy_from_slice(&ua);
                }
            }
        }
        Ok(reply)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.inner.read(timeout)
    }
}

struct Rig {
    worker: Worker<Script, SimClock>,
    clock: SimClock,
    knobs: Arc<Knobs>,
    log: Arc<Mutex<Log>>,
    events: Events,
}

fn rig_with(faults: FaultPlan) -> Rig {
    let clock = clock();
    let inner = Pipe::sim(Scenario::Load5v1a, faults, &clock, PipeOpts::default());
    let log = Arc::clone(&inner.log);
    let knobs = Arc::new(Knobs {
        ibus_ua: AtomicI32::new(1_000_000),
        stall_ms: AtomicU64::new(0),
        rewind_ms: AtomicU32::new(0),
    });
    let script = Script {
        inner,
        clock: clock.clone(),
        knobs: Arc::clone(&knobs),
        rewound_ms: 0,
    };
    let (events, sink) = capture(&clock);
    let worker = Worker::start(
        script,
        clock.clone(),
        WorkerConfig::new(Rate::Sps1000, 1),
        sink,
    )
    .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    let mut rig = Rig {
        worker,
        clock,
        knobs,
        log,
        events,
    };
    rig.run(300); // the queue runs by the time a recording starts
    rig
}

fn rig() -> Rig {
    rig_with(FaultPlan::new())
}

impl Rig {
    fn run(&mut self, ms: u64) -> Step {
        run_for(&mut self.worker, &self.clock, ms)
    }

    fn amps(&self, a: f64) {
        self.knobs
            .ibus_ua
            .store((a * 1e6).round() as i32, Ordering::SeqCst);
    }

    fn rows(&mut self) -> u64 {
        self.worker.recording_progress().expect("a recording").0
    }

    fn start(&mut self, limits: Limits) -> Csv {
        let file = Csv::default();
        self.worker
            .start_recording_with(file.writer(), limits)
            .expect("start recording");
        file
    }

    fn stopped(&self) -> Vec<RecordingSummary> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(_, e)| match e {
                BackendEvent::Recording(RecordingEvent::Stopped(s)) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    /// The `paused` flag of every progress event, in order.
    fn progress_paused(&self) -> Vec<bool> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(_, e)| match e {
                BackendEvent::Recording(RecordingEvent::Progress { paused, .. }) => Some(*paused),
                _ => None,
            })
            .collect()
    }

    /// Stream gaps the page was told about.
    fn gap_events(&self) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, e)| matches!(e, BackendEvent::Gap(_)))
            .count()
    }
}

// ---- a file the recorder writes to ----

#[derive(Clone, Default)]
struct Csv {
    bytes: Arc<Mutex<Vec<u8>>>,
    closed: Arc<AtomicBool>,
}

struct CsvWriter(Csv);

impl Write for CsvWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.0.bytes.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for CsvWriter {
    fn drop(&mut self) {
        self.0.closed.store(true, Ordering::SeqCst);
    }
}

impl Csv {
    fn writer(&self) -> Box<dyn Write + Send> {
        Box::new(CsvWriter(self.clone()))
    }

    fn lines(&self) -> Vec<String> {
        String::from_utf8(self.bytes.lock().unwrap().clone())
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

/// `h:mm:ss.mmm` in ms.
fn elapsed_ms(line: &str) -> u64 {
    let field = line.split(',').next().unwrap();
    let parts: Vec<&str> = field.split(':').collect();
    let [h, m, s] = parts[..] else {
        panic!("elapsed {field:?}")
    };
    let (s, ms) = s.split_once('.').unwrap();
    let n = |x: &str| x.parse::<u64>().unwrap();
    (n(h) * 3600 + n(m) * 60 + n(s)) * 1000 + n(ms)
}

/// Column `i` of a data row as a number. 3 = IBUS, 9 = CHARGE, 10 = ENERGY.
fn column(line: &str, i: usize) -> f64 {
    line.split(',').nth(i).unwrap().parse().unwrap()
}

fn auto_stop() -> Limits {
    Limits {
        below_a: Some(BELOW_A),
        for_ms: WAIT_MS,
        ..Limits::default()
    }
}

fn row_limit(max_rows: u64) -> Limits {
    Limits {
        max_rows,
        ..Limits::default()
    }
}

fn pause_gaps(summary: &RecordingSummary) -> usize {
    summary
        .gaps
        .iter()
        .filter(|g| g.kind == GapKind::Pause)
        .count()
}

fn assert_plain(err: &UiError) {
    assert!(!err.message.trim().is_empty(), "{err:?}");
    for raw in ["os error", "Os {", "Kind(", "Error {"] {
        assert!(!err.message.contains(raw), "raw error text: {err:?}");
    }
}

// ---- AC-AXHK2R.1: pause and resume ----

#[test]
fn a_pause_stops_the_rows_while_the_stream_goes_on_and_a_resume_continues_the_file() {
    let mut r = rig();
    let file = r.start(Limits::default());
    r.run(500);
    r.worker.pause_recording().expect("pause");
    let rows_at_pause = r.rows();
    assert!(rows_at_pause > 0);
    let samples_at_pause = samples(&r.events).len();

    r.run(1_500);
    assert_eq!(r.rows(), rows_at_pause, "rows written during the pause");
    assert!(
        samples(&r.events).len() > samples_at_pause + 1_000,
        "the chart stopped during the pause"
    );
    assert!(r.worker.is_recording(), "a pause ended the recording");

    r.worker.resume_recording().expect("resume");
    r.run(500);
    assert!(r.rows() > rows_at_pause, "no rows after the resume");

    let summary = r.worker.stop_recording().expect("stop");
    let lines = file.lines();
    assert_eq!(lines.first().map(String::as_str), Some(CSV_HEADER));
    assert_eq!(lines.len() as u64, summary.rows + 1);
    assert_eq!(summary.ended, EndReason::Stopped);

    let gaps: Vec<_> = summary
        .gaps
        .iter()
        .filter(|g| g.kind == GapKind::Pause)
        .collect();
    assert_eq!(gaps.len(), 1, "{:?}", summary.gaps);
    // The gap spans the rows on either side of the pause.
    let before = elapsed_ms(&lines[rows_at_pause as usize]);
    let after = elapsed_ms(&lines[rows_at_pause as usize + 1]);
    assert!(after - before >= 1_400, "file time {before} -> {after}");
    let span = gaps[0].to_ms - gaps[0].from_ms;
    assert!(
        span.abs_diff(after - before) <= 50,
        "gap of {span} ms for a jump of {} ms",
        after - before
    );
}

#[test]
fn a_pause_sends_the_meter_nothing_new() {
    let mut r = rig();
    let _file = r.start(Limits::default());
    r.run(300);
    r.worker.pause_recording().expect("pause");
    r.run(600);
    r.worker.resume_recording().expect("resume");
    r.run(300);
    r.worker.stop_recording().expect("stop");
    let log = r.log.lock().unwrap();
    assert!(log.of_type(0x0F).is_empty(), "a pause stopped the queue");
    for (_, f) in &log.frames {
        let ty = f.first().map_or(0, |b| b & 0x7F);
        assert!(SAFE_TYPES.contains(&ty), "unsafe frame {ty:#x}");
    }
}

#[test]
fn progress_says_whether_the_recording_is_paused() {
    let mut r = rig();
    let _file = r.start(Limits::default());
    r.run(700);
    assert_eq!(r.progress_paused().last(), Some(&false));
    r.worker.pause_recording().expect("pause");
    r.run(700);
    assert_eq!(r.progress_paused().last(), Some(&true));
    r.worker.resume_recording().expect("resume");
    r.run(700);
    assert_eq!(r.progress_paused().last(), Some(&false));
}

#[test]
fn a_pause_and_resume_with_no_row_between_them_stay_one_gap() {
    let mut r = rig();
    let _file = r.start(Limits::default());
    r.run(300);
    r.worker.pause_recording().expect("pause");
    r.run(300);
    r.worker.resume_recording().expect("resume");
    // No step: no row reaches the file before the next pause.
    r.worker.pause_recording().expect("pause again");
    r.run(300);
    r.worker.resume_recording().expect("resume again");
    r.run(300);
    let summary = r.worker.stop_recording().expect("stop");
    assert_eq!(pause_gaps(&summary), 1, "{:?}", summary.gaps);
}

#[test]
fn two_pauses_with_rows_between_them_are_two_gaps() {
    let mut r = rig();
    let _file = r.start(Limits::default());
    for _ in 0..2 {
        r.run(300);
        r.worker.pause_recording().expect("pause");
        r.run(300);
        r.worker.resume_recording().expect("resume");
    }
    r.run(300);
    let summary = r.worker.stop_recording().expect("stop");
    assert_eq!(pause_gaps(&summary), 2, "{:?}", summary.gaps);
}

#[test]
fn a_pause_before_the_first_row_adds_no_gap() {
    let mut r = rig();
    let _file = r.start(Limits::default());
    r.worker.pause_recording().expect("pause");
    r.run(500);
    assert_eq!(r.rows(), 0);
    r.worker.resume_recording().expect("resume");
    r.run(500);
    let summary = r.worker.stop_recording().expect("stop");
    assert!(summary.rows > 0);
    assert_eq!(pause_gaps(&summary), 0, "{:?}", summary.gaps);
}

#[test]
fn a_pause_still_open_at_the_stop_adds_no_gap() {
    let mut r = rig();
    let file = r.start(Limits::default());
    r.run(500);
    r.worker.pause_recording().expect("pause");
    let rows = r.rows();
    r.run(500);
    let summary = r.worker.stop_recording().expect("stop");
    assert_eq!(summary.rows, rows);
    assert_eq!(pause_gaps(&summary), 0, "{:?}", summary.gaps);
    assert_eq!(file.lines().len() as u64, rows + 1);
}

#[test]
fn the_summary_counts_pause_gaps_beside_stream_gaps() {
    let mut r = rig();
    let _file = r.start(Limits::default());
    r.run(300);
    r.worker.pause_recording().expect("pause");
    r.run(300);
    r.worker.resume_recording().expect("resume");
    r.run(300);
    r.knobs.stall_ms.store(500, Ordering::SeqCst);
    r.run(600);
    let summary = r.worker.stop_recording().expect("stop");
    assert_eq!(pause_gaps(&summary), 1, "{:?}", summary.gaps);
    assert!(
        summary.gaps.iter().any(|g| g.kind == GapKind::Stream),
        "{:?}",
        summary.gaps
    );
}

#[test]
fn a_stream_gap_during_a_pause_is_no_second_gap() {
    let mut r = rig();
    let _file = r.start(Limits::default());
    r.run(300);
    r.worker.pause_recording().expect("pause");
    r.run(300);
    r.knobs.stall_ms.store(500, Ordering::SeqCst);
    r.run(300);
    assert!(r.gap_events() > 0, "the chart got no gap: the stall missed");
    r.worker.resume_recording().expect("resume");
    r.run(300);
    let summary = r.worker.stop_recording().expect("stop");
    assert_eq!(summary.gaps.len(), 1, "{:?}", summary.gaps);
    assert_eq!(summary.gaps[0].kind, GapKind::Pause);
}

#[test]
fn pause_and_resume_without_a_recording_are_refused() {
    let mut r = rig();
    assert_plain(&r.worker.pause_recording().expect_err("nothing to pause"));
    assert_plain(&r.worker.resume_recording().expect_err("nothing to resume"));
}

// ---- AC-AXHK2R.2: charge and energy skip the pause ----

#[test]
fn charge_and_energy_do_not_count_the_paused_time() {
    let mut r = rig();
    let file = r.start(Limits::default());
    r.run(1_000);
    r.worker.pause_recording().expect("pause");
    let n = r.rows() as usize;
    r.run(4_000);
    r.worker.resume_recording().expect("resume");
    r.run(1_000);
    r.worker.stop_recording().expect("stop");

    let lines = file.lines();
    let (before, after) = (&lines[n], &lines[n + 1]);
    assert_eq!(column(before, 9), column(after, 9), "charge bridged");
    assert_eq!(column(before, 10), column(after, 10), "energy bridged");

    let last = lines.last().unwrap();
    let paused_ms = elapsed_ms(after) - elapsed_ms(before);
    assert!(paused_ms >= 3_900, "{paused_ms} ms");
    let active_h = (elapsed_ms(last) - paused_ms) as f64 / 3_600_000.0;
    let charge = column(last, 9);
    assert!(
        (charge - active_h).abs() < active_h * 0.05,
        "{charge} Ah for {active_h} h at 1 A"
    );
    let energy = column(last, 10);
    assert!(
        (energy - 5.0 * active_h).abs() < 5.0 * active_h * 0.05,
        "{energy} Wh for {active_h} h at 5 V 1 A"
    );
}

// ---- AC-AXHK2R.3: auto-stop ----

#[test]
fn a_recording_ends_when_current_stays_low_keeping_the_file_and_saying_why() {
    let mut r = rig();
    let file = r.start(auto_stop());
    r.run(500);
    r.amps(LOW_A);
    r.run(800);
    assert!(r.worker.is_recording(), "stopped before the wait was over");
    r.run(500);
    assert!(!r.worker.is_recording(), "still recording");

    let stopped = r.stopped();
    assert_eq!(stopped.len(), 1, "one notice: {stopped:?}");
    assert_eq!(stopped[0].ended, EndReason::LowCurrent);
    assert!(!stopped[0].disconnected);
    assert!(file.closed(), "the file is still open");
    let lines = file.lines();
    assert_eq!(lines.first().map(String::as_str), Some(CSV_HEADER));
    assert_eq!(lines.len() as u64, stopped[0].rows + 1, "rows lost");

    // The stream outlives the recording.
    let seen = samples(&r.events).len();
    assert_eq!(r.run(300), Step::Continue);
    assert!(samples(&r.events).len() > seen, "the stream stopped");
    assert!(r.log.lock().unwrap().of_type(0x0F).is_empty());
}

#[test]
fn current_that_comes_back_restarts_the_wait() {
    let mut r = rig();
    let _file = r.start(auto_stop());
    r.run(300);
    r.amps(LOW_A);
    r.run(700);
    r.amps(1.0);
    r.run(100);
    r.amps(LOW_A);
    r.run(700);
    assert!(r.worker.is_recording(), "the wait did not restart");
    r.run(500);
    assert!(!r.worker.is_recording(), "the second wait never ended");
}

#[test]
fn current_at_the_set_value_is_not_below_it() {
    let mut r = rig();
    let _file = r.start(Limits {
        below_a: Some(0.25),
        for_ms: WAIT_MS,
        ..Limits::default()
    });
    r.amps(0.25);
    r.run(2_000);
    assert!(r.worker.is_recording(), "0.25 A counted as below 0.25 A");
}

#[test]
fn auto_stop_reads_the_magnitude_of_the_current() {
    let mut r = rig();
    let _file = r.start(auto_stop());
    r.amps(-1.0);
    r.run(2_000);
    assert!(r.worker.is_recording(), "-1 A counted as low current");
    r.amps(-LOW_A);
    r.run(1_300);
    assert!(!r.worker.is_recording(), "-0.05 A did not count as low");
    assert_eq!(r.stopped()[0].ended, EndReason::LowCurrent);
}

#[test]
fn low_current_before_the_recording_does_not_count() {
    let mut r = rig();
    r.amps(LOW_A);
    r.run(2_000);
    let _file = r.start(auto_stop());
    r.run(800);
    assert!(r.worker.is_recording(), "time before the file counted");
    r.run(500);
    assert!(!r.worker.is_recording());
}

#[test]
fn without_auto_stop_a_recording_stays_open_at_no_current() {
    let mut r = rig();
    r.amps(0.0);
    let _file = r.start(Limits::default());
    r.run(3_000);
    assert!(r.worker.is_recording());
}

#[test]
fn the_limits_are_checked() {
    let with_below = |a: f64| Limits {
        below_a: Some(a),
        for_ms: 1_000,
        ..Limits::default()
    };
    assert!(Limits::default().check().is_ok());
    assert!(with_below(0.1).check().is_ok());
    for bad in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let err = with_below(bad).check().expect_err("a bad current");
        assert_plain(&err);
    }
    let no_wait = Limits {
        for_ms: 0,
        ..with_below(0.1)
    };
    assert_plain(&no_wait.check().expect_err("no time set"));
    assert_plain(&row_limit(0).check().expect_err("no rows"));
}

#[test]
fn the_default_limits_stop_at_ten_million_rows_and_nothing_else() {
    assert_eq!(DEFAULT_MAX_ROWS, 10_000_000);
    let limits = Limits::default();
    assert_eq!(limits.below_a, None);
    assert_eq!(limits.max_rows, DEFAULT_MAX_ROWS);
}

#[test]
fn a_start_with_bad_limits_is_refused_and_records_nothing() {
    let mut r = rig();
    let file = Csv::default();
    let bad = Limits {
        below_a: Some(0.0),
        for_ms: 1_000,
        ..Limits::default()
    };
    let err = r
        .worker
        .start_recording_with(file.writer(), bad)
        .expect_err("0 A");
    assert_plain(&err);
    assert!(!r.worker.is_recording());
    assert!(file.lines().is_empty(), "a refused start wrote the header");
}

// ---- AC-AXHK2R.4: auto-stop in the backend, on every record ----

#[test]
fn auto_stop_acts_on_the_record_that_ends_the_wait_not_on_a_later_poll() {
    let mut r = rig();
    let file = r.start(auto_stop());
    r.run(400);
    r.amps(LOW_A);
    r.run(1_500);
    assert_eq!(r.stopped().len(), 1, "no page asked, and no stop came");

    let lines = file.lines();
    let first_low = lines[1..]
        .iter()
        .find(|l| column(l, 3) < BELOW_A)
        .expect("a low row");
    let held = elapsed_ms(lines.last().unwrap()) - elapsed_ms(first_low);
    assert!(
        held.abs_diff(WAIT_MS) <= 3,
        "the file ends {held} ms into a {WAIT_MS} ms wait"
    );
}

#[test]
fn paused_time_does_not_count_toward_auto_stop() {
    let mut r = rig();
    let _file = r.start(auto_stop());
    r.run(300);
    r.amps(LOW_A);
    r.run(600);
    r.worker.pause_recording().expect("pause");
    r.run(3_000);
    assert!(r.worker.is_recording(), "the paused time counted");
    r.worker.resume_recording().expect("resume");
    // The wait starts over: 600 ms before the pause do not count either.
    r.run(800);
    assert!(r.worker.is_recording(), "a resume kept the old wait");
    r.run(500);
    assert!(
        !r.worker.is_recording(),
        "the wait after the resume never ended"
    );
    assert_eq!(r.stopped()[0].ended, EndReason::LowCurrent);
}

#[test]
fn a_stream_gap_restarts_the_wait() {
    let mut r = rig();
    let _file = r.start(auto_stop());
    r.run(300);
    r.amps(LOW_A);
    r.run(700);
    r.knobs.stall_ms.store(500, Ordering::SeqCst);
    r.run(100);
    assert!(r.gap_events() > 0, "the stall left no gap");
    r.run(700);
    assert!(r.worker.is_recording(), "the wait crossed a stream gap");
    r.run(500);
    assert!(
        !r.worker.is_recording(),
        "the wait after the gap never ended"
    );
}

#[test]
fn a_new_measuring_run_restarts_the_wait() {
    let mut r = rig();
    r.run(1_500); // device time to step back from
    let _file = r.start(auto_stop());
    r.run(300);
    r.amps(LOW_A);
    r.run(700);
    r.knobs.rewind_ms.store(1_000, Ordering::SeqCst);
    r.run(700);
    assert!(r.worker.is_recording(), "the wait crossed a new run");
    r.run(500);
    assert!(
        !r.worker.is_recording(),
        "the wait in the new run never ended"
    );
}

// ---- AC-AXHK2R.5: the row limit ----

#[test]
fn a_recording_ends_at_exactly_the_row_limit_keeping_the_file_and_saying_why() {
    let mut r = rig();
    let file = r.start(row_limit(1_000));
    r.run(3_000);
    assert!(!r.worker.is_recording(), "still recording");
    let stopped = r.stopped();
    assert_eq!(stopped.len(), 1, "one notice: {stopped:?}");
    assert_eq!(stopped[0].ended, EndReason::RowLimit);
    assert!(!stopped[0].disconnected);
    assert_eq!(stopped[0].rows, 1_000);
    assert_eq!(file.lines().len(), 1_001, "the rows in the file");
    assert!(file.closed(), "the file is still open");
    assert_eq!(r.run(300), Step::Continue, "the stream stopped");
}

#[test]
fn the_records_after_the_limit_in_the_same_poll_are_not_written() {
    let mut r = rig();
    let file = r.start(row_limit(1));
    r.run(500);
    assert_eq!(r.stopped()[0].rows, 1);
    assert_eq!(file.lines().len(), 2);
}

#[test]
fn paused_records_do_not_count_toward_the_row_limit() {
    let mut r = rig();
    let file = r.start(row_limit(600));
    r.run(300);
    r.worker.pause_recording().expect("pause");
    r.run(1_500);
    assert!(r.worker.is_recording(), "paused records counted");
    r.worker.resume_recording().expect("resume");
    r.run(1_000);
    assert!(!r.worker.is_recording());
    assert_eq!(r.stopped()[0].rows, 600);
    assert_eq!(file.lines().len(), 601);
}

// ---- the end reason ----

#[test]
fn a_stop_the_user_asked_for_ends_as_stopped() {
    let mut r = rig();
    let _file = r.start(auto_stop());
    r.run(300);
    let summary = r.worker.stop_recording().expect("stop");
    assert_eq!(summary.ended, EndReason::Stopped);
    assert!(!summary.disconnected);
    assert!(r.stopped().is_empty(), "the stop's answer is the news");
}

#[test]
fn an_unplug_ends_as_disconnected_even_while_paused() {
    let mut r = rig_with(FaultPlan::new().disconnect_after_polls(60));
    let file = r.start(Limits::default());
    r.run(100);
    r.worker.pause_recording().expect("pause");
    assert_eq!(r.run(30_000), Step::Ended);
    let stopped = r.stopped();
    assert_eq!(stopped.len(), 1, "{stopped:?}");
    assert_eq!(stopped[0].ended, EndReason::Disconnected);
    assert!(stopped[0].disconnected);
    assert!(file.closed());
}

#[test]
fn the_end_reason_and_the_pause_serialize_for_the_ui() {
    let text = |e: EndReason| serde_json::to_value(e).unwrap();
    assert_eq!(text(EndReason::Stopped), "stopped");
    assert_eq!(text(EndReason::Disconnected), "disconnected");
    assert_eq!(text(EndReason::LowCurrent), "low_current");
    assert_eq!(text(EndReason::RowLimit), "row_limit");
    assert_eq!(serde_json::to_value(GapKind::Pause).unwrap(), "pause");

    let v = serde_json::to_value(BackendEvent::Recording(RecordingEvent::Stopped(
        RecordingSummary {
            rows: 7,
            gaps: vec![],
            disconnected: false,
            ended: EndReason::LowCurrent,
        },
    )))
    .unwrap();
    assert_eq!(v["data"]["ended"], "low_current");
}

// ---- AC-AXHK2R.6: the file's name and place ----

fn fresh_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("open-powerz-limits-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn entries(dir: &Path) -> usize {
    std::fs::read_dir(dir).expect("the folder").count()
}

#[test]
fn a_name_may_not_hold_a_path_separator_or_a_character_windows_forbids() {
    for good in [
        "powerz",
        "bench-1",
        "bench 2_a",
        "powerz-20261021-090507.csv",
    ] {
        assert!(recording_name_ok(good), "{good:?} was refused");
    }
    for bad in [
        "a/b", "a\\b", "../up", "a:b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b", "a\tb",
    ] {
        assert!(!recording_name_ok(bad), "{bad:?} was accepted");
    }
}

#[test]
fn a_set_folder_gets_the_file_under_the_proposed_name() {
    let dir = fresh_dir("folder");
    std::fs::create_dir_all(&dir).unwrap();
    let path = folder_recording_path(&dir, "bench-1.csv").expect("a path");
    assert_eq!(path, dir.join("bench-1.csv"));
    assert!(path.is_file(), "the file was not claimed");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_set_folder_never_takes_an_existing_file() {
    let dir = fresh_dir("folder-taken");
    std::fs::create_dir_all(&dir).unwrap();
    let first = folder_recording_path(&dir, "bench-1.csv").expect("a path");
    std::fs::write(&first, "keep me").unwrap();
    let second = folder_recording_path(&dir, "bench-1.csv").expect("a path");
    assert_ne!(first, second);
    assert_eq!(second.parent(), Some(dir.as_path()));
    assert_eq!(std::fs::read_to_string(&first).unwrap(), "keep me");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_folder_that_does_not_exist_is_refused_not_created() {
    let dir = fresh_dir("gone");
    let err = folder_recording_path(&dir, "bench-1.csv").expect_err("no such folder");
    assert_plain(&err);
    assert!(!dir.exists(), "the missing folder was created");
}

#[test]
fn a_file_in_place_of_the_folder_is_refused() {
    let base = fresh_dir("not-a-folder");
    std::fs::create_dir_all(&base).unwrap();
    let file = base.join("file");
    std::fs::write(&file, "x").unwrap();
    assert!(folder_recording_path(&file, "bench-1.csv").is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "x");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn a_name_windows_would_refuse_or_change_or_a_hidden_one_is_refused() {
    for bad in [
        "con.csv",
        "NUL.csv",
        "aux.csv",
        "com1.csv",
        "LPT9.csv",
        "con.log.csv",
        "run .csv",
        "run..csv",
        ".hidden.csv",
    ] {
        assert!(!recording_name_ok(bad), "{bad:?} was accepted");
    }
    for good in ["console.csv", "com10.csv", "comet.csv", "run.1.csv"] {
        assert!(recording_name_ok(good), "{good:?} was refused");
    }
}

#[test]
fn a_name_that_leaves_the_folder_is_refused() {
    let base = fresh_dir("escape");
    let dir = base.join("runs");
    std::fs::create_dir_all(&dir).unwrap();
    assert!(folder_recording_path(&dir, "../evil.csv").is_err());
    assert_eq!(entries(&dir), 0);
    assert_eq!(entries(&base), 1, "a file landed outside the folder");
    let _ = std::fs::remove_dir_all(base);
}

#[cfg(unix)]
#[test]
fn an_unwritable_folder_is_refused_at_the_start() {
    use std::os::unix::fs::PermissionsExt;
    let dir = fresh_dir("read-only");
    std::fs::create_dir_all(&dir).unwrap();
    let mode = |m| std::fs::Permissions::from_mode(m);
    std::fs::set_permissions(&dir, mode(0o555)).unwrap();
    let writable = std::fs::File::create(dir.join("probe")).is_ok(); // root writes anywhere
    let answer = folder_recording_path(&dir, "bench-1.csv");
    std::fs::set_permissions(&dir, mode(0o755)).unwrap();
    if !writable {
        let err = answer.expect_err("a read-only folder");
        assert_eq!(err.kind, ErrorKind::PermissionDenied);
        assert_plain(&err);
    }
    let _ = std::fs::remove_dir_all(dir);
}

// ---- the commands, through the manager ----

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

fn connect(m: &mut DeviceManager<SimOpener, SimClock>) -> (String, mpsc::Receiver<BackendEvent>) {
    let (tx, rx) = mpsc::channel();
    m.connect("demo:load-5v-1a", 50, move |e| {
        let _ = tx.send(e);
    })
    .expect("connect");
    (m.target(None), rx)
}

/// The next event `pick` takes, within 10 s of real time.
fn wait_for<R>(rx: &mpsc::Receiver<BackendEvent>, pick: impl Fn(&BackendEvent) -> Option<R>) -> R {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(e) => {
                if let Some(found) = pick(&e) {
                    return found;
                }
            }
            Err(e) => panic!("the event never came ({e:?})"),
        }
    }
}

#[test]
fn the_control_starts_a_recording_with_limits_and_the_rule_ends_it() {
    let dir = fresh_dir("control");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bench.csv");
    let mut m = manager();
    let (id, rx) = connect(&mut m);
    m.control_for(&id)
        .unwrap()
        .start_recording_with(&path, row_limit(5))
        .expect("start");
    let summary = wait_for(&rx, |e| match e {
        BackendEvent::Recording(RecordingEvent::Stopped(s)) => Some(s.clone()),
        _ => None,
    });
    assert_eq!(summary.ended, EndReason::RowLimit);
    assert_eq!(summary.rows, 5);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap().lines().count(),
        6,
        "the file stays where it is"
    );
    m.disconnect().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_control_refuses_bad_limits_before_touching_the_file() {
    let dir = fresh_dir("control-refused");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bench.csv");
    let mut m = manager();
    let (id, _rx) = connect(&mut m);
    let err = m
        .control_for(&id)
        .unwrap()
        .start_recording_with(&path, row_limit(0))
        .expect_err("no rows");
    assert_plain(&err);
    assert!(!path.exists(), "a refused start created the file");
    m.disconnect().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_page_that_reattaches_sees_the_pause() {
    let dir = fresh_dir("attach");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bench.csv");
    let mut m = manager();
    let (id, rx) = connect(&mut m);
    m.start_recording_for(&id, &path).expect("start");
    wait_for(&rx, |e| match e {
        BackendEvent::Recording(RecordingEvent::Progress { rows, .. }) if *rows > 0 => Some(()),
        _ => None,
    });

    m.pause_recording_for(&id).expect("pause");
    let attached = m.attach(|_: BackendEvent| {}).expect("a session");
    assert!(attached.recording.expect("a recording").paused);

    m.resume_recording_for(&id).expect("resume");
    let attached = m.attach(|_: BackendEvent| {}).expect("a session");
    assert!(!attached.recording.expect("a recording").paused);

    let control = m.control_for(&id).unwrap();
    control
        .pause_recording()
        .expect("pause through the control");
    control
        .resume_recording()
        .expect("resume through the control");
    m.stop_recording_for(&id).expect("stop");
    assert_plain(&control.pause_recording().expect_err("nothing to pause"));
    m.disconnect().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

/// The source of the Tauri command `name`, up to the next command.
fn command_body(name: &str) -> &'static str {
    let source = include_str!("../src/lib.rs");
    let from = source
        .find(&format!("fn {name}"))
        .unwrap_or_else(|| panic!("the {name} command"));
    let rest = &source[from..];
    &rest[..rest.find("#[tauri::command]").unwrap_or(rest.len())]
}

#[test]
fn the_commands_are_registered_and_start_recording_takes_the_options() {
    let source = include_str!("../src/lib.rs");
    let handlers = source
        .split("generate_handler![")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("the handler list");
    for name in [
        "recording_pause",
        "recording_resume",
        "choose_recording_folder",
    ] {
        assert!(handlers.contains(name), "{name} is not registered");
    }
    let start = command_body("start_recording");
    assert!(
        start.contains("options"),
        "start_recording takes no options"
    );
    assert!(
        start.contains("folder_recording_path"),
        "a set folder is not used"
    );
    assert!(
        start.contains("recording_name_ok"),
        "the backend does not check the name"
    );
    assert!(
        start.contains(".check()"),
        "the backend does not check the limits"
    );
    assert!(
        !start.contains("dialog()"),
        "start_recording opens a dialog"
    );
}

#[test]
fn powerz_stream_does_not_learn_of_the_limits() {
    // The CLI's recording is the shared writer; the limits live in the app's Recorder.
    let source = include_str!("../../../../crates/powerz-record/src/lib.rs");
    for word in ["below_a", "max_rows", "paused"] {
        assert!(!source.contains(word), "{word} reached powerz-record");
    }
}
