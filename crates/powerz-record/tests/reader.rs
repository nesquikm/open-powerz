//! Reading a recorder CSV back (FR AXHK2S, AC .1, .2, .3 and .8).
//!
//! Every file here is written by the test or by [`CsvRecorder`]; no vendor
//! file is used.
//!
//! API pinned here: `reader::CsvReader::new(impl BufRead)` checks the header
//! (a UTF-8 BOM and CRLF are accepted) and `next_row()` yields
//! `Ok(Some(RowOutcome::Row(FileRow)))`, `Ok(Some(RowOutcome::Damaged))`,
//! `Ok(Some(RowOutcome::Blank))` or `Ok(None)` at the end of the file. An I/O failure is an `Err`; a damaged
//! row is not.

use std::io::{self, BufReader, Cursor, Read};

use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_record::reader::{CsvReader, FileRow, ReadError, RowOutcome};
use powerz_record::{CSV_HEADER, CsvRecorder, format_elapsed};

/// Column index of every field that must be there and readable.
const REQUIRED: [(usize, &str); 7] = [
    (0, "ElapsedTime"),
    (2, "VBUS"),
    (3, "IBUS"),
    (4, "DP"),
    (5, "DM"),
    (6, "CC1"),
    (7, "CC2"),
];

fn good_row(elapsed: &str) -> String {
    format!("{elapsed},1790000000.100000,5.0003,1.25,0.6,0.01,1.68,0.002,25.5,0.001,0.005,6.250375")
}

fn file(rows: &[&str]) -> Vec<u8> {
    let mut text = format!("{CSV_HEADER}\n");
    for row in rows {
        text.push_str(row);
        text.push('\n');
    }
    text.into_bytes()
}

/// Everything `reader` yields up to the end of the file: the rows, and how
/// many were damaged.
fn drain(bytes: Vec<u8>) -> (Vec<FileRow>, u64) {
    let mut reader = match CsvReader::new(Cursor::new(bytes)) {
        Ok(reader) => reader,
        Err(e) => panic!("the header was refused: {e}"),
    };
    let mut rows = Vec::new();
    let mut damaged = 0;
    while let Some(outcome) = reader.next_row().expect("no I/O error on a byte slice") {
        match outcome {
            RowOutcome::Row(row) => rows.push(row),
            RowOutcome::Damaged => damaged += 1,
            RowOutcome::Blank => {}
        }
    }
    (rows, damaged)
}

fn refusal(bytes: &[u8]) -> ReadError {
    match CsvReader::new(Cursor::new(bytes.to_vec())) {
        Ok(_) => panic!("the file was not refused"),
        Err(e) => e,
    }
}

fn assert_one_damaged(line: &str, why: &str) {
    let (rows, damaged) = drain(file(&[
        &good_row("0:00:00.000"),
        line,
        &good_row("0:00:00.900"),
    ]));
    assert_eq!(damaged, 1, "{why}: {line:?} was not counted as damaged");
    assert_eq!(rows.len(), 2, "{why}: the rows around it must stay");
}

// ---- AC-AXHK2S.1: the vendor layout ----

#[test]
fn a_row_with_the_vendor_header_reads_as_its_values() {
    let (rows, damaged) = drain(file(&[&good_row("1:02:03.456")]));
    assert_eq!(damaged, 0);
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r.elapsed_ms, 3_723_456);
    assert_eq!(r.unix, Some(1_790_000_000.1));
    assert_eq!(r.vbus_v, 5.0003);
    assert_eq!(r.ibus_a, 1.25);
    assert_eq!(r.dp_v, 0.6);
    assert_eq!(r.dm_v, 0.01);
    assert_eq!(r.cc1_v, 1.68);
    assert_eq!(r.cc2_v, 0.002);
    assert_eq!(r.temp_c, Some(25.5));
    assert_eq!(r.charge_ah, Some(0.001));
    assert_eq!(r.energy_wh, Some(0.005));
}

#[test]
fn hours_of_elapsed_time_are_not_padded_or_wrapped() {
    let (rows, _) = drain(file(&[&good_row("100:00:59.999")]));
    assert_eq!(rows[0].elapsed_ms, 100 * 3_600_000 + 59_999);
}

#[test]
fn a_byte_order_mark_and_crlf_line_ends_are_accepted() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(CSV_HEADER.as_bytes());
    bytes.extend_from_slice(b"\r\n");
    bytes.extend_from_slice(good_row("0:00:00.000").as_bytes());
    bytes.extend_from_slice(b"\r\n");
    bytes.extend_from_slice(good_row("0:00:00.100").as_bytes());
    bytes.extend_from_slice(b"\r\n");
    let (rows, damaged) = drain(bytes);
    assert_eq!(damaged, 0);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].energy_wh, Some(0.005));
}

#[test]
fn a_crlf_row_with_an_empty_last_field_is_not_damaged() {
    let row = "0:00:00.000,,5,1,0.6,0,1.68,0,,0.1,0.5,\r";
    let (rows, damaged) = drain(file(&[row]));
    assert_eq!((rows.len(), damaged), (1, 0));
}

#[test]
fn a_header_with_no_rows_reads_as_no_rows() {
    assert_eq!(drain(file(&[])).0.len(), 0);
}

#[test]
fn a_last_row_without_a_line_end_still_counts() {
    let mut bytes = file(&[&good_row("0:00:00.000")]);
    bytes.extend_from_slice(good_row("0:00:00.100").as_bytes());
    let (rows, damaged) = drain(bytes);
    assert_eq!((rows.len(), damaged), (2, 0));
}

#[test]
fn blank_lines_are_not_damaged_rows() {
    let row = good_row("0:00:00.000");
    let later = good_row("0:00:00.100");
    for (text, why) in [
        (format!("{CSV_HEADER}\n{row}\n\n{later}\n"), "in the middle"),
        (
            format!("{CSV_HEADER}\n\n{row}\n{later}\n"),
            "after the header",
        ),
        (
            format!("{CSV_HEADER}\n{row}\n{later}\n\n"),
            "two newlines at the end",
        ),
        (format!("{CSV_HEADER}\r\n{row}\r\n{later}\r\n\r\n"), "CRLF"),
    ] {
        let (rows, damaged) = drain(text.into_bytes());
        assert_eq!((rows.len(), damaged), (2, 0), "{why}");
    }
}

#[test]
fn a_line_of_spaces_is_still_damaged() {
    assert_one_damaged("   ", "spaces are no blank line");
}

#[test]
fn next_row_keeps_answering_none_at_the_end() {
    let mut reader = match CsvReader::new(Cursor::new(file(&[]))) {
        Ok(reader) => reader,
        Err(e) => panic!("the header was refused: {e}"),
    };
    assert!(reader.next_row().expect("end of file").is_none());
    assert!(reader.next_row().expect("still the end").is_none());
}

// ---- AC-AXHK2S.2: damaged rows are skipped and counted ----

#[test]
fn an_invalid_sentinel_row_is_damaged() {
    assert_one_damaged("invalid,,", "the vendor's sentinel row");
    assert_one_damaged("invalid,,,,,,,,,,,", "the sentinel with all its commas");
}

#[test]
fn a_short_row_is_damaged() {
    assert_one_damaged("0:00:00.500,1790000000.5,5,1", "short");
    assert_one_damaged("0:00:00.500", "one field");
}

#[test]
fn an_empty_required_field_is_damaged() {
    for (column, name) in REQUIRED {
        let mut fields: Vec<String> = good_row("0:00:00.500")
            .split(',')
            .map(String::from)
            .collect();
        fields[column] = String::new();
        assert_one_damaged(&fields.join(","), name);
    }
}

#[test]
fn an_unreadable_required_field_is_damaged() {
    for (column, name) in REQUIRED {
        let mut fields: Vec<String> = good_row("0:00:00.500")
            .split(',')
            .map(String::from)
            .collect();
        fields[column] = "garbled".into();
        assert_one_damaged(&fields.join(","), name);
    }
}

#[test]
fn an_elapsed_time_of_the_wrong_form_is_damaged() {
    for elapsed in ["12.5", "0:00", "x:00:00.500", "0:00:xx.500", "-1:00:00.000"] {
        assert_one_damaged(&good_row(elapsed), elapsed);
    }
}

#[test]
fn a_measured_value_out_of_range_is_damaged() {
    for (column, name) in REQUIRED.into_iter().skip(1) {
        for value in ["NaN", "inf", "-inf", "99999999", "-99999999"] {
            let mut fields: Vec<String> = good_row("0:00:00.500")
                .split(',')
                .map(String::from)
                .collect();
            fields[column] = value.into();
            assert_one_damaged(&fields.join(","), &format!("{name} = {value}"));
        }
    }
}

#[test]
fn a_time_that_goes_backwards_is_damaged_and_the_next_row_is_judged_by_the_last_good_one() {
    let (rows, damaged) = drain(file(&[
        &good_row("0:00:01.000"),
        &good_row("0:00:00.500"),
        &good_row("0:00:01.500"),
        // The same time twice does not go backwards.
        &good_row("0:00:01.500"),
    ]));
    assert_eq!(damaged, 1);
    let times: Vec<u64> = rows.iter().map(|r| r.elapsed_ms).collect();
    assert_eq!(times, [1_000, 1_500, 1_500]);
}

#[test]
fn a_row_that_is_not_text_is_damaged_and_the_next_line_still_reads() {
    let mut bytes = file(&[&good_row("0:00:00.000")]);
    bytes.extend_from_slice(b"0:00:00.100,\xFF\xFE\x80,5,1\n");
    bytes.extend_from_slice(good_row("0:00:00.200").as_bytes());
    bytes.push(b'\n');
    let (rows, damaged) = drain(bytes);
    assert_eq!((rows.len(), damaged), (2, 1));
    assert_eq!(rows[1].elapsed_ms, 200);
}

#[test]
fn a_row_past_4_kb_is_damaged_and_a_long_one_within_it_is_not() {
    // Padding in PWR keeps the row's values readable, so only its length counts.
    let padded = |len: usize| {
        let row = good_row("0:00:00.500");
        let zeros = "0".repeat(len - row.len());
        format!("{row}{zeros}")
    };
    assert_one_damaged(&padded(4_097), "just past 4 KB");
    assert_one_damaged(&padded(5_000), "past 4 KB");
    let (rows, damaged) = drain(file(&[&padded(4_000)]));
    assert_eq!((rows.len(), damaged), (1, 0));
}

#[test]
fn a_huge_line_is_one_damaged_row_and_the_next_line_still_reads() {
    let huge = "9".repeat(8 << 20);
    assert_one_damaged(&huge, "an 8 MB line");
}

#[test]
fn a_row_cut_off_at_the_end_of_the_file_is_damaged() {
    let mut bytes = file(&[&good_row("0:00:00.000")]);
    bytes.extend_from_slice(b"0:00:00.100,1790000000.1,5.00");
    let (rows, damaged) = drain(bytes);
    assert_eq!((rows.len(), damaged), (1, 1));
}

// ---- TEMP, CHARGE, ENERGY and PWR may be empty ----

#[test]
fn the_optional_fields_may_be_empty() {
    let (rows, damaged) = drain(file(&["0:00:00.000,1790000000.1,5,1,0.6,0,1.68,0,,,,"]));
    assert_eq!((rows.len(), damaged), (1, 0));
    let r = &rows[0];
    assert_eq!(r.temp_c, None);
    assert_eq!(r.charge_ah, None);
    assert_eq!(r.energy_wh, None);
}

#[test]
fn an_empty_unix_time_is_no_unix_time_and_not_damage() {
    let (rows, damaged) = drain(file(&["0:00:00.000,,5,1,0.6,0,1.68,0,,,,5"]));
    assert_eq!((rows.len(), damaged), (1, 0));
    assert_eq!(rows[0].unix, None);
}

// ---- AC-AXHK2S.3: a file that is not a recording is refused ----

#[test]
fn an_empty_file_is_refused() {
    assert!(matches!(refusal(b""), ReadError::Empty));
}

#[test]
fn a_wrong_header_is_refused() {
    for header in [
        "time,voltage,current\n0,5,1\n",
        "ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY\n",
        "ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR,EXTRA\n",
        "Unix,ElapsedTime,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR\n",
        "\n",
    ] {
        assert!(
            matches!(refusal(header.as_bytes()), ReadError::BadHeader),
            "{header:?} was not a bad header"
        );
    }
}

#[test]
fn a_binary_file_is_refused() {
    assert!(matches!(
        refusal(b"\x89PNG\r\n\x1a\n\xff\xfe\x00\x00\x80\x81"),
        ReadError::NotText
    ));
}

#[test]
fn a_file_of_one_huge_line_is_refused_without_reading_it_all_as_a_header() {
    let huge = vec![b'a'; 8 << 20];
    assert!(matches!(refusal(&huge), ReadError::BadHeader));
}

#[test]
fn every_refusal_is_said_in_plain_words() {
    let plain: [(ReadError, &str); 3] = [
        (refusal(b""), "empty"),
        (refusal(b"a,b\n"), "header"),
        (refusal(b"\xff\xfe\x00\x80"), "text"),
    ];
    for (error, word) in plain {
        let shown = error.to_string();
        assert!(
            shown.to_lowercase().contains(word),
            "{shown:?} lacks {word}"
        );
        assert!(!shown.contains("Utf8"), "{shown:?} is a debug dump");
        assert!(!shown.contains("BadHeader"), "{shown:?} is a debug dump");
    }
}

// ---- A read that fails is not a damaged row ----

/// Yields `head`, then fails every read.
struct FailsAfter {
    head: Vec<u8>,
}

impl Read for FailsAfter {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.head.is_empty() {
            return Err(io::Error::other("the disk went away"));
        }
        let n = out.len().min(self.head.len());
        out[..n].copy_from_slice(&self.head[..n]);
        self.head.drain(..n);
        Ok(n)
    }
}

fn failing(head: &[u8]) -> BufReader<FailsAfter> {
    BufReader::new(FailsAfter {
        head: head.to_vec(),
    })
}

#[test]
fn a_failed_read_of_the_header_is_an_io_error() {
    assert!(matches!(
        CsvReader::new(failing(b"")).err(),
        Some(ReadError::Io(..))
    ));
}

#[test]
fn a_failed_read_past_the_header_is_an_io_error_not_a_damaged_row() {
    let mut bytes = file(&[&good_row("0:00:00.000")]);
    bytes.extend_from_slice(b"0:00:00.100,");
    let mut reader = match CsvReader::new(failing(&bytes)) {
        Ok(reader) => reader,
        Err(e) => panic!("the header was refused: {e}"),
    };
    assert!(matches!(reader.next_row(), Ok(Some(RowOutcome::Row(_)))));
    assert!(matches!(reader.next_row(), Err(ReadError::Io(..))));
}

// ---- AC-AXHK2S.8: what the recorder writes reads back ----

fn near(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() <= 1e-6, "{what}: {a} against {b}");
}

#[test]
fn a_recording_reads_back_as_the_values_it_was_written_from() {
    let start_unix_ms = 1_790_000_000_000;
    let records: Vec<QueueRecord> = (0..500u32)
        .map(|k| {
            QueueRecord::from_raw(
                50_000 + k,
                20_000_000 + k as i32 * 37,
                -3_000_000 + k as i32 * 11,
                [1680 + k as u16, 4, 600, 1],
                Rate::Sps1000,
            )
        })
        .collect();
    let mut csv = CsvRecorder::new(Vec::new()).expect("header");
    for (k, r) in records.iter().enumerate() {
        if k == 250 {
            csv.break_run();
        }
        let temp = (k % 2 == 0).then_some(25.5);
        csv.push_with_temp(r, start_unix_ms, temp).expect("row");
    }
    let bytes = csv.finish().expect("flushed");

    let (rows, damaged) = drain(bytes);
    assert_eq!(damaged, 0);
    assert_eq!(rows.len(), records.len());
    for (k, (row, r)) in rows.iter().zip(&records).enumerate() {
        assert_eq!(row.elapsed_ms, k as u64);
        near(row.vbus_v, r.vbus_v(), "VBUS");
        near(row.ibus_a, r.ibus_a(), "IBUS");
        near(row.dp_v, r.dp_v(), "D+");
        near(row.dm_v, r.dm_v(), "D-");
        near(row.cc1_v, r.cc1_v(), "CC1");
        near(row.cc2_v, r.cc2_v(), "CC2");
        assert_eq!(row.temp_c, (k % 2 == 0).then_some(25.5));
        assert!(row.charge_ah.is_some() && row.energy_wh.is_some());
        near(
            row.unix.expect("a Unix time"),
            (start_unix_ms + k as u64) as f64 / 1000.0,
            "Unix",
        );
    }
}

#[test]
fn format_elapsed_reads_back_exactly() {
    for ms in [0, 7, 999, 61_005, 3_723_456, 360_000_000] {
        let (rows, damaged) = drain(file(&[&good_row(&format_elapsed(ms))]));
        assert_eq!((rows.len(), damaged), (1, 0));
        assert_eq!(rows[0].elapsed_ms, ms);
    }
}

/// `powerz records get` writes no line voltages at all: such a row is whole.
#[test]
fn a_row_with_all_four_lines_empty_is_a_meter_memory_row() {
    let text = format!("{CSV_HEADER}\n0:00:00.000,1.000,5,1,,,,,,0,0,5\n");
    let mut reader = CsvReader::new(text.as_bytes()).expect("the header is ours");
    match reader.next_row().expect("readable") {
        Some(RowOutcome::Row(row)) => {
            assert_eq!(row.vbus_v, 5.0);
            assert_eq!([row.dp_v, row.dm_v, row.cc1_v, row.cc2_v], [0.0; 4]);
        }
        other => panic!("not a row: {other:?}"),
    }
}
