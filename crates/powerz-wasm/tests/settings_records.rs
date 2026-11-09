//! FR W8SHX8: meter settings and offline records in the browser.
//!
//! - AC-W8SHX8.1: `settings_request` is the GET_DATA SETTINGS frame the native
//!   session sends, and `decode_settings` gives the desktop panel's rows.
//! - AC-W8SHX8.2: `RecordBook::list_request` / `decode_list` give the desktop
//!   Records view's list (1-based index, name, interval, points, duration,
//!   charge, energy, offset); an empty list is not an error.
//! - AC-W8SHX8.3: `chunks` plans only the listed record's own window in reads
//!   of at most 256 bytes; `chunk_request` refuses anything outside the latest
//!   list's windows; unlisted / empty records are refused with the desktop
//!   app's wording.
//! - AC-W8SHX8.4: `decode_record` equals the desktop app's downloaded record.
//! - AC-W8SHX8.5: `memory_read_request` still refuses everything but the four
//!   device-info blocks, record windows included.
//!
//! The native side is a `powerz_protocol::session::Session` over the demo sim
//! (bulk persona, manual clock); expected views are built the way the desktop
//! worker builds them.

mod common;

use common::*;
use powerz_protocol::decode::records::{MEMORY_READ_MAX_LEN, RecordEntry, RecordSample};
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::session::Session;
use powerz_sim::{FaultPlan, SimClock};
use powerz_wasm::{
    RecordBook, decode_memory_read, decode_settings, get_data_request, memory_read_request,
    settings_request,
};
use serde_json::{Value, json};

/// The GET_DATA exchanges whose mask is exactly `att`.
fn get_data_of(s: &Session<Recorder, FixedClock>, att: Attribute) -> Vec<Exchange> {
    exchanges_of(s, 0x0C)
        .into_iter()
        .filter(|e| frame::get_data_mask(&e.request).map(|a| a.bits()) == Some(att.bits()))
        .collect()
}

/// A native session over the demo sim with `faults`.
fn native_session_with(faults: FaultPlan) -> Session<Recorder, FixedClock> {
    let clock = SimClock::new(0);
    let recorder = Recorder {
        sim: demo_sim(&clock).with_faults(faults),
        log: Vec::new(),
    };
    Session::with_clock(recorder, FixedClock)
}

/// The desktop worker's `RecordEntryView` for entry `i` (0-based) as JSON.
fn desktop_entry(i: usize, e: &RecordEntry) -> Value {
    json!({
        "index": i + 1,
        "name": e.name,
        "interval_s": e.interval_s,
        "points": e.points,
        "duration_s": e.duration_s,
        "charge_ah": e.charge_ah(),
        "energy_wh": e.energy_wh(),
        "offset": e.offset,
    })
}

/// The desktop worker's `OfflineRecordView` as JSON.
fn desktop_record(index: usize, e: &RecordEntry, samples: &[RecordSample]) -> Value {
    json!({
        "index": index,
        "name": e.name,
        "interval_s": e.interval_s,
        "samples": samples
            .iter()
            .enumerate()
            .map(|(k, s)| json!({
                "t_s": e.sample_time_s(k) as f64,
                "vbus_v": s.vbus_v(),
                "ibus_a": s.ibus_a(),
                "charge_ah": s.charge_ah(),
                "energy_wh": s.energy_wh(),
            }))
            .collect::<Vec<_>>(),
    })
}

/// A connected native session that listed the records; returns the entries
/// and the RecordList reply it read.
fn listed() -> (Session<Recorder, FixedClock>, Vec<RecordEntry>, Vec<u8>) {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    let entries = s.list_records().unwrap();
    let reply = get_data_of(&s, Attribute::RECORD_LIST)
        .pop()
        .expect("the session sent a RecordList request")
        .reply();
    (s, entries, reply)
}

/// A RecordBook that decoded the demo meter's list.
fn book_with_demo_list() -> (RecordBook, Vec<RecordEntry>) {
    let (_, entries, reply) = listed();
    let mut book = RecordBook::new();
    book.decode_list(&reply).unwrap();
    (book, entries)
}

// ---- AC-W8SHX8.1: settings ----

#[test]
fn settings_request_is_the_native_settings_read() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    s.read_settings().unwrap();
    let sent = get_data_of(&s, Attribute::SETTINGS);
    assert_eq!(sent.len(), 1, "one SETTINGS read");
    let e = &sent[0];
    assert_eq!(settings_request(id_of(e)), e.request);
    assert_eq!(
        settings_request(5),
        get_data_request(5, Attribute::SETTINGS.bits())
    );
    assert_eq!(settings_request(5), vec![0x0C, 0x05, 0x10, 0x00]);
}

#[test]
fn decode_settings_gives_the_desktop_panel_rows() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    let native = s.read_meter_settings().unwrap();
    let reply = get_data_of(&s, Attribute::SETTINGS).pop().unwrap().reply();
    let view = decode_settings(&reply).unwrap();
    assert_eq!(json(&view.fields), json(&native.rows()));
    assert!(view.warnings.is_empty(), "a read has no warnings");
    let fields = json(&view);
    let rows = fields["fields"].as_array().expect("fields is a list");
    assert!(!rows.is_empty());
    for row in rows {
        for key in ["key", "label", "value", "tag"] {
            assert!(row[key].is_string(), "{key} in {row}");
        }
    }
    assert_eq!(fields["warnings"], json!([]));
}

#[test]
fn decode_settings_of_a_malformed_reply_is_a_malformed_error() {
    for bad in [
        vec![],
        vec![0x41, 0x01],
        vector("adc_52.hex"), // a PUT_DATA without a SETTINGS section
    ] {
        let err = decode_settings(&bad).unwrap_err();
        assert_eq!(err.kind, "malformed", "{bad:02x?}: {}", err.message);
        assert!(!err.message.is_empty());
    }
    let err = decode_settings(&[0x06, 0x01, 0x00, 0x00]).unwrap_err();
    assert_eq!(err.kind, "rejected", "{}", err.message);
}

#[test]
fn decode_settings_with_a_bad_crc_is_a_malformed_error() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    s.read_settings().unwrap();
    let mut reply = get_data_of(&s, Attribute::SETTINGS).pop().unwrap().reply();
    // Flip a byte of CRC-covered block B (past both headers).
    let at = 8 + 0x70;
    reply[at] ^= 0xFF;
    let err = decode_settings(&reply).unwrap_err();
    assert_eq!(err.kind, "malformed", "{}", err.message);
}

// ---- AC-W8SHX8.2: record list ----

#[test]
fn list_request_is_the_native_record_list_read() {
    let (s, _, _) = listed();
    let sent = get_data_of(&s, Attribute::RECORD_LIST);
    assert_eq!(sent.len(), 1);
    let book = RecordBook::new();
    assert_eq!(book.list_request(id_of(&sent[0])), sent[0].request);
    assert_eq!(book.list_request(9), vec![0x0C, 0x09, 0x00, 0x04]);
}

#[test]
fn decode_list_gives_the_desktop_records_view() {
    let (_, entries, reply) = listed();
    assert_eq!(entries.len(), 3, "the demo meter has three records");
    let mut book = RecordBook::new();
    let view = book.decode_list(&reply).unwrap();
    let want: Vec<Value> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| desktop_entry(i, e))
        .collect();
    assert_eq!(json(&view)["records"], Value::Array(want));
    assert_eq!(json(&view)["records"][0]["name"], "PHONE_5V");
}

#[test]
fn an_empty_list_is_not_an_error() {
    let (mut s, entries, _) = listed();
    let after = s.delete_records(&entries, &[0, 1, 2]).unwrap();
    assert!(after.is_empty());
    let reply = get_data_of(&s, Attribute::RECORD_LIST)
        .pop()
        .unwrap()
        .reply();
    let mut book = RecordBook::new();
    let view = book.decode_list(&reply).unwrap();
    assert_eq!(json(&view)["records"], json!([]));
}

#[test]
fn decode_list_of_a_reply_without_a_record_list_is_an_error() {
    let mut book = RecordBook::new();
    for bad in [vec![], vector("adc_52.hex")] {
        let err = book.decode_list(&bad).unwrap_err();
        assert_eq!(err.kind, "malformed", "{bad:02x?}: {}", err.message);
    }
}

// ---- AC-W8SHX8.3: chunk plan and window guard ----

#[test]
fn chunks_are_the_native_downloads_reads() {
    let (mut s, entries, reply) = listed();
    let mut book = RecordBook::new();
    book.decode_list(&reply).unwrap();
    for (i, entry) in entries.iter().enumerate() {
        let before = exchanges_of(&s, 0x44).len();
        s.download_record(entry).unwrap();
        let reads: Vec<Exchange> = exchanges_of(&s, 0x44).split_off(before);
        let chunks = book.chunks((i + 1) as _).unwrap();
        assert_eq!(chunks.len(), reads.len(), "record {}", i + 1);
        assert!(chunks.len() > 1, "each demo record takes several reads");
        let (addr, len) = entry.read_window().unwrap();
        let mut at = addr;
        for (c, e) in chunks.iter().zip(&reads) {
            assert!(c.len <= MEMORY_READ_MAX_LEN, "{} bytes", c.len);
            assert_eq!(c.addr, at, "chunks tile the window");
            at += c.len;
            assert_eq!(
                book.chunk_request(id_of(e), c.addr, c.len).unwrap(),
                e.request
            );
        }
        assert_eq!(at, addr + len, "chunks cover the whole window");
    }
}

#[test]
fn an_unlisted_record_is_refused_with_the_desktop_wording() {
    let book = RecordBook::new();
    let err = book.chunks(1).unwrap_err();
    assert_eq!(err.kind, "usage");
    assert_eq!(err.message, "That record is not on the meter.");

    let (book, entries) = book_with_demo_list();
    for index in [0, entries.len() + 1, 99] {
        let err = book.chunks(index as _).unwrap_err();
        assert_eq!(err.kind, "usage", "index {index}");
        assert_eq!(err.message, "That record is not on the meter.");
    }
}

#[test]
fn a_record_with_no_samples_is_refused_with_the_desktop_wording() {
    let mut s = native_session_with(FaultPlan::new().record_count(5));
    s.connect().unwrap();
    let entries = s.list_records().unwrap();
    assert_eq!(entries.len(), 5);
    let empty = entries
        .iter()
        .position(|e| e.points == 0)
        .expect("a padded record");
    let reply = get_data_of(&s, Attribute::RECORD_LIST)
        .pop()
        .unwrap()
        .reply();
    let mut book = RecordBook::new();
    book.decode_list(&reply).unwrap();
    let err = book.chunks((empty + 1) as _).unwrap_err();
    assert_eq!(err.kind, "usage");
    assert_eq!(err.message, "That record has no samples.");
}

#[test]
fn chunk_request_refuses_anything_outside_the_listed_windows() {
    let (_, entries, reply) = listed();
    let (addr, len) = entries[0].read_window().unwrap();

    // Before any list nothing is readable.
    let fresh = RecordBook::new();
    assert_eq!(
        fresh.chunk_request(1, addr, 256).unwrap_err().kind,
        "forbidden"
    );

    let mut book = RecordBook::new();
    book.decode_list(&reply).unwrap();
    assert!(book.chunk_request(1, addr, 256).is_ok());
    assert!(book.chunk_request(1, addr + len - 16, 16).is_ok());
    for (a, n, what) in [
        (addr, 257, "longer than 256 bytes"),
        (addr, len, "the whole window in one read"),
        (addr + len - 16, 32, "past the window's end"),
        (addr - 16, 32, "before the window's start"),
        (addr + 0x0010_0000, 256, "outside every window"),
        (UID.0, UID.1, "a device-info block"),
        (0x0800_0000, 256, "application flash"),
        (addr, 0, "an empty read"),
    ] {
        let err = book.chunk_request(1, a, n).unwrap_err();
        assert_eq!(err.kind, "forbidden", "{what}: {}", err.message);
        assert!(!err.message.is_empty());
    }
}

#[test]
fn a_new_list_replaces_the_readable_windows() {
    let (mut s, entries, reply) = listed();
    let (first, _) = entries[0].read_window().unwrap();
    let mut book = RecordBook::new();
    book.decode_list(&reply).unwrap();
    assert!(book.chunk_request(1, first, 256).is_ok());

    // Only the first record is deleted: its window is no longer readable.
    s.delete_records(&entries, &[0]).unwrap();
    let after = get_data_of(&s, Attribute::RECORD_LIST)
        .pop()
        .unwrap()
        .reply();
    book.decode_list(&after).unwrap();
    assert_eq!(
        book.chunk_request(1, first, 256).unwrap_err().kind,
        "forbidden"
    );
    let (second, _) = entries[1].read_window().unwrap();
    assert!(book.chunk_request(1, second, 256).is_ok());
    // The list is renumbered from 1.
    assert_eq!(book.chunks(1).unwrap()[0].addr, second);
}

#[test]
fn a_failed_list_clears_the_readable_windows() {
    let (_, entries, reply) = listed();
    let (addr, _) = entries[0].read_window().unwrap();
    let mut book = RecordBook::new();
    book.decode_list(&reply).unwrap();
    assert!(book.decode_list(&vector("adc_52.hex")).is_err());
    assert_eq!(
        book.chunk_request(1, addr, 256).unwrap_err().kind,
        "forbidden"
    );
    assert_eq!(
        book.chunks(1).unwrap_err().message,
        "That record is not on the meter."
    );
}

// ---- AC-W8SHX8.4: downloaded record ----

#[test]
fn decode_record_equals_the_desktop_download() {
    let (mut s, entries, reply) = listed();
    let mut book = RecordBook::new();
    book.decode_list(&reply).unwrap();
    for (i, entry) in entries.iter().enumerate() {
        let before = exchanges_of(&s, 0x44).len();
        let native = s.download_record(entry).unwrap();
        let reads = exchanges_of(&s, 0x44).split_off(before);
        let chunks = book.chunks((i + 1) as _).unwrap();
        let mut data = Vec::new();
        for (c, e) in chunks.iter().zip(&reads) {
            data.extend(decode_memory_read(&e.reply(), c.addr, c.len).unwrap());
        }
        let view = book.decode_record((i + 1) as _, &data).unwrap();
        assert_eq!(json(&view), desktop_record(i + 1, entry, &native));
        assert_eq!(
            json(&view)["samples"].as_array().unwrap().len(),
            usize::from(entry.points)
        );
    }
}

#[test]
fn decode_record_refuses_wrong_data_and_unlisted_records() {
    let (book, entries) = book_with_demo_list();
    let want = usize::from(entries[0].points) * 16;
    for len in [0, want - 16, want + 16, want - 1] {
        let err = book.decode_record(1, &vec![0u8; len]).unwrap_err();
        assert_eq!(err.kind, "malformed", "{len} bytes: {}", err.message);
    }
    let err = book.decode_record(9, &[]).unwrap_err();
    assert_eq!(err.kind, "usage");
    assert_eq!(err.message, "That record is not on the meter.");
}

// ---- AC-W8SHX8.5: the plain memory read stays on the four blocks ----

#[test]
fn memory_read_request_still_refuses_record_windows() {
    let (_, entries, _) = listed();
    for e in &entries {
        let (addr, _) = e.read_window().unwrap();
        let err = memory_read_request(1, addr, 256).unwrap_err();
        assert_eq!(err.kind, "forbidden", "{addr:#x}");
    }
    for (addr, len) in [UID, OTP, HARDWARE_INFO, FIRMWARE_INFO] {
        assert!(memory_read_request(1, addr, len).is_ok(), "{addr:#x}");
    }
    for (addr, len) in [(UID.0, UID.1 + 1), (0x9810_0000, 16), (0, 64)] {
        assert_eq!(
            memory_read_request(1, addr, len).unwrap_err().kind,
            "forbidden",
            "{addr:#x}+{len}"
        );
    }
}
