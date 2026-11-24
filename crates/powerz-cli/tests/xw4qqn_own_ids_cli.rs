//! FR XW4QQN in the CLI:
//!
//! - `POWERZ_SIM_MODE=legacy-bootloader-own-ids` (test-only) runs the
//!   simulated old bootloader that numbers its own replies and answers the
//!   first CONNECT late (AC .5); `firmware probe` on it gets CONNECT
//!   accepted and HEAD field 0 from the standard path, with no variant
//!   (AC .1).
//! - The probe says when an answer's id differs from ours, in text and
//!   JSON, on the standard path and per variant, and adds nothing when
//!   the ids match (AC .4).
//! - String 2 prints cut at its first NUL: `"BOOT"` (AC .6).
//!
//! Every value is synthetic; no hardware is touched.

use std::time::Duration;

use assert_cmd::Command;
use powerz_cli::commands::firmware_probe_detailed;
use powerz_protocol::session::{ProbeFrame, ProbePipe, ProbeVariant, Session};
use powerz_protocol::transport::discovery::{UsbDetails, UsbIdentity, UsbInterface};
use powerz_protocol::transport::{Error as TransportError, Transport};
use powerz_sim::SimClock;
use serde_json::{Value, json};

const ENV_KNOBS: [&str; 9] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
    "POWERZ_SIM_HEAD",
    "POWERZ_SIM_APP_VERSION",
    "POWERZ_SIM_MODE",
    "POWERZ_SIM_FLASH_FAULT",
    "POWERZ_SIM_REPLUG_AFTER",
    "POWERZ_SIM_PROBE_ANSWER",
];

const OWN_IDS: (&str, &str) = ("POWERZ_SIM_MODE", "legacy-bootloader-own-ids");
const SILENT_OLD: (&str, &str) = ("POWERZ_SIM_MODE", "legacy-bootloader");

/// `powerz --transport sim firmware probe [--json]` with `env`; returns
/// (stdout, stderr, exit code).
fn sim_probe(json: bool, env: &[(&str, &str)]) -> (String, String, Option<i32>) {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_CLOCK", "fake");
    for (k, v) in env {
        c.env(k, v);
    }
    c.timeout(Duration::from_secs(60));
    c.args(["--transport", "sim", "firmware", "probe"]);
    if json {
        c.arg("--json");
    }
    let out = c.output().expect("powerz runs");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code(),
    )
}

/// Text output on the sim with `env`; must exit 0.
fn text(env: &[(&str, &str)]) -> String {
    let (out, err, code) = sim_probe(false, env);
    assert_eq!(code, Some(0), "stdout:\n{out}\nstderr:\n{err}");
    out
}

/// JSON output on the sim with `env`; must exit 0 and parse.
fn json_out(env: &[(&str, &str)]) -> Value {
    let (out, err, code) = sim_probe(true, env);
    assert_eq!(code, Some(0), "stdout:\n{out}\nstderr:\n{err}");
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("JSON ({e}):\n{out}\nstderr:\n{err}"))
}

fn line_starting<'a>(out: &'a str, prefix: &str) -> &'a str {
    let lines: Vec<&str> = out
        .lines()
        .filter(|l| l.trim_start().starts_with(prefix))
        .collect();
    assert_eq!(lines.len(), 1, "one line starting {prefix:?} in:\n{out}");
    lines[0]
}

// ---------------------------------------------------------------- AC-XW4QQN.5

#[test]
fn xw4qqn_cli_accepts_the_own_ids_sim_mode() {
    let (out, err, code) = sim_probe(false, &[OWN_IDS]);
    assert_eq!(code, Some(0), "stdout:\n{out}\nstderr:\n{err}");
}

#[test]
fn xw4qqn_cli_own_ids_sim_is_the_old_bootloader() {
    let out = text(&[OWN_IDS]);
    assert!(
        line_starting(&out, "Mode:").contains("bootloader (old, 2.1-style; experimental CLI-only flash with --experimental-old-bootloader)"),
        "{out}"
    );
}

// ---------------------------------------------------------------- AC-XW4QQN.1 / .4, text

#[test]
fn xw4qqn_cli_own_ids_connect_is_accepted_with_mismatched_ids() {
    let out = text(&[OWN_IDS]);
    assert_eq!(
        line_starting(&out, "CONNECT:"),
        "CONNECT: accepted, answered with mismatched ids (meter id 0x00, ours 0x01)",
        "{out}"
    );
}

#[test]
fn xw4qqn_cli_own_ids_head_gives_field_0_with_mismatched_ids() {
    let out = text(&[OWN_IDS]);
    assert_eq!(
        line_starting(&out, "HEAD field:"),
        "HEAD field: 0, answered with mismatched ids (meter id 0x01, ours 0x02)",
        "{out}"
    );
}

#[test]
fn xw4qqn_cli_own_ids_runs_no_variant() {
    let out = text(&[OWN_IDS]);
    assert!(
        !out.lines().any(|l| l.trim_start().starts_with("variant ")),
        "{out}"
    );
    assert!(!out.contains("First variant"), "{out}");
}

// ---------------------------------------------------------------- AC-XW4QQN.1 / .4, JSON

#[test]
fn xw4qqn_cli_own_ids_json_connect_and_head() {
    let v = json_out(&[OWN_IDS]);
    assert_eq!(
        (&v["connect"], &v["head"], &v["head_field"]),
        (&json!("accepted"), &json!("field"), &json!(0)),
        "{v}"
    );
}

#[test]
fn xw4qqn_cli_own_ids_json_gives_both_id_mismatches() {
    let v = json_out(&[OWN_IDS]);
    assert_eq!(
        (&v["connect_id_mismatch"], &v["head_id_mismatch"]),
        (
            &json!({"meter": 0, "ours": 1}),
            &json!({"meter": 1, "ours": 2})
        ),
        "{v}"
    );
}

#[test]
fn xw4qqn_cli_own_ids_json_lists_no_variant() {
    let v = json_out(&[OWN_IDS]);
    assert_eq!(v["variants"], json!([]), "{v}");
}

// ---------------------------------------------------------------- AC-XW4QQN.4, unchanged when ids match

#[test]
fn xw4qqn_cli_application_text_says_nothing_of_ids() {
    let out = text(&[]);
    assert!(!out.contains("mismatched ids"), "{out}");
}

#[test]
fn xw4qqn_cli_application_json_has_no_id_mismatch_keys() {
    let v = json_out(&[]);
    let o = v.as_object().expect("a JSON object");
    assert!(!o.contains_key("connect_id_mismatch"), "{v}");
    assert!(!o.contains_key("head_id_mismatch"), "{v}");
}

#[test]
fn xw4qqn_cli_silent_old_bootloader_json_has_no_id_mismatch_keys() {
    let v = json_out(&[SILENT_OLD]);
    let o = v.as_object().expect("a JSON object");
    assert!(!o.contains_key("connect_id_mismatch"), "{v}");
    assert!(!o.contains_key("head_id_mismatch"), "{v}");
}

// ---------------------------------------------------------------- AC-XW4QQN.4, variants

/// A 2.5 bootloader whose standard path never answers; its variants answer
/// from `replies`, in order. Each reply's id is either fixed (`Some`) or
/// our request id (`None`).
#[derive(Debug)]
struct VariantPipe {
    replies: Vec<(u8, Option<u8>)>,
    next: usize,
}

impl Transport for VariantPipe {
    fn transact(&mut self, _r: &[u8], _t: Duration) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::Timeout)
    }
    fn read(&mut self, _t: Duration) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::Timeout)
    }
}

impl ProbePipe for VariantPipe {
    fn probe_variant(
        &mut self,
        _variant: ProbeVariant,
        frame: &ProbeFrame,
        _timeout: Duration,
    ) -> Result<Vec<u8>, TransportError> {
        let Some(&(ty, id)) = self.replies.get(self.next) else {
            return Err(TransportError::Timeout);
        };
        self.next += 1;
        let id = id.unwrap_or(frame.bytes()[1]);
        Ok(if ty == 0x05 {
            vec![0x05, id, 0x00, 0x00]
        } else {
            let mut r = vec![0xC0, id, 0x01, 0x01];
            r.extend([0u8; 8]);
            r.extend([0xFF; 4]);
            r.extend([0u8; 4]);
            r
        })
    }
}

fn bootloader_25() -> UsbIdentity {
    UsbIdentity {
        vid: 0x5FC9,
        pid: 0x0063,
        product: Some("POWER-Z KM003C".to_owned()),
        interfaces: vec![UsbInterface {
            number: 0,
            class: 0xFF,
            subclass: 0x00,
            protocol: 0,
            string: Some("BOOT".to_owned()),
        }],
    }
}

/// Strict 2.5 path: CONNECT 1, recovery, CONNECT 2, HEAD 3; then the
/// variants send CONNECT/HEAD 4/5, 6/7 and 8/9.
fn variant_probe(json: bool) -> String {
    let pipe = VariantPipe {
        replies: vec![
            // no-drain-no-clear-halt: both mismatched.
            (0x05, Some(0x00)),
            (0x40, Some(0x01)),
            // in-before-out: CONNECT echoed, HEAD mismatched.
            (0x05, None),
            (0x40, Some(0x03)),
            // all-in-endpoints: both echoed.
            (0x05, None),
            (0x40, None),
        ],
        next: 0,
    };
    let mut s = Session::with_clock(pipe, SimClock::new(1_790_000_000_000));
    let mut out = Vec::new();
    firmware_probe_detailed(
        &mut s,
        &bootloader_25(),
        &UsbDetails::default(),
        json,
        &mut out,
    )
    .unwrap_or_else(|e| panic!("probe failed: {e}"));
    String::from_utf8_lossy(&out).into_owned()
}

#[test]
fn xw4qqn_variant_line_notes_both_mismatches() {
    let out = variant_probe(false);
    assert_eq!(
        line_starting(&out, "variant no-drain-no-clear-halt:"),
        "variant no-drain-no-clear-halt: CONNECT accepted, answered with mismatched ids \
         (meter id 0x00, ours 0x04), HEAD field 0, answered with mismatched ids \
         (meter id 0x01, ours 0x05)",
        "{out}"
    );
}

#[test]
fn xw4qqn_variant_line_notes_only_the_head_mismatch() {
    let out = variant_probe(false);
    assert_eq!(
        line_starting(&out, "variant in-before-out:"),
        "variant in-before-out: CONNECT accepted, HEAD field 0, answered with mismatched ids \
         (meter id 0x03, ours 0x07)",
        "{out}"
    );
}

#[test]
fn xw4qqn_variant_line_with_echoed_ids_is_unchanged() {
    let out = variant_probe(false);
    assert_eq!(
        line_starting(&out, "variant all-in-endpoints:"),
        "variant all-in-endpoints: CONNECT accepted, HEAD field 0",
        "{out}"
    );
}

#[test]
fn xw4qqn_variant_json_gives_the_mismatches_only_where_they_are() {
    let out = variant_probe(true);
    let v: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("JSON ({e}):\n{out}"));
    let vs = v["variants"].as_array().cloned().unwrap_or_default();
    let ids: Vec<(Option<Value>, Option<Value>)> = vs
        .iter()
        .map(|o| {
            (
                o.get("connect_id_mismatch").cloned(),
                o.get("head_id_mismatch").cloned(),
            )
        })
        .collect();
    assert_eq!(
        ids,
        vec![
            (
                Some(json!({"meter": 0, "ours": 4})),
                Some(json!({"meter": 1, "ours": 5}))
            ),
            (None, Some(json!({"meter": 3, "ours": 7}))),
            (None, None),
        ],
        "{v}"
    );
}

// ---------------------------------------------------------------- AC-XW4QQN.6

#[test]
fn xw4qqn_cli_own_ids_string_2_reads_boot_without_the_nul() {
    let out = text(&[OWN_IDS]);
    assert_eq!(
        line_starting(&out, "string 2:"),
        "string 2: \"BOOT\"",
        "{out}"
    );
}
