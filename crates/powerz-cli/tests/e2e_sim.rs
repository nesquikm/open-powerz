//! End-to-end tests of the real `powerz` binary against the `powerz-sim`
//! demo device (FR G0W08Z, AC .9–.11). They run in the normal gate on Linux,
//! macOS and Windows; no hardware is touched.
//!
//! Test-only environment knobs the CLI honours for `--transport sim`:
//!
//! - `POWERZ_SIM_CLOCK=fake`: the simulator and the session share a
//!   simulated clock starting at a fixed epoch; session sleeps advance it
//!   instantly, so output is deterministic and streams finish at once.
//! - `POWERZ_SIM_FAULT=<plan>`: inject faults, in `FaultPlan::parse` syntax
//!   (`timeout`, `timeout@N`, `disconnect@N`, `wrong-id@N`, `permission`).
//! - `POWERZ_SIM_CTRL_C_AFTER=N`: stand in for Ctrl-C once the simulator has
//!   answered N GET_DATA requests (bounds `watch`, exit code 0).

use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use predicates::prelude::*;

const ENV_KNOBS: [&str; 3] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
];

fn powerz() -> Command {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.timeout(Duration::from_secs(60));
    c
}

/// `powerz --transport sim --scenario <scenario>` on real time.
fn sim(scenario: &str) -> Command {
    let mut c = powerz();
    c.args(["--transport", "sim", "--scenario", scenario]);
    c
}

/// As [`sim`], on the simulated clock.
fn sim_fake(scenario: &str) -> Command {
    let mut c = sim(scenario);
    c.env("POWERZ_SIM_CLOCK", "fake");
    c
}

fn stdout_of(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stdout).into_owned()
}

fn stderr_of(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stderr).into_owned()
}

fn temp_csv(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("powerz-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

fn near(what: &str, got: f64, want: f64, tol: f64) {
    assert!(
        (got - want).abs() <= tol,
        "{what}: got {got}, want {want} ± {tol}"
    );
}

// ---- AC-G0W08Z.9: --transport sim, --scenario, list --include-demo ----

#[test]
fn help_documents_the_demo_device_and_scenario_flag() {
    let a = powerz().arg("--help").assert().success();
    let out = stdout_of(&a);
    assert!(
        out.contains("sim"),
        "--help lacks the sim transport:\n{out}"
    );
    assert!(
        out.contains("demo device"),
        "--help lacks 'demo device':\n{out}"
    );
    assert!(
        out.contains("--scenario"),
        "--help lacks --scenario:\n{out}"
    );
    for name in ["idle", "load-5v-1a", "pd-9v-2a", "noisy-negative"] {
        assert!(out.contains(name), "--help lacks scenario {name}:\n{out}");
    }
    let a = powerz().args(["read", "--help"]).assert().success();
    let out = stdout_of(&a);
    assert!(out.contains("--scenario") && out.contains("sim"), "{out}");
    let a = powerz().args(["list", "--help"]).assert().success();
    assert!(stdout_of(&a).contains("--include-demo"));
}

#[test]
fn scenario_defaults_to_idle() {
    let a = powerz()
        .env("POWERZ_SIM_CLOCK", "fake")
        .args(["--transport", "sim", "read", "--json"])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&a)).unwrap();
    let vbus = v["vbus_v"].as_f64().unwrap();
    let ibus = v["ibus_a"].as_f64().unwrap();
    assert!(vbus.abs() < 0.05, "idle VBUS {vbus}");
    assert!(ibus.abs() < 0.01, "idle IBUS {ibus}");
}

#[test]
fn list_include_demo_lists_the_simulator() {
    let a = powerz().args(["list", "--include-demo"]).assert().success();
    let out = stdout_of(&a);
    let line = out
        .lines()
        .find(|l| l.contains("123456"))
        .unwrap_or_else(|| panic!("no simulator line:\n{out}"));
    assert!(line.contains("KM003C"), "{line}");
    assert!(line.to_lowercase().contains("demo"), "{line}");
}

#[test]
fn list_include_demo_json_has_the_simulator_entry() {
    let a = powerz()
        .args(["list", "--include-demo", "--json"])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&a)).unwrap();
    let arr = v.as_array().expect("JSON array");
    let sim = arr
        .iter()
        .find(|d| d["serial"] == "123456")
        .unwrap_or_else(|| panic!("no simulator entry: {v}"));
    assert_eq!(sim["model"], "KM003C");
}

// ---- AC-G0W08Z.10: real binary against the simulator ----

#[test]
fn info_against_the_simulator() {
    let a = sim("idle").arg("info").assert().success();
    let out = stdout_of(&a);
    assert_eq!(stderr_of(&a), "", "no stderr on success");
    assert!(out.contains("KM003C"), "{out}");
    assert!(out.contains("2.0.6"), "{out}");
    assert!(out.contains("123456"), "{out}");
    insta::assert_snapshot!("info_sim", out);
}

#[test]
fn info_json_against_the_simulator() {
    let a = sim("idle").args(["info", "--json"]).assert().success();
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&a)).unwrap();
    assert_eq!(v["model"], "KM003C");
    assert_eq!(v["firmware_version"], "2.0.6");
    assert_eq!(v["serial"], "123456");
    assert_eq!(v["uid"].as_str().unwrap().len(), 24);
}

#[test]
fn read_against_the_simulator() {
    let a = sim_fake("load-5v-1a").arg("read").assert().success();
    let out = stdout_of(&a);
    assert_eq!(stderr_of(&a), "");
    let vbus_line = out
        .lines()
        .find(|l| l.starts_with("VBUS:"))
        .expect("VBUS line");
    let vbus: f64 = vbus_line
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    near("VBUS", vbus, 5.0, 0.25);
    insta::assert_snapshot!("read_sim_load_5v_1a", out);
}

#[test]
fn read_is_deterministic_on_the_simulated_clock() {
    let first = stdout_of(&sim_fake("noisy-negative").arg("read").assert().success());
    let second = stdout_of(&sim_fake("noisy-negative").arg("read").assert().success());
    assert_eq!(first, second);
}

#[test]
fn read_json_against_the_simulator() {
    let a = sim("pd-9v-2a").args(["read", "--json"]).assert().success();
    assert_eq!(stderr_of(&a), "");
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&a)).unwrap();
    let f = |k: &str| v[k].as_f64().unwrap_or_else(|| panic!("{k} missing: {v}"));
    near("vbus_v", f("vbus_v"), 9.0, 0.45);
    near("ibus_a", f("ibus_a"), 2.0, 0.1);
    near("power_w", f("power_w"), 18.0, 1.8);
    let temp = f("temp_c");
    assert!((25.0..=35.0).contains(&temp), "temp_c {temp}");
    for k in ["dp_v", "dm_v", "cc1_v", "cc2_v"] {
        f(k);
    }
}

#[test]
fn bounded_watch_against_the_simulator() {
    let a = sim_fake("load-5v-1a")
        .env("POWERZ_SIM_CTRL_C_AFTER", "3")
        .args(["watch", "--interval", "50"])
        .assert()
        .success();
    assert_eq!(stderr_of(&a), "");
    let out = stdout_of(&a);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "one line per refresh (stdout is a pipe):\n{out}"
    );
    for l in lines {
        assert!(l.starts_with("VBUS "), "{l}");
        assert!(l.contains("IBUS ") && l.contains("CC2 "), "{l}");
    }
}

#[test]
fn stream_50_sps_for_2_seconds_against_the_simulator() {
    let csv = temp_csv("stream-50.csv");
    let a = sim("load-5v-1a")
        .args(["stream", "--rate", "50", "--duration", "2", "--csv"])
        .arg(&csv)
        .assert()
        .success();
    let err = stderr_of(&a);
    assert!(err.contains("samples recorded"), "stderr: {err}");
    assert!(!err.contains("gap"), "no gaps at 50 SPS: {err}");

    let text = std::fs::read_to_string(&csv).unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some(powerz_cli::record::CSV_HEADER));
    let rows: Vec<Vec<&str>> = lines.map(|l| l.split(',').collect()).collect();
    let n = rows.len() as f64;
    assert!(
        (95.0..=105.0).contains(&n),
        "{n} rows, want 100 ± 5% (50 SPS × 2 s)"
    );
    assert!(
        err.contains(&format!("{} samples recorded", rows.len())),
        "{err}"
    );
    // ENERGY (Wh) is column 10; the scenario draws 5 V × 1 A = 5 W.
    let energy: f64 = rows.last().unwrap()[10].parse().unwrap();
    let want = 5.0 * 2.0 / 3600.0;
    assert!(
        (energy - want).abs() <= 0.05 * want,
        "integrated energy {energy} Wh, want {want} Wh ± 5%"
    );
    let _ = std::fs::remove_file(&csv);
}

// ---- AC-G0W08Z.11: exit codes 2, 3, 4, 5 end to end ----

#[test]
fn unknown_scenario_is_usage_error_exit_2() {
    sim("no-such-scenario")
        .arg("read")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no-such-scenario"));
}

#[test]
fn unmatched_device_serial_is_no_device_exit_3() {
    sim("idle")
        .args(["--device", "999999", "read"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("999999"));
}

#[test]
fn permission_fault_is_exit_4() {
    sim("idle")
        .env("POWERZ_SIM_FAULT", "permission")
        .arg("read")
        .assert()
        .code(4)
        .stderr(predicate::str::contains("permission denied"));
}

#[test]
fn timeout_fault_is_device_error_exit_5() {
    sim("idle")
        .env("POWERZ_SIM_FAULT", "timeout")
        .arg("read")
        .assert()
        .code(5)
        .stderr(predicate::str::contains("timed out"));
}

#[test]
fn disconnect_mid_stream_is_device_error_exit_5() {
    let csv = temp_csv("stream-unplug.csv");
    sim_fake("load-5v-1a")
        .env("POWERZ_SIM_FAULT", "disconnect@3")
        .args(["stream", "--rate", "50", "--csv"])
        .arg(&csv)
        .assert()
        .code(5)
        .stderr(predicate::str::contains("disconnected"));
    let _ = std::fs::remove_file(&csv);
}
