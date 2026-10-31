//! AC-4PJRVM.7 on the desktop side: a settings write whose serial port is
//! held by another program says so in plain words, and the read-back also
//! compares the bytes of the block no field decodes, reporting a difference as
//! a warning on the view (the `stir_undecoded_settings` fault changes one).

mod common;

use std::collections::BTreeMap;
use std::time::Duration;

use common::*;
use open_powerz_desktop_lib::worker::{Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_protocol::transport::cdc::{self, PortCandidate};
use powerz_protocol::transport::{self, Transport};
use powerz_sim::SimClock;
use serde_json::Value;

/// The simulated meter on bulk, with its CDC port held by someone else.
struct HeldPort {
    pipe: Pipe,
    error: transport::Error,
}

impl Transport for HeldPort {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.pipe.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.pipe.read(timeout)
    }

    fn settings_pipe(&mut self) -> Option<Result<Box<dyn Transport>, transport::Error>> {
        Some(Err(self.error.clone()))
    }
}

fn changes() -> BTreeMap<String, i32> {
    BTreeMap::from([("brightness".to_string(), 30)])
}

fn held_port_message(error: transport::Error) -> String {
    let clock = clock();
    let pipe = Pipe::sim(
        Scenario::Load5v1a,
        FaultPlan::new(),
        &clock,
        PipeOpts::default(),
    );
    let (_events, sink) = capture(&clock);
    let mut worker = Worker::start(
        HeldPort { pipe, error },
        clock.clone(),
        WorkerConfig::new(Rate::Sps50, 1),
        sink,
    )
    .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    worker
        .write_settings(&changes())
        .expect_err("the port is held")
        .message
}

#[test]
fn a_port_held_by_another_program_is_named_in_plain_words() {
    let mut cases = vec![
        transport::Error::Io("Device or resource busy (os error 16)".into()),
        transport::Error::Io("Resource busy (os error 16)".into()),
    ];
    if cfg!(windows) {
        cases.push(transport::Error::Permission(
            "Access is denied. (os error 5)".into(),
        ));
    }
    for error in cases {
        let text = held_port_message(error).to_lowercase();
        assert!(text.contains("serial port"), "{text}");
        assert!(text.contains("another program"), "{text}");
    }
}

/// What `serialport` 4.10.1 returns for a port someone else holds: EBUSY on
/// unix and ERROR_ACCESS_DENIED on Windows both come back as kind `NoDevice`.
fn no_device() -> serialport::Error {
    serialport::Error::new(serialport::ErrorKind::NoDevice, "Resource busy")
}

fn meter_port() -> PortCandidate {
    PortCandidate {
        name: "/dev/cu.usbmodem1101".into(),
        vid: powerz_protocol::VID,
        pid: 0x0001,
        serial: Some("KM1".into()),
    }
}

#[test]
fn a_listed_port_that_real_open_calls_busy_is_named_in_plain_words() {
    let error = cdc::open_listed(&[meter_port()], "KM1", |_| Err::<(), _>(no_device()))
        .expect_err("the port is held");
    let text = held_port_message(error).to_lowercase();
    assert!(text.contains("serial port"), "{text}");
    assert!(text.contains("another program"), "{text}");
    assert!(!text.contains("no meter"), "{text}");
}

#[test]
fn a_meter_that_is_not_listed_is_still_no_meter_found() {
    let error =
        cdc::open_listed(&[], "KM1", |_| Err::<(), _>(no_device())).expect_err("nothing listed");
    let text = held_port_message(error).to_lowercase();
    assert!(text.contains("no meter"), "{text}");
    assert!(!text.contains("another program"), "{text}");
}

fn start_with(faults: FaultPlan) -> Worker<Pipe, SimClock> {
    let clock = clock();
    let pipe = Pipe::sim(Scenario::Load5v1a, faults, &clock, PipeOpts::default());
    let (_events, sink) = capture(&clock);
    Worker::start(pipe, clock.clone(), WorkerConfig::new(Rate::Sps50, 1), sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"))
}

fn view_after_write(faults: FaultPlan) -> Value {
    let mut worker = start_with(faults);
    let view = worker
        .write_settings(&changes())
        .unwrap_or_else(|e| panic!("the write is taken: {}", e.message));
    serde_json::to_value(view).unwrap()
}

#[test]
fn a_write_that_moves_nothing_else_has_no_warning() {
    let v = view_after_write(FaultPlan::new());
    assert_eq!(v["warnings"], Value::Array(vec![]), "{v}");
}

#[test]
fn a_difference_in_the_undecoded_bytes_is_a_warning_not_an_error() {
    let v = view_after_write(FaultPlan::new().stir_undecoded_settings());
    let warnings = v["warnings"].as_array().unwrap_or_else(|| panic!("{v}"));
    assert_eq!(warnings.len(), 1, "{v}");
    let text = warnings[0].as_str().unwrap_or_default();
    assert!(text.contains("byte"), "{text}");
    let brightness = v["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == "brightness")
        .unwrap();
    assert!(brightness["value"].as_str().unwrap().contains("30"), "{v}");
}
