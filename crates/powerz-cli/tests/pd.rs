//! `powerz pd` (FR TGE83A, AC .6): streams PD capture records as text
//! lines or JSON Lines until Ctrl-C, and sends the stop command (0x11) on
//! Ctrl-C, on error and on normal exit. Command-function tests against
//! scripted `MockTransport` sessions; all frames are synthetic.

mod common;

use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::Parser;
use common::*;
use powerz_cli::{Cli, CliError, Command, TransportKind, pd, validate};
use powerz_protocol::frame::SectionHeader;
use powerz_protocol::session::Session;
use powerz_protocol::transport::mock::{MockTransport, Step};
use powerz_protocol::transport::{Error as TransportError, Transport};
use serde_json::Value;

// ---------------------------------------------------------------- frames

fn start_req(id: u8) -> Vec<u8> {
    vec![0x10, id, 0x02, 0x00]
}

fn stop_req(id: u8) -> Vec<u8> {
    vec![0x11, id, 0x00, 0x00]
}

fn status(ts: u32) -> Vec<u8> {
    let mut s = Vec::new();
    s.extend(ts.to_le_bytes());
    s.extend(5_000u16.to_le_bytes());
    s.extend(0i16.to_le_bytes());
    s.extend(1_528u16.to_le_bytes());
    s.extend(0u16.to_le_bytes());
    s
}

fn msg(meter: bool, ts: u32, header: u16, body: &[u8]) -> Vec<u8> {
    let len = 8 + body.len();
    let mut r = vec![0x80 | if meter { 0x40 } else { 0 } | (len as u8 - 1)];
    r.extend(ts.to_le_bytes());
    r.push(0x00);
    r.extend(header.to_le_bytes());
    r.extend(body);
    r
}

fn event(meter: bool, ts: u32, code: u8) -> Vec<u8> {
    let mut r = vec![0x05 | if meter { 0x40 } else { 0 }];
    r.extend(ts.to_le_bytes());
    r.push(code);
    r
}

fn pd_reply(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = put_header(id, 1).to_vec();
    f.extend(
        SectionHeader {
            att: 0x010,
            next: false,
            chunk: 0,
            size: payload.len() as u16,
        }
        .encode(),
    );
    f.extend(payload);
    f
}

fn pd_poll(id: u8, payload: &[u8]) -> Step {
    Step::transact(get_data(id, 0x010), pd_reply(id, payload))
}

/// ADC snapshot reply (id 1) whose header reports auth level 1.
fn adc_level1() -> Step {
    let mut f = put_header(1, 1).to_vec();
    f.extend(
        SectionHeader {
            att: 0x001,
            next: false,
            chunk: 1,
            size: 40,
        }
        .encode(),
    );
    f.extend(adc_payload(ADC_A));
    Step::transact(get_data(1, 0x001), f)
}

const SRC_CAPS: [u8; 8] = [0x2C, 0x91, 0x81, 0x09, 0x2C, 0xD1, 0x02, 0x00];

/// Poll 3: Attach CC1 (meter) + Source_Capabilities (charger).
fn first_chunk() -> Vec<u8> {
    [
        status(10_000),
        event(true, 10_000, 0x11),
        msg(false, 10_040, 0x21A1, &SRC_CAPS),
    ]
    .concat()
}

/// Poll 4: Request (meter) + Hard Reset (meter).
fn second_chunk() -> Vec<u8> {
    [
        status(10_300),
        msg(true, 10_042, 0x1082, &[0x2C, 0xB1, 0x44, 0x13]),
        event(true, 10_250, 0x04),
    ]
    .concat()
}

/// Transport that sets `stop` (Ctrl-C) once `after` PD polls were answered.
struct CtrlCAfter {
    inner: MockTransport,
    stop: Arc<AtomicBool>,
    after: usize,
    polls: usize,
}

impl Transport for CtrlCAfter {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        let reply = self.inner.transact(request, timeout);
        if request.first() == Some(&0x0C) && request.get(2) == Some(&0x20) {
            self.polls += 1;
            if self.polls == self.after {
                self.stop.store(true, Ordering::SeqCst);
            }
        }
        reply
    }
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.inner.read(timeout)
    }
}

/// Session at level 1 (ADC read done) over a transport that presses Ctrl-C
/// after `after` PD polls; `steps` follow from id 2.
fn ctrl_c_session(
    steps: Vec<Step>,
    after: usize,
) -> (Session<CtrlCAfter, FakeClock>, Arc<AtomicBool>) {
    let stop = Arc::new(AtomicBool::new(false));
    let t = CtrlCAfter {
        inner: MockTransport::new([vec![adc_level1()], steps].concat()),
        stop: Arc::clone(&stop),
        after,
        polls: 0,
    };
    let mut s = session(t);
    s.read_adc().unwrap();
    assert_eq!(s.auth_level(), 1);
    (s, stop)
}

fn two_poll_script() -> Vec<Step> {
    vec![
        Step::transact(start_req(2), accept(2)),
        pd_poll(3, &first_chunk()),
        pd_poll(4, &second_chunk()),
        Step::transact(stop_req(5), accept(5)),
    ]
}

fn stops_sent(written: &[Vec<u8>]) -> usize {
    written.iter().filter(|w| w.first() == Some(&0x11)).count()
}

// ---- AC-TGE83A.6: arguments ----

#[test]
fn pd_subcommand_parses_with_and_without_json() {
    match Cli::try_parse_from(["powerz", "pd"]).unwrap().command {
        Command::Pd { json } => assert!(!json),
        other => panic!("{other:?}"),
    }
    let cli = Cli::try_parse_from(["powerz", "pd", "--json"]).unwrap();
    assert_eq!(cli.command, Command::Pd { json: true });
    assert_eq!(cli.command.name(), "pd");
}

#[test]
fn pd_needs_the_bulk_pipe() {
    for pipe in ["hid", "cdc"] {
        let cli = Cli::try_parse_from(["powerz", "--transport", pipe, "pd"]).unwrap();
        match validate(&cli) {
            Err(CliError::Usage(m)) => assert!(m.contains("pd"), "{m}"),
            other => panic!("{pipe}: expected a usage error, got {other:?}"),
        }
    }
    let cli = Cli::try_parse_from(["powerz", "--transport", "sim", "pd"]).unwrap();
    assert_eq!(cli.transport, TransportKind::Sim);
    assert_eq!(validate(&cli), Ok(()));
}

// ---- AC-TGE83A.6: output ----

#[test]
fn json_lines_one_object_per_record_until_ctrl_c_then_stop() {
    let (mut s, stop) = ctrl_c_session(two_poll_script(), 2);
    let mut out = Vec::new();
    pd(&mut s, true, &stop, &mut out).unwrap();
    assert_eq!(s.transport().inner.remaining(), 0, "0x11 sent on Ctrl-C");
    assert_eq!(s.transport().inner.written().last(), Some(&stop_req(5)));
    assert_eq!(stops_sent(s.transport().inner.written()), 1);

    let text = text(&out);
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l:?}: {e}")))
        .collect();
    assert_eq!(lines.len(), 4, "one JSON object per record:\n{text}");

    let attach = &lines[0];
    assert_eq!(attach["kind"], "event");
    assert_eq!(attach["event"], "attach");
    assert!(attach["cc"] == 1 || attach["cc"] == "cc1" || attach["cc"] == "CC1");
    assert_eq!(attach["from_meter"], true);
    assert_eq!(attach["elapsed_ms"], 0);

    let caps = &lines[1];
    assert_eq!(caps["kind"], "message");
    assert_eq!(caps["sop"], 0);
    assert_eq!(caps["header"], 0x21A1);
    assert_eq!(caps["from_meter"], false);
    assert_eq!(caps["elapsed_ms"], 40);
    assert_eq!(caps["body"], hex_of(&SRC_CAPS));

    let req = &lines[2];
    assert_eq!(req["kind"], "message");
    assert_eq!(req["header"], 0x1082);
    assert_eq!(req["from_meter"], true);
    assert_eq!(req["elapsed_ms"], 42);

    let reset = &lines[3];
    assert_eq!(reset["kind"], "event");
    assert_eq!(reset["event"], "hard_reset");
    assert_eq!(reset["from_meter"], true);
    assert_eq!(reset["elapsed_ms"], 250);
}

#[test]
fn text_lines_one_per_record() {
    let (mut s, stop) = ctrl_c_session(two_poll_script(), 2);
    let mut out = Vec::new();
    pd(&mut s, false, &stop, &mut out).unwrap();
    assert_eq!(s.transport().inner.remaining(), 0);

    let text = text(&out);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 4, "one line per record:\n{text}");
    assert!(
        serde_json::from_str::<Value>(lines[0]).is_err(),
        "plain text, not JSON: {}",
        lines[0]
    );
    let lower: Vec<String> = lines.iter().map(|l| l.to_lowercase()).collect();
    assert!(
        lower[0].contains("attach") && lower[0].contains("cc1"),
        "{}",
        lines[0]
    );
    assert!(
        lower[1].contains("21a1") || lower[1].contains("a121") || lower[1].contains("a1 21"),
        "message line shows the PD header: {}",
        lines[1]
    );
    assert!(lower[1].contains("sop"), "{}", lines[1]);
    assert!(
        lower[3].contains("hard reset") || lower[3].contains("hard_reset"),
        "{}",
        lines[3]
    );
}

#[test]
fn ctrl_c_before_any_record_still_stops() {
    let script = vec![
        Step::transact(start_req(2), accept(2)),
        pd_poll(3, &status(1)),
        Step::transact(stop_req(4), accept(4)),
    ];
    let (mut s, stop) = ctrl_c_session(script, 1);
    let mut out = Vec::new();
    pd(&mut s, true, &stop, &mut out).unwrap();
    assert!(out.is_empty(), "no records, no lines: {:?}", text(&out));
    assert_eq!(s.transport().inner.remaining(), 0);
    assert_eq!(stops_sent(s.transport().inner.written()), 1);
}

#[test]
fn polls_are_spaced_by_the_sniffer_period() {
    let script = [
        vec![Step::transact(start_req(2), accept(2))],
        (3..=7)
            .map(|id| pd_poll(id, &status(u32::from(id))))
            .collect(),
        vec![Step::transact(stop_req(8), accept(8))],
    ]
    .concat();
    let (mut s, stop) = ctrl_c_session(script, 5);
    let t0 = s.now_ms();
    pd(&mut s, true, &stop, &mut Vec::new()).unwrap();
    let elapsed = s.now_ms() - t0;
    assert!(
        (16..=30).contains(&elapsed),
        "5 polls ≈ 5 ms apart on the session clock: {elapsed} ms"
    );
}

#[test]
fn a_failed_poll_sends_stop_and_is_a_device_error() {
    let script = vec![
        Step::transact(start_req(2), accept(2)),
        pd_poll(3, &first_chunk()),
        Step::transact_err(get_data(4, 0x010), TransportError::Disconnected),
        Step::transact(stop_req(5), accept(5)),
    ];
    let (mut s, stop) = ctrl_c_session(script, usize::MAX);
    let mut out = Vec::new();
    let r = pd(&mut s, true, &stop, &mut out);
    assert!(matches!(r, Err(CliError::Device(_))), "{r:?}");
    assert_eq!(s.transport().inner.written().last(), Some(&stop_req(5)));
    assert_eq!(s.transport().inner.remaining(), 0);
    // What arrived before the error was printed.
    assert_eq!(text(&out).lines().count(), 2);
}

#[test]
fn a_rejected_start_is_a_device_error_without_stop() {
    let script = vec![Step::transact(start_req(2), vec![0x06, 0x02, 0x00, 0x00])];
    let (mut s, stop) = ctrl_c_session(script, usize::MAX);
    let r = pd(&mut s, true, &stop, &mut Vec::new());
    match r {
        Err(CliError::Device(m)) => assert!(m.contains("0x10"), "{m}"),
        other => panic!("expected a device error, got {other:?}"),
    }
    assert_eq!(stops_sent(s.transport().inner.written()), 0);
}

/// A sink whose every write fails (e.g. a closed pipe).
struct Broken;

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"))
    }
}

#[test]
fn an_output_error_sends_stop_and_fails() {
    let script = vec![
        Step::transact(start_req(2), accept(2)),
        pd_poll(3, &first_chunk()),
        Step::transact(stop_req(4), accept(4)),
    ];
    let (mut s, stop) = ctrl_c_session(script, usize::MAX);
    let r = pd(&mut s, false, &stop, &mut Broken);
    assert!(r.is_err(), "a failed write ends the capture with an error");
    assert_eq!(s.transport().inner.written().last(), Some(&stop_req(4)));
    assert_eq!(s.transport().inner.remaining(), 0);
}
