//! `powerz pd` shows what each PD message means (FR TGE83A, AC .10): text
//! lines carry the message name and the PD decoder's one-line summary, and
//! JSON message lines gain `name` and `summary` next to the raw `header`
//! and `body`. Pinned against the demo device's PD scenario, live and for
//! `pd decode` of a saved file. The decoder keeps its context across the
//! records of one capture, so a Request decodes against the
//! Source_Capabilities seen before it. No hardware is touched.
//!
//! Test-only knobs (see `e2e_sim.rs`): `POWERZ_SIM_CLOCK=fake`,
//! `POWERZ_SIM_CTRL_C_AFTER=N`.

use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use powerz_protocol::pd::{Capture, Input};
use serde_json::Value;

const ENV_KNOBS: [&str; 3] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
];

/// 24 000 polls at 5 ms = 2 minutes of simulated capture.
const POLLS: usize = 24_000;

fn sim(polls: usize) -> Command {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.timeout(Duration::from_secs(120));
    c.env("POWERZ_SIM_CLOCK", "fake");
    c.env("POWERZ_SIM_CTRL_C_AFTER", polls.to_string());
    c.args(["--transport", "sim"]);
    c
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("pd-decoded-cli");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

fn stdout_of(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stdout).into_owned()
}

fn json_lines(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l:?}: {e}")))
        .collect()
}

fn is_message(v: &Value) -> bool {
    v["kind"] == "message" || v["kind"] == "big_message"
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex body"))
        .collect()
}

/// What the PD decoder makes of the records in `lines`, fed in order with
/// one context for the whole capture: `(name, summary)` per message line,
/// `None` for events.
fn expected(lines: &[Value]) -> Vec<Option<(String, String)>> {
    let mut capture = Capture::new();
    lines
        .iter()
        .map(|v| {
            if is_message(v) {
                let body = unhex(v["body"].as_str().expect("body"));
                let input = Input::Message {
                    sop: u8::try_from(v["sop"].as_u64().expect("sop")).expect("sop u8"),
                    header: u16::try_from(v["header"].as_u64().expect("header"))
                        .expect("header u16"),
                    body: &body,
                };
                let d = capture.feed(input).expect("a message decodes");
                Some((d.header.message_type.name().to_string(), d.summary()))
            } else {
                let input = match v["event"].as_str() {
                    Some("attach") => Some(Input::Attach),
                    Some("detach") => Some(Input::Detach),
                    Some("hard_reset") => Some(Input::HardReset),
                    Some("cable_reset") => Some(Input::CableReset),
                    _ => None,
                };
                if let Some(i) = input {
                    capture.feed(i);
                }
                None
            }
        })
        .collect()
}

fn live_json() -> Vec<Value> {
    let a = sim(POLLS).args(["pd", "--json"]).assert().success();
    json_lines(&stdout_of(&a))
}

fn live_text() -> Vec<String> {
    let a = sim(POLLS).arg("pd").assert().success();
    stdout_of(&a)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

/// Every JSON message line carries the decoder's name and summary, next to
/// the raw header and body; events keep their fields.
fn assert_decoded_json(lines: &[Value]) {
    assert!(!lines.is_empty(), "the demo device plays a PD capture");
    let want = expected(lines);
    for (v, w) in lines.iter().zip(&want) {
        match w {
            Some((name, summary)) => {
                assert_eq!(v["name"].as_str(), Some(name.as_str()), "name: {v}");
                assert_eq!(
                    v["summary"].as_str(),
                    Some(summary.as_str()),
                    "summary: {v}"
                );
                assert!(v["header"].is_u64(), "raw header kept: {v}");
                assert!(v["body"].is_string(), "raw body kept: {v}");
            }
            None => {
                assert_eq!(v["kind"], "event", "{v}");
                assert!(v["event"].is_string(), "event field kept: {v}");
            }
        }
    }
}

// ------------------------------------------------- AC-TGE83A.10: live pd

#[test]
fn pd_json_message_lines_carry_the_decoded_name_and_summary() {
    let lines = live_json();
    assert_decoded_json(&lines);

    let caps = lines
        .iter()
        .find(|v| v["name"] == "Source_Capabilities")
        .expect("a Source_Capabilities line");
    assert!(
        caps["summary"]
            .as_str()
            .is_some_and(|s| s.starts_with("Source_Capabilities: Fixed 5.00 V")),
        "{caps}"
    );

    // The Request is decoded against the Source_Capabilities seen earlier
    // in the same capture, so its PDO is known.
    let req = lines
        .iter()
        .find(|v| v["name"] == "Request")
        .expect("a Request line");
    let summary = req["summary"].as_str().unwrap_or_default();
    assert!(summary.starts_with("Request: #"), "{req}");
    assert!(!summary.contains("unknown PDO"), "context kept: {req}");
    assert!(
        summary.contains(" V "),
        "voltage of the requested PDO: {req}"
    );

    for name in ["Accept", "PS_RDY"] {
        assert!(
            lines.iter().any(|v| v["name"] == name),
            "{name} named in the JSON lines"
        );
    }
}

#[test]
fn pd_text_lines_show_the_decoded_name_and_summary() {
    let json = live_json();
    let text = live_text();
    assert_eq!(text.len(), json.len(), "one text line per JSON record");
    for (line, w) in text.iter().zip(expected(&json)) {
        if let Some((name, summary)) = w {
            assert!(line.contains(&name), "name {name:?} in {line:?}");
            assert!(line.contains(&summary), "summary {summary:?} in {line:?}");
        }
    }
    assert!(
        text.iter()
            .any(|l| l.contains("Source_Capabilities: Fixed 5.00 V")),
        "Source_Capabilities summary in the text output"
    );
}

// ------------------------------------------------ AC-TGE83A.10: pd decode

#[test]
fn pd_decode_json_of_a_saved_file_carries_the_decoded_name_and_summary() {
    let path = scratch("decoded-json.pdcap");
    sim(POLLS)
        .args(["pd", "--json", "--save"])
        .arg(&path)
        .assert()
        .success();
    let a = sim(0)
        .args(["pd", "decode", "--json"])
        .arg(&path)
        .assert()
        .success();
    let lines = json_lines(&stdout_of(&a));
    assert_decoded_json(&lines);
    assert!(
        lines.iter().any(|v| v["name"] == "Request"
            && v["summary"]
                .as_str()
                .is_some_and(|s| !s.contains("unknown PDO"))),
        "the Request decodes against the saved Source_Capabilities"
    );
}

#[test]
fn pd_decode_text_of_a_saved_file_shows_the_decoded_name_and_summary() {
    let path = scratch("decoded-text.pdcap");
    let live = sim(POLLS)
        .args(["pd", "--json", "--save"])
        .arg(&path)
        .assert()
        .success();
    let json = json_lines(&stdout_of(&live));
    let a = sim(0).args(["pd", "decode"]).arg(&path).assert().success();
    let out = stdout_of(&a);
    let text: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(text.len(), json.len(), "one line per saved record");
    for (line, w) in text.iter().zip(expected(&json)) {
        if let Some((name, summary)) = w {
            assert!(line.contains(&name), "name {name:?} in {line:?}");
            assert!(line.contains(&summary), "summary {summary:?} in {line:?}");
        }
    }
}
