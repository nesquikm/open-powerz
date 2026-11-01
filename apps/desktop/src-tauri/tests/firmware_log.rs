//! FR AXHK2G: a firmware update reaches `app.log`, a line at its start, one
//! per stage it enters and one for its outcome (AC .3), and neither a full
//! demo session nor a flash leaves a serial or UID in the file (AC .7).
//!
//! The update's meter and model come from its lease, the way the
//! `firmware_flash` command gets them. Only the simulator is used; no real
//! device is opened. The container is synthetic and every identifier is made
//! up.

mod common;
#[path = "../../../../crates/powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use common::*;
use fw_container::*;
use open_powerz_desktop_lib::app_log::AppLog;
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener};
use open_powerz_desktop_lib::firmware_flash::{FlashDone, FlashStage, flash_file_logged};
use open_powerz_desktop_lib::ui_error::UiError;
use open_powerz_desktop_lib::worker::{BackendEvent, ConnectedInfo, Status};
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::session::Session;
use powerz_protocol::transport::{
    self, Transport, discovery::DeviceEntry, discovery::Model, guard::GuardedTransport,
};
use powerz_sim::{FlashFault, PdScenario, SimMeter};

const METER: &str = "usb:1:4";
const MODEL: &str = "KM003C";
/// A made-up serial.
const SERIAL: &str = "SERIAL-AXHK2G";

/// One data-phase block `[static]`.
const B: usize = 4080;
/// 3 whole blocks plus a 100-byte remainder: four payload writes.
const P: usize = 3 * B + 100;
/// The stored payload is 16 bytes longer than declared, as in the vendor file.
const STORED: usize = P + 16;

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn scratch(tag: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("firmware-log-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn firmware_file(dir: &Path) -> PathBuf {
    let payload: Vec<u8> = (0..STORED).map(|i| (i * 53 + 7) as u8).collect();
    let bytes = ContainerBuilder::new()
        .payload(&payload)
        .field(SIZE, &P.to_string())
        .build();
    let path = dir.join("km003c.mencrypt");
    std::fs::write(&path, bytes).expect("write container");
    path
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|s| s.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// What the test does to a session's line while it runs.
#[derive(Clone)]
struct Line {
    /// The cable is in; once out, every request fails.
    plugged: Arc<AtomicBool>,
    /// How many of the next ADC snapshots the meter refuses, still on the
    /// line.
    refuse_snapshots: Arc<AtomicUsize>,
}

/// The simulator behind a line the test can fault.
struct FaultedPipe {
    pipe: Pipe,
    line: Line,
}

impl Transport for FaultedPipe {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        if !self.line.plugged.load(Ordering::SeqCst) {
            return Err(transport::Error::Disconnected);
        }
        let snapshot =
            frame::get_data_mask(request).is_some_and(|m| m.bits() == Attribute::ADC.bits());
        if snapshot
            && self
                .line
                .refuse_snapshots
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok()
        {
            return Ok(vec![0x06, request[1], 0x00, 0x00]);
        }
        self.pipe.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        if !self.line.plugged.load(Ordering::SeqCst) {
            return Err(transport::Error::Disconnected);
        }
        self.pipe.read(timeout)
    }

    fn settings_pipe(&mut self) -> Option<Result<Box<dyn Transport>, transport::Error>> {
        self.pipe.settings_pipe()
    }
}

/// Simulated meters for the demo session; `faults` apply to every open.
struct SimOpener {
    clock: SimClock,
    faults: FaultPlan,
    line: Line,
}

impl Opener for SimOpener {
    type Transport = FaultedPipe;
    fn open(&self, device: &DeviceListing) -> Result<FaultedPipe, transport::Error> {
        let scenario = device
            .demo_scenario
            .as_deref()
            .and_then(Scenario::from_name)
            .unwrap_or(Scenario::Load5v1a);
        let dev = SimDevice::with_clock(Persona::Bulk, scenario, self.clock.clone())
            .with_pd_scenario(PdScenario::Negotiation)
            .with_faults(self.faults.clone());
        Ok(FaultedPipe {
            pipe: Pipe::new(dev, &self.clock, PipeOpts::default()),
            line: self.line.clone(),
        })
    }
}

fn listed() -> DeviceEntry {
    listed_at(4)
}

fn listed_at(address: u8) -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid: 0x0063,
        serial: Some(SERIAL.to_owned()),
        product: Some("POWER-Z KM003C".to_owned()),
        interface_string: Some("APP".to_owned()),
        bus: "1".to_owned(),
        address,
    }
}

/// A manager with one listed meter, logging to `log`.
fn manager(log: &Path, faults: FaultPlan) -> DeviceManager<SimOpener, SimClock> {
    let (m, _, _) = manager_on_a_line(log, faults);
    m
}

/// [`manager`], with the OS device list and the line under the test's control.
fn manager_on_a_line(
    log: &Path,
    faults: FaultPlan,
) -> (
    DeviceManager<SimOpener, SimClock>,
    Arc<Mutex<Vec<DeviceEntry>>>,
    Line,
) {
    let clock = clock();
    let attached = Arc::new(Mutex::new(vec![listed()]));
    let list = Arc::clone(&attached);
    let line = Line {
        plugged: Arc::new(AtomicBool::new(true)),
        refuse_snapshots: Arc::default(),
    };
    let mut m = DeviceManager::new(
        move || Ok(list.lock().unwrap().clone()),
        SimOpener {
            clock: clock.clone(),
            faults,
            line: line.clone(),
        },
        clock,
    );
    m.set_app_log(AppLog::open(log).expect("open app.log"));
    (m, attached, line)
}

/// Run a logged update of `id` on a simulated meter, as the `firmware_flash`
/// command does. `on_stage` sees each stage first, with the stop flag.
fn flash(
    m: &mut DeviceManager<SimOpener, SimClock>,
    id: &str,
    dir: &Path,
    fault: Option<FlashFault>,
    on_stage: impl Fn(&FlashStage, &AtomicBool),
) -> (Result<FlashDone, UiError>, Vec<FlashStage>) {
    let lease = m.begin_flash(id, &[]).expect("the meter is free");
    let log = lease.session_log();
    let clock = clock();
    let mut dev = SimDevice::with_clock(Persona::Bulk, Scenario::Idle, clock.clone())
        .with_mode(DeviceMode::Application);
    if let Some(f) = fault {
        dev = dev.with_flash_fault(f);
    }
    let mut meter = SimMeter::new(dev);
    let stop = lease.control().stop();
    let mut stages = Vec::new();
    let done = flash_file_logged(
        &firmware_file(dir),
        "2.0.7",
        &mut meter,
        clock,
        stop,
        &log,
        &mut |s| {
            on_stage(&s, stop);
            stages.push(s);
        },
    );
    (done, stages)
}

fn no_stop(_: &FlashStage, _: &AtomicBool) {}

/// The stages entered, in order: repeats folded, replug prompts left out.
fn entered(stages: &[FlashStage]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for stage in stages {
        let value = serde_json::to_value(stage).expect("a stage serialises");
        let name = value["stage"].as_str().expect("a stage name").to_owned();
        if name != "replug" && names.last() != Some(&name) {
            names.push(name);
        }
    }
    names
}

/// The log is a start line, a line per entered stage in order, and an
/// outcome line, each stamped and naming the meter and its model. Returns
/// the outcome line.
fn assert_lines_of_one_update(log: &Path, stages: &[FlashStage]) -> String {
    let all = lines(log);
    let entered = entered(stages);
    assert_eq!(
        all.len(),
        entered.len() + 2,
        "start, {entered:?}, outcome: {all:#?}"
    );
    for line in &all {
        assert!(
            line.chars().take(4).all(|c| c.is_ascii_digit()),
            "no timestamp: {line}"
        );
        assert!(line.contains(METER), "no meter: {line}");
        assert!(line.contains(MODEL), "no model: {line}");
    }
    assert!(all[0].contains("start"), "not a start line: {}", all[0]);
    for (line, stage) in all[1..].iter().zip(&entered) {
        assert!(line.contains(stage.as_str()), "not {stage}: {line}");
    }
    all[all.len() - 1].clone()
}

/// The error kind by the name the UI gets.
fn kind_of(error: &UiError) -> String {
    serde_json::to_value(error.kind)
        .expect("a kind serialises")
        .as_str()
        .expect("a kind is a name")
        .to_owned()
}

#[test]
fn an_update_logs_its_start_each_stage_once_and_that_it_finished() {
    let dir = scratch("finished");
    let log = dir.join("app.log");
    let mut m = manager(&log, FaultPlan::new());
    let (done, stages) = flash(&mut m, METER, &dir, None, no_stop);
    assert!(
        matches!(done, Ok(FlashDone::Verified { .. })),
        "the update ran: {done:?}"
    );
    let writes = stages
        .iter()
        .filter(|s| matches!(s, FlashStage::Writing { .. }))
        .count();
    assert!(writes > 1, "several write percents to fold: {writes}");
    let outcome = assert_lines_of_one_update(&log, &stages);
    assert!(outcome.contains("finished"), "{outcome}");
}

#[test]
fn an_update_that_needs_a_power_cycle_also_finished() {
    let dir = scratch("power-cycle");
    let log = dir.join("app.log");
    let mut m = manager(&log, FaultPlan::new());
    let fault = Some(FlashFault::JumpApromIgnored);
    let (done, stages) = flash(&mut m, METER, &dir, fault, no_stop);
    assert_eq!(done, Ok(FlashDone::PowerCycleNeeded));
    let outcome = assert_lines_of_one_update(&log, &stages);
    assert!(outcome.contains("finished"), "{outcome}");
}

#[test]
fn a_failed_update_logs_the_failure_and_its_kind() {
    for fault in [
        FlashFault::DescriptorAnswered,
        FlashFault::MissingSync { write: 2 },
        FlashFault::NoFinished,
    ] {
        let dir = scratch("failed");
        let log = dir.join("app.log");
        let mut m = manager(&log, FaultPlan::new());
        let (done, stages) = flash(&mut m, METER, &dir, Some(fault), no_stop);
        let error = done.expect_err("the update fails");
        let outcome = assert_lines_of_one_update(&log, &stages);
        assert!(outcome.contains("failed"), "{outcome}");
        assert!(outcome.contains(&kind_of(&error)), "{outcome}");
    }
}

#[test]
fn a_cancelled_update_logs_that_it_was_cancelled_not_that_it_failed() {
    let dir = scratch("cancelled");
    let log = dir.join("app.log");
    let mut m = manager(&log, FaultPlan::new());
    let (done, stages) = flash(&mut m, METER, &dir, None, |stage, stop| {
        if matches!(stage, FlashStage::Switching) {
            stop.store(true, Ordering::SeqCst);
        }
    });
    assert!(done.is_err(), "the update stopped: {done:?}");
    let outcome = assert_lines_of_one_update(&log, &stages);
    assert!(outcome.contains("cancelled"), "{outcome}");
    assert!(!outcome.contains("failed"), "{outcome}");
}

#[test]
fn the_flash_command_goes_through_the_logged_update() {
    let source = include_str!("../src/lib.rs");
    let start = source
        .find("async fn firmware_flash(")
        .expect("the command exists");
    let end = source[start..]
        .find("async fn firmware_cancel(")
        .expect("the next command");
    let command = &source[start..start + end];
    assert!(command.contains(".session_log()"), "{command}");
    assert_eq!(
        command
            .matches("firmware_flash::flash_file_logged(")
            .count(),
        2,
        "both the demo and the real meter log: {command}"
    );
    assert!(
        !command.contains("firmware_flash::flash_file("),
        "an update that skips the log: {command}"
    );
}

/// Events until `want` matches one.
fn wait_for(rx: &mpsc::Receiver<BackendEvent>, want: impl Fn(&BackendEvent) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if want(&rx.recv_timeout(left).expect("the event never came")) {
            return;
        }
    }
}

/// The first `Connected` status; waits for it.
fn connected_info(rx: &mpsc::Receiver<BackendEvent>) -> ConnectedInfo {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left).expect("the meter never connected") {
            BackendEvent::Status(Status::Connected(info)) => return info,
            _ => continue,
        }
    }
}

/// The UID a session reads from the simulator, as hex with no separators.
fn sim_uid() -> String {
    let clock = clock();
    let dev = SimDevice::with_clock(Persona::Bulk, Scenario::Idle, clock.clone());
    let mut session = Session::with_clock(GuardedTransport::new(dev), clock);
    session.connect().expect("connect");
    let uid = session.device_info().expect("device info").uid;
    let hex: String = uid
        .to_lowercase()
        .chars()
        .filter(char::is_ascii_hexdigit)
        .collect();
    assert_eq!(hex.len(), 24, "12 bytes of hex: {uid}");
    hex
}

/// Meter `METER` connects, loses its cable and comes back at a new address,
/// then disconnects. Only a listed meter can do this: a demo one has no cable.
fn unplug_and_replug(
    m: &mut DeviceManager<SimOpener, SimClock>,
    attached: &Mutex<Vec<DeviceEntry>>,
    line: &Line,
) {
    let (tx, rx) = mpsc::channel();
    m.connect(METER, 50, move |e| {
        let _ = tx.send(e);
    })
    .expect("connect");
    let samples = |e: &BackendEvent| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty());
    wait_for(&rx, samples);

    line.plugged.store(false, Ordering::SeqCst);
    attached.lock().unwrap().clear();
    wait_for(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Waiting { .. }))
    });
    let list = m.list_devices();
    m.devices_changed(&list);

    line.plugged.store(true, Ordering::SeqCst);
    attached.lock().unwrap().push(listed_at(5));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let list = m.list_devices();
        if m.devices_changed(&list) {
            break;
        }
        assert!(Instant::now() < deadline, "the meter did not come back");
        std::thread::sleep(Duration::from_millis(10));
    }
    wait_for(&rx, samples);
    for id in m.connected_ids() {
        m.disconnect_for(&id).expect("disconnect");
    }
}

/// AC-AXHK2G.7: a demo session that connects, records, changes its rate, runs
/// a PD capture and stops, with a refused rate, a recording that cannot start,
/// a failed settings write and record delete, and a refused snapshot; then an
/// unplug and a replug of a listed meter, and a flash that works and one that
/// fails. None of those lines holds a serial or the UID. A recording that
/// fails mid-write is not in this run: the manager has no fake file to give it,
/// and its line is the `error` line a refused start gets.
#[test]
fn a_full_demo_session_and_flash_leave_no_serial_or_uid_in_the_log() {
    let dir = scratch("scan");
    let log = dir.join("app.log");
    let faults = FaultPlan::new()
        .ignore_settings_write()
        .ignore_table_write();
    let (mut m, attached, line) = manager_on_a_line(&log, faults);
    let demo = m
        .list_devices()
        .into_iter()
        .find(|d| d.demo_scenario.is_some())
        .expect("a demo entry");
    let id = demo.id.clone();

    let (tx, rx) = mpsc::channel();
    m.connect(&id, 50, move |e| {
        let _ = tx.send(e);
    })
    .expect("connect");
    let info = connected_info(&rx);
    // The session read the UID: every block came back, and the simulator
    // grants authentication only to a host that echoes it.
    assert!(info.firmware_problems.is_empty(), "{info:?}");
    assert!(!info.rate_locked, "the UID was not read: {info:?}");

    m.start_recording_for(&id, &dir.join("session.csv"))
        .expect("record");
    // A recording holds the rate, and the worker refuses the change.
    assert!(
        m.set_rate_for(&id, 10).is_err(),
        "a recording holds the rate"
    );
    m.stop_recording_for(&id).expect("stop recording");
    assert!(
        m.start_recording_for(&id, &dir.join("no-such-folder").join("session.csv"))
            .is_err(),
        "no folder to record to"
    );
    assert!(m.set_rate_for(&id, 7).is_err(), "not a rate");
    m.set_rate_for(&id, 10).expect("change the rate");
    let control = m.control_for(&id).expect("connected");
    control.start_pd().expect("start the PD capture");
    control.stop_pd().expect("stop the PD capture");
    let changes = BTreeMap::from([("brightness".to_owned(), 80)]);
    assert!(control.write_settings(&changes).is_err(), "settings fault");
    let listed = serde_json::to_value(control.list_records().expect("list")).unwrap();
    let name = listed["records"][0]["name"].as_str().expect("a record");
    let backup = dir.join("backup");
    assert!(
        control.delete_records(Some(1), name, &backup).is_err(),
        "table fault"
    );
    line.refuse_snapshots.store(1, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(5);
    while line.refuse_snapshots.load(Ordering::SeqCst) > 0 {
        assert!(Instant::now() < deadline, "no snapshot was refused");
        std::thread::sleep(Duration::from_millis(1));
    }
    m.disconnect_for(&id).expect("disconnect");

    unplug_and_replug(&mut m, &attached, &line);

    let (done, _) = flash(&mut m, &id, &dir, None, no_stop);
    assert!(done.is_ok(), "the update ran: {done:?}");
    let fault = Some(FlashFault::MissingSync { write: 2 });
    let (done, _) = flash(&mut m, &id, &dir, fault, no_stop);
    assert!(done.is_err(), "the update fails");

    let text = std::fs::read_to_string(&log).expect("read app.log");
    // The scan looked at a log with something in it: each of these is a
    // line only the event it names can write.
    for word in [
        "connected",
        "disconnected",
        "unplugged",
        "replug usb:1:4 to usb:1:5",
        "start",
        "failed",
        "finished",
        "Stop the recording",
        "7 samples per second",
        "The meter refused the request.",
    ] {
        assert!(text.contains(word), "no {word} in the log:\n{text}");
    }
    assert!(text.contains(&id), "no meter in the log:\n{text}");

    let serials = [
        Some(SIM_SERIAL.to_owned()),
        demo.serial.clone(),
        Some(info.serial.clone()),
        Some(SERIAL.to_owned()),
    ];
    for serial in serials.into_iter().flatten() {
        assert!(!serial.is_empty());
        assert!(!text.contains(&serial), "serial {serial} in:\n{text}");
    }
    // Hex however it is written; the timestamps' colons go too, which is
    // harmless to a 24-digit value.
    let squeezed: String = text
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, ' ' | ':' | '-' | ','))
        .collect();
    let uid = sim_uid();
    assert!(!squeezed.contains(&uid), "the UID {uid} is in:\n{text}");
}
