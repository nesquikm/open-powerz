//! FR BKSE70 (AC .4) in the CLI: `powerz firmware flash` and `firmware jump`
//! on a meter in the old (2.1) bootloader print the old-bootloader error (not
//! supported yet, unplug and replug to return to the application firmware,
//! the how-to link) and exit with the device error code 5.
//!
//! Every run uses `--transport sim` with `POWERZ_SIM_MODE=legacy-bootloader`
//! (test-only) on the fake sim clock; no real meter is opened. The container
//! is synthetic.

#[path = "../../powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use fw_container::*;

/// The how-to section the error links, written out independently.
const HOWTO_URL: &str = "https://github.com/nesquikm/open-powerz/blob/main/docs/how-to/update-firmware.md#old-bootloader";

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

/// `powerz --transport sim <args>` on a meter in the old bootloader.
fn on_old_bootloader(args: &[&OsStr]) -> (String, String, Option<i32>) {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_CLOCK", "fake");
    c.env("POWERZ_SIM_MODE", "legacy-bootloader");
    c.timeout(Duration::from_secs(60));
    c.args(["--transport", "sim"]);
    c.args(args);
    let out = c.output().expect("powerz runs");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code(),
    )
}

fn valid_file() -> PathBuf {
    let payload: Vec<u8> = (0..3 * 4080 + 100).map(|i| (i * 29 + 3) as u8).collect();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bkse70-flash-cli");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("old-bootloader.mencrypt");
    std::fs::write(
        &path,
        ContainerBuilder::new().payload_sized(&payload).build(),
    )
    .expect("write container");
    path
}

fn flash() -> (String, String, Option<i32>) {
    let path = valid_file();
    let args: Vec<OsString> = vec![
        "firmware".into(),
        "flash".into(),
        path.into_os_string(),
        "--i-understand".into(),
    ];
    let refs: Vec<&OsStr> = args.iter().map(|a| a.as_os_str()).collect();
    on_old_bootloader(&refs)
}

fn jump() -> (String, String, Option<i32>) {
    let args = ["firmware", "jump", "--to", "application", "--i-understand"];
    let refs: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    on_old_bootloader(&refs)
}

/// The old-bootloader error, by what it says.
fn assert_old_bootloader_message(what: &str, out: &str, err: &str) {
    let lower = err.to_lowercase();
    assert!(
        lower.contains("not supported yet"),
        "{what}: stderr does not say not supported yet:\n{err}\nstdout:\n{out}"
    );
    assert!(lower.contains("unplug"), "{what}: no unplug step:\n{err}");
    assert!(
        lower.contains("application firmware"),
        "{what}: does not name the application firmware:\n{err}"
    );
    assert!(err.contains(HOWTO_URL), "{what}: no how-to link:\n{err}");
}

#[test]
fn bkse70_cli_flash_on_the_old_bootloader_prints_the_old_bootloader_error() {
    let (out, err, _) = flash();
    assert_old_bootloader_message("flash", &out, &err);
}

#[test]
fn bkse70_cli_flash_on_the_old_bootloader_exits_with_the_device_code() {
    let (out, err, code) = flash();
    assert_eq!(code, Some(5), "stdout: {out}\nstderr: {err}");
}

#[test]
fn bkse70_cli_jump_on_the_old_bootloader_prints_the_old_bootloader_error() {
    let (out, err, _) = jump();
    assert_old_bootloader_message("jump", &out, &err);
}

#[test]
fn bkse70_cli_jump_on_the_old_bootloader_exits_with_the_device_code() {
    let (out, err, code) = jump();
    assert_eq!(code, Some(5), "stdout: {out}\nstderr: {err}");
}
