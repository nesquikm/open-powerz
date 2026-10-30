//! AC-JKWT3V.6: the demo meter keeps a record table that a delete changes.
//! A SET_DATA (0x48) with attribute 0x28 replaces the table with the
//! entries it carries (the data minus the four zero bytes) and answers
//! FINISHED, as the real meter does; every other SET_DATA attribute is
//! REJECTed and changes nothing. Two faults stand in for the failure paths:
//! `reject-record-reads` REJECTs every memory read inside the record area
//! (the backup download fails; the UID and info reads still work), and
//! `ignore-table-write` answers the write with FINISHED but keeps the old
//! table (the re-read does not match).

mod common;

use common::*;
use powerz_protocol::decode::records::{ENTRY_LEN, RecordEntry, decode_record_list};
use powerz_protocol::frame::Attribute;
use powerz_protocol::session::{Error, Session};
use powerz_protocol::transport::{GuardedTransport, Transport};

const RECORD_LIST: Attribute = Attribute(0x200);

/// Bulk session with `faults`, connected and at level 1.
fn authed_with(faults: FaultPlan) -> (SimSession, SimClock) {
    let clock = clock();
    let dev = device(Persona::Bulk, Scenario::Idle, &clock).with_faults(faults);
    let mut s = session(dev, &clock);
    s.connect().expect("CONNECT");
    assert_eq!(s.authenticate(1), Ok(1));
    (s, clock)
}

fn listed_with(faults: FaultPlan) -> (SimSession, Vec<RecordEntry>) {
    let (mut s, _clock) = authed_with(faults);
    let list = s.list_records().expect("list");
    assert!(list.len() >= 3, "{list:?}");
    (s, list)
}

/// A raw SET_DATA frame laid out like the record-table write.
fn set_data(id: u8, att: u16, data: &[u8]) -> Vec<u8> {
    let len = data.len() as u32;
    let mut f = (0x48u32 | (u32::from(id) << 8) | ((len / 4 + 2) << 22))
        .to_le_bytes()
        .to_vec();
    f.extend((u32::from(att) | ((len & 0x3FF) << 22)).to_le_bytes());
    f.extend_from_slice(data);
    f
}

fn table_of(list: &[RecordEntry]) -> Vec<u8> {
    let mut d: Vec<u8> = list.iter().flat_map(|e| e.raw).collect();
    d.extend([0u8; 4]);
    d
}

#[test]
fn delete_one_removes_it_from_the_list_the_meter_serves_afterwards() {
    let (mut s, list) = listed_with(FaultPlan::new());
    let after = s.delete_records(&list, &[1]).expect("delete");
    let want: Vec<RecordEntry> = list
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 1)
        .map(|(_, e)| e.clone())
        .collect();
    assert_eq!(after, want);
    // The table stays changed for later reads.
    assert_eq!(s.list_records().unwrap(), want);
    assert!(
        !s.list_records()
            .unwrap()
            .iter()
            .any(|e| e.name == list[1].name)
    );
}

#[test]
fn records_left_after_a_delete_still_download() {
    let (mut s, list) = listed_with(FaultPlan::new());
    let after = s.delete_records(&list, &[0]).expect("delete");
    let samples = s.download_record(&after[0]).expect("download");
    assert_eq!(samples.len(), usize::from(after[0].points));
}

#[test]
fn delete_all_leaves_an_empty_list() {
    let (mut s, list) = listed_with(FaultPlan::new());
    let all: Vec<usize> = (0..list.len()).collect();
    assert_eq!(s.delete_records(&list, &all).expect("delete"), Vec::new());
    assert_eq!(s.list_records().unwrap(), Vec::new());
}

#[test]
fn two_deletes_in_a_row_each_change_the_table() {
    let (mut s, list) = listed_with(FaultPlan::new());
    let once = s.delete_records(&list, &[0]).expect("first delete");
    let twice = s.delete_records(&once, &[0]).expect("second delete");
    assert_eq!(twice, list[2..].to_vec());
}

#[test]
fn the_delete_works_through_the_shared_guard() {
    let clock = clock();
    let dev = device(Persona::Bulk, Scenario::Idle, &clock);
    let mut s = Session::with_clock(GuardedTransport::new(dev), clock.clone());
    s.connect().unwrap();
    let list = s.list_records().unwrap();
    let after = s.delete_records(&list, &[2]).expect("delete");
    assert_eq!(after, list[..2].to_vec());
}

#[test]
fn a_raw_table_write_replaces_the_table_and_is_finished() {
    let (mut s, list) = listed_with(FaultPlan::new());
    let keep = [list[2].clone(), list[0].clone()];
    let dev = s.transport_mut();
    let reply = dev
        .transact(&set_data(0x30, 0x28, &table_of(&keep)), T)
        .unwrap();
    // FINISHED with the request id, as the KM003C answers [device].
    assert_eq!(&reply[..2], &[0x07, 0x30], "FINISHED: {reply:02x?}");
    let reply = dev.transact(&get_data(0x31, RECORD_LIST), T).unwrap();
    let secs = sections(&reply);
    assert_eq!(secs.len(), 1);
    assert_eq!(secs[0].1.len(), 2 * ENTRY_LEN);
    assert_eq!(decode_record_list(&secs[0].1).unwrap(), keep.to_vec());
}

#[test]
fn every_other_set_data_attribute_is_rejected_and_changes_nothing() {
    let (mut s, list) = listed_with(FaultPlan::new());
    let dev = s.transport_mut();
    for (k, att) in [0x08u16, 0x09, 0x0A, 0x0B, 0x1C, 0x27, 0x29]
        .into_iter()
        .enumerate()
    {
        let id = 0x40 + k as u8;
        let reply = dev.transact(&set_data(id, att, &table_of(&[])), T).unwrap();
        assert_eq!(ty(&reply), 0x06, "att {att:#x}: {reply:02x?}");
    }
    assert_eq!(s.list_records().unwrap(), list);
}

// ---- faults: a failed backup and a mismatching re-read ----

#[test]
fn reject_record_reads_fails_the_download_but_not_the_list_or_auth() {
    let (mut s, list) = listed_with(FaultPlan::new().reject_record_reads());
    let first = list.iter().find(|e| e.points > 0).unwrap();
    assert!(s.download_record(first).is_err());
    // The info block (outside the record area) still reads.
    assert!(s.device_info().is_ok());
    // The table itself is untouched.
    assert_eq!(s.list_records().unwrap(), list);
}

#[test]
fn ignore_table_write_finishes_the_write_but_keeps_the_table() {
    let (mut s, list) = listed_with(FaultPlan::new().ignore_table_write());
    let err = s.delete_records(&list, &[0]).expect_err("mismatch");
    assert!(
        matches!(err, Error::RecordTableMismatch { .. }),
        "got {err:?}"
    );
    assert_eq!(s.list_records().unwrap(), list);
}

#[test]
fn the_record_faults_parse_from_the_env_syntax() {
    assert_eq!(
        FaultPlan::parse("reject-record-reads"),
        Ok(FaultPlan::new().reject_record_reads())
    );
    assert_eq!(
        FaultPlan::parse("ignore-table-write"),
        Ok(FaultPlan::new().ignore_table_write())
    );
    assert_eq!(
        FaultPlan::parse("reject-record-reads, ignore-table-write"),
        Ok(FaultPlan::new().reject_record_reads().ignore_table_write())
    );
    let err = FaultPlan::parse("bogus").unwrap_err();
    assert!(err.contains("reject-record-reads"), "{err}");
    assert!(err.contains("ignore-table-write"), "{err}");
}
