//! Desktop PD capture save and load (FR TGE83E, AC .2): a capture saved as
//! a pdcap file and opened again gives the packet table, the details of
//! every record and the timeline's status samples exactly as they were.
//!
//! API pinned here (`open_powerz_desktop_lib::pd_capture::PdCapture`):
//!
//! - `to_pdcap(&self) -> Vec<powerz_protocol::pdcap::Entry>`: every status
//!   sample and record kept so far, in capture order, on session time. It
//!   reads what the capture keeps, not the rows waiting to go out, so rows
//!   already sent to the UI are saved too.
//! - `PdCapture::from_pdcap(&[Entry]) -> PdCapture`: a capture rebuilt from
//!   a file, with every row and status sample waiting to go out
//!   (`take_all`, `take_all_status`) and every record's details answered.
//!
//! Every byte vector here is synthetic.

use std::io::Cursor;

use open_powerz_desktop_lib::pd_capture::{PdCapture, PdDetails, PdRow, PdStatusSample};
use powerz_protocol::decode::pd_packet::{CaptureClock, PdChunk, PdStatus};
use powerz_protocol::pdcap::{self, Entry, FORMAT_VERSION, Header};
use powerz_protocol::session::CapturedRecord;
use proptest::prelude::*;

fn status_bytes(s: &PdStatus) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(s.time_ms.to_le_bytes());
    b.extend(s.vbus_mv.to_le_bytes());
    b.extend(s.ibus_ma.to_le_bytes());
    b.extend(s.cc1_mv.to_le_bytes());
    b.extend(s.cc2_mv.to_le_bytes());
    b
}

/// A PD message record (b0 bit 7).
fn msg(meter: bool, ts: u32, sop: u8, header: u16, body: &[u8]) -> Vec<u8> {
    let len = 8 + body.len();
    let mut r = vec![0x80 | if meter { 0x40 } else { 0 } | (len as u8 - 1)];
    r.extend(ts.to_le_bytes());
    r.push(sop);
    r.extend(header.to_le_bytes());
    r.extend(body);
    r
}

/// An event record with event byte `code`.
fn event(meter: bool, ts: u32, code: u8) -> Vec<u8> {
    let mut r = vec![0x05 | if meter { 0x40 } else { 0 }];
    r.extend(ts.to_le_bytes());
    r.push(code);
    r
}

/// One poll of the capture: its status block, its records and, before it,
/// a new session offset (a reconnect moves the timeline).
#[derive(Debug, Clone)]
struct Poll {
    offset_ms: Option<i64>,
    status: PdStatus,
    records: Vec<Vec<u8>>,
}

/// Feed `polls` through the capture as the worker does: the chunk parsed by
/// the capture core, record times from a capture clock.
fn capture_of(polls: &[Poll]) -> PdCapture {
    let mut pd = PdCapture::new();
    let mut clock = CaptureClock::new();
    pd.start_run();
    for p in polls {
        if let Some(off) = p.offset_ms {
            pd.set_session_offset(off);
        }
        let mut chunk = status_bytes(&p.status);
        for r in &p.records {
            chunk.extend(r);
        }
        let parsed = PdChunk::parse(&chunk).expect("synthetic chunk parses");
        let records: Vec<CapturedRecord> = parsed
            .records
            .into_iter()
            .map(|record| CapturedRecord {
                elapsed_ms: clock.elapsed_ms(record.time_ms),
                record,
            })
            .collect();
        pd.push(&records, Some(parsed.status));
    }
    pd
}

/// What the UI sees of a capture: its rows, its status samples and the
/// details of every row.
#[derive(Debug, PartialEq)]
struct View {
    rows: Vec<PdRow>,
    status: Vec<PdStatusSample>,
    details: Vec<Option<PdDetails>>,
}

fn view(pd: &mut PdCapture) -> View {
    let rows = pd.take_all().unwrap_or_default();
    let status = pd.take_all_status().unwrap_or_default();
    let details = rows.iter().map(|r| pd.details(r.index)).collect();
    View {
        rows,
        status,
        details,
    }
}

fn header() -> Header {
    Header {
        format_version: FORMAT_VERSION,
        app_version: "0.0.0-test".to_owned(),
        model: "KM003C".to_owned(),
        firmware_version: "2.0.7".to_owned(),
    }
}

/// Save `pd` to pdcap bytes and open them again.
fn save_and_open(pd: &PdCapture) -> PdCapture {
    let mut w = pdcap::Writer::new(Vec::new(), &header()).expect("header written");
    for e in pd.to_pdcap() {
        w.write(&e).expect("entry written");
    }
    let bytes = w.finish().expect("flushed");
    let loaded = pdcap::load(Cursor::new(&bytes));
    assert!(loaded.error.is_none(), "{:?}", loaded.error);
    PdCapture::from_pdcap(&loaded.entries)
}

const SRC_CAPS: [u8; 8] = [0x2C, 0x91, 0x81, 0x09, 0x2C, 0xD1, 0x02, 0x00];

fn status(time_ms: u32, vbus_mv: u16) -> PdStatus {
    PdStatus {
        time_ms,
        vbus_mv,
        ibus_ma: 120,
        cc1_mv: 1_620,
        cc2_mv: 0,
    }
}

/// Attach, Source_Capabilities, Request, Accept, PS_RDY, Hard Reset.
fn negotiation() -> Vec<Poll> {
    vec![
        Poll {
            offset_ms: Some(-7_000),
            status: status(10_000, 5_010),
            records: vec![
                event(true, 10_000, 0x11),
                msg(false, 10_040, 0, 0x21A1, &SRC_CAPS),
            ],
        },
        Poll {
            offset_ms: None,
            status: status(10_300, 5_020),
            records: vec![
                msg(true, 10_042, 0, 0x1082, &[0x2C, 0xB1, 0x44, 0x13]),
                msg(false, 10_050, 0, 0x03A3, &[]),
            ],
        },
        Poll {
            offset_ms: Some(-6_500),
            status: status(10_600, 9_010),
            records: vec![
                msg(false, 10_200, 0, 0x05A6, &[]),
                event(true, 10_250, 0x04),
            ],
        },
        Poll {
            offset_ms: None,
            status: status(10_900, 0),
            records: vec![],
        },
    ]
}

#[test]
fn a_saved_negotiation_opens_with_the_same_table_details_and_timeline() {
    let mut original = capture_of(&negotiation());
    let before = view(&mut original);
    assert_eq!(before.rows.len(), 6, "every record is a row");
    assert_eq!(before.status.len(), 4, "every status block is a sample");

    let mut opened = save_and_open(&original);
    assert_eq!(view(&mut opened), before);
}

#[test]
fn saving_takes_every_status_sample_and_record_in_capture_order() {
    let mut pd = capture_of(&negotiation());
    // The rows went out to the UI before the save.
    let _ = view(&mut pd);
    let entries = pd.to_pdcap();
    let kinds: Vec<&str> = entries
        .iter()
        .map(|e| match e {
            Entry::Status { .. } => "status",
            Entry::Record { .. } => "record",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "status", "record", "record", "status", "record", "record", "status", "record",
            "record", "status"
        ]
    );
}

#[test]
fn saving_does_not_take_the_rows_still_waiting_to_go_out() {
    let mut pd = capture_of(&negotiation());
    let _ = pd.to_pdcap();
    assert_eq!(pd.take_all().map(|r| r.len()), Some(6));
    assert_eq!(pd.take_all_status().map(|s| s.len()), Some(4));
}

/// One synthetic record at meter time `ts`: a message (any header, SOP 0–2,
/// 0–7 data objects) or an event.
fn record_at(ts: u32) -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        3 => (any::<bool>(), 0u8..3, any::<u16>(), proptest::collection::vec(any::<u8>(), 0..=28))
            .prop_map(move |(meter, sop, header, body)| msg(meter, ts, sop, header, &body)),
        1 => (any::<bool>(), any::<u8>()).prop_map(move |(meter, code)| event(meter, ts, code)),
    ]
}

fn poll_strategy() -> impl Strategy<Value = Poll> {
    (
        proptest::option::of(-1_000_000i64..1_000_000),
        any::<u32>(),
        any::<u16>(),
        any::<i16>(),
        any::<u16>(),
        any::<u16>(),
    )
        .prop_flat_map(|(offset_ms, time_ms, vbus_mv, ibus_ma, cc1_mv, cc2_mv)| {
            let status = PdStatus {
                time_ms,
                vbus_mv,
                ibus_ma,
                cc1_mv,
                cc2_mv,
            };
            let records = proptest::collection::vec(
                (0u32..400).prop_flat_map(move |d| record_at(time_ms.wrapping_sub(d))),
                0..5,
            );
            records.prop_map(move |records| Poll {
                offset_ms,
                status,
                records,
            })
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn any_capture_reopens_exactly_as_it_was(
        polls in proptest::collection::vec(poll_strategy(), 0..12),
    ) {
        let mut original = capture_of(&polls);
        let before = view(&mut original);
        let mut opened = save_and_open(&original);
        prop_assert_eq!(view(&mut opened), before);
    }
}
