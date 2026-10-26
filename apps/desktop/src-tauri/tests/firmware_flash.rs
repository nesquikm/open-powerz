//! The desktop firmware update (FR V9SK55) against the simulated
//! bootloader: the typed confirmation the backend re-checks before it
//! mints the flash acknowledgement (AC .3), each stage reported for the
//! progress view (AC .4), the new version or the error with the recovery
//! steps (AC .5), a failure at each step (AC .6), and exclusive ownership of
//! the meter for the update (AC .4).
//!
//! Only the simulator is used; no real device is opened. Every container is
//! synthetic, built by the shared test builder, and every identifier is the
//! simulator's made-up one.

mod common;
#[path = "../../../../crates/powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use fw_container::*;
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener};
use open_powerz_desktop_lib::firmware_flash::{FlashDone, FlashStage, flash_file};
use open_powerz_desktop_lib::ui_error::{ErrorKind, UiError};
use open_powerz_desktop_lib::worker::Status;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::transport::{self, discovery::DeviceEntry, discovery::Model};
use powerz_sim::{FlashFault, SimMeter};
use serde_json::{Value, json};

/// One data-phase block `[static]`.
const B: usize = 4080;
/// 3 whole blocks plus a 100-byte remainder: four payload writes.
const P: usize = 3 * B + 100;
/// The stored payload is 16 bytes longer than declared, as in the vendor file.
const STORED: usize = P + 16;

fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 53 + 7) as u8).collect()
}

/// A KM003C 2.0.7 container; `model` overrides the model field.
fn container(model: Option<&str>) -> Vec<u8> {
    let b = ContainerBuilder::new()
        .payload(&payload(STORED))
        .field(SIZE, &P.to_string());
    match model {
        Some(m) => b.field(MODEL, m).build(),
        None => b.build(),
    }
}

fn write_file(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("firmware-flash-desktop");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write container");
    path
}

/// A simulated KM003C running its 2.0.6 application (or its bootloader),
/// with `fault` injected.
fn sim_meter(mode: DeviceMode, fault: Option<FlashFault>) -> (SimMeter, SimClock) {
    let clock = clock();
    let mut dev =
        SimDevice::with_clock(Persona::Bulk, Scenario::Idle, clock.clone()).with_mode(mode);
    if let Some(f) = fault {
        dev = dev.with_flash_fault(f);
    }
    (SimMeter::new(dev), clock)
}

/// Run the desktop flash of `path` with `confirmation` on `meter`; returns
/// the result and every stage reported, in order.
fn run(
    meter: &SimMeter,
    clock: &SimClock,
    path: &std::path::Path,
    confirmation: &str,
) -> (Result<FlashDone, UiError>, Vec<FlashStage>) {
    let mut opener = meter.clone();
    let stop = AtomicBool::new(false);
    let mut stages = Vec::new();
    let r = flash_file(
        path,
        confirmation,
        &mut opener,
        clock.clone(),
        &stop,
        &mut |s| stages.push(s),
    );
    (r, stages)
}

/// Each stage as the UI receives it.
fn json_stages(stages: &[FlashStage]) -> Vec<Value> {
    stages
        .iter()
        .map(|s| serde_json::to_value(s).expect("a stage serialises for IPC"))
        .collect()
}

/// The stage names in order, consecutive repeats folded. A replug prompt is
/// not a stage of its own: the UI keeps showing the stage it came in.
fn stage_names(stages: &[FlashStage]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for v in json_stages(stages) {
        let name = v["stage"]
            .as_str()
            .unwrap_or_else(|| panic!("a stage without a name: {v}"))
            .to_owned();
        if name != "replug" && names.last() != Some(&name) {
            names.push(name);
        }
    }
    names
}

/// The recovery steps of AC-V9SK55.5: hold Back while plugging in, then retry.
fn assert_recovery_hint(e: &UiError) {
    let hint = e
        .hint
        .as_deref()
        .unwrap_or_else(|| panic!("no recovery hint: {e:?}"));
    assert!(
        hint.contains("Back"),
        "no hold-Back step in the hint: {hint}"
    );
    assert!(
        hint.to_lowercase().contains("plug"),
        "no plug-in step in the hint: {hint}"
    );
    let lower = hint.to_lowercase();
    assert!(
        lower.contains("again") || lower.contains("retry"),
        "no retry step in the hint: {hint}"
    );
}

/// Frame types of the framed writes (requests and jumps, ≤ 68 bytes).
fn framed_types(writes: &[Vec<u8>]) -> Vec<u8> {
    writes
        .iter()
        .filter(|w| w.len() <= 68)
        .map(|w| w[0] & 0x7F)
        .collect()
}

// ---- AC-V9SK55.3: the typed confirmation, re-checked by the backend ----

#[test]
fn a_confirmation_that_is_not_the_files_version_sends_nothing() {
    let path = write_file("confirm.bin", &container(None));
    for typed in ["", "2.0.6", "2.0.7x", "yes", "2.0"] {
        let (m, clock) = sim_meter(DeviceMode::Application, None);
        let (r, stages) = run(&m, &clock, &path, typed);
        let e = match r {
            Err(e) => e,
            Ok(done) => panic!("{typed:?} started a flash: {done:?}"),
        };
        assert_eq!(e.kind, ErrorKind::Usage, "{typed:?}: {e:?}");
        assert!(
            e.message.contains("2.0.7"),
            "{typed:?}: the refusal names the version to type: {e:?}"
        );
        assert!(stages.is_empty(), "{typed:?}: stages reported: {stages:?}");
        assert!(
            m.writes().is_empty(),
            "{typed:?}: frames reached the meter: {:?}",
            framed_types(&m.writes())
        );
        assert_eq!(m.mode(), DeviceMode::Application);
    }
}

#[test]
fn a_file_that_fails_inspection_sends_nothing() {
    let broken = write_file("broken.bin", b"not a firmware container at all");
    let missing = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("no-such-firmware.bin");
    for path in [broken, missing] {
        let (m, clock) = sim_meter(DeviceMode::Application, None);
        let (r, stages) = run(&m, &clock, &path, "2.0.7");
        let e = r.expect_err("a broken file was flashed");
        assert_eq!(e.kind, ErrorKind::Usage, "{path:?}: {e:?}");
        assert!(stages.is_empty(), "{path:?}: {stages:?}");
        assert!(m.writes().is_empty(), "{path:?}: frames reached the meter");
    }
}

#[test]
fn a_file_for_another_model_is_refused_and_the_meter_is_left_alone() {
    let path = write_file("km002c.bin", &container(Some("KM002C")));
    let (m, clock) = sim_meter(DeviceMode::Application, None);
    let (r, _) = run(&m, &clock, &path, "2.0.7");
    let e = r.expect_err("a KM002C file was flashed onto a KM003C");
    assert!(
        e.message.contains("KM002C"),
        "names the file's model: {e:?}"
    );
    assert_eq!(m.mode(), DeviceMode::Application, "the meter left its app");
    assert_eq!(m.app_version(), "2.0.6");
    assert!(m.received_descriptor().is_none(), "a descriptor was sent");
}

// ---- AC-V9SK55.4 / .5: every stage, then the version read back ----

#[test]
fn a_flash_from_application_mode_reports_each_stage_and_the_new_version() {
    let path = write_file("ok.bin", &container(None));
    let (m, clock) = sim_meter(DeviceMode::Application, None);
    let (r, stages) = run(&m, &clock, &path, "2.0.7");
    let done = r.unwrap_or_else(|e| panic!("the flash failed: {e:?}"));
    assert_eq!(
        serde_json::to_value(&done).unwrap(),
        json!({ "outcome": "verified", "version": "2.0.7" })
    );
    assert_eq!(m.app_version(), "2.0.7", "the sim reports the new version");
    assert_eq!(m.mode(), DeviceMode::Application);
    assert_eq!(
        stage_names(&stages),
        [
            "switching",
            "erasing",
            "writing",
            "verifying",
            "finished",
            "returning"
        ]
    );
    let replugs: Vec<Value> = json_stages(&stages)
        .into_iter()
        .filter(|v| v["stage"] == "replug")
        .collect();
    assert_eq!(
        replugs,
        [
            json!({ "stage": "replug", "to": "bootloader", "reply": "none" }),
            json!({ "stage": "replug", "to": "application", "reply": "none" }),
        ]
    );
}

#[test]
fn the_write_stage_carries_bytes_done_and_total_up_to_the_whole_image() {
    let path = write_file("progress.bin", &container(None));
    let (m, clock) = sim_meter(DeviceMode::Application, None);
    let (r, stages) = run(&m, &clock, &path, "2.0.7");
    assert!(r.is_ok(), "{r:?}");
    let writing: Vec<(u64, u64)> = json_stages(&stages)
        .into_iter()
        .filter(|v| v["stage"] == "writing")
        .map(|v| {
            let done = v["done"].as_u64().unwrap_or_else(|| panic!("no done: {v}"));
            let total = v["total"]
                .as_u64()
                .unwrap_or_else(|| panic!("no total: {v}"));
            (done, total)
        })
        .collect();
    assert!(writing.len() >= 2, "{writing:?}");
    assert!(
        writing.iter().all(|(_, t)| *t == STORED as u64),
        "{writing:?}"
    );
    assert!(
        writing.windows(2).all(|w| w[0].0 <= w[1].0),
        "bytes done never go back: {writing:?}"
    );
    assert_eq!(writing.last().map(|w| w.0), Some(STORED as u64));
}

#[test]
fn a_flash_from_bootloader_mode_skips_the_switch() {
    let path = write_file("boot.bin", &container(None));
    let (m, clock) = sim_meter(DeviceMode::Bootloader, None);
    let (r, stages) = run(&m, &clock, &path, "2.0.7");
    assert!(
        matches!(&r, Ok(FlashDone::Verified { version }) if version == "2.0.7"),
        "{r:?}"
    );
    let names = stage_names(&stages);
    assert_eq!(
        names.first().map(String::as_str),
        Some("erasing"),
        "{names:?}"
    );
    assert!(!names.iter().any(|n| n == "switching"), "{names:?}");
}

#[test]
fn a_meter_that_stays_in_its_bootloader_after_the_update_asks_for_a_power_cycle() {
    let path = write_file("stays.bin", &container(None));
    let (m, clock) = sim_meter(DeviceMode::Application, Some(FlashFault::JumpApromIgnored));
    let (r, _) = run(&m, &clock, &path, "2.0.7");
    let done = r.unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        serde_json::to_value(&done).unwrap(),
        json!({ "outcome": "power_cycle_needed" })
    );
}

// ---- AC-V9SK55.5 / .6: a failure at each step, with the recovery steps ----

/// Each injected fault, the stage the UI was last told about, and a word
/// the error message carries.
fn stage_faults() -> Vec<(FlashFault, &'static str, &'static str)> {
    vec![
        (FlashFault::JumpDfuIgnored, "switching", "bootloader"),
        (FlashFault::ConnectNoAccept, "switching", "read error"),
        (FlashFault::DescriptorAnswered, "erasing", "write error"),
        (FlashFault::NoAcceptAfterErase, "erasing", "erase error"),
        (
            FlashFault::MissingSync { write: 2 },
            "writing",
            "download error",
        ),
        (FlashFault::NoFinished, "verifying", "verify error"),
        (FlashFault::WrongVerifyReply, "verifying", "verify error"),
        (FlashFault::ImageNotTaken, "returning", "verify error"),
    ]
}

#[test]
fn a_failure_at_each_step_reports_the_error_and_the_recovery_steps() {
    let path = write_file("faults.bin", &container(None));
    for (fault, last_stage, word) in stage_faults() {
        let (m, clock) = sim_meter(DeviceMode::Application, Some(fault));
        let (r, stages) = run(&m, &clock, &path, "2.0.7");
        let e = match r {
            Err(e) => e,
            Ok(done) => panic!("{fault:?}: the flash succeeded: {done:?}"),
        };
        assert_ne!(e.kind, ErrorKind::Usage, "{fault:?}: {e:?}");
        assert!(
            e.message.to_lowercase().contains(word),
            "{fault:?}: the message says what failed ({word}): {e:?}"
        );
        assert_recovery_hint(&e);
        let names = stage_names(&stages);
        assert_eq!(
            names.last().map(String::as_str),
            Some(last_stage),
            "{fault:?}: {names:?}"
        );
    }
}

// ---- AC-V9SK55.4: exclusive ownership of the meter during the update ----

/// A KM003C as the OS lists it, in application or bootloader mode.
fn listed(interface: &str, address: u8) -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid: 0x0063,
        serial: Some(format!("SERIAL-{address:04}")),
        product: Some("POWER-Z KM003C".to_owned()),
        interface_string: Some(interface.to_owned()),
        bus: "1".to_owned(),
        address,
    }
}

/// Hands out the simulated meter behind a logging pipe and keeps every
/// pipe's log.
#[derive(Clone)]
struct LoggingOpener {
    clock: SimClock,
    logs: Arc<Mutex<Vec<Arc<Mutex<Log>>>>>,
}

impl Opener for LoggingOpener {
    type Transport = Pipe;
    fn open(&self, _device: &DeviceListing) -> Result<Pipe, transport::Error> {
        let dev = SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, self.clock.clone());
        let pipe = Pipe::new(dev, &self.clock, PipeOpts::default());
        self.logs.lock().unwrap().push(Arc::clone(&pipe.log));
        Ok(pipe)
    }
}

/// Wait (in real time) for `events` to hold a `connected` status after
/// `from` events.
fn wait_connected(events: &Events, from: usize) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if statuses(events)
            .iter()
            .skip(from)
            .any(|s| matches!(s, Status::Connected(_)))
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "never connected: {:?}",
            statuses(events)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The error of a call that must be refused because a firmware update runs.
fn refused_for_update<T>(what: &str, r: Result<T, UiError>) -> UiError {
    let e = match r {
        Err(e) => e,
        Ok(_) => panic!("{what} was allowed during the firmware update"),
    };
    assert_eq!(e.kind, ErrorKind::Usage, "{what}: {e:?}");
    assert!(
        e.message.to_lowercase().contains("firmware"),
        "{what}: the refusal names the firmware update: {e:?}"
    );
    e
}

#[test]
fn the_update_takes_the_meter_from_the_stream_and_refuses_everything_else_until_released() {
    let clock = clock();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let opener = LoggingOpener {
        clock: clock.clone(),
        logs: Arc::clone(&logs),
    };
    let mut m = DeviceManager::new(|| Ok(vec![listed("APP", 4)]), opener, clock.clone());
    let id = m
        .list_devices()
        .into_iter()
        .find(|d| d.demo_scenario.is_none())
        .expect("the meter is listed")
        .id;
    let (events, sink) = capture(&clock);
    m.connect(&id, 50, sink)
        .unwrap_or_else(|e| panic!("the meter did not connect: {e:?}"));
    wait_connected(&events, 0);

    let lease = m
        .begin_flash(&id)
        .unwrap_or_else(|e| panic!("the update could not take the meter: {e:?}"));
    assert_eq!(lease.device().id, id);

    // The measurement stream let go of the meter: its pipe is closed and
    // nothing more is sent on it.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !logs
        .lock()
        .unwrap()
        .iter()
        .all(|l| l.lock().unwrap().closed)
    {
        assert!(Instant::now() < deadline, "the stream kept the meter open");
        std::thread::sleep(Duration::from_millis(10));
    }
    let sent = |logs: &Arc<Mutex<Vec<Arc<Mutex<Log>>>>>| -> usize {
        logs.lock()
            .unwrap()
            .iter()
            .map(|l| l.lock().unwrap().frames.len())
            .sum()
    };
    let before = sent(&logs);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(sent(&logs), before, "frames were sent during the update");

    // The stream, PD capture (and recording), triggering and device
    // switching are all refused while the update holds the meter.
    let (_e2, sink2) = capture(&clock);
    refused_for_update("reconnecting the stream", m.connect(&id, 50, sink2));
    let (_e3, sink3) = capture(&clock);
    refused_for_update(
        "switching to the demo device",
        m.connect("demo:idle", 50, sink3),
    );
    refused_for_update("the session control (PD capture, recording)", m.control());
    refused_for_update("opening a trigger session", m.trigger_opener());
    refused_for_update("a second update", m.begin_flash(&id));
    assert!(
        !m.devices_changed(&m.list_devices()),
        "a re-enumerated meter reconnected during the update"
    );
    assert_eq!(sent(&logs), before, "a refused request reached the meter");

    // Released afterwards: the meter connects again.
    drop(lease);
    let (events, sink) = capture(&clock);
    m.connect(&id, 50, sink)
        .unwrap_or_else(|e| panic!("the meter stayed locked after the update: {e:?}"));
    wait_connected(&events, 0);
    m.disconnect().expect("disconnect");
}

#[test]
fn a_meter_in_bootloader_mode_can_be_taken_for_an_update() {
    let clock = clock();
    let opener = LoggingOpener {
        clock: clock.clone(),
        logs: Arc::default(),
    };
    let mut m = DeviceManager::new(|| Ok(vec![listed("BOOT", 9)]), opener, clock.clone());
    let device = m
        .list_devices()
        .into_iter()
        .find(|d| d.demo_scenario.is_none())
        .expect("the meter is listed");
    let lease = m
        .begin_flash(&device.id)
        .unwrap_or_else(|e| panic!("a bootloader-mode meter cannot be recovered: {e:?}"));
    assert_eq!(lease.device().mode, DeviceMode::Bootloader);
}

#[test]
fn an_unlisted_meter_cannot_be_taken_for_an_update() {
    let clock = clock();
    let opener = LoggingOpener {
        clock: clock.clone(),
        logs: Arc::default(),
    };
    let mut m = DeviceManager::new(|| Ok(vec![listed("APP", 4)]), opener, clock.clone());
    let e = match m.begin_flash("usb:9:99") {
        Err(e) => e,
        Ok(_) => panic!("an unlisted meter was taken"),
    };
    assert_eq!(e.kind, ErrorKind::Usage, "{e:?}");
}
