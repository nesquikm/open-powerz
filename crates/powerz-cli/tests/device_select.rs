//! `--device` picks the meter for every command that talks to one (FR
//! 1HHT03, AC .5), and the CLI stays a single-device tool. The demo device
//! stands in for a second meter through the simulator's serial override,
//! set with the test-only `POWERZ_SIM_SERIAL` knob. Every serial here is
//! synthetic; nothing touches hardware.

use std::time::Duration;

use assert_cmd::Command;
use predicates::prelude::*;

const ENV_KNOBS: [&str; 5] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
    "POWERZ_SIM_SERIAL",
    "POWERZ_SIM_CHARGER",
];

/// A made-up serial for the second simulated meter.
const OTHER_SERIAL: &str = "SIM-B-0002";
/// The demo device's own made-up serial.
const DEMO_SERIAL: &str = "123456";

fn powerz() -> Command {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_CLOCK", "fake");
    c.timeout(Duration::from_secs(60));
    c
}

/// `powerz --transport sim --scenario load-5v-1a`.
fn sim() -> Command {
    let mut c = powerz();
    c.args(["--transport", "sim", "--scenario", "load-5v-1a"]);
    c
}

/// Placeholder for the CSV path `stream` needs, swapped for a temp file.
const CSV: &str = "{csv}";

/// Every subcommand that talks to a meter, with the arguments it needs.
const METER_COMMANDS: &[&[&str]] = &[
    &["info"],
    &["read"],
    &["settings"],
    &["records", "list"],
    &["watch", "--interval", "50"],
    &["stream", "--rate", "50", "--duration", "1", "--csv", CSV],
    &["pd", "--json"],
    &["detect"],
    &["trigger", "pd", "list"],
    &["console"],
    &["firmware", "probe"],
    &["firmware", "jump", "--to", "application", "--i-understand"],
];

#[test]
fn every_meter_command_refuses_a_device_that_is_not_there() {
    let dir = std::env::temp_dir().join(format!("powerz-device-select-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("never.csv");
    for args in METER_COMMANDS {
        let args: Vec<String> = args
            .iter()
            .map(|a| {
                if *a == CSV {
                    csv.display().to_string()
                } else {
                    (*a).to_owned()
                }
            })
            .collect();
        let a = sim()
            .args(["--device", "999999"])
            .args(&args)
            .write_stdin("")
            .assert();
        let out = a.get_output();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            out.status.code(),
            Some(3),
            "`{}` with an unknown --device: exit {:?}, stderr {stderr}",
            args.join(" "),
            out.status.code()
        );
        assert!(
            stderr.contains("999999"),
            "`{}` does not name the missing device: {stderr}",
            args.join(" ")
        );
    }
    assert!(!csv.exists(), "a CSV was written for a missing device");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_simulators_serial_override_is_what_device_selects() {
    let a = sim()
        .env("POWERZ_SIM_SERIAL", OTHER_SERIAL)
        .args(["--device", OTHER_SERIAL, "info", "--json"])
        .assert()
        .success();
    let v: serde_json::Value =
        serde_json::from_slice(&a.get_output().stdout).expect("info --json is JSON");
    assert_eq!(v["serial"], OTHER_SERIAL);

    // The demo device's own serial no longer names it.
    sim()
        .env("POWERZ_SIM_SERIAL", OTHER_SERIAL)
        .args(["--device", DEMO_SERIAL, "read"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(DEMO_SERIAL));
}

#[test]
fn list_include_demo_shows_the_overridden_serial() {
    let a = powerz()
        .env("POWERZ_SIM_SERIAL", OTHER_SERIAL)
        .args(["list", "--include-demo", "--json"])
        .assert()
        .success();
    let v: serde_json::Value =
        serde_json::from_slice(&a.get_output().stdout).expect("list --json is JSON");
    // Only the demo entry is looked at (and printed): a real meter listed by
    // the OS never shows up in the test output.
    let demo: Vec<&serde_json::Value> = v
        .as_array()
        .expect("a JSON list")
        .iter()
        .filter(|d| d["bus"] == "demo")
        .collect();
    assert_eq!(demo.len(), 1, "one demo entry: {demo:?}");
    assert_eq!(
        demo[0]["serial"], OTHER_SERIAL,
        "the demo entry lacks the override"
    );
}

#[test]
fn the_trigger_commands_follow_device_too() {
    sim()
        .env("POWERZ_SIM_SERIAL", OTHER_SERIAL)
        .args(["--device", OTHER_SERIAL, "trigger", "pd", "list"])
        .assert()
        .success();
    sim()
        .env("POWERZ_SIM_SERIAL", OTHER_SERIAL)
        .args(["--device", DEMO_SERIAL, "trigger", "pd", "list"])
        .assert()
        .code(3);
}

#[test]
fn without_device_the_single_demo_meter_is_used_as_before() {
    let a = sim().args(["info", "--json"]).assert().success();
    let v: serde_json::Value = serde_json::from_slice(&a.get_output().stdout).unwrap();
    assert_eq!(v["serial"], DEMO_SERIAL);
}
