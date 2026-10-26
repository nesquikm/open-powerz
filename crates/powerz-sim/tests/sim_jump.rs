//! FR V9SK56, AC .7 against the simulator: the library's write-only mode
//! switch ([`flash::jump`]) moves a [`SimMeter`] between application and
//! bootloader mode with one jump frame and nothing else.
//!
//! Only the simulator is used; no real device is opened.

mod common;

use std::sync::atomic::AtomicBool;

use common::*;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::firmware::flash::{
    self, FlashAck, FlashError, JumpOutcome, JumpReply, JumpSent, JumpTarget,
};
use powerz_sim::{FlashFault, SimMeter};

/// A simulated KM003C in `mode` with `fault` (if any) on a fake clock.
fn meter(mode: DeviceMode, fault: Option<FlashFault>) -> (SimMeter, SimClock) {
    let clock = clock();
    let mut dev = device(Persona::Bulk, Scenario::Idle, &clock).with_mode(mode);
    if let Some(f) = fault {
        dev = dev.with_flash_fault(f);
    }
    (SimMeter::new(dev), clock)
}

fn run(meter: &SimMeter, clock: &SimClock, target: JumpTarget) -> Result<JumpOutcome, FlashError> {
    run_sent(meter, clock, target).0
}

/// [`run`], also returning each jump report.
fn run_sent(
    meter: &SimMeter,
    clock: &SimClock,
    target: JumpTarget,
) -> (Result<JumpOutcome, FlashError>, Vec<JumpSent>) {
    let mut opener = meter.clone();
    let stop = AtomicBool::new(false);
    let mut sent = Vec::new();
    let r = flash::jump(
        FlashAck::acknowledge(),
        target,
        &mut opener,
        clock.clone(),
        &stop,
        &mut |s| sent.push(*s),
    );
    (r, sent)
}

/// Every write the meter saw is the one jump frame `ty id 00 00`.
fn assert_only_the_jump(writes: &[Vec<u8>], ty: u8) {
    assert_eq!(writes.len(), 1, "exactly one write: {writes:02x?}");
    let w = &writes[0];
    assert_eq!(w.len(), 4, "{w:02x?}");
    assert_eq!(w[0], ty, "{w:02x?}");
    assert_eq!(&w[2..4], &[0, 0], "{w:02x?}");
}

#[test]
fn sim_jump_dfu_puts_the_application_into_its_bootloader() {
    let (m, clock) = meter(DeviceMode::Application, None);
    let r = run(&m, &clock, JumpTarget::Bootloader);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Bootloader)));
    assert_eq!(m.mode(), DeviceMode::Bootloader);
    assert_only_the_jump(&m.writes(), 0x09);
}

#[test]
fn sim_jump_aprom_brings_the_bootloader_back_to_the_application() {
    let (m, clock) = meter(DeviceMode::Bootloader, None);
    let r = run(&m, &clock, JumpTarget::Application);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Application)));
    assert_eq!(m.mode(), DeviceMode::Application);
    assert_only_the_jump(&m.writes(), 0x08);
}

#[test]
fn sim_there_and_back_again() {
    let (m, clock) = meter(DeviceMode::Application, None);
    assert_eq!(
        run(&m, &clock, JumpTarget::Bootloader),
        Ok(JumpOutcome::Reached(DeviceMode::Bootloader))
    );
    assert_eq!(
        run(&m, &clock, JumpTarget::Application),
        Ok(JumpOutcome::Reached(DeviceMode::Application))
    );
    let types: Vec<u8> = m.writes().iter().map(|w| w[0]).collect();
    assert_eq!(types, vec![0x09, 0x08], "only the two jumps were sent");
}

#[test]
fn sim_a_jump_to_the_mode_it_is_already_in_is_refused_and_sends_nothing() {
    for (mode, target) in [
        (DeviceMode::Application, JumpTarget::Application),
        (DeviceMode::Bootloader, JumpTarget::Bootloader),
    ] {
        let (m, clock) = meter(mode, None);
        let e = run(&m, &clock, target).expect_err("already there");
        assert_eq!(e, FlashError::AlreadyInMode { mode });
        assert!(m.writes().is_empty(), "{:02x?}", m.writes());
        assert_eq!(m.mode(), mode);
    }
}

#[test]
fn sim_an_unknown_mode_is_refused_and_sends_nothing() {
    let (m, clock) = meter(DeviceMode::Unknown, None);
    let e = run(&m, &clock, JumpTarget::Bootloader).expect_err("unknown mode");
    assert_eq!(e, FlashError::UnknownMode);
    assert!(m.writes().is_empty(), "{:02x?}", m.writes());
}

#[test]
fn sim_an_ignored_jump_dfu_is_not_reached_in_application_mode() {
    let (m, clock) = meter(DeviceMode::Application, Some(FlashFault::JumpDfuIgnored));
    let r = run(&m, &clock, JumpTarget::Bootloader);
    assert_eq!(
        r,
        Ok(JumpOutcome::NotReached {
            seen: Some(DeviceMode::Application)
        })
    );
    assert_only_the_jump(&m.writes(), 0x09);
}

#[test]
fn sim_an_ignored_jump_aprom_is_not_reached_in_bootloader_mode() {
    let (m, clock) = meter(DeviceMode::Bootloader, Some(FlashFault::JumpApromIgnored));
    let r = run(&m, &clock, JumpTarget::Application);
    assert_eq!(
        r,
        Ok(JumpOutcome::NotReached {
            seen: Some(DeviceMode::Bootloader)
        })
    );
    assert_only_the_jump(&m.writes(), 0x08);
}

#[test]
fn sim_a_jump_write_that_disconnects_still_reaches_the_target() {
    let (m, clock) = meter(
        DeviceMode::Application,
        Some(FlashFault::JumpDfuDisconnects),
    );
    let r = run(&m, &clock, JumpTarget::Bootloader);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Bootloader)));
    assert_only_the_jump(&m.writes(), 0x09);

    let (m, clock) = meter(
        DeviceMode::Bootloader,
        Some(FlashFault::JumpApromDisconnects),
    );
    let r = run(&m, &clock, JumpTarget::Application);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Application)));
    assert_only_the_jump(&m.writes(), 0x08);
}

#[test]
fn sim_a_failing_jump_dfu_write_is_a_device_error() {
    let (m, clock) = meter(DeviceMode::Application, Some(FlashFault::JumpDfuWriteFails));
    let e = run(&m, &clock, JumpTarget::Bootloader).expect_err("the write fails");
    assert!(matches!(e, FlashError::Device { .. }), "got {e:?}");
    assert_eq!(m.mode(), DeviceMode::Application);
    assert_only_the_jump(&m.writes(), 0x09);
}

// ---- AC-V9SK56.7: the reply to the jump, and a jump at the next power-up ----

#[test]
fn sim_the_jump_reply_is_none_and_no_stale_frame_is_taken_for_it() {
    let (m, clock) = meter(DeviceMode::Application, None);
    let (r, sent) = run_sent(&m, &clock, JumpTarget::Bootloader);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Bootloader)));
    assert_eq!(
        sent,
        vec![JumpSent {
            from: DeviceMode::Application,
            target: JumpTarget::Bootloader,
            reply: JumpReply::NoReply,
        }]
    );
}

/// A meter that, like 2.0.6, takes a jump at the next power-up, replugged
/// after `opens` looks (`None`: never).
fn power_up_meter(mode: DeviceMode, opens: Option<usize>) -> (SimMeter, SimClock) {
    let clock = clock();
    let dev = device(Persona::Bulk, Scenario::Idle, &clock)
        .with_mode(mode)
        .with_jump_at_power_up();
    let m = SimMeter::new(dev);
    (opens.map_or(m.clone(), |n| m.with_replug_after(n)), clock)
}

#[test]
fn sim_a_jump_at_power_up_is_not_live_and_a_replug_applies_it() {
    let (m, clock) = power_up_meter(DeviceMode::Application, None);
    let r = run(&m, &clock, JumpTarget::Bootloader);
    assert_eq!(
        r,
        Ok(JumpOutcome::NotReached {
            seen: Some(DeviceMode::Application)
        }),
        "no live switch"
    );
    assert_eq!(m.mode(), DeviceMode::Application);
    m.replug();
    assert_eq!(m.mode(), DeviceMode::Bootloader, "the next power-up");
    m.replug();
    assert_eq!(
        m.mode(),
        DeviceMode::Application,
        "a plain replug leaves it"
    );
}

#[test]
fn sim_a_jump_at_power_up_reaches_the_target_once_the_meter_is_replugged() {
    let (m, clock) = power_up_meter(DeviceMode::Application, Some(8));
    let r = run(&m, &clock, JumpTarget::Bootloader);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Bootloader)));
    assert_only_the_jump(&m.writes(), 0x09);

    let (m, clock) = power_up_meter(DeviceMode::Bootloader, Some(8));
    let r = run(&m, &clock, JumpTarget::Application);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Application)));
}
