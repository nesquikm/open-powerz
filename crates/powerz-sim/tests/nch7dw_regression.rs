//! FR NCH7DW, AC .1 (regression), with only API that exists before the
//! fix: an application-mode simulated meter that comes back from JUMP_DFU
//! in the own-ids old bootloader (two simulated meters behind one opener),
//! flashed with the opt-in, runs end to end on the old-bootloader path with
//! no refusal.
//!
//! Only the simulator is used; every container is synthetic.

mod common;
#[path = "../../powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::sync::atomic::AtomicBool;

use common::*;
use fw_container::*;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::firmware::container::FirmwareContainer;
use powerz_protocol::firmware::flash::{
    self, FlashAck, FlashError, FlashOutcome, Meter, MeterOpener,
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

/// Application mode until JUMP_DFU, then the own-ids old bootloader (which
/// JUMP_APROM returns to its application).
struct ComesBackOld {
    app: SimMeter,
    old: SimMeter,
}

impl MeterOpener for ComesBackOld {
    type Pipe = SimMeterPipe;

    fn open(&mut self) -> Result<Option<Meter<SimMeterPipe>>, TransportError> {
        if self.app.mode() == DeviceMode::Application {
            self.app.open()
        } else {
            self.old.open()
        }
    }
}

struct Switched {
    result: Result<FlashOutcome, FlashError>,
    old: SimMeter,
}

fn flash_after_switch() -> Switched {
    let clock = clock();
    let app = SimMeter::new(
        device(Persona::Bulk, Scenario::Idle, &clock).with_mode(DeviceMode::Application),
    );
    let old = SimMeter::new(
        device(Persona::Bulk, Scenario::Idle, &clock)
            .with_mode(DeviceMode::LegacyBootloader)
            .with_own_reply_ids(),
    );
    let mut opener = ComesBackOld {
        app,
        old: old.clone(),
    };
    let bytes = container();
    let c = FirmwareContainer::parse(&bytes).expect("a synthetic container parses");
    let stop = AtomicBool::new(false);
    let result = flash::flash(
        FlashAck::acknowledge().with_experimental_old_bootloader(),
        &c,
        &mut opener,
        clock.clone(),
        &stop,
        &mut |_| {},
    );
    Switched { result, old }
}

#[test]
fn nch7dw_regression_opted_in_flash_goes_on_after_the_switch_to_the_old_bootloader() {
    let s = flash_after_switch();
    match &s.result {
        Ok(FlashOutcome::Verified { version }) => assert_eq!(version, "2.0.7"),
        other => panic!("expected the flash to run on after the switch, got {other:?}"),
    }
}

#[test]
fn nch7dw_regression_the_old_bootloader_receives_the_whole_download_after_the_switch() {
    let s = flash_after_switch();
    let writes = s.old.writes();
    let types: Vec<u8> = writes
        .iter()
        .filter(|w| w.len() <= 68)
        .map(|w| w[0] & 0x7F)
        .collect();
    let at = |ty: u8| types.iter().position(|t| *t == ty);
    assert!(
        matches!(
            (at(0x02), at(0x40), at(0x42), at(0x08)),
            (Some(c), Some(h), Some(d), Some(a)) if c < h && h < d && d < a
        ),
        "CONNECT, HEAD, descriptor, JUMP_APROM on the old bootloader: {types:02x?} ({:?})",
        s.result
    );
    let chunks = writes.iter().filter(|w| w.len() > 68).count();
    assert_eq!(chunks, 4, "four payload writes; {:?}", s.result);
}
