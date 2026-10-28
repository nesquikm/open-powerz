//! `powerz stream --source cdc` (FR 1HHT02, Technical Design; used by the
//! 1HHT04 ADCNEW stage): record the ADCNEW push stream on the CDC port to
//! the `stream` CSV at 50 or 1000 records/s, then print one summary line
//! with the records, gaps, dropped frames and CRC errors. Runs against the
//! simulator's CDC persona on its simulated clock.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use clap::Parser;
use powerz_cli::args::WatchSource;
use powerz_cli::commands::stream_cdc;
use powerz_cli::record::CSV_HEADER;
use powerz_cli::{Cli, Command, StreamOptions, validate};
use powerz_protocol::decode::Rate;
use powerz_protocol::session::Session;
use powerz_sim::{FaultPlan, Persona, Scenario, SimClock, SimDevice};

const T0: u64 = 1_790_000_000_000;

fn text(buf: &[u8]) -> String {
    String::from_utf8_lossy(buf).into_owned()
}

fn session(persona: Persona, faults: FaultPlan) -> Session<SimDevice, SimClock> {
    let clock = SimClock::new(T0);
    let dev = SimDevice::with_clock(persona, Scenario::Load5v1a, clock.clone()).with_faults(faults);
    Session::with_clock(dev, clock)
}

fn opts(rate: Rate, secs: u64) -> StreamOptions {
    StreamOptions {
        rate,
        duration: Some(Duration::from_secs(secs)),
        start_unix_ms: T0,
    }
}

/// CSV data rows (header checked).
fn rows(csv: &[u8]) -> Vec<Vec<String>> {
    let t = text(csv);
    let mut lines = t.lines();
    assert_eq!(lines.next(), Some(CSV_HEADER), "{t}");
    lines
        .map(|l| l.split(',').map(str::to_owned).collect())
        .collect()
}

// ---- arguments ----

#[test]
fn stream_source_defaults_to_the_queue_and_takes_cdc() {
    let c = Cli::try_parse_from(["powerz", "stream", "--rate", "50", "--csv", "a.csv"]).unwrap();
    assert!(
        matches!(
            c.command,
            Command::Stream {
                source: WatchSource::Poll,
                ..
            }
        ),
        "{:?}",
        c.command
    );
    let c = Cli::try_parse_from([
        "powerz",
        "stream",
        "--source",
        "cdc",
        "--rate",
        "1000",
        "--duration",
        "120",
        "--csv",
        "a.csv",
    ])
    .unwrap();
    assert!(
        matches!(
            c.command,
            Command::Stream {
                source: WatchSource::Cdc,
                rate: 1000,
                duration: Some(d),
                ..
            } if d == 120.0
        ),
        "{:?}",
        c.command
    );
    assert_eq!(validate(&c), Ok(()));
}

#[test]
fn cdc_stream_takes_only_50_or_1000_records_per_second() {
    for rate in ["2", "10"] {
        let c = Cli::try_parse_from([
            "powerz", "stream", "--source", "cdc", "--rate", rate, "--csv", "a.csv",
        ])
        .unwrap();
        let e = validate(&c).expect_err(rate);
        assert!(e.to_string().contains("50"), "{e}");
        assert!(e.to_string().contains("1000"), "{e}");
    }
}

#[test]
fn cdc_stream_refuses_hid_but_ignores_another_transport() {
    let hid = Cli::try_parse_from([
        "powerz",
        "--transport",
        "hid",
        "stream",
        "--source",
        "cdc",
        "--rate",
        "50",
        "--csv",
        "a.csv",
    ])
    .unwrap();
    assert!(validate(&hid).is_err());
    for t in ["bulk", "cdc", "sim"] {
        let c = Cli::try_parse_from([
            "powerz",
            "--transport",
            t,
            "stream",
            "--source",
            "cdc",
            "--rate",
            "50",
            "--csv",
            "a.csv",
        ])
        .unwrap();
        assert_eq!(validate(&c), Ok(()), "--transport {t}");
    }
}

// ---- recording ----

#[test]
fn cdc_stream_writes_one_row_per_record_until_the_duration() {
    let mut s = session(Persona::Cdc, FaultPlan::new());
    let stop = AtomicBool::new(false);
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let summary = stream_cdc(&mut s, &opts(Rate::Sps50, 2), &stop, &mut csv, &mut err).unwrap();

    let rows = rows(&csv);
    // 2 s at 50 records/s, the record at exactly 2 s not written.
    assert_eq!(rows.len(), 100, "rows: {}", rows.len());
    assert_eq!(summary.records, 100);
    assert_eq!(rows[0][0], "0:00:00.000");
    assert_eq!(rows[1][0], "0:00:00.020");
    let vbus: f64 = rows[10][2].parse().unwrap();
    assert!((vbus - 5.0).abs() < 0.1, "VBUS {vbus}");
    assert_eq!(rows[10][8], "", "ADCNEW records carry no temperature");
    assert!(summary.gaps.is_empty(), "{:?}", summary.gaps);
    assert_eq!((summary.dropped_frames, summary.crc_errors), (0, 0));
    assert!(
        !s.transport().adcnew_active(),
        "the stream must be stopped at the end"
    );
    assert_eq!(
        text(&err).trim_end(),
        "100 records, 0 gaps, 0 dropped frames, 0 CRC errors"
    );
}

#[test]
fn cdc_stream_runs_at_1000_records_per_second() {
    let mut s = session(Persona::Cdc, FaultPlan::new());
    let stop = AtomicBool::new(false);
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let summary = stream_cdc(&mut s, &opts(Rate::Sps1000, 1), &stop, &mut csv, &mut err).unwrap();
    assert_eq!(summary.records, 1000);
    assert_eq!(rows(&csv)[1][0], "0:00:00.001");
    assert!(summary.gaps.is_empty(), "{:?}", summary.gaps);
}

#[test]
fn a_corrupt_frame_counts_as_a_crc_error_and_a_gap() {
    let mut s = session(Persona::Cdc, FaultPlan::new().corrupt_adcnew_at(5));
    let stop = AtomicBool::new(false);
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let summary = stream_cdc(&mut s, &opts(Rate::Sps50, 2), &stop, &mut csv, &mut err).unwrap();
    assert_eq!(summary.crc_errors, 1);
    assert_eq!(summary.gaps.len(), 1, "{:?}", summary.gaps);
    let lost = summary.gaps[0].expected - summary.gaps[0].received;
    assert_eq!(summary.records + u64::from(lost), 100);
    let err = text(&err);
    let first = err.lines().next().unwrap_or_default();
    assert_eq!(
        first,
        format!(
            "{} records, 1 gaps, 0 dropped frames, 1 CRC errors",
            summary.records
        )
    );
    assert!(err.contains("gap "), "gap lines follow: {err}");
}

#[test]
fn cdc_stream_needs_the_cdc_pipe_and_writes_no_csv_without_it() {
    let mut s = session(Persona::Bulk, FaultPlan::new());
    let stop = AtomicBool::new(false);
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let r = stream_cdc(&mut s, &opts(Rate::Sps50, 1), &stop, &mut csv, &mut err);
    assert!(r.is_err());
    assert!(csv.is_empty(), "no header-only CSV: {}", text(&csv));
}

#[test]
fn cdc_stream_stops_on_ctrl_c() {
    let mut s = session(Persona::Cdc, FaultPlan::new());
    let stop = AtomicBool::new(true);
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let opts = StreamOptions {
        rate: Rate::Sps50,
        duration: None,
        start_unix_ms: T0,
    };
    stream_cdc(&mut s, &opts, &stop, &mut csv, &mut err).unwrap();
    assert!(!s.transport().adcnew_active());
}

#[test]
fn a_start_echoed_after_a_bulk_session_is_sent_again_and_the_stream_records() {
    // [device] 2026-10-28: right after a bulk session the meter echoes the
    // start (`02 02`) and streams nothing until the host keeps quiet ~10 s.
    let faults = FaultPlan::parse("echo-start").unwrap();
    let mut s = session(Persona::Cdc, faults);
    let stop = AtomicBool::new(false);
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let summary = stream_cdc(&mut s, &opts(Rate::Sps50, 2), &stop, &mut csv, &mut err).unwrap();
    assert_eq!(summary.records, 100);
    assert_eq!((summary.crc_errors, summary.dropped_frames), (0, 0));
    assert!(summary.gaps.is_empty(), "{:?}", summary.gaps);
    let err = text(&err);
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines.len(), 2, "{err}");
    assert!(lines[0].contains("echoed"), "{err}");
    assert_eq!(
        lines[1],
        "100 records, 0 gaps, 0 dropped frames, 0 CRC errors"
    );
}
