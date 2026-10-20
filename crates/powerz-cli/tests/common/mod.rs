//! Helpers for the `powerz-cli` command-function tests (FR STQEA1).
//!
//! Every reply here is synthetic: a made-up serial (`123456`), UID, model
//! and version strings, encrypted with the protocol crate's shipped keys the
//! way the meter would answer. No real-device serial or UID is used.

#![allow(dead_code)]

use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::frame::{DataHeader, SectionHeader};
use powerz_protocol::session::{Clock, Session, memory_read_plaintext, memory_read_request};
use powerz_protocol::transport::discovery::{DeviceEntry, Model};
use powerz_protocol::transport::mock::{MockTransport, Step};
use powerz_protocol::transport::{Error as TransportError, Transport};

pub const HW_INFO_ADDR: u32 = 0x0000_0420;
pub const FW_INFO_ADDR: u32 = 0x0000_4420;
pub const OTP_ADDR: u32 = 0x0300_0C00;
pub const UID_ADDR: u32 = 0x4001_0450;

/// Made-up serial number.
pub const SYNTH_SERIAL: &str = "123456";
/// Made-up MCU UID.
pub const SYNTH_UID: [u8; 12] = [
    0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB,
];
pub const SYNTH_UID_HEX: &str = "a0a1a2a3a4a5a6a7a8a9aaab";
pub const SYNTH_MODEL: &str = "KM003C";
pub const SYNTH_FW: &str = "9.8.7";
pub const SYNTH_HW: &str = "4.3";

// ---------------------------------------------------------------- sinks

/// A `Write` sink whose bytes stay readable after it is moved.
#[derive(Debug, Clone, Default)]
pub struct SharedBuf(pub Rc<RefCell<Vec<u8>>>);

impl SharedBuf {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.borrow()).into_owned()
    }
}

impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn text(buf: &[u8]) -> String {
    String::from_utf8_lossy(buf).into_owned()
}

pub fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------- clock

/// Deterministic clock: time moves only when the session sleeps.
#[derive(Debug, Clone)]
pub struct FakeClock(pub Rc<RefCell<(u64, u64)>>);

impl FakeClock {
    pub fn new(now_ms: u64) -> FakeClock {
        FakeClock(Rc::new(RefCell::new((now_ms, 0x0123_4567_89AB_CDEF))))
    }
}

impl Clock for FakeClock {
    fn now_ms(&mut self) -> u64 {
        self.0.borrow().0
    }
    fn random_u64(&mut self) -> u64 {
        let mut s = self.0.borrow_mut();
        s.1 = s.1.wrapping_add(0x1111);
        s.1
    }
    fn sleep(&mut self, d: Duration) {
        self.0.borrow_mut().0 += d.as_millis() as u64;
    }
}

pub const T0: u64 = 1_790_000_000_000;

pub fn session<T: Transport>(transport: T) -> Session<T, FakeClock> {
    Session::with_clock(transport, FakeClock::new(T0))
}

// ---------------------------------------------------------------- frames

pub fn get_data(id: u8, mask: u16) -> Vec<u8> {
    let [lo, hi] = (mask << 1).to_le_bytes();
    vec![0x0C, id, lo, hi]
}

pub fn accept(id: u8) -> Vec<u8> {
    vec![0x05, id, 0, 0]
}

pub fn start_queue(id: u8, code: u8) -> Vec<u8> {
    vec![0x0E, id, code << 1, 0x00]
}

pub fn stop_queue(id: u8) -> Vec<u8> {
    vec![0x0F, id, 0x00, 0x00]
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

/// Synthetic ADC snapshot values (raw units of the 40-byte payload).
#[derive(Debug, Clone, Copy)]
pub struct Adc {
    pub vbus_uv: i32,
    pub ibus_ua: i32,
    pub temp_raw: i16,
    pub cc1: u16,
    pub cc2: u16,
    pub dp: u16,
    pub dm: u16,
}

/// 5.123 V, -2 A, 25 °C, D+ 0.6 V, D− 0.4 V, CC1 1.65 V, CC2 0.1234 V.
pub const ADC_A: Adc = Adc {
    vbus_uv: 5_123_000,
    ibus_ua: -2_000_000,
    temp_raw: 25 * 128,
    cc1: 16_500,
    cc2: 1_234,
    dp: 6_000,
    dm: 4_000,
};

pub fn adc_payload(a: Adc) -> Vec<u8> {
    let mut p = vec![0u8; 40];
    p[0x00..0x04].copy_from_slice(&a.vbus_uv.to_le_bytes());
    p[0x04..0x08].copy_from_slice(&a.ibus_ua.to_le_bytes());
    p[0x08..0x0C].copy_from_slice(&a.vbus_uv.to_le_bytes());
    p[0x0C..0x10].copy_from_slice(&a.ibus_ua.to_le_bytes());
    p[0x10..0x14].copy_from_slice(&a.vbus_uv.to_le_bytes());
    p[0x14..0x18].copy_from_slice(&a.ibus_ua.to_le_bytes());
    p[0x18..0x1A].copy_from_slice(&a.temp_raw.to_le_bytes());
    p[0x1A..0x1C].copy_from_slice(&a.cc1.to_le_bytes());
    p[0x1C..0x1E].copy_from_slice(&a.cc2.to_le_bytes());
    p[0x1E..0x20].copy_from_slice(&a.dp.to_le_bytes());
    p[0x20..0x22].copy_from_slice(&a.dm.to_le_bytes());
    p[0x22..0x24].copy_from_slice(&33_000u16.to_le_bytes());
    p
}

/// PUT_DATA reply with one ADC section.
pub fn adc_reply(id: u8, a: Adc) -> Vec<u8> {
    let mut f = put_header(id, 0).to_vec();
    f.extend(
        SectionHeader {
            att: 0x001,
            next: false,
            chunk: 1,
            size: 40,
        }
        .encode(),
    );
    f.extend(adc_payload(a));
    f
}

/// Scripted GET_DATA(ADC) with id `id`.
pub fn adc_step(id: u8, a: Adc) -> Step {
    Step::transact(get_data(id, 0x001), adc_reply(id, a))
}

/// One 20-byte queue record: VBUS 5 V, IBUS -2 A, CC1 raw 805, CC2 7,
/// D+ 8, D− 9 (1 mV units at 10 SPS).
pub fn record(time_ms: u32) -> Vec<u8> {
    let mut r = Vec::new();
    r.extend(time_ms.to_le_bytes());
    r.extend(5_000_000i32.to_le_bytes());
    r.extend((-2_000_000i32).to_le_bytes());
    r.extend(805u16.to_le_bytes());
    r.extend(7u16.to_le_bytes());
    r.extend(8u16.to_le_bytes());
    r.extend(9u16.to_le_bytes());
    r
}

/// PUT_DATA reply with one ADC_QUEUE section holding records at `times`.
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
        f.extend(record(t));
    }
    f
}

pub fn poll_step(id: u8, times: &[u32]) -> Step {
    Step::transact(get_data(id, 0x002), queue_reply(id, 1, times))
}

// ---------------------------------------------------------------- memory read

/// Scripted memory read of `block` with request id `id`.
pub fn memread_steps(id: u8, addr: u32, block: &[u8]) -> Vec<Step> {
    let len = block.len() as u32;
    let mut confirm = vec![0xC4, id, 0x01, 0x01];
    confirm.extend(&memory_read_plaintext(addr, len)[..16]);
    vec![
        Step::transact(memory_read_request(id, addr, len), confirm),
        Step::read(crypto::encrypt(&KEYS[0], block)),
    ]
}

/// Made-up 64-byte device-info block.
pub fn info_block(model: &str, version: &str) -> Vec<u8> {
    let mut b = vec![0u8; 64];
    b[0x10..0x10 + model.len()].copy_from_slice(model.as_bytes());
    b[0x1C..0x1C + version.len()].copy_from_slice(version.as_bytes());
    b[0x28..0x28 + 8].copy_from_slice(b"20260101");
    b
}

/// Made-up 64-byte OTP block starting with the synthetic serial.
pub fn otp_block() -> Vec<u8> {
    let mut b = b"123456 0f1e2d3c4b5a69788796a5b4c3d2e1f0 1700000000 T1".to_vec();
    b.resize(64, 0);
    b
}

/// The four memory reads of `Session::device_info`, ids `first..first+3`.
pub fn device_info_steps(first: u8) -> Vec<Step> {
    [
        memread_steps(first, HW_INFO_ADDR, &info_block(SYNTH_MODEL, SYNTH_HW)),
        memread_steps(first + 1, FW_INFO_ADDR, &info_block(SYNTH_MODEL, SYNTH_FW)),
        memread_steps(first + 2, OTP_ADDR, &otp_block()),
        memread_steps(first + 3, UID_ADDR, &SYNTH_UID),
    ]
    .concat()
}

// ---------------------------------------------------------------- simulated meter

/// Key-3 variant the meter encrypts its auth reply with: byte 1 is `X`.
fn reply_key() -> [u8; 16] {
    let mut k = KEYS[3];
    k[1] = b'X';
    k
}

/// A [`MockTransport`] that answers 0x4C auth requests like the meter
/// (echo under the reply key, level 1) and, once `after_polls` queue polls
/// (GET_DATA ADC_QUEUE) have been sent, sets `stop` — standing in for Ctrl-C.
#[derive(Debug)]
pub struct Meter {
    pub inner: MockTransport,
    pub stop: Arc<AtomicBool>,
    pub after_polls: Option<usize>,
    pub polls: usize,
}

impl Meter {
    pub fn new(script: Vec<Step>) -> Meter {
        Meter {
            inner: MockTransport::new(script),
            stop: Arc::new(AtomicBool::new(false)),
            after_polls: None,
            polls: 0,
        }
    }

    pub fn stop_after_polls(mut self, n: usize) -> Meter {
        self.after_polls = Some(n);
        self
    }
}

impl Transport for Meter {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        if request.first() == Some(&0x4C) {
            let plain = crypto::decrypt(&KEYS[3], &request[4..]);
            let word: u32 = 0x4C | (1 << 16) | (1u32 << 17) | (8 << 22);
            let mut reply = word.to_le_bytes().to_vec();
            reply.extend(crypto::encrypt(&reply_key(), &plain));
            return Ok(reply);
        }
        if request.len() == 4 && request[0] == 0x0C && request[2..] == [0x04, 0x00] {
            self.polls += 1;
            if self.after_polls == Some(self.polls) {
                self.stop.store(true, Ordering::SeqCst);
            }
        }
        self.inner.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.inner.read(timeout)
    }
}

/// Script prefix of `start_stream` from auth level 0: UID read (id 1),
/// auth (id 2), START_QUEUE (id 3).
pub fn start_steps(code: u8) -> Vec<Step> {
    let mut s = memread_steps(1, UID_ADDR, &SYNTH_UID);
    s.push(Step::transact(start_queue(3, code), accept(3)));
    s
}

// ---------------------------------------------------------------- devices

pub fn entry(model: Model, pid: u16, serial: Option<&str>, bus: &str, address: u8) -> DeviceEntry {
    DeviceEntry {
        model,
        pid,
        serial: serial.map(str::to_owned),
        bus: bus.to_owned(),
        address,
    }
}

pub fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 + 1e-6 * b.abs()
}
