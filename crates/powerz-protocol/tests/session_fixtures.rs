//! AC-N6EHQC.10: no committed fixture or test file carries the session-1
//! serial number or MCU UID. Only SHA-256 digests of those identifiers live
//! here; every window of every file is hashed and compared.

mod common;

use std::path::{Path, PathBuf};

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

/// ASCII digits hidden in the bytes of a hex fixture (e.g. a plaintext
/// serial stored as `31 32 ..`).
fn digits_in_hex_bytes(text: &str) -> Vec<char> {
    let body: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect();
    let h: Vec<char> = body.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    h.chunks_exact(2)
        .filter_map(|p| u8::from_str_radix(&p.iter().collect::<String>(), 16).ok())
        .filter(u8::is_ascii_digit)
        .map(char::from)
        .collect()
}

/// Identifier findings for one file's text.
fn findings(text: &str, is_hex: bool) -> Vec<&'static str> {
    let mut out = Vec::new();
    if contains_digest(&digits(text), 6, SERIAL_SHA256)
        || (is_hex && contains_digest(&digits_in_hex_bytes(text), 6, SERIAL_SHA256))
    {
        out.push("session-1 serial");
    }
    if contains_digest(&hex_squashed(text), 24, UID_SHA256) {
        out.push("session-1 UID");
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn crate_files() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    walk(&root.join("tests"), &mut files);
    walk(&root.join("src"), &mut files);
    files.sort();
    files
}

#[test]
fn session_fixtures_scan_covers_the_memread_vectors() {
    let files = crate_files();
    for want in ["memread_0420.hex", "memread_4420.hex", "queue_1268.hex"] {
        assert!(
            files.iter().any(|p| p.ends_with(want)),
            "{want} not found among scanned files"
        );
    }
}

#[test]
fn session_fixtures_scanner_detects_planted_identifiers() {
    // Positive control with the synthetic values (never the real ones).
    let serial_digest = sha256_hex(common::SYNTH_SERIAL);
    let uid_digest = sha256_hex(common::SYNTH_UID_HEX);
    assert!(contains_digest(
        &digits("serial: 12-34-56"),
        6,
        &serial_digest
    ));
    let planted = "# x\n00 A0A1A2A3 A4A5A6A7 A8A9AAAB ff\n";
    assert!(contains_digest(&hex_squashed(planted), 24, &uid_digest));
    let ascii = "# x\n20 313233343536 20\n";
    assert!(contains_digest(
        &digits_in_hex_bytes(ascii),
        6,
        &serial_digest
    ));
    // The synthetic identifiers are not the session-1 ones.
    assert_ne!(serial_digest, SERIAL_SHA256);
    assert_ne!(uid_digest, UID_SHA256);
}

#[test]
fn session_fixtures_hold_no_session1_serial_or_uid() {
    let mut bad = Vec::new();
    for path in crate_files() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let is_hex = path.extension().is_some_and(|e| e == "hex");
        for what in findings(&text, is_hex) {
            bad.push(format!("{}: {what}", path.display()));
        }
    }
    assert!(
        bad.is_empty(),
        "identifiers found in committed files: {bad:#?}"
    );
}
