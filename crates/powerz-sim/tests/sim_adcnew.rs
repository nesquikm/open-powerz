//! The simulator's ADCNEW stream on the CDC persona (FR 1HHT02, AC .3, .5
//! and .6): `02` / `02 <att>` start frames at the requested rate with a
//! correct CRC, `03` stops them, `01` keeps them alive, the one-hour expiry
//! runs on the sim clock, a fault knob corrupts one frame, and the
//! protocol library's ADCNEW session and PDM session run against it.

mod common;

use std::time::Duration;

use common::*;
use powerz_protocol::adcnew::{CdcDemux, CdcItem, Content, Frame};
use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_protocol::pdm::PdmSession;
use powerz_protocol::transport::{Error as TransportError, Transport};
use powerz_sim::{SimCharger, SimPdm};

const MIN: u64 = 60_000;

fn cdc(scenario: Scenario, clock: &SimClock) -> SimDevice {
    device(Persona::Cdc, scenario, clock)
}

/// Write raw request bytes to the device as the host would; ADCNEW
/// requests get no reply.
fn send(dev: &mut SimDevice, bytes: &[u8]) {
    match dev.transact(bytes, T) {
        Ok(reply) => panic!("ADCNEW request {bytes:02x?} got a reply {reply:02x?}"),
        Err(TransportError::Timeout) => {}
        Err(e) => panic!("ADCNEW request {bytes:02x?} failed: {e}"),
    }
}

/// Every byte waiting on the pipe.
fn drain_bytes(dev: &mut SimDevice) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..10_000_000 {
        match dev.read(T) {
            Ok(chunk) => out.extend(chunk),
            Err(TransportError::Timeout) => return out,
            Err(e) => panic!("read failed: {e}"),
        }
    }
    panic!("the pipe never ran dry");
}

/// Demultiplex `bytes`: (frames, other items, dropped frames).
fn demux(bytes: &[u8]) -> (Vec<Frame>, Vec<CdcItem>, usize) {
    let mut d = CdcDemux::new();
    d.push(bytes);
    let mut frames = Vec::new();
    let mut other = Vec::new();
    while let Some(item) = d.next_item() {
        match item.expect("demux error") {
            CdcItem::Frame(f) => frames.push(f),
            o => other.push(o),
        }
    }
    assert_eq!(d.buffered(), 0, "a partial frame was left on the pipe");
    (frames, other, d.dropped_frames())
}

fn records(frames: &[Frame]) -> Vec<QueueRecord> {
    frames
        .iter()
        .flat_map(|f| match f {
            Frame::Records { records, .. } => records.clone(),
            Frame::Raw { .. } => Vec::new(),
        })
        .collect()
}

fn rates(frames: &[Frame]) -> Vec<Rate> {
    frames
        .iter()
        .filter_map(|f| match f {
            Frame::Records { rate, .. } => Some(*rate),
            Frame::Raw { .. } => None,
        })
        .collect()
}

// ---- AC-1HHT02.6: frames at the requested rate, CRC correct ----

#[test]
fn start_at_50_sps_streams_one_record_per_20_ms() {
    let clock = clock();
    let mut dev = cdc(Scenario::Load5v1a, &clock);
    send(&mut dev, &[0x02, 0x02]);
    clock.advance(ms(1000));
    let (frames, other, dropped) = demux(&drain_bytes(&mut dev));
    assert_eq!(dropped, 0, "every simulated frame has a correct CRC");
    assert!(other.is_empty(), "{other:?}");
    assert!(rates(&frames).iter().all(|r| *r == Rate::Sps50));
    let recs = records(&frames);
    assert!(
        (45..=55).contains(&recs.len()),
        "{} records in 1 s",
        recs.len()
    );
    for pair in recs.windows(2) {
        assert_eq!(pair[1].time_ms() - pair[0].time_ms(), 20);
    }
    for r in &recs {
        assert!((r.vbus_v() - 5.0).abs() < 0.1, "VBUS {}", r.vbus_v());
        assert!((r.ibus_a() - 1.0).abs() < 0.05, "IBUS {}", r.ibus_a());
    }
}

#[test]
fn start_at_1000_sps_streams_a_thousand_records_a_second() {
    let clock = clock();
    let mut dev = cdc(Scenario::Load5v1a, &clock);
    send(&mut dev, &[0x02, 0x03]);
    clock.advance(ms(1000));
    let (frames, _, dropped) = demux(&drain_bytes(&mut dev));
    assert_eq!(dropped, 0);
    assert!(rates(&frames).iter().all(|r| *r == Rate::Sps1000));
    let recs = records(&frames);
    assert!((950..=1050).contains(&recs.len()), "{} records", recs.len());
    for pair in recs.windows(2) {
        assert_eq!(pair[1].time_ms() - pair[0].time_ms(), 1);
    }
}

#[test]
fn a_bare_start_sends_the_section_frame_then_records_at_the_meter_rate() {
    let clock = clock();
    let mut dev = cdc(Scenario::Load5v1a, &clock);
    send(&mut dev, &[0x02]);
    clock.advance(ms(500));
    let (frames, _, dropped) = demux(&drain_bytes(&mut dev));
    assert_eq!(dropped, 0);
    match frames.first() {
        Some(Frame::Raw { att: 0x03, body }) => assert!(!body.is_empty()),
        other => panic!("expected the att-0x03 frame first, got {other:?}"),
    }
    assert!(frames.len() > 5);
    assert!(rates(&frames).iter().all(|r| *r == Rate::Sps50));
}

#[test]
fn pd_content_still_streams_adc_records() {
    let clock = clock();
    let mut dev = cdc(Scenario::Pd9v2a, &clock);
    send(&mut dev, &[0x02, 0x12]);
    clock.advance(ms(1000));
    let (frames, _, dropped) = demux(&drain_bytes(&mut dev));
    assert_eq!(dropped, 0);
    let recs = records(&frames);
    assert!((45..=55).contains(&recs.len()), "{}", recs.len());
    assert!((recs[0].vbus_v() - 9.0).abs() < 0.1);
}

// ---- AC-1HHT02.6: stop, keep-alive and the one-hour expiry ----

#[test]
fn stop_ends_the_stream() {
    let clock = clock();
    let mut dev = cdc(Scenario::Load5v1a, &clock);
    send(&mut dev, &[0x02, 0x02]);
    assert!(dev.adcnew_active());
    clock.advance(ms(200));
    let _ = drain_bytes(&mut dev);
    send(&mut dev, &[0x03]);
    assert!(!dev.adcnew_active());
    clock.advance(ms(1000));
    let (frames, _, _) = demux(&drain_bytes(&mut dev));
    assert!(frames.is_empty(), "{} frames after stop", frames.len());
}

#[test]
fn the_stream_expires_one_hour_after_the_last_start() {
    let clock = clock();
    let mut dev = cdc(Scenario::Load5v1a, &clock);
    send(&mut dev, &[0x02, 0x00]); // 2 SPS
    clock.advance(ms(61 * MIN));
    let recs = records(&demux(&drain_bytes(&mut dev)).0);
    let first = u64::from(recs.first().expect("records").time_ms());
    let last = u64::from(recs.last().unwrap().time_ms());
    assert!(last - first <= 60 * MIN, "streamed {} ms", last - first);
    assert!(
        last - first >= 60 * MIN - 1_000,
        "stopped early: {} ms",
        last - first
    );
    assert!(!dev.adcnew_active(), "still streaming after an hour");
    clock.advance(ms(MIN));
    assert!(records(&demux(&drain_bytes(&mut dev)).0).is_empty());
}

#[test]
fn a_keep_alive_within_the_hour_extends_the_stream() {
    let clock = clock();
    let mut dev = cdc(Scenario::Load5v1a, &clock);
    send(&mut dev, &[0x02, 0x00]);
    clock.advance(ms(50 * MIN));
    let mut recs = records(&demux(&drain_bytes(&mut dev)).0);
    send(&mut dev, &[0x01]);
    clock.advance(ms(20 * MIN));
    recs.extend(records(&demux(&drain_bytes(&mut dev)).0));
    let first = u64::from(recs.first().unwrap().time_ms());
    let last = u64::from(recs.last().unwrap().time_ms());
    assert!(
        last - first >= 69 * MIN,
        "the keep-alive did not hold: {} ms",
        last - first
    );
    assert!(dev.adcnew_active());
}

#[test]
fn a_keep_alive_after_the_hour_does_not_restart_it() {
    let clock = clock();
    let mut dev = cdc(Scenario::Load5v1a, &clock);
    send(&mut dev, &[0x02, 0x00]);
    clock.advance(ms(61 * MIN));
    let _ = drain_bytes(&mut dev);
    send(&mut dev, &[0x01]);
    clock.advance(ms(MIN));
    assert!(records(&demux(&drain_bytes(&mut dev)).0).is_empty());
    assert!(!dev.adcnew_active());
}

// ---- AC-1HHT02.6: corrupt-frame fault knob ----

#[test]
fn the_corrupt_frame_fault_breaks_exactly_one_crc() {
    let clock = clock();
    let mut dev =
        cdc(Scenario::Load5v1a, &clock).with_faults(FaultPlan::new().corrupt_adcnew_at(5));
    send(&mut dev, &[0x02, 0x02]);
    clock.advance(ms(1000));
    let (frames, _, dropped) = demux(&drain_bytes(&mut dev));
    assert_eq!(dropped, 1);
    let recs = records(&frames);
    assert!((44..=54).contains(&recs.len()), "{}", recs.len());
    let jumps = recs
        .windows(2)
        .filter(|p| p[1].time_ms() - p[0].time_ms() > 20)
        .count();
    assert_eq!(jumps, 1, "one record missing");
}

#[test]
fn the_corrupt_frame_fault_parses() {
    assert_eq!(
        FaultPlan::parse("corrupt-adcnew@5"),
        Ok(FaultPlan::new().corrupt_adcnew_at(5))
    );
    assert!(FaultPlan::parse("corrupt-adcnew").is_err());
}

// ---- AC-1HHT02.5: the stream exists on CDC only ----

#[test]
fn bulk_and_hid_personas_never_stream_adcnew() {
    for persona in [Persona::Bulk, Persona::Hid] {
        let clock = clock();
        let dev = device(persona, Scenario::Load5v1a, &clock);
        let mut s = session(dev, &clock);
        assert!(
            s.start_adcnew(Content::Adc, Rate::Sps50).is_err(),
            "{persona:?}"
        );
        assert!(!s.transport().adcnew_active(), "{persona:?}");
    }
}

// ---- AC-1HHT02.3 / .6: the library's ADCNEW session against the simulator ----

#[test]
fn library_session_streams_and_stops_against_the_simulator() {
    let clock = clock();
    let mut s = session(cdc(Scenario::Load5v1a, &clock), &clock);
    s.start_adcnew(Content::Adc, Rate::Sps50).expect("start");
    assert!(s.transport().adcnew_active());
    let mut recs = Vec::new();
    for _ in 0..10 {
        s.sleep(ms(100));
        let poll = s.poll_adcnew().expect("poll");
        assert!(poll.gaps.is_empty(), "{:?}", poll.gaps);
        recs.extend(poll.records);
    }
    assert!((45..=55).contains(&recs.len()), "{}", recs.len());
    assert!((recs[0].vbus_v() - 5.0).abs() < 0.1);
    s.stop_adcnew().expect("stop");
    assert!(!s.transport().adcnew_active());
}

#[test]
fn library_session_keeps_the_simulated_stream_alive_past_the_hour() {
    let clock = clock();
    let mut s = session(cdc(Scenario::Load5v1a, &clock), &clock);
    s.start_adcnew(Content::Adc, Rate::Sps2).expect("start");
    let mut last = 0u32;
    for _ in 0..(3 * 60) {
        s.sleep(Duration::from_secs(60));
        if let Some(r) = s.poll_adcnew().expect("poll").records.last() {
            last = r.time_ms();
        }
    }
    assert!(s.transport().adcnew_active(), "the stream expired");
    assert!(u64::from(last) >= 179 * MIN, "records stopped at {last} ms");
}

#[test]
fn library_session_drops_the_corrupt_frame_and_reports_a_gap() {
    let clock = clock();
    let dev = cdc(Scenario::Load5v1a, &clock).with_faults(FaultPlan::new().corrupt_adcnew_at(10));
    let mut s = session(dev, &clock);
    s.start_adcnew(Content::Adc, Rate::Sps50).unwrap();
    let mut gaps = Vec::new();
    for _ in 0..10 {
        s.sleep(ms(100));
        gaps.extend(s.poll_adcnew().unwrap().gaps);
    }
    assert_eq!(gaps.len(), 1, "{gaps:?}");
}

// ---- AC-1HHT02.2: a PDM text session keeps the stream frames ----

#[test]
fn a_pdm_session_hands_out_the_stream_frames_it_read() {
    let clock = SimClock::new(T0);
    let sim = SimPdm::new(SimCharger::Epr140W, clock.clone()).with_interleaved_binary();
    let mut s = PdmSession::open_with_clock(sim.open_port(), clock.clone()).expect("open");
    let frames = s.take_stream_frames();
    assert!(
        !frames.is_empty(),
        "the interleaved ADCNEW frame was thrown away"
    );
    for f in &frames {
        match f {
            Frame::Records { rate, records } => {
                assert_eq!(*rate, Rate::Sps50);
                assert_eq!(records.len(), 1);
            }
            other => panic!("unexpected frame {other:?}"),
        }
    }
    assert!(s.take_stream_frames().is_empty(), "taken once");
    s.close().expect("close");
}
