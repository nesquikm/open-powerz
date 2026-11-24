//! FR NCH7DW against the simulator: a meter that starts in application
//! mode and comes back from JUMP_DFU in the old bootloader that numbers its
//! own replies (`SimDevice::with_jump_dfu_to_old_bootloader`).
//!
//! - AC .1 (regression): with the opt-in it is flashed end to end on the
//!   old-bootloader path, with no refusal.
//! - AC .2: without the opt-in the switch ends with nothing sent to the old
//!   bootloader, with the after-switch error.
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
use powerz_protocol::firmware::flash::{self, FlashAck, FlashError, FlashOutcome};
use powerz_sim::SimMeter;

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

fn opted_in() -> FlashAck {
    FlashAck::acknowledge().with_experimental_old_bootloader()
}

/// An application-mode meter whose JUMP_DFU lands in the own-ids old
/// bootloader.
fn app_that_jumps_to_old(clock: &SimClock) -> SimMeter {
    SimMeter::new(
        device(Persona::Bulk, Scenario::Idle, clock)
            .with_mode(DeviceMode::Application)
            .with_jump_dfu_to_old_bootloader(),
    )
}

fn flash_with(ack: FlashAck) -> (Result<FlashOutcome, FlashError>, SimMeter) {
    let clock = clock();
    let meter = app_that_jumps_to_old(&clock);
    let bytes = container();
    let c = FirmwareContainer::parse(&bytes).expect("a synthetic container parses");
    let stop = AtomicBool::new(false);
    let r = flash::flash(
        ack,
        &c,
        &mut meter.clone(),
        clock.clone(),
        &stop,
        &mut |_| {},
    );
    (r, meter)
}

/// Index of the JUMP_DFU write (`09 ..`, 4 bytes).
fn jump_dfu_at(writes: &[Vec<u8>]) -> Option<usize> {
    writes
        .iter()
        .position(|w| w.len() == 4 && w[0] & 0x7F == 0x09)
}

// ---------------------------------------------------------------- AC-NCH7DW.1

#[test]
fn nch7dw_sim_opted_in_switch_to_old_boot_is_flashed_and_reports_the_new_version() {
    let (r, _) = flash_with(opted_in());
    match &r {
        Ok(FlashOutcome::Verified { version }) => assert_eq!(version, "2.0.7"),
        other => panic!("expected the new version after the switch, got {other:?}"),
    }
}

#[test]
fn nch7dw_sim_opted_in_switch_to_old_boot_runs_the_download_after_jump_dfu() {
    let (r, meter) = flash_with(opted_in());
    assert!(
        !matches!(
            r,
            Err(FlashError::LegacyBootloader | FlashError::SwitchedToLegacyBootloader)
        ),
        "refused after the switch: {r:?}"
    );
    let writes = meter.writes();
    let jump = jump_dfu_at(&writes).expect("JUMP_DFU sent");
    let after = &writes[jump + 1..];
    let types: Vec<u8> = after
        .iter()
        .filter(|w| w.len() <= 68)
        .map(|w| w[0] & 0x7F)
        .collect();
    let at = |ty: u8| types.iter().position(|t| *t == ty);
    let (connect, head, desc, aprom) = (at(0x02), at(0x40), at(0x42), at(0x08));
    assert!(
        matches!((connect, head, desc, aprom), (Some(c), Some(h), Some(d), Some(a)) if c < h && h < d && d < a),
        "CONNECT, HEAD, descriptor, JUMP_APROM after JUMP_DFU: {types:02x?}"
    );
    let chunks = after.iter().filter(|w| w.len() > 68).count();
    assert_eq!(chunks, 4, "four payload writes after the switch");
}

#[test]
fn nch7dw_sim_opted_in_switch_to_old_boot_ends_in_application_with_the_new_version() {
    let (r, meter) = flash_with(opted_in());
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(meter.mode(), DeviceMode::Application);
    assert_eq!(meter.app_version(), "2.0.7");
}

// ---------------------------------------------------------------- AC-NCH7DW.2

#[test]
fn nch7dw_sim_without_the_opt_in_the_switch_ends_with_the_after_switch_error() {
    let (r, _) = flash_with(FlashAck::acknowledge());
    assert!(
        matches!(r, Err(FlashError::SwitchedToLegacyBootloader)),
        "{r:?}"
    );
}

#[test]
fn nch7dw_sim_without_the_opt_in_nothing_is_sent_to_the_old_bootloader() {
    let (r, meter) = flash_with(FlashAck::acknowledge());
    assert!(r.is_err(), "{r:?}");
    let writes = meter.writes();
    let jump = jump_dfu_at(&writes).expect("JUMP_DFU sent");
    assert!(
        writes[jump + 1..].is_empty(),
        "frames after JUMP_DFU: {:02x?}",
        &writes[jump + 1..]
    );
    assert_eq!(meter.mode(), DeviceMode::LegacyBootloader, "it stays there");
}
