//! AC-TGE83B.9: the decoder does no I/O and builds for WebAssembly.
//!
//! The wasm32-unknown-unknown build itself runs in CI (`.github/workflows/
//! ci.yml`, step "PD decoder wasm32 build"). Locally these tests check the
//! structure that makes that build possible: the crate has no I/O
//! dependencies, its sources import no I/O, and CI builds it for wasm32.

use std::fs;
use std::path::{Path, PathBuf};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn sources() -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    rust_sources(&crate_dir().join("src"), &mut files);
    files
        .into_iter()
        .map(|p| {
            let text = fs::read_to_string(&p).expect("read source");
            (p, text)
        })
        .collect()
}

#[test]
fn decoder_sources_exist() {
    let all: String = sources().into_iter().map(|(_, t)| t).collect();
    for item in [
        "pub struct Capture",
        "pub struct Decoded",
        "pub struct Field",
        "pub enum Input",
    ] {
        assert!(all.contains(item), "the decoder defines `{item}`");
    }
}

#[test]
fn decoder_sources_import_no_io() {
    let forbidden = [
        "std::fs",
        "std::net",
        "std::io",
        "std::process",
        "std::thread",
        "std::env",
        "std::time::SystemTime",
        "std::time::Instant",
        "nusb",
        "hidapi",
        "serialport",
        "powerz_protocol",
        "transport",
        "println!",
        "eprintln!",
        "print!(",
        "dbg!(",
    ];
    for (path, text) in sources() {
        for word in forbidden {
            assert!(
                !text.contains(word),
                "{} uses `{word}`: the PD decoder must stay pure (no I/O)",
                path.display()
            );
        }
    }
}

#[test]
fn crate_has_no_io_dependencies() {
    let manifest = fs::read_to_string(crate_dir().join("Cargo.toml")).expect("Cargo.toml");
    let deps: String = manifest
        .split("\n[")
        .filter(|section| section.starts_with("dependencies]") || section.starts_with("target."))
        .collect();
    for dep in [
        "nusb",
        "hidapi",
        "serialport",
        "getrandom",
        "powerz-protocol",
        "tokio",
        "libc",
    ] {
        assert!(!deps.contains(dep), "powerz-pd must not depend on {dep}");
    }
}

#[test]
fn ci_builds_the_decoder_for_wasm32() {
    let ci =
        fs::read_to_string(crate_dir().join("../../.github/workflows/ci.yml")).expect("ci.yml");
    assert!(
        ci.contains("wasm32-unknown-unknown"),
        "CI installs the wasm32-unknown-unknown target"
    );
    assert!(
        ci.lines().any(|l| l.contains("cargo build")
            && l.contains("-p powerz-pd")
            && l.contains("--target wasm32-unknown-unknown")),
        "CI builds powerz-pd for wasm32-unknown-unknown"
    );
}
