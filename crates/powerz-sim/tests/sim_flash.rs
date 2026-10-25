//! FR V9SK54 against the simulator: the bootloader emulation (mode switch,
//! erase, write, verify) and an injected failure at every step (AC .8),
//! driven by the library's flash engine (AC .1–.6).
//!
//! Only the simulator is used; no real device is opened. Every container
//! is synthetic, built by the shared test builder, and every identifier is
//! the simulator's made-up one.

mod common;
#[path = "../../powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::sync::atomic::AtomicBool;

use common::*;
use fw_container::*;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::firmware::container::FirmwareContainer;
use powerz_protocol::firmware::flash::{self, FlashAck, FlashError, FlashOutcome, FlashProgress};
use powerz_sim::{FlashFault, SimMeter};

/// One data-phase block `[static]`.
const B: usize = 4080;
/// 3 whole blocks plus a 100-byte remainder: N = 3, four writes.
const P: usize = 3 * B + 100;

fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 53 + 7) as u8).collect()
}

/// A KM003C 2.0.7 container with a `P + 16`-byte stored payload declaring
/// `P` bytes (16 extra bytes, as in the vendor file).
fn container() -> Vec<u8> {
    ContainerBuilder::new()
        .payload(&payload(P + 16))
        .field(SIZE, &P.to_string())
        .build()
}

fn parse(bytes: &[u8]) -> FirmwareContainer<'_> {
    FirmwareContainer::parse(bytes).expect("a synthetic container parses")
}

/// A simulated KM003C in `mode` with `fault` (if any) on a fake clock.
fn meter(mode: DeviceMode, fault: Option<FlashFault>) -> (SimMeter, SimClock) {
    let clock = clock();
    let mut dev = device(Persona::Bulk, Scenario::Idle, &clock).with_mode(mode);
    if let Some(f) = fault {
        dev = dev.with_flash_fault(f);
    }
    (SimMeter::new(dev), clock)
}

/// Run the flash engine against `meter`.
fn run(
    meter: &SimMeter,
    clock: &SimClock,
    c: &FirmwareContainer<'_>,
) -> (Result<FlashOutcome, FlashError>, Vec<FlashProgress>) {
    let mut opener = meter.clone();
    let mut events = Vec::new();
    let stop = AtomicBool::new(false);
    let r = flash::flash(
        FlashAck::acknowledge(),
        c,
        &mut opener,
        clock.clone(),
        &stop,
        &mut |p| events.push(p),
    );
    (r, events)
}

/// Frame types of the framed writes (≤ 68 bytes: requests and jumps);
/// payload chunks here are always longer.
fn framed_types(writes: &[Vec<u8>]) -> Vec<u8> {
    writes
        .iter()
        .filter(|w| w.len() <= 68)
        .map(|w| w[0] & 0x7F)
        .collect()
}

/// The payload chunks among the writes.
fn chunks(writes: &[Vec<u8>]) -> Vec<Vec<u8>> {
    writes.iter().filter(|w| w.len() > 68).cloned().collect()
}

fn assert_recovery_text(e: &FlashError) {
    let msg = e.to_string();
    assert!(msg.contains("Back"), "no hold-Back hint in: {msg}");
    assert!(
        msg.to_lowercase().contains("plug"),
        "no plug-in hint in: {msg}"
    );
}

// ---- AC-V9SK54.8: the bootloader emulation, happy paths ----

#[test]
fn sim_flash_from_application_mode_switches_flashes_and_comes_back_with_the_new_version() {
    let bytes = container();
    let c = parse(&bytes);
    let (m, clock) = meter(DeviceMode::Application, None);
    assert_eq!(m.app_version(), "2.0.6");
    let (r, events) = run(&m, &clock, &c);
    match r {
        Ok(FlashOutcome::Verified { version }) => assert_eq!(version, "2.0.7"),
        other => panic!("expected the new version, got {other:?}"),
    }
    assert_eq!(m.mode(), DeviceMode::Application);
    assert_eq!(
        m.app_version(),
        "2.0.7",
        "the sim reports the flashed version"
    );
    assert!(matches!(events.first(), Some(FlashProgress::Switching)));
    let types = framed_types(&m.writes());
    assert_eq!(
        types.iter().filter(|t| **t == 0x09).count(),
        1,
        "{types:02x?}"
    );
    assert_eq!(
        types.iter().filter(|t| **t == 0x08).count(),
        1,
        "{types:02x?}"
    );
}

#[test]
fn sim_flash_from_bootloader_mode_needs_no_switch() {
    let bytes = container();
    let c = parse(&bytes);
    let (m, clock) = meter(DeviceMode::Bootloader, None);
    let (r, events) = run(&m, &clock, &c);
    assert!(matches!(r, Ok(FlashOutcome::Verified { .. })), "got {r:?}");
    assert!(!events.iter().any(|e| matches!(e, FlashProgress::Switching)));
    assert!(!framed_types(&m.writes()).contains(&0x09));
    assert_eq!(m.mode(), DeviceMode::Application);
}

#[test]
fn sim_receives_the_payload_byte_for_byte() {
    let bytes = container();
    let c = parse(&bytes);
    let (m, clock) = meter(DeviceMode::Bootloader, None);
    let (r, _) = run(&m, &clock, &c);
    assert!(r.is_ok(), "got {r:?}");
    assert_eq!(m.received_payload(), c.payload());
    let lens: Vec<usize> = chunks(&m.writes()).iter().map(Vec::len).collect();
    assert_eq!(lens, vec![B, B, B, 116]);
}

#[test]
fn sim_records_the_descriptor_fields() {
    let bytes = container();
    let c = parse(&bytes);
    let (m, clock) = meter(DeviceMode::Bootloader, None);
    let _ = run(&m, &clock, &c);
    let d = m.received_descriptor().expect("the sim saw a descriptor");
    assert_eq!(d.len(), 64);
    let word = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    assert_eq!(word(0x00), 0x4000);
    assert_eq!(word(0x04), 0x4000 + P as u32);
    assert_eq!(word(0x08), P as u32);
    assert_eq!(word(0x0C), 0xA175_E4A0);
    assert_eq!(&d[0x10..0x16], b"KM003C");
    assert_eq!(&d[0x1C..0x21], b"2.0.7");
    assert_eq!(&d[0x28..0x31], b"2026.9.29");
}

#[test]
fn sim_mode_follows_jump_dfu_and_jump_aprom() {
    let bytes = container();
    let c = parse(&bytes);
    let (m, clock) = meter(DeviceMode::Application, Some(FlashFault::NoFinished));
    let _ = run(&m, &clock, &c);
    // JUMP_DFU took it to the bootloader; the failed verify left it there.
    assert_eq!(m.mode(), DeviceMode::Bootloader);
    assert_eq!(
        m.app_version(),
        "2.0.6",
        "a failed flash changes no version"
    );
}

#[test]
fn sim_progress_reports_every_stage_in_order() {
    let bytes = container();
    let c = parse(&bytes);
    let (m, clock) = meter(DeviceMode::Application, None);
    let (_, events) = run(&m, &clock, &c);
    let mut stages: Vec<&str> = Vec::new();
    for e in &events {
        let s = match e {
            FlashProgress::Switching => "switching",
            FlashProgress::Erasing => "erasing",
            FlashProgress::Writing { .. } => "writing",
            FlashProgress::Verifying => "verifying",
            FlashProgress::Finished => "finished",
            FlashProgress::Returning => "returning",
        };
        if stages.last() != Some(&s) {
            stages.push(s);
        }
    }
    assert_eq!(
        stages,
        vec![
            "switching",
            "erasing",
            "writing",
            "verifying",
            "finished",
            "returning"
        ]
    );
    let last_write = events.iter().rev().find_map(|e| match e {
        FlashProgress::Writing { done, total } => Some((*done, *total)),
        _ => None,
    });
    assert_eq!(last_write, Some((P + 16, P + 16)));
}

// ---- AC-V9SK54.8 + .2 + .6: one injected failure per step ----

/// Flash from bootloader mode with `fault`; return the error and the meter.
fn boot_failure(fault: FlashFault) -> (FlashError, SimMeter) {
    let bytes = container();
    let c = parse(&bytes);
    let (m, clock) = meter(DeviceMode::Bootloader, Some(fault));
    let (r, _) = run(&m, &clock, &c);
    let e = r.expect_err("the injected failure fails the flash");
    (e, m)
}

/// The meter was left in the bootloader, unflashed, with the hint given.
fn assert_left_in_bootloader(e: &FlashError, m: &SimMeter) {
    assert_eq!(m.mode(), DeviceMode::Bootloader);
    assert_eq!(m.app_version(), "2.0.6");
    assert!(!framed_types(&m.writes()).contains(&0x08), "no JUMP_APROM");
    assert_recovery_text(e);
}

#[test]
fn sim_connect_without_accept_is_a_read_error_and_nothing_follows() {
    let (e, m) = boot_failure(FlashFault::ConnectNoAccept);
    assert!(matches!(e, FlashError::Read { .. }), "got {e:?}");
    assert_eq!(e.code(), -108);
    assert_eq!(
        framed_types(&m.writes()),
        vec![0x02],
        "one CONNECT, no retry"
    );
    assert_left_in_bootloader(&e, &m);
}

#[test]
fn sim_non_zero_head_field_is_a_work_mode_error_and_nothing_follows() {
    let bytes = container();
    let c = parse(&bytes);
    let clock = clock();
    let dev = device(Persona::Bulk, Scenario::Idle, &clock)
        .with_mode(DeviceMode::Bootloader)
        .with_head_field(1);
    let m = SimMeter::new(dev);
    let (r, _) = run(&m, &clock, &c);
    let e = r.expect_err("a non-zero HEAD field stops the flash");
    assert!(matches!(e, FlashError::WorkMode { .. }), "got {e:?}");
    assert_eq!(e.code(), -111);
    assert_eq!(framed_types(&m.writes()), vec![0x02, 0x40]);
    assert_left_in_bootloader(&e, &m);
}

#[test]
fn sim_descriptor_answered_is_a_write_error_and_nothing_follows() {
    let (e, m) = boot_failure(FlashFault::DescriptorAnswered);
    assert!(matches!(e, FlashError::Write { .. }), "got {e:?}");
    assert_eq!(e.code(), -107);
    assert_eq!(framed_types(&m.writes()), vec![0x02, 0x40, 0x42]);
    assert!(chunks(&m.writes()).is_empty());
    assert_left_in_bootloader(&e, &m);
}

#[test]
fn sim_early_accept_during_erase_is_an_erase_error_and_nothing_follows() {
    let (e, m) = boot_failure(FlashFault::EarlyAcceptDuringErase);
    assert!(matches!(e, FlashError::Erase { .. }), "got {e:?}");
    assert_eq!(e.code(), -105);
    assert_eq!(framed_types(&m.writes()), vec![0x02, 0x40, 0x42]);
    assert!(chunks(&m.writes()).is_empty());
    assert_left_in_bootloader(&e, &m);
}

#[test]
fn sim_no_accept_after_erase_is_an_erase_error_and_nothing_follows() {
    let (e, m) = boot_failure(FlashFault::NoAcceptAfterErase);
    assert!(matches!(e, FlashError::Erase { .. }), "got {e:?}");
    assert_eq!(e.code(), -105);
    assert!(chunks(&m.writes()).is_empty());
    assert_left_in_bootloader(&e, &m);
}

#[test]
fn sim_missing_sync_on_a_write_is_a_download_error_and_nothing_follows() {
    let (e, m) = boot_failure(FlashFault::MissingSync { write: 2 });
    assert!(matches!(e, FlashError::Download { .. }), "got {e:?}");
    assert_eq!(e.code(), -106);
    let writes = m.writes();
    assert_eq!(chunks(&writes).len(), 2, "no write after the unacked one");
    assert_eq!(
        writes.last().map(Vec::as_slice),
        Some(&payload(P + 16)[B..2 * B]),
        "the unacked write is the last thing sent"
    );
    assert_left_in_bootloader(&e, &m);
}

#[test]
fn sim_no_finished_is_a_verify_error_and_nothing_follows() {
    let (e, m) = boot_failure(FlashFault::NoFinished);
    assert!(matches!(e, FlashError::Verify { .. }), "got {e:?}");
    assert_eq!(e.code(), -103);
    assert_eq!(chunks(&m.writes()).len(), 4);
    assert!(
        m.writes().last().is_some_and(|w| w.len() > 68),
        "last is data"
    );
    assert_left_in_bootloader(&e, &m);
}

#[test]
fn sim_wrong_verify_reply_is_a_verify_error_and_nothing_follows() {
    let (e, m) = boot_failure(FlashFault::WrongVerifyReply);
    assert!(matches!(e, FlashError::Verify { .. }), "got {e:?}");
    assert_eq!(e.code(), -103);
    assert!(
        m.writes().last().is_some_and(|w| w.len() > 68),
        "last is data"
    );
    assert_left_in_bootloader(&e, &m);
}

#[test]
fn sim_ignored_jump_dfu_stops_with_the_back_button_fallback() {
    let bytes = container();
    let c = parse(&bytes);
    let (m, clock) = meter(DeviceMode::Application, Some(FlashFault::JumpDfuIgnored));
    let (r, _) = run(&m, &clock, &c);
    let e = r.expect_err("a meter that ignores JUMP_DFU stops the flash");
    assert!(matches!(e, FlashError::NotInBootloader { .. }), "got {e:?}");
    assert_recovery_text(&e);
    assert_eq!(m.mode(), DeviceMode::Application);
    let writes = m.writes();
    assert_eq!(
        writes.last().map(|w| w[0] & 0x7F),
        Some(0x09),
        "JUMP_DFU is the last thing sent"
    );
    assert!(!framed_types(&writes).contains(&0x42));
    assert!(chunks(&writes).is_empty());
}

#[test]
fn sim_ignored_jump_aprom_asks_for_a_power_cycle_not_an_error() {
    let bytes = container();
    let c = parse(&bytes);
    let (m, clock) = meter(DeviceMode::Bootloader, Some(FlashFault::JumpApromIgnored));
    let (r, _) = run(&m, &clock, &c);
    assert!(matches!(r, Ok(FlashOutcome::PowerCycleNeeded)), "got {r:?}");
    assert_eq!(m.mode(), DeviceMode::Bootloader);
    let types = framed_types(&m.writes());
    assert_eq!(
        types.iter().filter(|t| **t == 0x08).count(),
        1,
        "not retried"
    );
    assert_eq!(
        types.last(),
        Some(&0x08),
        "JUMP_APROM is the last thing sent"
    );
}

#[test]
fn sim_a_user_retry_from_the_bootloader_after_a_failure_succeeds() {
    let bytes = container();
    let c = parse(&bytes);
    let (m, clock) = meter(
        DeviceMode::Application,
        Some(FlashFault::MissingSync { write: 3 }),
    );
    let (r, _) = run(&m, &clock, &c);
    assert!(r.is_err());
    assert_eq!(m.mode(), DeviceMode::Bootloader);
    m.clear_flash_fault();
    let (r, events) = run(&m, &clock, &c);
    match r {
        Ok(FlashOutcome::Verified { version }) => assert_eq!(version, "2.0.7"),
        other => panic!("the retry flashes, got {other:?}"),
    }
    assert!(!events.iter().any(|e| matches!(e, FlashProgress::Switching)));
}

#[test]
fn sim_model_mismatch_sends_no_write_capable_frame() {
    let bytes = ContainerBuilder::new()
        .field(MODEL, "KM002C")
        .payload_sized(&payload(P))
        .build();
    let c = parse(&bytes);
    let (m, clock) = meter(DeviceMode::Application, None);
    let (r, _) = run(&m, &clock, &c);
    let e = r.expect_err("a KM002C file is refused on the simulated KM003C");
    assert!(matches!(e, FlashError::Product { .. }), "got {e:?}");
    let types = framed_types(&m.writes());
    assert!(
        types.iter().all(|t| matches!(t, 0x02 | 0x40 | 0x44)),
        "only read-only requests, got {types:02x?}"
    );
    assert!(chunks(&m.writes()).is_empty());
    assert_eq!(m.mode(), DeviceMode::Application);
}

// ---- AC-V9SK54.8 + .5: the version check after the return ----

#[test]
fn sim_an_image_that_does_not_take_is_a_verify_error_naming_both_versions() {
    let bytes = container();
    let c = parse(&bytes);
    let (m, clock) = meter(DeviceMode::Bootloader, Some(FlashFault::ImageNotTaken));
    let (r, _) = run(&m, &clock, &c);
    let e = match r {
        Err(e) => e,
        Ok(outcome) => panic!("an image that did not take is no success, got {outcome:?}"),
    };
    assert!(matches!(e, FlashError::Verify { .. }), "got {e:?}");
    assert_eq!(e.code(), -103);
    let msg = e.to_string();
    assert!(msg.contains("2.0.6"), "names the meter's version: {msg}");
    assert!(msg.contains("2.0.7"), "names the file's version: {msg}");
    assert!(
        msg.to_lowercase().contains("again") || msg.to_lowercase().contains("retry"),
        "says how to retry: {msg}"
    );
    assert_eq!(m.mode(), DeviceMode::Application);
    assert_eq!(m.app_version(), "2.0.6", "the sim kept the old version");
}

// ---- AC-V9SK54.8: more injected failures ----

#[test]
fn sim_a_head_reply_of_the_wrong_type_is_a_product_error_and_nothing_follows() {
    let (e, m) = boot_failure(FlashFault::HeadWrongType);
    assert!(matches!(e, FlashError::Product { .. }), "got {e:?}");
    assert_eq!(e.code(), -100);
    assert_eq!(framed_types(&m.writes()), vec![0x02, 0x40]);
    assert!(chunks(&m.writes()).is_empty());
    assert_left_in_bootloader(&e, &m);
}

#[test]
fn sim_a_payload_write_io_error_is_a_download_error_and_nothing_follows() {
    let (e, m) = boot_failure(FlashFault::WriteFails { write: 2 });
    match &e {
        FlashError::Download { write, .. } => assert_eq!(*write, 2),
        other => panic!("expected a download error, got {other:?}"),
    }
    assert_eq!(e.code(), -106);
    let writes = m.writes();
    assert_eq!(chunks(&writes).len(), 2, "no write after the failed one");
    assert_eq!(
        writes.last().map(Vec::as_slice),
        Some(&payload(P + 16)[B..2 * B]),
        "the failed write is the last thing sent"
    );
    assert_left_in_bootloader(&e, &m);
}

#[test]
fn sim_a_failing_jump_dfu_write_is_a_device_error_and_nothing_follows() {
    let bytes = container();
    let c = parse(&bytes);
    let (m, clock) = meter(DeviceMode::Application, Some(FlashFault::JumpDfuWriteFails));
    let (r, events) = run(&m, &clock, &c);
    let e = r.expect_err("a failed JUMP_DFU write stops the flash");
    assert!(matches!(e, FlashError::Device { .. }), "got {e:?}");
    assert_eq!(e.code(), -110);
    assert!(e.to_string().contains("JUMP_DFU"), "{e}");
    assert_eq!(m.mode(), DeviceMode::Application);
    let writes = m.writes();
    assert_eq!(
        writes.last().map(|w| w[0] & 0x7F),
        Some(0x09),
        "the failed JUMP_DFU is the last thing sent"
    );
    assert_eq!(
        framed_types(&writes).iter().filter(|t| **t == 0x09).count(),
        1,
        "JUMP_DFU is not retried"
    );
    assert!(!framed_types(&writes).contains(&0x42));
    assert!(chunks(&writes).is_empty());
    assert!(!events.iter().any(|e| matches!(e, FlashProgress::Erasing)));
}

#[test]
fn sim_a_failing_info_read_after_the_return_is_a_verify_error_and_nothing_follows() {
    let bytes = container();
    let c = parse(&bytes);
    let (m, clock) = meter(
        DeviceMode::Bootloader,
        Some(FlashFault::InfoReadFailsAfterReturn),
    );
    let (r, _) = run(&m, &clock, &c);
    let e = match r {
        Err(e) => e,
        Ok(outcome) => panic!("an unreadable info block is no success, got {outcome:?}"),
    };
    assert!(matches!(e, FlashError::Verify { .. }), "got {e:?}");
    assert_eq!(e.code(), -103);
    assert_eq!(m.mode(), DeviceMode::Application);
    let writes = m.writes();
    let aprom = writes
        .iter()
        .position(|w| w.len() == 4 && w[0] & 0x7F == 0x08)
        .expect("JUMP_APROM was sent");
    let after = framed_types(&writes[aprom + 1..]);
    assert!(
        after.iter().all(|t| matches!(t, 0x02 | 0x44)),
        "only the info read after the return, got {after:02x?}"
    );
    assert!(
        after.iter().filter(|t| **t == 0x44).count() <= 1,
        "the info read is not retried: {after:02x?}"
    );
    assert!(chunks(&writes[aprom + 1..]).is_empty());
    assert_eq!(
        framed_types(&writes).iter().filter(|t| **t == 0x08).count(),
        1
    );
}

// ---- AC-V9SK54.1 / .5 / .8 (review fix): a jump that resets mid-write ----

#[test]
fn sim_a_jump_dfu_that_disconnects_as_the_meter_resets_still_flashes() {
    let bytes = container();
    let c = parse(&bytes);
    let (m, clock) = meter(
        DeviceMode::Application,
        Some(FlashFault::JumpDfuDisconnects),
    );
    let (r, _) = run(&m, &clock, &c);
    match r {
        Ok(FlashOutcome::Verified { version }) => assert_eq!(version, "2.0.7"),
        other => panic!("the reset-as-it-jumps is not fatal, got {other:?}"),
    }
    assert_eq!(m.received_payload(), payload(P + 16));
    assert_eq!(m.mode(), DeviceMode::Application);
}

#[test]
fn sim_a_jump_aprom_that_disconnects_as_the_meter_resets_is_still_verified() {
    let bytes = container();
    let c = parse(&bytes);
    let (m, clock) = meter(
        DeviceMode::Bootloader,
        Some(FlashFault::JumpApromDisconnects),
    );
    let (r, _) = run(&m, &clock, &c);
    assert_eq!(
        r,
        Ok(FlashOutcome::Verified {
            version: "2.0.7".into()
        })
    );
    assert_eq!(m.mode(), DeviceMode::Application);
    assert_eq!(
        framed_types(&m.writes())
            .iter()
            .filter(|t| **t == 0x08)
            .count(),
        1,
        "JUMP_APROM is not retried"
    );
}
