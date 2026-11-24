//! FR RXGEC1 (AC .1): the `Mode:` line of `firmware probe` and `info` on a
//! meter in the old bootloader is exact; the JSON value stays.

mod common;

use common::*;
use powerz_cli::commands::{firmware_probe, info_in_mode};
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::session::Session;
use powerz_protocol::transport::discovery::{UsbIdentity, UsbInterface};
use powerz_protocol::transport::mock::MockTransport;
use powerz_sim::{FaultPlan, Persona, Scenario, SimClock, SimDevice};

const MODE: &str =
    "bootloader (old, 2.1-style; experimental CLI-only flash with --experimental-old-bootloader)";

fn probe(json: bool) -> String {
    let identity = UsbIdentity {
        vid: 0x5FC9,
        pid: 0x0063,
        product: Some("POWER-Z KM003C".to_owned()),
        interfaces: vec![UsbInterface {
            number: 0,
            class: 0xFF,
            subclass: 0xFF,
            protocol: 0,
            string: Some("POWER-Z KM003C".to_owned()),
        }],
    };
    let clock = SimClock::new(1_790_000_000_000);
    let silent = SimDevice::with_clock(Persona::Bulk, Scenario::Idle, clock.clone())
        .with_faults(FaultPlan::new().timeout_always());
    let mut s = Session::with_clock(silent, clock);
    let mut out = Vec::new();
    firmware_probe(&mut s, &identity, json, &mut out)
        .unwrap_or_else(|e| panic!("probe failed: {e}"));
    text(&out)
}

fn info(json: bool) -> String {
    let mut s = session(MockTransport::new(vec![]));
    let mut out = Vec::new();
    info_in_mode(&mut s, DeviceMode::LegacyBootloader, json, &mut out)
        .unwrap_or_else(|e| panic!("info failed: {e}"));
    text(&out)
}

/// What follows `Mode:` on its line.
fn mode(out: &str) -> &str {
    out.lines()
        .find_map(|l| l.strip_prefix("Mode:"))
        .unwrap_or_else(|| panic!("no Mode line in:\n{out}"))
        .trim()
}

fn json_mode(out: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(out).expect("valid JSON");
    v["mode"].clone()
}

#[test]
fn rxgec1_probe_text_names_the_old_bootloader_exactly() {
    assert_eq!(mode(&probe(false)), MODE);
}

#[test]
fn rxgec1_info_text_names_the_old_bootloader_exactly() {
    assert_eq!(mode(&info(false)), MODE);
}

#[test]
fn rxgec1_probe_json_keeps_legacy_bootloader() {
    assert_eq!(json_mode(&probe(true)), "legacy_bootloader");
}

#[test]
fn rxgec1_info_json_keeps_legacy_bootloader() {
    assert_eq!(json_mode(&info(true)), "legacy_bootloader");
}
