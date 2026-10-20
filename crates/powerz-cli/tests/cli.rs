//! `assert_cmd` tests of the `powerz` binary (FR STQEA1): `--help` of every
//! subcommand, argument errors and exit code 2. None of these touch a meter:
//! argument checks run before any device access.

use assert_cmd::Command;
use predicates::prelude::*;

fn powerz() -> Command {
    Command::cargo_bin("powerz").expect("powerz binary is built")
}

// ---- AC-STQEA1.1–.7: --help documents every subcommand and flag ----

#[test]
fn top_level_help_lists_subcommands_and_global_flags() {
    powerz().arg("--help").assert().success().stdout(
        predicate::str::contains("list")
            .and(predicate::str::contains("info"))
            .and(predicate::str::contains("read"))
            .and(predicate::str::contains("watch"))
            .and(predicate::str::contains("stream"))
            .and(predicate::str::contains("--device"))
            .and(predicate::str::contains("--transport"))
            .and(predicate::str::contains("--trace")),
    );
}

#[test]
fn every_subcommand_has_help() {
    let cases: [(&str, &[&str]); 5] = [
        ("list", &["--json"]),
        ("info", &["--json"]),
        ("read", &["--json"]),
        ("watch", &["--interval"]),
        ("stream", &["--rate", "--csv", "--duration"]),
    ];
    for (sub, flags) in cases {
        let assert = powerz().args([sub, "--help"]).assert().success();
        let out = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
        for flag in flags {
            assert!(
                out.contains(flag),
                "`powerz {sub} --help` lacks {flag}:\n{out}"
            );
        }
    }
}

#[test]
fn transport_help_names_the_three_pipes() {
    powerz().args(["read", "--help"]).assert().success().stdout(
        predicate::str::contains("bulk")
            .and(predicate::str::contains("cdc"))
            .and(predicate::str::contains("hid")),
    );
}

// ---- AC-STQEA1.8: argument errors exit with code 2 ----

#[test]
fn no_subcommand_is_usage_error() {
    powerz().assert().code(2);
}

#[test]
fn unknown_subcommand_is_usage_error() {
    powerz().arg("frobnicate").assert().code(2);
}

#[test]
fn unknown_transport_is_usage_error() {
    powerz()
        .args(["--transport", "usb", "read"])
        .assert()
        .code(2);
}

#[test]
fn watch_interval_below_50_is_usage_error() {
    powerz()
        .args(["watch", "--interval", "49"])
        .assert()
        .code(2);
}

#[test]
fn stream_bad_rate_is_usage_error() {
    powerz()
        .args(["stream", "--rate", "7", "--csv", "never-written.csv"])
        .assert()
        .code(2);
}

#[test]
fn stream_without_csv_is_usage_error() {
    powerz().args(["stream", "--rate", "10"]).assert().code(2);
}

// ---- AC-STQEA1.6: hid only for read and watch, checked before any device ----

#[test]
fn hid_with_info_is_usage_error() {
    powerz()
        .args(["--transport", "hid", "info"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("hid"));
}

#[test]
fn hid_with_stream_is_usage_error_and_writes_no_file() {
    let dir = std::env::temp_dir().join(format!("powerz-cli-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("hid-stream.csv");
    let _ = std::fs::remove_file(&csv);
    powerz()
        .args(["--transport", "hid", "stream", "--rate", "10", "--csv"])
        .arg(&csv)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("hid"));
    assert!(!csv.exists(), "no CSV may be created on a usage error");
    let _ = std::fs::remove_dir_all(&dir);
}
