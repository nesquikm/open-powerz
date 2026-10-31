//! `powerz info` survives a failing info block and reads each block once
//! (FR 4PJRVJ, AC .7). Runs the command over the simulator with chosen
//! memory reads timing out; no hardware is touched.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use powerz_cli::commands::info_in_mode;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::session::{Session, SystemClock};
use powerz_protocol::transport::{self, Transport};
use powerz_sim::{Persona, Scenario, SimDevice};
use serde_json::Value;

/// Memory read (0x44) message type `[static]`.
const MEMORY_READ: u8 = 0x44;

/// The simulator with the memory reads numbered in `fail` timing out.
struct Flaky {
    inner: SimDevice,
    fail: HashSet<usize>,
    reads: Arc<AtomicUsize>,
}

impl Transport for Flaky {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        if request[0] & 0x7F == MEMORY_READ {
            let n = self.reads.fetch_add(1, Ordering::SeqCst);
            if self.fail.contains(&n) {
                return Err(transport::Error::Timeout);
            }
        }
        self.inner.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.inner.read(timeout)
    }

    fn recover(&mut self) -> Result<(), transport::Error> {
        self.inner.recover()
    }
}

/// `powerz info` over the simulator: the result, what it printed and how
/// many memory reads it sent.
fn run(fail: &[usize], json: bool) -> (Result<(), String>, String, usize) {
    let reads = Arc::new(AtomicUsize::new(0));
    let flaky = Flaky {
        inner: SimDevice::new(Persona::Bulk, Scenario::Idle),
        fail: fail.iter().copied().collect(),
        reads: Arc::clone(&reads),
    };
    let mut session = Session::with_clock(flaky, SystemClock);
    session.connect().expect("the simulator accepts CONNECT");
    let mut out = Vec::new();
    let r = info_in_mode(&mut session, DeviceMode::Application, json, &mut out)
        .map_err(|e| e.to_string());
    let text = String::from_utf8_lossy(&out).into_owned();
    (r, text, reads.load(Ordering::SeqCst))
}

#[test]
fn a_failing_info_block_does_not_abort_powerz_info() {
    // Whichever of the four reads fails.
    for k in 0..4 {
        let (r, out, _) = run(&[k], true);
        assert_eq!(r, Ok(()), "memory read {k} failed");
        let v: Value = serde_json::from_str(&out).expect("JSON output");
        assert_eq!(v["mode"], "application", "read {k}");
        assert_eq!(v["model"], "KM003C", "the other block names it: read {k}");
        let problems = v["firmware_problems"].as_array().expect("problems");
        assert!(!problems.is_empty(), "read {k} shows as a problem: {v}");
    }
}

#[test]
fn every_info_block_failing_still_prints_a_report_with_the_problems() {
    let (r, out, _) = run(&[0, 1, 2, 3, 4, 5, 6, 7], false);
    assert_eq!(r, Ok(()));
    assert!(out.contains("Mode:       application"), "{out}");
    assert!(out.contains("Firmware status:"), "{out}");
}

#[test]
fn powerz_info_reads_each_block_once() {
    let (r, _, reads) = run(&[], true);
    assert_eq!(r, Ok(()));
    assert_eq!(reads, 4, "bootloader, application, OTP and UID, once each");
}
