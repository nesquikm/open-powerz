//! The pdcap capture file format (FR TGE83E, AC .1, .2, .3, .5).
//!
//! API pinned here (module `powerz_protocol::pdcap`, pure: no file I/O of
//! its own, it works on any `Write` / `BufRead`):
//!
//! - `FORMAT_VERSION: u32` (1).
//! - `Header { format_version, app_version, model, firmware_version }`; there
//!   is no serial number field.
//! - `Entry::Status { session_ms, status: PdStatus }` and
//!   `Entry::Record { session_ms, elapsed_ms, record: PdRecord }`.
//! - `Writer::new(out, &header)` writes the header line; `write(&entry)`
//!   writes one line; `finish()` flushes and hands `out` back.
//! - `Reader::new(input)` reads the header line (an `Error` when it is
//!   missing or bad); `header()`; it iterates `Result<Entry, Error>`, one per
//!   line, streaming.
//! - `load(input) -> Loaded { header: Option<Header>, entries, error }`:
//!   every valid line before the first damaged one, and the damage, if any.
//! - `Error::line()` is the 1-based line where reading stopped (the header
//!   is line 1); its `Display` names that line.
//!
//! File shape (JSON Lines, one compact object per line):
//!
//! ```text
//! {"format":"pdcap","format_version":1,"app_version":"…","model":"KM003C","firmware_version":"…"}
//! {"kind":"status","session_ms":1000,"raw":"<the 12 status bytes, lowercase hex>"}
//! {"kind":"record","session_ms":1000,"elapsed_ms":0,"raw":"<the record bytes, lowercase hex>"}
//! ```
//!
//! Every byte vector here is synthetic.

use std::io::Cursor;
use std::path::PathBuf;

use powerz_protocol::decode::pd_packet::{PdChunk, PdRecord, PdStatus};
use powerz_protocol::pdcap::{self, Entry, FORMAT_VERSION, Header, Reader, Writer};
use proptest::prelude::*;
use serde_json::Value;

// ---------------------------------------------------------------- vectors

fn status_bytes(s: &PdStatus) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(s.time_ms.to_le_bytes());
    b.extend(s.vbus_mv.to_le_bytes());
    b.extend(s.ibus_ma.to_le_bytes());
    b.extend(s.cc1_mv.to_le_bytes());
    b.extend(s.cc2_mv.to_le_bytes());
    b
}

fn status(time_ms: u32) -> PdStatus {
    PdStatus {
        time_ms,
        vbus_mv: 5_012,
        ibus_ma: -37,
        cc1_mv: 1_620,
        cc2_mv: 4,
    }
}

/// A PD message record (b0 bit 7) with `body` after the PD header.
fn msg(meter: bool, ts: u32, header: u16, body: &[u8]) -> Vec<u8> {
    let len = 8 + body.len();
    let mut r = vec![0x80 | if meter { 0x40 } else { 0 } | (len as u8 - 1)];
    r.extend(ts.to_le_bytes());
    r.push(0x00);
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

/// The records of `raws` as the capture core parses them.
fn records(raws: &[Vec<u8>]) -> Vec<PdRecord> {
    let mut chunk = status_bytes(&status(0));
    for r in raws {
        chunk.extend(r);
    }
    let parsed = PdChunk::parse(&chunk).expect("synthetic chunk parses");
    assert_eq!(parsed.skipped, 0, "synthetic records are well formed");
    parsed.records
}

fn header() -> Header {
    Header {
        format_version: FORMAT_VERSION,
        app_version: "1.2.3".to_owned(),
        model: "KM003C".to_owned(),
        firmware_version: "2.0.7".to_owned(),
    }
}

/// A small capture: attach, Source_Capabilities, Request, hard reset, with
/// a status block before each poll's records.
fn small_capture() -> Vec<Entry> {
    let recs = records(&[
        event(true, 10_000, 0x11),
        msg(false, 10_040, 0x21A1, &SRC_CAPS),
        msg(true, 10_042, 0x1082, &[0x2C, 0xB1, 0x44, 0x13]),
        event(true, 10_250, 0x04),
    ]);
    let mut out = Vec::new();
    let mut recs = recs.into_iter();
    out.push(Entry::Status {
        session_ms: 2_000,
        status: status(10_000),
    });
    for (session_ms, elapsed_ms) in [(2_000, 0), (2_040, 40)] {
        out.push(Entry::Record {
            session_ms,
            elapsed_ms,
            record: recs.next().expect("record"),
        });
    }
    out.push(Entry::Status {
        session_ms: 2_300,
        status: status(10_300),
    });
    for (session_ms, elapsed_ms) in [(2_042, 42), (2_250, 250)] {
        out.push(Entry::Record {
            session_ms,
            elapsed_ms,
            record: recs.next().expect("record"),
        });
    }
    out
}

fn write_file(header: &Header, entries: &[Entry]) -> Vec<u8> {
    let mut w = Writer::new(Vec::new(), header).expect("header written");
    for e in entries {
        w.write(e).expect("entry written");
    }
    w.finish().expect("flushed")
}

fn json_lines(bytes: &[u8]) -> Vec<Value> {
    let text = std::str::from_utf8(bytes).expect("pdcap is UTF-8");
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l:?}: {e}")))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ------------------------------------------------- AC-TGE83E.1: the format

#[test]
fn the_first_line_is_a_header_with_the_format_app_model_and_firmware_versions() {
    let bytes = write_file(&header(), &small_capture());
    let lines = json_lines(&bytes);
    let h = &lines[0];
    assert_eq!(h["format"], "pdcap");
    assert_eq!(h["format_version"], FORMAT_VERSION);
    assert_eq!(FORMAT_VERSION, 1);
    assert_eq!(h["app_version"], "1.2.3");
    assert_eq!(h["model"], "KM003C");
    assert_eq!(h["firmware_version"], "2.0.7");
}

#[test]
fn every_status_sample_and_record_is_one_line_with_its_session_time_and_raw_hex() {
    let entries = small_capture();
    let bytes = write_file(&header(), &entries);
    let text = std::str::from_utf8(&bytes).expect("UTF-8");
    assert!(text.ends_with('\n'), "every line ends with a newline");
    let lines = json_lines(&bytes);
    assert_eq!(
        lines.len(),
        1 + entries.len(),
        "header + one line per entry"
    );
    for (line, entry) in lines[1..].iter().zip(&entries) {
        match entry {
            Entry::Status { session_ms, status } => {
                assert_eq!(line["kind"], "status", "{line}");
                assert_eq!(line["session_ms"], *session_ms, "{line}");
                assert_eq!(line["raw"], hex(&status_bytes(status)), "{line}");
            }
            Entry::Record {
                session_ms,
                elapsed_ms,
                record,
            } => {
                assert_eq!(line["kind"], "record", "{line}");
                assert_eq!(line["session_ms"], *session_ms, "{line}");
                assert_eq!(line["elapsed_ms"], *elapsed_ms, "{line}");
                assert_eq!(line["raw"], hex(&record.raw), "{line}");
            }
        }
    }
}

#[test]
fn lines_are_compact_objects_in_lowercase_hex() {
    let bytes = write_file(&header(), &small_capture());
    let text = std::str::from_utf8(&bytes).expect("UTF-8");
    for line in text.lines() {
        assert!(line.starts_with('{') && line.ends_with('}'), "{line:?}");
        let v: Value = serde_json::from_str(line).expect("JSON");
        if let Some(raw) = v["raw"].as_str() {
            assert!(
                raw.chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
                "lowercase hex: {raw}"
            );
        }
    }
}

#[test]
fn no_line_carries_a_serial_number_or_uid_field() {
    let bytes = write_file(&header(), &small_capture());
    for line in json_lines(&bytes) {
        let obj = line.as_object().expect("every line is an object");
        for key in obj.keys() {
            let k = key.to_lowercase();
            assert!(
                !k.contains("serial") && !k.contains("uid"),
                "key {key:?} in {line}"
            );
        }
    }
}

// ------------------------------------------- AC-TGE83E.2: lossless reload

#[test]
fn a_saved_capture_loads_back_exactly() {
    let entries = small_capture();
    let bytes = write_file(&header(), &entries);
    let loaded = pdcap::load(Cursor::new(&bytes));
    assert_eq!(loaded.header, Some(header()));
    assert_eq!(loaded.entries, entries);
    assert!(loaded.error.is_none(), "{:?}", loaded.error);
}

#[test]
fn the_reader_streams_the_header_and_then_one_entry_per_line() {
    let entries = small_capture();
    let bytes = write_file(&header(), &entries);
    let reader = Reader::new(Cursor::new(&bytes)).expect("header reads");
    assert_eq!(reader.header(), &header());
    let read: Vec<Entry> = reader.map(|e| e.expect("every line reads")).collect();
    assert_eq!(read, entries);
}

#[test]
fn an_empty_capture_round_trips_to_just_its_header() {
    let bytes = write_file(&header(), &[]);
    assert_eq!(json_lines(&bytes).len(), 1);
    let loaded = pdcap::load(Cursor::new(&bytes));
    assert_eq!(loaded.header, Some(header()));
    assert!(loaded.entries.is_empty());
    assert!(loaded.error.is_none());
}

/// One synthetic record: a PD message with `objects` data objects, a big
/// message, or an event.
fn record_raw() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        (
            any::<bool>(),
            any::<u32>(),
            0u8..3,
            any::<u16>(),
            proptest::collection::vec(any::<u8>(), 0..=28)
        )
            .prop_map(|(meter, ts, sop, header, body)| {
                let mut r = msg(meter, ts, header, &body);
                r[5] = sop;
                r
            }),
        (any::<bool>(), any::<u32>(), any::<u8>())
            .prop_map(|(meter, ts, code)| event(meter, ts, code)),
        (
            any::<u32>(),
            any::<u16>(),
            proptest::collection::vec(any::<u8>(), 0..=40)
        )
            .prop_map(|(ts, header, body)| {
                // Big message: base 10 bytes (sop + extra length + header),
                // then `body.len()` extra bytes.
                let mut r = vec![0x20 | 9];
                r.extend(ts.to_le_bytes());
                r.push(0);
                r.extend((body.len() as u16).to_le_bytes());
                r.extend(header.to_le_bytes());
                r.extend(&body);
                r
            }),
    ]
}

fn status_strategy() -> impl Strategy<Value = PdStatus> {
    (
        any::<u32>(),
        any::<u16>(),
        any::<i16>(),
        any::<u16>(),
        any::<u16>(),
    )
        .prop_map(|(time_ms, vbus_mv, ibus_ma, cc1_mv, cc2_mv)| PdStatus {
            time_ms,
            vbus_mv,
            ibus_ma,
            cc1_mv,
            cc2_mv,
        })
}

fn entry_strategy() -> impl Strategy<Value = Entry> {
    prop_oneof![
        (any::<u64>(), status_strategy())
            .prop_map(|(session_ms, status)| Entry::Status { session_ms, status }),
        (any::<u64>(), any::<u64>(), record_raw()).prop_map(|(session_ms, elapsed_ms, raw)| {
            let record = records(&[raw]).remove(0);
            Entry::Record {
                session_ms,
                elapsed_ms,
                record,
            }
        }),
    ]
}

fn header_strategy() -> impl Strategy<Value = Header> {
    (".{0,12}", ".{0,12}", ".{0,12}").prop_map(|(app_version, model, firmware_version)| Header {
        format_version: FORMAT_VERSION,
        app_version,
        model,
        firmware_version,
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn any_capture_round_trips_through_a_pdcap_file(
        header in header_strategy(),
        entries in proptest::collection::vec(entry_strategy(), 0..40),
    ) {
        let bytes = write_file(&header, &entries);
        let loaded = pdcap::load(Cursor::new(&bytes));
        prop_assert!(loaded.error.is_none(), "{:?}", loaded.error);
        prop_assert_eq!(loaded.header, Some(header));
        prop_assert_eq!(loaded.entries, entries);
    }
}

// ------------------------------------ AC-TGE83E.3: truncated and corrupt files

/// Byte offset where each line's content ends (before its newline).
fn line_ends(bytes: &[u8]) -> Vec<usize> {
    bytes
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'\n')
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn a_file_cut_at_any_byte_loads_every_whole_line_and_names_the_cut_line() {
    let header = header();
    let entries = small_capture();
    let bytes = write_file(&header, &entries);
    let ends = line_ends(&bytes);
    assert_eq!(ends.len(), 1 + entries.len());

    for cut in 0..=bytes.len() {
        let part = &bytes[..cut];
        let loaded = pdcap::load(Cursor::new(part));
        // Whole lines: their content is all there (the newline may be cut).
        let whole = ends.iter().filter(|&&end| end <= cut).count();
        // A line was cut in its middle when bytes past the last whole line
        // remain (beyond its newline).
        let next_start = if whole == 0 { 0 } else { ends[whole - 1] + 1 };
        let partial = cut > next_start;

        if whole == 0 {
            assert_eq!(loaded.header, None, "cut {cut}");
        } else {
            assert_eq!(loaded.header.as_ref(), Some(&header), "cut {cut}");
        }
        let expected = &entries[..whole.saturating_sub(1)];
        assert_eq!(
            loaded.entries, expected,
            "cut {cut}: entries before the damage"
        );

        match (&loaded.error, partial || cut == 0) {
            (Some(e), true) => assert_eq!(e.line(), whole + 1, "cut {cut}: {e}"),
            (None, false) => {}
            (got, want) => panic!("cut {cut}: error {got:?}, expected one: {want}"),
        }
    }
}

#[test]
fn a_corrupt_line_stops_the_load_there_and_names_its_line_number() {
    let header = header();
    let entries = small_capture();
    let bytes = write_file(&header, &entries);
    let text = std::str::from_utf8(&bytes).expect("UTF-8");
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    // Line 4 (the third entry) is garbled in its middle.
    let garbled = lines[3].replacen("\"raw\"", "\"r@w", 1);
    lines[3] = garbled;
    let file = lines.join("\n") + "\n";

    let loaded = pdcap::load(Cursor::new(file.as_bytes()));
    assert_eq!(loaded.header, Some(header));
    assert_eq!(
        loaded.entries,
        entries[..2].to_vec(),
        "the lines before the damage"
    );
    let err = loaded.error.expect("the damage is reported");
    assert_eq!(err.line(), 4);
    assert!(
        err.to_string().contains('4'),
        "the message names the line: {err}"
    );
}

#[test]
fn bad_hex_or_bytes_that_are_no_record_are_damage_too() {
    let header = header();
    let entries = small_capture();
    let good = write_file(&header, &entries);
    let text = std::str::from_utf8(&good).expect("UTF-8");
    let lines: Vec<&str> = text.lines().collect();

    for (what, bad) in [
        (
            "odd hex",
            r#"{"kind":"record","session_ms":1,"elapsed_ms":1,"raw":"057"}"#,
        ),
        (
            "not hex",
            r#"{"kind":"record","session_ms":1,"elapsed_ms":1,"raw":"zz"}"#,
        ),
        (
            "a record shorter than its length byte",
            r#"{"kind":"record","session_ms":1,"elapsed_ms":1,"raw":"8f0100"}"#,
        ),
        (
            "a short status block",
            r#"{"kind":"status","session_ms":1,"raw":"0102"}"#,
        ),
        (
            "an unknown kind",
            r#"{"kind":"bogus","session_ms":1,"raw":"00"}"#,
        ),
        (
            "no session time",
            r#"{"kind":"status","raw":"000000000000000000000000"}"#,
        ),
    ] {
        let file = format!("{}\n{}\n{}\n{}\n", lines[0], lines[1], bad, lines[2]);
        let loaded = pdcap::load(Cursor::new(file.as_bytes()));
        assert_eq!(loaded.entries, entries[..1].to_vec(), "{what}");
        let err = loaded.error.unwrap_or_else(|| panic!("{what}: no error"));
        assert_eq!(err.line(), 3, "{what}: {err}");
    }
}

#[test]
fn a_file_without_a_pdcap_header_is_refused_at_line_one() {
    for bad in [
        "",
        "not json\n",
        "{\"kind\":\"status\",\"session_ms\":1,\"raw\":\"000000000000000000000000\"}\n",
        "{\"format\":\"csv\",\"format_version\":1,\"app_version\":\"1\",\"model\":\"m\",\"firmware_version\":\"f\"}\n",
    ] {
        let loaded = pdcap::load(Cursor::new(bad.as_bytes()));
        assert_eq!(loaded.header, None, "{bad:?}");
        assert!(loaded.entries.is_empty(), "{bad:?}");
        assert_eq!(loaded.error.map(|e| e.line()), Some(1), "{bad:?}");
        assert!(Reader::new(Cursor::new(bad.as_bytes())).is_err(), "{bad:?}");
    }
}

#[test]
fn a_newer_format_version_is_refused_at_line_one() {
    let newer = Header {
        format_version: FORMAT_VERSION + 1,
        ..header()
    };
    let bytes = write_file(&newer, &small_capture());
    let loaded = pdcap::load(Cursor::new(&bytes));
    assert!(loaded.entries.is_empty());
    assert_eq!(loaded.error.map(|e| e.line()), Some(1));
}

#[test]
fn the_streaming_reader_reports_the_damaged_line_and_then_stops() {
    let entries = small_capture();
    let bytes = write_file(&header(), &entries);
    let cut = &bytes[..bytes.len() - 5];
    let reader = Reader::new(Cursor::new(cut)).expect("header reads");
    let mut items: Vec<Result<Entry, pdcap::Error>> = reader.collect();
    let last = items.pop().expect("items");
    let ok: Vec<Entry> = items
        .into_iter()
        .map(|r| r.expect("ok before damage"))
        .collect();
    assert_eq!(ok, entries[..entries.len() - 1].to_vec());
    let err = last.expect_err("the cut line is an error");
    assert_eq!(err.line(), 1 + entries.len());
}

#[test]
fn a_damage_message_names_only_the_line_of_the_file() {
    let entries = small_capture();
    let bytes = write_file(&header(), &entries);
    let cut = &bytes[..bytes.len() - 5];
    let err = Reader::new(Cursor::new(cut))
        .expect("header reads")
        .last()
        .expect("items")
        .expect_err("the cut line is an error");
    let text = err.to_string();
    let file_line = 1 + entries.len();
    assert!(
        text.starts_with(&format!("pdcap line {file_line}: ")),
        "{text}"
    );
    assert!(text.contains(" at column "), "{text}");
    assert!(!text.contains(" at line "), "{text}");
}

// ------------------------------------------ AC-TGE83E.5: the reference doc

fn repo_file(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn read_repo_file(rel: &str) -> String {
    let path = repo_file(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The contents of every fenced block whose info string is `lang`.
fn fenced(text: &str, lang: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(info) = t.strip_prefix("```") {
            match cur.take() {
                Some(block) => out.push(block),
                None if info.trim() == lang => cur = Some(String::new()),
                None => cur = Some(String::from("\u{0}skip")),
            }
            continue;
        }
        if let Some(block) = cur.as_mut() {
            block.push_str(line);
            block.push('\n');
        }
    }
    out.retain(|b| !b.starts_with('\u{0}'));
    out
}

#[test]
fn the_reference_doc_describes_every_field_of_the_format() {
    let doc = read_repo_file("docs/reference/pdcap.md");
    for field in [
        "format",
        "format_version",
        "app_version",
        "model",
        "firmware_version",
        "kind",
        "status",
        "record",
        "session_ms",
        "elapsed_ms",
        "raw",
    ] {
        assert!(
            doc.contains(&format!("`{field}`")),
            "`{field}` is documented"
        );
    }
    assert!(doc.contains("JSON Lines"), "the doc names JSON Lines");
    assert!(
        doc.to_lowercase().contains("serial"),
        "the doc says the serial number is never written"
    );
}

#[test]
fn the_reference_docs_worked_example_loads_with_the_reader() {
    let doc = read_repo_file("docs/reference/pdcap.md");
    let blocks = fenced(&doc, "jsonl");
    assert!(!blocks.is_empty(), "a ```jsonl worked example");
    let example = &blocks[0];
    let loaded = pdcap::load(Cursor::new(example.as_bytes()));
    assert!(loaded.error.is_none(), "{:?}", loaded.error);
    let header = loaded.header.expect("the example has a header");
    assert_eq!(header.format_version, FORMAT_VERSION);
    assert!(
        loaded
            .entries
            .iter()
            .any(|e| matches!(e, Entry::Status { .. })),
        "the example has a status line"
    );
    assert!(
        loaded
            .entries
            .iter()
            .any(|e| matches!(e, Entry::Record { .. })),
        "the example has a record line"
    );
}

#[test]
fn the_reference_index_and_cli_reference_link_the_format() {
    let cli = read_repo_file("docs/reference/cli.md");
    assert!(
        cli.contains("pdcap.md"),
        "cli.md links the format reference"
    );
    assert!(cli.contains("--save"), "cli.md documents pd --save");
    assert!(cli.contains("pd decode"), "cli.md documents pd decode");
}

// ------------------------------------------- a bounded read of a damaged file

/// A reader that counts the bytes taken from it.
struct Counted<R> {
    inner: R,
    taken: std::rc::Rc<std::cell::Cell<u64>>,
}

impl<R: std::io::Read> std::io::Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.taken.set(self.taken.get() + n as u64);
        Ok(n)
    }
}

#[test]
fn an_over_long_line_is_damage_at_its_line_and_is_not_read_whole() {
    use std::io::Read;

    let header = header();
    let entries = small_capture();
    let bytes = write_file(&header, &entries);
    let text = std::str::from_utf8(&bytes).expect("UTF-8");
    // The header and two entries, then a 16 MiB line with no end in sight.
    let head: String = text.lines().take(3).map(|l| format!("{l}\n")).collect();
    let huge = std::io::repeat(b'x').take(16 << 20);
    let taken = std::rc::Rc::new(std::cell::Cell::new(0));
    let input = Counted {
        inner: Cursor::new(head.into_bytes()).chain(huge),
        taken: std::rc::Rc::clone(&taken),
    };

    let loaded = pdcap::load(std::io::BufReader::new(input));
    assert_eq!(loaded.header, Some(header));
    assert_eq!(loaded.entries, entries[..2].to_vec(), "the lines before it");
    let err = loaded.error.expect("the long line is damage");
    assert_eq!(err.line(), 4);
    assert!(err.to_string().contains("too long"), "{err}");
    assert!(
        taken.get() < 1 << 20,
        "{} bytes read for a damaged line",
        taken.get()
    );
}
