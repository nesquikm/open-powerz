//! AC-G0W08Z.6: the ADC queue always runs, at the meter rate (50 SPS) or at
//! the rate set by 0x0E; records carry u32 ms timestamps from real or
//! injected time, at most 63 per reply, oldest dropped on overflow, so a
//! session that polls too slowly sees gaps.

mod common;

use common::*;
use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_protocol::frame::Attribute;
use powerz_protocol::session::{Gap, Session};
use powerz_protocol::transport::Transport;

fn deltas(records: &[QueueRecord]) -> Vec<u32> {
    records
        .windows(2)
        .map(|w| w[1].time_ms() - w[0].time_ms())
        .collect()
}

#[test]
fn queue_runs_at_the_meter_rate_without_auth() {
    let (mut s, clock) = connected(Scenario::Load5v1a);
    s.poll_stream().unwrap(); // drain whatever is there at t = 0
    clock.advance(ms(1000));
    let poll = s.poll_stream().unwrap();
    assert_eq!(s.auth_level(), 0, "polling needs no auth");
    let n = poll.records.len();
    assert!((49..=51).contains(&n), "{n} records in 1 s at 50 SPS");
    assert!(
        deltas(&poll.records).iter().all(|&d| d == 20),
        "50 SPS = 20 ms apart: {:?}",
        deltas(&poll.records)
    );
    let last = poll.records.last().unwrap().time_ms();
    assert!(
        (960..=1000).contains(&last),
        "u32 ms uptime timestamps: last {last}"
    );
    assert_near("VBUS", poll.records[0].vbus_v(), 5.0, 0.25);
}

#[test]
fn queue_runs_at_the_rate_set_by_0x0e() {
    for (rate, span_ms, interval) in [
        (Rate::Sps2, 5000u64, 500u32),
        (Rate::Sps10, 1000, 100),
        (Rate::Sps1000, 50, 1),
    ] {
        let (mut s, clock) = connected(Scenario::Idle);
        s.start_stream(rate).unwrap();
        s.poll_stream().unwrap(); // drain
        clock.advance(ms(span_ms));
        let poll = s.poll_stream().unwrap();
        let n = poll.records.len() as u64;
        let want = span_ms / u64::from(interval);
        assert!(
            (want - 1..=want + 1).contains(&n),
            "{rate:?}: {n} records in {span_ms} ms, want ≈ {want}"
        );
        assert!(
            deltas(&poll.records).iter().all(|&d| d == interval),
            "{rate:?}: deltas {:?}",
            deltas(&poll.records)
        );
        assert!(poll.gaps.is_empty(), "{rate:?}: {:?}", poll.gaps);
    }
}

#[test]
fn adc_rate_byte_follows_the_queue_rate() {
    let (mut s, _clock) = connected(Scenario::Idle);
    assert_eq!(s.read_adc().unwrap().rate_code(), Some(2), "meter rate");
    s.start_stream(Rate::Sps1000).unwrap();
    assert_eq!(s.read_adc().unwrap().rate_code(), Some(3), "after 0x0E");
    s.stop_stream().unwrap();
    assert_eq!(
        s.read_adc().unwrap().rate_code(),
        Some(2),
        "after 0x0F the queue falls back to the meter rate"
    );
}

#[test]
fn one_reply_holds_at_most_63_records() {
    let (mut s, clock) = connected(Scenario::Idle);
    s.start_stream(Rate::Sps1000).unwrap();
    s.poll_stream().unwrap();
    clock.advance(ms(500));
    let poll = s.poll_stream().unwrap();
    let n = poll.records.len();
    assert!(
        (50..=63).contains(&n),
        "{n} records in one reply after 500 ms"
    );
    assert!(
        deltas(&poll.records).iter().all(|&d| d == 1),
        "one reply holds contiguous records: {:?}",
        deltas(&poll.records)
    );
    let last = poll.records.last().unwrap().time_ms();
    let reply = s
        .transport_mut()
        .transact(
            &powerz_protocol::frame::get_data(0x77, Attribute::ADC_QUEUE),
            T,
        )
        .unwrap();
    let secs = sections(&reply);
    let more = secs.first().map_or(0, |s| s.1.len() / 20);
    assert!(more <= 63, "{more} records in a raw reply");
    assert!(
        last > 100,
        "the oldest records were dropped, not the newest: last {last}"
    );
}

#[test]
fn polling_too_slowly_drops_the_oldest_records_and_shows_gaps() {
    let (mut s, clock) = connected(Scenario::Idle);
    s.start_stream(Rate::Sps1000).unwrap();
    s.poll_stream().unwrap();
    clock.advance(ms(60_000));
    let mut gaps: Vec<Gap> = Vec::new();
    let mut total = 0usize;
    for _ in 0..2000 {
        let poll = s.poll_stream().unwrap();
        assert!(poll.records.len() <= 63);
        gaps.extend(poll.gaps);
        total += poll.records.len();
        if poll.records.is_empty() {
            break;
        }
    }
    assert!(!gaps.is_empty(), "overflow must show up as a gap");
    assert!(total < 60_000, "{total} of 60000 records survived overflow");
    let newest_first_gap = gaps[0];
    assert!(newest_first_gap.to_ms > newest_first_gap.from_ms + 1);
}

#[test]
fn records_are_produced_in_real_time() {
    let mut s = Session::open(SimDevice::new(Persona::Bulk, Scenario::Idle));
    s.connect().unwrap();
    s.poll_stream().unwrap();
    std::thread::sleep(ms(400));
    let poll = s.poll_stream().unwrap();
    let n = poll.records.len();
    assert!((10..=40).contains(&n), "{n} records in ~400 ms at 50 SPS");
    assert!(deltas(&poll.records).iter().all(|&d| d == 20));
}

#[test]
fn queue_poll_reply_has_one_adc_queue_section_of_20_byte_records() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    clock.advance(ms(200));
    let reply = d.transact(&get_data(5, Attribute::ADC_QUEUE), T).unwrap();
    let secs = sections(&reply);
    assert_eq!(secs.len(), 1);
    assert_eq!(secs[0].0, Attribute::ADC_QUEUE.bits());
    assert_eq!(secs[0].1.len() % 20, 0);
    assert!(secs[0].1.len() / 20 >= 9, "≈10 records after 200 ms");
}
