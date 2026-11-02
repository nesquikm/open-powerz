//! AC-4PJRVM.5: app.log records the session events (connect, disconnect,
//! replug, errors) so it can serve as evidence. The manager writes through an
//! `AppLog`, one timestamped line per event, naming the meter. Simulators
//! only; synthetic serials.

#![allow(dead_code)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;

use common::*;
use open_powerz_desktop_lib::app_log::AppLog;
use open_powerz_desktop_lib::trigger::TriggerWorker;
use open_powerz_desktop_lib::ui_error::{ErrorKind, UiError};
use open_powerz_desktop_lib::worker::WorkerControl;
use powerz_protocol::frame::{self, Attribute};
use powerz_sim::{SimCharger, SimPdm};

include!("common/two_meters.inc.rs");

fn log_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("open-powerz-applog-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("app.log")
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|s| s.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The first line with `word` (any case) and, when given, `id`; waits for it.
fn wait_line(path: &Path, word: &str, id: Option<&str>) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let found = lines(path)
            .into_iter()
            .find(|l| l.to_lowercase().contains(word) && id.is_none_or(|i| l.contains(i)));
        if let Some(line) = found {
            return line;
        }
        assert!(
            Instant::now() < deadline,
            "no {word:?} line in app.log: {:#?}",
            lines(path)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn rig_logging_to(path: &Path) -> Rig {
    let mut rig = Rig::new();
    rig.m.set_app_log(AppLog::open(path).expect("open app.log"));
    rig
}

#[test]
fn a_connect_and_a_disconnect_are_logged_with_time_and_meter() {
    let path = log_path("connect");
    let mut rig = rig_logging_to(&path);
    let (_rx, _) = stream(&mut rig, METER_A);
    let connected = wait_line(&path, "connect", Some(METER_A));
    assert!(
        connected.chars().take(4).all(|c| c.is_ascii_digit()),
        "a line starts with its timestamp: {connected}"
    );
    rig.m.disconnect_for(METER_A).unwrap();
    wait_line(&path, "disconnect", Some(METER_A));
}

#[test]
fn an_unplug_and_the_replug_are_logged() {
    let path = log_path("replug");
    let mut rig = rig_logging_to(&path);
    let (rx, _) = stream(&mut rig, METER_A);
    rig.unplug_a();
    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
    wait_line(&path, "unplug", Some(METER_A));
    let mut back = rig.replug_a();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !back && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
        let list = rig.m.list_devices();
        back = rig.m.devices_changed(&list);
    }
    assert!(back, "meter A did not come back");
    wait_line(&path, "replug", None);
    for id in rig.m.connected_ids() {
        rig.m.disconnect_for(&id).unwrap();
    }
}

#[test]
fn a_failed_connect_is_logged_as_an_error() {
    let path = log_path("error");
    let mut rig = rig_logging_to(&path);
    // Listed by the OS, but the opener cannot reach it.
    rig.plug_a.store(false, Ordering::SeqCst);
    let (_rx, sink) = channel_sink();
    let _ = rig.m.connect(METER_A, 50, sink);
    wait_line(&path, "error", Some(METER_A));
}

#[test]
fn a_new_app_run_adds_to_the_log_and_keeps_what_is_there() {
    let path = log_path("append");
    std::fs::write(&path, "2026-10-30T10:00:00Z earlier run\n").unwrap();
    let mut rig = rig_logging_to(&path);
    let (_rx, _) = stream(&mut rig, METER_A);
    wait_line(&path, "connect", Some(METER_A));
    assert_eq!(lines(&path)[0], "2026-10-30T10:00:00Z earlier run");
    rig.m.disconnect_for(METER_A).unwrap();
}

#[test]
fn the_app_logs_to_a_file_named_app_log() {
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("\"app.log\""), "lib.rs names app.log");
    assert!(source.contains("set_app_log"), "lib.rs wires the log in");
}

// ---- FR AXHK2G: errors, replugs and the size of the log ----

const MODEL: &str = "KM003C";

/// What the test does to meter A's line while its session runs.
#[derive(Clone, Default)]
struct Line {
    /// Refuse QUEUE STOP: a rate change then fails while the queue, and so
    /// the session, goes on.
    refuse_stop: Arc<AtomicBool>,
    /// How many of the next ADC snapshots the meter refuses, still on the
    /// line.
    refuse_snapshots: Arc<AtomicUsize>,
    /// How many ADC snapshots the meter answered.
    served_snapshots: Arc<AtomicUsize>,
    /// Hold the worker inside its next request, so it answers no command.
    stall: Arc<AtomicBool>,
    /// The worker is held.
    stalled: Arc<AtomicBool>,
    /// Refuse the PD sniffer's start (0x10) and stop (0x11) (AC-AXHK2N.16).
    refuse_pd: Arc<AtomicBool>,
}

impl Line {
    fn wait_while_stalled(&self) {
        if !self.stall.load(Ordering::SeqCst) {
            return;
        }
        self.stalled.store(true, Ordering::SeqCst);
        // Bounded: a test that fails must not hang its worker.
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.stall.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        self.stalled.store(false, Ordering::SeqCst);
    }

    /// Returns once the worker is held inside a request.
    fn hold_the_worker(&self) {
        self.stall.store(true, Ordering::SeqCst);
        wait_until("the worker to reach the stall", || {
            self.stalled.load(Ordering::SeqCst)
        });
    }

    fn let_the_worker_go(&self) {
        self.stall.store(false, Ordering::SeqCst);
    }
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "waited too long for {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Meter A's simulator behind a line the test can fault.
struct RefusingPipe {
    pipe: Pipe,
    line: Line,
}

impl Transport for RefusingPipe {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.line.wait_while_stalled();
        let refused = Ok(vec![0x06, request.get(1).copied().unwrap_or(0), 0x00, 0x00]);
        let stop = request.first().map(|b| b & 0x7F) == Some(0x0F);
        if stop && self.line.refuse_stop.load(Ordering::SeqCst) {
            return refused;
        }
        let analyzer = matches!(request.first().map(|b| b & 0x7F), Some(0x10 | 0x11));
        if analyzer && self.line.refuse_pd.load(Ordering::SeqCst) {
            return refused;
        }
        let snapshot =
            frame::get_data_mask(request).is_some_and(|m| m.bits() == Attribute::ADC.bits());
        if snapshot {
            let refuse =
                self.line
                    .refuse_snapshots
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
            if refuse.is_ok() {
                return refused;
            }
            self.line.served_snapshots.fetch_add(1, Ordering::SeqCst);
        }
        self.pipe.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.pipe.read(timeout)
    }

    fn settings_pipe(&mut self) -> Option<Result<Box<dyn Transport>, transport::Error>> {
        self.pipe.settings_pipe()
    }
}

struct FaultOpener {
    clock: SimClock,
    faults: FaultPlan,
    line: Line,
}

impl Opener for FaultOpener {
    type Transport = RefusingPipe;
    fn open(&self, _device: &DeviceListing) -> Result<RefusingPipe, transport::Error> {
        Ok(RefusingPipe {
            pipe: Pipe::sim(
                Scenario::Load5v1a,
                self.faults.clone(),
                &self.clock,
                PipeOpts::default(),
            ),
            line: self.line.clone(),
        })
    }
}

/// Meter A streaming under `faults`, logging to `path`; the connect's line is
/// in the log when this returns.
fn faulty_session(
    path: &Path,
    faults: FaultPlan,
) -> (
    DeviceManager<FaultOpener, SimClock>,
    Line,
    mpsc::Receiver<BackendEvent>,
) {
    let clock = clock();
    let line = Line::default();
    let mut m = DeviceManager::new(
        || Ok(vec![meter(SERIAL_A, "1", 7)]),
        FaultOpener {
            clock: clock.clone(),
            faults,
            line: line.clone(),
        },
        clock,
    );
    m.set_app_log(AppLog::open(path).expect("open app.log"));
    let (rx, sink) = channel_sink();
    m.connect(METER_A, 50, sink).expect("connect");
    recv_until(&rx, is_samples);
    wait_line(path, "connect", Some(METER_A));
    (m, line, rx)
}

/// `error` added exactly one line after the first `before` ones, naming
/// meter A, its model and the error's kind.
fn assert_one_failure_line(path: &Path, before: usize, error: &UiError) {
    assert_one_line_of_kind(path, before, &kind_of(error));
}

/// The error kind by the name the UI gets.
fn kind_of(error: &UiError) -> String {
    let kind = serde_json::to_value(error.kind).expect("a kind serialises");
    kind.as_str().expect("a kind is a name").to_owned()
}

/// A failure of `kind` added exactly one line after the first `before` ones.
fn assert_one_line_of_kind(path: &Path, before: usize, kind: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while lines(path).len() <= before {
        assert!(
            Instant::now() < deadline,
            "{kind} left no line in app.log: {:#?}",
            lines(path)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // Long enough for a second line to show up if there is one.
    std::thread::sleep(Duration::from_millis(100));
    let all = lines(path);
    let added = &all[before..];
    assert_eq!(added.len(), 1, "one line for one failure: {added:#?}");
    assert!(added[0].contains(METER_A), "names the meter: {}", added[0]);
    assert!(added[0].contains(MODEL), "names the model: {}", added[0]);
    assert!(added[0].contains(kind), "names {kind}: {}", added[0]);
    assert!(
        !added[0].contains(SERIAL_A),
        "a serial in the log: {}",
        added[0]
    );
}

/// AC-AXHK2G.2. Through the control the `set_rate` command uses.
#[test]
fn a_failed_rate_change_adds_one_line_naming_the_meter_and_the_kind() {
    let path = log_path("rate");
    let (m, line, _rx) = faulty_session(&path, FaultPlan::new());
    let before = lines(&path).len();
    line.refuse_stop.store(true, Ordering::SeqCst);
    let (control, rate) = m.rate_control_for(METER_A, 10).expect("the rate is valid");
    let error = control.set_rate(rate).expect_err("the meter refuses");
    assert_one_failure_line(&path, before, &error);
    assert_keeps_its_message(&path, &error);
    line.refuse_stop.store(false, Ordering::SeqCst);
}

/// AC-AXHK2G.2. Through the control the settings command uses.
#[test]
fn a_failed_settings_write_adds_one_line_naming_the_meter_and_the_kind() {
    let path = log_path("settings");
    let faults = FaultPlan::new().ignore_settings_write();
    let (m, _, _rx) = faulty_session(&path, faults);
    let before = lines(&path).len();
    let changes = std::collections::BTreeMap::from([("brightness".to_owned(), 80)]);
    let error = m
        .control_for(METER_A)
        .expect("connected")
        .write_settings(&changes)
        .expect_err("the read-back differs");
    assert_one_failure_line(&path, before, &error);
    assert_keeps_its_message(&path, &error);
}

/// AC-AXHK2G.2. Through the control the record delete command uses.
#[test]
fn a_failed_record_delete_adds_one_line_naming_the_meter_and_the_kind() {
    let path = log_path("delete");
    let faults = FaultPlan::new().ignore_table_write();
    let (m, _, _rx) = faulty_session(&path, faults);
    let control = m.control_for(METER_A).expect("connected");
    let listed = serde_json::to_value(control.list_records().expect("list")).unwrap();
    let name = listed["records"][0]["name"].as_str().expect("a record");
    let before = lines(&path).len();
    let backup = path.with_file_name("backup");
    let error = control
        .delete_records(Some(1), name, &backup)
        .expect_err("the table did not change");
    assert_one_failure_line(&path, before, &error);
}

/// AC-AXHK2G.7: a rate or settings message is fixed text, so its line keeps it.
fn assert_keeps_its_message(path: &Path, error: &UiError) {
    let all = lines(path);
    let last = all.last().expect("a line");
    assert!(last.contains(&error.message), "no message: {last}");
}

/// The error of a rate change refused before the worker.
fn refused_rate(
    result: Result<(WorkerControl, powerz_protocol::decode::Rate), UiError>,
) -> UiError {
    match result {
        Ok(_) => panic!("the rate change was not refused"),
        Err(error) => error,
    }
}

/// AC-AXHK2G.2: the rate is refused before the worker hears of it.
#[test]
fn a_rate_the_meter_does_not_support_adds_one_line_naming_the_meter_and_the_kind() {
    let path = log_path("rate-unsupported");
    let (m, _, _rx) = faulty_session(&path, FaultPlan::new());
    let before = lines(&path).len();
    let error = refused_rate(m.rate_control_for(METER_A, 7));
    assert_eq!(error.kind, ErrorKind::Usage);
    assert_one_failure_line(&path, before, &error);
}

/// AC-AXHK2G.2: the rate is refused before the worker hears of it.
#[test]
fn a_rate_refused_for_the_open_trigger_panel_adds_one_line_naming_the_meter_and_the_kind() {
    let path = log_path("rate-trigger");
    let (mut m, _, _rx) = faulty_session(&path, FaultPlan::new());
    let pdm = SimPdm::new(SimCharger::Epr140W, SimClock::new(T0));
    let trigger = TriggerWorker::spawn(pdm.open_port(), SimClock::new(T0)).expect("trigger opens");
    m.install_trigger_for(METER_A, trigger).expect("install");
    let before = lines(&path).len();
    let error = refused_rate(m.rate_control_for(METER_A, 1000));
    assert_eq!(error.kind, ErrorKind::Usage);
    assert_one_failure_line(&path, before, &error);
}

/// AC-AXHK2G.2: a meter waiting for its replug has no worker to refuse.
#[test]
fn a_rate_change_on_a_meter_with_no_session_adds_one_line_naming_the_meter_and_the_kind() {
    let path = log_path("rate-no-session");
    let mut rig = rig_logging_to(&path);
    let (rx, _) = stream(&mut rig, METER_A);
    pull_a(&mut rig, &rx);
    wait_line(&path, "unplug", Some(METER_A));
    let before = lines(&path).len();
    let error = refused_rate(rig.m.rate_control_for(METER_A, 10));
    assert_one_failure_line(&path, before, &error);
}

/// AC-AXHK2G.2: the caller's `not_connected`, from a worker that has gone,
/// for each of the three requests.
#[test]
fn a_request_to_a_worker_that_has_gone_adds_one_line_naming_the_meter_and_the_kind() {
    let path = log_path("not-connected");
    let (mut m, _, _rx) = faulty_session(&path, FaultPlan::new());
    let (control, rate) = m.rate_control_for(METER_A, 10).expect("the rate is valid");
    m.disconnect_for(METER_A).expect("disconnect");
    wait_line(&path, "disconnect", Some(METER_A));

    let before = lines(&path).len();
    let error = control.set_rate(rate).expect_err("no worker");
    assert_eq!(error.message, "The meter is not connected.");
    assert_one_failure_line(&path, before, &error);

    let before = lines(&path).len();
    let changes = std::collections::BTreeMap::from([("brightness".to_owned(), 80)]);
    let error = control.write_settings(&changes).expect_err("no worker");
    assert_eq!(error.message, "The meter is not connected.");
    assert_one_failure_line(&path, before, &error);

    let before = lines(&path).len();
    let backup = path.with_file_name("backup");
    let error = control
        .delete_records(Some(1), "any record", &backup)
        .expect_err("no worker");
    assert_eq!(error.message, "The meter is not connected.");
    assert_one_failure_line(&path, before, &error);
}

/// AC-AXHK2G.2: `busy`, then the worker fails the same request after the
/// caller gave up. One request reads as one error, the one its caller got.
/// Commands run in order, so the second answer comes after the first failed,
/// and the log writes in that order too.
#[test]
fn a_rate_change_that_timed_out_and_then_failed_adds_one_line() {
    let path = log_path("rate-busy");
    let (m, line, _rx) = faulty_session(&path, FaultPlan::new());
    let (control, rate) = m.rate_control_for(METER_A, 10).expect("the rate is valid");
    line.refuse_stop.store(true, Ordering::SeqCst);
    line.hold_the_worker();
    let before = lines(&path).len();
    let error = control.set_rate(rate).expect_err("the worker is held");
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert_one_failure_line(&path, before, &error);

    line.let_the_worker_go();
    let before = lines(&path).len();
    let error = control.set_rate(rate).expect_err("the meter refuses");
    assert_eq!(error.kind, ErrorKind::Protocol);
    assert_one_failure_line(&path, before, &error);
    line.refuse_stop.store(false, Ordering::SeqCst);
}

/// AC-AXHK2G.2: the same for a settings write.
#[test]
fn a_settings_write_that_timed_out_and_then_failed_adds_one_line() {
    let path = log_path("settings-busy");
    let faults = FaultPlan::new().ignore_settings_write();
    let (m, line, _rx) = faulty_session(&path, faults);
    let control = m.control_for(METER_A).expect("connected");
    let changes = std::collections::BTreeMap::from([("brightness".to_owned(), 80)]);
    line.hold_the_worker();
    let before = lines(&path).len();
    let error = control
        .write_settings(&changes)
        .expect_err("the worker is held");
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert_one_failure_line(&path, before, &error);

    line.let_the_worker_go();
    let before = lines(&path).len();
    let error = control
        .write_settings(&changes)
        .expect_err("the read-back differs");
    assert_ne!(error.kind, ErrorKind::Timeout);
    assert_one_failure_line(&path, before, &error);
}

/// AC-AXHK2G.2: a failed snapshot keeps the session and is logged like a
/// failed poll, once per run of failures.
#[test]
fn a_failed_snapshot_keeps_the_session_and_adds_one_line_per_run_of_failures() {
    let path = log_path("snapshot");
    let (m, line, rx) = faulty_session(&path, FaultPlan::new());
    let refused = || line.refuse_snapshots.load(Ordering::SeqCst) == 0;
    let answered_one_more = || {
        let served = line.served_snapshots.load(Ordering::SeqCst);
        wait_until("a good snapshot", || {
            line.served_snapshots.load(Ordering::SeqCst) > served
        });
    };

    let before = lines(&path).len();
    line.refuse_snapshots.store(3, Ordering::SeqCst);
    wait_until("the snapshots to be refused", refused);
    answered_one_more();
    assert_one_line_of_kind(&path, before, "protocol");

    // A good snapshot ended that run: this one is news.
    let before = lines(&path).len();
    line.refuse_snapshots.store(1, Ordering::SeqCst);
    wait_until("the snapshot to be refused", refused);
    answered_one_more();
    assert_one_line_of_kind(&path, before, "protocol");

    assert!(m.connected_ids().iter().any(|id| id == METER_A));
    drain(&rx);
    recv_until(&rx, |e| {
        assert!(
            !matches!(e, BackendEvent::Status(Status::Disconnected { .. })),
            "the error ended the session"
        );
        is_samples(e)
    });
}

/// A delete of meter A's first record that failed, on a session still open.
struct FailedDelete {
    path: PathBuf,
    before: usize,
    error: UiError,
    folder: PathBuf,
    name: String,
    _session: (
        DeviceManager<FaultOpener, SimClock>,
        mpsc::Receiver<BackendEvent>,
    ),
}

/// Fail a delete, with a folder and a record name a line could quote. `block`
/// readies the folder.
fn failed_delete(tag: &str, faults: FaultPlan, block: impl FnOnce(&Path, &str)) -> FailedDelete {
    let path = log_path(tag);
    let (m, _, rx) = faulty_session(&path, faults);
    let control = m.control_for(METER_A).expect("connected");
    let listed = serde_json::to_value(control.list_records().expect("list")).unwrap();
    let name = listed["records"][0]["name"].as_str().expect("a record");
    assert!(!name.is_empty());
    let folder = path.with_file_name("who-am-i-backup");
    block(&folder, name);
    let before = lines(&path).len();
    let error = control
        .delete_records(Some(1), name, &folder)
        .expect_err("the delete fails");
    let name = name.to_owned();
    FailedDelete {
        path,
        before,
        error,
        folder,
        name,
        _session: (m, rx),
    }
}

/// AC-AXHK2G.7: one line, ending in the kind, quoting nothing. The caller
/// still gets the folder: only the log leaves it out.
fn assert_only_the_kind_is_logged(failed: &FailedDelete) {
    let shown = failed.folder.display().to_string();
    let error = &failed.error;
    assert!(error.message.contains(&shown), "{}", error.message);
    assert_one_failure_line(&failed.path, failed.before, error);
    let all = lines(&failed.path);
    let (_time, text) = all[failed.before].split_once(' ').expect("a timestamp");
    assert!(
        text.ends_with(&format!(": {}", kind_of(error))),
        "more than the kind: {text}"
    );
    let name = failed.name.as_str();
    for quoted in ["/", "\\", "who-am-i", name, "os error", "cannot", "Backup"] {
        assert!(!text.contains(quoted), "{quoted:?} in: {text}");
    }
}

#[test]
fn a_delete_that_cannot_make_its_backup_folder_logs_only_the_kind() {
    // A file sits where the folder goes.
    let failed = failed_delete("delete-no-folder", FaultPlan::new(), |folder, _| {
        std::fs::write(folder, "in the way").unwrap();
    });
    assert_only_the_kind_is_logged(&failed);
}

#[test]
fn a_delete_that_cannot_write_its_backup_file_logs_only_the_kind() {
    // A folder sits where the record's file goes.
    let failed = failed_delete("delete-no-file", FaultPlan::new(), |folder, name| {
        let file = powerz_record::offline_record_file_name(1, name);
        std::fs::create_dir_all(folder.join(file)).unwrap();
    });
    assert!(
        failed.error.message.contains(&failed.name),
        "{}",
        failed.error.message
    );
    assert_only_the_kind_is_logged(&failed);
}

#[test]
fn a_delete_the_meter_did_not_apply_logs_only_the_kind() {
    let faults = FaultPlan::new().ignore_table_write();
    let failed = failed_delete("delete-table", faults, |_, _| {});
    assert_only_the_kind_is_logged(&failed);
}

/// The source of command `name`, up to `next`.
fn command_source(name: &str, next: &str) -> &'static str {
    let source = include_str!("../src/lib.rs");
    let start = source.find(name).expect("the command exists");
    let end = source[start..].find(next).expect("the next item");
    &source[start..start + end]
}

/// Neither command has a test runtime, so their wiring is pinned here. What
/// `session_log_for` does is in `tests/session_log_for.rs`.
fn assert_logs_a_refused_control(command: &str) {
    for bare in [" control(&state", "(control(&state"] {
        assert!(
            !command.contains(bare),
            "a refused control reaches no log: {command}"
        );
    }
    assert!(
        include_str!("../src/lib.rs").contains(".session_log_for("),
        "no command logs through the meter's session log"
    );
}

/// AC-AXHK2G.2: the two failures `records_delete` meets before the worker.
#[test]
fn the_delete_command_logs_what_fails_before_the_worker() {
    let command = command_source("async fn records_delete(", "fn backup_dir(");
    assert_logs_a_refused_control(command);
    assert!(
        !command.contains("backup_dir(&app)?"),
        "a backup folder not found reaches no log: {command}"
    );
}

/// AC-AXHK2G.2.
#[test]
fn the_settings_command_logs_what_fails_before_the_worker() {
    let command = command_source("async fn meter_settings_write(", "async fn records_list(");
    assert_logs_a_refused_control(command);
}

/// Pull meter A and wait until the manager waits for it.
fn pull_a(rig: &mut Rig, rx: &mpsc::Receiver<BackendEvent>) {
    rig.unplug_a();
    recv_until(rx, |e| {
        matches!(e, BackendEvent::Status(Status::Waiting { .. }))
    });
    let list = rig.m.list_devices();
    rig.m.devices_changed(&list);
}

/// Plug meter A back in at `address` and wait for its samples.
fn plug_a_at(rig: &mut Rig, rx: &mpsc::Receiver<BackendEvent>, address: u8) {
    rig.plug_a.store(true, Ordering::SeqCst);
    rig.attached
        .lock()
        .unwrap()
        .push(meter(SERIAL_A, "1", address));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let list = rig.m.list_devices();
        if rig.m.devices_changed(&list) {
            break;
        }
        assert!(Instant::now() < deadline, "meter A did not come back");
        std::thread::sleep(Duration::from_millis(10));
    }
    recv_until(rx, is_samples);
}

/// AC-AXHK2G.5: `replug <id>`, or `replug <old> to <new>`, where the old id
/// is the one the meter last had, not the one it first connected at.
#[test]
fn a_replug_names_its_old_and_new_id_only_when_they_differ() {
    let path = log_path("replug-ids");
    let mut rig = rig_logging_to(&path);
    let (rx, _) = stream(&mut rig, METER_A);
    for address in [7, 9, 9, 10] {
        pull_a(&mut rig, &rx);
        plug_a_at(&mut rig, &rx, address);
    }
    let replugs: Vec<String> = lines(&path)
        .into_iter()
        .filter(|l| l.contains("replug"))
        .map(|l| {
            let (_time, text) = l.split_once(' ').expect("a line has a timestamp");
            // Up to the model, which AC-AXHK2G.4 adds to the line.
            let text = text.split(" (").next().unwrap_or(text);
            text.to_owned()
        })
        .collect();
    assert_eq!(
        replugs,
        [
            "replug usb:1:7",
            "replug usb:1:7 to usb:1:9",
            "replug usb:1:9",
            "replug usb:1:9 to usb:1:10",
        ]
    );
    for id in rig.m.connected_ids() {
        rig.m.disconnect_for(&id).unwrap();
    }
}

/// AC-AXHK2G.4: the model on every line that names a meter, and the id the
/// meter really has on a `connected` line after a replug.
#[test]
fn every_line_naming_a_meter_carries_its_model() {
    let path = log_path("model");
    let mut rig = rig_logging_to(&path);
    let (rx, _) = stream(&mut rig, METER_A);
    pull_a(&mut rig, &rx);
    plug_a_at(&mut rig, &rx, 9);
    wait_line(&path, "replug", None);
    for id in rig.m.connected_ids() {
        rig.m.disconnect_for(&id).unwrap();
    }
    wait_line(&path, "disconnect", None);

    let all = lines(&path);
    assert!(
        all.iter().any(|l| l.contains(" connected usb:1:9")),
        "no connected line at the new id: {all:#?}"
    );
    for line in &all {
        assert!(line.contains(MODEL), "no model: {line}");
        assert!(!line.contains(SERIAL_A), "a serial in the log: {line}");
    }
}

/// AC-AXHK2G.4, the `error` line of a connect that failed.
#[test]
fn a_failed_connect_names_the_model() {
    let path = log_path("connect-error");
    let mut rig = rig_logging_to(&path);
    rig.plug_a.store(false, Ordering::SeqCst);
    let (_rx, sink) = channel_sink();
    let _ = rig.m.connect(METER_A, 50, sink);
    let line = wait_line(&path, "error", Some(METER_A));
    assert!(line.contains(MODEL), "no model: {line}");
}

/// `app.log` of `size` bytes starting with `marker`, as a run left it.
fn log_of_size(path: &Path, marker: &str, size: u64) {
    std::fs::write(path, format!("{marker}\n")).unwrap();
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_len(size).unwrap();
}

/// AC-AXHK2G.6.
#[test]
fn a_log_over_5_mb_moves_to_app_log_1_when_the_app_starts() {
    let path = log_path("rotate");
    let rotated = path.with_file_name("app.log.1");
    log_of_size(&path, "the big run", 5_300_000);
    AppLog::open(&path).expect("open app.log").line("a new run");
    let moved = std::fs::read(&rotated).expect("app.log.1 exists");
    assert_eq!(moved.len(), 5_300_000, "the whole log moved");
    assert!(moved.starts_with(b"the big run\n"), "it is the old log");
    let new = lines(&path);
    assert_eq!(new.len(), 1, "the new log starts empty: {}", new.len());
    assert!(new[0].ends_with("a new run"), "{}", new[0]);
}

/// AC-AXHK2G.6.
#[test]
fn a_rotation_replaces_any_earlier_app_log_1() {
    let path = log_path("rotate-replace");
    let rotated = path.with_file_name("app.log.1");
    std::fs::write(&rotated, "the run before\n").unwrap();
    log_of_size(&path, "the big run", 5_300_000);
    AppLog::open(&path).expect("open app.log");
    let moved = std::fs::read(&rotated).expect("app.log.1 exists");
    assert!(moved.starts_with(b"the big run\n"), "the old one stayed");
    assert_eq!(moved.len(), 5_300_000);
}

/// AC-AXHK2G.6: only a log over the limit moves.
#[test]
fn a_log_under_5_mb_stays_where_it_is() {
    let path = log_path("no-rotate");
    let rotated = path.with_file_name("app.log.1");
    log_of_size(&path, "a small run", 4_900_000);
    AppLog::open(&path).expect("open app.log");
    assert!(!rotated.exists(), "the log moved");
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 4_900_000);
}

// ---- AC-AXHK2N.16: the PD capture's start and stop, with the meter's answer ----

/// The `pd capture` lines of meter A, after waiting for `count` of them.
fn pd_capture_lines(path: &Path, count: usize) -> Vec<String> {
    let of_a = |path: &Path| -> Vec<String> {
        lines(path)
            .into_iter()
            .filter(|l| l.contains("pd capture") && l.contains(METER_A))
            .collect()
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while of_a(path).len() < count {
        assert!(
            Instant::now() < deadline,
            "fewer than {count} pd capture lines: {:#?}",
            lines(path)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let found = of_a(path);
    for line in &found {
        assert!(!line.contains(SERIAL_A), "a serial in the log: {line}");
    }
    found
}

#[test]
fn an_accepted_pd_capture_start_and_stop_are_logged() {
    let path = log_path("pd-capture-accepted");
    let (m, _line, _rx) = faulty_session(&path, FaultPlan::new());
    let control = m.control_for(METER_A).expect("connected");
    control.start_pd().expect("the capture starts");
    control.stop_pd().expect("the capture stops");
    let found = pd_capture_lines(&path, 2);
    assert_eq!(found.len(), 2, "one line each: {found:#?}");
    assert!(
        found[0].contains("pd capture start") && found[0].contains("accepted"),
        "{}",
        found[0]
    );
    assert!(
        found[1].contains("pd capture stop") && found[1].contains("accepted"),
        "{}",
        found[1]
    );
}

#[test]
fn a_refused_pd_capture_start_is_logged_as_refused() {
    let path = log_path("pd-capture-start-refused");
    let (m, line, _rx) = faulty_session(&path, FaultPlan::new());
    line.refuse_pd.store(true, Ordering::SeqCst);
    let control = m.control_for(METER_A).expect("connected");
    assert!(control.start_pd().is_err(), "the meter refused the start");
    let found = pd_capture_lines(&path, 1);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0].contains("pd capture start") && found[0].contains("refused"),
        "{}",
        found[0]
    );
}

#[test]
fn a_refused_pd_capture_stop_is_logged_as_refused() {
    let path = log_path("pd-capture-stop-refused");
    let (m, line, _rx) = faulty_session(&path, FaultPlan::new());
    let control = m.control_for(METER_A).expect("connected");
    control.start_pd().expect("the capture starts");
    line.refuse_pd.store(true, Ordering::SeqCst);
    let _ = control.stop_pd();
    let found = pd_capture_lines(&path, 2);
    assert!(
        found[1].contains("pd capture stop") && found[1].contains("refused"),
        "{}",
        found[1]
    );
}
