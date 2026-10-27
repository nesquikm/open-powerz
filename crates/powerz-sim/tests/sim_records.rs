//! AC-1HHT00.6: the demo meter keeps a few synthetic offline records. The
//! RecordList (GET_DATA 0x200) is served only at auth level 1 or above, and
//! each record's samples come back through its own 0x98100000 + offset
//! window in transfers of at most 4096 bytes; any other window is REJECTed.

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

#[test]
fn record_data_arrives_in_transfers_of_at_most_4096_bytes() {
    let (_s, list) = listed();
    let biggest = list
        .iter()
        .filter(|r| r.read_window().is_some())
        .max_by_key(|r| r.points)
        .unwrap();
    let (addr, len) = biggest.read_window().unwrap();
    assert!(
        len > 4096,
        "the demo meter needs one record over 4096 bytes to exercise chunking ({len})"
    );
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    let uid = credential(&mut d, 1);
    raw_auth(&mut d, 0x22, &uid);
    let confirm = d
        .transact(&memory_read_request(0x24, addr, len), T)
        .unwrap();
    assert_eq!(&confirm[..4], &[0xC4, 0x24, 0x01, 0x01]);
    let mut data = Vec::new();
    while data.len() < len as usize {
        let chunk = d.read(T).expect("record data");
        assert!(chunk.len() <= 4096, "transfer of {} bytes", chunk.len());
        assert_eq!(chunk.len() % 16, 0);
        data.extend(chunk);
    }
    assert_eq!(data.len(), len as usize);
    let plain = crypto::decrypt(&KEYS[0], &data);
    let vbus = i32::from_le_bytes(plain[0..4].try_into().unwrap());
    assert!(vbus > 0, "first sample VBUS {vbus} µV");
}

#[test]
fn any_other_window_is_rejected() {
    let (_s, list) = listed();
    let (addr, len) = list.iter().find_map(RecordEntry::read_window).unwrap();
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    let uid = credential(&mut d, 1);
    raw_auth(&mut d, 0x22, &uid);
    for (i, (a, l)) in [
        (addr, len + 16),
        (addr, len - 16),
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
