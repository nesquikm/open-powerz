//! The desktop backend inspects a local firmware container and returns its
//! metadata with the English release notes, or a plain-language error
//! (FR V9SK53, AC .2 and .3). No meter is involved. Every container is
//! synthetic, built by the shared test builder.

#[path = "../../../../crates/powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::path::PathBuf;

use fw_container::*;
use open_powerz_desktop_lib::firmware_file::inspect_firmware_file;
use open_powerz_desktop_lib::ui_error::ErrorKind;
use powerz_protocol::firmware::container::MAX_CONTAINER_BYTES;
use serde_json::json;

fn write_file(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("firmware-file-desktop");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write container");
    path
}

// ---- AC-V9SK53.3: metadata for the UI ----

#[test]
fn inspect_returns_the_metadata_with_english_notes() {
    let path = write_file(
        "valid.bin",
        &ContainerBuilder::new().payload_sized(&[5u8; 300]).build(),
    );
    let info = inspect_firmware_file(&path).expect("a valid container is inspected");
    let v = serde_json::to_value(&info).expect("serialisable for IPC");
    assert_eq!(
        v,
        json!({
            "vendor": "ChargerLab",
            "container_version": "V1.0",
            "model": "KM003C",
            "version": "2.0.7",
            "date": "2026.9.29",
            "crc": "A175E4A0",
            "size": 300,
            "release_notes_en": EN_NOTES,
        })
    );
}

#[test]
fn crc_is_eight_uppercase_hex_digits() {
    let path = write_file(
        "small-crc.bin",
        &ContainerBuilder::new().field(CRC, "0000beef").build(),
    );
    let info = inspect_firmware_file(&path).expect("inspected");
    let v = serde_json::to_value(&info).expect("serialisable");
    assert_eq!(v["crc"], "0000BEEF");
}

// ---- AC-V9SK53.2: refusals reach the UI as plain language ----

fn refused(name: &str, bytes: &[u8]) -> (ErrorKind, String) {
    let path = write_file(name, bytes);
    let e = inspect_firmware_file(&path).expect_err("refused");
    assert!(!e.message.is_empty());
    (e.kind, e.message)
}

#[test]
fn a_wrong_magic_is_a_usage_error_with_the_reason() {
    let (kind, msg) = refused(
        "bad-magic.bin",
        &ContainerBuilder::new().field(MAGIC, "Nope").build(),
    );
    assert_eq!(kind, ErrorKind::Usage);
    assert!(msg.to_lowercase().contains("firmware"), "{msg}");
}

#[test]
fn a_short_payload_is_refused_with_both_lengths() {
    let (kind, msg) = refused(
        "short.bin",
        &ContainerBuilder::new()
            .payload(&[0u8; 40])
            .field(SIZE, "41")
            .build(),
    );
    assert_eq!(kind, ErrorKind::Usage);
    assert!(msg.contains("41") && msg.contains("40"), "{msg}");
}

#[test]
fn a_truncated_file_is_refused() {
    let bytes = ContainerBuilder::new().build();
    let (kind, _) = refused("truncated.bin", &bytes[..bytes.len() / 3]);
    assert_eq!(kind, ErrorKind::Usage);
}

#[test]
fn a_missing_file_is_refused_with_a_message() {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("no-such-firmware.bin");
    let _ = std::fs::remove_file(&path);
    let e = inspect_firmware_file(&path).expect_err("missing file");
    assert!(!e.message.is_empty());
    assert_ne!(e.kind, ErrorKind::Internal, "a missing file is not a crash");
    let prefix = format!("Could not read {}: ", path.display());
    assert!(
        e.message.starts_with(&prefix),
        "same wording as the CLI: {}",
        e.message
    );
}

#[test]
fn a_container_refusal_is_prefixed_with_the_file_name() {
    let path = write_file(
        "prefixed.bin",
        &ContainerBuilder::new().field(MAGIC, "Nope").build(),
    );
    let e = inspect_firmware_file(&path).expect_err("refused");
    assert_eq!(e.kind, ErrorKind::Usage);
    let prefix = format!("{}: The file is not a firmware file", path.display());
    assert!(
        e.message.starts_with(&prefix),
        "`<file>: <reason>`: {}",
        e.message
    );
}

#[test]
fn a_file_larger_than_the_cap_is_a_usage_error() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("firmware-file-desktop-big");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("too-big.bin");
    // Sparse: no 8 MiB written to disk.
    let f = std::fs::File::create(&path).expect("create big file");
    f.set_len(MAX_CONTAINER_BYTES as u64 + 1)
        .expect("extend sparsely");
    drop(f);
    let result = inspect_firmware_file(&path);
    let _ = std::fs::remove_file(&path);
    let e = result.expect_err("too large");
    assert_eq!(e.kind, ErrorKind::Usage);
    assert!(
        e.message.contains("too-big.bin"),
        "names the file: {}",
        e.message
    );
    assert!(
        e.message.to_lowercase().contains("too large"),
        "{}",
        e.message
    );
}
