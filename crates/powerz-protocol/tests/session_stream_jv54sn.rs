//! FR JV54SN, AC .4: when the queue's records keep arriving at one steady
//! interval other than the one the session expects (a settings write
//! re-applied the meter's own rate to the running queue `[inferred]`), the
//! session switches to the rate of that interval after five such records in
//! a row, reports the switch once on the poll (`StreamPoll::rate_switch`)
//! and decodes with the new rate; it no longer reports a gap for every
//! record. A real hole in the stream is still a gap, before and after a
//! switch, and a steady interval that matches no known rate is no switch.

mod common;

use common::*;
use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_protocol::session::{Gap, Session};
use powerz_protocol::transport::mock::{MockTransport, Step};

/// ADC snapshot reply whose header reports auth level 1.
fn adc_level1(id: u8) -> Vec<u8> {
    let mut f = with_id(vector(ADC_52), id);
    f[..4].copy_from_slice(&put_header(id, 1));
    f
}

/// Session at level 1 with a stream started at `rate` (ids 1, 2); queue
/// polls answering `polls` follow from id 3.
fn started(rate: Rate, polls: &[Vec<u32>]) -> Session<MockTransport> {
    let mut script = vec![
        Step::transact(get_data(1, 0x001), adc_level1(1)),
        Step::transact([0x0E, 0x02, rate.code() << 1, 0x00], accept(2)),
    ];
    for (k, ts) in polls.iter().enumerate() {
        let id = 3 + k as u8;
        script.push(Step::transact(get_data(id, 0x002), queue_reply(id, 1, ts)));
    }
    let mut s = Session::open(MockTransport::new(script));
    s.read_adc().unwrap();
    s.start_stream(rate).unwrap();
    s
}

/// `n` device times from `start`, `step` ms apart.
fn steady(start: u32, step: u32, n: u32) -> Vec<u32> {
    (0..n).map(|k| start + k * step).collect()
}

/// Times from consecutive deltas, starting at `start`.
fn from_deltas(start: u32, deltas: &[u32]) -> Vec<u32> {
    let mut t = start;
    let mut out = vec![t];
    for d in deltas {
        t += d;
        out.push(t);
    }
    out
}

fn times(recs: &[QueueRecord]) -> Vec<u32> {
    recs.iter().map(QueueRecord::time_ms).collect()
}

/// Every poll's gaps and rate switches, in poll order.
fn run(s: &mut Session<MockTransport>, polls: usize) -> (Vec<Gap>, Vec<Option<Rate>>) {
    let mut gaps = Vec::new();
    let mut switches = Vec::new();
    for _ in 0..polls {
        let p = s.poll_stream().unwrap();
        gaps.extend(p.gaps);
        switches.push(p.rate_switch);
    }
    (gaps, switches)
}

#[test]
fn jv54sn_steady_100_ms_records_at_50_sps_switch_to_10_sps_after_five() {
    let fast = steady(10_000, 20, 10);
    let last_fast = *fast.last().unwrap();
    // The settings write: from here on the meter serves 10 SPS.
    let slow = steady(last_fast + 100, 100, 20);
    let mut s = started(Rate::Sps50, &[fast.clone(), slow.clone()]);

    let p1 = s.poll_stream().unwrap();
    assert_eq!(times(&p1.records), fast);
    assert!(p1.gaps.is_empty(), "{:?}", p1.gaps);
    assert_eq!(p1.rate_switch, None);

    let p2 = s.poll_stream().unwrap();
    assert_eq!(times(&p2.records), slow, "every record is kept");
    assert_eq!(
        p2.rate_switch,
        Some(Rate::Sps10),
        "five steady 100 ms steps make a switch to 10 SPS"
    );
    assert!(
        p2.gaps.len() <= 5,
        "a gap for at most the five records before the switch, not for all {}: {:?}",
        slow.len(),
        p2.gaps
    );
    // Nothing after the fifth slow record is a gap.
    let fifth = slow[4];
    assert!(
        p2.gaps.iter().all(|g| g.to_ms <= fifth),
        "gaps after the switch: {:?}",
        p2.gaps
    );
}

#[test]
fn jv54sn_after_the_switch_later_polls_report_no_gap_and_no_second_switch() {
    let fast = steady(10_000, 20, 5);
    let slow1 = steady(fast[4] + 100, 100, 8);
    let slow2 = steady(slow1[7] + 100, 100, 10);
    let slow3 = steady(slow2[9] + 100, 100, 10);
    let mut s = started(Rate::Sps50, &[fast, slow1, slow2, slow3]);
    let (_, switches) = run(&mut s, 2);
    assert_eq!(switches, vec![None, Some(Rate::Sps10)]);
    for k in 0..2 {
        let p = s.poll_stream().unwrap();
        assert!(p.gaps.is_empty(), "poll {k} after the switch: {:?}", p.gaps);
        assert_eq!(p.rate_switch, None, "the switch is reported once");
        assert_eq!(p.records.len(), 10);
    }
}

#[test]
fn jv54sn_the_five_records_may_span_several_polls() {
    let fast = steady(10_000, 20, 5);
    let mut polls = vec![fast.clone()];
    let mut t = fast[4];
    for _ in 0..6 {
        let pair = steady(t + 100, 100, 2);
        t = pair[1];
        polls.push(pair);
    }
    let mut s = started(Rate::Sps50, &polls);
    let (gaps, switches) = run(&mut s, polls.len());
    let switched: Vec<Rate> = switches.iter().flatten().copied().collect();
    assert_eq!(switched, vec![Rate::Sps10], "one switch: {switches:?}");
    assert!(
        gaps.len() <= 5,
        "12 slow records, at most 5 gaps before the switch: {gaps:?}"
    );
}

#[test]
fn jv54sn_small_jitter_around_the_interval_still_switches() {
    let fast = steady(10_000, 20, 5);
    let slow = from_deltas(fast[4] + 101, &[98, 102, 100, 99, 101, 100, 97, 103]);
    let mut s = started(Rate::Sps50, &[fast, slow]);
    let (gaps, switches) = run(&mut s, 2);
    assert_eq!(switches, vec![None, Some(Rate::Sps10)]);
    assert!(gaps.len() <= 5, "{gaps:?}");
}

#[test]
fn jv54sn_steady_500_ms_records_switch_to_2_sps_and_decode_with_its_scale() {
    let fast = steady(10_000, 20, 5);
    let slow = steady(fast[4] + 500, 500, 8);
    let later = steady(slow[7] + 500, 500, 4);
    let mut s = started(Rate::Sps50, &[fast, slow, later]);
    let (_, switches) = run(&mut s, 2);
    assert_eq!(switches, vec![None, Some(Rate::Sps2)]);
    let p = s.poll_stream().unwrap();
    assert!(p.gaps.is_empty(), "{:?}", p.gaps);
    // At 2 SPS CC and D± come in 0.1 mV units (`QueueRecord::decode`).
    let cc1 = p.records[0].cc1_v();
    assert!(
        (cc1 - 0.0805).abs() < 1e-12,
        "records after a switch to 2 SPS decode at its scale: cc1 {cc1}"
    );
}

#[test]
fn jv54sn_a_real_hole_at_the_expected_rate_is_still_a_gap_and_no_switch() {
    let before = steady(10_000, 20, 10);
    // One second missing, then the stream goes on at 50 SPS.
    let after = steady(before[9] + 1_000, 20, 20);
    let mut s = started(Rate::Sps50, &[before.clone(), after.clone()]);
    let (gaps, switches) = run(&mut s, 2);
    assert_eq!(switches, vec![None, None]);
    assert_eq!(
        gaps,
        vec![Gap {
            from_ms: before[9],
            to_ms: after[0],
            expected: 50,
            received: 1,
        }]
    );
}

#[test]
fn jv54sn_a_real_hole_after_the_switch_is_a_gap_at_the_new_interval() {
    let fast = steady(10_000, 20, 5);
    let slow = steady(fast[4] + 100, 100, 10);
    // Two seconds missing, then 10 SPS again.
    let after = steady(slow[9] + 2_000, 100, 5);
    let mut s = started(Rate::Sps50, &[fast, slow.clone(), after.clone()]);
    let (_, switches) = run(&mut s, 2);
    assert_eq!(switches, vec![None, Some(Rate::Sps10)]);
    let p = s.poll_stream().unwrap();
    assert_eq!(
        p.gaps,
        vec![Gap {
            from_ms: slow[9],
            to_ms: after[0],
            expected: 20,
            received: 1,
        }],
        "a hole after the switch is measured at 100 ms"
    );
}

#[test]
fn jv54sn_a_steady_interval_of_no_known_rate_is_no_switch() {
    let fast = steady(10_000, 20, 5);
    let odd = steady(fast[4] + 37, 37, 12);
    let mut s = started(Rate::Sps50, &[fast, odd.clone()]);
    let (gaps, switches) = run(&mut s, 2);
    assert_eq!(switches, vec![None, None]);
    assert_eq!(
        gaps.len(),
        odd.len(),
        "37 ms matches no rate: every step stays a gap: {gaps:?}"
    );
}

#[test]
fn jv54sn_four_slow_records_then_a_normal_one_reset_the_count() {
    let fast = steady(10_000, 20, 5);
    // 4 slow, 1 normal, 4 slow: never five in a row.
    let mut deltas = vec![100, 100, 100, 100, 20, 100, 100, 100, 100];
    deltas.extend([20, 20, 20]);
    let mixed = from_deltas(fast[4] + 100, &deltas[1..]);
    let mut s = started(Rate::Sps50, &[fast, mixed]);
    let (gaps, switches) = run(&mut s, 2);
    assert_eq!(switches, vec![None, None]);
    assert_eq!(gaps.len(), 8, "each slow step is a gap: {gaps:?}");
}

#[test]
fn jv54sn_holes_of_varying_length_are_no_steady_interval() {
    let fast = steady(10_000, 20, 5);
    // Holes, not a rate: 100, 500, 100, 500, 100, 500 ms.
    let holes = from_deltas(fast[4] + 100, &[500, 100, 500, 100, 500]);
    let mut s = started(Rate::Sps50, &[fast, holes.clone()]);
    let (gaps, switches) = run(&mut s, 2);
    assert_eq!(switches, vec![None, None]);
    assert_eq!(gaps.len(), holes.len(), "{gaps:?}");
}
