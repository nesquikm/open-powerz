//! End-to-end `powerz settings set <field>=<value> … --i-understand` against
//! the `powerz-sim` demo meter (FR JKWT3W, AC .3, .4, .5 and .6).
//!
//! Field names are the `powerz settings --json` keys of the writable fields;
//! values are written the way the settings are shown:
//! `language=chinese|english`, `brightness=1..100`,
//! `dim_timeout=off|15|30|60`, `theme=blue|green|pink|orange|yellow|dark-green|cyan`,
//! `rotation=0|90|180|270`, `sample_rate=2|10|50|1000`. No hardware is
//! touched.

use std::time::Duration;

use assert_cmd::Command;

fn sim() -> Command {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in [
        "POWERZ_SIM_CLOCK",
        "POWERZ_SIM_FAULT",
        "POWERZ_SIM_CTRL_C_AFTER",
    ] {
        c.env_remove(k);
    }
    c.timeout(Duration::from_secs(60));
    c.args(["--transport", "sim", "--scenario", "idle"]);
    c
}

fn stdout_of(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stdout).into_owned()
}

fn stderr_of(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stderr).into_owned()
}

/// Bytes of one `--trace` line (`> [len] hex …`).
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

/// Every frame of type `ty` the trace shows as sent.
fn sent_of_type(trace: &str, ty: u8) -> Vec<Vec<u8>> {
    trace
        .lines()
        .filter(|l| l.starts_with('>'))
        .map(traced_bytes)
        .filter(|b| b.first().map(|x| x & 0x7F) == Some(ty))
        .collect()
}

/// `(key, value)` entries of a type-0x0A frame.
fn entries(frame: &[u8]) -> Vec<(u16, i32)> {
    frame[4..]
        .chunks(8)
        .map(|e| {
            let key = u32::from_le_bytes([e[0], e[1], e[2], e[3]]) & 0x7FFF;
            (key as u16, i32::from_le_bytes([e[4], e[5], e[6], e[7]]))
        })
        .collect()
}

#[test]
fn set_writes_one_frame_and_prints_the_settings_read_back() {
    let a = sim()
        .args([
            "--trace",
            "settings",
            "set",
            "brightness=80",
            "theme=pink",
            "rotation=180",
            "--i-understand",
        ])
        .assert()
        .success();
    let out = stdout_of(&a);
    assert!(out.contains("80%"), "{out}");
    assert!(out.contains("pink"), "{out}");
    assert!(out.contains("180°"), "{out}");
    let writes = sent_of_type(&stderr_of(&a), 0x0A);
    assert_eq!(writes.len(), 1, "exactly one settings write");
    let e = entries(&writes[0]);
    let keys: Vec<u16> = e.iter().map(|(k, _)| *k).collect();
    assert_eq!(keys, vec![0x01, 0x03, 0x04, 0x06, 0x07, 0x14, 0x15, 0x7FFF]);
    let value = |k: u16| e.iter().find(|(key, _)| *key == k).unwrap().1;
    assert_eq!(value(0x03), 80);
    assert_eq!(value(0x07), 2);
    assert_eq!(value(0x14), 2, "rotation is key 0x14 [device]");
    // Unchanged ones at the demo meter's values: English, dim off, logo off.
    assert_eq!((value(0x01), value(0x04), value(0x06)), (1, 0, 0));
    assert_eq!(value(0x7FFF), 0x1234);
    // The settings block is read before and after the write.
    let reads = sent_of_type(&stderr_of(&a), 0x0C)
        .iter()
        .filter(|f| f.len() >= 4 && (u16::from_le_bytes([f[2], f[3]]) >> 1) & 0x008 != 0)
        .count();
    assert!(reads >= 2, "settings read before and after: {reads}");
}

/// The meter takes the settings write only on its CDC port: over bulk it
/// was silently ignored (2026-10-29) `[device]`. On CDC there is no CONNECT
/// (0x02 starts the ADCNEW stream there), so the trace shows only the two
/// SETTINGS reads and the write.
#[test]
fn set_goes_over_the_cdc_port_without_connect() {
    let a = sim()
        .args([
            "--trace",
            "settings",
            "set",
            "brightness=20",
            "--i-understand",
        ])
        .assert()
        .success();
    let trace = stderr_of(&a);
    let types: Vec<u8> = trace
        .lines()
        .filter(|l| l.starts_with('>'))
        .map(|l| traced_bytes(l)[0] & 0x7F)
        .collect();
    assert_eq!(types, vec![0x0C, 0x0A, 0x0C], "{trace}");
    assert!(stdout_of(&a).contains("20%"));
}

#[test]
fn set_takes_every_writable_field() {
    let a = sim()
        .args([
            "--trace",
            "settings",
            "set",
            "language=chinese",
            "dim_timeout=30",
            "sample_rate=10",
            "--i-understand",
        ])
        .assert()
        .success();
    let out = stdout_of(&a);
    assert!(out.contains("Chinese"), "{out}");
    assert!(out.contains("30 s"), "{out}");
    assert!(out.contains("10 SPS"), "{out}");
    let writes = sent_of_type(&stderr_of(&a), 0x0A);
    assert_eq!(writes.len(), 1);
    let e = entries(&writes[0]);
    let value = |k: u16| e.iter().find(|(key, _)| *key == k).unwrap().1;
    assert_eq!((value(0x01), value(0x04), value(0x15)), (0, 2, 1));
}

#[test]
fn set_without_i_understand_is_exit_2_and_sends_no_write() {
    let a = sim()
        .args(["--trace", "settings", "set", "brightness=80"])
        .assert()
        .code(2);
    let err = stderr_of(&a);
    assert!(err.contains("--i-understand"), "{err}");
    assert!(sent_of_type(&err, 0x0A).is_empty(), "{err}");
}

#[test]
fn set_refuses_fields_that_are_not_writable_with_exit_2() {
    for arg in [
        "voltage_cal=1",
        "current_cal=1",
        "offset_cal=0",
        "auto_rotate=off",
        "startup_logo=1",
        "otp=0",
        "factory_reset=1",
        "nonsense=3",
    ] {
        let a = sim()
            .args(["--trace", "settings", "set", arg, "--i-understand"])
            .assert()
            .code(2);
        let err = stderr_of(&a);
        let field = arg.split('=').next().unwrap();
        assert!(err.contains(field), "{arg}: {err}");
        assert!(sent_of_type(&err, 0x0A).is_empty(), "{arg}: {err}");
    }
}

#[test]
fn set_refuses_values_out_of_range_with_exit_2() {
    for arg in [
        "brightness=0",
        "brightness=101",
        "brightness=-5",
        "brightness=lots",
        "language=klingon",
        "dim_timeout=45",
        "theme=purple",
        "rotation=45",
        "sample_rate=1001",
        "brightness",
        "=80",
    ] {
        let a = sim()
            .args(["--trace", "settings", "set", arg, "--i-understand"])
            .assert()
            .code(2);
        assert!(
            sent_of_type(&stderr_of(&a), 0x0A).is_empty(),
            "{arg}: {}",
            stderr_of(&a)
        );
    }
}

#[test]
fn set_without_any_field_is_a_usage_error() {
    let a = sim()
        .args(["--trace", "settings", "set", "--i-understand"])
        .assert()
        .code(2);
    assert!(sent_of_type(&stderr_of(&a), 0x0A).is_empty());
}

#[test]
fn a_mismatching_re_read_is_exit_5_and_names_the_field() {
    let a = sim()
        .env("POWERZ_SIM_FAULT", "ignore-settings-write")
        .args([
            "--trace",
            "settings",
            "set",
            "brightness=80",
            "--i-understand",
        ])
        .assert()
        .code(5);
    let err = stderr_of(&a);
    assert!(err.contains("brightness"), "{err}");
    assert_eq!(sent_of_type(&err, 0x0A).len(), 1, "not retried: {err}");
}

#[test]
fn plain_settings_still_reads_only() {
    let a = sim().args(["--trace", "settings"]).assert().success();
    assert!(sent_of_type(&stderr_of(&a), 0x0A).is_empty());
    assert!(stdout_of(&a).contains("Brightness"));
}

#[test]
fn settings_help_lists_set() {
    let a = sim().args(["settings", "--help"]).assert().success();
    let out = stdout_of(&a);
    assert!(
        out.lines().any(|l| l.trim_start().starts_with("set ")),
        "a `set` subcommand line: {out}"
    );
}
