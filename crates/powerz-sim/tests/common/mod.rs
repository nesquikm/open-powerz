//! Shared helpers for the `powerz-sim` tests (FR G0W08Z).
//!
//! Every identifier is synthetic: the simulator's made-up serial `123456`
//! and its made-up UID.

#![allow(dead_code, unused_imports)]

use std::time::Duration;

use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::frame::{self, Attribute, MainHeader};
use powerz_protocol::session::{Session, auth_plaintext, auth_request, memory_read_request};
use powerz_protocol::transport::Transport;
pub use powerz_sim::{FaultPlan, Persona, Scenario, SimClock, SimDevice};

pub const HW_INFO: (u32, u32) = (0x0000_0420, 64);
pub const FW_INFO: (u32, u32) = (0x0000_4420, 64);
pub const OTP: (u32, u32) = (0x0300_0C00, 64);
pub const UID: (u32, u32) = (0x4001_0450, 12);
pub const ALLOWED: [(u32, u32); 4] = [HW_INFO, FW_INFO, OTP, UID];

/// Simulated wall-clock start.
pub const T0: u64 = 1_790_000_000_000;
/// Reply timeout passed to raw transport calls.
pub const T: Duration = Duration::from_millis(1000);

pub fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

pub fn clock() -> SimClock {
    SimClock::new(T0)
}

pub fn device(persona: Persona, scenario: Scenario, clock: &SimClock) -> SimDevice {
    SimDevice::with_clock(persona, scenario, clock.clone())
}

pub type SimSession = Session<SimDevice, SimClock>;

/// Session over `dev` whose clock is `clock` (sleeps advance the sim time).
pub fn session(dev: SimDevice, clock: &SimClock) -> SimSession {
    Session::with_clock(dev, clock.clone())
}

/// Bulk session on `scenario`, CONNECT already sent.
pub fn connected(scenario: Scenario) -> (SimSession, SimClock) {
    let clock = clock();
    let mut s = session(device(Persona::Bulk, scenario, &clock), &clock);
    s.connect()
        .expect("CONNECT is accepted by the bulk persona");
    (s, clock)
}

/// Raw bulk device with the stale DISCONNECT and the CONNECT (id 1) consumed.
pub fn raw_connected(scenario: Scenario, clock: &SimClock) -> SimDevice {
    raw_connect(device(Persona::Bulk, scenario, clock))
}

/// Send CONNECT (id 1) on a freshly opened raw bulk device: the first read
/// is the stale DISCONNECT, the ACCEPT follows.
pub fn raw_connect(mut d: SimDevice) -> SimDevice {
    let stale = d.transact(&frame::connect(1), T).expect("stale reply");
    assert_eq!(stale.len(), 4, "stale DISCONNECT is 4 bytes: {stale:02x?}");
    assert_eq!(
        stale[0] & 0x7F,
        0x03,
        "first read is DISCONNECT: {stale:02x?}"
    );
    let accept = d.read(T).expect("ACCEPT follows");
    assert_eq!(
        &accept[..2],
        &[0x05, 0x01],
        "ACCEPT to CONNECT: {accept:02x?}"
    );
    d
}

/// Main-header auth level (bits 17..18).
pub fn permi(reply: &[u8]) -> u8 {
    MainHeader::decode(reply).unwrap().data().permi
}

/// Main-header type (bits 0..6).
pub fn ty(reply: &[u8]) -> u8 {
    reply[0] & 0x7F
}

/// `(att, payload)` of every section of a PUT_DATA reply.
pub fn sections(reply: &[u8]) -> Vec<(u16, Vec<u8>)> {
    frame::parse_put_data(reply)
        .unwrap()
        .into_iter()
        .map(|s| (s.att.bits(), s.payload.to_vec()))
        .collect()
}

pub fn get_data(id: u8, mask: Attribute) -> Vec<u8> {
    frame::get_data(id, mask).to_vec()
}

/// Raw memory read of `(addr, len)` with request id `id`: confirmation
/// checked, data decrypted with key 0 and truncated to `len`.
pub fn raw_memread(d: &mut SimDevice, id: u8, (addr, len): (u32, u32)) -> Vec<u8> {
    let confirm = d
        .transact(&memory_read_request(id, addr, len), T)
        .expect("confirmation");
    assert_eq!(confirm.len(), 20, "confirmation length: {confirm:02x?}");
    let want = (len as usize).div_ceil(16) * 16;
    let mut data = Vec::new();
    while data.len() < want {
        data.extend(d.read(T).expect("memory-read data"));
    }
    assert_eq!(data.len(), want);
    let mut plain = crypto::decrypt(&KEYS[0], &data);
    plain.truncate(len as usize);
    plain
}

/// Raw 0x4C auth with `credential`; returns the reply frame.
pub fn raw_auth(d: &mut SimDevice, id: u8, credential: &[u8]) -> Vec<u8> {
    let plain = auth_plaintext(T0, credential, 0x0102_0304_0506_0708);
    d.transact(&auth_request(id, &plain), T)
        .expect("auth reply")
}

/// The level-`level` credential read from the device itself.
pub fn credential(d: &mut SimDevice, level: u8) -> Vec<u8> {
    match level {
        1 => raw_memread(d, 0x60, UID),
        _ => {
            let mut otp = raw_memread(d, 0x61, OTP);
            otp.truncate(16);
            otp
        }
    }
}

/// Raw control frame `ty id att 00` (att already shifted into byte 2).
pub fn control(ty: u8, id: u8, b2: u8) -> Vec<u8> {
    vec![ty, id, b2, 0x00]
}

pub fn assert_near(what: &str, got: f64, want: f64, tol: f64) {
    assert!(
        (got - want).abs() <= tol,
        "{what}: got {got}, want {want} ± {tol}"
    );
}
