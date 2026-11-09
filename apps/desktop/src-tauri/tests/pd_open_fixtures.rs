//! FR EAKFEC: what the desktop app shows for the committed PD fixtures the
//! web edition opens too (`tests/data/pd-capture.pdcap`, `pd-vendor.sqlite`).
//!
//! The expected view (`tests/data/pd-open-expected.json`: every row, status
//! sample, count, record details and the cable identity) is what the desktop
//! open and import answer; the web test (`src/lib/web/pdFiles.web.test.ts`)
//! asserts the same file against the browser's open and import over the
//! real wasm module and sql.js (AC-EAKFEC.2, AC-EAKFEC.3). Every file is
//! synthetic: the vendor log is written in the vendor layout from the
//! research notes `[static]`, no vendor file is used.
//!
//! `PD_FIXTURES_WRITE=1 cargo test -p open-powerz-desktop --test
//! pd_open_fixtures` writes the three files again.
//!
//! - `pd-capture.pdcap`: a negotiation over two polls with a reconnect, a
//!   cable Discover Identity ACK on SOP', then one damaged line.
//! - `pd-vendor.sqlite`: events and messages, a row whose Raw is text, a
//!   blob that is no whole record, a NULL Raw, one oversized blob, a NULL
//!   VBUS, and the chart table.

use std::io::Write;
use std::path::{Path, PathBuf};

use open_powerz_desktop_lib::pd_capture::{CableSeen, PdCapture};
use open_powerz_desktop_lib::{import_vendor_pd_file, open_pdcap_file};
use powerz_protocol::decode::pd_packet::{CaptureClock, PdChunk, PdStatus};
use powerz_protocol::pdcap::{self, FORMAT_VERSION, Header};
use powerz_protocol::session::CapturedRecord;
use rusqlite::{Connection, params};
use serde_json::{Value, json};

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

fn status_bytes(s: &PdStatus) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(s.time_ms.to_le_bytes());
    b.extend(s.vbus_mv.to_le_bytes());
    b.extend(s.ibus_ma.to_le_bytes());
    b.extend(s.cc1_mv.to_le_bytes());
    b.extend(s.cc2_mv.to_le_bytes());
    b
}

/// A PD message record (b0 bit 7) at meter time `ts` ms.
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

const SRC_CAPS: [u8; 8] = [0x2C, 0x91, 0x81, 0x09, 0x2C, 0xD1, 0x02, 0x00];

/// A 240 W cable's Discover Identity ACK on SOP' (header, objects). [device]
fn cable_ack() -> (u16, Vec<u8>) {
    let objects: [u32; 5] = [
        0xFF00_A841,
        0x1860_050D,
        0x0000_404D,
        0x8254_0000,
        0x000A_2640,
    ];
    (
        0x518F,
        objects.iter().flat_map(|o| o.to_le_bytes()).collect(),
    )
}

fn status(time_ms: u32, vbus_mv: u16, ibus_ma: i16) -> PdStatus {
    PdStatus {
        time_ms,
        vbus_mv,
        ibus_ma,
        cc1_mv: 1_620,
        cc2_mv: 0,
    }
}

/// The capture the pdcap fixture saves, fed as the worker feeds it.
fn live_capture() -> PdCapture {
    let (ack_header, ack) = cable_ack();
    let polls: Vec<(Option<i64>, PdStatus, Vec<Vec<u8>>)> = vec![
        (
            Some(-7_000),
            status(10_000, 5_010, 120),
            vec![
                event(true, 10_000, 0x11),
                msg(false, 10_040, 0, 0x21A1, &SRC_CAPS),
            ],
        ),
        (
            None,
            status(10_300, 5_020, 140),
            vec![
                msg(true, 10_042, 0, 0x1082, &[0x2C, 0xB1, 0x44, 0x13]),
                msg(false, 10_050, 0, 0x03A3, &[]),
                msg(false, 10_060, 1, ack_header, &ack),
            ],
        ),
        (
            Some(-6_500),
            status(10_600, 9_010, -250),
            vec![
                msg(false, 10_200, 0, 0x05A6, &[]),
                event(true, 10_250, 0x04),
            ],
        ),
    ];
    let mut pd = PdCapture::new();
    let mut clock = CaptureClock::new();
    pd.start_run();
    for (offset, s, records) in polls {
        if let Some(off) = offset {
            pd.set_session_offset(off);
        }
        let mut chunk = status_bytes(&s);
        for r in &records {
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

fn write_pdcap(path: &Path) {
    let header = Header {
        format_version: FORMAT_VERSION,
        app_version: "0.0.0-fixture".to_owned(),
        model: "KM003C".to_owned(),
        firmware_version: "2.0.7".to_owned(),
    };
    let mut w = pdcap::Writer::new(Vec::new(), &header).expect("header written");
    for e in live_capture().to_pdcap() {
        w.write(&e).expect("entry written");
    }
    let mut bytes = w.finish().expect("flushed");
    bytes.extend_from_slice(b"{\"kind\":\"rec");
    let mut f = std::fs::File::create(path).expect("pdcap fixture");
    f.write_all(&bytes).expect("pdcap written");
}

/// A `pd_table` row: Time (s), Vbus (V, maybe NULL), Ibus (A), Raw (maybe NULL).
type TableRow = (f64, Option<f64>, f64, Option<Vec<u8>>);

fn write_vendor_db(path: &Path) {
    let _ = std::fs::remove_file(path);
    let db = Connection::open(path).expect("create fixture database");
    db.execute_batch(
        "create table pd_chart(Time real, VBUS real, IBUS real, CC1 real, CC2 real);
         create table pd_table(Time real, Vbus real, Ibus real, Raw Blob);
         create table pd_table_key(key integer);",
    )
    .expect("vendor tables");
    let chart: [(f64, f64, f64, f64, f64); 4] = [
        (0.0, 5.01, 0.0, 1.62, 0.0),
        (0.1, 5.012, 0.021, 1.62, 0.0),
        (0.3, 9.004, 0.5, 1.61, 0.0),
        (1.3, 0.0, 0.0, 0.0, 0.0),
    ];
    for (t, v, i, cc1, cc2) in chart {
        db.execute(
            "insert into pd_chart(Time,VBUS,IBUS,CC1,CC2)values(?,?,?,?,?)",
            params![t, v, i, cc1, cc2],
        )
        .expect("chart row");
    }
    let (ack_header, ack) = cable_ack();
    let blobs: Vec<TableRow> = vec![
        (0.0, Some(5.01), 0.0, Some(event(true, 0, 0x11))),
        (
            0.04,
            Some(5.012),
            0.02,
            Some(msg(false, 40, 0, 0x21A1, &SRC_CAPS)),
        ),
        (
            0.042,
            Some(5.012),
            0.021,
            Some(msg(true, 42, 0, 0x1082, &[0x2C, 0xB1, 0x44, 0x13])),
        ),
        // No whole record: cut short, and a NULL.
        (0.045, Some(5.012), 0.021, Some(vec![0x8B, 0x01])),
        (0.046, Some(5.012), 0.021, None),
        (0.05, None, 0.022, Some(msg(false, 50, 0, 0x03A3, &[]))),
        (
            0.06,
            Some(5.013),
            0.022,
            Some(msg(false, 60, 1, ack_header, &ack)),
        ),
        // Oversized: skipped unread.
        (0.07, Some(5.013), 0.022, Some(vec![0x05; 5000])),
        (0.2, Some(9.004), 0.5, Some(msg(false, 200, 0, 0x05A6, &[]))),
        (1.25, Some(0.0), 0.0, Some(event(true, 1_250, 0x04))),
    ];
    for (k, (t, v, i, raw)) in blobs.into_iter().enumerate() {
        db.execute(
            "insert into pd_table(Time,Vbus,Ibus,Raw)values(?,?,?,?)",
            params![t, v, i, raw],
        )
        .expect("table row");
        db.execute("insert into pd_table_key(key)values(?)", params![k as i64])
            .expect("key row");
    }
    // Raw as text is no record bytes.
    db.execute(
        "insert into pd_table(Time,Vbus,Ibus,Raw)values(1.3,0,0,'not bytes')",
        [],
    )
    .expect("text row");
    db.execute_batch("VACUUM").expect("compact");
    db.close().expect("database closed");
}

/// What the page sees of an opened capture, the path left out (the browser
/// names the file only): the view, the details of every row and the cable.
fn seen((opened, pd): (open_powerz_desktop_lib::PdOpened, PdCapture)) -> Value {
    let details: Vec<Value> = opened
        .rows
        .iter()
        .map(|r| serde_json::to_value(pd.details(r.index)).expect("details"))
        .collect();
    let mut view = serde_json::to_value(&opened).expect("view");
    if let Some(o) = view.as_object_mut() {
        o.remove("path");
    }
    json!({
        "opened": view,
        "details": details,
        "cable": serde_json::to_value(pd.latest_cable_view()).expect("cable"),
    })
}

fn desktop_view() -> Value {
    let pdcap = open_pdcap_file(&data("pd-capture.pdcap")).expect("the pdcap fixture opens");
    let vendor =
        import_vendor_pd_file(&data("pd-vendor.sqlite")).expect("the vendor fixture imports");
    json!({ "pdcap": seen(pdcap), "vendor": seen(vendor) })
}

#[test]
fn the_desktop_opens_the_pd_fixtures_as_the_expected_view_says() {
    if std::env::var_os("PD_FIXTURES_WRITE").is_some() {
        write_pdcap(&data("pd-capture.pdcap"));
        write_vendor_db(&data("pd-vendor.sqlite"));
        let text = serde_json::to_string_pretty(&desktop_view()).expect("json");
        std::fs::write(data("pd-open-expected.json"), format!("{text}\n")).expect("expected");
    }
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(data("pd-open-expected.json")).expect("expected view"),
    )
    .expect("expected view parses");
    assert_eq!(desktop_view(), expected);
}

#[test]
fn the_fixtures_hold_what_the_web_test_relies_on() {
    let view = desktop_view();
    let pdcap = &view["pdcap"]["opened"];
    assert_eq!(pdcap["rows"].as_array().map(Vec::len), Some(7));
    assert_eq!(pdcap["status"].as_array().map(Vec::len), Some(3));
    assert!(pdcap["damaged_line"].as_u64().is_some(), "{pdcap}");
    let vendor = &view["vendor"]["opened"];
    assert_eq!(vendor["rows"].as_array().map(Vec::len), Some(7));
    assert_eq!(vendor["status"].as_array().map(Vec::len), Some(4));
    assert_eq!(vendor["oversized"], 1);
    assert_eq!(vendor["skipped"], 3);
    assert_eq!(vendor["truncated"], false);
    assert_eq!(view["pdcap"]["cable"]["vid"], 0x050D);
    assert_eq!(view["vendor"]["cable"]["vid"], 0x050D);
}
