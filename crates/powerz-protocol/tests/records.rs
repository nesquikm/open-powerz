//! Offline records, read-only (FR 1HHT00): the RecordList (GET_DATA 0x200)
//! entry decoder, the computed download window, the 16-byte sample decoder
//! and the exact frames a list + download sends.
//!
//! Layouts: research control notes §4, §4.1, §4.2 (static analysis), checked
//! on the real meter in the hardware FR (1HHT04): the sample fields are
//! `[device]`, a memory read returns at most 256 bytes, so a download is a run
//! of consecutive reads. The entries and samples here are synthetic, except
//! the one real RecordList entry below (no serial or UID in it).

mod common;

use common::*;
use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::decode::DecodeError;
use powerz_protocol::decode::records::{
    ENTRY_LEN, FLASH_WINDOW_BASE, MEMORY_READ_MAX_LEN, RECORD_AREA_LEN, RecordEntry, RecordSample,
    SAMPLE_FIELDS, SAMPLE_LEN, decode_record_list, decode_samples,
};
use powerz_protocol::decode::settings::Tag;
use powerz_protocol::frame::{Attribute, SectionHeader};
use powerz_protocol::session::{Error, Session};
use powerz_protocol::transport::mock::{MockTransport, Step};
use proptest::prelude::*;

const T0: u64 = 1_790_000_000_000;

/// Synthetic entry fields.
#[derive(Debug, Clone)]
struct E {
    name: &'static str,
    interval: u8,
    points: u16,
    duration: u32,
    charge: i32,
    energy: i32,
    offset: u32,
}

const REC_A: E = E {
    name: "REC001",
    interval: 2,
    points: 300,
    duration: 600,
    charge: 1_234_567,
    energy: -7_654_321,
    offset: 0x0000_1000,
};

const REC_B: E = E {
    name: "charger test",
    interval: 1,
    points: 3,
    duration: 3,
    charge: 42,
    energy: 210,
    offset: 0x0001_0000,
};

/// 48-byte entry with distinctive bytes in every unknown range.
fn entry_bytes(e: &E) -> Vec<u8> {
    let mut b = vec![0u8; ENTRY_LEN];
    let n = e.name.len().min(16);
    b[..n].copy_from_slice(&e.name.as_bytes()[..n]);
    b[0x10] = 0xA5;
    b[0x11] = e.interval;
    b[0x12..0x14].copy_from_slice(&e.points.to_le_bytes());
    b[0x14..0x18].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    b[0x18..0x1C].copy_from_slice(&e.duration.to_le_bytes());
    b[0x1C..0x20].copy_from_slice(&e.charge.to_le_bytes());
    b[0x20..0x24].copy_from_slice(&e.energy.to_le_bytes());
    b[0x24..0x28].copy_from_slice(&e.offset.to_le_bytes());
    b[0x28..0x30].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    b
}

fn entry(e: &E) -> RecordEntry {
    RecordEntry::decode(&entry_bytes(e)).unwrap_or_else(|err| panic!("decode failed: {err}"))
}

/// PUT_DATA reply carrying one RecordList section with `entries`; the
/// section `size` field holds only the low 10 bits of the payload length,
/// as a 10-bit field must.
fn list_reply(id: u8, level: u8, entries: &[Vec<u8>]) -> Vec<u8> {
    let payload: Vec<u8> = entries.concat();
    let mut f = put_header(id, level).to_vec();
    f.extend(
        SectionHeader {
            att: 0x200,
            next: false,
            chunk: 0,
            size: (payload.len() & 0x3FF) as u16,
        }
        .encode(),
    );
    f.extend(payload);
    f
}

/// One 16-byte sample.
fn sample_bytes(vbus_uv: i32, ibus_ua: i32, charge_uah: i32, energy_uwh: i32) -> Vec<u8> {
    let mut s = Vec::with_capacity(SAMPLE_LEN);
    s.extend(vbus_uv.to_le_bytes());
    s.extend(ibus_ua.to_le_bytes());
    s.extend(charge_uah.to_le_bytes());
    s.extend(energy_uwh.to_le_bytes());
    s
}

/// `n` synthetic samples: VBUS 5 V + k mV, IBUS -(k) mA (signed),
/// charge k µAh, energy 5k µWh.
fn samples(n: usize) -> Vec<u8> {
    (0..n as i32)
        .flat_map(|k| sample_bytes(5_000_000 + k * 1_000, -k * 1_000, k, 5 * k))
        .collect()
}

/// `(addr, len)` of every memory read a download of `e` sends: consecutive
/// reads of at most 256 bytes from `0x98100000 + offset`.
fn chunk_windows(e: &E) -> Vec<(u32, u32)> {
    let addr = FLASH_WINDOW_BASE + e.offset;
    let len = u32::from(e.points) * SAMPLE_LEN as u32;
    (0..len)
        .step_by(256)
        .map(|k| (addr + k, (len - k).min(256)))
        .collect()
}

/// The meter's answer to a record download as it behaves on the device
/// `[device]`: one memory read per chunk of at most 256 bytes (request ids
/// from `id` on), each a confirmation then that chunk's encrypted samples.
fn download_steps(id: u8, e: &E) -> Vec<Step> {
    let plain = samples(usize::from(e.points));
    let base = FLASH_WINDOW_BASE + e.offset;
    let mut steps = Vec::new();
    for (k, (addr, len)) in chunk_windows(e).into_iter().enumerate() {
        let id = id.wrapping_add(k as u8);
        let from = (addr - base) as usize;
        steps.push(Step::transact(
            memread_request(id, addr, len),
            memread_confirm(id, addr, len),
        ));
        steps.push(Step::read(crypto::encrypt(
            &KEYS[0],
            &plain[from..from + len as usize],
        )));
    }
    steps
}

/// Session from auth level 0: UID read (id 1), auth (id 2, answered by
/// [`AuthDevice`] at level 1), then `rest`.
fn authed_session(rest: Vec<Step>) -> Session<AuthDevice, FakeClock> {
    let script = [memread_steps(1, UID_ADDR, &SYNTH_UID), rest].concat();
    Session::with_clock(AuthDevice::new(script, 1), FakeClock::new(T0))
}

// ---- AC-1HHT00.1: RecordList entry decode ----

#[test]
fn ac1_record_list_attribute_is_0x200_and_its_request_is_0c_id_00_04() {
    assert_eq!(Attribute::RECORD_LIST.bits(), 0x200);
    assert_eq!(
        powerz_protocol::frame::get_data(7, Attribute::RECORD_LIST).to_vec(),
        vec![0x0C, 7, 0x00, 0x04]
    );
}

#[test]
fn ac1_entry_decodes_every_known_field() {
    let r = entry(&REC_A);
    assert_eq!(r.name, "REC001");
    assert_eq!(r.interval_s, 2);
    assert_eq!(r.points, 300);
    assert_eq!(r.duration_s, 600);
    assert_eq!(r.charge_uah, 1_234_567);
    assert_eq!(r.energy_uwh, -7_654_321, "energy is signed");
    assert_eq!(r.offset, 0x1000);
    assert!((r.charge_ah() - 1.234_567).abs() < 1e-12);
    assert!((r.energy_wh() + 7.654_321).abs() < 1e-12);
}

#[test]
fn ac1_entry_keeps_all_48_bytes_raw_including_the_unknown_ones() {
    let bytes = entry_bytes(&REC_A);
    let r = entry(&REC_A);
    assert_eq!(r.raw.to_vec(), bytes);
    assert_eq!(r.raw[0x10], 0xA5);
    assert_eq!(&r.raw[0x14..0x18], &[0xDE, 0xAD, 0xBE, 0xEF]);
    assert_eq!(r.capacity_points, 0xEFBE_ADDE, "0x14 is the u32 capacity");
    assert_eq!(&r.raw[0x28..0x30], &[1, 2, 3, 4, 5, 6, 7, 8]);
}

/// The real meter's RecordList entry from the hardware FR (1HHT04): record
/// "A01.d", 173 points at 1 s, 172 s, charge and energy negative for the
/// load direction used, capacity 10000, offset 0x400. No serial or UID; the unknown
/// bytes at 0x2C are replaced with `aa bb cc dd` in case they derive from the meter.
const REAL_ENTRY: [u8; ENTRY_LEN] = [
    0x41, 0x30, 0x31, 0x2e, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x45, 0x01, 0xad, 0x00, 0x10, 0x27, 0x00, 0x00, 0xac, 0x00, 0x00, 0x00, 0x7d, 0x6e, 0xff, 0xff,
    0x0c, 0x21, 0xfd, 0xff, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xaa, 0xbb, 0xcc, 0xdd,
];

#[test]
fn ac1_the_real_meter_entry_decodes_every_field() {
    let r = RecordEntry::decode(&REAL_ENTRY).unwrap();
    assert_eq!(r.name, "A01.d");
    assert_eq!(r.interval_s, 1);
    assert_eq!(r.points, 173);
    assert_eq!(r.capacity_points, 10_000, "the meter shows 173/10000");
    assert_eq!(r.duration_s, 172, "00:02:52 on the meter");
    assert_eq!(r.charge_uah, -37_251, "stored signed");
    assert_eq!(r.energy_uwh, -188_148, "stored signed");
    assert!((r.charge_ah() + 0.037_251).abs() < 1e-12);
    assert!((r.energy_wh() + 0.188_148).abs() < 1e-12);
    assert!((r.charge_magnitude_ah() - 0.037_251).abs() < 1e-12);
    assert!((r.energy_magnitude_wh() - 0.188_148).abs() < 1e-12);
    assert_eq!(r.offset, 0x400);
    assert_eq!(r.raw, REAL_ENTRY, "unknown bytes are kept raw");
    assert_eq!(r.raw[0x10], 0x45);
    assert_eq!(&r.raw[0x28..0x30], &[0, 0, 0, 0, 0xaa, 0xbb, 0xcc, 0xdd]);
    assert_eq!(r.read_window(), Some((0x9810_0400, 173 * 16)));
}

#[test]
fn ac1_magnitudes_never_overflow() {
    let r = entry(&E {
        charge: i32::MIN,
        energy: i32::MIN,
        ..REC_A
    });
    assert!((r.charge_magnitude_ah() - 2_147.483_648).abs() < 1e-9);
    assert!((r.energy_magnitude_wh() - 2_147.483_648).abs() < 1e-9);
}

#[test]
fn ac1_name_stops_at_nul_and_never_runs_past_16_bytes() {
    let mut b = entry_bytes(&REC_A);
    b[3] = 0;
    b[4] = b'X';
    assert_eq!(RecordEntry::decode(&b).unwrap().name, "REC");

    // A 16-character name fills 0x00..0x10; byte 0x10 (0xA5) is not part of it.
    let full = E {
        name: "ABCDEFGHIJKLMNOP",
        ..REC_A
    };
    assert_eq!(entry(&full).name, "ABCDEFGHIJKLMNOP");
}

#[test]
fn ac1_non_ascii_name_bytes_do_not_panic() {
    let mut b = entry_bytes(&REC_A);
    b[0..4].copy_from_slice(&[0xFF, 0xC3, 0x28, 0x80]);
    let r = RecordEntry::decode(&b).unwrap();
    assert!(r.name.chars().count() <= 16, "{:?}", r.name);
}

#[test]
fn ac1_short_entry_is_a_length_error() {
    let b = entry_bytes(&REC_A);
    assert!(matches!(
        RecordEntry::decode(&b[..47]),
        Err(DecodeError::Length { got: 47, .. })
    ));
}

#[test]
fn ac1_list_decodes_every_48_byte_entry_in_order() {
    let payload = [entry_bytes(&REC_A), entry_bytes(&REC_B)].concat();
    let list = decode_record_list(&payload).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].name, "REC001");
    assert_eq!(list[1].name, "charger test");
    assert_eq!(list[1].points, 3);
    assert_eq!(list[1].offset, 0x1_0000);
}

#[test]
fn ac1_empty_list_is_not_an_error() {
    assert_eq!(decode_record_list(&[]).unwrap(), Vec::<RecordEntry>::new());
}

#[test]
fn ac1_trailing_partial_entry_is_a_decode_error_not_a_panic() {
    let mut payload = [entry_bytes(&REC_A), entry_bytes(&REC_B)].concat();
    payload.truncate(ENTRY_LEN + 20);
    assert!(matches!(
        decode_record_list(&payload),
        Err(DecodeError::Length { .. })
    ));
}

#[test]
fn ac1_session_lists_records_after_level_1_auth() {
    let reply = list_reply(3, 1, &[entry_bytes(&REC_A), entry_bytes(&REC_B)]);
    let mut s = authed_session(vec![Step::transact(get_data(3, 0x200), reply)]);
    assert_eq!(s.auth_level(), 0);
    let list = s.list_records().unwrap();
    assert_eq!(s.auth_level(), 1, "list_records must authenticate first");
    assert_eq!(s.transport().auth_frames.len(), 1);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0], entry(&REC_A));
    assert_eq!(list[1], entry(&REC_B));
    assert_eq!(s.transport().inner.remaining(), 0);
}

#[test]
fn ac1_session_list_with_size_0_section_is_an_empty_list() {
    let mut s = authed_session(vec![Step::transact(
        get_data(3, 0x200),
        list_reply(3, 1, &[]),
    )]);
    assert_eq!(s.list_records().unwrap(), Vec::<RecordEntry>::new());
}

#[test]
fn ac1_session_list_does_not_trust_the_10_bit_size_field() {
    // 40 entries are 1920 bytes; a 10-bit size field holds 1920 & 0x3FF = 896.
    let entries: Vec<Vec<u8>> = (0..40u32)
        .map(|k| {
            entry_bytes(&E {
                name: "R",
                offset: k * 0x1000,
                ..REC_B
            })
        })
        .collect();
    let mut s = authed_session(vec![Step::transact(
        get_data(3, 0x200),
        list_reply(3, 1, &entries),
    )]);
    let list = s.list_records().unwrap();
    assert_eq!(list.len(), 40);
    assert_eq!(list[39].offset, 39 * 0x1000);
}

#[test]
fn ac1_session_list_with_a_partial_entry_is_a_decode_error() {
    let mut reply = list_reply(3, 1, &[entry_bytes(&REC_A)]);
    reply.extend([0u8; 10]);
    let mut s = authed_session(vec![Step::transact(get_data(3, 0x200), reply)]);
    assert!(matches!(
        s.list_records(),
        Err(Error::Decode(DecodeError::Length { .. }))
    ));
}

#[test]
fn ac1_session_list_not_served_is_an_error_not_an_empty_list() {
    // At level 0 the meter answers an empty PUT_DATA (no 0x200 section).
    let mut s = authed_session(vec![Step::transact(
        get_data(3, 0x200),
        put_header(3, 0).to_vec(),
    )]);
    assert!(s.list_records().is_err());
}

#[test]
fn ac1_session_already_at_level_1_does_not_authenticate_again() {
    let reply = list_reply(3, 1, &[entry_bytes(&REC_B)]);
    let mut s = authed_session(vec![Step::transact(get_data(3, 0x200), reply)]);
    s.authenticate(1).unwrap();
    assert_eq!(s.list_records().unwrap().len(), 1);
    assert_eq!(s.transport().auth_frames.len(), 1);
}

// ---- AC-1HHT00.2: the download window ----

#[test]
fn ac2_read_window_is_base_plus_offset_and_points_times_16() {
    assert_eq!(FLASH_WINDOW_BASE, 0x9810_0000);
    assert_eq!(RECORD_AREA_LEN, 0x20_0000);
    assert_eq!(entry(&REC_A).read_window(), Some((0x9810_1000, 300 * 16)));
    assert_eq!(entry(&REC_B).read_window(), Some((0x9811_0000, 48)));
}

#[test]
fn ac2_read_window_is_bounded_by_the_record_area() {
    let at = |offset: u32, points: u16| {
        entry(&E {
            offset,
            points,
            ..REC_A
        })
        .read_window()
    };
    // Ends exactly at the area end: allowed.
    assert_eq!(at(0x1F_FFF0, 1), Some((0x982F_FFF0, 16)));
    // One sample past the end: refused.
    assert_eq!(at(0x1F_FFF0, 2), None);
    assert_eq!(at(0x20_0000, 1), None);
    // No samples: nothing to read.
    assert_eq!(at(0, 0), None);
    // Largest point count at the start of the area.
    assert_eq!(at(0, u16::MAX), Some((0x9810_0000, 65_535 * 16)));
    // Offsets near u32::MAX must not wrap into the area.
    assert_eq!(at(u32::MAX, 1), None);
    assert_eq!(at(0xFFFF_FFF0, 1), None);
}

#[test]
fn ac2_a_memory_read_returns_at_most_256_bytes() {
    assert_eq!(MEMORY_READ_MAX_LEN, 256);
}

#[test]
fn ac2_read_chunks_cover_the_window_in_consecutive_reads_of_at_most_256_bytes() {
    assert_eq!(entry(&REC_B).read_chunks(), vec![(0x9811_0000, 48)]);
    let a = entry(&REC_A); // 4800 bytes: 18 x 256 + 192
    let chunks = a.read_chunks();
    assert_eq!(chunks, chunk_windows(&REC_A));
    assert_eq!(chunks.len(), 19);
    assert_eq!(chunks[0], (0x9810_1000, 256));
    assert_eq!(chunks[18], (0x9810_1000 + 18 * 256, 192));
    assert!(entry(&E { points: 0, ..REC_A }).read_chunks().is_empty());
    assert!(
        entry(&E {
            offset: 0x1F_FFF0,
            points: 2,
            ..REC_A
        })
        .read_chunks()
        .is_empty(),
        "no chunks outside the record area"
    );
}

#[test]
fn ac2_download_reads_the_window_in_consecutive_reads_of_at_most_256_bytes() {
    let reply = list_reply(3, 1, &[entry_bytes(&REC_A), entry_bytes(&REC_B)]);
    let script = [
        vec![Step::transact(get_data(3, 0x200), reply)],
        download_steps(4, &REC_A),
    ]
    .concat();
    let mut s = authed_session(script);
    let list = s.list_records().unwrap();
    let got = s.download_record(&list[0]).unwrap();
    assert_eq!(got.len(), 300);
    assert_eq!(got[0].vbus_uv, 5_000_000);
    assert_eq!(
        got[16].vbus_uv, 5_016_000,
        "first sample of the second chunk"
    );
    assert_eq!(got[299].vbus_uv, 5_299_000);
    assert_eq!(got[299].ibus_ua, -299_000);
    assert_eq!(s.transport().inner.remaining(), 0);
}

#[test]
fn ac2_a_173_point_record_downloads_in_11_reads() {
    // The real meter's record: 173 x 16 = 2768 bytes = 10 x 256 + 208 [device].
    let rec = E {
        name: "A01.d",
        interval: 1,
        points: 173,
        duration: 172,
        charge: -37_251,
        energy: -188_148,
        offset: 0x400,
    };
    let reply = list_reply(3, 1, &[entry_bytes(&rec)]);
    let script = [
        vec![Step::transact(get_data(3, 0x200), reply)],
        download_steps(4, &rec),
    ]
    .concat();
    let mut s = authed_session(script);
    let list = s.list_records().unwrap();
    let got = s.download_record(&list[0]).unwrap();
    assert_eq!(got.len(), 173);
    assert_eq!(got[172].vbus_uv, 5_172_000);
    let reads: Vec<Vec<u8>> = s.transport().inner.written()[2..].to_vec();
    let want: Vec<Vec<u8>> = (0..11u32)
        .map(|k| {
            let len = if k < 10 { 256 } else { 208 };
            memread_request(4 + k as u8, 0x9810_0400 + k * 256, len)
        })
        .collect();
    assert_eq!(reads, want, "10 x 256 + 208 bytes, consecutive addresses");
    assert_eq!(s.transport().inner.remaining(), 0);
}

#[test]
fn ac2_no_download_read_is_longer_than_256_bytes() {
    // A read longer than 256 bytes would get only its first 256 bytes back.
    let reply = list_reply(3, 1, &[entry_bytes(&REC_A)]);
    let script = [
        vec![Step::transact(get_data(3, 0x200), reply)],
        download_steps(4, &REC_A),
    ]
    .concat();
    let mut s = authed_session(script);
    let list = s.list_records().unwrap();
    s.download_record(&list[0]).unwrap();
    for f in &s.transport().inner.written()[2..] {
        let plain = crypto::decrypt(&KEYS[0], &f[4..36]);
        let len = u32::from_le_bytes(plain[4..8].try_into().unwrap());
        assert!(len <= 256, "a read of {len} bytes");
    }
}

#[test]
fn ac2_download_of_a_0_point_record_is_forbidden_and_sends_nothing() {
    let empty = E { points: 0, ..REC_B };
    let reply = list_reply(3, 1, &[entry_bytes(&empty)]);
    let mut s = authed_session(vec![Step::transact(get_data(3, 0x200), reply)]);
    let list = s.list_records().unwrap();
    let before = s.transport().inner.written().len();
    assert!(matches!(
        s.download_record(&list[0]),
        Err(Error::Forbidden { .. })
    ));
    assert_eq!(s.transport().inner.written().len(), before);
}

#[test]
fn ac2_download_past_the_record_area_is_forbidden_and_sends_nothing() {
    let past = E {
        offset: 0x1F_FF00,
        points: 32, // 0x1FFF00 + 512 > 0x200000
        ..REC_B
    };
    let reply = list_reply(3, 1, &[entry_bytes(&past)]);
    let mut s = authed_session(vec![Step::transact(get_data(3, 0x200), reply)]);
    let list = s.list_records().unwrap();
    let before = s.transport().inner.written().len();
    assert!(matches!(
        s.download_record(&list[0]),
        Err(Error::Forbidden { .. })
    ));
    assert_eq!(s.transport().inner.written().len(), before);
}

#[test]
fn ac2_download_of_an_entry_that_was_not_listed_is_forbidden() {
    let reply = list_reply(3, 1, &[entry_bytes(&REC_B)]);
    let mut s = authed_session(vec![Step::transact(get_data(3, 0x200), reply)]);
    s.list_records().unwrap();
    // In bounds, but not one of the decoded entries.
    let other = entry(&E {
        offset: 0x2_0000,
        ..REC_B
    });
    let before = s.transport().inner.written().len();
    assert!(matches!(
        s.download_record(&other),
        Err(Error::Forbidden { .. })
    ));
    assert_eq!(s.transport().inner.written().len(), before);

    // Nothing listed at all: refused as well.
    let mut fresh = Session::open(MockTransport::new(vec![]));
    assert!(matches!(
        fresh.download_record(&entry(&REC_B)),
        Err(Error::Forbidden { .. })
    ));
    assert!(fresh.transport().written().is_empty());
}

#[test]
fn ac2_a_failed_relisting_forgets_the_previous_windows() {
    let reply = list_reply(3, 1, &[entry_bytes(&REC_B)]);
    let mut s = authed_session(vec![
        Step::transact(get_data(3, 0x200), reply),
        Step::transact(get_data(4, 0x200), put_header(4, 1).to_vec()),
    ]);
    s.list_records().unwrap();
    assert!(s.list_records().is_err());
    let before = s.transport().inner.written().len();
    assert!(matches!(
        s.download_record(&entry(&REC_B)),
        Err(Error::Forbidden { .. })
    ));
    assert_eq!(s.transport().inner.written().len(), before);
}

#[test]
fn ac2_plain_memory_read_of_the_record_area_stays_forbidden() {
    for (addr, len) in [
        (0x9810_0000u32, 16u32),
        (0x9810_1000, 300 * 16),
        (0x9800_0000, 64),
        (0x982F_FFF0, 16),
        (0x0010_0000, 16),
    ] {
        let mut s = Session::open(MockTransport::new(vec![]));
        assert!(
            matches!(s.memory_read(addr, len), Err(Error::Forbidden { .. })),
            "{addr:#x}/{len} must stay forbidden"
        );
        assert!(s.transport().written().is_empty(), "{addr:#x}/{len}");
    }
}

#[test]
fn ac2_download_cut_short_is_an_error() {
    let reply = list_reply(3, 1, &[entry_bytes(&REC_A)]);
    let mut steps = download_steps(4, &REC_A);
    steps.pop(); // the last data transfer never arrives
    let script = [vec![Step::transact(get_data(3, 0x200), reply)], steps].concat();
    let mut s = authed_session(script);
    let list = s.list_records().unwrap();
    assert!(s.download_record(&list[0]).is_err());
}

// ---- AC-1HHT00.3: the 16-byte sample ----

#[test]
fn ac3_sample_decodes_four_signed_32_bit_values() {
    let b = sample_bytes(20_123_456, -3_250_000, -1_000_001, 65_000_000);
    let s = RecordSample::decode(&b).unwrap();
    assert_eq!(s.vbus_uv, 20_123_456);
    assert_eq!(s.ibus_ua, -3_250_000);
    assert_eq!(s.charge_uah, -1_000_001);
    assert_eq!(s.energy_uwh, 65_000_000);
    assert!((s.vbus_v() - 20.123_456).abs() < 1e-12);
    assert!((s.ibus_a() + 3.25).abs() < 1e-12);
    assert!((s.charge_ah() + 1.000_001).abs() < 1e-12);
    assert!((s.energy_wh() - 65.0).abs() < 1e-12);
    assert_eq!(s.raw.to_vec(), b, "the raw 16 bytes are kept");
}

#[test]
fn ac3_short_sample_is_a_length_error() {
    let b = sample_bytes(1, 2, 3, 4);
    assert!(matches!(
        RecordSample::decode(&b[..15]),
        Err(DecodeError::Length { got: 15, .. })
    ));
}

#[test]
fn ac3_samples_need_exactly_points_times_16_bytes() {
    let r = entry(&REC_B); // 3 points
    let ok = decode_samples(&r, &samples(3)).unwrap();
    assert_eq!(ok.len(), 3);
    assert_eq!(ok[2].charge_uah, 2);
    assert_eq!(ok[2].energy_uwh, 10);
    assert!(matches!(
        decode_samples(&r, &samples(2)),
        Err(DecodeError::Length { .. })
    ));
    let mut long = samples(3);
    long.push(0);
    assert!(matches!(
        decode_samples(&r, &long),
        Err(DecodeError::Length { .. })
    ));
}

#[test]
fn ac3_sample_time_is_interval_times_index_in_seconds() {
    let r = entry(&REC_A); // 2 s interval
    assert_eq!(r.sample_time_s(0), 0);
    assert_eq!(r.sample_time_s(1), 2);
    assert_eq!(r.sample_time_s(299), 598);
}

#[test]
fn ac3_every_sample_field_is_documented_and_confirmed_on_the_meter() {
    let keys: Vec<(&str, usize)> = SAMPLE_FIELDS.iter().map(|f| (f.key, f.offset)).collect();
    assert_eq!(
        keys,
        vec![("vbus", 0), ("ibus", 4), ("charge", 8), ("energy", 12)]
    );
    for f in SAMPLE_FIELDS.iter() {
        assert_eq!(
            f.tag,
            Tag::Confirmed,
            "{} was checked against a real recording (1HHT04)",
            f.key
        );
    }
}

// ---- AC-1HHT00.7: the exact commands of a list + download ----

#[test]
fn ac7_list_and_download_send_exactly_these_frames() {
    let reply = list_reply(3, 1, &[entry_bytes(&REC_A), entry_bytes(&REC_B)]);
    let script = [
        vec![Step::transact(get_data(3, 0x200), reply)],
        download_steps(4, &REC_B),
    ]
    .concat();
    let mut s = authed_session(script);
    let list = s.list_records().unwrap();
    s.download_record(&list[1]).unwrap();
    assert_eq!(
        s.transport().inner.written(),
        &[
            memread_request(1, UID_ADDR, 12),
            get_data(3, 0x200),
            memread_request(4, 0x9811_0000, 48),
        ]
    );
    assert_eq!(s.transport().auth_frames.len(), 1, "one 0x4C");
    for f in s.transport().inner.written() {
        let ty = f[0] & 0x7F;
        assert!(
            [0x0C, 0x44].contains(&ty),
            "only GET_DATA and memory reads besides auth: {f:02x?}"
        );
    }
}

// ---- property tests ----

proptest! {
    #[test]
    fn prop_list_decode_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
        match decode_record_list(&bytes) {
            Ok(list) => {
                prop_assert_eq!(bytes.len() % ENTRY_LEN, 0);
                prop_assert_eq!(list.len(), bytes.len() / ENTRY_LEN);
            }
            Err(e) => {
                prop_assert!(bytes.len() % ENTRY_LEN != 0, "{e}");
            }
        }
    }

    #[test]
    fn prop_entry_fields_round_trip(
        interval in any::<u8>(),
        points in any::<u16>(),
        duration in any::<u32>(),
        charge in any::<i32>(),
        energy in any::<i32>(),
        offset in any::<u32>(),
    ) {
        let r = entry(&E { name: "P", interval, points, duration, charge, energy, offset });
        prop_assert_eq!(r.interval_s, interval);
        prop_assert_eq!(r.points, points);
        prop_assert_eq!(r.duration_s, duration);
        prop_assert_eq!(r.charge_uah, charge);
        prop_assert_eq!(r.energy_uwh, energy);
        prop_assert_eq!(r.offset, offset);
        // The chunks tile the window exactly, at most 256 bytes each.
        let chunks = r.read_chunks();
        if let Some((addr, len)) = r.read_window() {
            let mut at = addr;
            for &(a, l) in &chunks {
                prop_assert_eq!(a, at);
                prop_assert!(l > 0 && l <= 256);
                at += l;
            }
            prop_assert_eq!(at, addr + len);
        } else {
            prop_assert!(chunks.is_empty());
        }
        // The window, when given, is always inside the record area.
        if let Some((addr, len)) = r.read_window() {
            prop_assert!(points > 0);
            prop_assert_eq!(len, u32::from(points) * 16);
            prop_assert!(addr >= FLASH_WINDOW_BASE);
            prop_assert!(u64::from(addr - FLASH_WINDOW_BASE) + u64::from(len) <= u64::from(RECORD_AREA_LEN));
        } else {
            prop_assert!(points == 0 || u64::from(offset) + u64::from(points) * 16 > u64::from(RECORD_AREA_LEN));
        }
    }

    #[test]
    fn prop_sample_round_trip(v in any::<i32>(), i in any::<i32>(), c in any::<i32>(), e in any::<i32>()) {
        let s = RecordSample::decode(&sample_bytes(v, i, c, e)).unwrap();
        prop_assert_eq!((s.vbus_uv, s.ibus_ua, s.charge_uah, s.energy_uwh), (v, i, c, e));
    }
}
