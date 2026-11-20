//! FR JT38C8 (AC .3, .4): a meter on the old bootloader (2.1) in the CLI.
//!
//! - `firmware probe` names its mode `bootloader (old, not supported yet)`
//!   in text and `legacy_bootloader` in JSON, as a command function over
//!   the field-reported 2.1 identity and as the binary on the simulator.
//! - `POWERZ_SIM_MODE=legacy-bootloader` (test-only) runs the simulated
//!   meter's old-bootloader persona for `firmware probe`, `flash` and `jump`.
//!
//! No hardware is touched; every identifier is synthetic.

#[path = "../../powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use fw_container::*;
use powerz_cli::commands::firmware_probe;
use powerz_protocol::session::Session;
use powerz_protocol::transport::discovery::{UsbIdentity, UsbInterface};
use powerz_sim::{FaultPlan, Persona, Scenario, SimClock, SimDevice};
use serde_json::Value;

const TEXT_MODE: &str = "bootloader (old, not supported yet)";
const JSON_MODE: &str = "legacy_bootloader";

const ENV_KNOBS: [&str; 8] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
    "POWERZ_SIM_HEAD",
    "POWERZ_SIM_APP_VERSION",
    "POWERZ_SIM_MODE",
    "POWERZ_SIM_FLASH_FAULT",
    "POWERZ_SIM_REPLUG_AFTER",
];

/// The field-reported bootloader 2.1 identity `[device]`.
fn bootloader_21() -> UsbIdentity {
    UsbIdentity {
        vid: 0x5FC9,
        pid: 0x0063,
        product: Some("POWER-Z KM003C".to_owned()),
        interfaces: vec![UsbInterface {
            number: 0,
            class: 0xFF,
            subclass: 0xFF,
            protocol: 0,
            string: Some("POWER-Z KM003C".to_owned()),
        }],
    }
}

/// `firmware_probe` on the 2.1 identity over a meter that never answers.
fn probe_21(json: bool) -> String {
    let clock = SimClock::new(1_790_000_000_000);
    let silent = SimDevice::with_clock(Persona::Bulk, Scenario::Idle, clock.clone())
        .with_faults(FaultPlan::new().timeout_always());
    let mut s = Session::with_clock(silent, clock);
    let mut out = Vec::new();
    firmware_probe(&mut s, &bootloader_21(), json, &mut out)
        .unwrap_or_else(|e| panic!("probe failed: {e}"));
    String::from_utf8_lossy(&out).into_owned()
}

fn mode_line(out: &str) -> &str {
    out.lines()
        .find(|l| l.starts_with("Mode:"))
        .unwrap_or_else(|| panic!("no Mode line in:\n{out}"))
}

/// `powerz --transport sim <args>` with `env`, on the fake sim clock.
fn sim(env: &[(&str, &str)], args: &[&std::ffi::OsStr]) -> Command {
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
    c.args(args);
    c
}

/// Run `c`; stdout, stderr and the exit code.
fn output(c: &mut Command) -> (String, String, Option<i32>) {
    let out = c.output().expect("powerz runs");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code(),
    )
}

fn os(args: &[&str]) -> Vec<std::ffi::OsString> {
    args.iter().map(std::ffi::OsString::from).collect()
}

fn refs(v: &[std::ffi::OsString]) -> Vec<&std::ffi::OsStr> {
    v.iter().map(|s| s.as_os_str()).collect()
}

const LEGACY: (&str, &str) = ("POWERZ_SIM_MODE", "legacy-bootloader");

// ---------------------------------------------------------------- AC-JT38C8.4, command function

#[test]
fn jt38c8_probe_text_names_the_2_1_bootloader_as_old_and_unsupported() {
    let out = probe_21(false);
    assert!(mode_line(&out).contains(TEXT_MODE), "{out}");
}

#[test]
fn jt38c8_probe_json_names_the_2_1_bootloader_legacy_bootloader() {
    let out = probe_21(true);
    let v: Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["mode"], JSON_MODE, "{v}");
}

// ---------------------------------------------------------------- AC-JT38C8.3 / .4, the binary

#[test]
fn jt38c8_sim_probe_in_legacy_mode_shows_the_2_1_descriptor_and_old_mode() {
    let args = os(&["firmware", "probe"]);
    let (out, err, code) = output(&mut sim(&[LEGACY], &refs(&args)));
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(mode_line(&out).contains(TEXT_MODE), "{out}");
    let if0 = out
        .lines()
        .find(|l| l.trim_start().starts_with("interface 0:"))
        .unwrap_or_else(|| panic!("no interface 0 line in:\n{out}"));
    assert!(
        if0.contains("class 0xff") && if0.contains("subclass 0xff"),
        "{if0}"
    );
    assert!(if0.contains("POWER-Z KM003C"), "{if0}");
    assert!(!out.contains("interface 1:"), "one interface only:\n{out}");
}

#[test]
fn jt38c8_sim_probe_in_legacy_mode_gets_no_answer_to_connect_or_head() {
    let args = os(&["firmware", "probe"]);
    let (out, err, code) = output(&mut sim(&[LEGACY], &refs(&args)));
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("CONNECT: timed out"), "{out}");
    assert!(out.contains("HEAD: timed out"), "{out}");
}

#[test]
fn jt38c8_sim_probe_json_in_legacy_mode() {
    let args = os(&["firmware", "probe", "--json"]);
    let (out, err, code) = output(&mut sim(&[LEGACY], &refs(&args)));
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    let v: Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["mode"], JSON_MODE, "{v}");
    let ifs = v["interfaces"].as_array().expect("interfaces array");
    assert_eq!(ifs.len(), 1, "{v}");
    assert_eq!(ifs[0]["class"], 0xFF, "{v}");
    assert_eq!(ifs[0]["subclass"], 0xFF, "{v}");
    assert_eq!(ifs[0]["string"], "POWER-Z KM003C", "{v}");
    assert_eq!(v["connect"], "timed out", "{v}");
    assert_eq!(v["head"], "timed out", "{v}");
}

#[test]
fn jt38c8_sim_jump_accepts_the_legacy_bootloader_mode() {
    let args = os(&["firmware", "jump", "--to", "application", "--i-understand"]);
    let (out, err, code) = output(&mut sim(&[LEGACY], &refs(&args)));
    assert!(
        !err.contains("POWERZ_SIM_MODE"),
        "legacy-bootloader is a valid POWERZ_SIM_MODE: {err}"
    );
    assert_ne!(code, Some(2), "not a usage error: {out}\n{err}");
}

fn valid_file(name: &str) -> PathBuf {
    let payload: Vec<u8> = (0..3 * 4080 + 100).map(|i| (i * 29 + 3) as u8).collect();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("jt38c8-flash-cli");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(name);
    std::fs::write(
        &path,
        ContainerBuilder::new().payload_sized(&payload).build(),
    )
    .expect("write container");
    path
}

#[test]
fn jt38c8_sim_flash_accepts_the_legacy_bootloader_mode() {
    let path = valid_file("legacy.mencrypt");
    let args = [path.as_os_str(), std::ffi::OsStr::new("--i-understand")];
    let mut full: Vec<&std::ffi::OsStr> = vec!["firmware".as_ref(), "flash".as_ref()];
    full.extend(args);
    let (out, err, code) = output(&mut sim(&[LEGACY], &full));
    assert!(
        !err.contains("POWERZ_SIM_MODE"),
        "legacy-bootloader is a valid POWERZ_SIM_MODE: {err}"
    );
    assert_ne!(code, Some(2), "not a usage error: {out}\n{err}");
}
