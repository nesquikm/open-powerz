//! FR W8SHXA: deleting offline records in the browser.
//!
//! - AC-W8SHXA.1: `RecordBook::delete_targets` works over the latest list: the
//!   typed confirmation must equal the record's name (or "all" for every
//!   record), else nothing is planned, with the desktop worker's wording.
//! - AC-W8SHXA.2: `backup_file` names and writes a record's backup exactly as
//!   the desktop worker does (`powerz_record::offline_record_file_name` +
//!   `offline_record_csv`), a record with no samples included.
//! - AC-W8SHXA.3: `delete_request` is byte for byte the SET_DATA 0x48 att 0x28
//!   frame the native `Session::delete_records` sends (the kept entries,
//!   unchanged and in order); a table too long to write is refused by
//!   `delete_targets` (before any backup) and `delete_request`; "all" on an
//!   empty list plans nothing.
//! - AC-W8SHXA.4: `take_delete_reply` takes ACCEPT or FINISHED, fails on REJECT
//!   (desktop wording) or anything else; `check_after_delete` compares the
//!   re-read list with the kept entries (desktop RecordTableMismatch wording).
//! - AC-W8SHXA.6: the only SET_DATA the binding builds passes the native
//!   `guard::check_record_table_write` and carries attribute 0x28.
//!
//! The native side is a `powerz_protocol::session::Session` over the demo sim
//! (which serves and rewrites the record table).

mod common;

use common::*;
use powerz_protocol::decode::records::RecordEntry;
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::session::{Error as SessionError, Session};
use powerz_protocol::transport::guard::check_record_table_write;
use powerz_sim::{FaultPlan, SimClock};
use powerz_wasm::{BackupFile, BindError, RecordBook, decode_memory_read};
use serde_json::json;

const NOT_LISTED: &str = "That record is not on the meter.";
const CONFIRM_MISMATCH: &str = "The typed confirmation does not match; nothing was deleted.";
const TOO_LONG: &str = "Deleting that would leave more records than one table write can hold.";
const TOO_LONG_HINT: &str = "Delete more records at once, or all of them.";
const REFUSED: &str = "The meter refused the request.";
const REFUSED_HINT: &str = "Updating the meter's firmware may help.";
const MISMATCH_HINT: &str = "Read the list again; nothing was retried.";

fn session_with(faults: FaultPlan) -> Session<Recorder, FixedClock> {
    let clock = SimClock::new(0);
    let recorder = Recorder {
        sim: demo_sim(&clock).with_faults(faults),
        log: Vec::new(),
    };
    Session::with_clock(recorder, FixedClock)
}

/// GET_DATA exchanges whose mask is exactly RECORD_LIST.
fn list_exchanges(s: &Session<Recorder, FixedClock>) -> Vec<Exchange> {
    exchanges_of(s, 0x0C)
        .into_iter()
        .filter(|e| {
            frame::get_data_mask(&e.request).map(|a| a.bits())
                == Some(Attribute::RECORD_LIST.bits())
        })
        .collect()
}

fn last_list_reply(s: &Session<Recorder, FixedClock>) -> Vec<u8> {
    list_exchanges(s)
        .pop()
        .expect("the session sent a RecordList request")
        .reply()
}

/// A connected native session over the sim with `faults` that listed the
/// records; returns the entries and the RecordList reply.
fn listed_with(faults: FaultPlan) -> (Session<Recorder, FixedClock>, Vec<RecordEntry>, Vec<u8>) {
    let mut s = session_with(faults);
    s.connect().unwrap();
    let entries = s.list_records().unwrap();
    let reply = last_list_reply(&s);
    (s, entries, reply)
}

fn listed() -> (Session<Recorder, FixedClock>, Vec<RecordEntry>, Vec<u8>) {
    listed_with(FaultPlan::new())
}

fn book_of(reply: &[u8]) -> RecordBook {
    let mut book = RecordBook::new();
    book.decode_list(reply).unwrap();
    book
}

fn assert_err(err: &BindError, kind: &str, message: &str, hint: Option<&str>) {
    assert_eq!(err.kind, kind, "{err:?}");
    assert_eq!(err.message, message, "{err:?}");
    assert_eq!(err.hint.as_deref(), hint, "{err:?}");
}

fn control(ty: u8, id: u8) -> Vec<u8> {
    vec![ty, id, 0x00, 0x00]
}

// ---- AC-W8SHXA.1: targets and the typed confirmation ----

#[test]
fn one_record_is_planned_when_the_confirmation_is_its_name() {
    let (_, entries, reply) = listed();
    let book = book_of(&reply);
    for (i, e) in entries.iter().enumerate() {
        let index = (i + 1) as u32;
        assert_eq!(
            book.delete_targets(Some(index), &e.name).unwrap(),
            vec![index]
        );
    }
}

#[test]
fn every_record_is_planned_when_the_confirmation_is_all() {
    let (_, entries, reply) = listed();
    assert_eq!(entries.len(), 3);
    let book = book_of(&reply);
    assert_eq!(book.delete_targets(None, "all").unwrap(), vec![1, 2, 3]);
}

#[test]
fn a_wrong_confirmation_plans_nothing_with_the_desktop_wording() {
    let (_, entries, reply) = listed();
    let book = book_of(&reply);
    let name = entries[0].name.as_str();
    for (target, confirm) in [
        (Some(1), "all"),
        (Some(1), ""),
        (Some(1), "phone_5v"),
        (Some(1), "PHONE_5V "),
        (Some(1), entries[1].name.as_str()),
        (None, name),
        (None, "ALL"),
        (None, ""),
    ] {
        let err = book.delete_targets(target, confirm).unwrap_err();
        assert_err(&err, "usage", CONFIRM_MISMATCH, None);
    }
}

#[test]
fn an_unlisted_record_is_refused_with_the_desktop_wording() {
    let (_, entries, reply) = listed();
    let book = book_of(&reply);
    for index in [0, 4, 99] {
        let err = book
            .delete_targets(Some(index), &entries[0].name)
            .unwrap_err();
        assert_err(&err, "usage", NOT_LISTED, None);
    }
    // Before any list nothing is on the meter.
    let fresh = RecordBook::new();
    let err = fresh.delete_targets(Some(1), "PHONE_5V").unwrap_err();
    assert_err(&err, "usage", NOT_LISTED, None);
}

#[test]
fn all_on_an_empty_list_plans_nothing() {
    let (mut s, entries, _) = listed();
    s.delete_records(&entries, &[0, 1, 2]).unwrap();
    let book = book_of(&last_list_reply(&s));
    assert_eq!(book.delete_targets(None, "all").unwrap(), Vec::<u32>::new());
    // A wrong confirmation is still refused on an empty list.
    let err = book.delete_targets(None, "nope").unwrap_err();
    assert_err(&err, "usage", CONFIRM_MISMATCH, None);
}

// ---- AC-W8SHXA.3: the too-long table is refused before the backup ----

#[test]
fn a_table_too_long_to_write_is_refused_by_the_planner() {
    let (_, entries, reply) = listed_with(FaultPlan::new().record_count(23));
    assert_eq!(entries.len(), 23);
    let book = book_of(&reply);
    let err = book.delete_targets(Some(1), &entries[0].name).unwrap_err();
    assert_err(&err, "usage", TOO_LONG, Some(TOO_LONG_HINT));
    // The confirmation is checked first, as on the desktop.
    let err = book.delete_targets(Some(1), "wrong").unwrap_err();
    assert_err(&err, "usage", CONFIRM_MISMATCH, None);
    // Deleting all of them leaves an empty table, which fits.
    let all = book.delete_targets(None, "all").unwrap();
    assert_eq!(all, (1..=23).collect::<Vec<u32>>());
}

#[test]
fn a_table_too_long_to_write_builds_no_frame() {
    let (_, _, reply) = listed_with(FaultPlan::new().record_count(23));
    let mut book = book_of(&reply);
    let err = book.delete_request(4, &[1]).unwrap_err();
    assert!(!err.message.is_empty(), "{err:?}");
    assert!(
        book.delete_request(4, &[]).is_err(),
        "nothing deleted is too long too"
    );
}

#[test]
fn delete_request_refuses_unlisted_records() {
    let (_, _, reply) = listed();
    let mut book = book_of(&reply);
    for bad in [&[0u32][..], &[4], &[1, 99]] {
        assert!(book.delete_request(4, bad).is_err(), "{bad:?}");
    }
    assert!(RecordBook::new().delete_request(4, &[1]).is_err());
}

// ---- AC-W8SHXA.3 / .4: the frame and the re-read, against the native delete ----

/// Runs the native delete of 0-based `positions` and the binding's of the same
/// records; checks frame, reply handling and re-read parity.
fn delete_parity(positions: &[usize]) {
    let (mut s, entries, reply) = listed();
    let native_after = s.delete_records(&entries, positions).unwrap();

    let writes = exchanges_of(&s, 0x48);
    assert_eq!(writes.len(), 1, "one table write");
    let write = &writes[0];

    let mut book = book_of(&reply);
    let indices: Vec<u32> = positions.iter().map(|&p| (p + 1) as u32).collect();
    let frame = book.delete_request(id_of(write), &indices).unwrap();
    assert_eq!(
        frame, write.request,
        "{positions:?}: the native frame, byte for byte"
    );

    book.take_delete_reply(&write.reply()).unwrap();
    let view = book.decode_list(&last_list_reply(&s)).unwrap();
    book.check_after_delete().unwrap();
    let names: Vec<String> = native_after.iter().map(|e| e.name.clone()).collect();
    let got: Vec<String> = view.records.iter().map(|r| r.name.clone()).collect();
    assert_eq!(got, names);
    let kept: Vec<&RecordEntry> = entries
        .iter()
        .enumerate()
        .filter(|(i, _)| !positions.contains(i))
        .map(|(_, e)| e)
        .collect();
    assert_eq!(native_after.iter().collect::<Vec<_>>(), kept);
}

#[test]
fn deleting_one_record_sends_the_native_frame() {
    for p in 0..3 {
        delete_parity(&[p]);
    }
}

#[test]
fn deleting_two_records_sends_the_native_frame() {
    delete_parity(&[0, 2]);
}

#[test]
fn deleting_every_record_sends_the_native_frame() {
    delete_parity(&[0, 1, 2]);
}

// ---- AC-W8SHXA.6: only the record-table write ----

#[test]
fn the_only_set_data_built_is_the_guarded_record_table_write() {
    let (_, entries, reply) = listed();
    let mut book = book_of(&reply);
    for indices in [&[1u32][..], &[2], &[1, 3], &[1, 2, 3]] {
        let f = book.delete_request(9, indices).unwrap();
        assert_eq!(f[0] & 0x7F, 0x48, "SET_DATA");
        assert_eq!(f[1], 9, "the request id");
        let ext = u32::from_le_bytes([f[4], f[5], f[6], f[7]]);
        assert_eq!(ext & 0x7FFF, 0x28, "attribute 0x28");
        assert_eq!(check_record_table_write(&f), Ok(()), "{indices:?}");
        // The kept entries' table: one 48-byte entry per kept record, plus 4.
        assert_eq!(f.len(), 8 + (entries.len() - indices.len()) * 48 + 4);
    }
}

// ---- AC-W8SHXA.4: the write's reply ----

#[test]
fn accept_or_finished_counts_as_written() {
    let (_, _, reply) = listed();
    for ty in [0x05, 0x07] {
        let mut book = book_of(&reply);
        book.delete_request(3, &[1]).unwrap();
        book.take_delete_reply(&control(ty, 3)).unwrap();
    }
}

#[test]
fn a_reject_fails_with_the_desktop_wording() {
    let (_, _, reply) = listed();
    let mut book = book_of(&reply);
    book.delete_request(3, &[1]).unwrap();
    let err = book.take_delete_reply(&control(0x06, 3)).unwrap_err();
    assert_err(&err, "protocol", REFUSED, Some(REFUSED_HINT));
}

#[test]
fn any_other_reply_fails() {
    let (_, _, reply) = listed();
    for other in [vector("adc_52.hex"), control(0x01, 3), vec![], vec![0x41]] {
        let mut book = book_of(&reply);
        book.delete_request(3, &[1]).unwrap();
        let err = book.take_delete_reply(&other).unwrap_err();
        assert_eq!(err.kind, "protocol", "{other:02x?}: {err:?}");
        assert!(!err.message.is_empty());
        assert_ne!(err.message, REFUSED, "{other:02x?}");
    }
}

// ---- AC-W8SHXA.4: the re-read must equal the kept entries ----

#[test]
fn a_re_read_with_another_count_is_the_desktop_mismatch() {
    let (mut s, entries, reply) = listed_with(FaultPlan::new().ignore_table_write());
    let native = s.delete_records(&entries, &[0]).unwrap_err();
    assert!(
        matches!(
            native,
            SessionError::RecordTableMismatch {
                expected: 2,
                actual: 3
            }
        ),
        "{native:?}"
    );
    let write = exchanges_of(&s, 0x48).pop().unwrap();
    let mut book = book_of(&reply);
    assert_eq!(
        book.delete_request(id_of(&write), &[1]).unwrap(),
        write.request
    );
    book.take_delete_reply(&write.reply()).unwrap();
    book.decode_list(&last_list_reply(&s)).unwrap();
    let err = book.check_after_delete().unwrap_err();
    assert_err(
        &err,
        "protocol",
        "The record list does not match after the delete: the meter lists 3 records, expected 2.",
        Some(MISMATCH_HINT),
    );
}

#[test]
fn a_re_read_with_the_right_count_but_other_entries_is_the_desktop_mismatch() {
    // The meter's list after deleting record 2 ([1, 3]), read back after the
    // binding deleted record 1 (kept [2, 3]).
    let (mut s, entries, reply) = listed();
    s.delete_records(&entries, &[1]).unwrap();
    let other_list = last_list_reply(&s);

    let mut book = book_of(&reply);
    book.delete_request(5, &[1]).unwrap();
    book.take_delete_reply(&control(0x07, 5)).unwrap();
    book.decode_list(&other_list).unwrap();
    let err = book.check_after_delete().unwrap_err();
    assert_err(
        &err,
        "protocol",
        "The record list does not match after the delete: 2 records as expected, but the entries differ.",
        Some(MISMATCH_HINT),
    );
}

// ---- AC-W8SHXA.2: the backup file ----

/// The record window bytes for 1-based `index`, read through the binding's
/// chunk plan from the native download's replies.
fn window_bytes(
    s: &mut Session<Recorder, FixedClock>,
    book: &RecordBook,
    entry: &RecordEntry,
    index: u32,
) -> Vec<u8> {
    let before = exchanges_of(s, 0x44).len();
    s.download_record(entry).unwrap();
    let reads = exchanges_of(s, 0x44).split_off(before);
    let chunks = book.chunks(index).unwrap();
    assert_eq!(chunks.len(), reads.len());
    let mut data = Vec::new();
    for (c, e) in chunks.iter().zip(&reads) {
        data.extend(decode_memory_read(&e.reply(), c.addr, c.len).unwrap());
    }
    data
}

#[test]
fn backup_file_is_the_desktop_backup() {
    let (mut s, entries, reply) = listed();
    let book = book_of(&reply);
    for (i, entry) in entries.iter().enumerate() {
        let index = (i + 1) as u32;
        let samples = s.download_record(entry).unwrap();
        let data = window_bytes(&mut s, &book, entry, index);
        let file: BackupFile = book.backup_file(index, &data).unwrap();
        let name = powerz_record::offline_record_file_name(i + 1, &entry.name);
        let csv = powerz_record::offline_record_csv(entry, &samples);
        assert_eq!(file.name, name, "record {index}");
        assert_eq!(file.csv, csv, "record {index}");
        assert_eq!(json(&file), json!({ "name": name, "csv": csv }));
        assert!(file.name.ends_with(".csv"), "{}", file.name);
        assert!(
            file.csv.lines().count() > usize::from(entry.points),
            "header plus samples"
        );
    }
}

#[test]
fn backup_file_of_a_record_with_no_samples_is_the_desktop_backup() {
    let (_, entries, reply) = listed_with(FaultPlan::new().record_count(5));
    let book = book_of(&reply);
    let p = entries
        .iter()
        .position(|e| e.points == 0)
        .expect("a padded record");
    let entry = &entries[p];
    let file = book.backup_file((p + 1) as u32, &[]).unwrap();
    assert_eq!(
        file.name,
        powerz_record::offline_record_file_name(p + 1, &entry.name)
    );
    assert_eq!(file.csv, powerz_record::offline_record_csv(entry, &[]));
}

#[test]
fn backup_file_refuses_wrong_data_and_unlisted_records() {
    let (_, entries, reply) = listed();
    let book = book_of(&reply);
    let want = usize::from(entries[0].points) * 16;
    for len in [0, want - 16, want + 16] {
        let err = book.backup_file(1, &vec![0u8; len]).unwrap_err();
        assert_eq!(err.kind, "malformed", "{len} bytes: {err:?}");
    }
    let err = book.backup_file(9, &[]).unwrap_err();
    assert_err(&err, "usage", NOT_LISTED, None);
}
