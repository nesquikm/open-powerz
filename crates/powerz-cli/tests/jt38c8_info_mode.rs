//! FR JT38C8 (AC .4): `info` names a meter on the old bootloader as
//! `bootloader (old, 2.1-style; experimental CLI-only flash
//! with --experimental-old-bootloader)` in text and `legacy_bootloader` in
//! JSON, and sends it nothing (it answers no request).

mod common;

use common::*;
use powerz_cli::commands::info_in_mode;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::transport::mock::MockTransport;
use serde_json::Value;

fn info_legacy(json: bool) -> (String, usize) {
    let mut s = session(MockTransport::new(vec![]));
    let mut out = Vec::new();
    info_in_mode(&mut s, DeviceMode::LegacyBootloader, json, &mut out)
        .unwrap_or_else(|e| panic!("info failed: {e}"));
    (text(&out), s.transport().written().len())
}

#[test]
fn jt38c8_info_text_names_the_old_bootloader() {
    let (out, sent) = info_legacy(false);
    let line = out
        .lines()
        .find(|l| l.starts_with("Mode:"))
        .unwrap_or_else(|| panic!("no Mode line in:\n{out}"));
    assert!(
        line.contains("bootloader (old, 2.1-style; experimental CLI-only flash with --experimental-old-bootloader)"),
        "{line}"
    );
    assert_eq!(sent, 0, "nothing sent");
}

#[test]
fn jt38c8_info_json_names_the_old_bootloader_legacy_bootloader() {
    let (out, sent) = info_legacy(true);
    let v: Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["mode"], "legacy_bootloader", "{v}");
    assert_eq!(sent, 0, "nothing sent");
}
