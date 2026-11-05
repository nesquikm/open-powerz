//! AC-AXHK2M.13 and .14 on the backend side: a stop the user asked for is
//! reported as its own kind, `cancelled`, never as a failed update, so the
//! page (live or reloaded) can show it as stopped; and a meter the OS lists
//! in bootloader mode is listed as such without a single command sent.
//!
//! Only the simulator is used; no real device is opened. The container is
//! synthetic and every identifier is the simulator's made-up one.

mod common;
#[path = "../../../../crates/powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::*;
use fw_container::*;
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener};
use open_powerz_desktop_lib::firmware_flash::{FlashControl, FlashDone, FlashStage, flash_file};
use open_powerz_desktop_lib::ui_error::{ErrorKind, UiError};
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::transport::{self, discovery::DeviceEntry, discovery::Model};
use powerz_sim::SimMeter;

/// One data-phase block `[static]`.
const B: usize = 4080;
/// 2 whole blocks plus a 100-byte remainder.
const P: usize = 2 * B + 100;

fn file() -> PathBuf {
    let payload: Vec<u8> = (0..P + 16).map(|i| (i * 31 + 3) as u8).collect();
    let bytes = ContainerBuilder::new()
        .payload(&payload)
        .field(SIZE, &P.to_string())
        .build();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("firmware-stop");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("stop-{n}.mencrypt"));
    std::fs::write(&path, bytes).expect("write container");
    path
}

/// Flash a simulated meter in `mode`; `on_stage` sees each stage first.
fn run(
    mode: DeviceMode,
    control: &FlashControl,
    on_stage: impl Fn(&FlashStage),
) -> Result<FlashDone, UiError> {
    let clock = clock();
    let dev = SimDevice::with_clock(Persona::Bulk, Scenario::Idle, clock.clone()).with_mode(mode);
    let mut opener = SimMeter::new(dev);
    flash_file(
        &file(),
        "2.0.7",
        &mut opener,
        clock,
        control.stop(),
        &mut |s| {
            control.observe(&s);
            on_stage(&s);
        },
    )
}

/// The words a stop must never carry: it is not a failure, and nothing
/// needs a retry.
fn assert_reads_as_a_stop(e: &UiError) {
    assert_eq!(e.kind, ErrorKind::Cancelled, "{e:?}");
    assert_eq!(
        serde_json::to_value(e).expect("serialises")["kind"],
        "cancelled",
        "{e:?}"
    );
    let all = format!("{} {}", e.message, e.hint.as_deref().unwrap_or("")).to_lowercase();
    for banned in [
        "failed",
        "retry",
        "run the update again",
        "interrupted:",
        "jump_dfu",
    ] {
        assert!(!all.contains(banned), "a stop reads {banned:?}: {e:?}");
    }
    assert!(all.contains("stop"), "the message says it stopped: {e:?}");
}

// ---------------------------------------------------------------- AC-AXHK2M.13

#[test]
fn a_stop_before_the_first_stage_is_reported_as_cancelled_not_failed() {
    let control = FlashControl::default();
    control.cancel().expect("nothing is erased yet");
    let e = run(DeviceMode::Application, &control, |_| {}).expect_err("stopped");
    assert_reads_as_a_stop(&e);
}

#[test]
fn a_stop_while_switching_is_reported_as_cancelled_with_the_meter_s_likely_state() {
    let control = FlashControl::default();
    let e = run(DeviceMode::Application, &control, |s| {
        if *s == FlashStage::Switching {
            control.cancel().expect("the erase has not started");
        }
    })
    .expect_err("stopped");
    assert_reads_as_a_stop(&e);
    let all = format!("{} {}", e.message, e.hint.as_deref().unwrap_or("")).to_lowercase();
    // Plain words on the state the meter may be in: nothing was erased, and
    // it may be sitting in its bootloader.
    assert!(all.contains("bootloader"), "{e:?}");
    assert!(all.contains("erased"), "{e:?}");
}

#[test]
fn a_real_failure_keeps_its_failure_kind() {
    // A missing file is refused: a failure of its own, not a stop.
    let control = FlashControl::default();
    let clock = clock();
    let dev = SimDevice::with_clock(Persona::Bulk, Scenario::Idle, clock.clone())
        .with_mode(DeviceMode::Application);
    let mut opener = SimMeter::new(dev);
    let bad = file().with_extension("missing");
    let e = flash_file(
        &bad,
        "2.0.7",
        &mut opener,
        clock,
        control.stop(),
        &mut |_| {},
    )
    .expect_err("no such file");
    assert_ne!(e.kind, ErrorKind::Cancelled, "{e:?}");
}

/// Hands out the simulated meter and counts every open.
#[derive(Clone)]
struct CountingOpener {
    clock: SimClock,
    opens: Arc<Mutex<usize>>,
}

impl Opener for CountingOpener {
    type Transport = SimDevice;
    fn open(&self, _device: &DeviceListing) -> Result<SimDevice, transport::Error> {
        *self.opens.lock().unwrap() += 1;
        Ok(SimDevice::with_clock(
            Persona::Bulk,
            Scenario::Load5v1a,
            self.clock.clone(),
        ))
    }
}

fn entry(interface: &str, address: u8) -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid: 0x0063,
        serial: Some("SERIAL-STOP-0001".to_owned()),
        product: Some("POWER-Z KM003C".to_owned()),
        interface_string: Some(interface.to_owned()),
        bus: "1".to_owned(),
        address,
    }
}

#[test]
fn a_kept_stop_reads_as_cancelled_after_a_reload() {
    let clock = clock();
    let opener = CountingOpener {
        clock: clock.clone(),
        opens: Default::default(),
    };
    let mut m = DeviceManager::new(|| Ok(vec![entry("APP", 4)]), opener, clock);
    let id = m
        .list_devices()
        .into_iter()
        .find(|d| d.demo_scenario.is_none())
        .expect("listed")
        .id;
    let lease = m.begin_flash(&id, &[]).expect("the meter is free");
    let control = FlashControl::default();
    control.cancel().expect("nothing is erased yet");
    let stopped = run(DeviceMode::Application, &control, |_| {});
    lease.finish(&stopped);
    let kept = m.flash_status().expect("the outcome is kept");
    let error = kept
        .outcome
        .and_then(|o| o.error)
        .expect("it did not succeed");
    assert_reads_as_a_stop(&error);
}

// ---------------------------------------------------------------- AC-AXHK2M.14

#[test]
fn a_bootloader_mode_meter_is_listed_as_such_and_listing_it_sends_nothing() {
    let clock = clock();
    let opens = Arc::new(Mutex::new(0));
    let opener = CountingOpener {
        clock: clock.clone(),
        opens: Arc::clone(&opens),
    };
    let m = DeviceManager::new(|| Ok(vec![entry("BOOTLOADER", 9)]), opener, clock);
    for _ in 0..3 {
        let list = m.list_devices();
        let meter = list
            .iter()
            .find(|d| d.demo_scenario.is_none())
            .expect("the bootloader-mode meter is listed");
        assert_eq!(meter.mode, DeviceMode::Bootloader);
        assert!(meter.label.contains("bootloader mode"), "{}", meter.label);
        assert_eq!(meter.serial.as_deref(), Some("SERIAL-STOP-0001"));
    }
    assert_eq!(*opens.lock().unwrap(), 0, "listing opened the meter");
}

#[test]
fn a_bootloader_mode_meter_seen_by_hotplug_is_never_opened() {
    let clock = clock();
    let opens = Arc::new(Mutex::new(0));
    let opener = CountingOpener {
        clock: clock.clone(),
        opens: Arc::clone(&opens),
    };
    let mut m = DeviceManager::new(|| Ok(vec![entry("BOOTLOADER", 9)]), opener, clock);
    let list = m.list_devices();
    assert!(!m.devices_changed(&list), "nothing may start");
    assert_eq!(*opens.lock().unwrap(), 0, "hotplug opened the meter");
}
