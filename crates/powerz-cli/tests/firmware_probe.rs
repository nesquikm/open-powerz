//! `powerz firmware probe` (FR V9SK56, AC .6): the meter's USB identity,
//! then only CONNECT and HEAD, printing the HEAD field. As a command
//! function over a scripted mock transport, and as the real binary against
//! the simulator in application and bootloader mode. No hardware is touched.
//!
//! Test-only simulator knobs used here (honoured with `--transport sim`):
//!
//! - `POWERZ_SIM_MODE=bootloader`: the simulated meter runs its bootloader
//!   (vendor interface string `BOOT`); unset, application mode.
//! - `POWERZ_SIM_HEAD=<n>`: HEAD answers with field `n` (default 0).
//! - `POWERZ_SIM_FAULT=timeout`: the simulated meter never answers.

mod common;

use std::time::Duration;

use assert_cmd::Command;
use clap::Parser;
use common::*;
use powerz_cli::args::{Command as CliCommand, FirmwareAction};
use powerz_cli::commands::{firmware_probe, firmware_probe_unavailable};
use powerz_cli::{Cli, CliError, validate};
use powerz_protocol::decode::head;
use powerz_protocol::frame;
use powerz_protocol::transport;
use powerz_protocol::transport::discovery::{UsbIdentity, UsbInterface};
use powerz_protocol::transport::guard::FORBIDDEN_TYPES;
use powerz_protocol::transport::mock::{MockTransport, Step};
use serde_json::Value;

// ---------------------------------------------------------------- helpers

fn iface(number: u8, class: u8, string: Option<&str>) -> UsbInterface {
    UsbInterface {
        number,
        class,
        subclass: 0,
        protocol: 0,
        string: string.map(str::to_owned),
    }
}

fn app_identity() -> UsbIdentity {
    UsbIdentity {
        vid: 0x5FC9,
        pid: 0x0063,
        product: Some("POWER-Z KM003C".to_owned()),
        interfaces: vec![
            iface(0, 0xFF, Some("APP")),
            iface(1, 0x02, Some("APP")),
            iface(2, 0x0A, None),
            iface(3, 0x03, None),
        ],
    }
}

fn head_reply(id: u8, field: u32) -> Vec<u8> {
    let mut f = vec![0x40, id, 0x00, 0x01];
    f.extend(field.to_le_bytes());
    f.extend([0u8; 12]);
    f
}

fn reject(id: u8) -> Vec<u8> {
    vec![0x06, id, 0, 0]
}

/// Run `firmware_probe` on a fresh session over `script`; return the
/// output and the frame types written.
fn probe(script: Vec<Step>, json: bool) -> (String, Vec<u8>) {
    let mut s = session(MockTransport::new(script));
    let mut out = Vec::new();
    firmware_probe(&mut s, &app_identity(), json, &mut out)
        .unwrap_or_else(|e| panic!("probe failed: {e}"));
    let types = s
        .transport()
        .written()
        .iter()
        .map(|f| f[0] & 0x7F)
        .collect();
    (text(&out), types)
}

fn ok_script(field: u32) -> Vec<Step> {
    vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact(head::request(2).to_vec(), head_reply(2, field)),
    ]
}

/// The line of `out` describing interface `n` (`interface <n>`, any case).
fn interface_line(out: &str, n: u8) -> String {
    let want = format!("interface {n}");
    let lines: Vec<String> = out
        .lines()
        .map(str::to_ascii_lowercase)
        .filter(|l| l.contains(&want))
        .collect();
    assert_eq!(lines.len(), 1, "one line for interface {n} in:\n{out}");
    lines[0].clone()
}

// ---------------------------------------------------------------- argument parsing

#[test]
fn firmware_probe_parses_with_and_without_json() {
    let cli = Cli::try_parse_from(["powerz", "firmware", "probe"]).expect("parses");
    assert_eq!(
        cli.command,
        CliCommand::Firmware {
            action: FirmwareAction::Probe { json: false }
        }
    );
    let cli = Cli::try_parse_from(["powerz", "firmware", "probe", "--json"]).expect("parses");
    assert_eq!(
        cli.command,
        CliCommand::Firmware {
            action: FirmwareAction::Probe { json: true }
        }
    );
}

// ---------------------------------------------------------------- command function

#[test]
fn probe_prints_identity_connect_and_head_field() {
    let (out, types) = probe(ok_script(0), false);
    let lower = out.to_ascii_lowercase();
    assert!(lower.contains("0x5fc9"), "VID:\n{out}");
    assert!(lower.contains("0x0063"), "PID:\n{out}");
    assert!(out.contains("POWER-Z KM003C"), "product:\n{out}");
    let if0 = interface_line(&out, 0);
    assert!(if0.contains("0xff") && if0.contains("app"), "{if0}");
    let if1 = interface_line(&out, 1);
    assert!(if1.contains("0x02") && if1.contains("app"), "{if1}");
    assert!(interface_line(&out, 2).contains("0x0a"), "{out}");
    assert!(interface_line(&out, 3).contains("0x03"), "{out}");
    assert!(out.contains("CONNECT: accepted"), "{out}");
    assert!(out.contains("HEAD field: 0"), "{out}");
    assert_eq!(types, vec![0x02, 0x40], "only CONNECT and HEAD");
}

#[test]
fn probe_prints_a_non_zero_head_field() {
    let (out, _) = probe(ok_script(5), false);
    assert!(out.contains("HEAD field: 5"), "{out}");
}

#[test]
fn probe_prints_a_rejected_connect_and_still_asks_head() {
    let (out, types) = probe(
        vec![
            Step::transact(frame::connect(1).to_vec(), reject(1)),
            Step::transact(head::request(2).to_vec(), head_reply(2, 0)),
        ],
        false,
    );
    assert!(out.contains("CONNECT: rejected"), "{out}");
    assert!(out.contains("HEAD field: 0"), "{out}");
    assert_eq!(types, vec![0x02, 0x40]);
}

#[test]
fn probe_prints_the_raw_head_reply_when_it_does_not_decode() {
    let (out, _) = probe(
        vec![
            Step::transact(frame::connect(1).to_vec(), accept(1)),
            Step::transact(head::request(2).to_vec(), vec![0x40, 2, 0x00, 0x01]),
        ],
        false,
    );
    assert!(!out.contains("HEAD field:"), "no decoded field:\n{out}");
    assert!(
        out.to_ascii_lowercase().contains("40 02 00 01"),
        "raw HEAD reply as hex:\n{out}"
    );
}

#[test]
fn probe_prints_a_head_timeout_as_data() {
    let (out, _) = probe(
        vec![
            Step::transact(frame::connect(1).to_vec(), accept(1)),
            Step::transact_err(head::request(2).to_vec(), transport::Error::Timeout),
        ],
        false,
    );
    assert!(out.contains("CONNECT: accepted"), "{out}");
    assert!(out.contains("HEAD: timed out"), "{out}");
}

#[test]
fn probe_json_has_identity_connect_and_head() {
    let (out, types) = probe(ok_script(0), true);
    let v: Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["vid"], 0x5FC9, "{v}");
    assert_eq!(v["pid"], 0x0063, "{v}");
    assert_eq!(v["product"], "POWER-Z KM003C", "{v}");
    let ifs = v["interfaces"].as_array().expect("interfaces array");
    assert_eq!(ifs.len(), 4, "{v}");
    assert_eq!(ifs[0]["number"], 0);
    assert_eq!(ifs[0]["class"], 0xFF);
    assert_eq!(ifs[0]["string"], "APP");
    assert_eq!(ifs[2]["class"], 0x0A);
    assert_eq!(ifs[2]["string"], Value::Null);
    assert_eq!(v["connect"], "accepted", "{v}");
    assert_eq!(v["head_field"], 0, "{v}");
    assert_eq!(v["head_raw"], Value::Null, "{v}");
    assert_eq!(types, vec![0x02, 0x40]);
}

#[test]
fn probe_json_keeps_the_raw_reply_and_a_rejected_connect() {
    let (out, _) = probe(
        vec![
            Step::transact(frame::connect(1).to_vec(), reject(1)),
            Step::transact(head::request(2).to_vec(), vec![0x40, 2, 0x00, 0x01]),
        ],
        true,
    );
    let v: Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["connect"], "rejected", "{v}");
    assert_eq!(v["head_field"], Value::Null, "{v}");
    assert_eq!(v["head_raw"], "40020001", "{v}");
}

// ---------------------------------------------------------------- raw HEAD bytes in both modes

/// A bootloader-mode identity: one vendor interface with string `BOOT`.
fn boot_identity() -> UsbIdentity {
    UsbIdentity {
        vid: 0x5FC9,
        pid: 0x0063,
        product: Some("POWER-Z KM003C".to_owned()),
        interfaces: vec![iface(0, 0xFF, Some("BOOT"))],
    }
}

/// `firmware_probe` with `identity` on a fresh session over CONNECT →
/// ACCEPT and HEAD → `head_reply`; returns the output.
fn probe_head_as(identity: &UsbIdentity, head_reply: Vec<u8>, json: bool) -> String {
    let mut s = session(MockTransport::new(vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact(head::request(2).to_vec(), head_reply),
    ]));
    let mut out = Vec::new();
    firmware_probe(&mut s, identity, json, &mut out)
        .unwrap_or_else(|e| panic!("probe failed: {e}"));
    text(&out)
}

fn both_modes() -> [(&'static str, UsbIdentity); 2] {
    [
        ("application", app_identity()),
        ("bootloader", boot_identity()),
    ]
}

/// A HEAD reply of another type (not 0x40, not REJECT).
fn wrong_type(id: u8) -> Vec<u8> {
    vec![0x41, id, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04]
}

#[test]
fn probe_prints_the_raw_bytes_of_a_rejected_head_in_both_modes() {
    for (mode, identity) in both_modes() {
        let out = probe_head_as(&identity, reject(2), false);
        assert!(
            out.contains("HEAD reply (raw): 06 02 00 00"),
            "{mode}: raw REJECT bytes:\n{out}"
        );
        assert!(
            out.to_ascii_lowercase().contains("rejected"),
            "{mode}: still says HEAD was rejected:\n{out}"
        );
        assert!(!out.contains("HEAD field:"), "{mode}: no field:\n{out}");
    }
}

#[test]
fn probe_json_keeps_the_raw_bytes_of_a_rejected_head_in_both_modes() {
    for (mode, identity) in both_modes() {
        let out = probe_head_as(&identity, reject(2), true);
        let v: Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["head"], "rejected", "{mode}: {v}");
        assert_eq!(v["head_field"], Value::Null, "{mode}: {v}");
        assert_eq!(v["head_raw"], "06020000", "{mode}: {v}");
    }
}

#[test]
fn probe_prints_the_raw_bytes_of_a_wrong_type_head_in_both_modes() {
    for (mode, identity) in both_modes() {
        let out = probe_head_as(&identity, wrong_type(2), false);
        assert!(
            out.contains("HEAD reply (raw): 41 02 00 00 01 02 03 04"),
            "{mode}: raw wrong-type bytes:\n{out}"
        );
        assert!(!out.contains("HEAD field:"), "{mode}: no field:\n{out}");
    }
}

#[test]
fn probe_json_keeps_the_raw_bytes_of_a_wrong_type_head_in_both_modes() {
    for (mode, identity) in both_modes() {
        let out = probe_head_as(&identity, wrong_type(2), true);
        let v: Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["head"], "raw", "{mode}: {v}");
        assert_eq!(v["head_field"], Value::Null, "{mode}: {v}");
        assert_eq!(v["head_raw"], "4102000001020304", "{mode}: {v}");
    }
}

// ---------------------------------------------------------------- id-0 HEAD replies (AC-V9SK52.3)
//
// The real meter answers HEAD with type 0x40 and id 0 (field 0x00001000 on
// 2.0.6, then 12 stale bytes). The probe reports what it got without
// judging it: the field and the raw reply, in both modes.

/// The real application-mode HEAD reply: `40 00 00 01 00 10 00 00` and 12
/// stale bytes (synthesised from a memory-read confirmation tail).
fn real_id0_reply() -> Vec<u8> {
    vec![
        0x40, 0x00, 0x00, 0x01, 0x00, 0x10, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff,
        0xff, 0xf4, 0x95, 0x53, 0xf2,
    ]
}

#[test]
fn probe_prints_the_field_and_raw_bytes_of_an_id_0_head_reply_in_both_modes() {
    for (mode, identity) in both_modes() {
        let out = probe_head_as(&identity, real_id0_reply(), false);
        assert!(
            out.contains("HEAD field: 4096"),
            "{mode}: the field:
{out}"
        );
        assert!(
            out.contains(
                "HEAD reply (raw): 40 00 00 01 00 10 00 00 40 00 00 00 ff ff ff ff f4 95 53 f2"
            ),
            "{mode}: the raw reply:
{out}"
        );
        assert!(
            !out.contains("HEAD: timed out"),
            "{mode}: no timeout:
{out}"
        );
        let lower = out.to_lowercase();
        assert!(
            !lower.contains("update required") && !lower.contains("unrecognised"),
            "{mode}: the probe does not judge the field:
{out}"
        );
    }
}

#[test]
fn probe_json_keeps_the_field_and_raw_bytes_of_an_id_0_head_reply_in_both_modes() {
    for (mode, identity) in both_modes() {
        let out = probe_head_as(&identity, real_id0_reply(), true);
        let v: Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["head"], "field", "{mode}: {v}");
        assert_eq!(v["head_field"], 0x1000, "{mode}: {v}");
        assert_eq!(
            v["head_raw"], "400000010010000040000000fffffffff49553f2",
            "{mode}: {v}"
        );
    }
}

#[test]
fn probe_sends_one_head_for_an_id_0_reply() {
    let (out, types) = probe(
        vec![
            Step::transact(frame::connect(1).to_vec(), accept(1)),
            Step::transact(head::request(2).to_vec(), real_id0_reply()),
        ],
        false,
    );
    assert!(out.contains("HEAD field: 4096"), "{out}");
    assert_eq!(types, vec![0x02, 0x40], "one HEAD, no re-send");
}

// ---------------------------------------------------------------- the binary on the simulator

const ENV_KNOBS: [&str; 6] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
    "POWERZ_SIM_HEAD",
    "POWERZ_SIM_APP_VERSION",
    "POWERZ_SIM_MODE",
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

/// Frame types of the `> ` (sent) lines of a `--trace` log.
fn sent_types(trace: &str) -> Vec<u8> {
    trace
        .lines()
        .filter_map(|l| l.strip_prefix("> "))
        .map(|hex| {
            let first = hex.split_whitespace().next().expect("a byte");
            u8::from_str_radix(first, 16).expect("hex byte") & 0x7F
        })
        .collect()
}

#[test]
fn sim_probe_in_application_mode_prints_identity_and_head_field() {
    let (out, _) = sim_probe(false, false, &[]);
    let lower = out.to_ascii_lowercase();
    assert!(
        lower.contains("0x5fc9") && lower.contains("0x0063"),
        "{out}"
    );
    assert!(out.contains("POWER-Z KM003C"), "{out}");
    let if0 = interface_line(&out, 0);
    assert!(if0.contains("0xff") && if0.contains("app"), "{if0}");
    assert!(interface_line(&out, 1).contains("0x02"), "{out}");
    assert!(interface_line(&out, 2).contains("0x0a"), "{out}");
    assert!(interface_line(&out, 3).contains("0x03"), "{out}");
    assert!(out.contains("CONNECT: accepted"), "{out}");
    assert!(out.contains("HEAD field: 0"), "{out}");
}

#[test]
fn sim_probe_in_bootloader_mode_shows_boot_and_answers() {
    let (out, _) = sim_probe(false, false, &[("POWERZ_SIM_MODE", "bootloader")]);
    let if0 = interface_line(&out, 0);
    assert!(if0.contains("0xff") && if0.contains("boot"), "{if0}");
    assert!(
        !out.to_ascii_lowercase().contains("interface 1"),
        "one interface only:\n{out}"
    );
    assert!(out.contains("CONNECT: accepted"), "{out}");
    assert!(out.contains("HEAD field: 0"), "{out}");
}

#[test]
fn sim_probe_trace_shows_only_connect_and_head_in_both_modes() {
    for mode in ["application", "bootloader"] {
        let env: Vec<(&str, &str)> = if mode == "bootloader" {
            vec![("POWERZ_SIM_MODE", "bootloader")]
        } else {
            vec![]
        };
        let (_, err) = sim_probe(false, true, &env);
        let types = sent_types(&err);
        assert!(!types.is_empty(), "{mode}: trace logged frames:\n{err}");
        assert!(
            types.iter().all(|t| *t == 0x02 || *t == 0x40),
            "{mode}: only CONNECT and HEAD on the wire: {types:02x?}\n{err}"
        );
        assert_eq!(
            types.iter().filter(|t| **t == 0x40).count(),
            1,
            "{mode}: one HEAD: {types:02x?}"
        );
        assert!(types.contains(&0x02), "{mode}: CONNECT sent: {types:02x?}");
        for t in &types {
            assert!(!FORBIDDEN_TYPES.contains(t), "{mode}: {t:#04x}");
        }
    }
}

#[test]
fn sim_probe_prints_the_configured_head_field() {
    let (out, _) = sim_probe(false, false, &[("POWERZ_SIM_HEAD", "3")]);
    assert!(out.contains("HEAD field: 3"), "{out}");
}

#[test]
fn sim_probe_of_a_silent_meter_reports_timeouts_and_exits_zero() {
    let (out, _) = sim_probe(false, false, &[("POWERZ_SIM_FAULT", "timeout")]);
    assert!(out.contains("CONNECT: timed out"), "{out}");
    assert!(out.contains("HEAD: timed out"), "{out}");
}

#[test]
fn sim_probe_json_in_bootloader_mode() {
    let (out, _) = sim_probe(true, false, &[("POWERZ_SIM_MODE", "bootloader")]);
    let v: Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["vid"], 0x5FC9, "{v}");
    let ifs = v["interfaces"].as_array().expect("interfaces array");
    assert_eq!(ifs.len(), 1, "{v}");
    assert_eq!(ifs[0]["class"], 0xFF);
    assert_eq!(ifs[0]["string"], "BOOT");
    assert_eq!(v["connect"], "accepted", "{v}");
    assert_eq!(v["head_field"], 0, "{v}");
}

#[test]
fn invalid_sim_mode_is_a_usage_error() {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_MODE", "sideways");
    c.timeout(Duration::from_secs(60));
    c.args(["--transport", "sim", "firmware", "probe"]);
    let out = c.assert().failure().get_output().clone();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("POWERZ_SIM_MODE"), "{err}");
}

// ---------------------------------------------------------------- a pipe that will not open

/// A transport standing in for a bulk pipe that cannot be opened: every
/// transfer fails with `error`. Records what was sent.
struct Unopenable {
    error: transport::Error,
    sent: Vec<Vec<u8>>,
}

impl transport::Transport for Unopenable {
    fn transact(&mut self, request: &[u8], _t: Duration) -> Result<Vec<u8>, transport::Error> {
        self.sent.push(request.to_vec());
        Err(self.error.clone())
    }
    fn read(&mut self, _t: Duration) -> Result<Vec<u8>, transport::Error> {
        Err(self.error.clone())
    }
}

/// Run `firmware_probe` over an [`Unopenable`] pipe; return the result,
/// the output written and the frame types sent.
fn probe_unopenable(
    error: transport::Error,
    json: bool,
) -> (Result<(), CliError>, String, Vec<u8>) {
    let mut s = session(Unopenable {
        error,
        sent: Vec::new(),
    });
    let mut out = Vec::new();
    let result = firmware_probe(&mut s, &app_identity(), json, &mut out);
    let sent = s.transport().sent.iter().map(|f| f[0] & 0x7F).collect();
    (result, text(&out), sent)
}

#[test]
fn probe_whose_pipe_is_not_found_fails_with_a_no_device_error() {
    let (result, out, sent) = probe_unopenable(transport::Error::NotFound, false);
    let err = result.expect_err("an unopenable pipe is a failure, not data");
    assert_ne!(err.exit_code(), 0, "{err:?}");
    assert!(
        matches!(err, CliError::NoDevice(_)),
        "NotFound maps to the no-device exit: {err:?}"
    );
    assert!(
        err.to_string().contains("no matching device"),
        "the reason: {err}"
    );
    assert!(!out.contains("CONNECT"), "no CONNECT line at all:\n{out}");
    assert!(
        !out.to_ascii_lowercase().contains("failed"),
        "no `failed` outcome:\n{out}"
    );
    assert!(
        out.contains("POWER-Z KM003C"),
        "identity still shown:\n{out}"
    );
    assert!(!sent.contains(&0x40), "no HEAD: {sent:02x?}");
}

#[test]
fn probe_whose_pipe_fails_with_io_exits_with_a_device_error_naming_the_reason() {
    let (result, out, sent) = probe_unopenable(
        transport::Error::Io("claim interface 0 failed".into()),
        false,
    );
    let err = result.expect_err("an I/O failure is a failure, not data");
    assert_eq!(err.exit_code(), 5, "device error exit: {err:?}");
    assert!(
        err.to_string().contains("claim interface 0 failed"),
        "the reason: {err}"
    );
    assert!(!out.contains("CONNECT"), "no CONNECT line at all:\n{out}");
    assert!(!sent.contains(&0x40), "no HEAD: {sent:02x?}");
}

#[test]
fn probe_json_whose_pipe_fails_writes_no_json_report() {
    let (result, out, _) = probe_unopenable(transport::Error::Io("open failed".into()), true);
    assert!(result.is_err(), "{result:?}");
    assert!(
        !out.contains("\"connect\""),
        "no report with a `connect` key:\n{out}"
    );
}

#[test]
fn probe_head_io_failure_after_accept_is_an_error_not_a_head_outcome() {
    let mut s = session(MockTransport::new(vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact_err(
            head::request(2).to_vec(),
            transport::Error::Io("bulk read failed".into()),
        ),
    ]));
    let mut out = Vec::new();
    let err = firmware_probe(&mut s, &app_identity(), false, &mut out)
        .expect_err("an I/O failure during HEAD is a failure");
    assert!(err.to_string().contains("bulk read failed"), "{err}");
    let out = text(&out);
    assert!(!out.contains("HEAD:"), "no HEAD outcome line:\n{out}");
}

// ---------------------------------------------------------------- no vendor interface

/// A meter that shows only CDC and HID interfaces (no 0xFF bulk pipe).
fn no_vendor_identity() -> UsbIdentity {
    UsbIdentity {
        vid: 0x5FC9,
        pid: 0x0063,
        product: Some("POWER-Z KM003C".to_owned()),
        interfaces: vec![
            iface(0, 0x02, Some("APP")),
            iface(1, 0x0A, None),
            iface(2, 0x03, None),
        ],
    }
}

#[test]
fn unavailable_text_prints_the_identity_and_says_the_probe_cannot_run() {
    let mut out = Vec::new();
    let mut err = Vec::new();
    firmware_probe_unavailable(&no_vendor_identity(), false, &mut out, &mut err)
        .expect("not an error: the identity is the answer");
    let out = text(&out);
    let err = text(&err);
    let lower = out.to_ascii_lowercase();
    assert!(
        lower.contains("0x5fc9") && lower.contains("0x0063"),
        "{out}"
    );
    assert!(out.contains("POWER-Z KM003C"), "{out}");
    assert!(interface_line(&out, 0).contains("0x02"), "{out}");
    assert!(interface_line(&out, 1).contains("0x0a"), "{out}");
    assert!(interface_line(&out, 2).contains("0x03"), "{out}");
    assert!(!out.contains("CONNECT"), "nothing was sent:\n{out}");
    assert!(!out.contains("HEAD"), "nothing was sent:\n{out}");
    assert!(err.contains("cannot run"), "stderr says why:\n{err}");
    assert!(
        err.contains("0xFF"),
        "stderr names the missing class:\n{err}"
    );
}

#[test]
fn unavailable_json_has_the_identity_and_null_answers() {
    let mut out = Vec::new();
    let mut err = Vec::new();
    firmware_probe_unavailable(&no_vendor_identity(), true, &mut out, &mut err)
        .expect("not an error");
    let v: Value = serde_json::from_str(&text(&out)).expect("valid JSON");
    assert_eq!(v["vid"], 0x5FC9, "{v}");
    assert_eq!(v["pid"], 0x0063, "{v}");
    assert_eq!(v["product"], "POWER-Z KM003C", "{v}");
    let ifs = v["interfaces"].as_array().expect("interfaces array");
    assert_eq!(ifs.len(), 3, "{v}");
    assert!(ifs.iter().all(|i| i["class"] != 0xFF), "{v}");
    for key in ["connect", "head", "head_field", "head_raw"] {
        assert!(v.get(key).is_some(), "key {key} present: {v}");
        assert_eq!(v[key], Value::Null, "{key} is null: {v}");
    }
    assert!(text(&err).contains("cannot run"), "stderr says why");
}

// ---------------------------------------------------------------- the probe needs the bulk pipe

#[test]
fn probe_over_hid_or_cdc_is_a_usage_error_pointing_at_the_bulk_pipe() {
    for kind in ["hid", "cdc"] {
        let cli = Cli::try_parse_from(["powerz", "--transport", kind, "firmware", "probe"])
            .expect("parses");
        let err = validate(&cli).expect_err("refused before any device access");
        assert!(matches!(err, CliError::Usage(_)), "{kind}: {err:?}");
        assert_eq!(err.exit_code(), 2, "{kind}");
        let msg = err.to_string();
        assert!(msg.contains("bulk pipe"), "{kind}: says why: {msg}");
        assert!(msg.contains("--transport bulk"), "{kind}: the fix: {msg}");
        assert!(msg.contains("sim"), "{kind}: sim works too: {msg}");
    }
}

#[test]
fn probe_over_bulk_or_sim_passes_validation() {
    for kind in ["bulk", "sim"] {
        let cli = Cli::try_parse_from(["powerz", "--transport", kind, "firmware", "probe"])
            .expect("parses");
        assert_eq!(validate(&cli), Ok(()), "{kind}");
    }
}

#[test]
fn probe_help_says_it_uses_the_bulk_pipe() {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    c.timeout(Duration::from_secs(60));
    c.args(["firmware", "probe", "--help"]);
    let out = c.assert().success().get_output().clone();
    let help = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
    assert!(help.contains("bulk pipe"), "{help}");
}
