//! `powerz info` reports the device mode, both versions and firmware
//! warnings (FR V9SK52, AC .2 and .4), run as the real binary against the
//! simulator. No hardware is touched.
//!
//! Test-only simulator knobs used here (honoured with `--transport sim`):
//!
//! - `POWERZ_SIM_HEAD=<n>`: the simulator answers HEAD with old-firmware
//!   field `n` (default 0).
//! - `POWERZ_SIM_APP_VERSION=<v>`: the application info block (0x4420)
//!   reports version `v` (default `2.0.6`).

use std::time::Duration;

use assert_cmd::Command;

const ENV_KNOBS: [&str; 5] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
    "POWERZ_SIM_HEAD",
    "POWERZ_SIM_APP_VERSION",
];

fn info(json: bool, env: &[(&str, &str)]) -> (String, String) {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_CLOCK", "fake");
    for (k, v) in env {
        c.env(k, v);
    }
    c.timeout(Duration::from_secs(60));
    c.args(["--transport", "sim", "--scenario", "idle", "info"]);
    if json {
        c.arg("--json");
    }
    let out = c.assert().success().get_output().clone();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The stdout line starting with `label`.
fn line<'a>(out: &'a str, label: &str) -> &'a str {
    out.lines()
        .find(|l| l.starts_with(label))
        .unwrap_or_else(|| panic!("no `{label}` line in:\n{out}"))
}

fn warning_lines(out: &str) -> Vec<&str> {
    out.lines().filter(|l| l.starts_with("Warning:")).collect()
}

// ---- AC-V9SK52.2: mode and both versions ----

#[test]
fn info_prints_mode_and_both_versions_with_dates() {
    let (out, _) = info(false, &[]);
    assert!(
        line(&out, "Mode:").to_lowercase().contains("application"),
        "{out}"
    );
    let fw = line(&out, "Firmware:");
    assert!(fw.contains("2.0.6"), "{out}");
    assert!(fw.contains("2025.06.01"), "application date: {out}");
    let boot = line(&out, "Bootloader:");
    assert!(boot.contains("2.5"), "{out}");
    assert!(boot.contains("2024.01.01"), "bootloader date: {out}");
}

#[test]
fn info_json_has_mode_both_blocks_and_warnings() {
    let (out, _) = info(true, &[]);
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["mode"], "application");
    assert_eq!(v["application"]["model"], "KM003C");
    assert_eq!(v["application"]["version"], "2.0.6");
    assert_eq!(v["application"]["date"], "2025.06.01");
    assert_eq!(v["bootloader"]["model"], "KM003C");
    assert_eq!(v["bootloader"]["version"], "2.5");
    assert_eq!(v["bootloader"]["date"], "2024.01.01");
    assert_eq!(v["update_required"], false);
    assert_eq!(v["warnings"], serde_json::json!(["outdated"]));
    // The existing keys stay.
    assert_eq!(v["firmware_version"], "2.0.6");
    assert_eq!(v["serial"], "123456");
}

// ---- AC-V9SK52.4: warnings ----

#[test]
fn info_warns_when_firmware_is_older_than_the_newest_known() {
    let (out, err) = info(false, &[]);
    let w = warning_lines(&out);
    assert_eq!(w.len(), 1, "one warning: {out}");
    assert!(w[0].contains("2.0.7"), "names the newest version: {out}");
    assert_eq!(err, "", "warnings go to stdout, stderr stays empty");
}

#[test]
fn info_warns_when_firmware_is_older_than_1_7_0() {
    let (out, _) = info(false, &[("POWERZ_SIM_APP_VERSION", "1.6.0")]);
    assert!(line(&out, "Firmware:").contains("1.6.0"), "{out}");
    let w = warning_lines(&out);
    assert!(
        w.iter().any(|l| l.contains("1.7.0")),
        "a warning names 1.7.0: {out}"
    );
    let (json, _) = info(true, &[("POWERZ_SIM_APP_VERSION", "1.6.0")]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["warnings"], serde_json::json!(["too_old"]));
}

#[test]
fn info_warns_when_the_meter_reports_update_required() {
    let (out, _) = info(false, &[("POWERZ_SIM_HEAD", "1")]);
    let w = warning_lines(&out);
    assert!(
        w.iter()
            .any(|l| l.to_lowercase().contains("update required")),
        "an update-required warning: {out}"
    );
    let (json, _) = info(true, &[("POWERZ_SIM_HEAD", "1")]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["update_required"], true);
    let codes = v["warnings"].as_array().unwrap();
    assert!(codes.contains(&serde_json::json!("update_required")), "{v}");
}

#[test]
fn newest_firmware_prints_no_warning() {
    let (out, _) = info(false, &[("POWERZ_SIM_APP_VERSION", "2.0.7")]);
    assert_eq!(warning_lines(&out), Vec::<&str>::new(), "{out}");
    let (json, _) = info(true, &[("POWERZ_SIM_APP_VERSION", "2.0.7")]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["warnings"], serde_json::json!([]));
}

// ---- AC-V9SK52.2: one CONNECT per `info` run ----

#[test]
fn info_sends_exactly_one_connect() {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_CLOCK", "fake");
    c.timeout(Duration::from_secs(60));
    c.args([
        "--transport",
        "sim",
        "--scenario",
        "idle",
        "--trace",
        "info",
    ]);
    let out = c.assert().success().get_output().clone();
    let trace = String::from_utf8_lossy(&out.stderr).into_owned();
    let sent: Vec<String> = trace
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix('>'))
        .map(|h| h.chars().filter(|c| !c.is_whitespace()).collect::<String>())
        .map(|h| h.to_lowercase())
        .collect();
    assert!(!sent.is_empty(), "trace shows sent frames:\n{trace}");
    let connects = sent
        .iter()
        .filter(|h| h.len() == 8 && h.starts_with("02"))
        .count();
    assert_eq!(connects, 1, "one CONNECT in total:\n{trace}");
}
