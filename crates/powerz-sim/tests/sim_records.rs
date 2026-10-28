//! AC-1HHT00.6: the demo meter keeps a few synthetic offline records. The
//! RecordList (GET_DATA 0x200) is served only at auth level 1 or above, and
//! each record's samples come back through memory reads inside its own
//! 0x98100000 + offset window; any read outside a listed window is REJECTed.
//! Like the real meter `[device]`, a memory read returns at most 256 bytes:
//! a longer one gets its confirmation and the first 256 bytes, then nothing.

mod common;

use common::*;
use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::decode::records::{RecordEntry, decode_record_list};
use powerz_protocol::frame::Attribute;
use powerz_protocol::session::memory_read_request;
use powerz_protocol::transport::Transport;

const RECORD_LIST: Attribute = Attribute(0x200);

/// Bulk session, connected and authenticated at level 1.
fn authed() -> (SimSession, SimClock) {
    let (mut s, clock) = connected(Scenario::Idle);
    assert_eq!(s.authenticate(1), Ok(1));
    (s, clock)
}

fn listed() -> (SimSession, Vec<RecordEntry>) {
    let (mut s, _clock) = authed();
    let list = s.list_records().expect("the demo meter serves its list");
    (s, list)
}

#[test]
fn the_demo_meter_lists_at_least_three_records() {
    let (_s, list) = listed();
    assert!(list.len() >= 3, "{list:?}");
    for r in &list {
        assert!(!r.name.is_empty(), "{r:?}");
        assert!(r.name.is_ascii(), "{r:?}");
        assert!(r.interval_s > 0, "{r:?}");
    }
    let downloadable: Vec<_> = list.iter().filter(|r| r.read_window().is_some()).collect();
    assert!(downloadable.len() >= 3, "{list:?}");
}

#[test]
fn entries_carry_a_capacity_and_one_record_is_stored_negative_like_the_meter() {
    // [device] The real meter stores capacity 10000 at 0x14 and negative
    // charge and energy for the load direction it was checked with.
    let (mut s, list) = listed();
    for r in &list {
        assert_eq!(r.capacity_points, 10_000, "{}", r.name);
        assert!(r.points as u32 <= r.capacity_points, "{}", r.name);
    }
    let neg = list
        .iter()
        .find(|r| r.charge_uah < 0 && r.energy_uwh < 0)
        .expect("one record with negative totals");
    let samples = s.download_record(neg).expect("download");
    assert!(
        samples.iter().skip(1).all(|x| x.ibus_ua < 0),
        "{}",
        neg.name
    );
}

#[test]
fn the_list_is_deterministic() {
    let (_a, one) = listed();
    let (_b, two) = listed();
    assert_eq!(one, two);
}

#[test]
fn record_windows_do_not_overlap() {
    let (_s, list) = listed();
    let mut spans: Vec<(u32, u32)> = list.iter().filter_map(RecordEntry::read_window).collect();
    spans.sort();
    for w in spans.windows(2) {
        assert!(w[0].0 + w[0].1 <= w[1].0, "{spans:x?}");
    }
}

#[test]
fn every_record_downloads_its_points_with_consistent_totals() {
    let (mut s, list) = listed();
    for r in list.iter().filter(|r| r.points > 0) {
        let samples = s
            .download_record(r)
            .unwrap_or_else(|e| panic!("{}: {e}", r.name));
        assert_eq!(samples.len(), usize::from(r.points), "{}", r.name);
        for x in &samples {
            assert!(
                (0.0..=30.0).contains(&x.vbus_v()),
                "{}: VBUS {}",
                r.name,
                x.vbus_v()
            );
            assert!(x.ibus_a().abs() <= 10.0, "{}: IBUS {}", r.name, x.ibus_a());
        }
        let last = samples.last().unwrap();
        assert_eq!(last.charge_uah, r.charge_uah, "{}: charge total", r.name);
        assert_eq!(last.energy_uwh, r.energy_uwh, "{}: energy total", r.name);
        assert_eq!(
            r.duration_s,
            u32::from(r.interval_s) * u32::from(r.points - 1),
            "{}: duration",
            r.name
        );
    }
}

#[test]
fn level_0_gets_an_empty_put_data_for_the_record_list() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    let reply = d.transact(&get_data(0x21, RECORD_LIST), T).unwrap();
    assert_eq!(ty(&reply), 0x41, "{reply:02x?}");
    assert_eq!(reply.len(), 4, "no section at level 0: {reply:02x?}");
}

#[test]
fn level_1_gets_a_0x200_section_of_48_byte_entries() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    let uid = credential(&mut d, 1);
    let auth = raw_auth(&mut d, 0x22, &uid);
    assert_eq!(permi(&auth), 1);
    let reply = d.transact(&get_data(0x23, RECORD_LIST), T).unwrap();
    assert_eq!(ty(&reply), 0x41);
    assert_eq!(permi(&reply), 1);
    let payload = &reply[8..];
    assert_eq!(u16::from_le_bytes([reply[4], reply[5]]) & 0x7FFF, 0x200);
    assert_eq!(payload.len() % 48, 0, "{} bytes", payload.len());
    assert_eq!(
        decode_record_list(payload).unwrap().len(),
        payload.len() / 48
    );
}

/// Raw device at auth level 1.
fn raw_authed(clock: &SimClock) -> SimDevice {
    let mut d = raw_connected(Scenario::Idle, clock);
    let uid = credential(&mut d, 1);
    raw_auth(&mut d, 0x22, &uid);
    d
}

#[test]
fn a_read_longer_than_256_bytes_gets_the_confirmation_and_256_bytes_then_nothing() {
    let (_s, list) = listed();
    let biggest = list
        .iter()
        .filter(|r| r.read_window().is_some())
        .max_by_key(|r| r.points)
        .unwrap();
    let (addr, len) = biggest.read_window().unwrap();
    assert!(
        len > 256,
        "the demo meter needs a record over 256 bytes ({len})"
    );
    let clock = clock();
    let mut d = raw_authed(&clock);
    let confirm = d
        .transact(&memory_read_request(0x24, addr, len), T)
        .unwrap();
    assert_eq!(&confirm[..4], &[0xC4, 0x24, 0x01, 0x01]);
    let data = d.read(T).expect("the first 256 bytes");
    assert_eq!(data.len(), 256);
    assert!(d.read(T).is_err(), "nothing after the first 256 bytes");
    let plain = crypto::decrypt(&KEYS[0], &data);
    let vbus = i32::from_le_bytes(plain[0..4].try_into().unwrap());
    assert!(vbus > 0, "first sample VBUS {vbus} µV");
}

#[test]
fn consecutive_reads_of_at_most_256_bytes_return_the_whole_record() {
    let (mut s, list) = listed();
    let biggest = list
        .iter()
        .filter(|r| r.read_window().is_some())
        .max_by_key(|r| r.points)
        .unwrap()
        .clone();
    let (addr, len) = biggest.read_window().unwrap();
    let clock = clock();
    let mut d = raw_authed(&clock);
    let mut plain = Vec::new();
    for (k, at) in (addr..addr + len).step_by(256).enumerate() {
        let n = (addr + len - at).min(256);
        let id = 0x30u8.wrapping_add(k as u8);
        let confirm = d.transact(&memory_read_request(id, at, n), T).unwrap();
        assert_eq!(&confirm[..4], &[0xC4, id, 0x01, 0x01], "{at:#x}");
        let data = d.read(T).expect("chunk data");
        assert_eq!(data.len(), n as usize, "{at:#x}");
        plain.extend(crypto::decrypt(&KEYS[0], &data));
    }
    assert_eq!(plain.len(), len as usize);
    let want = s.download_record(&biggest).unwrap();
    assert_eq!(
        plain,
        want.iter().flat_map(|x| x.raw).collect::<Vec<u8>>(),
        "the chunks are the record's samples in order"
    );
}

#[test]
fn any_read_reaching_outside_a_listed_window_is_rejected() {
    let (_s, list) = listed();
    let (addr, len) = list.iter().find_map(RecordEntry::read_window).unwrap();
    let clock = clock();
    let mut d = raw_authed(&clock);
    for (i, (a, l)) in [
        (addr, len + 16),
        (addr + len - 16, 32),
        (addr + 16, len),
        (0x9810_0000 - 16, 16),
        (0x9830_0000, 16),
        (0x9800_0000, 64),
    ]
    .into_iter()
    .enumerate()
    {
        let id = 0x40 + i as u8;
        let reply = d.transact(&memory_read_request(id, a, l), T).unwrap();
        assert_eq!(reply, vec![0x06, id, 0x00, 0x00], "{a:#x}/{l}");
    }
}
