//! FR BKSE70 (AC .5) on the backend side: the desktop firmware update ends
//! with the old-bootloader message when the engine reports it, with no
//! 120 s wait, both when the meter comes back on its old (2.1) bootloader
//! after JUMP_DFU and when it is already in it.
//!
//! Only the simulator is used; no real device is opened. The container is
//! synthetic.

mod common;
#[path = "../../../../crates/powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use common::*;
use fw_container::*;
use open_powerz_desktop_lib::firmware_flash::{FlashDone, FlashOpener, FlashStage, flash_file};
use open_powerz_desktop_lib::ui_error::UiError;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::firmware::flash::{FlashAck, Meter, MeterOpener, SWITCH_TIMEOUT};
use powerz_protocol::transport;
use powerz_sim::{SimMeter, SimMeterPipe};

/// The how-to section the message links, written out independently.
const HOWTO_URL: &str = "https://github.com/nesquikm/open-powerz/blob/main/docs/how-to/update-firmware.md#old-bootloader";

/// 3 whole blocks plus a 100-byte remainder.
const P: usize = 3 * 4080 + 100;

fn write_file() -> PathBuf {
    let payload: Vec<u8> = (0..P + 16).map(|i| (i * 53 + 7) as u8).collect();
    let bytes = ContainerBuilder::new()
        .payload(&payload)
        .field(SIZE, &P.to_string())
        .build();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bkse70-flash-desktop");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    // One file per call: the tests run in parallel, and a shared path let
    // one test read another's half-written file.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("fw-{}-{n}.mencrypt", std::process::id()));
    std::fs::write(&path, bytes).expect("write container");
    path
}

fn sim(mode: DeviceMode, clock: &SimClock) -> SimMeter {
    SimMeter::new(
        SimDevice::with_clock(Persona::Bulk, Scenario::Idle, clock.clone()).with_mode(mode),
    )
}

/// A meter in application mode that comes back on its old bootloader
/// after JUMP_DFU.
struct ComesBackOld {
    app: SimMeter,
    old: SimMeter,
}

impl MeterOpener for ComesBackOld {
    type Pipe = SimMeterPipe;

    fn open(&mut self) -> Result<Option<Meter<SimMeterPipe>>, transport::Error> {
        if self.app.mode() == DeviceMode::Application {
            self.app.open()
        } else {
            self.old.open()
        }
    }
}

impl FlashOpener for ComesBackOld {
    fn arm(&mut self, _ack: FlashAck) {}
}

fn run<O: FlashOpener>(
    opener: &mut O,
    clock: &SimClock,
) -> (Result<FlashDone, UiError>, Vec<FlashStage>) {
    let path = write_file();
    let stop = AtomicBool::new(false);
    let mut stages = Vec::new();
    let r = flash_file(&path, "2.0.7", opener, clock.clone(), &stop, &mut |s| {
        stages.push(s)
    });
    (r, stages)
}

fn assert_old_bootloader_message(e: &UiError) {
    let lower = e.message.to_lowercase();
    assert!(
        lower.contains("supported only experimentally, from the cli"),
        "{e:?}"
    );
    assert!(lower.contains("unplug"), "{e:?}");
    assert!(e.message.contains(HOWTO_URL), "{e:?}");
}

#[test]
fn bkse70_desktop_update_ends_with_the_old_bootloader_message_when_the_meter_comes_back_old() {
    let clock = clock();
    let mut opener = ComesBackOld {
        app: sim(DeviceMode::Application, &clock),
        old: sim(DeviceMode::LegacyBootloader, &clock),
    };
    let (r, _) = run(&mut opener, &clock);
    let e = r.expect_err("an old bootloader cannot be flashed");
    assert_old_bootloader_message(&e);
}

#[test]
fn bkse70_desktop_update_does_not_wait_out_the_switch_timeout_for_the_old_bootloader() {
    let clock = clock();
    let mut opener = ComesBackOld {
        app: sim(DeviceMode::Application, &clock),
        old: sim(DeviceMode::LegacyBootloader, &clock),
    };
    let start = clock.now_ms();
    let (r, _) = run(&mut opener, &clock);
    assert!(r.is_err(), "{r:?}");
    let elapsed = clock.now_ms() - start;
    assert!(
        elapsed < SWITCH_TIMEOUT.as_millis() as u64 / 10,
        "the update took {elapsed} ms of simulated time"
    );
}

#[test]
fn bkse70_desktop_update_on_a_meter_already_in_the_old_bootloader_ends_with_the_message() {
    let clock = clock();
    let m = sim(DeviceMode::LegacyBootloader, &clock);
    let (r, _) = run(&mut m.clone(), &clock);
    let e = r.expect_err("refused");
    assert_old_bootloader_message(&e);
}

#[test]
fn bkse70_desktop_update_on_a_meter_already_in_the_old_bootloader_sends_nothing() {
    let clock = clock();
    let m = sim(DeviceMode::LegacyBootloader, &clock);
    let (r, _) = run(&mut m.clone(), &clock);
    assert!(r.is_err(), "{r:?}");
    assert!(m.writes().is_empty(), "{:02x?}", m.writes());
}
