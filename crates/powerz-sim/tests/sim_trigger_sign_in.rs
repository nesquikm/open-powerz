//! AC-AXHK2P.12, the simulator side: a trigger session drops the meter's
//! bulk sign-in, as the f4abf09 hardware run saw it `[device]`. After a PDM
//! text session on the meter's serial port (`pdm open` … `pdm close`), the
//! bulk side's PUT_DATA reports auth level 0 and a memory read (0x44, the
//! sign-in's UID read) is REJECTed. On the meter a replug clears it; in the
//! simulator a CONNECT does (the app never sends one on its own for this).
//!
//! [`SimDevice::pdm_twin`] is the meter's PDM persona beside its bulk one,
//! the way [`SimDevice::cdc_twin`] is its CDC persona. Synthetic UID only.

mod common;

use std::io::{self, Read, Write};

use common::*;
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::session::Error;
use powerz_protocol::transport::Transport;
use powerz_sim::{SimCharger, SimPdm, SimPdmPort};

const REJECT: u8 = 0x06;
const MEM_READ: u8 = 0x44;

fn read_all(port: &mut SimPdmPort) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 64];
    loop {
        match port.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::TimedOut => break,
            Err(e) => panic!("read: {e}"),
        }
    }
    out
}

fn ask(port: &mut SimPdmPort, cmd: &str) -> Vec<u8> {
    port.write_all(cmd.as_bytes()).expect("write");
    read_all(port)
}

/// One trigger session on the PDM twin: open, enter PD, back to 5 V, close.
fn trigger_session(pdm: &SimPdm) {
    let mut port = pdm.open_port();
    ask(&mut port, "pdm open");
    ask(&mut port, "entry pd");
    ask(&mut port, "pd req=1");
    ask(&mut port, "pdm close");
}

/// A bulk meter with its PDM twin, connected and signed in at level 1.
fn signed_in_with_twin() -> (SimDevice, SimPdm) {
    let clock = clock();
    let dev = device(Persona::Bulk, Scenario::Idle, &clock);
    let pdm = dev.pdm_twin(SimCharger::Epr140W, clock.clone());
    let mut d = raw_connect(dev);
    let uid = credential(&mut d, 1);
    let reply = raw_auth(&mut d, 0x40, &uid);
    assert_eq!(permi(&reply), 1, "signed in at level 1: {reply:02x?}");
    let data = d.transact(&get_data(0x41, Attribute::ADC), T).unwrap();
    assert_eq!(permi(&data), 1, "PUT_DATA carries level 1: {data:02x?}");
    (d, pdm)
}

#[test]
fn a_trigger_session_drops_the_sign_in_level_put_data_reports() {
    let (mut d, pdm) = signed_in_with_twin();
    trigger_session(&pdm);
    let data = d.transact(&get_data(0x42, Attribute::ADC), T).unwrap();
    assert_eq!(ty(&data), 0x41, "still a PUT_DATA: {data:02x?}");
    assert_eq!(
        permi(&data),
        0,
        "a trigger session leaves no sign-in: {data:02x?}"
    );
}

#[test]
fn after_a_trigger_session_the_sign_in_read_is_rejected() {
    let (mut d, pdm) = signed_in_with_twin();
    trigger_session(&pdm);
    let reply = d
        .transact(
            &powerz_protocol::session::memory_read_request(0x43, UID.0, UID.1),
            T,
        )
        .unwrap();
    assert_eq!(ty(&reply), REJECT, "0x44 is refused: {reply:02x?}");
}

#[test]
fn after_a_trigger_session_the_capture_start_is_rejected() {
    let (mut d, pdm) = signed_in_with_twin();
    trigger_session(&pdm);
    let start = frame::start_pd_sniffer(0x44);
    let reply = d.transact(&start, T).unwrap();
    assert_eq!(ty(&reply), REJECT, "0x10 is refused: {reply:02x?}");
}

#[test]
fn a_connect_brings_the_sign_in_read_back() {
    let (mut d, pdm) = signed_in_with_twin();
    trigger_session(&pdm);
    let accept = d.transact(&frame::connect(0x45), T).unwrap();
    assert_eq!(ty(&accept), 0x05, "CONNECT accepted: {accept:02x?}");
    let uid = credential(&mut d, 1);
    let reply = raw_auth(&mut d, 0x46, &uid);
    assert_eq!(permi(&reply), 1, "signed in again: {reply:02x?}");
}

#[test]
fn a_bulk_meter_with_no_trigger_session_keeps_its_sign_in() {
    let (mut d, _pdm) = signed_in_with_twin();
    let data = d.transact(&get_data(0x47, Attribute::ADC), T).unwrap();
    assert_eq!(permi(&data), 1, "{data:02x?}");
    let reply = d
        .transact(
            &powerz_protocol::session::memory_read_request(0x48, UID.0, UID.1),
            T,
        )
        .unwrap();
    assert_ne!(ty(&reply), REJECT, "0x44 still served: {reply:02x?}");
}

/// Through the library session: after the trigger session the next
/// PUT_DATA sets the session's level to 0, so the capture start signs in
/// first and is refused at the sign-in read (0x44), not at 0x10.
#[test]
fn the_session_capture_start_is_refused_at_the_sign_in_read() {
    let clock = clock();
    let dev = device(Persona::Bulk, Scenario::Idle, &clock);
    let pdm = dev.pdm_twin(SimCharger::Epr140W, clock.clone());
    let mut s = session(dev, &clock);
    s.connect().expect("connect");
    assert_eq!(s.authenticate(1).expect("sign in"), 1);
    trigger_session(&pdm);
    s.read_adc().expect("the stream goes on");
    match s.start_pd_sniffer() {
        Err(Error::Rejected { request_type }) => assert_eq!(
            request_type & 0x7F,
            MEM_READ,
            "refused at the sign-in read, got {request_type:#04x}"
        ),
        other => panic!("expected a refused sign-in read, got {other:?}"),
    }
}
