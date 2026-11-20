//! FR BKSE70 (AC .1–.4) against the simulator: a meter on the old (2.1)
//! bootloader in the flash engine.
//!
//! - AC .1: a flash from application mode whose meter re-enumerates as the
//!   old bootloader after JUMP_DFU stops at the first poll that sees it,
//!   far below the 120 s switch timeout on the simulated clock, with the
//!   old-bootloader error, and sends nothing more.
//! - AC .2 / .3: a flash or a jump on a meter already in the old bootloader
//!   is refused before any frame.
//! - AC .4: the error says the bootloader is not supported yet, that
//!   unplugging and replugging returns the meter to its application
//!   firmware, and links the how-to's old-bootloader section.
//!
//! Only the simulator is used; no real device is opened. Every container is
//! synthetic.

mod common;
#[path = "../../powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;

use common::*;
use fw_container::*;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::firmware::container::FirmwareContainer;
use powerz_protocol::firmware::flash::{
    self, FlashAck, FlashError, FlashOutcome, JumpOutcome, JumpTarget, Meter, MeterOpener,
    SWITCH_TIMEOUT,
};
use powerz_protocol::transport::Error as TransportError;
use powerz_sim::{SimMeter, SimMeterPipe};

/// The how-to section the error links, written out independently.
const HOWTO_URL: &str = "https://github.com/nesquikm/open-powerz/blob/main/docs/how-to/update-firmware.md#old-bootloader";

/// 3 whole blocks plus a 100-byte remainder.
const P: usize = 3 * 4080 + 100;

fn container() -> Vec<u8> {
    let payload: Vec<u8> = (0..P + 16).map(|i| (i * 53 + 7) as u8).collect();
    ContainerBuilder::new()
        .payload(&payload)
        .field(SIZE, &P.to_string())
        .build()
}

fn parse(bytes: &[u8]) -> FirmwareContainer<'_> {
    FirmwareContainer::parse(bytes).expect("a synthetic container parses")
}

fn sim(mode: DeviceMode, clock: &SimClock) -> SimMeter {
    SimMeter::new(device(Persona::Bulk, Scenario::Idle, clock).with_mode(mode))
}

/// A KM003C in application mode that comes back on its old bootloader after
/// JUMP_DFU: opens see the application until it has obeyed the jump, then
/// the old-bootloader meter. Both share one simulated clock.
struct ComesBackOld {
    app: SimMeter,
    old: SimMeter,
    opens_after_jump: Rc<Cell<usize>>,
}

impl MeterOpener for ComesBackOld {
    type Pipe = SimMeterPipe;

    fn open(&mut self) -> Result<Option<Meter<SimMeterPipe>>, TransportError> {
        if self.app.mode() == DeviceMode::Application {
            self.app.open()
        } else {
            self.opens_after_jump.set(self.opens_after_jump.get() + 1);
            self.old.open()
        }
    }
}

fn flash_on<O: MeterOpener>(opener: &mut O, clock: &SimClock) -> Result<FlashOutcome, FlashError> {
    let bytes = container();
    let c = parse(&bytes);
    let stop = AtomicBool::new(false);
    flash::flash(
        FlashAck::acknowledge(),
        &c,
        opener,
        clock.clone(),
        &stop,
        &mut |_| {},
    )
}

fn jump_on(
    meter: &SimMeter,
    clock: &SimClock,
    target: JumpTarget,
) -> Result<JumpOutcome, FlashError> {
    let mut opener = meter.clone();
    let stop = AtomicBool::new(false);
    flash::jump(
        FlashAck::acknowledge(),
        target,
        &mut opener,
        clock.clone(),
        &stop,
        &mut |_| {},
    )
}

/// The old-bootloader error, recognised by what it says: not supported yet,
/// and the link to the how-to's section.
fn assert_names_the_old_bootloader(e: &FlashError) {
    let msg = e.to_string();
    assert!(
        msg.to_lowercase().contains("not supported yet"),
        "not the old-bootloader error: {msg}"
    );
    assert!(msg.contains(HOWTO_URL), "no how-to link in: {msg}");
}

/// The frame types of the framed writes (requests and jumps, ≤ 68 bytes).
fn framed_types(writes: &[Vec<u8>]) -> Vec<u8> {
    writes
        .iter()
        .filter(|w| w.len() <= 68)
        .map(|w| w[0] & 0x7F)
        .collect()
}

struct Switched {
    result: Result<FlashOutcome, FlashError>,
    app: SimMeter,
    old: SimMeter,
    opens_after_jump: usize,
    elapsed_ms: u64,
}

fn flash_that_comes_back_old() -> Switched {
    let clock = clock();
    let app = sim(DeviceMode::Application, &clock);
    let old = sim(DeviceMode::LegacyBootloader, &clock);
    let opens = Rc::new(Cell::new(0));
    let mut opener = ComesBackOld {
        app: app.clone(),
        old: old.clone(),
        opens_after_jump: opens.clone(),
    };
    let start = clock.now_ms();
    let result = flash_on(&mut opener, &clock);
    Switched {
        result,
        app,
        old,
        opens_after_jump: opens.get(),
        elapsed_ms: clock.now_ms() - start,
    }
}

// ---- AC-BKSE70.1: the switch wait ends when the old bootloader shows up ----

#[test]
fn bkse70_flash_stops_with_the_old_bootloader_error_when_the_meter_comes_back_old() {
    let s = flash_that_comes_back_old();
    let e = s.result.expect_err("an old bootloader cannot be flashed");
    assert_names_the_old_bootloader(&e);
}

#[test]
fn bkse70_the_switch_wait_ends_at_the_first_poll_that_sees_the_old_bootloader() {
    let s = flash_that_comes_back_old();
    assert!(s.result.is_err(), "{:?}", s.result);
    assert_eq!(
        s.opens_after_jump, 1,
        "the wait looks once and stops; it took {} looks over {} ms",
        s.opens_after_jump, s.elapsed_ms
    );
}

#[test]
fn bkse70_the_switch_wait_is_far_below_the_switch_timeout_on_the_sim_clock() {
    let s = flash_that_comes_back_old();
    assert!(s.result.is_err(), "{:?}", s.result);
    let limit = SWITCH_TIMEOUT.as_millis() as u64 / 10;
    assert!(
        s.elapsed_ms < limit,
        "the flash took {} ms of simulated time; the switch timeout is {} ms",
        s.elapsed_ms,
        SWITCH_TIMEOUT.as_millis()
    );
}

#[test]
fn bkse70_nothing_is_sent_after_jump_dfu_when_the_meter_comes_back_old() {
    let s = flash_that_comes_back_old();
    assert!(s.result.is_err(), "{:?}", s.result);
    let app_types = framed_types(&s.app.writes());
    assert_eq!(
        app_types.last(),
        Some(&0x09),
        "JUMP_DFU is the last frame to the application: {app_types:02x?}"
    );
    assert!(
        s.old.writes().is_empty(),
        "nothing goes to the old bootloader: {:02x?}",
        s.old.writes()
    );
}

// ---- AC-BKSE70.2: a flash on a meter already in the old bootloader ----

#[test]
fn bkse70_a_flash_on_the_old_bootloader_sends_no_frame() {
    let clock = clock();
    let m = sim(DeviceMode::LegacyBootloader, &clock);
    let r = flash_on(&mut m.clone(), &clock);
    assert!(r.is_err(), "{r:?}");
    assert!(m.writes().is_empty(), "{:02x?}", m.writes());
}

#[test]
fn bkse70_a_flash_on_the_old_bootloader_is_refused_with_the_old_bootloader_error() {
    let clock = clock();
    let m = sim(DeviceMode::LegacyBootloader, &clock);
    let e = flash_on(&mut m.clone(), &clock).expect_err("refused");
    assert_names_the_old_bootloader(&e);
}

// ---- AC-BKSE70.3: a jump on a meter in the old bootloader ----

#[test]
fn bkse70_a_jump_on_the_old_bootloader_sends_no_frame() {
    for target in [JumpTarget::Application, JumpTarget::Bootloader] {
        let clock = clock();
        let m = sim(DeviceMode::LegacyBootloader, &clock);
        let r = jump_on(&m, &clock, target);
        assert!(r.is_err(), "{target:?}: {r:?}");
        assert!(m.writes().is_empty(), "{target:?}: {:02x?}", m.writes());
    }
}

#[test]
fn bkse70_a_jump_on_the_old_bootloader_is_refused_with_the_old_bootloader_error() {
    for target in [JumpTarget::Application, JumpTarget::Bootloader] {
        let clock = clock();
        let m = sim(DeviceMode::LegacyBootloader, &clock);
        let e = jump_on(&m, &clock, target).expect_err("refused");
        assert_names_the_old_bootloader(&e);
    }
}

// ---- AC-BKSE70.4: what the error says ----

fn old_bootloader_message() -> String {
    let clock = clock();
    let m = sim(DeviceMode::LegacyBootloader, &clock);
    jump_on(&m, &clock, JumpTarget::Application)
        .expect_err("refused")
        .to_string()
}

#[test]
fn bkse70_the_error_says_this_bootloader_is_not_supported_yet() {
    let msg = old_bootloader_message();
    let lower = msg.to_lowercase();
    assert!(lower.contains("bootloader"), "{msg}");
    assert!(lower.contains("not supported yet"), "{msg}");
}

#[test]
fn bkse70_the_error_says_unplugging_and_replugging_returns_to_the_application_firmware() {
    let msg = old_bootloader_message();
    let lower = msg.to_lowercase();
    assert!(lower.contains("unplug"), "{msg}");
    assert!(
        lower.contains("replug")
            || lower.contains("plug it back in")
            || lower.contains("plug it in again"),
        "{msg}"
    );
    assert!(lower.contains("application firmware"), "{msg}");
}

#[test]
fn bkse70_the_error_links_the_old_bootloader_section_of_the_how_to() {
    let msg = old_bootloader_message();
    assert!(msg.contains(HOWTO_URL), "{msg}");
}
