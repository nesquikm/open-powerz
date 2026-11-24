//! FR NCH7DW in the CLI.
//!
//! - AC .1 / .2: `POWERZ_SIM_MODE=application` with
//!   `POWERZ_SIM_JUMP_TO_OLD_BOOT=1` (test-only) runs a meter whose
//!   JUMP_DFU lands in the own-ids old bootloader: with the flag it is
//!   flashed, without it the flash ends after the switch with nothing sent
//!   to the old bootloader and the after-switch message.
//! - AC .3: the experimental warning says open-powerz never sends a write
//!   below 0x4000, and that bootloader 2.1 keeping that layout and taking a
//!   retry is expected but not verified; it states neither as fact.
//! - AC .5: the warning prints once, only when the meter is in the old
//!   bootloader (at open or after the switch), before any frame to it; on
//!   application or bootloader 2.5 meters that stay there it does not.
//! - AC .7: the probe JSON cuts string descriptors at their first NUL, and
//!   the `POWERZ_SIM_MODE` usage error lists legacy-bootloader-own-ids.
//!
//! Every run uses `--transport sim` on the fake sim clock; no real meter is
//! opened. Every container is synthetic.

// Several helpers serve only the `#[cfg(unix)]` tests below.
#![cfg_attr(not(unix), allow(dead_code))]

#[path = "../../powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use fw_container::*;
use serde_json::Value;

const FLAG: &str = "--experimental-old-bootloader";
const ACK: &str = "--i-understand";

const ENV_KNOBS: [&str; 10] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
    "POWERZ_SIM_HEAD",
    "POWERZ_SIM_APP_VERSION",
    "POWERZ_SIM_MODE",
    "POWERZ_SIM_FLASH_FAULT",
    "POWERZ_SIM_REPLUG_AFTER",
    "POWERZ_SIM_PROBE_ANSWER",
    "POWERZ_SIM_JUMP_TO_OLD_BOOT",
];

const OWN_IDS: &str = "legacy-bootloader-own-ids";
/// A phrase only the experimental warning carries.
const WARNING_MARK: &str = "never sends a write below 0x4000";

/// Writes `bytes` to a path no other call shares: tests run in parallel and
/// may reuse a name (several share one helper), so each call gets its own
/// pid-and-counter prefix.
fn write_file(name: &str, bytes: &[u8]) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("nch7dw-flash-cli");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(format!("{}-{n}-{name}", std::process::id()));
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

/// Markdown-free, line breaks joined, lowercase.
fn plain(s: &str) -> String {
    s.replace(['*', '`'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// One run, stdout and stderr merged in order (`2>&1` through `sh`), so
/// the warning's place among the traced frames is known.
struct Run {
    text: String,
    code: Option<i32>,
}

impl Run {
    fn lines(&self) -> Vec<String> {
        self.text.lines().map(|l| l.to_lowercase()).collect()
    }

    /// Indices of traced frames sent (`> [len] bytes…`).
    fn frames(&self) -> Vec<(usize, String)> {
        self.text
            .lines()
            .enumerate()
            .filter(|(_, l)| l.starts_with('>'))
            .map(|(i, l)| (i, l.split(' ').nth(2).unwrap_or("").to_owned()))
            .collect()
    }

    /// Lines carrying the experimental warning.
    fn warning_lines(&self) -> Vec<usize> {
        self.lines()
            .iter()
            .enumerate()
            .filter(|(_, l)| l.contains(WARNING_MARK) || l.contains("tested only in simulation"))
            .map(|(i, _)| i)
            .collect()
    }

    /// Times the warning is printed (each print counted once, by its mark).
    fn warning_count(&self) -> usize {
        plain(&self.text).matches(WARNING_MARK).count()
    }
}

/// `powerz --trace --transport sim <args>` with `env`, merged output.
#[cfg(unix)]
fn powerz(env: &[(&str, &str)], args: &[OsString]) -> Run {
    let bin = env!("CARGO_BIN_EXE_powerz");
    let mut c = std::process::Command::new("sh");
    c.arg("-c")
        .arg("exec \"$0\" \"$@\" 2>&1")
        .arg(bin)
        .args(["--trace", "--transport", "sim"])
        .args(args);
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_CLOCK", "fake");
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c.output().expect("sh runs powerz");
    Run {
        text: String::from_utf8_lossy(&out.stdout).into_owned(),
        code: out.status.code(),
    }
}

fn flash_args(file: PathBuf, extra: &[&str]) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec!["firmware".into(), "flash".into(), file.into_os_string()];
    a.extend(extra.iter().map(OsString::from));
    a
}

const APP_TO_OLD: [(&str, &str); 2] = [
    ("POWERZ_SIM_MODE", "application"),
    ("POWERZ_SIM_JUMP_TO_OLD_BOOT", "1"),
];

/// Index of the traced JUMP_DFU frame.
fn jump_dfu(r: &Run) -> usize {
    r.frames()
        .iter()
        .find(|(_, t)| t == "09")
        .map(|(i, _)| *i)
        .unwrap_or_else(|| panic!("no JUMP_DFU traced:\n{}", r.text))
}

// ---------------------------------------------------------------- AC-NCH7DW.1 / .2

#[cfg(unix)]
#[test]
fn nch7dw_cli_with_the_flag_a_switch_to_the_old_bootloader_is_flashed() {
    let r = powerz(
        &APP_TO_OLD,
        &flash_args(valid_file("switch-ok.mencrypt"), &[ACK, FLAG]),
    );
    assert_eq!(r.code, Some(0), "{}", r.text);
    assert!(r.text.contains("2.0.7"), "the new version:\n{}", r.text);
    // The meter really came back in the old bootloader: the warning that
    // only an old-bootloader meter gets was printed.
    assert_eq!(
        r.warning_count(),
        1,
        "the old bootloader was reached:\n{}",
        r.text
    );
}

#[cfg(unix)]
#[test]
fn nch7dw_cli_without_the_flag_a_switch_to_the_old_bootloader_ends_with_nothing_sent_to_it() {
    let r = powerz(
        &APP_TO_OLD,
        &flash_args(valid_file("switch-refused.mencrypt"), &[ACK]),
    );
    assert_eq!(r.code, Some(5), "{}", r.text);
    let jump = jump_dfu(&r);
    let after: Vec<_> = r.frames().into_iter().filter(|(i, _)| *i > jump).collect();
    assert!(
        after.is_empty(),
        "frames after JUMP_DFU: {after:?}\n{}",
        r.text
    );
    let t = plain(&r.text);
    assert!(
        t.contains("now in its old"),
        "the after-switch message:\n{}",
        r.text
    );
    assert!(t.contains(FLAG), "names the flag:\n{}", r.text);
}

// ---------------------------------------------------------------- AC-NCH7DW.3

#[cfg(unix)]
fn warning_text() -> String {
    let r = powerz(
        &[("POWERZ_SIM_MODE", OWN_IDS)],
        &flash_args(valid_file("warn-text.mencrypt"), &[ACK, FLAG]),
    );
    let lines: Vec<&str> = r.text.lines().collect();
    let first = lines
        .iter()
        .position(|l| plain(l).contains("experimental"))
        .unwrap_or_else(|| panic!("no warning:\n{}", r.text));
    let end = lines
        .iter()
        .skip(first)
        .position(|l| l.starts_with('>'))
        .map_or(lines.len(), |n| first + n);
    plain(&lines[first..end].join("\n"))
}

#[cfg(unix)]
#[test]
fn nch7dw_cli_warning_says_open_powerz_never_sends_a_write_below_0x4000() {
    let w = warning_text();
    assert!(w.contains(WARNING_MARK), "{w}");
}

#[cfg(unix)]
#[test]
fn nch7dw_cli_warning_says_the_2_1_layout_and_retry_are_expected_but_not_verified() {
    let w = warning_text();
    assert!(w.contains("expected"), "{w}");
    assert!(w.contains("not verified"), "{w}");
}

#[cfg(unix)]
#[test]
fn nch7dw_cli_warning_states_neither_as_fact() {
    let w = warning_text();
    assert!(!w.contains("is never written"), "{w}");
    assert!(!w.contains("never written"), "{w}");
}

// ---------------------------------------------------------------- AC-NCH7DW.5

#[cfg(unix)]
#[test]
fn nch7dw_cli_warning_prints_once_after_the_switch_before_any_frame_to_the_old_bootloader() {
    let r = powerz(
        &APP_TO_OLD,
        &flash_args(valid_file("warn-after-switch.mencrypt"), &[ACK, FLAG]),
    );
    assert_eq!(r.warning_count(), 1, "printed once:\n{}", r.text);
    let jump = jump_dfu(&r);
    let warning = r.warning_lines()[0];
    assert!(
        warning > jump,
        "the warning must come after the switch, not before JUMP_DFU:\n{}",
        r.text
    );
    let next_frame = r
        .frames()
        .into_iter()
        .find(|(i, _)| *i > jump)
        .map(|(i, _)| i)
        .unwrap_or_else(|| panic!("no frame after JUMP_DFU:\n{}", r.text));
    assert!(
        warning < next_frame,
        "the warning must come before the first frame to the old bootloader:\n{}",
        r.text
    );
}

#[cfg(unix)]
#[test]
fn nch7dw_cli_warning_prints_once_at_open_on_the_old_bootloader() {
    let r = powerz(
        &[("POWERZ_SIM_MODE", OWN_IDS)],
        &flash_args(valid_file("warn-at-open.mencrypt"), &[ACK, FLAG]),
    );
    assert_eq!(r.code, Some(0), "{}", r.text);
    assert_eq!(r.warning_count(), 1, "printed once:\n{}", r.text);
}

#[cfg(unix)]
#[test]
fn nch7dw_cli_no_warning_on_an_application_meter_that_stays_there() {
    let r = powerz(
        &[("POWERZ_SIM_MODE", "application")],
        &flash_args(valid_file("warn-app.mencrypt"), &[ACK, FLAG]),
    );
    assert_eq!(r.code, Some(0), "{}", r.text);
    assert!(r.warning_lines().is_empty(), "no warning:\n{}", r.text);
}

#[cfg(unix)]
#[test]
fn nch7dw_cli_no_warning_on_bootloader_2_5() {
    let r = powerz(
        &[("POWERZ_SIM_MODE", "bootloader")],
        &flash_args(valid_file("warn-bl25.mencrypt"), &[ACK, FLAG]),
    );
    assert_eq!(r.code, Some(0), "{}", r.text);
    assert!(r.warning_lines().is_empty(), "no warning:\n{}", r.text);
}

// ---------------------------------------------------------------- AC-NCH7DW.7

/// `powerz --transport sim firmware probe [--json]` with `POWERZ_SIM_MODE`.
fn probe(mode: &str, json: bool) -> (String, String, Option<i32>) {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_CLOCK", "fake");
    c.env("POWERZ_SIM_MODE", mode);
    c.timeout(Duration::from_secs(60));
    c.args(["--transport", "sim", "firmware", "probe"]);
    if json {
        c.arg("--json");
    }
    let out = c.output().expect("powerz runs");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code(),
    )
}

#[test]
fn nch7dw_cli_probe_json_cuts_string_descriptors_at_the_first_nul() {
    let (out, err, code) = probe(OWN_IDS, true);
    assert_eq!(code, Some(0), "stdout:\n{out}\nstderr:\n{err}");
    let v: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("JSON ({e}):\n{out}"));
    assert_eq!(v["strings"]["2"], Value::from("BOOT"), "{}", v["strings"]);
}

#[test]
fn nch7dw_cli_sim_mode_usage_error_lists_legacy_bootloader_own_ids() {
    let (out, err, code) = probe("no-such-mode", false);
    assert_eq!(code, Some(2), "stdout:\n{out}\nstderr:\n{err}");
    assert!(err.contains("POWERZ_SIM_MODE"), "{err}");
    assert!(
        err.contains(OWN_IDS),
        "the usage error lists {OWN_IDS}: {err}"
    );
}
