//! FR QF92WX in the CLI: `powerz firmware probe` prints (and `--json`
//! carries) every endpoint under its interface (AC .1), string descriptors
//! 1 to 4 (AC .2), which direction a CONNECT / HEAD timeout hit (AC .3), and
//! the outcome of each read-only variant with the first one that answered
//! (AC .4, .6). The command function runs over a hand-written fake pipe
//! with fixture descriptors; the binary against the simulator is in
//! `qf92wx_sim_probe_cli.rs`.
//!
//! Every value is synthetic; no hardware is touched.

mod common;

use std::time::Duration;

use common::*;
use powerz_cli::commands::firmware_probe_detailed;
use powerz_protocol::session::{ProbeFrame, ProbePipe, ProbeVariant};
use powerz_protocol::transport::discovery::{
    EndpointDirection, EndpointInfo, InterfaceEndpoints, StringDescriptor, TransferType,
    UsbDetails, UsbIdentity, UsbInterface,
};
use powerz_protocol::transport::{Error as TransportError, TimeoutDirection, Transport};
use serde_json::{Value, json};

const CONNECT: u8 = 0x02;

// ---------------------------------------------------------------- fixtures

/// The old bootloader's identity: one vendor interface (subclass 0xFF)
/// named like the product, plus a second interface to check grouping.
fn old_bootloader_identity() -> UsbIdentity {
    UsbIdentity {
        vid: 0x5FC9,
        pid: 0x0063,
        product: Some("POWER-Z KM003C".to_owned()),
        interfaces: vec![
            UsbInterface {
                number: 0,
                class: 0xFF,
                subclass: 0xFF,
                protocol: 0,
                string: Some("POWER-Z KM003C".to_owned()),
            },
            UsbInterface {
                number: 1,
                class: 0x0A,
                subclass: 0,
                protocol: 0,
                string: None,
            },
        ],
    }
}

fn ep(
    address: u8,
    direction: EndpointDirection,
    transfer: TransferType,
    max_packet_size: u16,
    interval: u8,
) -> EndpointInfo {
    EndpointInfo {
        address,
        direction,
        transfer,
        max_packet_size,
        interval,
    }
}

fn details() -> UsbDetails {
    use EndpointDirection::{In, Out};
    UsbDetails {
        interfaces: vec![
            InterfaceEndpoints {
                interface: 0,
                endpoints: vec![
                    ep(0x01, Out, TransferType::Bulk, 64, 0),
                    ep(0x81, In, TransferType::Bulk, 64, 0),
                    ep(0x82, In, TransferType::Interrupt, 8, 10),
                ],
            },
            InterfaceEndpoints {
                interface: 1,
                endpoints: vec![ep(0x83, In, TransferType::Bulk, 64, 0)],
            },
        ],
        strings: vec![
            (1, StringDescriptor::Text("ChargerLAB".to_owned())),
            (2, StringDescriptor::Text("POWER-Z KM003C".to_owned())),
            (3, StringDescriptor::Absent),
            (4, StringDescriptor::Unavailable("timed out".to_owned())),
        ],
    }
}

fn head_reply(field: u32) -> Vec<u8> {
    let mut f = vec![0x40, 0x00, 0x00, 0x01];
    f.extend(field.to_le_bytes());
    f.extend([0u8; 12]);
    f
}

/// A pipe silent on the standard path (timing out in `dir`) that answers
/// only on `answer_on`.
#[derive(Debug)]
struct Fake {
    dir: TimeoutDirection,
    answer_on: Option<ProbeVariant>,
}

impl Transport for Fake {
    fn transact(&mut self, _r: &[u8], _t: Duration) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::Timeout)
    }
    fn read(&mut self, _t: Duration) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::Timeout)
    }
    fn last_timeout_direction(&self) -> Option<TimeoutDirection> {
        Some(self.dir)
    }
}

impl ProbePipe for Fake {
    fn probe_variant(
        &mut self,
        variant: ProbeVariant,
        frame: &ProbeFrame,
        _t: Duration,
    ) -> Result<Vec<u8>, TransportError> {
        if self.answer_on != Some(variant) {
            return Err(TransportError::Timeout);
        }
        let f = frame.bytes();
        Ok(match f[0] & 0x7F {
            CONNECT => accept(f[1]),
            _ => head_reply(7),
        })
    }
}

/// Run the detailed probe over `fake` on the old-bootloader fixture.
fn run(fake: Fake, json: bool) -> String {
    let mut s = session(fake);
    let mut out = Vec::new();
    firmware_probe_detailed(
        &mut s,
        &old_bootloader_identity(),
        &details(),
        json,
        &mut out,
    )
    .unwrap_or_else(|e| panic!("probe failed: {e}"));
    text(&out)
}

fn silent_out() -> Fake {
    Fake {
        dir: TimeoutDirection::Out,
        answer_on: None,
    }
}

fn answering_in_before_out() -> Fake {
    Fake {
        dir: TimeoutDirection::In,
        answer_on: Some(ProbeVariant::InBeforeOut),
    }
}

fn line_index(out: &str, needle: &str) -> usize {
    out.lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no line with {needle:?} in:\n{out}"))
}

/// The one line starting (after indentation) with `prefix`.
fn line_starting(out: &str, prefix: &str) -> String {
    let lines: Vec<&str> = out
        .lines()
        .filter(|l| l.trim_start().starts_with(prefix))
        .collect();
    assert_eq!(lines.len(), 1, "one line starting {prefix:?} in:\n{out}");
    lines[0].to_owned()
}

// ---------------------------------------------------------------- AC .1 text

#[test]
fn qf92wx_text_prints_each_endpoint_with_every_field() {
    let out = run(silent_out(), false);
    for want in [
        "endpoint 0x01: OUT, bulk, max packet 64, interval 0",
        "endpoint 0x81: IN, bulk, max packet 64, interval 0",
        "endpoint 0x82: IN, interrupt, max packet 8, interval 10",
        "endpoint 0x83: IN, bulk, max packet 64, interval 0",
    ] {
        assert!(out.contains(want), "missing {want:?} in:\n{out}");
    }
}

#[test]
fn qf92wx_text_prints_endpoints_under_their_interface() {
    let out = run(silent_out(), false);
    let if0 = line_index(&out, "interface 0:");
    let if1 = line_index(&out, "interface 1:");
    let ep82 = line_index(&out, "endpoint 0x82:");
    let ep83 = line_index(&out, "endpoint 0x83:");
    assert!(if0 < ep82 && ep82 < if1, "0x82 under interface 0:\n{out}");
    assert!(if1 < ep83, "0x83 under interface 1:\n{out}");
}

// ---------------------------------------------------------------- AC .2 text

#[test]
fn qf92wx_text_prints_strings_1_to_4_by_index() {
    let out = run(silent_out(), false);
    for want in [
        "string 1: \"ChargerLAB\"",
        "string 2: \"POWER-Z KM003C\"",
        "string 3: (none)",
        "string 4: (unavailable: timed out)",
    ] {
        assert!(out.contains(want), "missing {want:?} in:\n{out}");
    }
}

// ---------------------------------------------------------------- AC .3 text

#[test]
fn qf92wx_text_names_the_out_write_timeout() {
    let out = run(silent_out(), false);
    assert!(out.contains("CONNECT: timed out (OUT write)"), "{out}");
    assert!(out.contains("HEAD: timed out (OUT write)"), "{out}");
}

#[test]
fn qf92wx_text_names_the_in_read_timeout() {
    let out = run(answering_in_before_out(), false);
    assert!(out.contains("CONNECT: timed out (IN read)"), "{out}");
    assert!(out.contains("HEAD: timed out (IN read)"), "{out}");
}

// ---------------------------------------------------------------- AC .4 text

#[test]
fn qf92wx_text_prints_each_variant_outcome() {
    let out = run(answering_in_before_out(), false);
    let quiet = line_starting(&out, "variant no-drain-no-clear-halt:");
    assert!(
        quiet.contains("CONNECT timed out") && quiet.contains("HEAD timed out"),
        "{quiet}"
    );
    let answered = line_starting(&out, "variant in-before-out:");
    assert!(
        answered.contains("CONNECT accepted") && answered.contains("HEAD field 7"),
        "{answered}"
    );
    line_starting(&out, "variant all-in-endpoints:");
}

#[test]
fn qf92wx_text_names_the_first_variant_to_answer() {
    let out = run(answering_in_before_out(), false);
    assert!(
        out.contains("First variant to answer: in-before-out"),
        "{out}"
    );
}

#[test]
fn qf92wx_text_says_no_variant_answered() {
    let out = run(silent_out(), false);
    assert!(out.contains("First variant to answer: none"), "{out}");
}

// ---------------------------------------------------------------- AC .6 JSON

fn run_json(fake: Fake) -> Value {
    let out = run(fake, true);
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("JSON ({e}):\n{out}"))
}

#[test]
fn qf92wx_json_carries_endpoints_per_interface() {
    let v = run_json(silent_out());
    assert_eq!(
        v["interfaces"][0]["endpoints"],
        json!([
            {"address": 1, "direction": "out", "transfer": "bulk", "max_packet_size": 64, "interval": 0},
            {"address": 129, "direction": "in", "transfer": "bulk", "max_packet_size": 64, "interval": 0},
            {"address": 130, "direction": "in", "transfer": "interrupt", "max_packet_size": 8, "interval": 10},
        ])
    );
    assert_eq!(
        v["interfaces"][1]["endpoints"],
        json!([
            {"address": 131, "direction": "in", "transfer": "bulk", "max_packet_size": 64, "interval": 0},
        ])
    );
}

#[test]
fn qf92wx_json_carries_strings_1_to_4() {
    let v = run_json(silent_out());
    assert_eq!(
        v["strings"],
        json!({
            "1": "ChargerLAB",
            "2": "POWER-Z KM003C",
            "3": null,
            "4": {"unavailable": "timed out"},
        })
    );
}

#[test]
fn qf92wx_json_carries_the_timeout_direction() {
    let v = run_json(silent_out());
    assert_eq!(v["connect_timeout"], json!("out"));
    assert_eq!(v["head_timeout"], json!("out"));
}

#[test]
fn qf92wx_json_has_one_entry_per_variant() {
    let v = run_json(answering_in_before_out());
    let variants = v["variants"].as_array().expect("variants array");
    let got: Vec<Value> = variants
        .iter()
        .map(|e| json!({"name": e["name"], "connect": e["connect"], "head": e["head"]}))
        .collect();
    assert_eq!(
        got,
        vec![
            json!({"name": "no-drain-no-clear-halt", "connect": "timed out", "head": "timed out"}),
            json!({"name": "in-before-out", "connect": "accepted", "head": "field"}),
            json!({"name": "all-in-endpoints", "connect": "timed out", "head": "timed out"}),
        ]
    );
}

#[test]
fn qf92wx_json_names_the_first_variant_to_answer() {
    let v = run_json(answering_in_before_out());
    assert_eq!(v["variant_answered"], json!("in-before-out"));
}

#[test]
fn qf92wx_json_variant_answered_is_null_when_none_did() {
    let v = run_json(silent_out());
    assert_eq!(v["variant_answered"], Value::Null);
    assert_eq!(v["variants"].as_array().map(Vec::len), Some(3));
}
