//! Shared helpers for the `session_*` integration tests (FR N6EHQC).
//!
//! Real-device bytes are used only for the 0x420 and 0x4420 info blocks
//! (`tests/vectors/memread_*.hex`). The OTP block, the UID and every auth
//! exchange are synthetic: made-up values encrypted here with the crate's
//! shipped keys, the way the meter would answer.

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::frame::{DataHeader, SectionHeader};
use powerz_protocol::session::Clock;
use powerz_protocol::transport::mock::{MockTransport, Step};
use powerz_protocol::transport::{Error as TransportError, Transport};

pub const UID_ADDR: u32 = 0x4001_0450;
pub const OTP_ADDR: u32 = 0x0300_0C00;

/// Made-up serial number of the synthetic OTP block.
pub const SYNTH_SERIAL: &str = "123456";
/// Made-up MCU UID.
pub const SYNTH_UID: [u8; 12] = [
    0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB,
];
pub const SYNTH_UID_HEX: &str = "a0a1a2a3a4a5a6a7a8a9aaab";

/// Made-up 64-byte OTP block: serial, a 128-bit hex id, a time, a tag.
pub fn synth_otp() -> Vec<u8> {
    let mut b = b"123456 0f1e2d3c4b5a69788796a5b4c3d2e1f0 1700000000 T1".to_vec();
    b.resize(64, 0);
    b
}

pub fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Frames of a one-frame-per-line fixture (comment lines start with `#`).
pub fn frames(text: &str) -> Vec<Vec<u8>> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(hex)
        .collect()
}

/// Whole-frame fixture (all non-comment lines joined).
pub fn vector(text: &str) -> Vec<u8> {
    let joined: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect();
    hex(&joined)
}

pub const MEMREAD_0420: &str = include_str!("../vectors/memread_0420.hex");
pub const MEMREAD_4420: &str = include_str!("../vectors/memread_4420.hex");
pub const ADC_52: &str = include_str!("../vectors/adc_52.hex");
pub const SETTINGS_188: &str = include_str!("../vectors/settings_188.hex");
pub const ADC_SETTINGS_236: &str = include_str!("../vectors/adc_settings_236.hex");

/// `frame` with its transaction id (byte 1) replaced.
pub fn with_id(mut frame: Vec<u8>, id: u8) -> Vec<u8> {
    frame[1] = id;
    frame
}

/// Memory-read plaintext built independently of the crate: addr, len,
/// 0xFFFFFFFF, CRC-32 of those 12 bytes, 0xFF padding to 32 bytes.
pub fn memread_plain(addr: u32, len: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend(addr.to_le_bytes());
    p.extend(len.to_le_bytes());
    p.extend(0xFFFF_FFFFu32.to_le_bytes());
    let crc = crc32fast::hash(&p);
    p.extend(crc.to_le_bytes());
    p.extend([0xFF; 16]);
    p
}

/// Memory-read request frame `44 id 01 01` + AES(key 0) of the plaintext.
pub fn memread_request(id: u8, addr: u32, len: u32) -> Vec<u8> {
    let mut f = vec![0x44, id, 0x01, 0x01];
    f.extend(crypto::encrypt(&KEYS[0], &memread_plain(addr, len)));
    f
}

/// Confirmation `c4 id 01 01` + the first 16 plaintext bytes.
pub fn memread_confirm(id: u8, addr: u32, len: u32) -> Vec<u8> {
    let mut f = vec![0xC4, id, 0x01, 0x01];
    f.extend(&memread_plain(addr, len)[..16]);
    f
}

/// Scripted synthetic memory read of `block` (encrypted here with key 0),
/// data in a single transfer.
pub fn memread_steps(id: u8, addr: u32, block: &[u8]) -> Vec<Step> {
    let len = block.len() as u32;
    vec![
        Step::transact(
            memread_request(id, addr, len),
            memread_confirm(id, addr, len),
        ),
        Step::read(crypto::encrypt(&KEYS[0], block)),
    ]
}

/// Scripted replay of a captured info-block read (request id from the
/// capture is kept as is).
pub fn captured_memread_steps(fixture: &str) -> Vec<Step> {
    let f = frames(fixture);
    vec![
        Step::transact(f[0].clone(), f[1].clone()),
        Step::read(f[2].clone()),
    ]
}

/// PUT_DATA main header with `id` and auth level `level`.
pub fn put_header(id: u8, level: u8) -> [u8; 4] {
    DataHeader {
        ty: 0x41,
        extend: false,
        id,
        encode: false,
        permi: level,
        obj: 0,
    }
    .to_header()
    .encode()
}

/// One 20-byte queue record.
pub fn record(time_ms: u32, vbus_uv: i32, ibus_ua: i32, cc1: u16) -> Vec<u8> {
    let mut r = Vec::new();
    r.extend(time_ms.to_le_bytes());
    r.extend(vbus_uv.to_le_bytes());
    r.extend(ibus_ua.to_le_bytes());
    r.extend(cc1.to_le_bytes());
    r.extend(7u16.to_le_bytes()); // CC2
    r.extend(8u16.to_le_bytes()); // D+
    r.extend(9u16.to_le_bytes()); // D-
    r
}

/// PUT_DATA reply holding one ADC_QUEUE section with records at `times`
/// (VBUS = 5 V + time µV, IBUS = -time µA, CC1 = 805).
pub fn queue_reply(id: u8, level: u8, times: &[u32]) -> Vec<u8> {
    let mut f = put_header(id, level).to_vec();
    f.extend(
        SectionHeader {
            att: 0x002,
            next: false,
            chunk: times.len() as u8,
            size: 20,
        }
        .encode(),
    );
    for &t in times {
        f.extend(record(t, 5_000_000 + t as i32, -(t as i32), 805));
    }
    f
}

pub fn get_data(id: u8, mask: u16) -> Vec<u8> {
    let [lo, hi] = (mask << 1).to_le_bytes();
    vec![0x0C, id, lo, hi]
}

pub fn accept(id: u8) -> Vec<u8> {
    vec![0x05, id, 0, 0]
}

pub fn reject(id: u8) -> Vec<u8> {
    vec![0x06, id, 0, 0]
}

pub fn finished(id: u8) -> Vec<u8> {
    vec![0x07, id, 0, 0]
}

/// Deterministic [`Clock`]: time moves only when the session sleeps; random
/// values come from a counter. Clones share state.
#[derive(Debug, Clone)]
pub struct FakeClock(pub Rc<RefCell<ClockState>>);

#[derive(Debug, Default)]
pub struct ClockState {
    pub now_ms: u64,
    pub next_random: u64,
    pub slept: Vec<Duration>,
    /// Every value handed out by `random_u64`.
    pub randoms: Vec<u64>,
}

impl FakeClock {
    pub fn new(now_ms: u64) -> FakeClock {
        FakeClock(Rc::new(RefCell::new(ClockState {
            now_ms,
            next_random: 0x0123_4567_89AB_CDEF,
            slept: Vec::new(),
            randoms: Vec::new(),
        })))
    }

    pub fn total_slept(&self) -> Duration {
        self.0.borrow().slept.iter().sum()
    }
}

impl Clock for FakeClock {
    fn now_ms(&mut self) -> u64 {
        self.0.borrow().now_ms
    }
    fn random_u64(&mut self) -> u64 {
        let mut s = self.0.borrow_mut();
        s.next_random = s.next_random.wrapping_add(0x1111);
        let r = s.next_random;
        s.randoms.push(r);
        r
    }
    fn sleep(&mut self, d: Duration) {
        let mut s = self.0.borrow_mut();
        s.now_ms += d.as_millis() as u64;
        s.slept.push(d);
    }
}

/// How the simulated meter answers 0x4C requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// Echo the plaintext under the reply key, header with the given level.
    Echo,
    /// Echo, but corrupt the random value.
    WrongRandom,
    /// Echo, but corrupt the time value.
    WrongTime,
    /// Encrypt the echo under plain key 3 instead of the reply variant.
    WrongKey,
    /// Answer REJECT.
    Reject,
    /// Give no reply (timeout), as over HID.
    Timeout,
}

/// Key-3 variant the meter uses for its auth reply: byte 1 is ASCII `X`.
pub fn reply_key() -> [u8; 16] {
    let mut k = KEYS[3];
    k[1] = b'X';
    k
}

/// A [`MockTransport`] for everything except 0x4C, which is answered the way
/// the meter does: the decrypted plaintext comes back under the reply key in
/// a `4C 00 xx 02` frame (id 0, bit 16 set, level in bits 17..18).
#[derive(Debug)]
pub struct AuthDevice {
    pub inner: MockTransport,
    pub mode: AuthMode,
    /// Level written into the reply header.
    pub level: u8,
    /// Raw 0x4C request frames, in order.
    pub auth_frames: Vec<Vec<u8>>,
    /// Decrypted (key 3) plaintexts of those frames.
    pub auth_plaintexts: Vec<Vec<u8>>,
}

impl AuthDevice {
    pub fn new(script: Vec<Step>, level: u8) -> AuthDevice {
        AuthDevice {
            inner: MockTransport::new(script),
            mode: AuthMode::Echo,
            level,
            auth_frames: Vec::new(),
            auth_plaintexts: Vec::new(),
        }
    }

    pub fn with_mode(mut self, mode: AuthMode) -> AuthDevice {
        self.mode = mode;
        self
    }
}

impl Transport for AuthDevice {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        if request.first() != Some(&0x4C) {
            return self.inner.transact(request, timeout);
        }
        self.auth_frames.push(request.to_vec());
        assert_eq!(request.len(), 36, "auth frame must be 4 + 32 bytes");
        let plain = crypto::decrypt(&KEYS[3], &request[4..]);
        self.auth_plaintexts.push(plain.clone());
        let mut echo = plain;
        let key = match self.mode {
            AuthMode::Reject => return Ok(vec![0x06, request[1], 0, 0]),
            AuthMode::Timeout => return Err(TransportError::Timeout),
            AuthMode::WrongKey => KEYS[3],
            _ => reply_key(),
        };
        let rnd_off = echo.len() - 8 - echo.iter().rev().take_while(|&&b| b == 0xFF).count();
        match self.mode {
            AuthMode::WrongRandom => echo[rnd_off] ^= 0x01,
            AuthMode::WrongTime => echo[0] ^= 0x01,
            _ => {}
        }
        let word: u32 = 0x4C | (1 << 16) | (u32::from(self.level) << 17) | (8 << 22);
        let mut reply = word.to_le_bytes().to_vec();
        reply.extend(crypto::encrypt(&key, &echo));
        Ok(reply)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.inner.read(timeout)
    }
}
