//! A scripted CDC serial port for FR 3B70S8: it answers GET_DATA (ADC) with
//! the reply carrying the request's id, optionally after stray bytes that a
//! meter streaming to another program sends on the same port (an echoed
//! ADCNEW start, an ADCNEW record frame, a text line, replies to other ids),
//! and can echo every ADCNEW start back instead of streaming, as the meter
//! does while a bulk (WebUSB) session runs `[device]`.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::rc::Rc;

use powerz_protocol::session::Session;
use powerz_protocol::transport::GuardedTransport;
use powerz_protocol::transport::cdc::CdcTransport;
use powerz_sim::SimClock;

pub const T0: u64 = 1_790_000_000_000;

/// The 52-byte ADC snapshot reply from our own test vectors.
const ADC_52: &str = include_str!("../../../powerz-protocol/tests/vectors/adc_52.hex");

/// The echo of the ADCNEW start for ADC at 50/s.
pub const ECHOED_START: [u8; 2] = [0x02, 0x02];

/// Parse a `.hex` vector: `#` lines are comments, the rest is hex digits.
fn hex(text: &str) -> Vec<u8> {
    let digits: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .flat_map(|l| l.chars().filter(char::is_ascii_hexdigit))
        .collect();
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).expect("hex digits"))
        .collect()
}

/// The ADC reply carrying request id `id` and VBUS `vbus_uv` µV.
pub fn adc_reply(id: u8, vbus_uv: i32) -> Vec<u8> {
    let mut f = hex(ADC_52);
    assert_eq!(f.len(), 52);
    f[1] = id;
    // Payload starts after the 4-byte main and 4-byte section headers.
    f[8..12].copy_from_slice(&vbus_uv.to_le_bytes());
    f
}

/// A one-record ADCNEW frame (att 0x12) with device time `n` ms.
pub fn record_frame(n: u32) -> Vec<u8> {
    let mut body = n.to_le_bytes().to_vec();
    body.extend([0u8; 16]);
    let mut f = vec![0x02, 0x12, 0x05, powerz_protocol::adcnew::crc8(&body)];
    f.extend(body);
    f
}

#[derive(Default)]
pub struct State {
    pub written: Vec<Vec<u8>>,
    reads: VecDeque<u8>,
    /// GET_DATA requests answered so far.
    pub answered: u32,
    /// Send stray frames before every reply.
    pub stray: bool,
    /// Echo every ADCNEW start (`02 <att>`) instead of streaming.
    pub echo_starts: bool,
    /// GET_DATA requests (0-based, counting every one written) left
    /// unanswered, as a meter busy with another session does (FR NVN6M7).
    pub drop_requests: Vec<u32>,
    /// From this GET_DATA request (0-based) on the port is gone: reads
    /// return end of stream (FR NVN6M7).
    pub gone_from_request: Option<u32>,
    /// GET_DATA requests written so far, answered or not.
    pub requested: u32,
    /// The port went away.
    pub gone: bool,
}

/// The scripted port; clones share one state.
#[derive(Clone, Default)]
pub struct StrayPort(pub Rc<RefCell<State>>);

impl StrayPort {
    pub fn new(stray: bool, echo_starts: bool) -> StrayPort {
        let p = StrayPort::default();
        {
            let mut s = p.0.borrow_mut();
            s.stray = stray;
            s.echo_starts = echo_starts;
        }
        p
    }

    /// A port that leaves the GET_DATA requests `drop` (0-based) unanswered.
    pub fn dropping(stray: bool, echo_starts: bool, drop: &[u32]) -> StrayPort {
        let p = StrayPort::new(stray, echo_starts);
        p.0.borrow_mut().drop_requests = drop.to_vec();
        p
    }

    /// Leave the GET_DATA requests `drop` (0-based) unanswered and go away
    /// at GET_DATA request `gone` (0-based).
    pub fn going_away(echo_starts: bool, drop: &[u32], gone: u32) -> StrayPort {
        let p = StrayPort::dropping(false, echo_starts, drop);
        p.0.borrow_mut().gone_from_request = Some(gone);
        p
    }

    /// Every write, one request each.
    pub fn written(&self) -> Vec<Vec<u8>> {
        self.0.borrow().written.clone()
    }

    /// GET_DATA requests written so far.
    pub fn get_data_requests(&self) -> usize {
        self.written()
            .iter()
            .filter(|w| w.len() >= 4 && w[0] & 0x7F == 0x0C)
            .count()
    }

    /// VBUS of the `n`th reply (0-based), in V, as the watch prints it.
    pub fn vbus_v(n: u32) -> f64 {
        f64::from(Self::vbus_uv(n)) / 1_000_000.0
    }

    fn vbus_uv(n: u32) -> i32 {
        5_000_000 + 10_000 * i32::try_from(n).expect("small n")
    }
}

impl Read for StrayPort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut s = self.0.borrow_mut();
        if s.gone {
            return Ok(0);
        }
        if s.reads.is_empty() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "fake timeout"));
        }
        // Small reads, so frames and replies are split across them.
        let n = buf.len().min(s.reads.len()).min(13);
        for b in buf.iter_mut().take(n) {
            *b = s.reads.pop_front().expect("counted");
        }
        Ok(n)
    }
}

impl Write for StrayPort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut s = self.0.borrow_mut();
        s.written.push(buf.to_vec());
        if buf.len() == 2 && buf[0] == 0x02 && s.echo_starts {
            s.reads.extend(buf.iter().copied());
        } else if buf.len() >= 4 && buf[0] & 0x7F == 0x0C {
            let id = buf[1];
            let nth = s.requested;
            s.requested += 1;
            if s.gone_from_request.is_some_and(|g| nth >= g) {
                s.gone = true;
                return Ok(buf.len());
            }
            if s.drop_requests.contains(&nth) {
                return Ok(buf.len());
            }
            if s.stray {
                let n = s.answered;
                let mut stray = ECHOED_START.to_vec();
                stray.extend(record_frame(n * 20));
                stray.extend(b"ok\r\n");
                stray.extend([0x05, id.wrapping_add(100), 0x00, 0x00]);
                stray.extend(adc_reply(id.wrapping_sub(1), 1));
                stray.extend(ECHOED_START);
                s.reads.extend(stray);
            }
            let reply = adc_reply(id, StrayPort::vbus_uv(s.answered));
            s.reads.extend(reply);
            if s.stray {
                // And a stream frame right behind the reply.
                let n = s.answered;
                s.reads.extend(record_frame(n * 20 + 10));
            }
            s.answered += 1;
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub type StraySession = Session<GuardedTransport<CdcTransport<StrayPort>>, SimClock>;

/// A session over the CDC transport on `port`, on a simulated clock.
pub fn session(port: &StrayPort) -> (StraySession, SimClock) {
    let clock = SimClock::new(T0);
    let s = Session::with_clock(CdcTransport::with_port(port.clone()), clock.clone());
    (s, clock)
}

/// The number after `label` in a watch line.
pub fn value(line: &str, label: &str) -> f64 {
    let mut words = line.split_whitespace();
    while let Some(w) = words.next() {
        if w == label {
            return words
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| panic!("no number after {label} in {line}"));
        }
    }
    panic!("no {label} in {line}");
}

/// The line with every number (and the `-` of a missing value) replaced by
/// `#`: what must match between two watch lines of the same format.
pub fn shape(line: &str) -> String {
    line.split_whitespace()
        .map(|t| {
            if t == "-" || t.parse::<f64>().is_ok() {
                "#"
            } else {
                t
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
