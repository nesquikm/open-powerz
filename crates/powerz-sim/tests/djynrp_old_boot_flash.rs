//! FR DJYNRP against the simulator's old bootloader that numbers its own
//! replies (`with_mode(LegacyBootloader).with_own_reply_ids()`).
//!
//! - AC .1: with the opt-in, a flash on it runs end to end: CONNECT, HEAD,
//!   the descriptor, the erase, every payload write with SYNC, FINISHED,
//!   JUMP_APROM, then the version check after the return.
//! - AC .2: without the opt-in it is refused before any frame; `jump` keeps
//!   refusing it; the switch wait after JUMP_DFU still stops at once when
//!   the old bootloader appears, even with the opt-in.
//! - AC .5: on application and bootloader 2.5 meters the frames sent are
//!   identical with and without the opt-in.
//!
//! Only the simulator is used; every container is synthetic.

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
    self, FlashAck, FlashError, FlashOutcome, FlashProgress, JumpTarget, Meter, MeterOpener,
};
use powerz_protocol::transport::Error as TransportError;
use powerz_sim::{SimMeter, SimMeterPipe};

const B: usize = 4080;
/// 3 whole blocks plus a 100-byte remainder.
const P: usize = 3 * B + 100;

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

fn opted_in() -> FlashAck {
    FlashAck::acknowledge().with_experimental_old_bootloader()
}

/// The own-ids old bootloader on `clock`.
fn own_ids(clock: &SimClock) -> SimMeter {
    SimMeter::new(
        device(Persona::Bulk, Scenario::Idle, clock)
            .with_mode(DeviceMode::LegacyBootloader)
            .with_own_reply_ids(),
    )
}

fn sim(mode: DeviceMode, clock: &SimClock) -> SimMeter {
    SimMeter::new(device(Persona::Bulk, Scenario::Idle, clock).with_mode(mode))
}

fn flash_on<O: MeterOpener>(
    ack: FlashAck,
    opener: &mut O,
    clock: &SimClock,
) -> (Result<FlashOutcome, FlashError>, Vec<FlashProgress>) {
    let bytes = container();
    let c = parse(&bytes);
    let stop = AtomicBool::new(false);
    let mut events = Vec::new();
    let r = flash::flash(ack, &c, opener, clock.clone(), &stop, &mut |p| {
        events.push(p)
    });
    (r, events)
}

/// Frame types of the framed writes (≤ 68 bytes: requests and jumps).
fn framed_types(writes: &[Vec<u8>]) -> Vec<u8> {
    writes
        .iter()
        .filter(|w| w.len() <= 68)
        .map(|w| w[0] & 0x7F)
        .collect()
}

fn chunks(writes: &[Vec<u8>]) -> Vec<Vec<u8>> {
    writes.iter().filter(|w| w.len() > 68).cloned().collect()
}

struct Flashed {
    result: Result<FlashOutcome, FlashError>,
    events: Vec<FlashProgress>,
    meter: SimMeter,
}

fn flash_own_ids() -> Flashed {
    let clock = clock();
    let meter = own_ids(&clock);
    let (result, events) = flash_on(opted_in(), &mut meter.clone(), &clock);
    Flashed {
        result,
        events,
        meter,
    }
}

// ---------------------------------------------------------------- AC-DJYNRP.1

#[test]
fn djynrp_sim_own_ids_old_boot_flashes_end_to_end_and_reports_the_new_version() {
    let f = flash_own_ids();
    match &f.result {
        Ok(FlashOutcome::Verified { version }) => assert_eq!(version, "2.0.7"),
        other => panic!("expected the new version, got {other:?}"),
    }
}

#[test]
fn djynrp_sim_own_ids_old_boot_is_back_in_application_mode_with_the_new_version() {
    let f = flash_own_ids();
    assert!(f.result.is_ok(), "{:?}", f.result);
    assert_eq!(f.meter.mode(), DeviceMode::Application);
    assert_eq!(f.meter.app_version(), "2.0.7");
}

#[test]
fn djynrp_sim_own_ids_old_boot_sends_the_bootloader_sequence_in_order() {
    let f = flash_own_ids();
    assert!(f.result.is_ok(), "{:?}", f.result);
    let writes = f.meter.writes();
    let types = framed_types(&writes);
    let at = |ty: u8| {
        types
            .iter()
            .position(|t| *t == ty)
            .unwrap_or_else(|| panic!("no {ty:#04x} in {types:02x?}"))
    };
    let (connect, head, desc, aprom) = (at(0x02), at(0x40), at(0x42), at(0x08));
    assert!(
        connect < head && head < desc && desc < aprom,
        "CONNECT, HEAD, descriptor, JUMP_APROM: {types:02x?}"
    );
    // The version check after the return: CONNECT, then the info read.
    let after = &types[aprom + 1..];
    let c = after.iter().position(|t| *t == 0x02);
    let m = after.iter().position(|t| *t == 0x44);
    assert!(
        matches!((c, m), (Some(c), Some(m)) if c < m),
        "CONNECT then the info read after JUMP_APROM: {types:02x?}"
    );
}

#[test]
fn djynrp_sim_own_ids_old_boot_receives_every_payload_write() {
    let f = flash_own_ids();
    assert!(f.result.is_ok(), "{:?}", f.result);
    let bytes = container();
    let c = parse(&bytes);
    let sent = chunks(&f.meter.writes());
    let expected: Vec<Vec<u8>> = c.payload().chunks(B).map(<[u8]>::to_vec).collect();
    assert_eq!(sent.len(), 4, "four payload writes");
    assert_eq!(sent, expected);
}

#[test]
fn djynrp_sim_own_ids_old_boot_receives_a_descriptor_from_0x4000() {
    let f = flash_own_ids();
    assert!(f.result.is_ok(), "{:?}", f.result);
    let d = f.meter.received_descriptor().expect("a descriptor arrived");
    let word = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
    assert_eq!(word(0), 0x4000);
    assert_eq!(word(8), P as u32);
}

#[test]
fn djynrp_sim_own_ids_old_boot_reaches_finished_and_returns() {
    let f = flash_own_ids();
    assert!(f.result.is_ok(), "{:?}", f.result);
    for stage in [
        FlashProgress::Erasing,
        FlashProgress::Verifying,
        FlashProgress::Finished,
        FlashProgress::Returning,
    ] {
        assert!(f.events.contains(&stage), "no {stage:?} in {:?}", f.events);
    }
    assert!(
        f.events
            .iter()
            .any(|e| matches!(e, FlashProgress::Writing { done, .. } if *done == P + 16)),
        "every write acknowledged: {:?}",
        f.events
    );
}

// ---------------------------------------------------------------- AC-DJYNRP.2

#[test]
fn djynrp_sim_without_the_opt_in_own_ids_old_boot_is_refused_before_any_frame() {
    let clock = clock();
    let meter = own_ids(&clock);
    let (r, _) = flash_on(FlashAck::acknowledge(), &mut meter.clone(), &clock);
    assert!(matches!(r, Err(FlashError::LegacyBootloader)), "{r:?}");
    assert!(meter.writes().is_empty(), "{:02x?}", meter.writes());
}

#[test]
fn djynrp_sim_jump_keeps_refusing_own_ids_old_boot_with_the_opt_in() {
    for target in [JumpTarget::Application, JumpTarget::Bootloader] {
        let clock = clock();
        let meter = own_ids(&clock);
        let stop = AtomicBool::new(false);
        let r = flash::jump(
            opted_in(),
            target,
            &mut meter.clone(),
            clock.clone(),
            &stop,
            &mut |_| {},
        );
        assert!(
            matches!(r, Err(FlashError::LegacyBootloader)),
            "{target:?}: {r:?}"
        );
        assert!(meter.writes().is_empty(), "{target:?}");
    }
}

/// Application mode that comes back on the old bootloader after JUMP_DFU.
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

#[test]
fn djynrp_sim_switch_wait_still_stops_at_once_on_the_old_bootloader_with_the_opt_in() {
    let clock = clock();
    let app = sim(DeviceMode::Application, &clock);
    let old = own_ids(&clock);
    let opens = Rc::new(Cell::new(0));
    let mut opener = ComesBackOld {
        app: app.clone(),
        old: old.clone(),
        opens_after_jump: opens.clone(),
    };
    let (r, _) = flash_on(opted_in(), &mut opener, &clock);
    assert!(matches!(r, Err(FlashError::LegacyBootloader)), "{r:?}");
    assert_eq!(opens.get(), 1, "the wait looks once and stops");
    assert!(old.writes().is_empty(), "{:02x?}", old.writes());
}

// ---------------------------------------------------------------- AC-DJYNRP.5

fn frames_of(mode: DeviceMode, ack: FlashAck) -> (Result<FlashOutcome, FlashError>, Vec<Vec<u8>>) {
    let clock = clock();
    let meter = sim(mode, &clock);
    let (r, _) = flash_on(ack, &mut meter.clone(), &clock);
    (r, meter.writes())
}

#[test]
fn djynrp_sim_application_mode_frames_are_identical_with_and_without_the_opt_in() {
    let (r1, plain) = frames_of(DeviceMode::Application, FlashAck::acknowledge());
    let (r2, opted) = frames_of(DeviceMode::Application, opted_in());
    assert!(r1.is_ok() && r2.is_ok(), "{r1:?} / {r2:?}");
    assert_eq!(opted, plain);
}

#[test]
fn djynrp_sim_bootloader_2_5_frames_are_identical_with_and_without_the_opt_in() {
    let (r1, plain) = frames_of(DeviceMode::Bootloader, FlashAck::acknowledge());
    let (r2, opted) = frames_of(DeviceMode::Bootloader, opted_in());
    assert!(r1.is_ok() && r2.is_ok(), "{r1:?} / {r2:?}");
    assert_eq!(opted, plain);
}
