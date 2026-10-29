//! Deleting offline records (FR JKWT3V, AC .1, .4 and .5, plus the size
//! limit of the Notes): the record-table builder, the exact SET_DATA frame a
//! delete sends, the re-read check and the narrow guard exception.
//!
//! Layout from `docs/protocol.md` "Deleting records" `[static]`: one SET_DATA
//! (0x48) with attribute 0x28 carrying the remaining 48-byte entries,
//! unchanged and in list order, then four zero bytes. Main header
//! `0x48 | id << 8 | ((len / 4) + 2) << 22`, then one extended section header
//! (att 0x28, next 0, chunk 0, size = len), then the data. The 10-bit `size`
//! holds at most 1023 bytes, so 22 or more remaining entries are refused
//! before anything is sent. All entries here are synthetic.

mod common;

use common::*;
use powerz_protocol::decode::records::{ENTRY_LEN, RecordEntry, encode_record_table};
use powerz_protocol::frame::SectionHeader;
use powerz_protocol::session::{Error, Session, record_table_request};
use powerz_protocol::transport::GuardedTransport;
use powerz_protocol::transport::mock::Step;

const T0: u64 = 1_790_000_000_000;

/// A synthetic 48-byte entry: name, interval 1, `points` points, offset
/// `k * 0x1000`, and distinctive bytes in the unknown ranges (0x10 and
/// 0x28..0x30), which a rewrite must keep.
fn entry_bytes(name: &str, points: u16, k: u32) -> Vec<u8> {
    let mut b = vec![0u8; ENTRY_LEN];
    b[..name.len()].copy_from_slice(name.as_bytes());
    b[0x10] = 0x5A;
    b[0x11] = 1;
    b[0x12..0x14].copy_from_slice(&points.to_le_bytes());
    b[0x14..0x18].copy_from_slice(&10_000u32.to_le_bytes());
    b[0x18..0x1C].copy_from_slice(&u32::from(points.saturating_sub(1)).to_le_bytes());
    b[0x1C..0x20].copy_from_slice(&(-(k as i32) * 7).to_le_bytes());
    b[0x20..0x24].copy_from_slice(&((k as i32) * 35).to_le_bytes());
    b[0x24..0x28].copy_from_slice(&(k * 0x1000).to_le_bytes());
    b[0x28..0x30].copy_from_slice(&[0xC0 | k as u8, 1, 2, 3, 4, 5, 6, 7]);
    b
}

fn entry(name: &str, points: u16, k: u32) -> RecordEntry {
    RecordEntry::decode(&entry_bytes(name, points, k)).expect("synthetic entry decodes")
}

/// `n` synthetic entries named `R00`, `R01`, ...
fn entries(n: u32) -> Vec<Vec<u8>> {
    (0..n)
        .map(|k| entry_bytes(&format!("R{k:02}"), 3, k))
        .collect()
}

fn a() -> Vec<u8> {
    entry_bytes("REC001", 300, 1)
}
fn b() -> Vec<u8> {
    entry_bytes("charger test", 3, 2)
}
fn c() -> Vec<u8> {
    entry_bytes("LAST", 60, 3)
}

/// PUT_DATA reply with one RecordList section (size keeps only 10 bits).
fn list_reply(id: u8, list: &[Vec<u8>]) -> Vec<u8> {
    let payload = list.concat();
    let mut f = put_header(id, 1).to_vec();
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

/// The SET_DATA frame the protocol notes document, built by hand.
fn expected_frame(id: u8, remaining: &[Vec<u8>]) -> Vec<u8> {
    let mut data = remaining.concat();
    data.extend([0u8; 4]);
    let len = data.len() as u32;
    let mut f = (0x48u32 | (u32::from(id) << 8) | ((len / 4 + 2) << 22))
        .to_le_bytes()
        .to_vec();
    f.extend((0x28u32 | (len << 22)).to_le_bytes());
    f.extend(data);
    f
}

type Guarded = Session<GuardedTransport<AuthDevice>, FakeClock>;

/// Guarded session: UID read (id 1), auth (id 2, level 1), the list (id 3)
/// of `listed`, then `rest`.
fn session(listed: &[Vec<u8>], rest: Vec<Step>) -> Guarded {
    let script = [
        memread_steps(1, UID_ADDR, &SYNTH_UID),
        vec![Step::transact(get_data(3, 0x200), list_reply(3, listed))],
        rest,
    ]
    .concat();
    Session::with_clock(
        GuardedTransport::new(AuthDevice::new(script, 1)),
        FakeClock::new(T0),
    )
}

fn written(s: &Guarded) -> &[Vec<u8>] {
    s.transport().inner().inner.written()
}

fn set_data_frames(s: &Guarded) -> Vec<Vec<u8>> {
    written(s)
        .iter()
        .filter(|f| f[0] & 0x7F == 0x48)
        .cloned()
        .collect()
}

// ---- AC-JKWT3V.1: the table builder ----

#[test]
fn table_builder_writes_the_remaining_entries_unchanged_and_in_order_then_four_zero_bytes() {
    let list = [entry("REC001", 300, 1), entry("LAST", 60, 3)];
    let table = encode_record_table(&list);
    assert_eq!(table.len(), 2 * ENTRY_LEN + 4);
    assert_eq!(&table[..ENTRY_LEN], &a()[..]);
    assert_eq!(&table[ENTRY_LEN..2 * ENTRY_LEN], &c()[..]);
    assert_eq!(&table[2 * ENTRY_LEN..], &[0, 0, 0, 0]);
}

#[test]
fn table_builder_keeps_the_unknown_bytes_of_every_entry() {
    let e = entry("charger test", 3, 2);
    let table = encode_record_table(std::slice::from_ref(&e));
    assert_eq!(&table[..ENTRY_LEN], &e.raw[..]);
    assert_eq!(table[0x10], 0x5A);
    assert_eq!(&table[0x28..0x30], &[0xC2, 1, 2, 3, 4, 5, 6, 7]);
}

#[test]
fn table_builder_of_an_empty_list_is_four_zero_bytes() {
    assert_eq!(encode_record_table(&[]), vec![0u8; 4]);
}

#[test]
fn table_builder_with_the_last_entry_removed_is_the_prefix_of_the_list() {
    let all: Vec<RecordEntry> = entries(5)
        .iter()
        .map(|e| RecordEntry::decode(e).unwrap())
        .collect();
    let table = encode_record_table(&all[..4]);
    let mut want = entries(4).concat();
    want.extend([0u8; 4]);
    assert_eq!(table, want);
}

#[test]
fn table_request_has_the_documented_main_and_section_headers() {
    let table = encode_record_table(&[entry("REC001", 300, 1), entry("LAST", 60, 3)]);
    let f = record_table_request(9, &table).expect("100 bytes fit in the size field");
    assert_eq!(f, expected_frame(9, &[a(), c()]));
    // 0x48 | 9 << 8 | (100 / 4 + 2) << 22; then att 0x28 | 100 << 22.
    assert_eq!(&f[..4], &(0x48u32 | (9 << 8) | (27 << 22)).to_le_bytes());
    assert_eq!(&f[4..8], &(0x28u32 | (100 << 22)).to_le_bytes());
    assert_eq!(f.len(), 8 + 100);
}

#[test]
fn table_request_of_an_empty_table_is_twelve_bytes() {
    let f = record_table_request(2, &encode_record_table(&[])).expect("4 bytes fit");
    assert_eq!(f, expected_frame(2, &[]));
    assert_eq!(f.len(), 12);
}

#[test]
fn table_request_holds_21_entries_and_refuses_22() {
    let list: Vec<RecordEntry> = entries(22)
        .iter()
        .map(|e| RecordEntry::decode(e).unwrap())
        .collect();
    let t21 = encode_record_table(&list[..21]);
    assert_eq!(t21.len(), 1012);
    let f = record_table_request(1, &t21).expect("1012 bytes fit in 10 bits");
    assert_eq!(f, expected_frame(1, &entries(21)));
    let t22 = encode_record_table(&list);
    assert_eq!(t22.len(), 1060);
    assert_eq!(record_table_request(1, &t22), None, "1060 bytes do not fit");
}

// ---- AC-JKWT3V.1 / .5: the delete sends exactly one SET_DATA 0x28 ----

#[test]
fn delete_one_sends_one_set_data_with_the_rewritten_table_then_reads_the_list_again() {
    let listed = [a(), b(), c()];
    let mut s = session(
        &listed,
        vec![
            Step::transact(expected_frame(4, &[a(), c()]), accept(4)),
            Step::transact(get_data(5, 0x200), list_reply(5, &[a(), c()])),
        ],
    );
    let list = s.list_records().unwrap();
    let after = s.delete_records(&list, &[1]).expect("delete succeeds");
    assert_eq!(after, vec![list[0].clone(), list[2].clone()]);
    assert_eq!(
        written(&s),
        &[
            memread_request(1, UID_ADDR, 12),
            get_data(3, 0x200),
            expected_frame(4, &[a(), c()]),
            get_data(5, 0x200),
        ]
    );
    assert_eq!(s.transport().inner().inner.remaining(), 0);
}

#[test]
fn delete_of_the_first_and_of_the_last_entry_keep_the_rest_in_order() {
    for (pos, rest) in [(0usize, vec![b(), c()]), (2, vec![a(), b()])] {
        let mut s = session(
            &[a(), b(), c()],
            vec![
                Step::transact(expected_frame(4, &rest), accept(4)),
                Step::transact(get_data(5, 0x200), list_reply(5, &rest)),
            ],
        );
        let list = s.list_records().unwrap();
        let after = s.delete_records(&list, &[pos]).expect("delete succeeds");
        assert_eq!(after.len(), 2, "position {pos}");
        assert_eq!(set_data_frames(&s), vec![expected_frame(4, &rest)]);
    }
}

#[test]
fn delete_all_sends_an_empty_table_and_expects_an_empty_list() {
    let mut s = session(
        &[a(), b(), c()],
        vec![
            Step::transact(expected_frame(4, &[]), accept(4)),
            Step::transact(get_data(5, 0x200), list_reply(5, &[])),
        ],
    );
    let list = s.list_records().unwrap();
    let after = s
        .delete_records(&list, &[0, 1, 2])
        .expect("delete succeeds");
    assert!(after.is_empty());
    assert_eq!(set_data_frames(&s), vec![expected_frame(4, &[])]);
}

#[test]
fn delete_all_of_40_records_fits_because_the_table_is_empty() {
    let listed = entries(40);
    let mut s = session(
        &listed,
        vec![
            Step::transact(expected_frame(4, &[]), accept(4)),
            Step::transact(get_data(5, 0x200), list_reply(5, &[])),
        ],
    );
    let list = s.list_records().unwrap();
    let all: Vec<usize> = (0..list.len()).collect();
    assert_eq!(s.delete_records(&list, &all).unwrap(), Vec::new());
}

#[test]
fn the_delete_path_gets_through_the_shared_guard() {
    // The session runs over GuardedTransport: its write reaches the pipe,
    // while the same frame on the shared path is refused.
    let mut s = session(
        &[a(), b()],
        vec![
            Step::transact(expected_frame(4, &[a()]), accept(4)),
            Step::transact(get_data(5, 0x200), list_reply(5, &[a()])),
        ],
    );
    let list = s.list_records().unwrap();
    s.delete_records(&list, &[1]).expect("delete succeeds");
    assert_eq!(set_data_frames(&s).len(), 1);
    let mut g = GuardedTransport::new(powerz_protocol::transport::MockTransport::new(vec![]));
    use powerz_protocol::transport::Transport;
    assert!(
        g.transact(
            &expected_frame(4, &[a()]),
            std::time::Duration::from_millis(10)
        )
        .is_err()
    );
    assert!(g.inner().written().is_empty());
}

#[test]
fn delete_of_a_position_not_in_the_list_sends_nothing() {
    let mut s = session(&[a(), b()], vec![]);
    let list = s.list_records().unwrap();
    for bad in [vec![2usize], vec![0, 9]] {
        assert!(s.delete_records(&list, &bad).is_err(), "{bad:?}");
    }
    assert!(set_data_frames(&s).is_empty());
    assert_eq!(written(&s).len(), 2, "only the auth read and the list");
}

// ---- AC-JKWT3V.4: the re-read must equal the expected table ----

#[test]
fn a_re_read_that_differs_is_a_mismatch_error_and_nothing_is_retried() {
    // The meter accepted the write but still lists every record.
    let mut s = session(
        &[a(), b(), c()],
        vec![
            Step::transact(expected_frame(4, &[a(), c()]), accept(4)),
            Step::transact(get_data(5, 0x200), list_reply(5, &[a(), b(), c()])),
        ],
    );
    let list = s.list_records().unwrap();
    let err = s.delete_records(&list, &[1]).expect_err("mismatch");
    assert!(
        matches!(err, Error::RecordTableMismatch { .. }),
        "got {err:?}"
    );
    assert_eq!(set_data_frames(&s).len(), 1, "the write is not retried");
    assert_eq!(written(&s).len(), 4, "no frame after the re-read");
    assert_eq!(s.transport().inner().inner.remaining(), 0);
}

#[test]
fn a_re_read_with_the_right_count_but_other_bytes_is_a_mismatch() {
    let mut changed = c();
    changed[0x30 - 1] ^= 0xFF;
    let mut s = session(
        &[a(), b(), c()],
        vec![
            Step::transact(expected_frame(4, &[a(), c()]), accept(4)),
            Step::transact(get_data(5, 0x200), list_reply(5, &[a(), changed])),
        ],
    );
    let list = s.list_records().unwrap();
    assert!(matches!(
        s.delete_records(&list, &[1]),
        Err(Error::RecordTableMismatch { .. })
    ));
}

// ---- FR Notes: the 10-bit size limit ----

#[test]
fn a_delete_leaving_22_or_more_entries_is_refused_before_anything_is_sent() {
    for n in [23u32, 30, 40] {
        let mut s = session(&entries(n), vec![]);
        let list = s.list_records().unwrap();
        let err = s.delete_records(&list, &[0]).expect_err("too long");
        assert!(
            matches!(err, Error::RecordTableTooLong { .. }),
            "{n}: got {err:?}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains(&(n - 1).to_string()) && msg.contains("1023"),
            "the message names the remaining count and the limit: {msg}"
        );
        assert!(set_data_frames(&s).is_empty(), "{n}: a SET_DATA was sent");
        assert_eq!(written(&s).len(), 2, "{n}: only the auth read and the list");
    }
}

#[test]
fn a_delete_leaving_21_entries_is_sent() {
    let listed = entries(22);
    let rest = entries(21);
    let mut s = session(
        &listed,
        vec![
            Step::transact(expected_frame(4, &rest), accept(4)),
            Step::transact(get_data(5, 0x200), list_reply(5, &rest)),
        ],
    );
    let list = s.list_records().unwrap();
    let after = s.delete_records(&list, &[21]).expect("21 remaining fit");
    assert_eq!(after.len(), 21);
    assert_eq!(set_data_frames(&s), vec![expected_frame(4, &rest)]);
}

// ---- AC-JKWT3V.1: the reply the real meter sends ----

/// The KM003C answered the record-table write of request id 7 with
/// `07 07 00 00` (FINISHED, same id) and had deleted the record
/// `[device]` (JKWT3X run 2026-10-29).
const REAL_FINISHED_REPLY: [u8; 4] = [0x07, 0x07, 0x00, 0x00];

#[test]
fn a_finished_reply_with_the_request_id_is_success_as_on_the_real_meter() {
    let listed = [a(), b(), c()];
    // Lists at ids 3..=6, so the write goes out with id 7 like on the meter.
    let lists: Vec<Step> = (4..=6)
        .map(|id| Step::transact(get_data(id, 0x200), list_reply(id, &listed)))
        .collect();
    let mut s = session(
        &listed,
        [
            lists,
            vec![
                Step::transact(expected_frame(7, &[a(), b()]), REAL_FINISHED_REPLY),
                Step::transact(get_data(8, 0x200), list_reply(8, &[a(), b()])),
            ],
        ]
        .concat(),
    );
    let mut list = s.list_records().unwrap();
    for _ in 4..=6 {
        list = s.list_records().unwrap();
    }
    let after = s.delete_records(&list, &[2]).expect("FINISHED is success");
    assert_eq!(after, vec![list[0].clone(), list[1].clone()]);
    assert_eq!(set_data_frames(&s), vec![expected_frame(7, &[a(), b()])]);
    assert_eq!(s.transport().inner().inner.remaining(), 0);
}

#[test]
fn a_rejected_delete_is_an_error_and_is_not_retried() {
    let mut s = session(
        &[a(), b(), c()],
        vec![Step::transact(expected_frame(4, &[a(), c()]), reject(4))],
    );
    let list = s.list_records().unwrap();
    let err = s.delete_records(&list, &[1]).expect_err("REJECT");
    assert!(matches!(err, Error::Rejected { .. }), "got {err:?}");
    assert_eq!(set_data_frames(&s).len(), 1, "the write is not retried");
    assert_eq!(written(&s).len(), 3, "no re-read after a REJECT");
}

#[test]
fn any_other_reply_to_the_delete_is_an_error() {
    // DISCONNECT and a PUT_DATA with the request id are neither ACCEPT nor
    // FINISHED.
    for reply in [vec![0x03, 4, 0, 0], vec![0x41, 4, 0, 0]] {
        let mut s = session(
            &[a(), b(), c()],
            vec![Step::transact(
                expected_frame(4, &[a(), c()]),
                reply.clone(),
            )],
        );
        let list = s.list_records().unwrap();
        let err = s.delete_records(&list, &[1]).expect_err("not a success");
        assert!(matches!(err, Error::Protocol(_)), "{reply:02x?}: {err:?}");
        assert_eq!(written(&s).len(), 3, "{reply:02x?}: no re-read");
    }
}
