//! FR TGE83A, AC .8 (and the simulator side of .1, .2): the simulator plays
//! a PD capture with a full negotiation, an EPR entry and a reset. It
//! answers 0x10 / 0x11 (REJECT below auth level 1) and PD_PACKET polls with
//! a 12-byte status block plus records while the sniffer runs.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use common::*;
use powerz_protocol::decode::pd_packet::{CaptureClock, PdEvent, PdRecordKind};
use powerz_protocol::frame::Attribute;
use powerz_protocol::session::{CapturedRecord, Error, PdOptions, PdSniffer, Session};
use powerz_protocol::transport::{Error as TransportError, Transport};
use powerz_sim::PdScenario;

/// Bulk device playing the PD capture scenario.
fn pd_device(clock: &SimClock) -> SimDevice {
    device(Persona::Bulk, Scenario::Idle, clock).with_pd_scenario(PdScenario::Negotiation)
}

fn pd_session() -> (SimSession, SimClock) {
    let clock = clock();
    let mut s = session(pd_device(&clock), &clock);
    s.connect().unwrap();
    (s, clock)
}

// ---- PD message classification by the 16-bit header (USB PD r3.2) ----

fn header(r: &CapturedRecord) -> Option<u16> {
    match &r.record.kind {
        PdRecordKind::Message(m) | PdRecordKind::BigMessage(m) => Some(m.header),
        PdRecordKind::Event(_) => None,
    }
}

fn is_sop(r: &CapturedRecord) -> bool {
    matches!(&r.record.kind, PdRecordKind::Message(m) | PdRecordKind::BigMessage(m) if m.sop == 0)
}

fn data(r: &CapturedRecord, ty: u16) -> bool {
    header(r).is_some_and(|h| h & 0x8000 == 0 && (h >> 12) & 7 != 0 && h & 0x1F == ty)
}

fn is_control(r: &CapturedRecord, ty: u16) -> bool {
    header(r).is_some_and(|h| h & 0x8000 == 0 && (h >> 12) & 7 == 0 && h & 0x1F == ty)
}

fn extended(r: &CapturedRecord, ty: u16) -> bool {
    header(r).is_some_and(|h| h & 0x8000 != 0 && h & 0x1F == ty)
}

const SOURCE_CAPS: u16 = 1;
const REQUEST: u16 = 2;
const EPR_REQUEST: u16 = 9;
const EPR_MODE: u16 = 10;
const ACCEPT: u16 = 3;
const PS_RDY: u16 = 6;
const EPR_SOURCE_CAPS: u16 = 17;

fn position(recs: &[CapturedRecord], from: usize, f: impl Fn(&CapturedRecord) -> bool) -> usize {
    recs[from..]
        .iter()
        .position(f)
        .map(|i| i + from)
        .unwrap_or_else(|| panic!("not found after record {from}"))
}

/// Capture until a Source_Capabilities after a Hard Reset is seen, or two
/// minutes of simulated time pass. Returns the records and the highest
/// VBUS of the status blocks.
fn capture(s: &mut SimSession) -> (Vec<CapturedRecord>, u16, usize) {
    let mut records = Vec::new();
    let mut max_vbus = 0u16;
    let mut skipped = 0;
    let mut polls = 0usize;
    let mut sniffer = PdSniffer::start(s, PdOptions::default()).unwrap();
    sniffer
        .run(|poll| {
            polls += 1;
            if let Some(st) = poll.status {
                max_vbus = max_vbus.max(st.vbus_mv);
            }
            skipped += poll.skipped;
            records.extend(poll.records.iter().cloned());
            let reset = records
                .iter()
                .position(|r| matches!(r.record.kind, PdRecordKind::Event(PdEvent::HardReset)));
            let renegotiated =
                reset.is_some_and(|i| records[i..].iter().any(|r| data(r, SOURCE_CAPS)));
            !renegotiated && polls < 24_000
        })
        .unwrap();
    sniffer.stop().unwrap();
    (records, max_vbus, skipped)
}

#[test]
fn pd_capture_scenario_has_negotiation_epr_entry_and_reset() {
    let (mut s, _clock) = pd_session();
    let (recs, max_vbus, skipped) = capture(&mut s);
    assert_eq!(skipped, 0);
    assert!(!recs.is_empty(), "the scenario produces records");

    // Capture-relative times never go backwards.
    assert!(
        recs.windows(2).all(|w| w[0].elapsed_ms <= w[1].elapsed_ms),
        "elapsed times are monotonic"
    );

    // Attach first.
    assert!(
        matches!(recs[0].record.kind, PdRecordKind::Event(PdEvent::Attach(_))),
        "first record is an attach: {:?}",
        recs[0].record
    );

    // SPR negotiation: Source_Capabilities, Request, Accept, PS_RDY.
    let caps = position(&recs, 0, |r| is_sop(r) && data(r, SOURCE_CAPS));
    let req = position(&recs, caps, |r| is_sop(r) && data(r, REQUEST));
    let acc = position(&recs, req, |r| is_sop(r) && is_control(r, ACCEPT));
    let rdy = position(&recs, acc, |r| is_sop(r) && is_control(r, PS_RDY));

    // EPR entry: EPR_Mode, EPR_Source_Capabilities, EPR_Request, Accept, PS_RDY.
    let mode = position(&recs, rdy, |r| data(r, EPR_MODE));
    let epr_caps = position(&recs, mode, |r| extended(r, EPR_SOURCE_CAPS));
    let epr_req = position(&recs, epr_caps, |r| data(r, EPR_REQUEST));
    let epr_acc = position(&recs, epr_req, |r| is_control(r, ACCEPT));
    position(&recs, epr_acc, |r| is_control(r, PS_RDY));

    // A reset, then a fresh negotiation.
    let reset = position(&recs, epr_req, |r| {
        matches!(r.record.kind, PdRecordKind::Event(PdEvent::HardReset))
    });
    position(&recs, reset, |r| data(r, SOURCE_CAPS));

    assert!(
        max_vbus > 5_500,
        "VBUS in the status blocks follows the contract: max {max_vbus} mV"
    );
}

#[test]
fn the_scenario_is_deterministic_on_simulated_time() {
    let (mut a, _) = pd_session();
    let (mut b, _) = pd_session();
    let (ra, _, _) = capture(&mut a);
    let (rb, _, _) = capture(&mut b);
    assert_eq!(ra, rb);
}

#[test]
fn start_is_rejected_below_auth_level_1() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock).with_pd_scenario(PdScenario::Negotiation);
    let reply = d.transact(&control(0x10, 2, 0x02), T).unwrap();
    assert_eq!(ty(&reply), 0x06, "0x10 at level 0: {reply:02x?}");
    let reply = d.transact(&control(0x11, 3, 0x00), T).unwrap();
    assert_eq!(ty(&reply), 0x06, "0x11 at level 0: {reply:02x?}");
}

#[test]
fn polls_without_the_sniffer_return_the_status_block_only() {
    let (mut s, clock) = pd_session();
    for _ in 0..5 {
        clock.advance(Duration::from_secs(2));
        let sections = s.get_data(Attribute::PD_PACKET).unwrap();
        let pd: Vec<_> = sections
            .iter()
            .filter(|x| x.att == Attribute::PD_PACKET)
            .collect();
        assert_eq!(pd.len(), 1);
        assert_eq!(pd[0].payload.len(), 12, "status block only");
    }
}

#[test]
fn polls_after_stop_return_the_status_block_only() {
    let (mut s, clock) = pd_session();
    let _ = capture(&mut s);
    clock.advance(Duration::from_secs(5));
    let sections = s.get_data(Attribute::PD_PACKET).unwrap();
    let pd = sections
        .iter()
        .find(|x| x.att == Attribute::PD_PACKET)
        .expect("PD section");
    assert_eq!(pd.payload.len(), 12);
}

#[test]
fn other_scenarios_give_status_only_captures() {
    let clock = clock();
    let mut s = session(device(Persona::Bulk, Scenario::Load5v1a, &clock), &clock);
    s.connect().unwrap();
    let mut sniffer = PdSniffer::start(&mut s, PdOptions::default()).unwrap();
    let mut records = 0;
    let mut polls = 0;
    sniffer
        .run(|p| {
            records += p.records.len();
            polls += 1;
            polls < 2_000
        })
        .unwrap();
    sniffer.stop().unwrap();
    assert_eq!(records, 0);
}

#[test]
fn poll_with_adc_returns_both_sections() {
    let (mut s, _clock) = pd_session();
    let mut sniffer = PdSniffer::start(&mut s, PdOptions { with_adc: true }).unwrap();
    let poll = sniffer.poll().unwrap();
    assert!(poll.adc.is_some(), "ADC asked for");
    assert!(poll.status.is_some(), "PD status block");
    sniffer.stop().unwrap();
}

// ---- stop on error, against the simulator ----

/// Transport wrapper recording every request.
struct Recorder<T> {
    inner: T,
    sent: Rc<RefCell<Vec<Vec<u8>>>>,
}

impl<T: Transport> Transport for Recorder<T> {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.sent.borrow_mut().push(request.to_vec());
        self.inner.transact(request, timeout)
    }
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.inner.read(timeout)
    }
}

#[test]
fn a_poll_timeout_still_stops_the_sniffer() {
    let clock = clock();
    // Requests: 1 CONNECT, 2 memory read (UID), 3 auth, 4 start, 5.. polls.
    let dev = pd_device(&clock).with_faults(FaultPlan::new().timeout_at(8));
    let sent = Rc::new(RefCell::new(Vec::new()));
    let rec = Recorder {
        inner: dev,
        sent: Rc::clone(&sent),
    };
    let mut s = Session::with_clock(rec, clock.clone());
    s.connect().unwrap();
    {
        let mut sniffer = PdSniffer::start(&mut s, PdOptions::default()).unwrap();
        let err = sniffer.run(|_| true).unwrap_err();
        assert_eq!(err, Error::Transport(TransportError::Timeout));
    }
    let last = sent.borrow().last().cloned().unwrap();
    assert_eq!(
        last[0], 0x11,
        "stop sent after the failed poll: {last:02x?}"
    );
    assert_eq!(sent.borrow().iter().filter(|r| r[0] == 0x10).count(), 1);

    // The simulator really stopped sniffing.
    clock.advance(Duration::from_secs(10));
    let sections = s.get_data(Attribute::PD_PACKET).unwrap();
    let pd = sections
        .iter()
        .find(|x| x.att == Attribute::PD_PACKET)
        .expect("PD section");
    assert_eq!(pd.payload.len(), 12);
}

// ---- AC-4PJRVK.5: the PD scenario and the measurement stream agree ----

/// How far the measurement may sit from the negotiated VBUS, V.
const AGREE_V: f64 = 0.3;

/// One poll of a capture on `scenario`, every 100 ms of simulated time, for
/// four seconds: the status block and the ADC snapshot of the same request,
/// and the queue records that came since the last poll.
fn contract_and_measurements(
    scenario: Scenario,
) -> (
    Vec<(u32, u16, f64)>,
    Vec<powerz_protocol::decode::QueueRecord>,
) {
    let clock = clock();
    let dev = device(Persona::Bulk, scenario, &clock).with_pd_scenario(PdScenario::Negotiation);
    let mut s = session(dev, &clock);
    s.connect().unwrap();
    s.start_pd_sniffer().unwrap();
    let mut capture_clock = CaptureClock::default();
    let mut polls = Vec::new();
    let mut queue = Vec::new();
    for _ in 0..40 {
        clock.advance(Duration::from_millis(100));
        let poll = s
            .poll_pd_sniffer(PdOptions { with_adc: true }, &mut capture_clock)
            .unwrap();
        let status = poll.status.expect("a status block");
        let adc = poll.adc.expect("an ADC snapshot");
        polls.push((status.time_ms, status.vbus_mv, adc.vbus_v()));
        queue.extend(s.poll_stream().unwrap().records);
    }
    s.stop_pd_sniffer().unwrap();
    (polls, queue)
}

#[test]
fn the_meter_reading_follows_the_negotiated_contract() {
    for scenario in [Scenario::Load5v1a, Scenario::Idle] {
        let (polls, _) = contract_and_measurements(scenario);
        assert!(
            polls.iter().any(|&(_, mv, _)| mv > 5_500),
            "the contract goes above 5 V: {polls:?}"
        );
        for (t, mv, measured) in polls {
            let negotiated = f64::from(mv) / 1000.0;
            assert!(
                (measured - negotiated).abs() <= AGREE_V,
                "{scenario:?} at {t} ms: {negotiated} V negotiated, {measured} V measured"
            );
        }
    }
}

#[test]
fn the_measurement_stream_follows_the_negotiated_contract() {
    let (polls, queue) = contract_and_measurements(Scenario::Load5v1a);
    // Between two polls that agree on the contract, it held all along.
    let mut checked = 0;
    for w in polls.windows(2) {
        let (from, mv, _) = w[0];
        let (to, next_mv, _) = w[1];
        if mv != next_mv {
            continue;
        }
        for r in queue.iter().filter(|r| (from..=to).contains(&r.time_ms())) {
            checked += 1;
            let negotiated = f64::from(mv) / 1000.0;
            assert!(
                (r.vbus_v() - negotiated).abs() <= AGREE_V,
                "at {} ms: {negotiated} V negotiated, {} V in the stream",
                r.time_ms(),
                r.vbus_v()
            );
        }
    }
    assert!(checked > 0, "no stream record to compare");
}
