//! Shared helpers for the powerz-wasm parity tests (FR QFTCWY).
//!
//! The native side of every parity check is a `powerz_protocol::session::Session`
//! over a [`Recorder`]: a transport that forwards to the simulator and keeps
//! every request with the reply transfers it got, so a test can compare the
//! binding's builders and decoders against exactly what the session sent and
//! read. Recorded reply vectors are the protocol crate's own fixtures
//! (`crates/powerz-protocol/tests/vectors`, no device identifiers).

#![allow(dead_code)]

use std::path::Path;
use std::time::Duration;

use powerz_protocol::session::{Clock, Session};
use powerz_protocol::transport::{Error as TransportError, Transport};
use powerz_sim::{PdScenario, Persona, Scenario, SimClock, SimDevice};

/// Fixed wall-clock time of the injected session clock, ms since the epoch.
pub const NOW_MS: u64 = 1_790_000_000_000;
/// Fixed auth random value of the injected session clock.
pub const RANDOM: u64 = 0x0123_4567_89AB_CDEF;

pub const UID: (u32, u32) = (0x4001_0450, 12);
pub const OTP: (u32, u32) = (0x0300_0C00, 64);
pub const HARDWARE_INFO: (u32, u32) = (0x0000_0420, 64);
pub const FIRMWARE_INFO: (u32, u32) = (0x0000_4420, 64);

/// A session clock that always reads [`NOW_MS`] and draws [`RANDOM`].
#[derive(Debug, Clone, Default)]
pub struct FixedClock;

impl Clock for FixedClock {
    fn now_ms(&mut self) -> u64 {
        NOW_MS
    }
    fn random_u64(&mut self) -> u64 {
        RANDOM
    }
    fn sleep(&mut self, _duration: Duration) {}
}

/// One request the session sent, with the reply transfers it read.
#[derive(Debug, Clone)]
pub struct Exchange {
    pub request: Vec<u8>,
    pub replies: Vec<Vec<u8>>,
}

impl Exchange {
    /// All reply transfers joined, as the demo meter's `transact` returns them.
    pub fn reply(&self) -> Vec<u8> {
        self.replies.concat()
    }
}

/// Transport that forwards to the simulator and records every exchange.
#[derive(Debug)]
pub struct Recorder {
    pub sim: SimDevice,
    pub log: Vec<Exchange>,
}

impl Transport for Recorder {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        let reply = self.sim.transact(request, timeout);
        self.log.push(Exchange {
            request: request.to_vec(),
            replies: reply.iter().cloned().collect(),
        });
        reply
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        let reply = self.sim.read(timeout);
        if let (Ok(r), Some(last)) = (&reply, self.log.last_mut()) {
            last.replies.push(r.clone());
        }
        reply
    }
}

/// The desktop app's demo meter: the bulk persona with the PD capture, on a
/// manual clock the test advances.
pub fn demo_sim(clock: &SimClock) -> SimDevice {
    SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, clock.clone())
        .with_pd_scenario(PdScenario::Negotiation)
}

/// A native session over the recorded demo sim; returns the sim clock too.
pub fn native_session() -> (Session<Recorder, FixedClock>, SimClock) {
    let clock = SimClock::new(0);
    let recorder = Recorder {
        sim: demo_sim(&clock),
        log: Vec::new(),
    };
    (Session::with_clock(recorder, FixedClock), clock)
}

/// The recorded exchanges whose request type (byte 0, bit 7 masked) is `ty`.
pub fn exchanges_of(session: &Session<Recorder, FixedClock>, ty: u8) -> Vec<Exchange> {
    session
        .transport()
        .log
        .iter()
        .filter(|e| e.request.first().map(|b| b & 0x7F) == Some(ty))
        .cloned()
        .collect()
}

/// The last recorded exchange whose request type is `ty`.
pub fn last_of(session: &Session<Recorder, FixedClock>, ty: u8) -> Exchange {
    exchanges_of(session, ty)
        .pop()
        .unwrap_or_else(|| panic!("the session sent no request of type {ty:#04x}"))
}

/// Request id (byte 1) of a recorded request.
pub fn id_of(e: &Exchange) -> u8 {
    e.request[1]
}

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn vector_text(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../powerz-protocol/tests/vectors")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// A whole-frame recorded vector (comment lines start with `#`).
pub fn vector(name: &str) -> Vec<u8> {
    let joined: String = vector_text(name)
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect();
    hex(&joined)
}

/// A one-frame-per-line recorded vector.
pub fn frames(name: &str) -> Vec<Vec<u8>> {
    vector_text(name)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(hex)
        .collect()
}

/// `value` serialized the way the binding hands it to JS (serde field names).
pub fn json<T: serde::Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).expect("binding values serialize")
}

/// `v[key]` as f64, panicking with the key name when absent.
pub fn f(v: &serde_json::Value, key: &str) -> f64 {
    v[key]
        .as_f64()
        .unwrap_or_else(|| panic!("{key} is a number in {v}"))
}

pub fn assert_close(got: f64, want: f64, what: &str) {
    assert!(
        (got - want).abs() < 1e-9,
        "{what}: binding {got} vs native {want}"
    );
}
