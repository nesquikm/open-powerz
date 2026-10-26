//! `powerz firmware jump --to bootloader|application --i-understand`
//! (FR V9SK56, AC .7): one JUMP_DFU or JUMP_APROM through the flash guard,
//! then the mode the meter comes back in. Stage 2 of the hardware checks
//! uses it.
//!
//! Every run here uses `--transport sim` on the fake sim clock; no test
//! ever opens a real meter.
//!
//! Test-only simulator knobs used here (honoured with `--transport sim`):
//!
//! - `POWERZ_SIM_MODE=bootloader`: the simulated meter starts in its
//!   bootloader; unset, in application mode.
//! - `POWERZ_SIM_CLOCK=fake`: the mode-switch wait takes no real time.
//! - `POWERZ_SIM_FLASH_FAULT=<fault>`: the simulated meter injects this
//!   flash fault (`jump-dfu-ignored`, `jump-aprom-ignored`, ...).
//! - `POWERZ_SIM_CTRL_C_AFTER=0`: Ctrl-C is pressed before the jump.
//! - `POWERZ_SIM_REPLUG_AFTER=N`: like 2.0.6, the simulated meter takes a
//!   jump at the next power-up, and is replugged after N looks at it.

use std::time::Duration;

use assert_cmd::Command;
use clap::Parser;
use powerz_cli::{Cli, validate};

/// Simulator knobs cleared before every run.
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

/// `powerz --transport <transport> firmware jump <args>` on the fake sim
/// clock.
fn jump(transport: &str, env: &[(&str, &str)], args: &[&str]) -> Command {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_CLOCK", "fake");
    for (k, v) in env {
        c.env(k, v);
    }
    c.timeout(Duration::from_secs(60));
    c.args(["--transport", transport, "firmware", "jump"]);
    c.args(args);
    c
}

fn sim_jump(env: &[(&str, &str)], args: &[&str]) -> Command {
    jump("sim", env, args)
}

/// Run `c`; return stdout, stderr and the exit code.
fn output(c: &mut Command) -> (String, String, Option<i32>) {
    let out = c.output().expect("powerz runs");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code(),
    )
}

// ---- AC-V9SK56.7: parsing ----

#[test]
fn firmware_jump_parses_both_targets_with_the_acknowledgement_flag() {
    for to in ["bootloader", "application"] {
        let cli = Cli::try_parse_from([
            "powerz",
            "--transport",
            "sim",
            "firmware",
            "jump",
            "--to",
            to,
            "--i-understand",
        ]);
        let cli = cli.unwrap_or_else(|e| panic!("`firmware jump --to {to}` parses: {e}"));
        assert!(
            format!("{:?}", cli.command).contains("Jump"),
            "{:?}",
            cli.command
        );
    }
}

#[test]
fn firmware_jump_needs_a_target() {
    assert!(
        Cli::try_parse_from(["powerz", "firmware", "jump", "--i-understand"]).is_err(),
        "--to is required"
    );
    assert!(
        Cli::try_parse_from([
            "powerz",
            "firmware",
            "jump",
            "--to",
            "sideways",
            "--i-understand"
        ])
        .is_err(),
        "only bootloader or application"
    );
}

#[test]
fn firmware_jump_parses_without_the_flag_so_the_command_can_explain_the_risk() {
    let cli = Cli::try_parse_from(["powerz", "firmware", "jump", "--to", "bootloader"]);
    assert!(
        cli.is_ok(),
        "parses; the refusal is the command's: {:?}",
        cli.err()
    );
}

// ---- AC-V9SK56.7: refusals before anything is sent ----

#[test]
fn without_the_acknowledgement_flag_it_refuses_with_a_usage_error_and_sends_nothing() {
    let (out, err, code) = output(&mut sim_jump(&[], &["--to", "bootloader"]));
    assert_eq!(code, Some(2), "a usage error; stderr: {err}");
    assert!(err.contains("--i-understand"), "names the flag: {err}");
    assert!(
        err.to_lowercase().contains("risk"),
        "explains the risk: {err}"
    );
    assert!(out.is_empty(), "nothing sent, nothing printed: {out}");
}

#[test]
fn jump_over_hid_or_cdc_is_refused_like_flash_and_probe() {
    for transport in ["hid", "cdc"] {
        let cli = Cli::try_parse_from([
            "powerz",
            "--transport",
            transport,
            "firmware",
            "jump",
            "--to",
            "bootloader",
            "--i-understand",
        ])
        .unwrap_or_else(|e| panic!("parses: {e}"));
        let e = validate(&cli).expect_err("bulk only");
        let msg = e.to_string();
        assert!(msg.contains("firmware jump"), "names the command: {msg}");
        assert!(msg.contains("bulk"), "points at the bulk pipe: {msg}");

        let (out, err, code) = output(&mut jump(
            transport,
            &[],
            &["--to", "bootloader", "--i-understand"],
        ));
        assert_eq!(code, Some(2), "a usage error over {transport}: {err}");
        assert!(err.contains("bulk"), "{err}");
        assert!(out.is_empty(), "{out}");
    }
}

// ---- AC-V9SK56.7: the jump on the simulator ----

#[test]
fn from_application_mode_it_jumps_to_the_bootloader_and_prints_both_modes() {
    let (out, err, code) = output(&mut sim_jump(
        &[],
        &["--to", "bootloader", "--i-understand"],
    ));
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines,
        [
            "Was in:  application mode (sent JUMP_DFU)",
            "Reply:   none",
            REPLUG_TO_BOOTLOADER,
            "Now in:  bootloader mode"
        ],
        "stdout:\n{out}"
    );
}

#[test]
fn from_bootloader_mode_it_jumps_to_the_application_and_prints_both_modes() {
    let (out, err, code) = output(&mut sim_jump(
        &[("POWERZ_SIM_MODE", "bootloader")],
        &["--to", "application", "--i-understand"],
    ));
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines,
        [
            "Was in:  bootloader mode (sent JUMP_APROM)",
            "Reply:   none",
            REPLUG_TO_APPLICATION,
            "Now in:  application mode"
        ],
        "stdout:\n{out}"
    );
}

#[test]
fn a_jump_to_the_mode_it_is_already_in_fails_with_a_clear_message() {
    for (mode, to) in [("application", "application"), ("bootloader", "bootloader")] {
        let (out, err, code) = output(&mut sim_jump(
            &[("POWERZ_SIM_MODE", mode)],
            &["--to", to, "--i-understand"],
        ));
        assert_eq!(code, Some(5), "a device error, already in {mode}: {err}");
        assert!(out.is_empty(), "nothing sent, nothing reported: {out}");
        let lower = err.to_lowercase();
        assert!(lower.contains("already"), "says it is already there: {err}");
        assert!(lower.contains(mode), "names the mode: {err}");
    }
}

#[test]
fn an_ignored_jump_dfu_fails_with_the_mode_seen_and_the_hold_back_hint() {
    let (out, err, code) = output(&mut sim_jump(
        &[("POWERZ_SIM_FLASH_FAULT", "jump-dfu-ignored")],
        &["--to", "bootloader", "--i-understand"],
    ));
    assert_eq!(
        code,
        Some(5),
        "a device error; stdout: {out}\nstderr: {err}"
    );
    assert!(
        err.to_lowercase().contains("application"),
        "names the mode it stayed in: {err}"
    );
    assert!(err.contains("Back"), "the hold-Back hint: {err}");
}

#[test]
fn an_ignored_jump_aprom_fails_naming_the_bootloader() {
    let (out, err, code) = output(&mut sim_jump(
        &[
            ("POWERZ_SIM_MODE", "bootloader"),
            ("POWERZ_SIM_FLASH_FAULT", "jump-aprom-ignored"),
        ],
        &["--to", "application", "--i-understand"],
    ));
    assert_eq!(
        code,
        Some(5),
        "a device error; stdout: {out}\nstderr: {err}"
    );
    assert!(
        err.to_lowercase().contains("bootloader"),
        "names the mode it stayed in: {err}"
    );
}

#[test]
fn a_jump_write_that_disconnects_still_succeeds() {
    let (out, err, code) = output(&mut sim_jump(
        &[("POWERZ_SIM_FLASH_FAULT", "jump-dfu-disconnects")],
        &["--to", "bootloader", "--i-understand"],
    ));
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.to_lowercase().contains("bootloader"), "{out}");
}

#[test]
fn an_ignored_jump_still_prints_the_mode_it_was_in_before_the_error() {
    let (out, err, code) = output(&mut sim_jump(
        &[("POWERZ_SIM_FLASH_FAULT", "jump-dfu-ignored")],
        &["--to", "bootloader", "--i-understand"],
    ));
    assert_eq!(code, Some(5), "stdout: {out}\nstderr: {err}");
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        [
            "Was in:  application mode (sent JUMP_DFU)",
            "Reply:   none",
            REPLUG_TO_BOOTLOADER
        ],
        "stdout:\n{out}"
    );
    assert!(!out.contains("Now in:"), "{out}");
}

#[test]
fn a_failed_jump_write_still_prints_the_mode_it_was_in_before_the_error() {
    let (out, err, code) = output(&mut sim_jump(
        &[("POWERZ_SIM_FLASH_FAULT", "jump-dfu-write-fails")],
        &["--to", "bootloader", "--i-understand"],
    ));
    assert_eq!(code, Some(5), "stdout: {out}\nstderr: {err}");
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        ["Was in:  application mode (sent JUMP_DFU)"],
        "the Was in line comes before the error; stdout:\n{out}"
    );
    assert!(err.contains("JUMP_DFU"), "names the jump: {err}");
}

#[test]
fn ctrl_c_before_the_jump_sends_nothing_and_fails_as_interrupted() {
    for (mode, to) in [("application", "bootloader"), ("bootloader", "application")] {
        let (out, err, code) = output(&mut sim_jump(
            &[("POWERZ_SIM_MODE", mode), ("POWERZ_SIM_CTRL_C_AFTER", "0")],
            &["--to", to, "--i-understand"],
        ));
        assert_ne!(code, Some(0), "an interrupted jump fails: {out}\n{err}");
        assert!(
            err.to_lowercase().contains("interrupt"),
            "says it was interrupted: {err}"
        );
        assert!(
            !out.contains("sent JUMP") && !out.contains("Now in:"),
            "nothing sent: {out}"
        );
    }
}

// ---- AC-V9SK56.7: the reply, the replug prompt, a jump at power-up ----

/// The prompt printed after JUMP_DFU, before the wait.
const REPLUG_TO_BOOTLOADER: &str = "Unplug the meter and plug it back in (no buttons) if it does not \
     restart by itself; waiting up to 120 s for bootloader mode";
/// The prompt printed after JUMP_APROM, before the wait.
const REPLUG_TO_APPLICATION: &str = "Unplug the meter and plug it back in (no buttons) if it does \
     not restart by itself; waiting up to 120 s for application mode";

#[test]
fn a_jump_taken_at_the_next_power_up_reaches_the_bootloader_after_the_replug() {
    for (mode, to, want) in [
        ("application", "bootloader", "Now in:  bootloader mode"),
        ("bootloader", "application", "Now in:  application mode"),
    ] {
        let (out, err, code) = output(&mut sim_jump(
            &[("POWERZ_SIM_MODE", mode), ("POWERZ_SIM_REPLUG_AFTER", "6")],
            &["--to", to, "--i-understand"],
        ));
        assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
        assert_eq!(out.lines().last(), Some(want), "stdout:\n{out}");
    }
}

#[test]
fn a_bad_replug_knob_is_a_usage_error() {
    let (_, err, code) = output(&mut sim_jump(
        &[("POWERZ_SIM_REPLUG_AFTER", "soon")],
        &["--to", "bootloader", "--i-understand"],
    ));
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("POWERZ_SIM_REPLUG_AFTER"), "{err}");
}

// ---- AC-V9SK56.8: --trace for firmware jump ----

/// `powerz --trace --transport sim firmware jump --to <to> --i-understand`
/// with the meter starting in `mode`; returns the `>` lines of stderr.
fn traced_jump(mode: &str, to: &str) -> Vec<String> {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_CLOCK", "fake")
        .env("POWERZ_SIM_MODE", mode)
        .timeout(Duration::from_secs(60))
        .args([
            "--trace",
            "--transport",
            "sim",
            "firmware",
            "jump",
            "--to",
            to,
        ])
        .arg("--i-understand");
    let (out, err, code) = output(&mut c);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(!err.lines().any(|l| l.starts_with('x')), "{err}");
    err.lines()
        .filter(|l| l.starts_with('>'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn trace_from_application_mode_shows_connect_uid_read_auth_then_jump_dfu() {
    let sent = traced_jump("application", "bootloader");
    let types: Vec<String> = sent
        .iter()
        .map(|l| l.split_whitespace().nth(1).unwrap_or("").to_owned())
        .collect();
    assert_eq!(
        types,
        ["02", "44", "4c", "09"],
        "CONNECT, the UID read, 0x4C, then JUMP_DFU:\n{sent:#?}"
    );
    assert_eq!(
        sent[0].split_whitespace().skip(3).collect::<Vec<_>>(),
        ["00", "00"],
        "a bare CONNECT: {sent:#?}"
    );
    assert_eq!(
        sent[2].split_whitespace().count(),
        1 + 36,
        "a 36-byte 0x4C request: {sent:#?}"
    );
    assert_eq!(
        sent[3].split_whitespace().skip(3).collect::<Vec<_>>(),
        ["00", "00"],
        "a 4-byte JUMP_DFU: {sent:#?}"
    );
    assert_eq!(sent[3].split_whitespace().count(), 1 + 4, "{sent:#?}");
}

#[test]
fn trace_from_bootloader_mode_shows_only_jump_aprom() {
    let sent = traced_jump("bootloader", "application");
    assert_eq!(sent, ["> 08 01 00 00"], "only JUMP_APROM: {sent:#?}");
}
