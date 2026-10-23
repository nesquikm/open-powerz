//! ADC-queue streaming (FR N6EHQC): start at every rate, polls, gaps,
//! stop. Queue replies are synthetic frames built with the section layout;
//! auth exchanges come from the simulated meter in `common`.

mod common;

use std::time::Duration;

use common::*;
use powerz_protocol::decode::Rate;
use powerz_protocol::session::{Error, Gap, Session, Streamer};
use powerz_protocol::transport::mock::{MockTransport, Step};

const T0: u64 = 1_790_000_000_000;

fn interval_ms(rate: Rate) -> u32 {
    1000 / rate.sps()
}

fn times(recs: &[powerz_protocol::decode::QueueRecord]) -> Vec<u32> {
    recs.iter().map(|r| r.time_ms()).collect()
}

/// ADC snapshot reply whose header reports auth level 1 (so the session
/// knows it is authenticated without a 0x4C exchange).
fn adc_level1(id: u8) -> Vec<u8> {
    let mut f = with_id(vector(ADC_52), id);
    f[..4].copy_from_slice(&put_header(id, 1));
    f
}

/// Session already at level 1 with a stream started at `rate` (ids 1, 2);
/// `polls` follow from id 3.
fn started(rate: Rate, polls: Vec<Step>) -> Session<MockTransport> {
    let script = [
        vec![
            Step::transact(get_data(1, 0x001), adc_level1(1)),
            Step::transact([0x0E, 0x02, rate.code() << 1, 0x00], accept(2)),
        ],
        polls,
    ]
    .concat();
    let mut s = Session::open(MockTransport::new(script));
    s.read_adc().unwrap();
    assert_eq!(s.auth_level(), 1);
    s.start_stream(rate).unwrap();
    s
}

fn poll(id: u8, ts: &[u32]) -> Step {
    Step::transact(get_data(id, 0x002), queue_reply(id, 1, ts))
}

// ---- AC-N6EHQC.8 / .10: start at each rate (auth at level 0), polls, stop ----

#[test]
fn session_stream_start_poll_stop_at_every_rate() {
    for rate in Rate::ALL {
        let i = interval_ms(rate);
        let base = 50_000u32;
        let first: Vec<u32> = (0..3).map(|k| base + k * i).collect();
        let second: Vec<u32> = (3..5).map(|k| base + k * i).collect();
        let script = [
            memread_steps(1, UID_ADDR, &SYNTH_UID),
            vec![
                Step::transact([0x0E, 0x03, rate.code() << 1, 0x00], accept(3)),
                poll(4, &first),
                poll(5, &second),
                Step::transact([0x0F, 0x06, 0x00, 0x00], accept(6)),
            ],
        ]
        .concat();
        let mut s = Session::with_clock(AuthDevice::new(script, 1), FakeClock::new(T0));
        assert_eq!(s.auth_level(), 0);
        s.start_stream(rate).unwrap();
        assert_eq!(
            s.auth_level(),
            1,
            "{rate:?}: start_stream must authenticate"
        );
        assert_eq!(s.transport().auth_frames.len(), 1);

        let p1 = s.poll_stream().unwrap();
        assert_eq!(times(&p1.records), first, "{rate:?}");
        assert!(p1.gaps.is_empty(), "{rate:?}: {:?}", p1.gaps);
        let p2 = s.poll_stream().unwrap();
        assert_eq!(times(&p2.records), second, "{rate:?}");
        assert!(p2.gaps.is_empty(), "{rate:?}: {:?}", p2.gaps);

        let r = &p1.records[1];
        assert_eq!(r.vbus_v(), f64::from(5_000_000 + first[1]) / 1e6);
        assert_eq!(r.ibus_a(), -f64::from(first[1]) / 1e6);
        let cc1 = if rate == Rate::Sps2 { 0.0805 } else { 0.805 };
        assert!(
            (r.cc1_v() - cc1).abs() < 1e-12,
            "{rate:?}: cc1 {}",
            r.cc1_v()
        );

        s.stop_stream().unwrap();
        assert_eq!(s.transport().inner.remaining(), 0, "{rate:?}");
    }
}

#[test]
fn session_stream_restart_sends_stop_first() {
    let mut s = started(
        Rate::Sps10,
        vec![
            Step::transact([0x0F, 0x03, 0x00, 0x00], accept(3)),
            Step::transact([0x0E, 0x04, 0x06, 0x00], accept(4)),
        ],
    );
    s.start_stream(Rate::Sps1000).unwrap();
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn session_stream_start_after_stop_sends_no_extra_stop() {
    let mut s = started(
        Rate::Sps50,
        vec![
            Step::transact([0x0F, 0x03, 0x00, 0x00], accept(3)),
            Step::transact([0x0E, 0x04, 0x00, 0x00], accept(4)),
        ],
    );
    s.stop_stream().unwrap();
    s.start_stream(Rate::Sps2).unwrap();
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn session_stream_start_reject_is_rejected_error() {
    let mut s = Session::open(MockTransport::new(vec![
        Step::transact(get_data(1, 0x001), adc_level1(1)),
        Step::transact([0x0E, 0x02, 0x02, 0x00], reject(2)),
    ]));
    s.read_adc().unwrap();
    assert_eq!(
        s.start_stream(Rate::Sps10),
        Err(Error::Rejected { request_type: 0x0E })
    );
}

#[test]
fn session_stream_stop_sends_0f() {
    let mut s = started(
        Rate::Sps2,
        vec![Step::transact([0x0F, 0x03, 0x00, 0x00], accept(3))],
    );
    s.stop_stream().unwrap();
    assert_eq!(
        s.transport().written().last().unwrap(),
        &vec![0x0F, 0x03, 0x00, 0x00]
    );
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn session_stream_poll_without_auth_returns_records() {
    let mut s = Session::open(MockTransport::new(vec![Step::transact(
        get_data(1, 0x002),
        queue_reply(1, 0, &[700, 1200, 1700]),
    )]));
    let p = s.poll_stream().unwrap();
    assert_eq!(times(&p.records), vec![700, 1200, 1700]);
    assert_eq!(p.records[2].vbus_v(), f64::from(5_001_700) / 1e6);
    assert_eq!(s.auth_level(), 0);
    assert!(s.transport().written().iter().all(|f| f[0] == 0x0C));
}

// ---- AC-N6EHQC.9: ordering, gaps, Streamer ----

#[test]
fn session_stream_records_come_back_in_device_time_order() {
    let mut s = started(Rate::Sps10, vec![poll(3, &[1200, 1000, 1100])]);
    let p = s.poll_stream().unwrap();
    assert_eq!(times(&p.records), vec![1000, 1100, 1200]);
    assert!(p.gaps.is_empty());
}

#[test]
fn session_stream_gap_inside_one_poll_is_reported_not_filled() {
    let mut s = started(Rate::Sps10, vec![poll(3, &[1000, 1100, 1500, 1600])]);
    let p = s.poll_stream().unwrap();
    assert_eq!(times(&p.records), vec![1000, 1100, 1500, 1600]);
    assert_eq!(
        p.gaps,
        vec![Gap {
            from_ms: 1100,
            to_ms: 1500,
            expected: 4,
            received: 1
        }]
    );
}

#[test]
fn session_stream_gap_between_polls_is_reported() {
    let mut s = started(
        Rate::Sps10,
        vec![poll(3, &[1000, 1100]), poll(4, &[1400, 1500])],
    );
    assert!(s.poll_stream().unwrap().gaps.is_empty());
    let p = s.poll_stream().unwrap();
    assert_eq!(times(&p.records), vec![1400, 1500]);
    assert_eq!(
        p.gaps,
        vec![Gap {
            from_ms: 1100,
            to_ms: 1400,
            expected: 3,
            received: 1
        }]
    );
}

#[test]
fn session_stream_no_gap_across_contiguous_polls() {
    let mut s = started(
        Rate::Sps50,
        vec![poll(3, &[1000, 1020]), poll(4, &[1040, 1060])],
    );
    assert!(s.poll_stream().unwrap().gaps.is_empty());
    assert!(s.poll_stream().unwrap().gaps.is_empty());
}

#[test]
fn session_stream_gap_at_1000_sps() {
    let mut s = started(Rate::Sps1000, vec![poll(3, &[10, 11, 12, 14, 15])]);
    let p = s.poll_stream().unwrap();
    assert_eq!(p.records.len(), 5);
    assert_eq!(
        p.gaps,
        vec![Gap {
            from_ms: 12,
            to_ms: 14,
            expected: 2,
            received: 1
        }]
    );
}

#[test]
fn session_stream_disconnect_poll_is_empty() {
    let mut s = started(
        Rate::Sps10,
        vec![Step::transact(
            get_data(3, 0x002),
            vec![0x03, 0x03, 0x00, 0x00],
        )],
    );
    let p = s.poll_stream().unwrap();
    assert!(p.records.is_empty());
    assert!(p.gaps.is_empty());
}

#[test]
fn session_streamer_poll_period_keeps_up_with_each_rate() {
    let fast = Streamer::poll_period(Rate::Sps1000);
    assert!(
        fast > Duration::ZERO && fast <= Duration::from_millis(30),
        "{fast:?}"
    );
    for rate in [Rate::Sps2, Rate::Sps10, Rate::Sps50] {
        let p = Streamer::poll_period(rate);
        assert!(
            p > Duration::ZERO && p <= Duration::from_millis(200),
            "{rate:?}: {p:?}"
        );
    }
}

// ---- Hardening: meter rate for unauthenticated polls, Streamer loop ----

#[test]
fn session_stream_unknown_meter_rate_reports_no_gaps() {
    let mut s = Session::open(MockTransport::new(vec![Step::transact(
        get_data(1, 0x002),
        queue_reply(1, 0, &[700, 1200, 1700]),
    )]));
    let p = s.poll_stream().unwrap();
    assert_eq!(p.records.len(), 3);
    assert!(p.gaps.is_empty(), "no rate known, so no gap can be claimed");
}

#[test]
fn session_stream_unauthenticated_poll_uses_meter_rate_from_adc() {
    // The captured ADC snapshot reports queue rate code 2 (50 SPS).
    let mut s = Session::open(MockTransport::new(vec![
        Step::transact(get_data(1, 0x001), with_id(vector(ADC_52), 1)),
        Step::transact(get_data(2, 0x002), queue_reply(2, 0, &[1000, 1020, 1040])),
        Step::transact(get_data(3, 0x002), queue_reply(3, 0, &[1100])),
    ]));
    s.read_adc().unwrap();
    assert!(s.poll_stream().unwrap().gaps.is_empty());
    let p = s.poll_stream().unwrap();
    assert_eq!(
        p.gaps,
        vec![Gap {
            from_ms: 1040,
            to_ms: 1100,
            expected: 3,
            received: 1
        }]
    );
}

#[test]
fn session_streamer_run_polls_until_callback_stops_and_sleeps_between() {
    let clock = FakeClock::new(T0);
    let mut s = Session::with_clock(
        MockTransport::new(vec![
            Step::transact(get_data(1, 0x002), queue_reply(1, 0, &[1000])),
            Step::transact(get_data(2, 0x002), queue_reply(2, 0, &[1100])),
            Step::transact(get_data(3, 0x002), queue_reply(3, 0, &[1200])),
        ]),
        clock.clone(),
    );
    let mut seen = Vec::new();
    Streamer::run(&mut s, Rate::Sps10, |poll| {
        seen.extend(times(&poll.records));
        seen.len() < 3
    })
    .unwrap();
    assert_eq!(seen, vec![1000, 1100, 1200]);
    assert_eq!(clock.total_slept(), Streamer::poll_period(Rate::Sps10) * 2);
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn session_meter_rate_code_4_behaves_like_code_0() {
    // FW 2.0.6 accepts rate code 4 but runs it like code 0 (2 SPS, 0.1 mV).
    let mut adc = with_id(vector(ADC_52), 1);
    adc[8 + 0x26] = 4;
    let mut s = Session::open(MockTransport::new(vec![
        Step::transact(get_data(1, 0x001), adc),
        Step::transact(get_data(2, 0x002), queue_reply(2, 0, &[1000, 1500])),
    ]));
    s.read_adc().unwrap();
    let p = s.poll_stream().unwrap();
    assert!(p.gaps.is_empty(), "500 ms steps are contiguous at 2 SPS");
    assert_eq!(
        p.records[0].cc1_v(),
        805.0 / 10_000.0,
        "0.1 mV units at code 0/4"
    );
}

// ---- AC-DGSQ6S.12: a queue reply shorter than it declares ----

/// A queue reply for `times` whose last `cut` bytes never arrived: the
/// section header still declares every record.
fn short_queue_reply(id: u8, level: u8, times: &[u32], cut: usize) -> Vec<u8> {
    let mut f = queue_reply(id, level, times);
    f.truncate(f.len() - cut);
    f
}

/// Lost samples the gaps account for (`expected - received` per gap).
fn lost(gaps: &[Gap]) -> u32 {
    gaps.iter().map(|g| g.expected - g.received).sum()
}

#[test]
fn session_stream_short_backlog_reply_keeps_whole_records_and_reports_the_lost_tail() {
    // AC-DGSQ6S.12 [device]: FW 2.0.6 at 50 SPS after a 70 s pause: the
    // queue section declares 63 records (1260 bytes) but 1208 arrive.
    let backlog: Vec<u32> = (0..63).map(|k| 1060 + k * 20).collect();
    let mut s = started(
        Rate::Sps50,
        vec![
            poll(3, &[1000, 1020, 1040]),
            Step::transact(get_data(4, 0x002), short_queue_reply(4, 1, &backlog, 52)),
            poll(5, &[2320, 2340]),
        ],
    );
    assert!(s.poll_stream().unwrap().gaps.is_empty());

    let short = s
        .poll_stream()
        .expect("AC-DGSQ6S.12: a short backlog reply is not fatal");
    // 1208 bytes = 60 whole records + 8 bytes of the 61st, which is dropped.
    assert_eq!(times(&short.records), backlog[..60].to_vec());
    assert_eq!(
        short.records[59].vbus_v(),
        f64::from(5_000_000 + 2240) / 1e6
    );

    let next = s
        .poll_stream()
        .expect("AC-DGSQ6S.12: the stream carries on after a short reply");
    assert_eq!(times(&next.records), vec![2320, 2340]);

    // The three records that never arrived (2260, 2280, 2300) are reported
    // as a gap after the last whole record, and counted once.
    let gaps = [short.gaps.clone(), next.gaps.clone()].concat();
    assert!(
        gaps.iter().any(|g| g.from_ms == 2240),
        "AC-DGSQ6S.12: no gap after the last whole record: {gaps:?}"
    );
    assert_eq!(lost(&gaps), 3, "AC-DGSQ6S.12: {gaps:?}");
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn session_stream_short_reply_without_a_known_rate_keeps_whole_records() {
    // AC-DGSQ6S.12: an unauthenticated poll, no rate known: whole records
    // only, no error, and no gap claimed.
    let mut s = Session::open(MockTransport::new(vec![
        Step::transact(
            get_data(1, 0x002),
            short_queue_reply(1, 0, &[700, 1200, 1700, 2200], 30),
        ),
        Step::transact(get_data(2, 0x002), queue_reply(2, 0, &[3200])),
    ]));
    let p = s
        .poll_stream()
        .expect("AC-DGSQ6S.12: a short reply is not fatal");
    // 80 - 30 = 50 bytes: two whole records.
    assert_eq!(times(&p.records), vec![700, 1200]);
    assert!(p.gaps.is_empty(), "{:?}", p.gaps);
    let p = s.poll_stream().expect("the next poll works");
    assert_eq!(times(&p.records), vec![3200]);
}

#[test]
fn session_stream_short_reply_with_no_whole_record_is_an_empty_poll() {
    // AC-DGSQ6S.12: fewer than 20 bytes of a declared record.
    let mut s = started(
        Rate::Sps10,
        vec![
            Step::transact(get_data(3, 0x002), short_queue_reply(3, 1, &[1000], 8)),
            poll(4, &[1100]),
        ],
    );
    let p = s
        .poll_stream()
        .expect("AC-DGSQ6S.12: a short reply is not fatal");
    assert!(p.records.is_empty(), "{:?}", times(&p.records));
    let p = s.poll_stream().expect("the next poll works");
    assert_eq!(times(&p.records), vec![1100]);
}

#[test]
fn session_short_section_of_another_attribute_still_fails() {
    // AC-DGSQ6S.12: only the ADC queue gets the leniency; a cut ADC
    // snapshot is still a frame error.
    let mut adc = with_id(vector(ADC_52), 1);
    adc.truncate(adc.len() - 8);
    let mut s = Session::open(MockTransport::new(vec![Step::transact(
        get_data(1, 0x001),
        adc,
    )]));
    let err = s.read_adc().expect_err("a short ADC section is an error");
    assert!(
        matches!(
            err,
            Error::Frame(powerz_protocol::frame::FrameError::SectionOverrun { att: 0x001, .. })
        ),
        "{err:?}"
    );
}
