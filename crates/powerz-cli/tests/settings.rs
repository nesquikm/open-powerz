//! `powerz settings` (FR 1HHT01, AC .3 and .6): the meter's settings block
//! read-only, as tagged lines, JSON or a hex dump, against scripted
//! `MockTransport` sessions that pin every byte sent.

mod common;

use clap::Parser;
use common::*;
use powerz_cli::commands::{SettingsOutput, settings};
use powerz_cli::{Cli, Command};
use powerz_protocol::decode::DecodeError;
use powerz_protocol::decode::settings::MENU_SAVE_STEPS;
use powerz_protocol::session;
use powerz_protocol::transport::mock::{MockTransport, Step};
use serde_json::Value;

const SETTINGS_188: &str = include_str!("../../powerz-protocol/tests/vectors/settings_188.hex");

/// The captured 188-byte SETTINGS reply (FW 2.0.6), id 1.
fn reply() -> Vec<u8> {
    let hex: String = SETTINGS_188
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .flat_map(|l| l.chars().filter(|c| !c.is_whitespace()))
        .collect();
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn run(reply: Vec<u8>, how: SettingsOutput) -> (Result<(), powerz_cli::CliError>, String) {
    let mut s = session(MockTransport::new(vec![Step::transact(
        get_data(1, 0x008),
        reply,
    )]));
    let mut out = Vec::new();
    let r = settings(&mut s, how, &mut out);
    // AC-1HHT01.6: exactly one GET_DATA for SETTINGS, nothing else.
    assert_eq!(s.transport().written(), &[get_data(1, 0x008)]);
    assert_eq!(s.transport().remaining(), 0);
    (r, text(&out))
}

// ---- AC-1HHT01.3: argument parsing ----

#[test]
fn settings_subcommand_parses_json_and_raw() {
    let c = Cli::try_parse_from(["powerz", "settings"]).unwrap();
    assert_eq!(
        c.command,
        Command::Settings {
            json: false,
            raw: false,
            action: None
        }
    );
    let c = Cli::try_parse_from(["powerz", "settings", "--json"]).unwrap();
    assert_eq!(
        c.command,
        Command::Settings {
            json: true,
            raw: false,
            action: None
        }
    );
    let c = Cli::try_parse_from(["powerz", "settings", "--raw"]).unwrap();
    assert_eq!(
        c.command,
        Command::Settings {
            json: false,
            raw: true,
            action: None
        }
    );
    assert!(Cli::try_parse_from(["powerz", "settings", "--json", "--raw"]).is_err());
}

// ---- AC-1HHT01.3: tagged text lines ----

#[test]
fn settings_prints_every_field_with_its_value_and_tag() {
    let (r, out) = run(reply(), SettingsOutput::Text);
    r.unwrap();
    let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(lines.len() >= 11, "{out}");
    let want = [
        ("Language", "English"),
        ("Brightness", "50%"),
        ("Dim", "off"),
        ("Startup logo", "off"),
        ("Theme", "blue"),
        ("Auto-rotate", "on"),
        ("Rotation", "90°"),
        ("Sample rate", "50 SPS"),
        ("Voltage", "606, 606, 606, 606, 606"),
        ("Current", "996668"),
        ("Offset", "80"),
    ];
    for (label, value) in want {
        let line = lines
            .iter()
            .find(|l| l.to_lowercase().starts_with(&label.to_lowercase()))
            .unwrap_or_else(|| panic!("no {label} line in:\n{out}"));
        assert!(line.contains(value), "{label}: {line}");
        // Screen-checked fields are confirmed; rotation and calibration not.
        let tag = if ["Rotation", "Voltage", "Current", "Offset"].contains(&label) {
            "[inferred]"
        } else {
            "[confirmed]"
        };
        assert!(line.contains(tag), "{label} is not {tag}: {line}");
    }
}

#[test]
fn settings_crc_mismatch_is_an_error_not_a_decode() {
    let mut bad = reply();
    bad[8 + 0x20] ^= 0x01;
    let (r, out) = run(bad, SettingsOutput::Text);
    let err = r.expect_err("a CRC mismatch must fail");
    assert!(err.to_string().contains("CRC"), "{err}");
    assert!(!out.contains("Brightness"), "decoded anyway:\n{out}");
}

// ---- AC-Z39H24.10: an invalid block exits 6 with the menu-save steps ----

/// The captured reply with block A's check value stale, as the meter
/// serves it after a startup-duration write (0x1C).
fn stale_a_reply() -> Vec<u8> {
    let mut r = reply();
    // Move the duration bits (bits 14..15 of the word at 0x00) to code 2
    // and leave CRC A as it was.
    r[8 + 1] = (r[8 + 1] & 0x3F) | 0x80;
    r
}

/// The captured reply with block B's CRC broken.
fn bad_b_reply() -> Vec<u8> {
    let mut r = reply();
    r[8 + 0xB0] ^= 0x01;
    r
}

#[test]
fn z39h24_settings_with_an_invalid_block_exits_6_with_the_steps() {
    for (what, bad) in [("block A", stale_a_reply()), ("block B", bad_b_reply())] {
        for how in [SettingsOutput::Text, SettingsOutput::Json] {
            let (r, out) = run(bad.clone(), how);
            let err = r.expect_err("an invalid block must fail");
            assert_eq!(err.exit_code(), 6, "{what} {how:?}: {err}");
            assert!(
                matches!(err, powerz_cli::CliError::SettingsNotSaved(_)),
                "{what} {how:?}: {err:?}"
            );
            assert!(err.to_string().contains(MENU_SAVE_STEPS), "{what}: {err}");
            assert!(err.to_string().contains("CRC"), "{what}: {err}");
            assert!(!out.contains("Brightness"), "decoded anyway:\n{out}");
            assert!(!out.contains("brightness"), "decoded anyway:\n{out}");
        }
    }
}

#[test]
fn z39h24_settings_raw_still_prints_a_stale_block() {
    let stale = stale_a_reply();
    let (r, out) = run(stale.clone(), SettingsOutput::Raw);
    r.expect("the raw view prints the block");
    let first: Vec<String> = stale[8..24].iter().map(|b| format!("{b:02x}")).collect();
    assert!(out.contains(&first.join(" ")), "{out}");
}

#[test]
fn z39h24_settings_not_saved_is_exit_code_6_and_maps_from_the_session() {
    let e = powerz_cli::CliError::SettingsNotSaved(format!("x {MENU_SAVE_STEPS}"));
    assert_eq!(e.exit_code(), 6);
    assert!(e.to_string().contains(MENU_SAVE_STEPS));
    let from: powerz_cli::CliError = session::Error::Decode(DecodeError::Crc {
        what: "SETTINGS block A",
    })
    .into();
    assert_eq!(from.exit_code(), 6, "{from:?}");
    assert!(from.to_string().contains(MENU_SAVE_STEPS), "{from}");
    assert!(from.to_string().contains("SETTINGS block A"), "{from}");
    // Other device failures stay exit 5.
    let other: powerz_cli::CliError = session::Error::Protocol("x".into()).into();
    assert_eq!(other.exit_code(), 5);
}

#[test]
fn z39h24_settings_set_with_a_stale_block_sends_no_settings_write() {
    let mut s = session(MockTransport::new(vec![
        Step::transact(get_data(1, 0x008), with_id(stale_a_reply(), 1)),
        Step::transact(get_data(2, 0x008), with_id(stale_a_reply(), 2)),
    ]));
    let (mut out, mut note) = (Vec::new(), Vec::new());
    let r = powerz_cli::commands::settings_set(
        &mut s,
        &["brightness=80".to_string()],
        true,
        &mut out,
        &mut note,
    );
    let err = r.expect_err("refused while block A is stale");
    assert_eq!(err.exit_code(), 6, "{err}");
    assert!(err.to_string().contains(MENU_SAVE_STEPS), "{err}");
    assert!(
        s.transport().written().iter().all(|f| f[0] & 0x7F != 0x0A),
        "a settings write was sent: {:02x?}",
        s.transport().written()
    );
    assert!(
        s.transport().written().iter().all(|f| f[0] & 0x7F == 0x0C),
        "only reads: {:02x?}",
        s.transport().written()
    );
}

fn with_id(mut f: Vec<u8>, id: u8) -> Vec<u8> {
    f[1] = id;
    f
}

// ---- AC-1HHT01.3: --json ----

#[test]
fn settings_json_lists_fields_with_key_label_value_and_tag() {
    let (r, out) = run(reply(), SettingsOutput::Json);
    r.unwrap();
    let v: Value = serde_json::from_str(&out).expect("valid JSON");
    let fields = v["fields"].as_array().expect("fields array");
    assert_eq!(fields.len(), 11);
    let by_key = |k: &str| {
        fields
            .iter()
            .find(|f| f["key"] == k)
            .unwrap_or_else(|| panic!("no {k} in {v}"))
    };
    assert_eq!(by_key("language")["value"], "English");
    assert_eq!(by_key("brightness")["value"], "50%");
    assert_eq!(by_key("rotation")["value"], "90°");
    assert_eq!(by_key("sample_rate")["value"], "50 SPS");
    assert_eq!(by_key("offset_cal")["value"], "80");
    for f in fields {
        let inferred = ["rotation", "voltage_cal", "current_cal", "offset_cal"];
        let want = if inferred.iter().any(|k| f["key"] == *k) {
            "inferred"
        } else {
            "confirmed"
        };
        assert_eq!(f["tag"], want, "{f}");
        assert!(f["label"].as_str().is_some_and(|l| !l.is_empty()), "{f}");
    }
}

// ---- AC-1HHT01.3: --raw hex dump ----

#[test]
fn settings_raw_dumps_all_180_bytes_as_hex_with_offsets() {
    let (r, out) = run(reply(), SettingsOutput::Raw);
    r.unwrap();
    let mut bytes = Vec::new();
    let mut offsets = Vec::new();
    for line in out.lines().filter(|l| !l.trim().is_empty()) {
        let mut tokens = line.split_whitespace();
        let off = tokens.next().unwrap().trim_end_matches(':');
        offsets.push(usize::from_str_radix(off, 16).unwrap_or_else(|_| panic!("{line}")));
        for t in tokens {
            bytes.push(u8::from_str_radix(t, 16).unwrap_or_else(|_| panic!("{line}")));
        }
    }
    assert_eq!(bytes, reply()[8..].to_vec(), "dump:\n{out}");
    assert_eq!(offsets.first(), Some(&0));
    assert_eq!(offsets.get(1), Some(&16), "16 bytes per line:\n{out}");
}

#[test]
fn settings_raw_still_dumps_a_block_with_a_bad_crc() {
    let mut bad = reply();
    bad[8 + 0x20] ^= 0x01;
    let (r, out) = run(bad.clone(), SettingsOutput::Raw);
    r.unwrap();
    assert!(out.to_lowercase().contains("5f 02 00 00"), "{out}");
}
