//! `powerz firmware flash FILE --i-understand` (FR V9SK54, AC .7, with the
//! progress of AC .4 and the version check of AC .5).
//!
//! Every run here uses `--transport sim`; no test ever opens a real meter.
//! Every container is synthetic, built by the shared test builder.
//!
//! Test-only simulator knobs used here (honoured with `--transport sim`):
//!
//! - `POWERZ_SIM_MODE=bootloader`: the simulated meter starts in its
//!   bootloader; unset, in application mode.
//! - `POWERZ_SIM_CLOCK=fake`: the simulator runs on a fake clock, so the
//!   mode-switch waits take no real time.
//! - `POWERZ_SIM_FLASH_FAULT=<fault>`: the simulated bootloader injects
//!   this flash fault, named after the sim's `FlashFault` variants in
//!   kebab-case (`missing-sync@3`, `no-finished`, `image-not-taken`, ...).
//! - `POWERZ_SIM_CTRL_C_AFTER=<k>`: with `firmware flash`, stands in for
//!   Ctrl-C once the simulated bootloader has taken `k` payload writes.
//!
//! The real USB discovery cannot be driven without a meter, so its
//! matching after the mode switch (AC .9: by USB serial only) is tested
//! through the pure `find_flashed_meter` over a hand-made device list.

#[path = "../../powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use clap::Parser;
use fw_container::*;
use powerz_cli::args::{Command as CliCommand, FirmwareAction};
use powerz_cli::commands::find_flashed_meter;
use powerz_cli::{Cli, CliError};
use powerz_protocol::transport::discovery::{DeviceEntry, Model, PID_KM003C};

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

/// 3 whole blocks plus a remainder.
const P: usize = 3 * 4080 + 100;

/// Write `bytes` to a fresh file under the test temp dir.
fn write_file(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("firmware-flash-cli");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write container");
    path
}

fn valid_file(name: &str) -> PathBuf {
    let payload: Vec<u8> = (0..P).map(|i| (i * 29 + 3) as u8).collect();
    write_file(
        name,
        &ContainerBuilder::new().payload_sized(&payload).build(),
    )
}

/// `powerz --transport sim firmware flash <args>` on the fake sim clock.
fn sim_flash(env: &[(&str, &str)], args: &[&std::ffi::OsStr]) -> Command {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_CLOCK", "fake");
    for (k, v) in env {
        c.env(k, v);
    }
    c.timeout(Duration::from_secs(60));
    c.args(["--transport", "sim", "firmware", "flash"]);
    c.args(args);
    c
}

fn output(c: &mut Command, success: bool) -> (String, String, Option<i32>) {
    let assert = c.assert();
    let assert = if success {
        assert.success()
    } else {
        assert.failure()
    };
    let out = assert.get_output().clone();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code(),
    )
}

// ---- AC-V9SK54.7: the flash command and its acknowledgement ----

#[test]
fn firmware_flash_parses_the_file_and_the_acknowledgement_flag() {
    let cli = Cli::try_parse_from([
        "powerz",
        "firmware",
        "flash",
        "fw.mencrypt",
        "--i-understand",
    ])
    .expect("parses");
    match cli.command {
        CliCommand::Firmware {
            action: FirmwareAction::Flash { file, i_understand },
        } => {
            assert_eq!(file, PathBuf::from("fw.mencrypt"));
            assert!(i_understand);
        }
        other => panic!("expected firmware flash, got {other:?}"),
    }
}

#[test]
fn firmware_flash_parses_without_the_flag_so_the_command_can_explain_the_risk() {
    let cli = Cli::try_parse_from(["powerz", "firmware", "flash", "fw.mencrypt"])
        .expect("parses; the refusal is the command's, with an explanation");
    match cli.command {
        CliCommand::Firmware {
            action: FirmwareAction::Flash { i_understand, .. },
        } => assert!(!i_understand),
        other => panic!("expected firmware flash, got {other:?}"),
    }
}

#[test]
fn without_the_acknowledgement_flag_it_refuses_with_a_usage_error_and_sends_nothing() {
    let path = valid_file("no-ack.mencrypt");
    let (out, err, code) = output(&mut sim_flash(&[], &[path.as_os_str()]), false);
    assert_eq!(code, Some(2), "a usage error; stderr: {err}");
    assert!(err.contains("--i-understand"), "names the flag: {err}");
    assert!(
        err.to_lowercase().contains("risk"),
        "explains the risk: {err}"
    );
    assert!(out.is_empty(), "no progress, nothing sent: {out}");
}

#[test]
fn a_file_that_fails_inspection_is_refused_before_any_device_is_opened() {
    let path = write_file("garbage.mencrypt", b"definitely not a firmware container");
    let ack = std::ffi::OsStr::new("--i-understand");
    let (out, err, code) = output(&mut sim_flash(&[], &[path.as_os_str(), ack]), false);
    assert_eq!(code, Some(2), "a usage error; stderr: {err}");
    assert!(err.contains("garbage.mencrypt"), "names the file: {err}");
    assert!(out.is_empty(), "no progress: {out}");
    let all = format!("{out}{err}").to_lowercase();
    assert!(!all.contains("switch") && !all.contains("eras"), "{all}");
}

#[test]
fn a_truncated_container_is_refused_before_any_device_is_opened() {
    let mut bytes = ContainerBuilder::new().payload_sized(&[7u8; 500]).build();
    bytes.truncate(bytes.len() - 10);
    let path = write_file("truncated.mencrypt", &bytes);
    let ack = std::ffi::OsStr::new("--i-understand");
    let (out, err, code) = output(&mut sim_flash(&[], &[path.as_os_str(), ack]), false);
    assert_eq!(code, Some(2), "stderr: {err}");
    assert!(err.contains("truncated.mencrypt"), "{err}");
    assert!(out.is_empty(), "{out}");
}

#[test]
fn the_first_line_names_the_image_size_the_file_declares() {
    // Like the real 2.0.7 file: the stored payload is longer than the
    // declared image (456912 stored, 456896 declared).
    let stored: Vec<u8> = (0..P + 16).map(|i| (i * 29 + 3) as u8).collect();
    let bytes = ContainerBuilder::new()
        .payload(&stored)
        .field(SIZE, &P.to_string())
        .build();
    let path = write_file("declared-size.mencrypt", &bytes);
    let ack = std::ffi::OsStr::new("--i-understand");
    let out = sim_flash(&[], &[path.as_os_str(), ack])
        .output()
        .expect("powerz runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let first = stdout.lines().next().unwrap_or_default();
    assert!(first.starts_with("Flashing"), "first line: {first:?}");
    assert!(
        first.contains(&P.to_string()),
        "names the declared size {P}: {first:?}"
    );
    assert!(
        !first.contains(&(P + 16).to_string()),
        "not the stored payload length: {first:?}"
    );
}

#[test]
fn a_file_for_another_model_is_refused() {
    let bytes = ContainerBuilder::new()
        .field(MODEL, "KM002C")
        .payload_sized(&[7u8; 500])
        .build();
    let path = write_file("km002c.mencrypt", &bytes);
    let ack = std::ffi::OsStr::new("--i-understand");
    let (_, err, _) = output(&mut sim_flash(&[], &[path.as_os_str(), ack]), false);
    assert!(err.contains("KM002C"), "names the file's model: {err}");
}

// ---- AC-V9SK54.4 / .5 through the CLI: progress and the new version ----

#[test]
fn a_full_sim_run_from_application_mode_prints_the_stages_and_the_new_version() {
    let path = valid_file("full-app.mencrypt");
    let ack = std::ffi::OsStr::new("--i-understand");
    let (out, err, _) = output(&mut sim_flash(&[], &[path.as_os_str(), ack]), true);
    let all = format!("{out}{err}");
    let lower = all.to_lowercase();
    for stage in ["switch", "eras", "writ", "verif"] {
        assert!(lower.contains(stage), "no `{stage}` stage in:\n{all}");
    }
    assert!(all.contains("2.0.7"), "the new version is shown:\n{all}");
    // The stages come in order.
    let at = |s: &str| lower.find(s).unwrap();
    assert!(at("switch") < at("eras") && at("eras") < at("writ") && at("writ") < at("verif"));
}

#[test]
fn a_full_sim_run_from_bootloader_mode_skips_the_switch() {
    let path = valid_file("full-boot.mencrypt");
    let ack = std::ffi::OsStr::new("--i-understand");
    let (out, err, _) = output(
        &mut sim_flash(
            &[("POWERZ_SIM_MODE", "bootloader")],
            &[path.as_os_str(), ack],
        ),
        true,
    );
    let all = format!("{out}{err}");
    let lower = all.to_lowercase();
    assert!(lower.contains("eras") && lower.contains("verif"), "{all}");
    assert!(
        !lower.contains("switch"),
        "no mode switch from the bootloader:\n{all}"
    );
    assert!(all.contains("2.0.7"), "{all}");
}

// ---- AC-V9SK54.7: a failing flash through the CLI ----

/// Run a flash from bootloader mode with `POWERZ_SIM_FLASH_FAULT=fault`.
fn failing_flash(name: &str, fault: &str) -> (String, String, Option<i32>) {
    let path = valid_file(name);
    let ack = std::ffi::OsStr::new("--i-understand");
    output(
        &mut sim_flash(
            &[
                ("POWERZ_SIM_MODE", "bootloader"),
                ("POWERZ_SIM_FLASH_FAULT", fault),
            ],
            &[path.as_os_str(), ack],
        ),
        false,
    )
}

fn assert_cli_recovery_text(err: &str) {
    assert!(err.contains("Back"), "no hold-Back hint in: {err}");
    assert!(
        err.to_lowercase().contains("plug"),
        "no plug-in hint in: {err}"
    );
}

#[test]
fn a_missing_sync_fault_exits_5_with_the_code_the_recovery_text_and_stages_so_far() {
    let (out, err, code) = failing_flash("fault-sync.mencrypt", "missing-sync@3");
    assert_eq!(code, Some(5), "a device error; stderr: {err}");
    assert!(err.contains("flash failed (-106): "), "{err}");
    assert_cli_recovery_text(&err);
    let lower = out.to_lowercase();
    assert!(lower.contains("eras") && lower.contains("writ"), "{out}");
    assert!(
        !lower.contains("verif"),
        "no stage after the failing one: {out}"
    );
    assert!(!out.contains("Done"), "{out}");
}

#[test]
fn a_no_finished_fault_exits_5_with_a_verify_error_after_the_verify_stage() {
    let (out, err, code) = failing_flash("fault-finished.mencrypt", "no-finished");
    assert_eq!(code, Some(5), "stderr: {err}");
    assert!(err.contains("flash failed (-103): "), "{err}");
    assert_cli_recovery_text(&err);
    let lower = out.to_lowercase();
    assert!(lower.contains("writ") && lower.contains("verif"), "{out}");
    assert!(!lower.contains("accepted"), "{out}");
    assert!(!out.contains("Done"), "{out}");
}

#[test]
fn an_image_that_does_not_take_exits_5_naming_both_versions() {
    let (out, err, code) = failing_flash("fault-not-taken.mencrypt", "image-not-taken");
    assert_eq!(code, Some(5), "stderr: {err}");
    assert!(err.contains("flash failed (-103): "), "{err}");
    assert!(err.contains("2.0.6"), "the meter's version: {err}");
    assert!(err.contains("2.0.7"), "the file's version: {err}");
    let lower = out.to_lowercase();
    assert!(lower.contains("verif") && lower.contains("return"), "{out}");
    assert!(!out.contains("Done"), "{out}");
}

#[test]
fn an_invalid_flash_fault_is_a_usage_error_naming_the_knob() {
    let (out, err, code) = failing_flash("fault-bogus.mencrypt", "bogus");
    assert_eq!(code, Some(2), "a usage error; stderr: {err}");
    assert!(
        err.contains("POWERZ_SIM_FLASH_FAULT"),
        "names the knob: {err}"
    );
    let lower = out.to_lowercase();
    assert!(!lower.contains("eras") && !lower.contains("writ"), "{out}");
}

// ---- AC-V9SK54.6 / .7 (review fix): Ctrl-C during the flash ----

#[test]
fn ctrl_c_during_the_payload_writes_stops_between_writes_with_the_recovery_text() {
    let path = valid_file("ctrl-c.mencrypt");
    let ack = std::ffi::OsStr::new("--i-understand");
    let (out, err, code) = output(
        &mut sim_flash(
            &[
                ("POWERZ_SIM_MODE", "bootloader"),
                ("POWERZ_SIM_CTRL_C_AFTER", "2"),
            ],
            &[path.as_os_str(), ack],
        ),
        false,
    );
    assert_ne!(code, Some(0), "an interrupted flash is a failure: {err}");
    assert!(
        err.to_lowercase().contains("interrupt"),
        "says the flash was interrupted: {err}"
    );
    assert_cli_recovery_text(&err);
    let lower = out.to_lowercase();
    assert!(lower.contains("eras") && lower.contains("writ"), "{out}");
    assert!(out.contains("8160"), "two writes were acknowledged: {out}");
    assert!(
        !out.contains(&format!("{P} / {P}")),
        "not all written: {out}"
    );
    assert!(!lower.contains("verif"), "no stage after writing: {out}");
    assert!(!out.contains("Done"), "{out}");
}

#[test]
fn ctrl_c_during_a_flash_from_application_mode_is_also_honoured() {
    let path = valid_file("ctrl-c-app.mencrypt");
    let ack = std::ffi::OsStr::new("--i-understand");
    let (out, err, code) = output(
        &mut sim_flash(
            &[("POWERZ_SIM_CTRL_C_AFTER", "1")],
            &[path.as_os_str(), ack],
        ),
        false,
    );
    assert_ne!(code, Some(0), "{err}");
    assert!(err.to_lowercase().contains("interrupt"), "{err}");
    assert_cli_recovery_text(&err);
    let lower = out.to_lowercase();
    assert!(lower.contains("switch") && lower.contains("writ"), "{out}");
    assert!(!lower.contains("verif"), "{out}");
}

// ---- AC-V9SK54.7 (review fix): finding the meter after the switch ----

fn entry(serial: Option<&str>, address: u8) -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid: PID_KM003C,
        serial: serial.map(str::to_owned),
        product: None,
        interface_string: None,
        bus: "1".into(),
        address,
    }
}

#[test]
fn after_the_switch_the_meter_with_the_selected_serial_is_picked() {
    let devices = vec![entry(Some("111111"), 1), entry(Some("222222"), 2)];
    let found = find_flashed_meter(&devices, Some("222222")).expect("a match");
    assert_eq!(found.map(|d| d.address), Some(2));
}

// AC-V9SK54.9: the serial is the same in application and bootloader mode
// [device], so a meter with another serial (or none) is another meter.

#[test]
fn after_the_switch_a_single_power_z_with_another_serial_is_not_picked() {
    let devices = vec![entry(Some("BOOT0001"), 7)];
    let found = find_flashed_meter(&devices, Some("222222")).expect("keep waiting, no error");
    assert_eq!(found.map(|d| d.address), None, "never another meter");
}

#[test]
fn after_the_switch_a_single_power_z_without_a_serial_is_picked_only_without_a_serial() {
    let devices = vec![entry(None, 9)];
    let found = find_flashed_meter(&devices, Some("222222")).expect("keep waiting, no error");
    assert_eq!(found.map(|d| d.address), None, "not the chosen meter");
    let found = find_flashed_meter(&devices, None).expect("a match");
    assert_eq!(found.map(|d| d.address), Some(9));
}

#[test]
fn after_the_switch_other_meters_with_a_chosen_serial_are_none_not_ambiguous() {
    let devices = vec![entry(Some("333333"), 3), entry(None, 4)];
    let found = find_flashed_meter(&devices, Some("222222"))
        .expect("not an error: the chosen meter is just not back yet");
    assert_eq!(found.map(|d| d.address), None);
}

#[test]
fn without_a_serial_two_candidates_are_ambiguous_not_no_meter() {
    let devices = vec![entry(Some("333333"), 3), entry(None, 4)];
    let e = find_flashed_meter(&devices, None).expect_err("two meters cannot be told apart");
    assert!(
        !matches!(e, CliError::NoDevice(_)),
        "an ambiguity, not `no meter`: {e:?}"
    );
    let msg = e.to_string();
    assert!(msg.contains("333333"), "names the candidates: {msg}");
    assert!(
        msg.to_lowercase().contains("several") || msg.to_lowercase().contains("ambiguous"),
        "says the choice is ambiguous: {msg}"
    );
}

#[test]
fn after_the_switch_no_power_z_at_all_is_none() {
    let found = find_flashed_meter(&[], Some("222222")).expect("not an error");
    assert!(found.is_none());
    let found = find_flashed_meter(&[], None).expect("not an error");
    assert!(found.is_none());
}

// ---- AC-V9SK56.8: --trace for firmware flash; AC-V9SK54.1 / .5: replugs ----

#[test]
fn trace_logs_every_frame_of_the_flash_raw_writes_included() {
    let file = valid_file("trace.mencrypt");
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_CLOCK", "fake")
        .timeout(Duration::from_secs(60))
        .args(["--trace", "--transport", "sim", "firmware", "flash"])
        .arg(&file)
        .arg("--i-understand");
    let (out, err, _) = output(&mut c, true);
    let sent: Vec<&str> = err.lines().filter(|l| l.starts_with('>')).collect();
    let types: Vec<&str> = sent
        .iter()
        .map(|l| l.split(' ').nth(1).unwrap_or(""))
        .collect();
    assert!(types.contains(&"09"), "JUMP_DFU traced: {err}");
    assert!(types.contains(&"42"), "the raw descriptor traced: {err}");
    assert!(types.contains(&"08"), "JUMP_APROM traced: {err}");
    assert!(
        sent.iter().filter(|l| l.len() > 3 * 4000).count() >= 3,
        "the payload writes traced"
    );
    assert!(err.lines().any(|l| l.starts_with('<')), "replies traced");
    assert!(out.contains("2.0.7"), "{out}");
}

#[test]
fn flash_with_jumps_at_power_up_prompts_for_both_replugs_and_succeeds() {
    let file = valid_file("replug.mencrypt");
    let (out, err, _) = output(
        &mut sim_flash(
            &[("POWERZ_SIM_REPLUG_AFTER", "6")],
            &[file.as_os_str(), "--i-understand".as_ref()],
        ),
        true,
    );
    let prompts: Vec<&str> = out
        .lines()
        .filter(|l| l.contains("Unplug the meter and plug it back in"))
        .collect();
    assert_eq!(prompts.len(), 2, "stdout:\n{out}\nstderr:\n{err}");
    assert!(prompts[0].contains("bootloader"), "{out}");
    assert!(prompts[1].contains("application"), "{out}");
    assert!(
        out.lines().any(|l| l == "JUMP_DFU reply: none"),
        "the reply to JUMP_DFU: {out}"
    );
    assert!(out.contains("2.0.7"), "{out}");
}
