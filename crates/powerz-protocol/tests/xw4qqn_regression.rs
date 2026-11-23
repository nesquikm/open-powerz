//! FR XW4QQN, the bug cases over the API that exists before the fix:
//!
//! - AC .1 (regression): on a field-fitted 2.1 bootloader (own reply ids,
//!   late first CONNECT, failing pipe recovery) the probe in old-bootloader
//!   mode gets CONNECT accepted and HEAD field 0 from the standard path
//!   alone and runs no variant.
//! - AC .2: in old-bootloader mode a reply answers by its type, whatever its
//!   id; a reply of another type is read past. Application and 2.5 keep the
//!   exact id match.
//! - AC .3: in old-bootloader mode the first CONNECT waits 2 s, there is no
//!   recovery, and an unanswered CONNECT is sent once more; application and
//!   2.5 keep 500 ms, the recovery and the retry.
//! - AC .6: a string descriptor prints cut at its first NUL.
//!
//! Field bytes from one trace; every other value is synthetic.

mod common;
#[path = "support/xw4qqn_pipes.rs"]
mod pipes;

use std::time::Duration;

use common::FakeClock;
use pipes::*;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::session::{ConnectOutcome, HeadOutcome, ProbeReport, Session};
use powerz_protocol::transport::Error as TransportError;
use powerz_protocol::transport::discovery::{
    StringDescriptor, UsbDetails, UsbIdentity, UsbInterface,
};

const T0: u64 = 1_790_000_000_000;

fn ok(b: Vec<u8>) -> Result<Vec<u8>, TransportError> {
    Ok(b)
}

fn probe_scripted(pipe: Scripted, mode: DeviceMode) -> (Result<ProbeReport, String>, Scripted) {
    let mut s = Session::with_clock(pipe, FakeClock::new(T0));
    let r = s
        .probe_with_variants(mode)
        .map(|(r, _)| r)
        .map_err(|e| e.to_string());
    let pipe = std::mem::replace(s.transport_mut(), Scripted::silent());
    (r, pipe)
}

// ---------------------------------------------------------------- AC-XW4QQN.1

fn field_probe() -> (ProbeReport, FieldMeter) {
    let mut s = Session::with_clock(FieldMeter::new(), FakeClock::new(T0));
    let (report, _) = s
        .probe_with_variants(DeviceMode::LegacyBootloader)
        .unwrap_or_else(|e| panic!("probe failed: {e}"));
    let meter = std::mem::take(s.transport_mut());
    (report, meter)
}

#[test]
fn xw4qqn_field_old_bootloader_connect_is_accepted_on_the_standard_path() {
    let (report, _) = field_probe();
    assert_eq!(report.connect, ConnectOutcome::Accepted, "{report:?}");
}

#[test]
fn xw4qqn_field_old_bootloader_head_gives_field_0_on_the_standard_path() {
    let (report, _) = field_probe();
    assert_eq!(report.head, HeadOutcome::Field(0), "{report:?}");
}

#[test]
fn xw4qqn_field_old_bootloader_runs_no_variant() {
    let (report, meter) = field_probe();
    assert!(report.variants.is_empty(), "{report:?}");
    assert!(
        !meter.events.iter().any(|e| matches!(e, Ev::Variant(..))),
        "a variant was sent: {:?}",
        meter.events
    );
}

// ---------------------------------------------------------------- AC-XW4QQN.2

#[test]
fn xw4qqn_old_bootloader_takes_an_accept_of_any_id_for_connect() {
    let pipe = Scripted::new(vec![ok(field_accept(0x37)), ok(field_head(0x38))], vec![]);
    let (r, _) = probe_scripted(pipe, DeviceMode::LegacyBootloader);
    assert!(
        matches!(&r, Ok(rep) if rep.connect == ConnectOutcome::Accepted),
        "{r:?}"
    );
}

#[test]
fn xw4qqn_old_bootloader_takes_a_reject_of_any_id_for_connect() {
    let pipe = Scripted::new(
        vec![ok(vec![0x06, 0x37, 0, 0]), ok(field_head(0x38))],
        vec![],
    );
    let (r, _) = probe_scripted(pipe, DeviceMode::LegacyBootloader);
    assert!(
        matches!(&r, Ok(rep) if rep.connect == ConnectOutcome::Rejected),
        "{r:?}"
    );
}

#[test]
fn xw4qqn_old_bootloader_takes_a_head_of_any_id_for_head() {
    let pipe = Scripted::new(vec![ok(field_accept(0x00)), ok(field_head(0x01))], vec![]);
    let (r, _) = probe_scripted(pipe, DeviceMode::LegacyBootloader);
    assert!(
        matches!(&r, Ok(rep) if rep.head == HeadOutcome::Field(0)),
        "{r:?}"
    );
}

#[test]
fn xw4qqn_old_bootloader_takes_a_reject_of_any_id_for_head() {
    let pipe = Scripted::new(
        vec![ok(field_accept(0x00)), ok(vec![0x06, 0x09, 0, 0])],
        vec![],
    );
    let (r, _) = probe_scripted(pipe, DeviceMode::LegacyBootloader);
    assert!(
        matches!(&r, Ok(rep) if rep.head == HeadOutcome::Rejected(vec![0x06, 0x09, 0, 0])),
        "{r:?}"
    );
}

#[test]
fn xw4qqn_old_bootloader_reads_past_a_reply_of_another_type_for_connect() {
    // A PUT_DATA (0x41) carrying the request id is not an answer to
    // CONNECT; the ACCEPT read after it is.
    let pipe = Scripted::new(
        vec![ok(vec![0x41, 0x01, 0, 0]), ok(field_head(0x01))],
        vec![ok(field_accept(0x00))],
    );
    let (r, _) = probe_scripted(pipe, DeviceMode::LegacyBootloader);
    assert!(
        matches!(&r, Ok(rep) if rep.connect == ConnectOutcome::Accepted),
        "{r:?}"
    );
}

#[test]
fn xw4qqn_old_bootloader_reads_past_a_stray_accept_for_head() {
    // The late ACCEPT of the field trace lands on the HEAD read; the HEAD
    // reply read after it is the answer.
    let pipe = Scripted::new(
        vec![ok(field_accept(0x00)), ok(field_accept(0x00))],
        vec![ok(field_head(0x01))],
    );
    let (r, _) = probe_scripted(pipe, DeviceMode::LegacyBootloader);
    assert!(
        matches!(&r, Ok(rep) if rep.head == HeadOutcome::Field(0)),
        "{r:?}"
    );
}

#[test]
fn xw4qqn_bootloader_2_5_still_drops_an_accept_with_another_id() {
    // Strict ids: the ACCEPT with id 0 does not answer CONNECT id 1, nor
    // (after the recovery) CONNECT id 2.
    let pipe = Scripted::new(vec![ok(field_accept(0x00)), ok(field_accept(0x00))], vec![]);
    let (r, _) = probe_scripted(pipe, DeviceMode::Bootloader);
    assert!(
        matches!(&r, Ok(rep) if rep.connect == ConnectOutcome::TimedOut),
        "{r:?}"
    );
}

#[test]
fn xw4qqn_application_still_drops_an_accept_with_another_id() {
    let pipe = Scripted::new(vec![ok(field_accept(0x00)), ok(field_accept(0x00))], vec![]);
    let (r, _) = probe_scripted(pipe, DeviceMode::Application);
    assert!(
        matches!(&r, Ok(rep) if rep.connect == ConnectOutcome::TimedOut),
        "{r:?}"
    );
}

// ---------------------------------------------------------------- AC-XW4QQN.3

#[test]
fn xw4qqn_old_bootloader_first_connect_waits_2_s() {
    let (_, pipe) = probe_scripted(
        Scripted::silent().with_recover_ok(false),
        DeviceMode::LegacyBootloader,
    );
    assert_eq!(
        pipe.transact_timeouts().first().copied(),
        Some(Duration::from_millis(2000)),
        "{:?}",
        pipe.events
    );
}

#[test]
fn xw4qqn_old_bootloader_sends_no_pipe_recovery() {
    let (_, pipe) = probe_scripted(Scripted::silent(), DeviceMode::LegacyBootloader);
    assert_eq!(pipe.recovers(), 0, "{:?}", pipe.events);
}

#[test]
fn xw4qqn_old_bootloader_retries_an_unanswered_connect_once() {
    // The field pipe's recovery fails; the retry must not depend on it.
    let (_, pipe) = probe_scripted(
        Scripted::silent().with_recover_ok(false),
        DeviceMode::LegacyBootloader,
    );
    let sent = pipe.transacted();
    assert_eq!(
        sent.first().cloned(),
        Some(connect_req(1)),
        "{:?}",
        pipe.events
    );
    assert_eq!(
        sent.get(1).cloned(),
        Some(connect_req(2)),
        "{:?}",
        pipe.events
    );
    assert_eq!(sent.get(2).cloned(), Some(head_req(3)), "{:?}", pipe.events);
}

#[test]
fn xw4qqn_bootloader_2_5_keeps_the_500_ms_wait_the_recovery_and_the_retry() {
    let (_, pipe) = probe_scripted(Scripted::silent(), DeviceMode::Bootloader);
    assert_eq!(
        pipe.events.get(..3),
        Some(
            &[
                Ev::Transact(connect_req(1), Duration::from_millis(500)),
                Ev::Recover,
                Ev::Transact(connect_req(2), Duration::from_millis(2000)),
            ][..]
        ),
        "{:?}",
        pipe.events
    );
}

#[test]
fn xw4qqn_application_keeps_the_500_ms_wait_the_recovery_and_the_retry() {
    let (_, pipe) = probe_scripted(Scripted::silent(), DeviceMode::Application);
    assert_eq!(
        pipe.events.get(..3),
        Some(
            &[
                Ev::Transact(connect_req(1), Duration::from_millis(500)),
                Ev::Recover,
                Ev::Transact(connect_req(2), Duration::from_millis(2000)),
            ][..]
        ),
        "{:?}",
        pipe.events
    );
}

#[test]
fn xw4qqn_bootloader_2_5_does_not_retry_when_the_recovery_fails() {
    let (_, pipe) = probe_scripted(
        Scripted::silent().with_recover_ok(false),
        DeviceMode::Bootloader,
    );
    let sent = pipe.transacted();
    assert_eq!(sent.get(..2), Some(&[connect_req(1), head_req(2)][..]));
}

// ---------------------------------------------------------------- AC-XW4QQN.6

#[test]
fn xw4qqn_string_descriptor_prints_cut_at_the_first_nul() {
    let identity = UsbIdentity {
        vid: 0x5FC9,
        pid: 0x0063,
        product: Some("POWER-Z KM003C".to_owned()),
        interfaces: vec![UsbInterface {
            number: 0,
            class: 0xFF,
            subclass: 0xFF,
            protocol: 0,
            string: Some("POWER-Z KM003C".to_owned()),
        }],
    };
    let details = UsbDetails {
        interfaces: Vec::new(),
        strings: vec![(2, StringDescriptor::Text("BOOT\0".to_owned()))],
    };
    let mut text = String::new();
    identity
        .write_text(&mut text, Some(&details))
        .expect("text writes");
    let line = text
        .lines()
        .find(|l| l.starts_with("string 2: "))
        .unwrap_or_else(|| panic!("no string 2 line in:\n{text}"));
    assert_eq!(line, "string 2: \"BOOT\"", "{text}");
}
