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

// ---- AC-1HHT01.3: `powerz settings` against the simulator ----

#[test]
fn settings_against_the_simulator() {
    let a = sim("idle").arg("settings").assert().success();
    let out = stdout_of(&a);
    assert_eq!(stderr_of(&a), "", "no stderr on success");
    // The demo meter mirrors the captured FW 2.0.6 settings word.
    assert!(out.contains("English"), "{out}");
    assert!(out.contains("50%"), "{out}");
    assert!(out.contains("[confirmed]"), "screen-checked fields: {out}");
    assert!(
        out.contains("[inferred]"),
        "rotation and calibration: {out}"
    );
    insta::assert_snapshot!("settings_sim", out);
}

#[test]
fn settings_json_and_raw_against_the_simulator() {
    let a = sim("idle").args(["settings", "--json"]).assert().success();
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&a)).unwrap();
    let fields = v["fields"].as_array().expect("fields array");
    assert_eq!(fields.len(), 11);
    // Seven fields were confirmed on the meter's screen (1HHT04); rotation
    // and the three calibration fields stay inferred.
    let confirmed = fields.iter().filter(|f| f["tag"] == "confirmed").count();
    let inferred = fields.iter().filter(|f| f["tag"] == "inferred").count();
    assert_eq!((confirmed, inferred), (7, 4), "{v}");

    let a = sim("idle").args(["settings", "--raw"]).assert().success();
    let out = stdout_of(&a);
    let n: usize = out
        .lines()
        .map(|l| l.split_whitespace().skip(1).count())
        .sum();
    assert_eq!(n, 180, "{out}");
}

#[test]
fn settings_sends_no_write_against_the_simulator() {
    let a = sim("idle").args(["--trace", "settings"]).assert().success();
    let trace = stderr_of(&a);
    let sent: Vec<&str> = trace.lines().filter(|l| l.starts_with('>')).collect();
    assert!(!sent.is_empty(), "no frames traced:\n{trace}");
    for line in sent {
        let ty = traced_bytes(line).first().copied().unwrap_or(0xFF) & 0x7F;
        assert_ne!(ty, 0x0A, "settings write sent: {line}");
        assert_ne!(ty, 0x48, "SET_DATA sent: {line}");
    }
    assert!(!trace.lines().any(|l| l.starts_with('x')), "{trace}");
}

// ---- AC-1HHT00.4 / .6 / .7: `powerz records` against the simulator ----

#[test]
fn records_list_against_the_simulator() {
    let a = sim("idle").args(["records", "list"]).assert().success();
    let out = stdout_of(&a);
    assert_eq!(stderr_of(&a), "", "no stderr on success");
    let rows = out.lines().filter(|l| !l.trim().is_empty()).count();
    assert!(rows >= 4, "header + at least three demo records:\n{out}");
    insta::assert_snapshot!("records_list_sim", out);
}

#[test]
fn records_list_json_against_the_simulator() {
    let a = sim("idle")
        .args(["records", "list", "--json"])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&a)).unwrap();
    let recs = v["records"].as_array().expect("records array");
    assert!(recs.len() >= 3, "{v}");
    for (i, r) in recs.iter().enumerate() {
        assert_eq!(r["index"], i + 1, "{r}");
        assert!(r["name"].as_str().is_some_and(|n| !n.is_empty()), "{r}");
        assert!(r["points"].as_u64().is_some(), "{r}");
    }
}

#[test]
fn records_get_all_against_the_simulator_writes_csv_files() {
    let dir = std::env::temp_dir().join(format!("powerz-e2e-records-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let a = sim("idle")
        .args(["records", "get", "--all", "--json", "--out"])
        .arg(&dir)
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&a)).unwrap();
    let written = v["written"].as_array().expect("written array");
    assert!(written.len() >= 3, "{v}");
    for w in written {
        let path = PathBuf::from(w["path"].as_str().unwrap());
        assert_eq!(path.parent(), Some(dir.as_path()), "{w}");
        let csv = std::fs::read_to_string(&path).unwrap();
        let mut lines = csv.lines();
        assert_eq!(
            lines.next(),
            Some("ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR")
        );
        assert_eq!(lines.count() as u64, w["rows"].as_u64().unwrap(), "{w}");
    }
    // `get <n>` writes the same file for one record.
    let first = PathBuf::from(written[0]["path"].as_str().unwrap());
    let all_csv = std::fs::read_to_string(&first).unwrap();
    std::fs::remove_file(&first).unwrap();
    sim("idle")
        .args(["records", "get", "1", "--out"])
        .arg(&dir)
        .assert()
        .success();
    assert_eq!(std::fs::read_to_string(&first).unwrap(), all_csv);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn records_get_out_of_range_against_the_simulator_is_exit_2() {
    let dir = std::env::temp_dir().join(format!("powerz-e2e-records-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let a = sim("idle")
        .args(["records", "get", "99", "--out"])
        .arg(&dir)
        .assert()
        .code(2);
    let err = stderr_of(&a);
    assert!(
        err.contains("99"),
        "the error names the missing record: {err}"
    );
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn records_get_sends_no_write_against_the_simulator() {
    let dir = std::env::temp_dir().join(format!("powerz-e2e-records-tr-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let a = sim("idle")
        .args(["--trace", "records", "get", "--all", "--out"])
        .arg(&dir)
        .assert()
        .success();
    let trace = stderr_of(&a);
    let sent: Vec<&str> = trace.lines().filter(|l| l.starts_with('>')).collect();
    assert!(!sent.is_empty(), "no frames traced:\n{trace}");
    let mut get_data_record_list = 0;
    for line in sent {
        let bytes = traced_bytes(line);
        let ty = bytes.first().copied().unwrap_or(0xFF) & 0x7F;
        assert!(
            [0x02, 0x03, 0x0C, 0x40, 0x44, 0x4C].contains(&ty),
            "outside the read-only set: {line}"
        );
        if ty == 0x0C && bytes.get(2..4) == Some(&[0x00, 0x04]) {
            get_data_record_list += 1;
        }
    }
    assert_eq!(get_data_record_list, 1, "one RecordList read:\n{trace}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- AC-1HHT02.4 / .5: `powerz watch --source cdc` against the simulator ----
//
// With `--source cdc`, `POWERZ_SIM_CTRL_C_AFTER=N` stands in for Ctrl-C after
// N refreshes (the stream has no GET_DATA to count).

/// A watch line with its numbers (and a missing value's `-`) as `#`.
fn watch_shape(line: &str) -> String {
    line.split_whitespace()
        .map(|t| {
            if t == "-" || t.parse::<f64>().is_ok() {
                "#"
            } else {
                t
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The bytes of a `--trace` frame line `<marker> [<len>] <hex bytes>`;
/// the `[len]` prefix must match.
fn traced_bytes(line: &str) -> Vec<u8> {
    let mut tokens = line.split_whitespace().skip(1);
    let len: usize = tokens
        .next()
        .and_then(|t| t.strip_prefix('[')?.strip_suffix(']')?.parse().ok())
        .unwrap_or_else(|| panic!("no [len] after the marker: {line}"));
    let bytes: Vec<u8> = tokens
        .map(|t| u8::from_str_radix(t, 16).unwrap_or_else(|_| panic!("not hex: {line}")))
        .collect();
    assert_eq!(bytes.len(), len, "[len] counts the bytes: {line}");
    bytes
}

#[test]
fn watch_source_cdc_prints_the_polled_line_format() {
    let a = sim_fake("load-5v-1a")
        .env("POWERZ_SIM_CTRL_C_AFTER", "3")
        .args(["watch", "--source", "cdc", "--interval", "50"])
        .assert()
        .success();
    assert_eq!(stderr_of(&a), "");
    let out = stdout_of(&a);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "one line per refresh:\n{out}");

    let polled = sim_fake("load-5v-1a")
        .env("POWERZ_SIM_CTRL_C_AFTER", "1")
        .args(["watch", "--interval", "50"])
        .assert()
        .success();
    let polled_out = stdout_of(&polled);
    let polled_line = polled_out.lines().next().expect("a polled line");
    for l in &lines {
        assert_eq!(
            watch_shape(l),
            watch_shape(polled_line),
            "\ncdc:    {l}\npolled: {polled_line}"
        );
        let vbus: f64 = l
            .split_whitespace()
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("{l}"));
        near("VBUS", vbus, 5.0, 0.1);
    }
}

#[test]
fn watch_source_cdc_starts_and_stops_the_stream_with_adcnew_requests_only() {
    let a = sim_fake("load-5v-1a")
        .env("POWERZ_SIM_CTRL_C_AFTER", "3")
        .args(["--trace", "watch", "--source", "cdc", "--interval", "50"])
        .assert()
        .success();
    let trace = stderr_of(&a);
    let sent: Vec<Vec<u8>> = trace
        .lines()
        .filter(|l| l.starts_with('>'))
        .map(traced_bytes)
        .collect();
    assert!(!trace.lines().any(|l| l.starts_with('x')), "{trace}");
    assert_eq!(
        sent.first().and_then(|f| f.first()),
        Some(&0x02),
        "starts with ADCNEW start:\n{trace}"
    );
    assert_eq!(
        sent.last(),
        Some(&vec![0x03]),
        "ends with ADCNEW stop:\n{trace}"
    );
    for f in &sent {
        let ty = f.first().copied().unwrap_or(0xFF) & 0x7F;
        // AC-1HHT02.5: on CDC, 0x01..0x03 only as 1–2 byte ADCNEW requests;
        // no CONNECT, no queue start, no auth.
        if (0x01..=0x03).contains(&ty) {
            assert!(f.len() <= 2, "not an ADCNEW request: {f:02x?}\n{trace}");
        }
        assert!(
            ![0x0E, 0x0F, 0x4C].contains(&ty),
            "queue/auth request on the stream path: {f:02x?}\n{trace}"
        );
    }
}

#[test]
fn polled_watch_never_sends_adcnew_or_disconnect_types() {
    // AC-1HHT02.5: on bulk, 0x02 is CONNECT only; 0x01 and 0x03 are never sent.
    let a = sim_fake("load-5v-1a")
        .env("POWERZ_SIM_CTRL_C_AFTER", "3")
        .args(["--trace", "watch", "--interval", "50"])
        .assert()
        .success();
    let trace = stderr_of(&a);
    for f in trace
        .lines()
        .filter(|l| l.starts_with('>'))
        .map(traced_bytes)
    {
        let ty = f.first().copied().unwrap_or(0xFF) & 0x7F;
        assert!(ty != 0x01 && ty != 0x03, "{f:02x?}\n{trace}");
        if ty == 0x02 {
            assert_eq!(f.len(), 4, "0x02 only as CONNECT: {f:02x?}");
            assert_eq!(&f[2..], &[0x00, 0x00], "CONNECT: {f:02x?}");
        }
    }
}

// ---- 1HHT02 / 1HHT04: `powerz stream --source cdc` against the simulator ----

#[test]
fn stream_source_cdc_records_the_adcnew_stream_and_prints_the_summary() {
    let csv = temp_csv("stream-cdc-50.csv");
    let a = sim_fake("load-5v-1a")
        .args([
            "--trace",
            "stream",
            "--source",
            "cdc",
            "--rate",
            "50",
            "--duration",
            "2",
            "--csv",
        ])
        .arg(&csv)
        .assert()
        .success();
    let err = stderr_of(&a);
    let summary = err
        .lines()
        .find(|l| l.ends_with("CRC errors"))
        .unwrap_or_else(|| panic!("no summary line:\n{err}"));
    assert_eq!(
        summary,
        "100 records, 0 gaps, 0 dropped frames, 0 CRC errors"
    );
    // Only ADCNEW requests go out: start first, stop last; no CONNECT,
    // queue or auth.
    let sent: Vec<Vec<u8>> = err
        .lines()
        .filter(|l| l.starts_with('>'))
        .map(traced_bytes)
        .collect();
    let start = powerz_protocol::adcnew::Request::Start {
        content: powerz_protocol::adcnew::Content::Adc,
        rate: powerz_protocol::decode::Rate::Sps50,
    }
    .encode();
    assert_eq!(sent.first(), Some(&start), "{err}");
    assert_eq!(sent.last(), Some(&vec![0x03]), "{err}");
    assert!(sent.iter().all(|f| f.len() <= 2), "{err}");

    let text = std::fs::read_to_string(&csv).unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some(powerz_cli::record::CSV_HEADER));
    assert_eq!(lines.count(), 100);
    let _ = std::fs::remove_file(&csv);
}

#[test]
fn stream_source_cdc_counts_a_corrupt_frame() {
    let csv = temp_csv("stream-cdc-crc.csv");
    let a = sim_fake("load-5v-1a")
        .env("POWERZ_SIM_FAULT", "corrupt-adcnew@5")
        .args([
            "stream",
            "--source",
            "cdc",
            "--rate",
            "50",
            "--duration",
            "2",
            "--csv",
        ])
        .arg(&csv)
        .assert()
        .success();
    let err = stderr_of(&a);
    assert!(
        err.lines()
            .any(|l| l.ends_with(", 1 gaps, 0 dropped frames, 1 CRC errors")),
        "{err}"
    );
    let _ = std::fs::remove_file(&csv);
}
