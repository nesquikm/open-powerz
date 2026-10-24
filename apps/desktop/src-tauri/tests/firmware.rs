//! Device mode, both firmware versions and firmware warnings in the
//! desktop backend (FR V9SK52, AC .4 and .5), against the simulator on its
//! fake clock. Every identifier is synthetic.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::*;
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener, list_devices_from};
use open_powerz_desktop_lib::ui_error::ErrorKind;
use open_powerz_desktop_lib::worker::{BackendEvent, ConnectedInfo, Status, Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_protocol::transport::discovery::{DeviceEntry, Model};
use powerz_protocol::transport::{self, Transport};
use serde_json::{Value, json};

/// A KM003C as the OS lists it: iProduct is always the model name
/// [device]; `interface` is the IF0 interface string (string 2) that
/// decides the mode.
fn meter(interface: Option<&str>, address: u8) -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid: 0x0063,
        serial: Some(format!("SERIAL-{address:04}")),
        product: Some("POWER-Z KM003C".to_owned()),
        interface_string: interface.map(str::to_owned),
        bus: "1".to_owned(),
        address,
    }
}

/// Connect a worker to `dev` and return the `connected` status as JSON
/// (what the UI receives) plus the types of every frame sent.
fn connect(dev: SimDevice, clock: &SimClock) -> (Value, Vec<u8>) {
    let pipe = Pipe::new(dev, clock, PipeOpts::default());
    let log = Arc::clone(&pipe.log);
    connect_over(pipe, &log, clock)
}

/// [`connect`] over any transport whose requests end up in `log`.
fn connect_over<T: Transport + Send + 'static>(
    transport: T,
    log: &Arc<std::sync::Mutex<Log>>,
    clock: &SimClock,
) -> (Value, Vec<u8>) {
    let (events, sink) = capture(clock);
    let _worker = Worker::start(
        transport,
        clock.clone(),
        WorkerConfig::new(Rate::Sps50, 1),
        sink,
    )
    .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    let errors: Vec<_> = events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, e)| match e {
            BackendEvent::Error(err) => Some(err.clone()),
            _ => None,
        })
        .collect();
    assert!(errors.is_empty(), "an error reached the UI: {errors:?}");
    let all = statuses(&events);
    let info: ConnectedInfo = all
        .into_iter()
        .find_map(|s| match s {
            Status::Connected(info) => Some(info),
            _ => None,
        })
        .expect("no connected status");
    let types = log
        .lock()
        .unwrap()
        .frames
        .iter()
        .map(|(_, f)| f[0] & 0x7F)
        .collect();
    (serde_json::to_value(info).unwrap(), types)
}

fn sim(clock: &SimClock) -> SimDevice {
    SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, clock.clone())
}

// ---- AC-V9SK52.5: the device list shows the mode ----

#[test]
fn a_bootloader_mode_meter_is_listed_as_such() {
    let list = list_devices_from(Ok(vec![meter(Some("BOOT"), 9)]));
    let usb = &list[0];
    assert!(usb.demo_scenario.is_none());
    let v = serde_json::to_value(usb).unwrap();
    assert_eq!(v["mode"], "bootloader", "{v}");
    assert!(
        usb.label.to_lowercase().contains("bootloader"),
        "label says bootloader: {}",
        usb.label
    );
}

#[test]
fn an_application_mode_meter_and_the_demo_entries_are_listed_as_application() {
    let list = list_devices_from(Ok(vec![meter(Some("APP"), 7)]));
    for d in &list {
        let v = serde_json::to_value(d).unwrap();
        assert_eq!(v["mode"], "application", "{}: {v}", d.label);
        assert!(
            !d.label.to_lowercase().contains("bootloader"),
            "{}",
            d.label
        );
    }
}

#[test]
fn a_meter_with_no_interface_string_is_listed_as_unknown_mode() {
    let list = list_devices_from(Ok(vec![meter(None, 3)]));
    let v = serde_json::to_value(&list[0]).unwrap();
    assert_eq!(v["mode"], "unknown", "{v}");
}

#[test]
fn an_unknown_mode_meter_gets_the_plain_label_with_no_suffix() {
    for interface in [None, Some("XYZ")] {
        let list = list_devices_from(Ok(vec![meter(interface, 3)]));
        let usb = &list[0];
        let v = serde_json::to_value(usb).unwrap();
        assert_eq!(v["mode"], "unknown", "{interface:?}: {v}");
        assert_eq!(
            usb.label, "KM003C — SERIAL-0003",
            "{interface:?}: no mode suffix in the label"
        );
        assert!(
            !usb.label.to_lowercase().contains("unknown"),
            "{interface:?}: {}",
            usb.label
        );
    }
    // The bootloader suffix stays.
    let list = list_devices_from(Ok(vec![meter(Some("BOOT"), 3)]));
    assert_ne!(list[0].label, "KM003C — SERIAL-0003");
    assert!(list[0].label.to_lowercase().contains("bootloader"));
}

// ---- AC-V9SK52.5 / .4: device details after connect ----

#[test]
fn connected_status_carries_mode_both_versions_and_the_outdated_warning() {
    let clock = clock();
    let (v, _) = connect(sim(&clock), &clock);
    assert_eq!(v["mode"], "application", "{v}");
    assert_eq!(v["firmware_version"], "2.0.6", "{v}");
    assert_eq!(v["bootloader_version"], "2.5", "{v}");
    assert_eq!(v["warnings"], json!(["outdated"]), "{v}");
}

#[test]
fn connected_status_warns_too_old_below_1_7_0() {
    let clock = clock();
    let (v, _) = connect(sim(&clock).with_app_version("1.6.0"), &clock);
    assert_eq!(v["firmware_version"], "1.6.0", "{v}");
    assert_eq!(v["warnings"], json!(["too_old"]), "{v}");
}

#[test]
fn connected_status_warns_when_head_reports_update_required() {
    let clock = clock();
    let (v, types) = connect(sim(&clock).with_head_field(1), &clock);
    let warnings = v["warnings"].as_array().expect("warnings array");
    assert!(warnings.contains(&json!("update_required")), "{v}");
    let connect_at = types.iter().position(|&t| t == 0x02).unwrap();
    let head_at = types
        .iter()
        .position(|&t| t == 0x40)
        .unwrap_or_else(|| panic!("HEAD never sent: {types:02x?}"));
    assert!(connect_at < head_at, "CONNECT before HEAD: {types:02x?}");
}

#[test]
fn newest_firmware_connects_with_no_warning() {
    let clock = clock();
    let (v, types) = connect(sim(&clock).with_app_version("2.0.7"), &clock);
    assert_eq!(v["warnings"], json!([]), "{v}");
    for ty in types {
        assert!(SAFE_TYPES.contains(&ty), "unsafe frame type {ty:#04x}");
    }
}

#[test]
fn the_product_string_alone_does_not_make_a_meter_application_mode() {
    // A meter whose IF0 interface string is unreadable: the model name in
    // iProduct says nothing about the mode.
    let list = list_devices_from(Ok(vec![meter(None, 5)]));
    assert_eq!(list[0].mode.name(), "unknown");
    let list = list_devices_from(Ok(vec![meter(Some("APP"), 5)]));
    assert_eq!(list[0].mode.name(), "application");
}

// ---- AC-V9SK52.5 hardening: a failed firmware status read never fails the connect ----

/// How [`HeadFault`] spoils the HEAD request.
#[derive(Clone, Copy)]
enum HeadFailure {
    /// No reply (timeout), every time.
    Timeout,
    /// A reply with the right id that is too short to decode.
    Garbage,
}

/// The simulated meter behind a pipe, except that HEAD (0x40) fails.
struct HeadFault {
    pipe: Pipe,
    how: HeadFailure,
}

impl Transport for HeadFault {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        if request.first().map(|b| b & 0x7F) == Some(0x40) {
            self.pipe
                .log
                .lock()
                .unwrap()
                .frames
                .push((0, request.to_vec()));
            return match self.how {
                HeadFailure::Timeout => Err(transport::Error::Timeout),
                HeadFailure::Garbage => Ok(vec![0x40, request[1]]),
            };
        }
        self.pipe.transact(request, timeout)
    }
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.pipe.read(timeout)
    }
    fn recover(&mut self) -> Result<(), transport::Error> {
        self.pipe.recover()
    }
}

fn connect_with_head(how: HeadFailure, clock: &SimClock) -> (Value, Vec<u8>) {
    let pipe = Pipe::new(sim(clock), clock, PipeOpts::default());
    let log = Arc::clone(&pipe.log);
    connect_over(HeadFault { pipe, how }, &log, clock)
}

#[test]
fn a_head_that_times_out_still_connects_without_a_firmware_warning() {
    let clock = clock();
    let (v, types) = connect_with_head(HeadFailure::Timeout, &clock);
    assert!(types.contains(&0x40), "HEAD was tried: {types:02x?}");
    assert_eq!(v["mode"], "application", "{v}");
    assert_eq!(v["model"], "KM003C", "{v}");
    let warnings = v["warnings"].as_array().expect("warnings array");
    assert!(
        !warnings.contains(&json!("update_required")),
        "no update-required warning without a HEAD reply: {v}"
    );
    // The queue still starts: the session is fully up.
    assert!(types.contains(&0x0E), "queue never started: {types:02x?}");
}

#[test]
fn an_undecodable_head_reply_still_connects_without_a_firmware_warning() {
    let clock = clock();
    let (v, types) = connect_with_head(HeadFailure::Garbage, &clock);
    assert_eq!(v["mode"], "application", "{v}");
    let warnings = v["warnings"].as_array().expect("warnings array");
    assert!(!warnings.contains(&json!("update_required")), "{v}");
    assert!(types.contains(&0x0E), "queue never started: {types:02x?}");
}

#[test]
fn a_one_off_head_timeout_from_the_sim_still_connects() {
    // Request 1 is CONNECT, request 2 is HEAD.
    let clock = clock();
    let dev = sim(&clock).with_faults(FaultPlan::new().timeout_at(2));
    let (v, types) = connect(dev, &clock);
    assert_eq!(types.first(), Some(&0x02), "{types:02x?}");
    assert_eq!(types.get(1), Some(&0x40), "{types:02x?}");
    assert_eq!(v["mode"], "application", "{v}");
    let warnings = v["warnings"].as_array().expect("warnings array");
    assert!(!warnings.contains(&json!("update_required")), "{v}");
}

// ---- AC-V9SK52.5 hardening: the backend refuses a bootloader-mode meter ----

/// Counts every open; the transport it hands out logs every frame.
#[derive(Clone)]
struct CountingOpener {
    opens: Arc<AtomicUsize>,
    clock: SimClock,
}

impl Opener for CountingOpener {
    type Transport = Pipe;
    fn open(&self, _device: &DeviceListing) -> Result<Pipe, transport::Error> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Pipe::new(
            sim(&self.clock).with_mode(powerz_protocol::firmware::DeviceMode::Bootloader),
            &self.clock,
            PipeOpts::default(),
        ))
    }
}

#[test]
fn connecting_to_a_bootloader_mode_meter_is_refused_before_any_frame() {
    let clock = clock();
    let opens = Arc::new(AtomicUsize::new(0));
    let opener = CountingOpener {
        opens: Arc::clone(&opens),
        clock: clock.clone(),
    };
    let mut m = DeviceManager::new(|| Ok(vec![meter(Some("BOOT"), 9)]), opener, clock.clone());
    let id = m
        .list_devices()
        .into_iter()
        .find(|d| d.demo_scenario.is_none())
        .expect("the meter is listed")
        .id;
    let (events, sink) = capture(&clock);
    let err = m
        .connect(&id, 50, sink)
        .expect_err("a bootloader-mode meter was connected");
    assert_eq!(err.kind, ErrorKind::Usage, "{err:?}");
    assert!(
        err.message.to_lowercase().contains("bootloader"),
        "the error names bootloader mode: {err:?}"
    );
    // Give a worker that should not exist a moment, then check nothing ran.
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(opens.load(Ordering::SeqCst), 0, "the meter was opened");
    assert!(
        !statuses(&events)
            .iter()
            .any(|s| matches!(s, Status::Connected(_))),
        "a session was opened"
    );
}

// ---- AC-V9SK52.5: the connected status carries the listing's mode ----

/// Hands out the simulated meter (answering like application firmware)
/// whatever the listing's interface string said.
#[derive(Clone)]
struct SimPipeOpener {
    clock: SimClock,
}

impl Opener for SimPipeOpener {
    type Transport = Pipe;
    fn open(&self, _device: &DeviceListing) -> Result<Pipe, transport::Error> {
        Ok(Pipe::new(
            sim(&self.clock),
            &self.clock,
            PipeOpts::default(),
        ))
    }
}

#[test]
fn an_unknown_mode_meter_that_connects_reports_unknown_mode() {
    let clock = clock();
    let opener = SimPipeOpener {
        clock: clock.clone(),
    };
    let mut m = DeviceManager::new(|| Ok(vec![meter(Some("XYZ"), 4)]), opener, clock.clone());
    let listing = m
        .list_devices()
        .into_iter()
        .find(|d| d.demo_scenario.is_none())
        .expect("the meter is listed");
    assert_eq!(listing.mode.name(), "unknown");
    let (events, sink) = capture(&clock);
    m.connect(&listing.id, 50, sink)
        .unwrap_or_else(|e| panic!("an unknown-mode meter was refused: {e:?}"));
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let info = loop {
        let found = statuses(&events).into_iter().find_map(|s| match s {
            Status::Connected(info) => Some(info),
            _ => None,
        });
        if let Some(info) = found {
            break info;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "never connected: {:?}",
            statuses(&events)
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let v = serde_json::to_value(info).unwrap();
    assert_eq!(
        v["mode"], "unknown",
        "the listing's mode, not application: {v}"
    );
}
