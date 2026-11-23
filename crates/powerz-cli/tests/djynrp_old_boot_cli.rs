//! FR DJYNRP in the CLI: `powerz firmware flash FILE --i-understand
//! --experimental-old-bootloader` on the simulated old bootloader that
//! numbers its own replies (`POWERZ_SIM_MODE=legacy-bootloader-own-ids`,
//! test-only).
//!
//! - AC .1: the flag exists, and with it the flash runs end to end (traced
//!   with `--trace`) and prints the new version.
//! - AC .2: without it the flash is refused with exit 5 before any frame,
//!   naming the flag and calling the path experimental; `jump` still
//!   refuses the old bootloader.
//! - AC .3: with it a warning comes before any frame: tested only in
//!   simulation, open-powerz never sends a write below 0x4000 (FR NCH7DW
//!   wording), a failed run is retried by holding Back while plugging in.
//! - AC .6: the risk acknowledgement, the file checks and the model check
//!   still apply.
//!
//! Every run uses `--transport sim` on the fake sim clock; no real meter is
//! opened. Every container is synthetic.

#[path = "../../powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use clap::Parser;
use fw_container::*;
use powerz_cli::Cli;

const FLAG: &str = "--experimental-old-bootloader";
const ACK: &str = "--i-understand";
/// The how-to section the old-bootloader error links.
const HOWTO_URL: &str = "https://github.com/nesquikm/open-powerz/blob/main/docs/how-to/update-firmware.md#old-bootloader";

const ENV_KNOBS: [&str; 9] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
    "POWERZ_SIM_HEAD",
    "POWERZ_SIM_APP_VERSION",
    "POWERZ_SIM_MODE",
    "POWERZ_SIM_FLASH_FAULT",
    "POWERZ_SIM_REPLUG_AFTER",
    "POWERZ_SIM_PROBE_ANSWER",
];

const OWN_IDS: &str = "legacy-bootloader-own-ids";

fn write_file(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("djynrp-flash-cli");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write container");
    path
}

fn valid_file(name: &str) -> PathBuf {
    let payload: Vec<u8> = (0..3 * 4080 + 100).map(|i| (i * 29 + 3) as u8).collect();
    write_file(
        name,
        &ContainerBuilder::new().payload_sized(&payload).build(),
    )
}

struct Run {
    out: String,
    err: String,
    code: Option<i32>,
}

impl Run {
    fn all(&self) -> String {
        format!("{}{}", self.out, self.err)
    }

    /// The frames sent, as traced (`> [len] bytes…`).
    fn sent(&self) -> Vec<&str> {
        self.err.lines().filter(|l| l.starts_with('>')).collect()
    }

    /// The type byte of each traced frame sent.
    fn sent_types(&self) -> Vec<String> {
        self.sent()
            .iter()
            .map(|l| l.split(' ').nth(2).unwrap_or("").to_owned())
            .collect()
    }
}

/// `powerz --trace --transport sim <args>` with `POWERZ_SIM_MODE=mode`.
fn powerz(mode: &str, args: &[OsString]) -> Run {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_CLOCK", "fake");
    c.env("POWERZ_SIM_MODE", mode);
    c.timeout(Duration::from_secs(60));
    c.args(["--trace", "--transport", "sim"]);
    c.args(args);
    let out = c.output().expect("powerz runs");
    Run {
        out: String::from_utf8_lossy(&out.stdout).into_owned(),
        err: String::from_utf8_lossy(&out.stderr).into_owned(),
        code: out.status.code(),
    }
}

fn flash_args(file: PathBuf, extra: &[&str]) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec!["firmware".into(), "flash".into(), file.into_os_string()];
    a.extend(extra.iter().map(OsString::from));
    a
}

fn flash_with_flag(name: &str) -> Run {
    powerz(OWN_IDS, &flash_args(valid_file(name), &[ACK, FLAG]))
}

fn flash_without_flag(name: &str) -> Run {
    powerz(OWN_IDS, &flash_args(valid_file(name), &[ACK]))
}

/// Markdown-free, lowercase.
fn plain(s: &str) -> String {
    s.replace(['*', '`'], "").to_lowercase()
}

// ---------------------------------------------------------------- AC-DJYNRP.1

#[test]
fn djynrp_cli_flash_takes_the_experimental_old_bootloader_flag() {
    let r = Cli::try_parse_from(["powerz", "firmware", "flash", "fw.mencrypt", ACK, FLAG]);
    assert!(
        r.is_ok(),
        "the flag is refused: {}",
        r.err().map(|e| e.to_string()).unwrap_or_default()
    );
}

#[test]
fn djynrp_cli_flash_with_the_flag_on_the_own_ids_old_bootloader_succeeds() {
    let r = flash_with_flag("ok.mencrypt");
    assert_eq!(r.code, Some(0), "stdout:\n{}\nstderr:\n{}", r.out, r.err);
    assert!(
        r.out.contains("2.0.7"),
        "the new version is shown:\n{}",
        r.out
    );
}

#[test]
fn djynrp_cli_flash_with_the_flag_traces_the_whole_bootloader_sequence() {
    let r = flash_with_flag("trace.mencrypt");
    let types = r.sent_types();
    let at = |ty: &str| {
        types
            .iter()
            .position(|t| t == ty)
            .unwrap_or_else(|| panic!("no {ty} sent: {types:?}\nstderr:\n{}", r.err))
    };
    let (connect, head, desc, aprom) = (at("02"), at("40"), at("42"), at("08"));
    assert!(
        connect < head && head < desc && desc < aprom,
        "CONNECT, HEAD, descriptor, JUMP_APROM: {types:?}"
    );
    let payload = r.sent().iter().filter(|l| l.len() > 3 * 4000).count();
    // 3 full 4080-byte blocks; the 100-byte tail traces short.
    assert!(payload >= 3, "every full payload write traced: {payload}");
    let after = &types[aprom + 1..];
    assert!(
        after.iter().any(|t| t == "02") && after.iter().any(|t| t == "44"),
        "the version check after the return: {types:?}"
    );
}

// ---------------------------------------------------------------- AC-DJYNRP.2

#[test]
fn djynrp_cli_without_the_flag_the_old_bootloader_is_refused_with_exit_5_and_no_frame() {
    let r = flash_without_flag("no-flag.mencrypt");
    assert_eq!(r.code, Some(5), "stderr:\n{}", r.err);
    assert!(r.sent().is_empty(), "nothing sent: {:?}", r.sent());
}

#[test]
fn djynrp_cli_the_refusal_names_the_flag() {
    let r = flash_without_flag("names-flag.mencrypt");
    assert!(
        r.err.contains(FLAG),
        "the refusal does not name {FLAG}:\n{}",
        r.err
    );
}

#[test]
fn djynrp_cli_the_refusal_says_the_path_is_experimental() {
    let r = flash_without_flag("experimental.mencrypt");
    let rest = r.err.replace(FLAG, "").to_lowercase();
    assert!(
        rest.contains("experimental"),
        "not called experimental:\n{}",
        r.err
    );
}

#[test]
fn djynrp_cli_jump_still_refuses_the_own_ids_old_bootloader() {
    let args: Vec<OsString> = ["firmware", "jump", "--to", "application", ACK]
        .iter()
        .map(OsString::from)
        .collect();
    let r = powerz(OWN_IDS, &args);
    assert_eq!(r.code, Some(5), "stderr:\n{}", r.err);
    assert!(
        r.err.contains(HOWTO_URL),
        "the old-bootloader error:\n{}",
        r.err
    );
    assert!(r.sent().is_empty(), "nothing sent: {:?}", r.sent());
}

// ---------------------------------------------------------------- AC-DJYNRP.3

#[test]
fn djynrp_cli_with_the_flag_warns_it_was_tested_only_in_simulation() {
    let r = flash_with_flag("warn-sim.mencrypt");
    assert!(
        plain(&r.all()).contains("tested only in simulation"),
        "no simulation-only warning:\n{}",
        r.all()
    );
}

#[test]
fn djynrp_cli_with_the_flag_warns_no_write_is_sent_below_0x4000() {
    // FR NCH7DW: the warning says open-powerz never sends a write below
    // 0x4000, not that the region is never written.
    let r = flash_with_flag("warn-4000.mencrypt");
    let all = plain(&r.all());
    assert!(all.contains("0x4000"), "no 0x4000:\n{}", r.all());
    assert!(
        all.contains("never sends a write below 0x4000"),
        "no never-sends-a-write:\n{}",
        r.all()
    );
}

#[test]
fn djynrp_cli_with_the_flag_warns_a_failed_run_is_retried_by_holding_back_while_plugging() {
    let r = flash_with_flag("warn-back.mencrypt");
    assert!(
        plain(&r.all()).contains("hold back while plugging"),
        "no retry hint:\n{}",
        r.all()
    );
}

#[test]
fn djynrp_cli_the_warning_shows_even_when_no_frame_is_ever_sent() {
    // A file for another model: refused after the meter is opened, before
    // any frame. The warning must already be out.
    let bytes = ContainerBuilder::new()
        .field(MODEL, "KM002C")
        .payload_sized(&[7u8; 500])
        .build();
    let r = powerz(
        OWN_IDS,
        &flash_args(write_file("warn-km002c.mencrypt", &bytes), &[ACK, FLAG]),
    );
    assert!(r.sent().is_empty(), "nothing sent: {:?}", r.sent());
    assert!(
        plain(&r.all()).contains("tested only in simulation"),
        "no warning:\n{}",
        r.all()
    );
}

/// stdout and stderr through one pipe, in order (POSIX `sh` only).
#[cfg(unix)]
#[test]
fn djynrp_cli_the_warning_comes_before_the_first_frame() {
    let bin = env!("CARGO_BIN_EXE_powerz");
    let file = valid_file("warn-order.mencrypt");
    let mut c = std::process::Command::new("sh");
    c.arg("-c")
        .arg("exec \"$0\" \"$@\" 2>&1")
        .arg(bin)
        .args(["--trace", "--transport", "sim", "firmware", "flash"])
        .arg(&file)
        .args([ACK, FLAG]);
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_CLOCK", "fake")
        .env("POWERZ_SIM_MODE", OWN_IDS);
    let out = c.output().expect("sh runs powerz");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let lines: Vec<String> = text.lines().map(plain).collect();
    let warning = lines
        .iter()
        .position(|l| l.contains("tested only in simulation"))
        .unwrap_or_else(|| panic!("no warning in:\n{text}"));
    let first_frame = lines
        .iter()
        .position(|l| l.starts_with('>'))
        .unwrap_or_else(|| panic!("no frame traced in:\n{text}"));
    assert!(
        warning < first_frame,
        "the warning comes after a frame:\n{text}"
    );
}

// ---------------------------------------------------------------- AC-DJYNRP.6

#[test]
fn djynrp_cli_the_flag_without_the_risk_acknowledgement_is_a_usage_error() {
    let r = powerz(OWN_IDS, &flash_args(valid_file("no-ack.mencrypt"), &[FLAG]));
    assert_eq!(r.code, Some(2), "stderr:\n{}", r.err);
    assert!(r.err.contains(ACK), "names {ACK}:\n{}", r.err);
    assert!(
        r.err.to_lowercase().contains("risk"),
        "explains the risk:\n{}",
        r.err
    );
    assert!(r.sent().is_empty(), "nothing sent: {:?}", r.sent());
}

#[test]
fn djynrp_cli_the_flag_with_a_broken_file_is_refused_naming_it() {
    let path = write_file(
        "garbage-djynrp.mencrypt",
        b"definitely not a firmware container",
    );
    let r = powerz(OWN_IDS, &flash_args(path, &[ACK, FLAG]));
    assert_eq!(r.code, Some(2), "stderr:\n{}", r.err);
    assert!(
        r.err.contains("garbage-djynrp.mencrypt"),
        "names the file:\n{}",
        r.err
    );
    assert!(r.sent().is_empty(), "nothing sent: {:?}", r.sent());
}

#[test]
fn djynrp_cli_the_flag_with_a_file_for_another_model_is_a_product_error() {
    let bytes = ContainerBuilder::new()
        .field(MODEL, "KM002C")
        .payload_sized(&[7u8; 500])
        .build();
    let r = powerz(
        OWN_IDS,
        &flash_args(write_file("km002c-djynrp.mencrypt", &bytes), &[ACK, FLAG]),
    );
    assert_eq!(r.code, Some(5), "stderr:\n{}", r.err);
    assert!(r.err.contains("(-100)"), "the product error:\n{}", r.err);
    assert!(r.err.contains("KM002C"), "names the model:\n{}", r.err);
    assert!(r.sent().is_empty(), "nothing sent: {:?}", r.sent());
}
