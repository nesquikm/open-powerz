//! FR QFTCWX, AC-QFTCWX.2: the simulator runs on a caller-driven clock.
//!
//! A browser has no clock it can block on, so the web edition builds the
//! demo meter with [`SimDevice::manual`] and moves its time forward itself
//! with [`SimDevice::advance`] (milliseconds). Time on that device moves
//! only when the caller advances it.

mod common;

use common::*;
use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_protocol::frame::Attribute;
use powerz_protocol::transport::Transport;

/// The queue records of one ADC-queue poll.
fn poll(d: &mut SimDevice, id: u8) -> Vec<QueueRecord> {
    let reply = d
        .transact(&get_data(id, Attribute::ADC_QUEUE), T)
        .expect("queue reply");
    let secs = sections(&reply);
    assert_eq!(secs.len(), 1, "one ADC-queue section: {reply:02x?}");
    assert_eq!(secs[0].0, Attribute::ADC_QUEUE.bits());
    secs[0]
        .1
        .chunks(20)
        .map(|r| QueueRecord::decode(r, Rate::Sps50).expect("20-byte record"))
        .collect()
}

fn manual_connected(persona: Persona) -> SimDevice {
    raw_connect(SimDevice::manual(persona, Scenario::Load5v1a))
}

#[test]
fn advance_moves_the_queue_timestamps_by_the_given_milliseconds() {
    let mut d = manual_connected(Persona::Bulk);
    poll(&mut d, 5); // drain whatever is there at t = 0
    d.advance(1000);
    let records = poll(&mut d, 6);
    let n = records.len();
    assert!((49..=51).contains(&n), "{n} records in 1 s at 50 SPS");
    let last = records.last().expect("records").time_ms();
    assert!(
        (960..=1000).contains(&last),
        "manual uptime drives the timestamps: last {last}"
    );
    assert!(
        records
            .windows(2)
            .all(|w| w[1].time_ms() - w[0].time_ms() == 20),
        "50 SPS = 20 ms apart"
    );

    d.advance(500);
    let more = poll(&mut d, 7);
    let first = more.first().expect("records after a second advance");
    let last2 = more.last().expect("records").time_ms();
    assert!(first.time_ms() > last, "timestamps keep going up");
    assert!(
        (1460..=1500).contains(&last2),
        "a second advance adds on: last {last2}"
    );
}

#[test]
fn time_stands_still_until_the_caller_advances_it() {
    let mut d = manual_connected(Persona::Bulk);
    poll(&mut d, 5);
    d.advance(200);
    let before = poll(&mut d, 6);
    assert!(before.len() >= 9, "≈10 records after 200 ms");
    // Real time passes, simulated time does not.
    std::thread::sleep(std::time::Duration::from_millis(120));
    let idle = poll(&mut d, 7);
    assert!(
        idle.is_empty(),
        "no records without an advance, got {} (last {:?})",
        idle.len(),
        idle.last().map(QueueRecord::time_ms)
    );
}
