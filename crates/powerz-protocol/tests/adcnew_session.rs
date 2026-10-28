//! Driving the ADCNEW stream over the CDC transport (FR 1HHT02, AC .2, .3
//! and .5): start with content and rate, poll records, stop, the keep-alive
//! before the meter's one-hour limit, binary requests while the stream runs,
//! and types 0x01..0x03 written only as ADCNEW requests and only on CDC.
//!
//! A scripted serial port stands in for the meter; a fake clock shares one
//! time line with it so the hour passes without sleeping.

mod common;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::time::Duration;

use common::FakeClock;
use powerz_protocol::adcnew::{self, Content, Frame};
use powerz_protocol::decode::Rate;
use powerz_protocol::session::{Clock, Session};
use powerz_protocol::transport::cdc::CdcTransport;
use powerz_protocol::transport::mock::MockTransport;
use powerz_protocol::transport::{Error as TransportError, GuardedTransport, Transport};

const T0: u64 = 1_790_000_000_000;
const HOUR_MS: u64 = 3_600_000;

// ---------------------------------------------------------------- fixtures

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Two 50/s record frames from the T11 session (firmware 2.0.6). [device]
const TWO_FRAMES_50: &str = "
    0212057c 383b0100a10f0000b5ffffff53001c0041004c00
    021205e4 4c3b0100a10f0000b5ffffff53001b0046004b00";

/// The 52-byte binary ADC reply (id 1) read on the CDC port. [device]
const CDC_ADC_REPLY: &str = "
    41018002 0100000b 64100000e9ffffff651000008bffffffc9100000dbffffff
    010f33032701d402f402337d008002f00000ff00";

fn ref_crc8(body: &[u8]) -> u8 {
    let mut c = 0u8;
    for &b in body {
        c ^= b;
        for _ in 0..8 {
            c = if c & 0x80 != 0 {
                (c << 1) ^ 0x29
            } else {
                c << 1
            };
        }
    }
    c
}

fn record(time_ms: u32, vbus_uv: i32, ibus_ua: i32) -> Vec<u8> {
    let mut r = Vec::with_capacity(20);
    r.extend(time_ms.to_le_bytes());
    r.extend(vbus_uv.to_le_bytes());
    r.extend(ibus_ua.to_le_bytes());
    for l in [1680u16, 0, 600, 600] {
        r.extend(l.to_le_bytes());
    }
    r
}

fn frame(att: u8, body: &[u8]) -> Vec<u8> {
    let mut f = vec![0x02, att, (body.len() / 4) as u8, ref_crc8(body)];
    f.extend_from_slice(body);
    f
}

fn record_frame_50(time_ms: u32) -> Vec<u8> {
    frame(0x12, &record(time_ms, 5_000_000, 1_000_000))
}

// ---------------------------------------------------------------- fake port

#[derive(Default)]
struct PortState {
    /// Chunks the next reads return, in order.
    incoming: VecDeque<Vec<u8>>,
    /// (fake time ms, bytes) per `write` call.
    writes: Vec<(u64, Vec<u8>)>,
}

/// Scripted serial port: reads hand out the fed chunks, then time out.
#[derive(Clone)]
struct Port {
    state: Rc<RefCell<PortState>>,
    clock: FakeClock,
}

impl Port {
    fn new(clock: &FakeClock) -> Port {
        Port {
            state: Rc::default(),
            clock: clock.clone(),
        }
    }
    fn feed(&self, bytes: &[u8]) {
        self.state.borrow_mut().incoming.push_back(bytes.to_vec());
    }
    fn writes(&self) -> Vec<(u64, Vec<u8>)> {
        self.state.borrow().writes.clone()
    }
    fn written(&self) -> Vec<Vec<u8>> {
        self.writes().into_iter().map(|(_, w)| w).collect()
    }
}

impl Read for Port {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut s = self.state.borrow_mut();
        match s.incoming.pop_front() {
            Some(mut chunk) => {
                let n = chunk.len().min(buf.len());
                buf[..n].copy_from_slice(&chunk[..n]);
                if n < chunk.len() {
                    let rest = chunk.split_off(n);
                    s.incoming.push_front(rest);
                }
                Ok(n)
            }
            None => Err(io::Error::new(io::ErrorKind::TimedOut, "nothing pending")),
        }
    }
}

impl Write for Port {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let now = self.clock.clone().now_ms();
        self.state.borrow_mut().writes.push((now, buf.to_vec()));
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

type CdcSession = Session<GuardedTransport<CdcTransport<Port>>, FakeClock>;

fn cdc_session() -> (CdcSession, Port, FakeClock) {
    let clock = FakeClock::new(T0);
    let port = Port::new(&clock);
    let session = Session::with_clock(CdcTransport::with_port(port.clone()), clock.clone());
    (session, port, clock)
}

fn times(poll: &powerz_protocol::session::StreamPoll) -> Vec<u32> {
    poll.records.iter().map(|r| r.time_ms()).collect()
}

/// Every write must be an ADCNEW request when its type is 0x01..0x03:
/// one or two bytes, never a 4-byte SYNC / CONNECT / DISCONNECT frame.
fn assert_only_adcnew_use_types_1_to_3(written: &[Vec<u8>]) {
    for w in written {
        if let Some(ty) = w.first().map(|b| b & 0x7F)
            && (0x01..=0x03).contains(&ty)
        {
            assert!(
                w.len() <= 2,
                "type {ty:#04x} written as a {}-byte frame on CDC: {w:02x?}",
                w.len()
            );
        }
    }
}

// ---- AC-1HHT02.3: start picks content and rate, poll, stop ----

#[test]
fn start_writes_the_content_and_rate_request_and_nothing_else() {
    let (mut s, port, _) = cdc_session();
    s.start_adcnew(Content::Adc, Rate::Sps50).expect("start");
    assert_eq!(
        port.written(),
        vec![vec![0x02, 0x02]],
        "no auth, no queue start: just ADCNEW start"
    );
}

#[test]
fn start_encodes_every_content_and_rate() {
    for (content, rate, att) in [
        (Content::Adc, Rate::Sps2, 0x00),
        (Content::Adc, Rate::Sps1000, 0x03),
        (Content::AdcPd, Rate::Sps10, 0x11),
        (Content::AdcUfcs, Rate::Sps50, 0x22),
        (Content::AdcPdUfcs, Rate::Sps1000, 0x33),
    ] {
        let (mut s, port, _) = cdc_session();
        s.start_adcnew(content, rate).expect("start");
        assert_eq!(
            port.written(),
            vec![vec![0x02, att]],
            "{content:?} {rate:?}"
        );
    }
}

#[test]
fn poll_returns_the_streamed_records() {
    let (mut s, port, _) = cdc_session();
    s.start_adcnew(Content::Adc, Rate::Sps50).unwrap();
    port.feed(&hex(TWO_FRAMES_50));
    let poll = s.poll_adcnew().expect("poll");
    assert_eq!(times(&poll), vec![80_696, 80_716]);
    assert!(poll.gaps.is_empty(), "{:?}", poll.gaps);
    assert_eq!(poll.records[0].rate(), Rate::Sps50);
    assert_eq!(poll.records[0].vbus_uv(), 4_001);
}

#[test]
fn a_frame_split_across_polls_is_not_lost() {
    let (mut s, port, _) = cdc_session();
    s.start_adcnew(Content::Adc, Rate::Sps50).unwrap();
    let bytes = hex(TWO_FRAMES_50);
    port.feed(&bytes[..30]);
    let first = s.poll_adcnew().unwrap();
    port.feed(&bytes[30..]);
    let second = s.poll_adcnew().unwrap();
    let mut all = times(&first);
    all.extend(times(&second));
    assert_eq!(all, vec![80_696, 80_716]);
}

#[test]
fn missing_records_are_reported_as_a_gap() {
    let (mut s, port, _) = cdc_session();
    s.start_adcnew(Content::Adc, Rate::Sps50).unwrap();
    for t in [10_000, 10_020, 10_100, 10_120] {
        port.feed(&record_frame_50(t));
    }
    let poll = s.poll_adcnew().unwrap();
    assert_eq!(times(&poll), vec![10_000, 10_020, 10_100, 10_120]);
    assert_eq!(poll.gaps.len(), 1, "{:?}", poll.gaps);
    assert_eq!(poll.gaps[0].from_ms, 10_020);
    assert_eq!(poll.gaps[0].to_ms, 10_100);
}

#[test]
fn a_corrupt_frame_costs_its_records_only() {
    let (mut s, port, _) = cdc_session();
    s.start_adcnew(Content::Adc, Rate::Sps50).unwrap();
    let mut bad = record_frame_50(10_020);
    bad[3] ^= 0xFF;
    port.feed(&record_frame_50(10_000));
    port.feed(&bad);
    port.feed(&record_frame_50(10_040));
    let poll = s.poll_adcnew().unwrap();
    assert_eq!(times(&poll), vec![10_000, 10_040]);
    assert_eq!(poll.gaps.len(), 1, "the dropped record shows as a gap");
}

#[test]
fn pd_and_ufcs_frames_are_kept_raw_beside_the_records() {
    let (mut s, port, _) = cdc_session();
    s.start_adcnew(Content::AdcPd, Rate::Sps50).unwrap();
    assert_eq!(port.written(), vec![vec![0x02, 0x12]]);
    let pd = frame(0x21, &[0x5A; 8]);
    port.feed(&record_frame_50(10_000));
    port.feed(&pd);
    port.feed(&record_frame_50(10_020));
    let poll = s.poll_adcnew().unwrap();
    assert_eq!(times(&poll), vec![10_000, 10_020]);
    assert_eq!(
        s.take_adcnew_raw(),
        vec![Frame::Raw {
            att: 0x21,
            body: vec![0x5A; 8]
        }]
    );
    assert!(s.take_adcnew_raw().is_empty(), "taken once");
}

#[test]
fn raw_frames_nobody_takes_are_capped_to_the_newest() {
    let (mut s, port, _) = cdc_session();
    s.start_adcnew(Content::AdcPd, Rate::Sps50).unwrap();
    let cap = powerz_protocol::adcnew::MAX_RAW_FRAMES;
    for i in 0..cap + 5 {
        port.feed(&frame(0x21, &(i as u32).to_le_bytes()));
        s.poll_adcnew().unwrap();
    }
    let raw = s.take_adcnew_raw();
    assert_eq!(raw.len(), cap);
    assert_eq!(
        raw.first(),
        Some(&Frame::Raw {
            att: 0x21,
            body: 5u32.to_le_bytes().to_vec()
        })
    );
}

#[test]
fn stop_writes_the_stop_request() {
    let (mut s, port, _) = cdc_session();
    s.start_adcnew(Content::Adc, Rate::Sps50).unwrap();
    s.stop_adcnew().expect("stop");
    assert_eq!(port.written(), vec![vec![0x02, 0x02], vec![0x03]]);
}

#[test]
fn a_new_start_replaces_the_running_stream() {
    let (mut s, port, _) = cdc_session();
    s.start_adcnew(Content::Adc, Rate::Sps50).unwrap();
    s.start_adcnew(Content::Adc, Rate::Sps1000).unwrap();
    let w = port.written();
    assert_eq!(w.first(), Some(&vec![0x02, 0x02]));
    assert_eq!(w.last(), Some(&vec![0x02, 0x03]));
    // Records after the restart carry the new rate.
    port.feed(&frame(0x13, &record(1, 2, 3)));
    let poll = s.poll_adcnew().unwrap();
    assert_eq!(poll.records[0].rate(), Rate::Sps1000);
}

// ---- AC-1HHT02.3: refreshed before the meter's one-hour limit ----

#[test]
fn the_keep_alive_period_is_well_inside_the_hour() {
    assert!(adcnew::KEEP_ALIVE_PERIOD < Duration::from_secs(55 * 60));
    assert!(adcnew::KEEP_ALIVE_PERIOD >= Duration::from_secs(60));
}

#[test]
fn polling_refreshes_the_stream_before_every_hour_runs_out() {
    let (mut s, port, clock) = cdc_session();
    s.start_adcnew(Content::Adc, Rate::Sps50).unwrap();
    // Three hours of polls, two minutes apart on the fake clock.
    for _ in 0..90 {
        let mut c = clock.clone();
        c.sleep(Duration::from_secs(120));
        s.poll_adcnew().expect("poll");
    }
    let writes = port.writes();
    // Start and every refresh: ADCNEW keep-alive `01` or a fresh start `02 ..`.
    let refreshes: Vec<u64> = writes
        .iter()
        .filter(|(_, w)| matches!(w.first(), Some(0x01 | 0x02)))
        .map(|(t, _)| *t)
        .collect();
    assert!(refreshes.len() >= 4, "too few refreshes: {writes:?}");
    let mut last = T0;
    for &t in &refreshes[1..] {
        assert!(t - last < HOUR_MS, "{} ms without a refresh", t - last);
        last = t;
    }
    let end = clock.clone().now_ms();
    assert!(end - last < HOUR_MS, "the last hour had no refresh");
    // Not on every poll: at most one per keep-alive period.
    let period = adcnew::KEEP_ALIVE_PERIOD.as_millis() as u64;
    for pair in refreshes.windows(2) {
        assert!(
            pair[1] - pair[0] >= period,
            "refreshed too often: {refreshes:?}"
        );
    }
    assert!(
        writes.iter().all(|(_, w)| w.first() != Some(&0x03)),
        "the stream was stopped: {writes:?}"
    );
    assert_only_adcnew_use_types_1_to_3(&port.written());
}

#[test]
fn a_stopped_stream_is_not_kept_alive() {
    let (mut s, port, clock) = cdc_session();
    s.start_adcnew(Content::Adc, Rate::Sps50).unwrap();
    s.stop_adcnew().unwrap();
    let before = port.written().len();
    let mut c = clock.clone();
    c.sleep(Duration::from_secs(2 * 3600));
    let _ = s.poll_adcnew();
    assert_eq!(port.written().len(), before, "{:02x?}", port.written());
}

// ---- AC-1HHT02.2: binary requests while the stream runs ----

#[test]
fn an_adc_read_during_the_stream_gets_its_reply_and_loses_no_frame() {
    let (mut s, port, _) = cdc_session();
    s.start_adcnew(Content::Adc, Rate::Sps50).unwrap();
    let frames = hex(TWO_FRAMES_50);
    // A frame, the ADC reply, another frame: all already on the wire.
    port.feed(&frames[..24]);
    port.feed(&hex(CDC_ADC_REPLY));
    port.feed(&frames[24..]);
    let adc = s.read_adc().expect("ADC read during the stream");
    assert!((adc.vbus_v() - 0.004196).abs() < 1e-9, "{}", adc.vbus_v());
    let poll = s.poll_adcnew().unwrap();
    assert_eq!(times(&poll), vec![80_696, 80_716]);
    let written = port.written();
    assert_eq!(written[0], vec![0x02, 0x02]);
    assert_eq!(written[1][0], 0x0C, "GET_DATA went out: {written:02x?}");
}

// ---- AC-1HHT02.5: types 0x01..0x03 only as ADCNEW, only on CDC ----

#[test]
fn the_generic_cdc_path_still_refuses_types_1_to_3() {
    let clock = FakeClock::new(T0);
    let port = Port::new(&clock);
    let mut t = CdcTransport::with_port(port.clone());
    for ty in [0x01, 0x02, 0x03] {
        assert_eq!(
            t.transact(&[ty, 0x01, 0x00, 0x00], Duration::from_millis(50)),
            Err(TransportError::UnsupportedOnCdc { ty })
        );
    }
    assert!(port.written().is_empty());
}

#[test]
fn a_full_cdc_stream_session_writes_types_1_to_3_only_as_adcnew() {
    let (mut s, port, clock) = cdc_session();
    s.start_adcnew(Content::Adc, Rate::Sps50).unwrap();
    for i in 0..30u32 {
        port.feed(&record_frame_50(10_000 + 20 * i));
        let mut c = clock.clone();
        c.sleep(Duration::from_secs(60));
        s.poll_adcnew().unwrap();
    }
    s.stop_adcnew().unwrap();
    let written = port.written();
    assert_only_adcnew_use_types_1_to_3(&written);
    assert_eq!(written.first(), Some(&vec![0x02, 0x02]));
    assert_eq!(written.last(), Some(&vec![0x03]));
}

#[test]
fn adcnew_is_refused_off_cdc_without_writing() {
    // A non-CDC pipe (here the scripted mock) never gets ADCNEW requests.
    let mut s = Session::with_clock(MockTransport::new(vec![]), FakeClock::new(T0));
    assert!(s.start_adcnew(Content::Adc, Rate::Sps50).is_err());
    assert!(s.poll_adcnew().is_err());
    assert!(s.stop_adcnew().is_err());
    assert!(
        s.transport().written().is_empty(),
        "{:?}",
        s.transport().written()
    );
}
