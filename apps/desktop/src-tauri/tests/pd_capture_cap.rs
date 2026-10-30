//! The backend's PD record store is capped (FR 4PJRVK, AC .3): a long capture
//! drops its oldest records and counts them, so it cannot grow without limit.
//!
//! API pinned here (`open_powerz_desktop_lib::pd_capture`):
//!
//! - `MAX_RECORDS`: how many records a capture keeps by default.
//! - `PdCapture::with_limit(max_records) -> PdCapture`: a capture keeping at
//!   most `max_records` records; `PdCapture::limit(&self) -> usize` tells it.
//! - `PdCapture::dropped(&self) -> u64`: records dropped since the last clear.
//!
//! Record indices keep counting: a dropped record has no details any more and
//! is not saved, and the kept ones keep their indices.
//!
//! Every byte vector here is synthetic.

use open_powerz_desktop_lib::pd_capture::{MAX_RECORDS, PdCapture};
use powerz_protocol::decode::pd_packet::{PdMessage, PdRecord, PdRecordKind};
use powerz_protocol::pdcap::Entry;
use powerz_protocol::session::CapturedRecord;

/// An Accept message (rev 3, source) at `elapsed_ms`.
fn accept(elapsed_ms: u64) -> CapturedRecord {
    CapturedRecord {
        elapsed_ms,
        record: PdRecord {
            time_ms: elapsed_ms as u32,
            from_meter: false,
            raw: Vec::new(),
            kind: PdRecordKind::Message(PdMessage {
                sop: 0,
                header: 0x01A3,
                body: Vec::new(),
            }),
        },
    }
}

/// A capture of `records` records keeping at most `limit`.
fn filled(limit: usize, records: u64) -> PdCapture {
    let mut pd = PdCapture::with_limit(limit);
    for t in 0..records {
        pd.push(&[accept(t)], None);
    }
    pd
}

fn saved_records(pd: &PdCapture) -> Vec<u64> {
    pd.to_pdcap()
        .iter()
        .filter_map(|e| match e {
            Entry::Record { elapsed_ms, .. } => Some(*elapsed_ms),
            Entry::Status { .. } => None,
        })
        .collect()
}

#[test]
fn a_new_capture_keeps_at_most_the_default_limit() {
    const _: () = assert!(
        MAX_RECORDS >= 100_000,
        "the analyzer promises 100,000 records"
    );
    assert_eq!(PdCapture::new().limit(), MAX_RECORDS);
}

#[test]
fn a_capture_within_its_limit_drops_nothing() {
    let pd = filled(10, 10);
    assert_eq!(pd.dropped(), 0);
    assert_eq!(saved_records(&pd).len(), 10);
    assert!(pd.details(0).is_some());
}

#[test]
fn past_the_limit_the_oldest_records_are_dropped_and_counted() {
    let pd = filled(10, 25);
    assert_eq!(pd.dropped(), 15);
    assert_eq!(pd.details(14), None, "a dropped record has no details");
    assert!(pd.details(15).is_some(), "the oldest kept record");
    assert!(pd.details(24).is_some(), "the newest record");
    assert_eq!(pd.details(25), None);
}

#[test]
fn a_save_holds_the_kept_records_only() {
    let pd = filled(10, 25);
    assert_eq!(saved_records(&pd), (15..25).collect::<Vec<u64>>());
}

#[test]
fn rows_keep_counting_past_the_limit() {
    let mut pd = filled(10, 25);
    let rows = pd.take_all().unwrap_or_default();
    assert_eq!(rows.last().map(|r| r.index), Some(24));
}

#[test]
fn a_clear_starts_the_count_of_dropped_records_again() {
    let mut pd = filled(10, 25);
    let next = pd.clear();
    assert_eq!(next, 25, "indices do not restart");
    assert_eq!(pd.dropped(), 0);
    pd.push(&[accept(0)], None);
    assert!(pd.details(25).is_some());
}
