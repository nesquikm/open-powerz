//! `powerz pd --save FILE` and `powerz pd decode FILE` against the demo
//! device (FR TGE83E, AC .1, .3, .4): a live capture is saved as a pdcap
//! file while it streams, and the saved file decodes to the same lines the
//! live capture printed, without touching any meter.
//!
//! Command shapes pinned here:
//!
//! - `powerz pd [--json] --save FILE`: the live capture as before on stdout,
//!   and every status sample and record written to FILE (pdcap JSON Lines).
//! - `powerz pd decode [--json] FILE`: the records of FILE in the same text
//!   or JSON Lines format as the live `pd`; no device is opened (no frame is
//!   sent). A damaged file prints the records before the damage and names the
//!   line where reading stopped on stderr.
//!
//! Test-only knobs (see `e2e_sim.rs`): `POWERZ_SIM_CLOCK=fake`,
//! `POWERZ_SIM_CTRL_C_AFTER=N`. No hardware is touched.

use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use powerz_sim::SIM_SERIAL;
use serde_json::Value;

const ENV_KNOBS: [&str; 3] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
];

/// `powerz --transport sim ...` on the simulated clock, Ctrl-C after `polls`
/// GET_DATA requests.
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

/// A fresh path for `name` under this test target's scratch directory.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("pdcap-cli");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

fn stdout_of(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stdout).into_owned()
}

fn stderr_of(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stderr).into_owned()
}

fn json_lines(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l:?}: {e}")))
        .collect()
}

/// Save a 2-minute demo capture to `name`; returns the live JSON output.
fn save_capture(name: &str) -> (PathBuf, String) {
    let path = scratch(name);
    let a = sim(24_000)
        .args(["pd", "--json", "--save"])
        .arg(&path)
        .assert()
        .success();
    (path, stdout_of(&a))
}

/// Every string anywhere in `v`.
fn strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        Value::Object(o) => o.iter().for_each(|(k, x)| {
            out.push(k.clone());
            strings(x, out);
        }),
        _ => {}
    }
}

// ----------------------------------------------- AC-TGE83E.4: pd --save

#[test]
fn pd_save_writes_a_pdcap_file_with_a_header_status_samples_and_records() {
    let (path, live) = save_capture("save.pdcap");
    let text = std::fs::read_to_string(&path).expect("the pdcap file was written");
    let lines = json_lines(&text);
    assert!(lines.len() > 2, "header and entries: {}", lines.len());

    // AC-TGE83E.1: the header line.
    let h = &lines[0];
    assert_eq!(h["format"], "pdcap");
    assert_eq!(h["format_version"], 1);
    assert_eq!(h["app_version"], env!("CARGO_PKG_VERSION"));
    assert!(
        h["model"].as_str().is_some_and(|m| m.contains("KM00")),
        "meter model: {h}"
    );
    assert!(
        h["firmware_version"]
            .as_str()
            .is_some_and(|f| !f.is_empty()),
        "firmware version: {h}"
    );

    let kinds: Vec<&str> = lines[1..]
        .iter()
        .map(|l| l["kind"].as_str().unwrap_or(""))
        .collect();
    assert!(kinds.contains(&"status"), "status samples are saved");
    let records = kinds.iter().filter(|k| **k == "record").count();
    assert_eq!(
        records,
        json_lines(&live).len(),
        "every record printed live is saved"
    );
    for l in &lines[1..] {
        assert!(l["session_ms"].is_u64(), "{l}");
        assert!(l["raw"].as_str().is_some_and(|r| !r.is_empty()), "{l}");
    }
}

#[test]
fn pd_save_never_writes_the_meters_serial_number() {
    let (path, _) = save_capture("no-serial.pdcap");
    let text = std::fs::read_to_string(&path).expect("the pdcap file was written");
    for line in json_lines(&text) {
        let mut all = Vec::new();
        strings(&line, &mut all);
        for s in all {
            assert!(!s.to_lowercase().contains("serial"), "{s:?} in {line}");
            assert_ne!(s, SIM_SERIAL, "the serial number in {line}");
        }
    }
}

// ---------------------------------------------- AC-TGE83E.4: pd decode

#[test]
fn pd_decode_json_prints_the_same_lines_as_the_live_capture() {
    let (path, live) = save_capture("decode-json.pdcap");
    let a = sim(0)
        .args(["--trace", "pd", "decode", "--json"])
        .arg(&path)
        .assert()
        .success();
    assert_eq!(json_lines(&stdout_of(&a)), json_lines(&live));
    let err = stderr_of(&a);
    assert!(
        !err.lines().any(|l| l.starts_with('>')),
        "decode sends no frame to any device:\n{err}"
    );
}

#[test]
fn pd_decode_text_prints_the_live_text_lines() {
    let path = scratch("decode-text.pdcap");
    let live = sim(24_000)
        .args(["pd", "--save"])
        .arg(&path)
        .assert()
        .success();
    let live = stdout_of(&live);
    let a = sim(0).args(["pd", "decode"]).arg(&path).assert().success();
    assert_eq!(stdout_of(&a), live);
}

// ---------------------------------- AC-TGE83E.3: a damaged file in decode

#[test]
fn pd_decode_of_a_cut_file_prints_the_records_before_the_cut_and_names_the_line() {
    let (path, live) = save_capture("cut.pdcap");
    let text = std::fs::read_to_string(&path).expect("saved");
    let lines: Vec<&str> = text.lines().collect();
    // Keep the header and 10 whole lines, then half of line 12.
    let keep = 11;
    let half = &lines[keep][..lines[keep].len() / 2];
    let cut_text = format!("{}\n{half}", lines[..keep].join("\n"));
    let cut_path = scratch("cut-short.pdcap");
    std::fs::write(&cut_path, cut_text).expect("cut file written");

    let records_kept = lines[1..keep]
        .iter()
        .filter(|l| l.contains("\"kind\":\"record\""))
        .count();
    let a = sim(0)
        .args(["pd", "decode", "--json"])
        .arg(&cut_path)
        .assert();
    let out = json_lines(&stdout_of(&a));
    assert_eq!(out, json_lines(&live)[..records_kept].to_vec());
    let err = stderr_of(&a);
    assert!(err.contains("line 12"), "the cut line is named:\n{err}");
}

#[test]
fn pd_decode_of_a_missing_file_fails_with_a_message() {
    let path = scratch("missing.pdcap");
    let a = sim(0).args(["pd", "decode"]).arg(&path).assert().failure();
    assert!(stderr_of(&a).contains("missing.pdcap"), "{}", stderr_of(&a));
}

// -------------------------------- AC-4PJRVK.6: a file error is not a device error

/// A file error fails with a code of its own kind: 5 means the meter failed,
/// and `pd decode` opens no meter.
fn assert_file_error(a: &assert_cmd::assert::Assert, name: &str, what: &str) {
    let code = a.get_output().status.code();
    assert_ne!(code, Some(0), "a file error fails");
    assert_ne!(code, Some(5), "a file error is no device error");
    let err = stderr_of(a);
    assert!(err.contains(name), "the file is named:\n{err}");
    assert!(err.contains(what), "the reason is given:\n{err}");
    assert_eq!(err.lines().count(), 1, "one clear line:\n{err}");
}

#[test]
fn pd_decode_of_a_file_that_is_no_capture_is_a_file_error() {
    let path = scratch("not-a-capture.pdcap");
    std::fs::write(&path, "not a pdcap file\n").expect("written");
    let a = sim(0).args(["pd", "decode"]).arg(&path).assert();
    assert_file_error(&a, "not-a-capture.pdcap", "line 1");
}

#[test]
fn pd_decode_of_a_cut_file_is_a_file_error() {
    let (path, _) = save_capture("cut-error.pdcap");
    let text = std::fs::read_to_string(&path).expect("saved");
    let first = text.lines().next().expect("a header");
    let cut = scratch("cut-error-short.pdcap");
    std::fs::write(&cut, format!("{first}\n{{\"kind\":")).expect("written");
    let a = sim(0).args(["pd", "decode"]).arg(&cut).assert();
    assert_file_error(&a, "cut-error-short.pdcap", "line 2");
}

#[test]
fn pd_decode_of_a_folder_is_a_file_error() {
    let dir = scratch("a-folder.pdcap");
    std::fs::create_dir_all(&dir).expect("a folder");
    let a = sim(0).args(["pd", "decode"]).arg(&dir).assert();
    assert_file_error(&a, "a-folder.pdcap", "");
}
