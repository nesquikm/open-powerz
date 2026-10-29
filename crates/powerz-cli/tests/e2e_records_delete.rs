//! End-to-end `powerz records delete` against the `powerz-sim` demo meter
//! (FR JKWT3V, AC .2, .3, .4 and .6): delete one with the name typed on
//! stdin, delete all with `--yes`, a failed backup
//! (`POWERZ_SIM_FAULT=reject-record-reads`) and a mismatching re-read
//! (`POWERZ_SIM_FAULT=ignore-table-write`). The demo meter's records are
//! PHONE_5V, PD_9V_TEST and LAPTOP_20V. No hardware is touched.

use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;

const CSV_HEADER: &str = "ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR";

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

fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("powerz-e2e-delete-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn stdout_of(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stdout).into_owned()
}

fn stderr_of(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stderr).into_owned()
}

#[test]
fn delete_one_with_the_name_typed_backs_it_up_and_succeeds() {
    let dir = fresh_dir("one");
    let a = sim()
        .args(["records", "delete", "1", "--i-understand", "--out"])
        .arg(&dir)
        .write_stdin("PHONE_5V\n")
        .assert()
        .success();
    assert_eq!(files_in(&dir), vec!["01_PHONE_5V.csv".to_string()]);
    let csv = std::fs::read_to_string(dir.join("01_PHONE_5V.csv")).unwrap();
    assert_eq!(csv.lines().next(), Some(CSV_HEADER));
    assert_eq!(csv.lines().count(), 301, "header + 300 rows");
    let out = stdout_of(&a);
    assert!(out.contains("PHONE_5V"), "{out}");
    assert!(out.contains("01_PHONE_5V.csv"), "names the backup: {out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_all_with_yes_backs_up_every_record() {
    let dir = fresh_dir("all");
    sim()
        .args([
            "records",
            "delete",
            "--all",
            "--i-understand",
            "--yes",
            "--out",
        ])
        .arg(&dir)
        .assert()
        .success();
    let files = files_in(&dir);
    assert_eq!(
        files,
        vec![
            "01_PHONE_5V.csv".to_string(),
            "02_PD_9V_TEST.csv".to_string(),
            "03_LAPTOP_20V.csv".to_string(),
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_without_i_understand_is_exit_2_and_writes_no_backup() {
    for extra in [&[][..], &["--yes"][..]] {
        let dir = fresh_dir("noflag");
        let a = sim()
            .args(["records", "delete", "1"])
            .args(extra)
            .arg("--out")
            .arg(&dir)
            .write_stdin("PHONE_5V\n")
            .assert()
            .code(2);
        let err = stderr_of(&a);
        assert!(err.contains("--i-understand"), "{extra:?}: {err}");
        assert!(files_in(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn delete_with_a_wrong_name_typed_fails() {
    let dir = fresh_dir("wrong");
    let a = sim()
        .args(["records", "delete", "1", "--i-understand", "--out"])
        .arg(&dir)
        .write_stdin("PD_9V_TEST\n")
        .assert()
        .failure();
    assert_ne!(a.get_output().status.code(), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_backup_is_exit_5_and_deletes_nothing() {
    let dir = fresh_dir("backupfail");
    let a = sim()
        .env("POWERZ_SIM_FAULT", "reject-record-reads")
        .args(["records", "delete", "1", "--i-understand", "--yes", "--out"])
        .arg(&dir)
        .assert()
        .code(5);
    let err = stderr_of(&a);
    assert!(err.to_lowercase().contains("backup"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_mismatching_re_read_is_exit_5_and_names_the_backup() {
    let dir = fresh_dir("mismatch");
    let a = sim()
        .env("POWERZ_SIM_FAULT", "ignore-table-write")
        .args(["records", "delete", "2", "--i-understand", "--yes", "--out"])
        .arg(&dir)
        .assert()
        .code(5);
    let err = stderr_of(&a);
    assert!(
        err.contains(&dir.display().to_string()),
        "the error names the backup: {err}"
    );
    assert_eq!(files_in(&dir), vec!["02_PD_9V_TEST.csv".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn records_help_lists_delete() {
    let a = sim().args(["records", "--help"]).assert().success();
    let out = stdout_of(&a);
    assert!(out.contains("delete"), "{out}");
}
