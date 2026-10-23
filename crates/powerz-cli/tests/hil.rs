//! Hardware-in-the-loop suite (FR G0W08Z, AC .12): `list`, `info`, `read`,
//! `read --json` and a 5-second `stream --rate 50` against the attached
//! meter over bulk. Opt-in: every `hil_*` test returns at once unless
//! `POWERZ_HIL=1`; CI never sets it. The flows use only the safe command set
//! of `docs/protocol.md` (the CLI sends nothing else).
//!
//! The same flows also run in the normal gate against the simulator
//! (`hil_flows_dry_run_against_the_simulator`), so the suite cannot rot
//! between hardware sessions.

use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;

/// Where a flow runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// The attached meter, default (bulk) transport.
    Meter,
    /// The demo device on the simulated clock.
    Sim,
}

fn hil_enabled() -> bool {
    std::env::var("POWERZ_HIL").as_deref() == Ok("1")
}

/// Skip unless `POWERZ_HIL=1`; returns `true` when the test should run.
fn hil_or_skip(name: &str) -> bool {
    if hil_enabled() {
        true
    } else {
        eprintln!("{name}: skipped (set POWERZ_HIL=1 with a meter attached)");
        false
    }
}

fn powerz(target: Target) -> Command {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in [
        "POWERZ_SIM_CLOCK",
        "POWERZ_SIM_FAULT",
        "POWERZ_SIM_CTRL_C_AFTER",
        "POWERZ_SIM_CHARGER",
    ] {
        c.env_remove(k);
    }
    c.timeout(Duration::from_secs(60));
    match target {
        Target::Meter => {
            c.args(["--transport", "bulk"]);
        }
        Target::Sim => {
            c.env("POWERZ_SIM_CLOCK", "fake");
            c.args(["--transport", "sim", "--scenario", "load-5v-1a"]);
        }
    }
    c
}

fn stdout(c: &mut Command) -> String {
    let a = c.assert().success();
    String::from_utf8_lossy(&a.get_output().stdout).into_owned()
}

fn flow_list(target: Target) {
    let mut c = powerz(target);
    c.arg("list");
    if target == Target::Sim {
        c.arg("--include-demo");
    }
    let out = stdout(&mut c);
    assert!(
        out.lines()
            .any(|l| l.contains("KM003C") || l.contains("KM002C")),
        "no meter listed:\n{out}"
    );
}

fn flow_info(target: Target) {
    let out = stdout(powerz(target).arg("info"));
    for label in ["Model:", "Firmware:", "Hardware:", "Serial:", "UID:"] {
        assert!(out.contains(label), "info lacks {label}:\n{out}");
    }
}

fn flow_read(target: Target) {
    let out = stdout(powerz(target).arg("read"));
    for label in ["VBUS:", "IBUS:", "Power:", "Temp:", "CC1:", "CC2:"] {
        assert!(out.contains(label), "read lacks {label}:\n{out}");
    }
}

fn flow_read_json(target: Target) {
    let out = stdout(powerz(target).args(["read", "--json"]));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    for k in [
        "vbus_v", "ibus_a", "power_w", "dp_v", "dm_v", "cc1_v", "cc2_v",
    ] {
        assert!(v[k].is_number(), "{k} missing: {v}");
    }
    let vbus = v["vbus_v"].as_f64().unwrap();
    assert!((-1.0..=50.0).contains(&vbus), "implausible VBUS {vbus}");
}

fn csv_path(dir: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    p
}

fn flow_stream_5s(target: Target) {
    let dir = std::env::temp_dir().join(format!("powerz-hil-{}", std::process::id()));
    let csv = csv_path(&dir, &format!("stream-{target:?}.csv"));
    powerz(target)
        .args(["stream", "--rate", "50", "--duration", "5", "--csv"])
        .arg(&csv)
        .assert()
        .success();
    let text = std::fs::read_to_string(&csv).unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some(powerz_cli::record::CSV_HEADER));
    let rows = lines.count() as f64;
    assert!(
        (237.5..=262.5).contains(&rows),
        "{rows} rows, want 250 ± 5% (50 SPS × 5 s)"
    );
    let _ = std::fs::remove_file(&csv);
}

/// AC-DGSQ6P.7 ladder: 9, 15, 20, 28 V, AVS 15 V, then a hard reset to
/// 5 V; each delivered voltage within 5 % of the request.
fn flow_trigger_pd_ladder(target: Target) {
    let within = |got: u64, want: u64| got.abs_diff(want) * 20 <= want;
    for (args, want_mv) in [
        (vec!["2"], 9000),
        (vec!["3"], 15_000),
        (vec!["4"], 20_000),
        (vec!["8"], 28_000),
        (
            vec!["9", "--voltage-mv", "15000", "--current-ma", "1000"],
            15_000,
        ),
    ] {
        let mut c = powerz(target);
        c.args(["trigger", "pd", "request"]).args(&args).args([
            "--allow-high-voltage",
            "--hold",
            "0",
            "--json",
        ]);
        let v: serde_json::Value = serde_json::from_str(&stdout(&mut c)).unwrap();
        let got = v["delivered_mv"].as_u64().expect("delivered_mv");
        eprintln!("trigger pd request {args:?}: delivered {got} mV (want {want_mv})");
        assert!(
            within(got, want_mv),
            "{args:?}: {got} mV, want {want_mv} ± 5%"
        );
    }
    let out = stdout(powerz(target).args(["trigger", "pd", "reset", "--json"]));
    eprintln!("trigger pd reset: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let got = v["vbus_mv"].as_u64().expect("vbus_mv");
    assert!(within(got, 5000), "reset: {got} mV, want 5000 ± 5%");
}

// ---- AC-DGSQ6P.7: opt-in, real meter, 140 W charger, nothing on the output ----

/// Raises VBUS up to 28 V: needs `POWERZ_HIL=1` and `POWERZ_HIL_PD=1`, and
/// the wall-charger setup (laptop on the data port, output empty).
#[test]
fn hil_trigger_pd_ladder() {
    if hil_or_skip("hil_trigger_pd_ladder") && std::env::var("POWERZ_HIL_PD").as_deref() == Ok("1")
    {
        flow_trigger_pd_ladder(Target::Meter);
    }
}

#[test]
fn hil_trigger_pd_ladder_dry_run_against_the_simulator() {
    flow_trigger_pd_ladder(Target::Sim);
}

/// AC-DGSQ6R.6 flow: `detect --list --json`; BC1.2 / Apple results and
/// every list entry carry a known support value, and the hint has D+ / D-.
fn flow_detect(target: Target) {
    let out = stdout(powerz(target).args(["detect", "--list", "--json"]));
    eprintln!("detect --list: {out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let known = ["supported", "not-supported", "unknown"];
    for key in ["bc12", "apple"] {
        let r = v[key].as_str().unwrap_or_default();
        assert!(known.contains(&r), "{key}: {r:?}");
    }
    let entries = v["list"]["entries"].as_array().expect("list entries");
    for e in entries {
        let r = e["support"].as_str().unwrap_or_default();
        assert!(known.contains(&r), "{e}");
    }
    assert!(v["list"]["complete"].is_boolean(), "{v}");
    assert!(
        v["hint"]["dp_mv"].is_u64() && v["hint"]["dm_mv"].is_u64(),
        "{v}"
    );
}

// ---- AC-DGSQ6R.6: opt-in, real meter, each charger, nothing on the output ----

/// Detection only (at most 5 V): needs `POWERZ_HIL=1` and
/// `POWERZ_HIL_DETECT=1`, a charger on the input and nothing on the output.
#[test]
fn hil_detect() {
    if hil_or_skip("hil_detect") && std::env::var("POWERZ_HIL_DETECT").as_deref() == Ok("1") {
        flow_detect(Target::Meter);
    }
}

#[test]
fn hil_detect_dry_run_against_the_simulator() {
    flow_detect(Target::Sim);
}

// ---- AC-G0W08Z.12: opt-in, real meter over bulk ----

#[test]
fn hil_list() {
    if hil_or_skip("hil_list") {
        flow_list(Target::Meter);
    }
}

#[test]
fn hil_info() {
    if hil_or_skip("hil_info") {
        flow_info(Target::Meter);
    }
}

#[test]
fn hil_read() {
    if hil_or_skip("hil_read") {
        flow_read(Target::Meter);
    }
}

#[test]
fn hil_read_json() {
    if hil_or_skip("hil_read_json") {
        flow_read_json(Target::Meter);
    }
}

#[test]
fn hil_stream_5s_at_50_sps() {
    if hil_or_skip("hil_stream_5s_at_50_sps") {
        flow_stream_5s(Target::Meter);
    }
}

// ---- the same flows, in the normal gate, against the demo device ----

#[test]
fn hil_flows_dry_run_against_the_simulator() {
    flow_list(Target::Sim);
    flow_info(Target::Sim);
    flow_read(Target::Sim);
    flow_read_json(Target::Sim);
    flow_stream_5s(Target::Sim);
}
