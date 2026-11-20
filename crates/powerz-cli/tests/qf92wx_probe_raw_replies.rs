//! FR QF92WX, AC .4 / .6 in the CLI: a variant's unexpected replies stay
//! visible. A CONNECT reply that is neither ACCEPT nor REJECT prints as
//! `raw <hex>` (JSON `connect` "raw" with `connect_raw`); a raw or rejected
//! HEAD reply is in the variant's JSON `head_raw`; a variant that got any
//! reply counts as the first to answer.
//!
//! Over a hand-written fake pipe; every value is synthetic.

mod common;

use std::time::Duration;

use common::*;
use powerz_cli::commands::firmware_probe_detailed;
use powerz_protocol::session::{ProbeFrame, ProbePipe, ProbeVariant};
use powerz_protocol::transport::discovery::{UsbDetails, UsbIdentity, UsbInterface};
use powerz_protocol::transport::{Error as TransportError, TimeoutDirection, Transport};
use serde_json::{Value, json};

fn identity() -> UsbIdentity {
    UsbIdentity {
        vid: 0x5FC9,
        pid: 0x0063,
        product: Some("POWER-Z KM003C".to_owned()),
        interfaces: vec![UsbInterface {
            number: 0,
            class: 0xFF,
            subclass: 0xFF,
            protocol: 0,
            string: None,
        }],
    }
}

/// Standard path silent; `no-drain-no-clear-halt` answers CONNECT with a
/// PUT_DATA-typed frame and HEAD with REJECT; `in-before-out` answers HEAD
/// with an undecodable 0x40 frame; the rest time out.
#[derive(Debug)]
struct Fake;

const CONNECT_ODD: [u8; 4] = [0x41, 0x00, 0x00, 0x00];
const HEAD_REJECT: [u8; 4] = [0x06, 0x00, 0x00, 0x00];
const HEAD_SHORT: [u8; 4] = [0x40, 0x00, 0x00, 0x01];

impl Transport for Fake {
    fn transact(&mut self, _r: &[u8], _t: Duration) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::Timeout)
    }
    fn read(&mut self, _t: Duration) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::Timeout)
    }
    fn last_timeout_direction(&self) -> Option<TimeoutDirection> {
        Some(TimeoutDirection::In)
    }
}

impl ProbePipe for Fake {
    fn probe_variant(
        &mut self,
        variant: ProbeVariant,
        frame: &ProbeFrame,
        _t: Duration,
    ) -> Result<Vec<u8>, TransportError> {
        let connect = frame.bytes()[0] & 0x7F == 0x02;
        match (variant, connect) {
            (ProbeVariant::NoDrainNoClearHalt, true) => Ok(CONNECT_ODD.to_vec()),
            (ProbeVariant::NoDrainNoClearHalt, false) => Ok(HEAD_REJECT.to_vec()),
            (ProbeVariant::InBeforeOut, false) => Ok(HEAD_SHORT.to_vec()),
            _ => Err(TransportError::Timeout),
        }
    }
}

fn run(json: bool) -> String {
    let mut s = session(Fake);
    let mut out = Vec::new();
    firmware_probe_detailed(&mut s, &identity(), &UsbDetails::default(), json, &mut out)
        .unwrap_or_else(|e| panic!("probe failed: {e}"));
    text(&out)
}

fn variant_json(v: &Value, name: &str) -> Value {
    v["variants"]
        .as_array()
        .expect("variants array")
        .iter()
        .find(|e| e["name"] == json!(name))
        .cloned()
        .unwrap_or_else(|| panic!("no variant {name} in {v}"))
}

#[test]
fn qf92wx_text_prints_an_unexpected_variant_connect_reply_raw() {
    let out = run(false);
    let line = out
        .lines()
        .find(|l| l.starts_with("variant no-drain-no-clear-halt:"))
        .unwrap_or_else(|| panic!("no variant line in:\n{out}"));
    assert!(line.contains("CONNECT raw 41 00 00 00"), "{line}");
    assert!(line.contains("HEAD rejected 06 00 00 00"), "{line}");
}

#[test]
fn qf92wx_text_names_a_variant_that_only_got_odd_replies_as_answered() {
    let out = run(false);
    assert!(
        out.contains("First variant to answer: no-drain-no-clear-halt"),
        "{out}"
    );
}

#[test]
fn qf92wx_json_variant_carries_connect_raw_and_head_raw() {
    let v: Value = serde_json::from_str(&run(true)).expect("JSON");
    let odd = variant_json(&v, "no-drain-no-clear-halt");
    assert_eq!(odd["connect"], json!("raw"));
    assert_eq!(odd["connect_raw"], json!("41000000"));
    assert_eq!(odd["head"], json!("rejected"));
    assert_eq!(odd["head_raw"], json!("06000000"));

    let short = variant_json(&v, "in-before-out");
    assert_eq!(short["connect"], json!("timed out"));
    assert_eq!(short["connect_raw"], Value::Null);
    assert_eq!(short["head"], json!("raw"));
    assert_eq!(short["head_raw"], json!("40000001"));

    let quiet = variant_json(&v, "all-in-endpoints");
    assert_eq!(quiet["connect_raw"], Value::Null);
    assert_eq!(quiet["head_raw"], Value::Null);

    assert_eq!(v["variant_answered"], json!("no-drain-no-clear-halt"));
}
