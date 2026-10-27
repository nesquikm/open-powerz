//! Command-function tests for `powerz` (FR STQEA1, AC .1–.8) against
//! scripted `MockTransport` sessions and in-memory sinks.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::Parser;
use common::*;
use powerz_cli::commands::info_in_mode;
use powerz_cli::record::CSV_HEADER;
use powerz_cli::trace::TraceTransport;
use powerz_cli::{
    Cli, CliError, Command, StreamOptions, TransportKind, WatchOptions, info, list, read,
    select_device, stream, validate, watch,
};
use powerz_protocol::decode::Rate;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::session::{self, Gap, Streamer, memory_read_request};
use powerz_protocol::transport::discovery::Model;
use powerz_protocol::transport::mock::{MockTransport, Step};
use powerz_protocol::transport::{self, Transport, permission_error};
use serde_json::Value;

fn two_devices() -> Vec<powerz_protocol::transport::discovery::DeviceEntry> {
    vec![
        entry(Model::Km003c, 0x0063, Some(SYNTH_SERIAL), "3", 42),
        entry(Model::Km002c, 0x0061, Some("654321"), "5", 17),
    ]
}

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).unwrap_or_else(|e| panic!("{args:?}: {e}"))
}

// ---- AC-STQEA1.1: list ----

#[test]
fn list_prints_one_line_per_meter_with_model_serial_bus_address() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    list(&two_devices(), false, &mut out, &mut err).unwrap();
    let out = text(&out);
    let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 2, "output:\n{out}");
    assert!(lines[0].contains("KM003C"), "{}", lines[0]);
    assert!(lines[0].contains(SYNTH_SERIAL), "{}", lines[0]);
    assert!(
        lines[0].contains('3') && lines[0].contains("42"),
        "{}",
        lines[0]
    );
    assert!(lines[1].contains("KM002C"), "{}", lines[1]);
    assert!(lines[1].contains("654321"), "{}", lines[1]);
    assert!(
        lines[1].contains('5') && lines[1].contains("17"),
        "{}",
        lines[1]
    );
    assert!(text(&err).is_empty(), "stderr: {}", text(&err));
}

#[test]
fn list_with_no_meter_reports_on_stderr_and_succeeds() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let r = list(&[], false, &mut out, &mut err);
    assert_eq!(r, Ok(()));
    assert!(
        text(&err).contains("no POWER-Z devices found"),
        "stderr: {}",
        text(&err)
    );
    assert!(text(&out).trim().is_empty(), "stdout: {}", text(&out));
}

#[test]
fn list_json_is_array_of_model_serial_bus_address() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    list(&two_devices(), true, &mut out, &mut err).unwrap();
    let v: Value = serde_json::from_slice(&out).expect("valid JSON");
    let arr = v.as_array().expect("JSON array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["model"], "KM003C");
    assert_eq!(arr[0]["serial"], SYNTH_SERIAL);
    assert_eq!(arr[0]["bus"], "3");
    assert_eq!(arr[0]["address"], 42);
    assert_eq!(arr[1]["model"], "KM002C");
    assert_eq!(arr[1]["serial"], "654321");
    assert_eq!(arr[1]["bus"], "5");
    assert_eq!(arr[1]["address"], 17);
}

#[test]
fn list_json_with_no_meter_is_empty_array() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    list(&[], true, &mut out, &mut err).unwrap();
    let v: Value = serde_json::from_slice(&out).expect("valid JSON");
    assert_eq!(v, Value::Array(vec![]));
}

// ---- AC-STQEA1.2: info ----

#[test]
fn info_prints_model_versions_serial_and_uid() {
    let mut s = session(MockTransport::new(
        [device_info_steps(1), read_firmware_status_steps(5)].concat(),
    ));
    let mut out = Vec::new();
    info(&mut s, false, &mut out).unwrap();
    let out = text(&out);
    for want in [SYNTH_MODEL, SYNTH_FW, SYNTH_HW, SYNTH_SERIAL, SYNTH_UID_HEX] {
        assert!(out.contains(want), "missing {want} in:\n{out}");
    }
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn info_json_is_one_object() {
    let mut s = session(MockTransport::new(
        [device_info_steps(1), read_firmware_status_steps(5)].concat(),
    ));
    let mut out = Vec::new();
    info(&mut s, true, &mut out).unwrap();
    let v: Value = serde_json::from_slice(&out).expect("valid JSON");
    assert!(v.is_object(), "{v}");
    assert_eq!(v["model"], SYNTH_MODEL);
    assert_eq!(v["firmware_version"], SYNTH_FW);
    assert_eq!(v["hardware_version"], SYNTH_HW);
    assert_eq!(v["serial"], SYNTH_SERIAL);
    assert_eq!(v["uid"], SYNTH_UID_HEX);
    assert_eq!(v["update_required"], false, "HEAD answered field 0: {v}");
    assert_eq!(v["firmware_problems"], serde_json::json!([]), "{v}");
    assert!(
        v.get("status_problems").is_none(),
        "renamed to firmware_problems: {v}"
    );
}

// ---- AC-V9SK52.2 / .4: info by mode ----

/// `info_in_mode` on `script`; the call's result, its stdout text and the
/// frames it sent.
fn info_mode(
    mode: DeviceMode,
    json: bool,
    script: Vec<Step>,
) -> (Result<(), CliError>, String, Vec<Vec<u8>>) {
    let mut s = session(MockTransport::new(script));
    let mut out = Vec::new();
    let r = info_in_mode(&mut s, mode, json, &mut out);
    (r, text(&out), s.transport().written().to_vec())
}

#[test]
fn info_outside_application_mode_reports_the_mode_and_sends_nothing() {
    for mode in [DeviceMode::Bootloader, DeviceMode::Unknown] {
        let (r, out, sent) = info_mode(mode, false, vec![]);
        assert!(r.is_ok(), "{mode:?}: {r:?}");
        let line = out
            .lines()
            .find(|l| l.starts_with("Mode:"))
            .unwrap_or_else(|| panic!("{mode:?}: no Mode line in:\n{out}"));
        assert!(line.contains(mode.name()), "{mode:?}: {out}");
        assert_eq!(
            sent,
            Vec::<Vec<u8>>::new(),
            "{mode:?}: no HEAD, no memory read"
        );

        let (r, out, sent) = info_mode(mode, true, vec![]);
        assert!(r.is_ok(), "{mode:?} --json: {r:?}");
        let v: Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["mode"], mode.name(), "{v}");
        assert_eq!(v["firmware_problems"], serde_json::json!([]), "{v}");
        assert!(v.get("status_problems").is_none(), "{v}");
        assert_eq!(sent, Vec::<Vec<u8>>::new(), "{mode:?} --json sends nothing");
    }
}

#[test]
fn info_in_application_mode_sends_no_connect_of_its_own() {
    for json in [false, true] {
        let (r, out, sent) = info_mode(
            DeviceMode::Application,
            json,
            [device_info_steps(1), read_firmware_status_steps(5)].concat(),
        );
        assert!(r.is_ok(), "json={json}: {r:?}");
        assert!(out.contains(SYNTH_FW), "{out}");
        let connects = sent.iter().filter(|f| f.first() == Some(&0x02)).count();
        assert_eq!(connects, 0, "the session is already connected: {sent:02x?}");
        let heads = sent.iter().filter(|f| f.first() == Some(&0x40)).count();
        assert_eq!(heads, 1, "one HEAD: {sent:02x?}");
    }
}

/// HEAD at request id 7 (after the device-info reads and both info
/// blocks) fails as `head`; the recovery CONNECT (id 8) that follows a
/// failed HEAD is accepted.
fn info_with_bad_head(json: bool, head: Step) -> (Result<(), CliError>, String) {
    let (r, out, _) = info_mode(DeviceMode::Application, json, bad_head_script(head));
    (r, out)
}

/// Device info (ids 1..4), both info blocks (ids 5, 6), the failing `head`
/// (id 7), then the recovery CONNECT (id 8) the shared degrade helper sends.
fn bad_head_script(head: Step) -> Vec<Step> {
    [
        device_info_steps(1),
        memread_steps(5, HW_INFO_ADDR, &info_block(SYNTH_MODEL, SYNTH_HW)),
        memread_steps(6, FW_INFO_ADDR, &info_block(SYNTH_MODEL, SYNTH_FW)),
        vec![
            head,
            Step::transact(powerz_protocol::frame::connect(8).to_vec(), accept(8)),
        ],
    ]
    .concat()
}

fn bad_heads() -> Vec<(&'static str, Step)> {
    let req = powerz_protocol::decode::head::request(7).to_vec();
    vec![
        (
            "timeout",
            Step::transact_err(req.clone(), transport::Error::Timeout),
        ),
        // Header only: too short for the old-firmware field.
        (
            "undecodable",
            Step::transact(req, vec![0x40, 7, 0x00, 0x01]),
        ),
    ]
}

#[test]
fn info_degrades_when_head_times_out_or_is_undecodable() {
    for (what, head) in bad_heads() {
        let (r, out) = info_with_bad_head(false, head);
        assert!(r.is_ok(), "{what}: info still succeeds: {r:?}");
        for want in [SYNTH_MODEL, SYNTH_FW, SYNTH_HW, SYNTH_SERIAL] {
            assert!(out.contains(want), "{what}: missing {want} in:\n{out}");
        }
        assert!(
            !out.to_lowercase().contains("update required"),
            "{what}: no update-required warning:\n{out}"
        );
    }
    for (what, head) in bad_heads() {
        let (r, out) = info_with_bad_head(true, head);
        assert!(r.is_ok(), "{what} --json: {r:?}");
        let v: Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["model"], SYNTH_MODEL, "{v}");
        assert_eq!(v["firmware_version"], SYNTH_FW, "{v}");
        assert_eq!(v["hardware_version"], SYNTH_HW, "{v}");
        assert_eq!(
            v["update_required"],
            Value::Null,
            "{what}: unknown, not false: {v}"
        );
        let codes = v["warnings"].as_array().cloned().unwrap_or_default();
        assert!(
            !codes.contains(&serde_json::json!("update_required")),
            "{what}: {v}"
        );
    }
}

#[test]
fn info_reconnects_once_after_a_failed_head_before_continuing() {
    for json in [false, true] {
        for (what, head) in bad_heads() {
            let mut s = session(MockTransport::new(bad_head_script(head)));
            let mut out = Vec::new();
            let r = info_in_mode(&mut s, DeviceMode::Application, json, &mut out);
            assert!(r.is_ok(), "{what} json={json}: {r:?}");
            let sent = s.transport().written();
            let head_at = sent
                .iter()
                .position(|f| f.first() == Some(&0x40))
                .unwrap_or_else(|| panic!("{what}: no HEAD: {sent:02x?}"));
            let after: Vec<u8> = sent[head_at + 1..].iter().map(|f| f[0]).collect();
            assert_eq!(
                after,
                vec![0x02],
                "{what} json={json}: one recovery CONNECT after the failed HEAD"
            );
            assert_eq!(
                s.transport().remaining(),
                0,
                "{what} json={json}: the recovery CONNECT was not sent"
            );
        }
    }
}

// ---- AC-V9SK52.2 / .3: a failed HEAD keeps the dated blocks (real-meter bug) ----

/// Lowercase with every space removed, so a hex dump matches however it
/// is spaced.
fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

/// A 2.0.6 / 2.5 meter (dated `20260101`): device info (ids 1..4), both
/// info blocks (ids 5, 6), HEAD (id 7) answered with REJECT, then the
/// recovery CONNECT (id 8).
fn rejected_head_script() -> Vec<Step> {
    let boot = info_block(SYNTH_MODEL, "2.5");
    let app = info_block(SYNTH_MODEL, "2.0.6");
    [
        memread_steps(1, HW_INFO_ADDR, &boot),
        memread_steps(2, FW_INFO_ADDR, &app),
        memread_steps(3, OTP_ADDR, &otp_block()),
        memread_steps(4, UID_ADDR, &SYNTH_UID),
        memread_steps(5, HW_INFO_ADDR, &boot),
        memread_steps(6, FW_INFO_ADDR, &app),
        vec![
            Step::transact(
                powerz_protocol::decode::head::request(7).to_vec(),
                vec![0x06, 0x07, 0x00, 0x00],
            ),
            Step::transact(powerz_protocol::frame::connect(8).to_vec(), accept(8)),
        ],
    ]
    .concat()
}

fn line_of<'a>(out: &'a str, label: &str) -> &'a str {
    out.lines()
        .find(|l| l.starts_with(label))
        .unwrap_or_else(|| panic!("no `{label}` line in:\n{out}"))
}

#[test]
fn info_with_a_rejected_head_still_prints_dated_versions_and_the_outdated_warning() {
    let (r, out, _) = info_mode(DeviceMode::Application, false, rejected_head_script());
    assert!(r.is_ok(), "{r:?}");
    let fw = line_of(&out, "Firmware:");
    assert!(
        fw.contains("2.0.6 (20260101)"),
        "dated firmware line: {out}"
    );
    let boot = line_of(&out, "Bootloader:");
    assert!(
        boot.contains("2.5 (20260101)"),
        "dated bootloader line: {out}"
    );
    assert!(
        out.lines()
            .any(|l| l.starts_with("Warning:") && l.contains("2.0.7")),
        "the outdated warning: {out}"
    );
    assert!(
        !out.to_lowercase().contains("update required"),
        "no update-required warning: {out}"
    );
}

#[test]
fn info_with_a_rejected_head_says_the_update_check_is_unavailable_and_why() {
    let (_, out, _) = info_mode(DeviceMode::Application, false, rejected_head_script());
    let check = line_of(&out, "Update check:");
    assert!(
        check.starts_with("Update check: unavailable ("),
        "{check:?}"
    );
    assert!(check.ends_with(')'), "{check:?}");
    assert!(
        check.contains("the meter sent REJECT"),
        "the reason: {check:?}"
    );
    assert!(
        squash(check).contains("06070000"),
        "the raw reply: {check:?}"
    );
}

#[test]
fn info_json_with_a_rejected_head_keeps_both_blocks_and_lists_the_problem() {
    let (r, out, _) = info_mode(DeviceMode::Application, true, rejected_head_script());
    assert!(r.is_ok(), "{r:?}");
    let v: Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["application"]["model"], SYNTH_MODEL, "{v}");
    assert_eq!(v["application"]["version"], "2.0.6", "{v}");
    assert_eq!(v["application"]["date"], "20260101", "{v}");
    assert_eq!(v["bootloader"]["version"], "2.5", "{v}");
    assert_eq!(v["bootloader"]["date"], "20260101", "{v}");
    assert_eq!(v["update_required"], Value::Null, "unknown, not false: {v}");
    assert_eq!(v["warnings"], serde_json::json!(["outdated"]), "{v}");
    assert!(v.get("status_problems").is_none(), "{v}");
    let problems = v["firmware_problems"]
        .as_array()
        .unwrap_or_else(|| panic!("firmware_problems array: {v}"));
    assert!(
        problems.iter().all(Value::is_string),
        "each problem is its text: {v}"
    );
    let head = problems
        .iter()
        .filter_map(Value::as_str)
        .find(|p| p.contains("HEAD"))
        .unwrap_or_else(|| panic!("a HEAD problem: {v}"));
    assert!(head.contains("the meter sent REJECT"), "{head:?}");
    assert!(squash(head).contains("06070000"), "{head:?}");
}

#[test]
fn info_with_a_rejected_head_reads_the_blocks_before_head_and_reconnects_once() {
    let (_, _, sent) = info_mode(DeviceMode::Application, true, rejected_head_script());
    let types: Vec<u8> = sent.iter().map(|f| f[0]).collect();
    assert_eq!(
        types,
        vec![0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x40, 0x02],
        "device info, both blocks, HEAD, one recovery CONNECT"
    );
}

// ---- AC-V9SK52.4: every firmware-status problem is shown in text ----

/// Device info (ids 1..4), the bootloader block (id 5) or its timeout,
/// the application block (id 6, 0x4420) timing out, one recovery CONNECT
/// per failed step, then HEAD answered or rejected.
fn bad_block_script(boot_fails: bool, head_rejected: bool) -> Vec<Step> {
    let timeout = |id: u8, addr: u32| {
        Step::transact_err(memory_read_request(id, addr, 64), transport::Error::Timeout)
    };
    let connect = |id: u8| Step::transact(powerz_protocol::frame::connect(id).to_vec(), accept(id));
    let mut steps = device_info_steps(1);
    if boot_fails {
        steps.extend([timeout(5, HW_INFO_ADDR), connect(6)]);
    } else {
        steps.extend(memread_steps(
            5,
            HW_INFO_ADDR,
            &info_block(SYNTH_MODEL, SYNTH_HW),
        ));
    }
    let app_id = if boot_fails { 7 } else { 6 };
    steps.extend([timeout(app_id, FW_INFO_ADDR), connect(app_id + 1)]);
    let head_id = app_id + 2;
    let head_req = powerz_protocol::decode::head::request(head_id).to_vec();
    if head_rejected {
        steps.extend([
            Step::transact(head_req, vec![0x06, head_id, 0x00, 0x00]),
            connect(head_id + 1),
        ]);
    } else {
        steps.push(Step::transact(
            head_req,
            vec![0x40, head_id, 0x00, 0x01, 0, 0, 0, 0],
        ));
    }
    steps
}

fn status_lines(out: &str) -> Vec<&str> {
    out.lines()
        .filter(|l| l.starts_with("Firmware status: "))
        .collect()
}

#[test]
fn info_with_an_unread_application_block_says_why_the_version_is_a_fallback() {
    let (r, out, _) = info_mode(
        DeviceMode::Application,
        false,
        bad_block_script(false, false),
    );
    assert!(r.is_ok(), "{r:?}");
    assert!(
        line_of(&out, "Firmware:").contains(SYNTH_FW),
        "the device-info fallback: {out}"
    );
    let lines = status_lines(&out);
    assert_eq!(lines.len(), 1, "one problem, one line:\n{out}");
    let line = lines[0].to_lowercase();
    assert!(line.contains("application"), "{line:?}");
    assert!(line.contains("0x4420"), "{line:?}");
    assert!(line.contains("not read"), "{line:?}");
    assert!(
        !out.lines().any(|l| l.starts_with("Update check:")),
        "HEAD was read: {out}"
    );
}

#[test]
fn info_prints_each_block_problem_once_and_keeps_head_on_the_update_check_line() {
    let (r, out, _) = info_mode(DeviceMode::Application, false, bad_block_script(true, true));
    assert!(r.is_ok(), "{r:?}");
    let lines = status_lines(&out);
    assert_eq!(lines.len(), 2, "two block problems:\n{out}");
    assert!(
        lines[0].contains("bootloader") && lines[0].contains("0x420"),
        "{out}"
    );
    assert!(
        lines[1].contains("application") && lines[1].contains("0x4420"),
        "{out}"
    );
    assert!(
        lines.iter().all(|l| !l.contains("HEAD")),
        "the HEAD problem stays on its own line: {out}"
    );
    let check = line_of(&out, "Update check:");
    assert!(
        check.starts_with("Update check: unavailable ("),
        "{check:?}"
    );
    assert!(check.to_lowercase().contains("reject"), "{check:?}");
}

// ---- AC-STQEA1.3: read ----

/// The line of `out` holding `label` (any of the spellings) contains `value`.
fn assert_labelled(out: &str, labels: &[&str], value: &str) {
    let line = out
        .lines()
        .find(|l| labels.iter().any(|lab| l.contains(lab)))
        .unwrap_or_else(|| panic!("no line with {labels:?} in:\n{out}"));
    assert!(
        line.contains(value),
        "{labels:?}: expected {value} in {line:?}"
    );
}

#[test]
fn read_prints_every_value_with_units() {
    let mut s = session(MockTransport::new(vec![adc_step(1, ADC_A)]));
    let mut out = Vec::new();
    read(&mut s, false, &mut out).unwrap();
    let out = text(&out);
    assert_labelled(&out, &["VBUS"], "5.123");
    assert_labelled(&out, &["IBUS"], "-2.000");
    assert_labelled(&out, &["ower"], "-10.246");
    assert_labelled(&out, &["emp"], "25.0");
    assert_labelled(&out, &["D+"], "0.600");
    assert_labelled(&out, &["D-", "D−"], "0.400");
    assert_labelled(&out, &["CC1"], "1.650");
    assert_labelled(&out, &["CC2"], "0.123");
    for unit in ["V", "A", "W", "°C"] {
        assert!(out.contains(unit), "missing unit {unit} in:\n{out}");
    }
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn read_json_has_si_number_fields() {
    let mut s = session(MockTransport::new(vec![adc_step(1, ADC_A)]));
    let mut out = Vec::new();
    read(&mut s, true, &mut out).unwrap();
    let v: Value = serde_json::from_slice(&out).expect("valid JSON");
    let num = |k: &str| {
        v[k].as_f64()
            .unwrap_or_else(|| panic!("{k} not a number: {v}"))
    };
    assert!(approx(num("vbus_v"), 5.123), "{v}");
    assert!(approx(num("ibus_a"), -2.0), "{v}");
    assert!(approx(num("power_w"), -10.246), "{v}");
    assert!(approx(num("temp_c"), 25.0), "{v}");
    assert!(approx(num("dp_v"), 0.6), "{v}");
    assert!(approx(num("dm_v"), 0.4), "{v}");
    assert!(approx(num("cc1_v"), 1.65), "{v}");
    assert!(approx(num("cc2_v"), 0.1234), "{v}");
}

#[test]
fn read_json_temp_is_null_when_invalid() {
    let adc = Adc {
        temp_raw: i16::MIN,
        ..ADC_A
    };
    let mut s = session(MockTransport::new(vec![adc_step(1, adc)]));
    let mut out = Vec::new();
    read(&mut s, true, &mut out).unwrap();
    let v: Value = serde_json::from_slice(&out).expect("valid JSON");
    assert!(v.get("temp_c").is_some(), "temp_c must be present: {v}");
    assert!(v["temp_c"].is_null(), "{v}");
    assert!(approx(v["vbus_v"].as_f64().unwrap(), 5.123));
}

#[test]
fn read_shows_temperature_with_one_decimal() {
    // 3910 / 128 = 30.546875 °C.
    let adc = Adc {
        temp_raw: 3910,
        ..ADC_A
    };
    let mut s = session(MockTransport::new(vec![adc_step(1, adc)]));
    let mut out = Vec::new();
    read(&mut s, false, &mut out).unwrap();
    let out = text(&out);
    let line = out.lines().find(|l| l.contains("Temp")).unwrap();
    assert_eq!(line.trim_end(), "Temp:   30.5 °C", "in:\n{out}");
}

#[test]
fn read_json_numbers_are_plain_decimals() {
    // 1 µA: a float serializer would print -1e-6.
    let adc = Adc {
        ibus_ua: -1,
        ..ADC_A
    };
    let mut s = session(MockTransport::new(vec![adc_step(1, adc)]));
    let mut out = Vec::new();
    read(&mut s, true, &mut out).unwrap();
    let raw = text(&out);
    assert!(
        !["e-", "e+", "E-", "E+"].iter().any(|x| raw.contains(x)),
        "exponent in:\n{raw}"
    );
    assert!(raw.contains("\"ibus_a\": -0.000001"), "{raw}");
    assert!(raw.contains("\"temp_c\": 25.0"), "{raw}");
    let v: Value = serde_json::from_str(&raw).expect("valid JSON");
    assert!(approx(v["ibus_a"].as_f64().unwrap(), -0.000001), "{v}");
}

// ---- AC-STQEA1.4: watch ----

/// Run `watch` with three scripted snapshots; `sleep` sets the stop flag on
/// its third call (Ctrl-C). Returns (result, output, sleeps).
fn run_watch(tty: bool) -> (Result<(), CliError>, String, Vec<Duration>, usize) {
    let snaps = [
        ADC_A,
        Adc {
            vbus_uv: 9_001_000,
            ..ADC_A
        },
        Adc {
            vbus_uv: 12_002_000,
            ..ADC_A
        },
    ];
    let script = snaps
        .iter()
        .enumerate()
        .map(|(i, a)| adc_step(i as u8 + 1, *a))
        .collect();
    let mut s = session(MockTransport::new(script));
    let stop = AtomicBool::new(false);
    let mut sleeps = Vec::new();
    let mut out = Vec::new();
    let opts = WatchOptions {
        interval: Duration::from_millis(250),
        tty,
    };
    let r = watch(
        &mut s,
        &opts,
        &stop,
        &mut |d| {
            sleeps.push(d);
            if sleeps.len() == 3 {
                stop.store(true, Ordering::SeqCst);
            }
        },
        &mut out,
    );
    let remaining = s.transport().remaining();
    (r, text(&out), sleeps, remaining)
}

#[test]
fn watch_refreshes_every_interval_until_stopped() {
    let (r, out, sleeps, remaining) = run_watch(false);
    assert_eq!(r, Ok(()), "Ctrl-C must end watch successfully");
    assert_eq!(remaining, 0, "all three snapshots must be read");
    assert_eq!(sleeps, vec![Duration::from_millis(250); 3]);
    let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        3,
        "one line per refresh when not a tty:\n{out}"
    );
    assert!(lines[0].contains("5.123"), "{}", lines[0]);
    assert!(lines[1].contains("9.001"), "{}", lines[1]);
    assert!(lines[2].contains("12.002"), "{}", lines[2]);
    assert!(!out.contains('\x1b'), "no ANSI escapes off a tty: {out:?}");
}

#[test]
fn watch_on_a_tty_redraws_in_place_with_ansi() {
    let (r, out, _sleeps, remaining) = run_watch(true);
    assert_eq!(r, Ok(()));
    assert_eq!(remaining, 0);
    assert!(
        out.contains("\x1b["),
        "expected ANSI cursor movement: {out:?}"
    );
    assert!(out.contains("12.002"), "{out:?}");
}

#[test]
fn watch_interval_defaults_to_200_and_has_minimum_50() {
    match parse(&["powerz", "watch"]).command {
        Command::Watch { interval, .. } => assert_eq!(interval, 200),
        other => panic!("{other:?}"),
    }
    match parse(&["powerz", "watch", "--interval", "50"]).command {
        Command::Watch { interval, .. } => assert_eq!(interval, 50),
        other => panic!("{other:?}"),
    }
    let e = Cli::try_parse_from(["powerz", "watch", "--interval", "49"]).unwrap_err();
    assert_eq!(e.exit_code(), 2);
}

// ---- AC-STQEA1.5: stream ----

const START_UNIX_MS: u64 = 1_790_000_000_000;
const BASE: u32 = 50_000;

fn ts(offsets: &[u32]) -> Vec<u32> {
    offsets.iter().map(|o| BASE + o).collect()
}

fn csv_rows(csv: &[u8]) -> (String, Vec<Vec<String>>) {
    let text = text(csv);
    let header = text.lines().next().unwrap_or_default().to_owned();
    let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_reader(csv);
    let rows = rdr
        .records()
        .map(|r| {
            r.expect("valid CSV row")
                .iter()
                .map(str::to_owned)
                .collect()
        })
        .collect();
    (header, rows)
}

fn f(s: &str) -> f64 {
    s.parse().unwrap_or_else(|_| panic!("not a number: {s:?}"))
}

/// The scripted snapshot temperature: 31.5 °C (raw 1/128 °C).
const SNAP_TEMP_C: f64 = 31.5;

/// Scripted ADC snapshot (GET_DATA ATT_ADC, id `id`) that `stream` reads
/// after a queue poll whenever >= 500 ms passed since the last one (the
/// first after the first poll), for the TEMP column.
fn snap_step(id: u8) -> Step {
    adc_step(
        id,
        Adc {
            temp_raw: (SNAP_TEMP_C * 128.0) as i16,
            ..ADC_A
        },
    )
}

/// `temp`: the TEMP the row must carry (`None` = empty).
fn check_row(row: &[String], k: u32, temp: Option<f64>) {
    assert_eq!(row.len(), 12, "{row:?}");
    let elapsed_s = f64::from(k) * 0.1;
    assert_eq!(row[0], format!("0:00:00.{:03}", k * 100), "{row:?}");
    assert!(
        (f(&row[1]) - (START_UNIX_MS as f64 / 1000.0 + elapsed_s)).abs() < 1e-3,
        "Unix {row:?}"
    );
    assert!(approx(f(&row[2]), 5.0), "VBUS {row:?}");
    assert!(approx(f(&row[3]), -2.0), "IBUS {row:?}");
    assert!(approx(f(&row[4]), 0.008), "DP {row:?}");
    assert!(approx(f(&row[5]), 0.009), "DM {row:?}");
    assert!(approx(f(&row[6]), 0.805), "CC1 {row:?}");
    assert!(approx(f(&row[7]), 0.007), "CC2 {row:?}");
    match temp {
        None => assert_eq!(row[8], "", "TEMP must be empty: {row:?}"),
        Some(t) => assert!(approx(f(&row[8]), t), "TEMP {row:?}"),
    }
    assert!(
        approx(f(&row[9]), -2.0 * elapsed_s / 3600.0),
        "CHARGE {row:?}"
    );
    assert!(
        approx(f(&row[10]), -10.0 * elapsed_s / 3600.0),
        "ENERGY {row:?}"
    );
    assert!(approx(f(&row[11]), -10.0), "PWR {row:?}");
}

#[test]
fn stream_writes_vendor_csv_until_duration_and_stops_queue() {
    let script = [
        start_steps(Rate::Sps10.code()),
        vec![
            poll_step(4, &ts(&[0, 100])),
            snap_step(5),
            poll_step(6, &ts(&[200, 300])),
            Step::transact(stop_queue(7), accept(7)),
        ],
    ]
    .concat();
    let mut s = session(Meter::new(script));
    let stop = AtomicBool::new(false);
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let opts = StreamOptions {
        rate: Rate::Sps10,
        duration: Some(Duration::from_millis(250)),
        start_unix_ms: START_UNIX_MS,
    };
    let summary = stream(&mut s, &opts, &stop, &mut csv, &mut err).unwrap();

    let (header, rows) = csv_rows(&csv);
    assert_eq!(header, CSV_HEADER);
    assert_eq!(
        header,
        "ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR"
    );
    assert_eq!(rows.len(), 3, "rows: {rows:?}");
    // Rows before the first snapshot have no TEMP; later rows carry the
    // snapshot's temperature.
    for (k, row) in rows.iter().enumerate() {
        check_row(row, k as u32, (k >= 2).then_some(SNAP_TEMP_C));
    }
    assert!(approx(f(&rows[2][8]), SNAP_TEMP_C), "TEMP {:?}", rows[2]);
    assert_eq!(summary.samples, 3);
    assert!(summary.gaps.is_empty(), "{:?}", summary.gaps);
    assert_eq!(s.transport().inner.remaining(), 0, "queue must be stopped");
    assert_eq!(
        s.transport().inner.written().last(),
        Some(&stop_queue(7)),
        "last request must be STOP_QUEUE"
    );
    let err = text(&err);
    assert!(
        err.contains('3') && err.to_lowercase().contains("sample"),
        "stderr: {err}"
    );
}

/// AC-CWWKJ6.4: a row more than 2 s after the latest valid snapshot
/// temperature gets an empty TEMP; an invalid snapshot does not refresh it.
#[test]
fn stream_leaves_temp_empty_more_than_2_s_after_the_last_valid_snapshot() {
    // 10 SPS polls every 100 ms; a snapshot follows polls 0 and 5 (500 ms
    // apart). Only the first is valid, read right after the rows at 0 and
    // 100 ms; the second reads no temperature.
    let script = [
        start_steps(Rate::Sps10.code()),
        vec![
            poll_step(4, &ts(&[0, 100])),
            snap_step(5),
            poll_step(6, &ts(&[200, 1000])),
            poll_step(7, &ts(&[1900])),
            poll_step(8, &ts(&[2600])),
            poll_step(9, &ts(&[3000])),
            poll_step(10, &ts(&[3200])),
            adc_step(
                11,
                Adc {
                    temp_raw: i16::MIN,
                    ..ADC_A
                },
            ),
            poll_step(12, &ts(&[3500])),
            Step::transact(stop_queue(13), accept(13)),
        ],
    ]
    .concat();
    let meter = Meter::new(script).stop_after_polls(7);
    let stop = meter.stop.clone();
    let mut s = session(meter);
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let opts = StreamOptions {
        rate: Rate::Sps10,
        duration: None,
        start_unix_ms: START_UNIX_MS,
    };
    stream(&mut s, &opts, &stop, &mut csv, &mut err).unwrap();
    assert_eq!(
        s.transport().inner.remaining(),
        0,
        "every scripted step ran"
    );
    let (_, rows) = csv_rows(&csv);
    let temps: Vec<&str> = rows.iter().map(|r| r[8].as_str()).collect();
    assert_eq!(rows.len(), 9, "rows: {rows:?}");
    // Rows at 0 and 100 ms come before the snapshot; 200..=1900 ms are
    // within 2 s of it; 2600 ms on are more than 2 s after it.
    assert_eq!(temps[..2], ["", ""], "{temps:?}");
    for (k, t) in temps[2..5].iter().enumerate() {
        assert!(approx(f(t), SNAP_TEMP_C), "row {}: TEMP {t:?}", k + 2);
    }
    assert_eq!(
        temps[5..],
        ["", "", "", ""],
        "TEMP must be empty more than 2 s after the last valid snapshot: {temps:?}"
    );
}

#[test]
fn stream_stops_queue_on_ctrl_c_and_keeps_received_rows() {
    let script = [
        start_steps(Rate::Sps10.code()),
        vec![
            poll_step(4, &ts(&[0, 100, 200])),
            snap_step(5),
            poll_step(6, &ts(&[300, 400])),
            Step::transact(stop_queue(7), accept(7)),
        ],
    ]
    .concat();
    let meter = Meter::new(script).stop_after_polls(2);
    let stop = meter.stop.clone();
    let mut s = session(meter);
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let opts = StreamOptions {
        rate: Rate::Sps10,
        duration: None,
        start_unix_ms: START_UNIX_MS,
    };
    let summary = stream(&mut s, &opts, &stop, &mut csv, &mut err).unwrap();
    let (_, rows) = csv_rows(&csv);
    assert_eq!(rows.len(), 5, "rows: {rows:?}");
    for (k, row) in rows.iter().enumerate() {
        check_row(row, k as u32, (k >= 3).then_some(SNAP_TEMP_C));
    }
    assert_eq!(summary.samples, 5);
    assert_eq!(s.transport().inner.remaining(), 0, "queue must be stopped");
    assert!(text(&err).contains('5'), "stderr: {}", text(&err));
}

#[test]
fn stream_reports_gaps_on_stderr() {
    let script = [
        start_steps(Rate::Sps10.code()),
        vec![
            poll_step(4, &ts(&[0, 100, 200])),
            snap_step(5),
            poll_step(6, &ts(&[300, 600])),
            Step::transact(stop_queue(7), accept(7)),
        ],
    ]
    .concat();
    let meter = Meter::new(script).stop_after_polls(2);
    let stop = meter.stop.clone();
    let mut s = session(meter);
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let opts = StreamOptions {
        rate: Rate::Sps10,
        duration: None,
        start_unix_ms: START_UNIX_MS,
    };
    let summary = stream(&mut s, &opts, &stop, &mut csv, &mut err).unwrap();
    assert_eq!(
        summary.gaps,
        vec![Gap {
            from_ms: BASE + 300,
            to_ms: BASE + 600,
            expected: 3,
            received: 1,
        }]
    );
    assert_eq!(summary.samples, 5);
    let (_, rows) = csv_rows(&csv);
    assert_eq!(rows.last().map(|r| r[0].as_str()), Some("0:00:00.600"));
    assert!(
        text(&err).to_lowercase().contains("gap"),
        "stderr: {}",
        text(&err)
    );
    assert_eq!(s.transport().inner.remaining(), 0);
}

#[test]
fn stream_rate_accepts_only_2_10_50_1000() {
    for rate in ["2", "10", "50", "1000"] {
        parse(&["powerz", "stream", "--rate", rate, "--csv", "out.csv"]);
    }
    for rate in ["0", "7", "100", "fast"] {
        let e = Cli::try_parse_from(["powerz", "stream", "--rate", rate, "--csv", "out.csv"])
            .unwrap_err();
        assert_eq!(e.exit_code(), 2, "rate {rate}");
    }
    match parse(&[
        "powerz",
        "stream",
        "--rate",
        "50",
        "--csv",
        "o.csv",
        "--duration",
        "1.5",
    ])
    .command
    {
        Command::Stream {
            rate,
            csv,
            duration,
        } => {
            assert_eq!(rate, 50);
            assert_eq!(csv, std::path::PathBuf::from("o.csv"));
            assert_eq!(duration, Some(1.5));
        }
        other => panic!("{other:?}"),
    }
}

// ---- AC-STQEA1.6: device and transport selection ----

#[test]
fn several_meters_without_device_is_usage_error_naming_serials() {
    let devices = two_devices();
    let e = select_device(&devices, None).unwrap_err();
    assert_eq!(e.exit_code(), 2, "{e:?}");
    let msg = e.to_string();
    assert!(
        msg.contains(SYNTH_SERIAL) && msg.contains("654321"),
        "{msg}"
    );
}

#[test]
fn device_flag_picks_the_matching_meter() {
    let devices = two_devices();
    assert_eq!(select_device(&devices, Some("654321")), Ok(&devices[1]));
    assert_eq!(select_device(&devices, Some(SYNTH_SERIAL)), Ok(&devices[0]));
}

#[test]
fn single_meter_needs_no_device_flag() {
    let devices = vec![two_devices().remove(0)];
    assert_eq!(select_device(&devices, None), Ok(&devices[0]));
}

#[test]
fn unknown_serial_or_no_meter_is_no_matching_device() {
    let devices = two_devices();
    let e = select_device(&devices, Some("999999")).unwrap_err();
    assert_eq!(e.exit_code(), 3, "{e:?}");
    let e = select_device(&[], None).unwrap_err();
    assert_eq!(e.exit_code(), 3, "{e:?}");
}

#[test]
fn transport_defaults_to_bulk_and_parses_all_three() {
    assert_eq!(parse(&["powerz", "read"]).transport, TransportKind::Bulk);
    for (arg, kind) in [
        ("bulk", TransportKind::Bulk),
        ("cdc", TransportKind::Cdc),
        ("hid", TransportKind::Hid),
    ] {
        assert_eq!(
            parse(&["powerz", "--transport", arg, "read"]).transport,
            kind
        );
    }
    assert_eq!(
        parse(&["powerz", "--device", "654321", "info"])
            .device
            .as_deref(),
        Some("654321")
    );
}

#[test]
fn hid_is_accepted_only_for_read_and_watch() {
    for args in [
        &["powerz", "--transport", "hid", "read"][..],
        &["powerz", "--transport", "hid", "watch"],
        &["powerz", "--transport", "bulk", "info"],
        // CDC carries read/watch only (FW 2.0.6: no memory read over CDC).
        &["powerz", "--transport", "cdc", "read"],
        &["powerz", "info"],
    ] {
        assert_eq!(validate(&parse(args)), Ok(()), "{args:?}");
    }
    for args in [
        &["powerz", "--transport", "hid", "info"][..],
        &[
            "powerz",
            "--transport",
            "cdc",
            "stream",
            "--rate",
            "10",
            "--csv",
            "o.csv",
        ],
        &[
            "powerz",
            "--transport",
            "hid",
            "stream",
            "--rate",
            "10",
            "--csv",
            "o.csv",
        ],
    ] {
        let e = validate(&parse(args)).unwrap_err();
        assert_eq!(e.exit_code(), 2, "{args:?}: {e:?}");
        // The message names the transport that was refused.
        assert!(e.to_string().to_lowercase().contains(args[2]), "{e}");
    }
}

// ---- AC-STQEA1.7: --trace ----

/// `(marker, lowercase hex)` of every non-empty trace line
/// (`<marker> [<len>] <hex bytes>`); the `[len]` prefix must equal the
/// number of bytes on the line.
fn trace_lines(s: &str) -> Vec<(char, String)> {
    s.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut tokens = l.split_whitespace();
            let marker = tokens.next().unwrap().chars().next().unwrap();
            let len: usize = tokens
                .next()
                .and_then(|t| t.strip_prefix('[')?.strip_suffix(']')?.parse().ok())
                .unwrap_or_else(|| panic!("no [len] after the marker: {l}"));
            let bytes: Vec<&str> = tokens.collect();
            assert_eq!(bytes.len(), len, "[len] counts the bytes: {l}");
            (marker, bytes.concat().to_lowercase())
        })
        .collect()
}

#[test]
fn trace_lines_carry_the_byte_length_right_after_the_marker() {
    // The length survives cutting the hex after the first 4 bytes.
    let sink = SharedBuf::default();
    let mut t = TraceTransport::new(
        MockTransport::new(vec![Step::read(vec![0x63; 256])]),
        sink.clone(),
    );
    t.read(Duration::from_millis(1000)).unwrap();
    assert!(
        sink.text().starts_with("< [256] 63 63 63 63 "),
        "{}",
        sink.text()
    );
}

#[test]
fn trace_writes_sent_and_received_frames_as_hex() {
    let req = get_data(1, 0x001);
    let reply = adc_reply(1, ADC_A);
    let sink = SharedBuf::default();
    let mut t = TraceTransport::new(
        MockTransport::new(vec![
            Step::transact(req.clone(), reply.clone()),
            Step::read([0xAB, 0x01]),
        ]),
        sink.clone(),
    );
    assert_eq!(
        t.transact(&req, Duration::from_millis(1000)).unwrap(),
        reply
    );
    assert_eq!(
        t.read(Duration::from_millis(1000)).unwrap(),
        vec![0xAB, 0x01]
    );
    assert_eq!(
        trace_lines(&sink.text()),
        vec![
            ('>', hex_of(&req)),
            ('<', hex_of(&reply)),
            ('<', "ab01".to_owned())
        ]
    );
}

#[test]
fn trace_wraps_a_whole_command() {
    let sink = SharedBuf::default();
    let traced = TraceTransport::new(MockTransport::new(vec![adc_step(1, ADC_A)]), sink.clone());
    let mut s = session(traced);
    let mut out = Vec::new();
    read(&mut s, true, &mut out).unwrap();
    let lines = trace_lines(&sink.text());
    assert_eq!(
        lines,
        vec![
            ('>', "0c010200".to_owned()),
            ('<', hex_of(&adc_reply(1, ADC_A)))
        ]
    );
    // Trace output never leaks into stdout.
    let v: Value = serde_json::from_slice(&out).expect("stdout stays pure JSON");
    assert!(v.is_object());
}

#[test]
fn trace_flag_parses_globally() {
    assert!(parse(&["powerz", "--trace", "read"]).trace);
    assert!(parse(&["powerz", "read", "--trace"]).trace);
    assert!(!parse(&["powerz", "read"]).trace);
}

// ---- AC-STQEA1.8: exit codes ----

#[test]
fn exit_codes_per_error_kind() {
    assert_eq!(CliError::Usage("u".into()).exit_code(), 2);
    assert_eq!(CliError::NoDevice("n".into()).exit_code(), 3);
    assert_eq!(CliError::Permission("p".into()).exit_code(), 4);
    assert_eq!(CliError::Device("d".into()).exit_code(), 5);
}

#[test]
fn errors_carry_their_message() {
    assert!(
        CliError::Usage("bad flag xyz".into())
            .to_string()
            .contains("bad flag xyz")
    );
    assert!(
        CliError::Device("meter went away".into())
            .to_string()
            .contains("meter went away")
    );
}

#[test]
fn session_and_transport_errors_map_to_exit_codes() {
    let t = |e: transport::Error| CliError::from(session::Error::Transport(e)).exit_code();
    assert_eq!(t(transport::Error::NotFound), 3);
    assert_eq!(t(transport::Error::Permission("denied".into())), 4);
    assert_eq!(t(transport::Error::Timeout), 5);
    assert_eq!(t(transport::Error::Disconnected), 5);
    assert_eq!(t(transport::Error::Io("x".into())), 5);
    assert_eq!(
        CliError::from(session::Error::Protocol("p".into())).exit_code(),
        5
    );
    assert_eq!(CliError::from(session::Error::AuthFailed).exit_code(), 5);
    assert_eq!(
        CliError::from(session::Error::Rejected { request_type: 0x0E }).exit_code(),
        5
    );
    assert_eq!(CliError::from(transport::Error::NotFound).exit_code(), 3);
    assert_eq!(
        CliError::from(transport::Error::Permission("p".into())).exit_code(),
        4
    );
    assert_eq!(CliError::from(transport::Error::Timeout).exit_code(), 5);
}

#[test]
fn linux_permission_error_names_the_udev_rule() {
    let e = CliError::from(session::Error::Transport(permission_error(
        "linux",
        "cannot open /dev/bus/usb/001/004",
    )));
    assert_eq!(e.exit_code(), 4);
    assert!(e.to_string().contains("60-powerz.rules"), "{e}");
}

#[test]
fn device_failure_mid_command_is_exit_5() {
    let mut s = session(MockTransport::new(vec![Step::transact_err(
        get_data(1, 0x001),
        transport::Error::Timeout,
    )]));
    let mut out = Vec::new();
    let e = read(&mut s, false, &mut out).unwrap_err();
    assert_eq!(e.exit_code(), 5, "{e:?}");
}

/// Regression (first real-device run): the KM003C answers the first poll
/// after 0x0E with an empty PUT_DATA. The elapsed-time origin must come from
/// the first *record*, not from that empty poll, or every later record at the
/// meter's large uptime looks past `--duration` and nothing is written.
#[test]
fn stream_empty_first_poll_does_not_fix_the_time_origin() {
    let script = [
        start_steps(Rate::Sps10.code()),
        vec![
            Step::transact(get_data(4, 0x002), put_header(4, 1).to_vec()),
            snap_step(5),
            poll_step(6, &ts(&[0, 100, 200])),
            poll_step(7, &ts(&[300, 400])),
            Step::transact(stop_queue(8), accept(8)),
        ],
    ]
    .concat();
    let mut s = session(Meter::new(script));
    let stop = AtomicBool::new(false);
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let opts = StreamOptions {
        rate: Rate::Sps10,
        duration: Some(Duration::from_millis(250)),
        start_unix_ms: START_UNIX_MS,
    };
    let summary = stream(&mut s, &opts, &stop, &mut csv, &mut err).unwrap();
    let (_, rows) = csv_rows(&csv);
    assert_eq!(rows.len(), 3, "rows: {rows:?}");
    assert_eq!(summary.samples, 3);
}

/// A start that is REJECTed writes nothing (not even the header), so a
/// previous CSV is not clobbered by a header-only file.
#[test]
fn stream_start_failure_writes_no_csv() {
    let mut script = start_steps(Rate::Sps10.code());
    script.pop();
    script.push(Step::transact(
        start_queue(3, Rate::Sps10.code()),
        vec![0x06, 3, 0, 0],
    ));
    let mut s = session(Meter::new(script));
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let opts = StreamOptions {
        rate: Rate::Sps10,
        duration: None,
        start_unix_ms: START_UNIX_MS,
    };
    assert!(stream(&mut s, &opts, &AtomicBool::new(false), &mut csv, &mut err).is_err());
    assert!(csv.is_empty(), "csv: {:?}", text(&csv));
}

/// Device time running backwards (meter reboot) ends the stream with a
/// device error instead of writing rows with a 49-day elapsed time.
#[test]
fn stream_backwards_device_time_is_a_device_error() {
    let script = [
        start_steps(Rate::Sps10.code()),
        vec![
            poll_step(4, &ts(&[0, 100])),
            snap_step(5),
            poll_step(6, &[BASE - 5_000]),
            Step::transact(stop_queue(7), accept(7)),
        ],
    ]
    .concat();
    let mut s = session(Meter::new(script));
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let opts = StreamOptions {
        rate: Rate::Sps10,
        duration: None,
        start_unix_ms: START_UNIX_MS,
    };
    let r = stream(&mut s, &opts, &AtomicBool::new(false), &mut csv, &mut err);
    assert!(matches!(r, Err(ref e) if e.exit_code() == 5), "{r:?}");
    assert_eq!(csv_rows(&csv).1.len(), 2);
    assert_eq!(s.transport().inner.remaining(), 0, "queue must be stopped");
}

/// A meter that stops sending data (e.g. it rebooted and answers every poll
/// empty) ends the stream after 5 s of polls instead of hanging forever.
#[test]
fn stream_gives_up_after_five_seconds_without_data() {
    let period = Streamer::poll_period(Rate::Sps10).as_millis() as u64;
    let polls = (5_000 / period) as u8;
    let mut script = start_steps(Rate::Sps10.code());
    let mut id = 4u8;
    for k in 0..=polls {
        script.push(Step::transact(
            get_data(id, 0x002),
            put_header(id, 1).to_vec(),
        ));
        id += 1;
        // A snapshot follows every poll that continues the stream once
        // 500 ms passed since the last one; the last poll ends it.
        if k < polls && (u64::from(k) * period).is_multiple_of(500) {
            script.push(snap_step(id));
            id += 1;
        }
    }
    script.push(Step::transact(stop_queue(id), accept(id)));
    let mut s = session(Meter::new(script));
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let opts = StreamOptions {
        rate: Rate::Sps10,
        duration: None,
        start_unix_ms: START_UNIX_MS,
    };
    let r = stream(&mut s, &opts, &AtomicBool::new(false), &mut csv, &mut err);
    assert!(matches!(r, Err(ref e) if e.exit_code() == 5), "{r:?}");
    assert!(r.unwrap_err().to_string().contains("no data"));
}

/// A frame the guard refuses is never sent, so `--trace` must not log it as
/// sent (`>`); it is logged as refused (`x`).
#[test]
fn trace_marks_a_refused_frame_as_not_sent() {
    use powerz_protocol::transport::Transport;
    use powerz_protocol::transport::guard::GuardedTransport;
    use powerz_protocol::transport::mock::MockTransport;
    let mut log = Vec::new();
    {
        let inner = GuardedTransport::new(MockTransport::new(vec![]));
        let mut t = TraceTransport::new(inner, &mut log);
        assert!(
            t.transact(&[0x08, 0x01, 0x00, 0x00], Duration::from_millis(10))
                .is_err()
        );
    }
    let log = text(&log);
    assert!(!log.contains("> [4] 08"), "{log}");
    assert!(log.starts_with("x [4] 08 01 00 00"), "{log}");
}

/// `start_unix_ms: 0` asks for the wall clock at the first received record,
/// so session setup time (CONNECT, auth, first poll) does not shift `Unix`.
#[test]
fn stream_unix_zero_uses_wall_clock_at_first_record() {
    let script = [
        start_steps(Rate::Sps10.code()),
        vec![
            poll_step(4, &ts(&[0, 100])),
            Step::transact(stop_queue(5), accept(5)),
        ],
    ]
    .concat();
    let mut s = session(Meter::new(script));
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let opts = StreamOptions {
        rate: Rate::Sps10,
        duration: Some(Duration::from_millis(50)),
        start_unix_ms: 0,
    };
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    stream(&mut s, &opts, &AtomicBool::new(false), &mut csv, &mut err).unwrap();
    let after = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let (_, rows) = csv_rows(&csv);
    let unix: f64 = rows[0][1].parse().unwrap();
    assert!(
        unix >= before - 0.001 && unix <= after + 0.001,
        "{unix} not in [{before}, {after}]"
    );
}

// ---- AC-V9SK52.4: a failed recovery CONNECT is shown, routed by kind ----

/// Device info (ids 1..4), both blocks (ids 5, 6), HEAD (id 7) rejected,
/// then the recovery CONNECT (id 8) rejected too.
fn rejected_recovery_script() -> Vec<Step> {
    let boot = info_block(SYNTH_MODEL, "2.5");
    let app = info_block(SYNTH_MODEL, "2.0.6");
    [
        memread_steps(1, HW_INFO_ADDR, &boot),
        memread_steps(2, FW_INFO_ADDR, &app),
        memread_steps(3, OTP_ADDR, &otp_block()),
        memread_steps(4, UID_ADDR, &SYNTH_UID),
        memread_steps(5, HW_INFO_ADDR, &boot),
        memread_steps(6, FW_INFO_ADDR, &app),
        vec![
            Step::transact(
                powerz_protocol::decode::head::request(7).to_vec(),
                vec![0x06, 0x07, 0x00, 0x00],
            ),
            Step::transact(
                powerz_protocol::frame::connect(8).to_vec(),
                vec![0x06, 0x08, 0x00, 0x00],
            ),
        ],
    ]
    .concat()
}

#[test]
fn info_with_a_failed_recovery_connect_prints_it_as_a_firmware_status_line() {
    let (r, out, _) = info_mode(DeviceMode::Application, false, rejected_recovery_script());
    assert!(
        r.is_ok(),
        "a failed recovery is a problem, not an error: {r:?}"
    );
    let check = line_of(&out, "Update check:");
    assert!(check.contains("the meter sent REJECT"), "{check:?}");
    assert!(
        !check.to_lowercase().contains("recovery"),
        "the recovery problem is not the HEAD reason: {check:?}"
    );
    let lines = status_lines(&out);
    assert_eq!(lines.len(), 1, "one recovery line:\n{out}");
    let line = lines[0].to_lowercase();
    assert!(
        line.contains("recovery connect") && line.contains("failed"),
        "{out}"
    );
    assert!(
        line_of(&out, "Firmware:").contains("2.0.6"),
        "the versions still print: {out}"
    );
}

#[test]
fn info_json_with_a_failed_recovery_connect_lists_both_problems_as_text() {
    let (r, out, _) = info_mode(DeviceMode::Application, true, rejected_recovery_script());
    assert!(r.is_ok(), "{r:?}");
    let v: Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["application"]["version"], "2.0.6", "{v}");
    let problems: Vec<&str> = v["firmware_problems"]
        .as_array()
        .unwrap_or_else(|| panic!("firmware_problems array: {v}"))
        .iter()
        .map(|p| p.as_str().unwrap_or_else(|| panic!("text, not {p}")))
        .collect();
    assert_eq!(problems.len(), 2, "HEAD, then the recovery: {v}");
    assert!(problems[0].contains("HEAD"), "{problems:?}");
    assert!(
        problems[1].to_lowercase().contains("recovery connect"),
        "{problems:?}"
    );
}

// ---- AC-V9SK52.3 (changed 2026-10-26): the real application-mode HEAD reply ----

/// The real meter's HEAD reply in application mode: type 0x40, id 0, field
/// 0x00001000, then 12 stale bytes (synthesised from a 0xC4 tail).
fn real_id0_head_reply() -> Vec<u8> {
    vec![
        0x40, 0x00, 0x00, 0x01, 0x00, 0x10, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff,
        0xff, 0xf4, 0x95, 0x53, 0xf2,
    ]
}

/// Device info (ids 1..4), both info blocks (ids 5, 6), then HEAD (id 7)
/// answered with the real id-0 reply; nothing after it.
fn real_head_script() -> Vec<Step> {
    [
        device_info_steps(1),
        memread_steps(5, HW_INFO_ADDR, &info_block(SYNTH_MODEL, SYNTH_HW)),
        memread_steps(6, FW_INFO_ADDR, &info_block(SYNTH_MODEL, SYNTH_FW)),
        vec![Step::transact(
            powerz_protocol::decode::head::request(7).to_vec(),
            real_id0_head_reply(),
        )],
    ]
    .concat()
}

#[test]
fn info_with_the_real_head_reply_says_the_update_check_is_unavailable_naming_the_field() {
    let (r, out, sent) = info_mode(DeviceMode::Application, false, real_head_script());
    assert!(r.is_ok(), "{r:?}");
    let check = line_of(&out, "Update check:");
    assert!(
        check.starts_with("Update check: unavailable ("),
        "{check:?}"
    );
    assert!(check.ends_with(')'), "{check:?}");
    assert!(check.contains("0x00001000"), "the field: {check:?}");
    assert!(
        squash(check).contains("400000010010000040000000fffffffff49553f2"),
        "the raw reply: {check:?}"
    );
    assert!(!check.contains("timed out"), "no timeout: {check:?}");
    assert!(
        !out.to_lowercase().contains("update required"),
        "no update-required warning: {out}"
    );
    let types: Vec<u8> = sent.iter().map(|f| f[0]).collect();
    assert_eq!(
        types,
        vec![0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x40],
        "one HEAD, no recovery CONNECT"
    );
}

#[test]
fn info_json_with_the_real_head_reply_has_update_required_null() {
    let (r, out, _) = info_mode(DeviceMode::Application, true, real_head_script());
    assert!(r.is_ok(), "{r:?}");
    let v: Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(
        v["update_required"],
        Value::Null,
        "unknown, never true: {v}"
    );
    let codes = v["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        !codes.contains(&serde_json::json!("update_required")),
        "{v}"
    );
    let head = v["firmware_problems"]
        .as_array()
        .unwrap_or_else(|| panic!("firmware_problems array: {v}"))
        .iter()
        .filter_map(Value::as_str)
        .find(|p| p.contains("HEAD"))
        .unwrap_or_else(|| panic!("a HEAD problem: {v}"))
        .to_owned();
    assert!(head.contains("0x00001000"), "{head:?}");
    assert!(
        squash(&head).contains("400000010010000040000000fffffffff49553f2"),
        "{head:?}"
    );
}
