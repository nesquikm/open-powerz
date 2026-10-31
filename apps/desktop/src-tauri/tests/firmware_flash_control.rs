//! Cancelling a firmware update and reporting a running one (FR 4PJRVJ,
//! AC .1 and .3), against the simulated bootloader. Cancel works until the
//! erase starts; the backend keeps the running stage for a reloaded page.
//!
//! Only the simulator is used; no real device is opened. The container is
//! synthetic and every identifier is the simulator's made-up one.

mod common;
#[path = "../../../../crates/powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use common::*;
use fw_container::*;
#[allow(unused_imports)]
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener};
use open_powerz_desktop_lib::firmware_flash::{FlashControl, FlashDone, FlashStage, flash_file};
use open_powerz_desktop_lib::ui_error::UiError;
use powerz_protocol::firmware::DeviceMode;
#[allow(unused_imports)]
use powerz_protocol::transport::{self, Transport, discovery::DeviceEntry, discovery::Model};
use powerz_sim::SimMeter;
use serde_json::json;
#[allow(unused_imports)]
use std::sync::{Arc, Mutex};

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

/// One data-phase block `[static]`.
const B: usize = 4080;
/// 3 whole blocks plus a 100-byte remainder.
const P: usize = 3 * B + 100;
/// The stored payload is 16 bytes longer than declared, as in the vendor file.
const STORED: usize = P + 16;

fn file() -> PathBuf {
    let payload: Vec<u8> = (0..STORED).map(|i| (i * 53 + 7) as u8).collect();
    let bytes = ContainerBuilder::new()
        .payload(&payload)
        .field(SIZE, &P.to_string())
        .build();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("firmware-flash-control");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("cancel.mencrypt");
    std::fs::write(&path, bytes).expect("write container");
    path
}

/// Flash a simulated meter in `mode`; `on_stage` sees each stage first.
fn run(
    mode: DeviceMode,
    control: &FlashControl,
    on_stage: impl Fn(&FlashStage),
) -> (Result<FlashDone, UiError>, Vec<FlashStage>) {
    let clock = clock();
    let dev = SimDevice::with_clock(Persona::Bulk, Scenario::Idle, clock.clone()).with_mode(mode);
    let mut opener = SimMeter::new(dev);
    let mut stages = Vec::new();
    let r = flash_file(
        &file(),
        "2.0.7",
        &mut opener,
        clock,
        control.stop(),
        &mut |s| {
            control.observe(&s);
            on_stage(&s);
            stages.push(s);
        },
    );
    (r, stages)
}

fn erased(stages: &[FlashStage]) -> bool {
    stages
        .iter()
        .any(|s| matches!(s, FlashStage::Erasing | FlashStage::Writing { .. }))
}

// ---------------------------------------------------------------- AC-4PJRVJ.1

#[test]
fn a_cancel_before_the_first_stage_sends_nothing() {
    let control = FlashControl::default();
    control.cancel().expect("nothing is erased yet");
    let (r, stages) = run(DeviceMode::Application, &control, |_| {});
    let e = r.expect_err("cancelled");
    assert!(
        e.message.to_lowercase().contains("stopped") || e.message.to_lowercase().contains("cancel"),
        "{e:?}"
    );
    assert_eq!(stages, Vec::new());
}

#[test]
fn a_cancel_while_switching_stops_before_the_erase() {
    let control = FlashControl::default();
    let (r, stages) = run(DeviceMode::Application, &control, |s| {
        if *s == FlashStage::Switching {
            control.cancel().expect("the erase has not started");
        }
    });
    assert!(r.is_err(), "{r:?}");
    assert!(!erased(&stages), "{stages:?}");
}

#[test]
fn once_erasing_began_cancel_is_refused_with_the_reason_and_the_update_goes_on() {
    let control = FlashControl::default();
    let (r, stages) = run(DeviceMode::Bootloader, &control, |s| {
        if !matches!(s, FlashStage::Erasing) {
            return;
        }
        let e = control.cancel().expect_err("too late to cancel");
        assert!(e.message.to_lowercase().contains("eras"), "{e:?}");
    });
    assert!(
        !control.stop().load(Ordering::SeqCst),
        "the flag stays down"
    );
    assert!(r.is_ok(), "{r:?}");
    assert!(stages.contains(&FlashStage::Finished), "{stages:?}");
}

#[test]
fn the_cancel_gate_stays_shut_for_every_stage_after_the_erase() {
    for later in [
        FlashStage::Writing { done: 1, total: 2 },
        FlashStage::Verifying,
        FlashStage::Finished,
        FlashStage::Returning,
    ] {
        let control = FlashControl::default();
        control.observe(&FlashStage::Erasing);
        control.observe(&later);
        assert!(control.cancel().is_err(), "{later:?}");
        assert!(!control.stop().load(Ordering::SeqCst), "{later:?}");
    }
}

#[test]
fn a_replug_before_the_erase_leaves_cancel_open() {
    let control = FlashControl::default();
    control.observe(&FlashStage::Switching);
    control.observe(&FlashStage::Replug {
        to: "bootloader",
        reply: "none".to_owned(),
    });
    control.cancel().expect("the erase has not started");
    assert!(control.stop().load(Ordering::SeqCst));
}

// ---------------------------------------------------------------- AC-4PJRVJ.3

#[test]
fn the_control_keeps_the_latest_stage() {
    let control = FlashControl::default();
    assert_eq!(control.stage(), None);
    control.observe(&FlashStage::Erasing);
    control.observe(&FlashStage::Writing { done: 4, total: 8 });
    assert_eq!(
        control.stage(),
        Some(FlashStage::Writing { done: 4, total: 8 })
    );
}

fn manager_with_meter() -> (DeviceManager<LoggingOpener, SimClock>, String) {
    let clock = clock();
    let opener = LoggingOpener {
        clock: clock.clone(),
        logs: Default::default(),
    };
    let m = DeviceManager::new(|| Ok(vec![listed("APP", 4)]), opener, clock);
    let id = m
        .list_devices()
        .into_iter()
        .find(|d| d.demo_scenario.is_none())
        .expect("the meter is listed")
        .id;
    (m, id)
}

#[test]
fn the_backend_reports_a_running_update_with_its_meter_and_stage() {
    let (mut m, id) = manager_with_meter();
    assert_eq!(m.flash_status(), None, "no update runs");

    let lease = m.begin_flash(&id).expect("the meter is free");
    let status = m.flash_status().expect("the lease is held");
    assert_eq!(status.device_id, id);
    assert_eq!(status.stage, None, "no stage reported yet");

    lease
        .control()
        .observe(&FlashStage::Writing { done: 4, total: 8 });
    let status = m.flash_status().expect("still running");
    assert_eq!(
        serde_json::to_value(&status).expect("serialises"),
        json!({
            "device_id": id,
            "stage": { "stage": "writing", "done": 4, "total": 8 },
        })
    );

    drop(lease);
    assert_eq!(m.flash_status(), None, "the update ended");
}

#[test]
fn the_manager_cancels_the_running_update_until_the_erase() {
    let (mut m, id) = manager_with_meter();
    assert!(m.cancel_flash().is_err(), "nothing runs to cancel");

    let lease = m.begin_flash(&id).expect("the meter is free");
    m.cancel_flash().expect("not erasing yet");
    assert!(lease.control().stop().load(Ordering::SeqCst));
    drop(lease);

    let lease = m.begin_flash(&id).expect("released");
    assert!(
        !lease.control().stop().load(Ordering::SeqCst),
        "a new update starts with the flag down"
    );
    lease.control().observe(&FlashStage::Erasing);
    let e = m.cancel_flash().expect_err("erasing");
    assert!(e.message.to_lowercase().contains("eras"), "{e:?}");
}
