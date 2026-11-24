//! FR XW4QQN over the session API the fix adds:
//!
//! - AC .2: [`Session::expect_own_reply_ids`] switches a session to by-type
//!   reply matching; a reply of another type is read past at most
//!   `MAX_STALE_REPLIES` (3) times.
//! - AC .3: under that policy the first CONNECT waits 2 s, is not followed
//!   by a recovery, and is sent once more when unanswered.
//! - AC .4: [`ProbeReport`] and [`VariantOutcome`] carry an [`IdMismatch`]
//!   for a CONNECT or HEAD answered with an id other than ours, and `None`
//!   when the ids match.
//!
//! Field bytes from one trace; every other value is synthetic.

mod common;
#[path = "support/xw4qqn_pipes.rs"]
mod pipes;

use std::time::Duration;

use common::FakeClock;
use pipes::*;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::session::{IdMismatch, ProbeReport, ProbeVariant, Session};
use powerz_protocol::transport::Error as TransportError;

const T0: u64 = 1_790_000_000_000;

fn ok(b: Vec<u8>) -> Result<Vec<u8>, TransportError> {
    Ok(b)
}

fn session(pipe: Scripted) -> Session<Scripted, FakeClock> {
    Session::with_clock(pipe, FakeClock::new(T0))
}

fn take(s: &mut Session<Scripted, FakeClock>) -> Scripted {
    std::mem::replace(s.transport_mut(), Scripted::silent())
}

fn probe(pipe: Scripted, mode: DeviceMode) -> ProbeReport {
    session(pipe)
        .probe_with_variants(mode)
        .unwrap_or_else(|e| panic!("probe failed: {e}"))
        .0
}

// ---------------------------------------------------------------- AC-XW4QQN.2

#[test]
fn xw4qqn_own_reply_ids_connect_takes_an_accept_of_any_id() {
    let mut s = session(Scripted::new(vec![ok(field_accept(0x42))], vec![]));
    s.expect_own_reply_ids();
    assert_eq!(s.connect(), Ok(()));
}

#[test]
fn xw4qqn_own_reply_ids_connect_reads_past_another_type_with_our_id() {
    let mut s = session(Scripted::new(
        vec![ok(vec![0x41, 0x01, 0, 0])],
        vec![ok(field_accept(0x00))],
    ));
    s.expect_own_reply_ids();
    assert_eq!(s.connect(), Ok(()));
}

#[test]
fn xw4qqn_own_reply_ids_reads_past_at_most_three_replies_of_another_type() {
    let mut s = session(Scripted::new(
        vec![ok(vec![0x41, 0x01, 0, 0])],
        vec![
            ok(vec![0x41, 0x02, 0, 0]),
            ok(vec![0x41, 0x03, 0, 0]),
            ok(vec![0x41, 0x04, 0, 0]),
            ok(field_accept(0x05)),
        ],
    ));
    s.expect_own_reply_ids();
    let _ = s.connect();
    let pipe = take(&mut s);
    let first_connect: Vec<&Ev> = pipe
        .events
        .iter()
        .take_while(|e| !matches!(e, Ev::Transact(r, _) if r[1] != 0x01))
        .collect();
    let reads = first_connect
        .iter()
        .filter(|e| matches!(e, Ev::Read(_)))
        .count();
    assert_eq!(reads, 3, "{:?}", pipe.events);
}

#[test]
fn xw4qqn_strict_session_connect_still_needs_our_id() {
    // Default policy: an ACCEPT with id 0 does not answer CONNECT id 1.
    let mut s = session(Scripted::new(
        vec![ok(field_accept(0x00)), ok(field_accept(0x00))],
        vec![],
    ));
    assert!(s.connect().is_err());
}

// ---------------------------------------------------------------- AC-XW4QQN.3

#[test]
fn xw4qqn_own_reply_ids_first_connect_waits_the_full_connect_timeout() {
    let mut s = session(Scripted::new(vec![ok(field_accept(0x00))], vec![]));
    s.expect_own_reply_ids();
    let _ = s.connect();
    let pipe = take(&mut s);
    assert_eq!(
        pipe.transact_timeouts(),
        vec![Duration::from_millis(2000)],
        "{:?}",
        pipe.events
    );
}

#[test]
fn xw4qqn_own_reply_ids_unanswered_connect_is_sent_once_more_without_recovery() {
    let mut s = session(Scripted::silent().with_recover_ok(false));
    s.expect_own_reply_ids();
    assert_eq!(
        s.connect(),
        Err(powerz_protocol::session::Error::Transport(
            TransportError::Timeout
        ))
    );
    let pipe = take(&mut s);
    assert_eq!(
        pipe.transacted(),
        vec![connect_req(1), connect_req(2)],
        "{:?}",
        pipe.events
    );
    assert_eq!(pipe.recovers(), 0, "{:?}", pipe.events);
}

#[test]
fn xw4qqn_own_reply_ids_answered_retry_is_taken() {
    let mut s = session(Scripted::new(
        vec![Err(TransportError::Timeout), ok(field_accept(0x00))],
        vec![],
    ));
    s.expect_own_reply_ids();
    assert_eq!(s.connect(), Ok(()));
}

// ---------------------------------------------------------------- AC-XW4QQN.4

#[test]
fn xw4qqn_probe_report_gives_the_connect_id_mismatch() {
    let r = probe(
        Scripted::new(vec![ok(field_accept(0x00)), ok(field_head(0x01))], vec![]),
        DeviceMode::LegacyBootloader,
    );
    assert_eq!(r.connect_ids, Some(IdMismatch { meter: 0, ours: 1 }));
}

#[test]
fn xw4qqn_probe_report_gives_the_head_id_mismatch() {
    let r = probe(
        Scripted::new(vec![ok(field_accept(0x00)), ok(field_head(0x01))], vec![]),
        DeviceMode::LegacyBootloader,
    );
    assert_eq!(r.head_ids, Some(IdMismatch { meter: 1, ours: 2 }));
}

#[test]
fn xw4qqn_probe_report_has_no_id_mismatch_when_the_ids_match() {
    // A 2.5 bootloader echoing our ids: CONNECT 1, HEAD 2.
    let mut head = field_head(0x02);
    head[0] = 0x40;
    let r = probe(
        Scripted::new(vec![ok(field_accept(0x01)), ok(head)], vec![]),
        DeviceMode::Bootloader,
    );
    assert_eq!((r.connect_ids, r.head_ids), (None, None), "{r:?}");
}

#[test]
fn xw4qqn_variant_outcome_gives_its_id_mismatches() {
    // Strict 2.5 path, silent: CONNECT 1, recovery, CONNECT 2, HEAD 3; the
    // first variant then sends CONNECT 4 and HEAD 5.
    let r = probe(
        Scripted::silent().with_variants(vec![ok(field_accept(0x00)), ok(field_head(0x01))]),
        DeviceMode::Bootloader,
    );
    let v = r
        .variants
        .iter()
        .find(|v| v.variant == ProbeVariant::NoDrainNoClearHalt)
        .expect("first variant ran");
    assert_eq!(
        (v.connect_ids, v.head_ids),
        (
            Some(IdMismatch { meter: 0, ours: 4 }),
            Some(IdMismatch { meter: 1, ours: 5 })
        )
    );
}

#[test]
fn xw4qqn_variant_outcome_has_no_id_mismatch_when_the_ids_match() {
    let mut head = field_head(0x05);
    head[0] = 0x40;
    let r = probe(
        Scripted::silent().with_variants(vec![ok(field_accept(0x04)), ok(head)]),
        DeviceMode::Bootloader,
    );
    let v = &r.variants[0];
    assert_eq!((v.connect_ids, v.head_ids), (None, None), "{v:?}");
}
