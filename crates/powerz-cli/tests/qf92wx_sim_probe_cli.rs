//! FR QF92WX in the binary: `powerz --transport sim firmware probe` on the
//! simulated old (2.1) bootloader prints every endpoint (AC .1), strings 1
//! to 4 (AC .2), which direction the CONNECT / HEAD timeout hit (AC .3,
//! the regression), each read-only variant's outcome and the first that
//! answered (AC .4, .6); the variants send only CONNECT and HEAD and
//! application mode gets none (AC .5).
//!
//! Only the binary's existing interface is used, so these tests run
//! unchanged before and after the fix. Test-only knobs:
//!
//! - `POWERZ_SIM_MODE=legacy-bootloader`: the old (2.1) bootloader, silent
//!   to the standard CONNECT and HEAD.
//! - `POWERZ_SIM_PROBE_ANSWER=<variant name>`: that persona answers on
//!   this one variant only.
//! - `POWERZ_SIM_FAULT=timeout`: the meter never answers.
//!
//! Every value is synthetic; no hardware is touched.

use std::time::Duration;

use assert_cmd::Command;
use serde_json::{Value, json};

const CONNECT: u8 = 0x02;
const HEAD: u8 = 0x40;

/// The one line starting (after indentation) with `prefix`.
fn line_starting(out: &str, prefix: &str) -> String {
    let lines: Vec<&str> = out
        .lines()
        .filter(|l| l.trim_start().starts_with(prefix))
        .collect();
    assert_eq!(lines.len(), 1, "one line starting {prefix:?} in:\n{out}");
    lines[0].to_owned()
}

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

/// `powerz --transport sim [--trace] firmware probe [--json]` with `env`;
/// returns (stdout, stderr). Must exit 0.
fn sim_probe(json: bool, trace: bool, env: &[(&str, &str)]) -> (String, String) {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_CLOCK", "fake");
    for (k, v) in env {
        c.env(k, v);
    }
    c.timeout(Duration::from_secs(60));
    c.args(["--transport", "sim"]);
    if trace {
        c.arg("--trace");
    }
    c.args(["firmware", "probe"]);
    if json {
        c.arg("--json");
    }
    let out = c.assert().success().get_output().clone();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

const OLD: (&str, &str) = ("POWERZ_SIM_MODE", "legacy-bootloader");

fn sim_json(env: &[(&str, &str)]) -> Value {
    let (out, err) = sim_probe(true, false, env);
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("JSON ({e}):\n{out}\nstderr:\n{err}"))
}

/// Frame types of the `> ` (sent) lines of a `--trace` log.
fn sent_types(trace: &str) -> Vec<u8> {
    trace
        .lines()
        .filter_map(|l| l.strip_prefix("> "))
        .map(|hex| {
            let first = hex.split_whitespace().nth(1).expect("a byte");
            u8::from_str_radix(first, 16).expect("hex byte") & 0x7F
        })
        .collect()
}

#[test]
fn qf92wx_sim_old_bootloader_says_the_in_read_timed_out() {
    let (out, _) = sim_probe(false, false, &[OLD]);
    assert!(out.contains("CONNECT: timed out (IN read)"), "{out}");
    assert!(out.contains("HEAD: timed out (IN read)"), "{out}");
}

#[test]
fn qf92wx_sim_old_bootloader_trace_still_names_the_direction() {
    let (out, _) = sim_probe(false, true, &[OLD]);
    assert!(out.contains("CONNECT: timed out (IN read)"), "{out}");
}

#[test]
fn qf92wx_sim_old_bootloader_json_says_the_in_read_timed_out() {
    let v = sim_json(&[OLD]);
    assert_eq!(v["connect_timeout"], json!("in"));
    assert_eq!(v["head_timeout"], json!("in"));
}

#[test]
fn qf92wx_sim_old_bootloader_prints_its_endpoints() {
    let (out, _) = sim_probe(false, false, &[OLD]);
    let endpoints: Vec<&str> = out
        .lines()
        .map(str::trim_start)
        .filter(|l| l.starts_with("endpoint 0x"))
        .collect();
    assert!(
        endpoints.iter().any(|l| l.contains(": IN, ")),
        "an IN endpoint line in:\n{out}"
    );
    assert!(
        endpoints.iter().any(|l| l.contains(": OUT, ")),
        "an OUT endpoint line in:\n{out}"
    );
    assert!(
        endpoints
            .iter()
            .all(|l| l.contains(", max packet ") && l.contains(", interval ")),
        "every endpoint line has max packet and interval:\n{out}"
    );
}

#[test]
fn qf92wx_sim_old_bootloader_prints_strings_1_to_4() {
    let (out, _) = sim_probe(false, false, &[OLD]);
    for i in 1..=4 {
        let prefix = format!("string {i}: ");
        assert!(
            out.lines().any(|l| l.trim_start().starts_with(&prefix)),
            "no {prefix:?} line in:\n{out}"
        );
    }
    assert!(out.contains("string 2: \"POWER-Z KM003C\""), "{out}");
}

#[test]
fn qf92wx_sim_old_bootloader_says_no_variant_answered() {
    let (out, _) = sim_probe(false, false, &[OLD]);
    for name in [
        "no-drain-no-clear-halt",
        "in-before-out",
        "all-in-endpoints",
    ] {
        line_starting(&out, &format!("variant {name}:"));
    }
    assert!(out.contains("First variant to answer: none"), "{out}");
}

#[test]
fn qf92wx_sim_old_bootloader_names_the_variant_told_to_answer() {
    let (out, _) = sim_probe(
        false,
        false,
        &[OLD, ("POWERZ_SIM_PROBE_ANSWER", "all-in-endpoints")],
    );
    assert!(
        out.contains("First variant to answer: all-in-endpoints"),
        "{out}"
    );
}

#[test]
fn qf92wx_sim_old_bootloader_json_lists_the_variants() {
    let v = sim_json(&[OLD, ("POWERZ_SIM_PROBE_ANSWER", "in-before-out")]);
    let names: Vec<Value> = v["variants"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|e| e["name"].clone())
        .collect();
    assert_eq!(
        names,
        vec![
            json!("no-drain-no-clear-halt"),
            json!("in-before-out"),
            json!("all-in-endpoints")
        ]
    );
    assert_eq!(v["variant_answered"], json!("in-before-out"));
}

#[test]
fn qf92wx_sim_old_bootloader_json_has_endpoints_and_strings() {
    let v = sim_json(&[OLD]);
    let eps = v["interfaces"][0]["endpoints"]
        .as_array()
        .map_or(0, Vec::len);
    assert!(eps > 0, "endpoints of interface 0: {v}");
    let keys: Vec<&str> = ["1", "2", "3", "4"]
        .into_iter()
        .filter(|k| v["strings"].get(*k).is_some())
        .collect();
    assert_eq!(keys, vec!["1", "2", "3", "4"], "{v}");
}

#[test]
fn qf92wx_sim_trace_variants_send_only_connect_and_head() {
    let (_, trace) = sim_probe(false, true, &[OLD]);
    let types = sent_types(&trace);
    // The standard CONNECT, its retry and HEAD, then CONNECT and HEAD in
    // each of the three variants.
    assert!(types.len() >= 9, "sent {types:02x?}:\n{trace}");
    assert!(
        types.iter().all(|t| *t == CONNECT || *t == HEAD),
        "sent {types:02x?}:\n{trace}"
    );
}

#[test]
fn qf92wx_sim_application_mode_timeout_gets_no_variants() {
    let v = sim_json(&[("POWERZ_SIM_FAULT", "timeout")]);
    assert_eq!(v["connect"], json!("timed out"));
    assert_eq!(v["variants"], json!([]));
    assert_eq!(v["variant_answered"], Value::Null);
}

#[test]
fn qf92wx_sim_application_mode_text_prints_no_variant_lines() {
    let (out, _) = sim_probe(false, false, &[("POWERZ_SIM_FAULT", "timeout")]);
    assert!(out.contains("CONNECT: timed out"), "{out}");
    assert!(
        !out.lines().any(|l| l.trim_start().starts_with("variant ")),
        "{out}"
    );
    // Endpoints still print in application mode.
    assert!(
        out.lines()
            .any(|l| l.trim_start().starts_with("endpoint 0x")),
        "{out}"
    );
}
