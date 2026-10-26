//! `powerz firmware info FILE [--json]` prints a firmware container's
//! metadata (FR V9SK53, AC .2 and .3). It needs no meter and sends nothing:
//! these tests run with no device and no simulator. Every container is
//! synthetic, built by the shared test builder.

#[path = "../../powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use fw_container::*;
use powerz_protocol::firmware::container::MAX_CONTAINER_BYTES;

/// Write `bytes` to a fresh file under the test temp dir.
fn write_file(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("firmware-file-cli");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write container");
    path
}

fn powerz() -> Command {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    c.timeout(Duration::from_secs(30));
    c
}

/// The stdout line starting with `label`.
fn line<'a>(out: &'a str, label: &str) -> &'a str {
    out.lines()
        .find(|l| l.starts_with(label))
        .unwrap_or_else(|| panic!("no `{label}` line in:\n{out}"))
}

// ---- AC-V9SK53.3: text and JSON metadata ----

#[test]
fn firmware_info_prints_the_metadata_and_english_notes() {
    let path = write_file(
        "valid.bin",
        &ContainerBuilder::new().payload_sized(&[9u8; 456]).build(),
    );
    let out = powerz()
        .args(["firmware", "info"])
        .arg(&path)
        .assert()
        .success()
        .get_output()
        .clone();
    let out = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(line(&out, "Model:").contains("KM003C"), "{out}");
    assert!(line(&out, "Version:").contains("2.0.7"), "{out}");
    assert!(line(&out, "Date:").contains("2026.9.29"), "{out}");
    assert!(line(&out, "CRC:").contains("A175E4A0"), "{out}");
    assert!(line(&out, "Size:").contains("456"), "{out}");
    assert!(line(&out, "Container:").contains("V1.0"), "{out}");
    assert!(line(&out, "Vendor:").contains("ChargerLab"), "{out}");
    for note in EN_NOTES.lines() {
        assert!(out.contains(note), "English note {note:?} missing:\n{out}");
    }
}

#[test]
fn firmware_info_json_has_the_same_fields() {
    let path = write_file(
        "valid-json.bin",
        &ContainerBuilder::new().payload_sized(&[3u8; 100]).build(),
    );
    let out = powerz()
        .args(["firmware", "info", "--json"])
        .arg(&path)
        .assert()
        .success()
        .get_output()
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
    assert_eq!(v["model"], "KM003C");
    assert_eq!(v["version"], "2.0.7");
    assert_eq!(v["date"], "2026.9.29");
    assert_eq!(v["crc"], "A175E4A0");
    assert_eq!(v["size"], 100);
    assert_eq!(v["container_version"], "V1.0");
    assert_eq!(v["vendor"], "ChargerLab");
    assert_eq!(v["release_notes_en"], EN_NOTES);
}

// ---- AC-V9SK53.2: refused files exit non-zero with the reason ----

fn refused(name: &str, bytes: &[u8]) -> String {
    let path = write_file(name, bytes);
    let out = powerz()
        .args(["firmware", "info"])
        .arg(&path)
        .assert()
        .failure()
        .get_output()
        .clone();
    assert!(out.stdout.is_empty(), "no metadata for a refused file");
    String::from_utf8_lossy(&out.stderr).to_lowercase()
}

#[test]
fn wrong_magic_is_refused_with_the_reason() {
    let err = refused(
        "bad-magic.bin",
        &ContainerBuilder::new().field(MAGIC, "Nope").build(),
    );
    assert!(
        err.contains("magic") || err.contains("not a firmware"),
        "{err}"
    );
}

#[test]
fn truncated_file_is_refused_with_the_reason() {
    let bytes = ContainerBuilder::new().build();
    let err = refused("truncated.bin", &bytes[..20]);
    assert!(err.contains("trunc") || err.contains("end"), "{err}");
}

#[test]
fn short_payload_is_refused_with_the_reason() {
    let err = refused(
        "short.bin",
        &ContainerBuilder::new()
            .payload(&[0u8; 40])
            .field(SIZE, "41")
            .build(),
    );
    assert!(err.contains("41") && err.contains("40"), "{err}");
}

#[test]
fn missing_file_is_refused() {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("no-such-firmware.bin");
    let _ = std::fs::remove_file(&path);
    let out = powerz()
        .args(["firmware", "info"])
        .arg(&path)
        .assert()
        .failure()
        .get_output()
        .clone();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("no-such-firmware.bin"),
        "names the file: {err}"
    );
    let prefix = format!("Could not read {}: ", path.display());
    assert!(err.contains(&prefix), "sentence-case read failure: {err}");
    assert!(!err.contains("cannot read"), "old wording: {err}");
}

/// The raw (not lowercased) stderr of a refused `firmware info PATH`.
fn refused_raw(path: &std::path::Path) -> String {
    let out = powerz()
        .args(["firmware", "info"])
        .arg(path)
        .assert()
        .failure()
        .get_output()
        .clone();
    assert!(out.stdout.is_empty(), "no metadata for a refused file");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_container_refusal_is_prefixed_with_the_file_name() {
    let path = write_file(
        "prefixed.bin",
        &ContainerBuilder::new().field(MAGIC, "Nope").build(),
    );
    let err = refused_raw(&path);
    let prefix = format!("{}: The file is not a firmware file", path.display());
    assert!(err.contains(&prefix), "`<file>: <reason>`: {err}");
}

#[test]
fn a_file_larger_than_the_cap_is_refused_as_too_large() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("firmware-file-cli-big");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("too-big.bin");
    // Sparse: no 8 MiB written to disk.
    let f = std::fs::File::create(&path).expect("create big file");
    f.set_len(MAX_CONTAINER_BYTES as u64 + 1)
        .expect("extend sparsely");
    drop(f);
    let out = powerz()
        .args(["firmware", "info"])
        .arg(&path)
        .assert()
        .failure()
        .get_output()
        .clone();
    let _ = std::fs::remove_file(&path);
    assert!(out.stdout.is_empty(), "no metadata for a refused file");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("too-big.bin"), "names the file: {err}");
    assert!(err.to_lowercase().contains("too large"), "{err}");
}
