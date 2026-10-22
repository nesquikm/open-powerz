//! PD sniffer session (FR TGE83A, AC .1, .2, .5): start (0x10, mode 1)
//! with auth to level 1 when needed, REJECT as a typed error, PD_PACKET
//! polling (optionally with ADC) every ~5 ms, stop (0x11) on `stop`, on
//! drop and after errors, and capture-relative record times. PD replies are
//! synthetic; auth exchanges come from the simulated meter in `common`.

mod common;

use std::time::Duration;

use common::*;
use powerz_protocol::decode::AdcSnapshot;
use powerz_protocol::decode::pd_packet::{PdRecordKind, PdStatus};
use powerz_protocol::frame::{Attribute, SectionHeader};
use powerz_protocol::session::{Error, PdOptions, PdSniffer, Session};
use powerz_protocol::transport::Error as TransportError;
use powerz_protocol::transport::mock::{MockTransport, Step};

const T0: u64 = 1_790_000_000_000;

fn start_req(id: u8) -> Vec<u8> {
    vec![0x10, id, 0x02, 0x00]
}

fn stop_req(id: u8) -> Vec<u8> {
    vec![0x11, id, 0x00, 0x00]
}

/// 12-byte status block.
fn status(ts: u32, vbus_mv: u16, ibus_ma: i16, cc1_mv: u16, cc2_mv: u16) -> Vec<u8> {
    let mut s = Vec::new();
    s.extend(ts.to_le_bytes());
    s.extend(vbus_mv.to_le_bytes());
    s.extend(ibus_ma.to_le_bytes());
    s.extend(cc1_mv.to_le_bytes());
    s.extend(cc2_mv.to_le_bytes());
    s
}

/// PD message record from the charger (SOP, no data objects).
fn ctrl_msg(ts: u32, header: u16) -> Vec<u8> {
    let mut r = vec![0x87];
    r.extend(ts.to_le_bytes());
    r.push(0x00);
    r.extend(header.to_le_bytes());
    r
}

/// PUT_DATA reply (auth level 1) with one PD_PACKET section holding
/// `payload` (status block + records).
fn pd_reply(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = put_header(id, 1).to_vec();
    f.extend(
        SectionHeader {
            att: 0x010,
            next: false,
            chunk: 0,
            size: payload.len() as u16,
        }
        .encode(),
    );
    f.extend(payload);
    f
}

fn pd_poll(id: u8, payload: &[u8]) -> Step {
    Step::transact(get_data(id, 0x010), pd_reply(id, payload))
}

/// ADC snapshot reply whose header reports auth level 1.
fn adc_level1(id: u8) -> Vec<u8> {
    let mut f = with_id(vector(ADC_52), id);
    f[..4].copy_from_slice(&put_header(id, 1));
    f
}

/// Session already at level 1 (ADC read with id 1); `steps` follow from id 2.
fn level1(steps: Vec<Step>) -> Session<MockTransport, FakeClock> {
    let script = [
        vec![Step::transact(get_data(1, 0x001), adc_level1(1))],
        steps,
    ]
    .concat();
    let mut s = Session::with_clock(MockTransport::new(script), FakeClock::new(T0));
    s.read_adc().unwrap();
    assert_eq!(s.auth_level(), 1);
    s
}

fn count_type(written: &[Vec<u8>], ty: u8) -> usize {
    written.iter().filter(|w| w.first() == Some(&ty)).count()
}

// ---- AC-TGE83A.1: start / stop, auth, REJECT ----

#[test]
fn start_authenticates_at_level_0_then_sends_0x10_mode_1() {
    let script = [
        memread_steps(1, UID_ADDR, &SYNTH_UID),
        vec![
            Step::transact(start_req(3), accept(3)),
            Step::transact(stop_req(4), accept(4)),
        ],
    ]
    .concat();
    let mut s = Session::with_clock(AuthDevice::new(script, 1), FakeClock::new(T0));
    assert_eq!(s.auth_level(), 0);
    let sniffer = PdSniffer::start(&mut s, PdOptions::default()).unwrap();
    sniffer.stop().unwrap();
    assert_eq!(s.auth_level(), 1, "start must authenticate to level 1");
    assert_eq!(s.transport().auth_frames.len(), 1);
    assert_eq!(s.transport().inner.remaining(), 0);
}

#[test]
fn start_at_level_1_sends_no_auth() {
    let mut s = level1(vec![
        Step::transact(start_req(2), accept(2)),
        Step::transact(stop_req(3), accept(3)),
    ]);
    let sniffer = PdSniffer::start(&mut s, PdOptions::default()).unwrap();
    sniffer.stop().unwrap();
    assert_eq!(s.transport().remaining(), 0);
    assert_eq!(count_type(s.transport().written(), 0x4C), 0);
    assert_eq!(count_type(s.transport().written(), 0x44), 0);
}

#[test]
fn start_reject_is_a_typed_error_and_sends_no_stop() {
    let mut s = level1(vec![Step::transact(start_req(2), reject(2))]);
    match PdSniffer::start(&mut s, PdOptions::default()) {
        Err(Error::Rejected { request_type }) => assert_eq!(request_type, 0x10),
        Err(other) => panic!("expected Rejected, got {other:?}"),
        Ok(_) => panic!("expected Rejected, got a running sniffer"),
    }
    assert_eq!(s.transport().remaining(), 0);
    assert_eq!(count_type(s.transport().written(), 0x11), 0);
}

#[test]
fn stop_reject_is_a_typed_error() {
    let mut s = level1(vec![
        Step::transact(start_req(2), accept(2)),
        Step::transact(stop_req(3), reject(3)),
    ]);
    let sniffer = PdSniffer::start(&mut s, PdOptions::default()).unwrap();
    assert_eq!(sniffer.stop(), Err(Error::Rejected { request_type: 0x11 }));
}

#[test]
fn dropping_the_sniffer_sends_stop() {
    let mut s = level1(vec![
        Step::transact(start_req(2), accept(2)),
        pd_poll(3, &status(100, 5_000, 0, 1_500, 0)),
        Step::transact(stop_req(4), accept(4)),
    ]);
    {
        let mut sniffer = PdSniffer::start(&mut s, PdOptions::default()).unwrap();
        sniffer.poll().unwrap();
    }
    assert_eq!(s.transport().remaining(), 0, "0x11 sent on drop");
    assert_eq!(s.transport().written().last(), Some(&stop_req(4)));
}

#[test]
fn explicit_stop_is_not_repeated_on_drop() {
    let mut s = level1(vec![
        Step::transact(start_req(2), accept(2)),
        Step::transact(stop_req(3), accept(3)),
    ]);
    let sniffer = PdSniffer::start(&mut s, PdOptions::default()).unwrap();
    sniffer.stop().unwrap();
    assert_eq!(count_type(s.transport().written(), 0x11), 1);
}

#[test]
fn a_failed_poll_still_leads_to_stop_on_drop() {
    let mut s = level1(vec![
        Step::transact(start_req(2), accept(2)),
        Step::transact_err(get_data(3, 0x010), TransportError::Timeout),
        Step::transact(stop_req(4), accept(4)),
    ]);
    {
        let mut sniffer = PdSniffer::start(&mut s, PdOptions::default()).unwrap();
        assert_eq!(
            sniffer.poll().unwrap_err(),
            Error::Transport(TransportError::Timeout)
        );
    }
    assert_eq!(s.transport().remaining(), 0);
    assert_eq!(s.transport().written().last(), Some(&stop_req(4)));
}

#[test]
fn a_failed_run_still_leads_to_stop_on_drop() {
    let mut s = level1(vec![
        Step::transact(start_req(2), accept(2)),
        pd_poll(3, &status(100, 5_000, 0, 1_500, 0)),
        Step::transact_err(get_data(4, 0x010), TransportError::Disconnected),
        Step::transact(stop_req(5), accept(5)),
    ]);
    {
        let mut sniffer = PdSniffer::start(&mut s, PdOptions::default()).unwrap();
        let err = sniffer.run(|_| true).unwrap_err();
        assert_eq!(err, Error::Transport(TransportError::Disconnected));
    }
    assert_eq!(s.transport().written().last(), Some(&stop_req(5)));
}

// ---- AC-TGE83A.2: polling ----

#[test]
fn poll_requests_pd_packet_and_parses_the_status_block() {
    let mut s = level1(vec![
        Step::transact(start_req(2), accept(2)),
        pd_poll(3, &status(0x02F7_0B5C, 5_182, -250, 1_528, 3)),
        Step::transact(stop_req(4), accept(4)),
    ]);
    let mut sniffer = PdSniffer::start(&mut s, PdOptions::default()).unwrap();
    let poll = sniffer.poll().unwrap();
    assert_eq!(
        poll.status,
        Some(PdStatus {
            time_ms: 0x02F7_0B5C,
            vbus_mv: 5_182,
            ibus_ma: -250,
            cc1_mv: 1_528,
            cc2_mv: 3,
        })
    );
    assert!(poll.records.is_empty());
    assert_eq!(poll.skipped, 0);
    assert!(poll.adc.is_none(), "ADC was not asked for");
    sniffer.stop().unwrap();
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn poll_with_adc_combines_the_masks_and_returns_the_snapshot() {
    let adc_payload = vector(ADC_52)[8..].to_vec();
    let pd = status(500, 9_000, 1_000, 1_100, 0);
    let mut reply = put_header(3, 1).to_vec();
    reply.extend(
        SectionHeader {
            att: 0x001,
            next: true,
            chunk: 0,
            size: adc_payload.len() as u16,
        }
        .encode(),
    );
    reply.extend(&adc_payload);
    reply.extend(
        SectionHeader {
            att: 0x010,
            next: false,
            chunk: 0,
            size: pd.len() as u16,
        }
        .encode(),
    );
    reply.extend(&pd);

    let mut s = level1(vec![
        Step::transact(start_req(2), accept(2)),
        Step::transact(get_data(3, 0x011), reply),
        Step::transact(stop_req(4), accept(4)),
    ]);
    let mut sniffer = PdSniffer::start(&mut s, PdOptions { with_adc: true }).unwrap();
    let poll = sniffer.poll().unwrap();
    let want = AdcSnapshot::decode(&adc_payload).unwrap();
    let adc = poll.adc.expect("ADC snapshot in the poll");
    assert_eq!(adc.vbus_v(), want.vbus_v());
    assert_eq!(adc.ibus_a(), want.ibus_a());
    assert_eq!(poll.status.map(|s| s.vbus_mv), Some(9_000));
    sniffer.stop().unwrap();
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn pd_packet_attribute_is_0x010() {
    assert_eq!(Attribute::PD_PACKET.bits(), 0x010);
}

#[test]
fn run_polls_about_every_5_ms() {
    assert_eq!(
        PdSniffer::<MockTransport, FakeClock>::POLL_PERIOD,
        Duration::from_millis(5)
    );
    let polls: Vec<Step> = (3..=6)
        .map(|id| pd_poll(id, &status(u32::from(id) * 5, 5_000, 0, 1_500, 0)))
        .collect();
    let clock = FakeClock::new(T0);
    let script = [
        vec![
            Step::transact(get_data(1, 0x001), adc_level1(1)),
            Step::transact(start_req(2), accept(2)),
        ],
        polls,
        vec![Step::transact(stop_req(7), accept(7))],
    ]
    .concat();
    let mut s = Session::with_clock(MockTransport::new(script), clock.clone());
    s.read_adc().unwrap();
    let mut sniffer = PdSniffer::start(&mut s, PdOptions::default()).unwrap();
    let mut seen = 0;
    sniffer
        .run(|_| {
            seen += 1;
            seen < 4
        })
        .unwrap();
    sniffer.stop().unwrap();
    assert_eq!(seen, 4);
    let slept = clock.0.borrow().slept.clone();
    assert!(slept.len() >= 3, "a sleep between polls: {slept:?}");
    assert!(
        slept.iter().all(|d| *d == Duration::from_millis(5)),
        "status-only polls are 5 ms apart: {slept:?}"
    );
    assert_eq!(s.transport().remaining(), 0);
}

// ---- AC-TGE83A.5: capture-relative record times ----

#[test]
fn records_carry_ms_since_capture_start_and_survive_a_clock_restart() {
    let first = [
        status(5_000, 5_000, 0, 1_500, 0),
        ctrl_msg(5_000, 0x03A3),
        ctrl_msg(5_100, 0x05A6),
    ]
    .concat();
    // The meter rebooted: its clock restarts near zero.
    let second = [
        status(300, 5_000, 0, 1_500, 0),
        ctrl_msg(200, 0x03A3),
        ctrl_msg(300, 0x05A6),
    ]
    .concat();
    let mut s = level1(vec![
        Step::transact(start_req(2), accept(2)),
        pd_poll(3, &first),
        pd_poll(4, &second),
        Step::transact(stop_req(5), accept(5)),
    ]);
    let mut sniffer = PdSniffer::start(&mut s, PdOptions::default()).unwrap();
    let p1 = sniffer.poll().unwrap();
    let p2 = sniffer.poll().unwrap();
    sniffer.stop().unwrap();

    let e1: Vec<u64> = p1.records.iter().map(|r| r.elapsed_ms).collect();
    assert_eq!(e1, [0, 100]);
    // Device times stay as they came.
    assert_eq!(p1.records[1].record.time_ms, 5_100);
    assert!(matches!(
        p1.records[0].record.kind,
        PdRecordKind::Message(_)
    ));

    let e2: Vec<u64> = p2.records.iter().map(|r| r.elapsed_ms).collect();
    assert!(e2[0] >= 100, "never backwards: {e1:?} then {e2:?}");
    assert_eq!(e2[1] - e2[0], 100, "spacing after the restart is kept");
    assert_eq!(p2.records[0].record.time_ms, 200);
}

#[test]
fn a_pd_section_shorter_than_the_status_block_is_skipped_not_fatal() {
    let mut s = level1(vec![
        Step::transact(start_req(2), accept(2)),
        pd_poll(3, &[0x01, 0x02, 0x03]),
        pd_poll(4, &status(0x0100, 5_000, 0, 0, 0)),
        Step::transact(stop_req(5), accept(5)),
    ]);
    let mut sniffer = PdSniffer::start(&mut s, PdOptions::default()).unwrap();
    let bad = sniffer
        .poll()
        .expect("a short PD section ended the capture");
    assert_eq!(bad.status, None);
    assert_eq!(bad.skipped, 1);
    let good = sniffer.poll().unwrap();
    assert_eq!(good.status.map(|st| st.vbus_mv), Some(5_000));
    sniffer.stop().unwrap();
    assert_eq!(s.transport().remaining(), 0);
}
