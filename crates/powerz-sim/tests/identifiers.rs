//! AC-G0W08Z.13: the simulator crate (and the CLI crate with its e2e tests
//! and snapshots) holds no session-1 serial or UID, checked by the same
//! SHA-256 digest scan as the protocol crate (digests only, never the
//! literals); and the sim e2e tests run in the normal gate on all three OSes.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use sha2::{Digest, Sha256};

/// SHA-256 of the session-1 serial (its 6 ASCII digits).
const SERIAL_SHA256: &str = "373c9204a506742bdde27bc2340e74bf9af68440d4baaa4bed5bc821a7992281";
/// SHA-256 of the session-1 UID (its 24 lowercase hex chars).
const UID_SHA256: &str = "f2b044ae70a59bb91d6fb348ecd8b2dc98da30e70bf249cd859081962f64b5cc";

fn sha256_hex(s: &str) -> String {
    Sha256::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn contains_digest(chars: &[char], len: usize, digest: &str) -> bool {
    chars
        .windows(len)
        .any(|w| sha256_hex(&w.iter().collect::<String>()) == digest)
}

fn digits(text: &str) -> Vec<char> {
    text.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn hex_squashed(text: &str) -> Vec<char> {
    text.chars()
        .filter(|c| c.is_ascii_hexdigit())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// ASCII digits hidden in hex byte pairs (e.g. a serial stored as `31 32 ..`).
fn digits_in_hex_bytes(text: &str) -> Vec<char> {
    let h = hex_squashed(text);
    h.as_chunks::<2>()
        .0
        .iter()
        .filter_map(|p| u8::from_str_radix(&p.iter().collect::<String>(), 16).ok())
        .filter(u8::is_ascii_digit)
        .map(char::from)
        .collect()
}

fn findings(text: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    if contains_digest(&digits(text), 6, SERIAL_SHA256)
        || contains_digest(&digits_in_hex_bytes(text), 6, SERIAL_SHA256)
    {
        out.push("session-1 serial");
    }
    if contains_digest(&hex_squashed(text), 24, UID_SHA256) {
        out.push("session-1 UID");
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn sim_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn cli_root() -> PathBuf {
    sim_root().join("../powerz-cli")
}

fn workspace_root() -> PathBuf {
    sim_root().join("../..")
}

/// Every file of the sim and CLI crates: sources, tests, snapshots, manifests.
fn scanned_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in [sim_root(), cli_root()] {
        walk(&root.join("src"), &mut files);
        walk(&root.join("tests"), &mut files);
        files.push(root.join("Cargo.toml"));
    }
    files.sort();
    files
}

#[test]
fn scan_covers_the_sim_crate_and_the_cli_e2e_tests() {
    let files = scanned_files();
    for want in [
        "powerz-sim/src/lib.rs",
        "powerz-cli/tests/e2e_sim.rs",
        "powerz-cli/tests/hil.rs",
    ] {
        assert!(
            files.iter().any(|p| p.ends_with(want)),
            "{want} not among scanned files"
        );
    }
}

#[test]
fn scanner_detects_planted_synthetic_identifiers() {
    let serial_digest = sha256_hex("123456");
    assert!(contains_digest(
        &digits("serial 12-34-56"),
        6,
        &serial_digest
    ));
    assert!(contains_digest(
        &digits_in_hex_bytes("20 313233343536 20"),
        6,
        &serial_digest
    ));
    let uid_digest = sha256_hex("a0a1a2a3a4a5a6a7a8a9aaab");
    assert!(contains_digest(
        &hex_squashed("A0A1A2A3 A4A5A6A7 A8A9AAAB"),
        24,
        &uid_digest
    ));
    assert_ne!(serial_digest, SERIAL_SHA256);
    assert_ne!(uid_digest, UID_SHA256);
}

#[test]
fn sim_and_cli_crates_hold_no_session1_serial_or_uid() {
    let mut bad = Vec::new();
    for path in scanned_files() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for what in findings(&text) {
            bad.push(format!("{}: {what}", path.display()));
        }
    }
    assert!(bad.is_empty(), "identifiers found: {bad:#?}");
}

#[test]
fn simulator_reports_only_synthetic_identifiers() {
    let (mut s, _clock) = connected(Scenario::Idle);
    let info = s.device_info().unwrap();
    assert_eq!(sha256_hex(&info.serial), sha256_hex("123456"));
    assert_ne!(sha256_hex(&info.serial), SERIAL_SHA256);
    assert_ne!(sha256_hex(&info.uid), UID_SHA256);
    assert!(findings(&format!("{} {}", info.serial, info.uid)).is_empty());
}

#[test]
fn sim_e2e_tests_run_in_the_normal_gate_on_all_three_oses() {
    let manifest = std::fs::read_to_string(workspace_root().join("Cargo.toml")).unwrap();
    let default_members = manifest
        .lines()
        .find(|l| l.trim_start().starts_with("default-members"))
        .expect("workspace default-members");
    assert!(
        default_members.contains("crates/powerz-sim")
            && default_members.contains("crates/powerz-cli"),
        "sim and CLI crates are default members: {default_members}"
    );
    let e2e = std::fs::read_to_string(cli_root().join("tests/e2e_sim.rs")).unwrap();
    assert!(
        !e2e.contains("#[ignore"),
        "sim e2e tests must not be ignored"
    );
    assert!(e2e.contains("#[test]"));
    let ci = std::fs::read_to_string(workspace_root().join(".github/workflows/ci.yml")).unwrap();
    for os in ["ubuntu", "macos", "windows"] {
        assert!(ci.contains(os), "CI matrix lacks {os}");
    }
    assert!(
        ci.contains("cargo test --workspace"),
        "CI runs the workspace tests"
    );
}
