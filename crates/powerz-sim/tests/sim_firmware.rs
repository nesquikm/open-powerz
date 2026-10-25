//! FR V9SK52 against the simulator: HEAD in application mode, the
//! bootloader persona, and `Session::firmware_status` (AC .1–.4).
//!
//! Every identifier is synthetic (the simulator's made-up serial and UID).

mod common;

use std::time::Duration;

use common::*;
use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::decode::head::{self, HeadReply};
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::firmware::version::FirmwareWarning;
use powerz_protocol::session::{FirmwareProblem, FirmwareProblemKind, Session};
use powerz_protocol::transport::guard::FORBIDDEN_TYPES;
use powerz_protocol::transport::{Error as TransportError, Transport};

/// Transport wrapper that logs every request with the simulated time it
/// was sent at.
#[derive(Debug)]
struct Logged {
    inner: SimDevice,
    clock: SimClock,
    sent: Vec<(u64, Vec<u8>)>,
}

impl Logged {
    fn new(inner: SimDevice, clock: &SimClock) -> Logged {
        Logged {
            inner,
            clock: clock.clone(),
            sent: Vec::new(),
        }
    }

    fn types(&self) -> Vec<u8> {
        self.sent.iter().map(|(_, f)| f[0] & 0x7F).collect()
    }
}

impl Transport for Logged {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.sent.push((self.clock.now_ms(), request.to_vec()));
        self.inner.transact(request, timeout)
    }
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.inner.read(timeout)
    }
    fn recover(&mut self) -> Result<(), TransportError> {
        self.inner.recover()
    }
}

fn logged_session(dev: SimDevice, clock: &SimClock) -> Session<Logged, SimClock> {
    Session::with_clock(Logged::new(dev, clock), clock.clone())
}

// ---- AC-V9SK52.1: the simulator reports its mode ----

#[test]
fn app_mode_sim_reports_the_app_interface_string_and_the_real_product_string() {
    let clock = clock();
    let dev = device(Persona::Bulk, Scenario::Idle, &clock);
    // iProduct (string 4) is the model name; `APP` is the interface string
    // of IF0 (string 2) [device].
    assert_eq!(dev.product_string(), "POWER-Z KM003C");
    assert_eq!(dev.interface_string(), "APP");
    assert_eq!(
        DeviceMode::from_interface_string(Some(dev.interface_string())),
        DeviceMode::Application
    );
    assert_eq!(
        DeviceMode::from_interface_string(Some(dev.product_string())),
        DeviceMode::Unknown,
        "the product string must not decide the mode"
    );
}

#[test]
fn bootloader_sim_reports_a_non_app_interface_string_read_as_bootloader() {
    let clock = clock();
    let dev = device(Persona::Bulk, Scenario::Idle, &clock).with_mode(DeviceMode::Bootloader);
    assert_ne!(dev.interface_string(), "APP");
    assert_eq!(
        DeviceMode::from_interface_string(Some(dev.interface_string())),
        DeviceMode::Bootloader
    );
}

// ---- AC-V9SK52.3: HEAD on the simulator ----

#[test]
fn sim_answers_head_after_connect_with_field_zero_by_default() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    let reply = d.transact(&head::request(2), T).expect("HEAD reply");
    assert_eq!(reply[0] & 0x7F, 0x40, "{reply:02x?}");
    assert_eq!(reply[1], 2, "HEAD reply carries the request id");
    let head = HeadReply::decode(&reply).unwrap();
    assert_eq!(head.old_firmware_field(), 0);
    assert!(!head.update_required());
}

#[test]
fn sim_head_field_is_configurable() {
    let clock = clock();
    let dev = device(Persona::Bulk, Scenario::Idle, &clock).with_head_field(3);
    let mut d = raw_connect(dev);
    let reply = d.transact(&head::request(2), T).expect("HEAD reply");
    let head = HeadReply::decode(&reply).unwrap();
    assert_eq!(head.old_firmware_field(), 3);
    assert!(head.update_required());
}

// ---- AC-V9SK52.2 / .3: Session::firmware_status ----

#[test]
fn firmware_status_reports_both_info_blocks() {
    let clock = clock();
    let mut s = logged_session(device(Persona::Bulk, Scenario::Idle, &clock), &clock);
    let st = s.firmware_status().expect("firmware status");
    let boot = st.bootloader.as_ref().expect("bootloader block");
    assert_eq!(boot.model(), "KM003C");
    assert_eq!(boot.version(), "2.5");
    assert_eq!(boot.date(), "2024.01.01");
    let app = st.application.as_ref().expect("application block");
    assert_eq!(app.model(), "KM003C");
    assert_eq!(app.version(), "2.0.6");
    assert_eq!(app.date(), "2025.06.01");
    assert_eq!(st.update_required, Some(false));
    assert_eq!(st.problems, Vec::<FirmwareProblem>::new());
}

#[test]
fn firmware_status_sends_connect_then_waits_then_head() {
    let clock = clock();
    let mut s = logged_session(device(Persona::Bulk, Scenario::Idle, &clock), &clock);
    s.firmware_status().expect("firmware status");
    let t = s.transport();
    let types = t.types();
    let connect = types
        .iter()
        .position(|&ty| ty == 0x02)
        .unwrap_or_else(|| panic!("no CONNECT: {types:02x?}"));
    let head_at = types
        .iter()
        .position(|&ty| ty == 0x40)
        .unwrap_or_else(|| panic!("no HEAD: {types:02x?}"));
    assert!(connect < head_at, "CONNECT before HEAD: {types:02x?}");
    // The last CONNECT before HEAD (a retried CONNECT counts).
    let last_connect = types[..head_at].iter().rposition(|&ty| ty == 0x02).unwrap();
    let waited = t.sent[head_at].0 - t.sent[last_connect].0;
    assert!(
        (40..=500).contains(&waited),
        "HEAD {waited} ms after CONNECT, expected about 50 ms"
    );
    let head_frame = &t.sent[head_at].1;
    assert_eq!(head_frame.len(), 20);
    assert_eq!(&head_frame[2..4], &[0x00, 0x01]);
    assert!(head_frame[4..].iter().all(|&b| b == 0));
}

#[test]
fn firmware_status_reports_update_required_from_a_non_zero_head_field() {
    let clock = clock();
    let dev = device(Persona::Bulk, Scenario::Idle, &clock).with_head_field(1);
    let mut s = logged_session(dev, &clock);
    let st = s.firmware_status().expect("firmware status");
    assert_eq!(st.update_required, Some(true));
    assert!(
        st.warnings().contains(&FirmwareWarning::UpdateRequired),
        "{:?}",
        st.warnings()
    );
}

#[test]
fn firmware_status_uses_only_safe_requests() {
    let clock = clock();
    let mut s = logged_session(device(Persona::Bulk, Scenario::Idle, &clock), &clock);
    s.firmware_status().expect("firmware status");
    for (_, f) in &s.transport().sent {
        let ty = f[0] & 0x7F;
        assert!(!FORBIDDEN_TYPES.contains(&ty), "forbidden type {ty:#04x}");
        assert!(
            matches!(ty, 0x02 | 0x40 | 0x44),
            "only CONNECT, HEAD and memory reads: sent {ty:#04x}"
        );
        if ty == 0x44 {
            let plain = crypto::decrypt(&KEYS[0], &f[4..36]);
            let addr = u32::from_le_bytes(plain[0..4].try_into().unwrap());
            let len = u32::from_le_bytes(plain[4..8].try_into().unwrap());
            assert!(
                [HW_INFO, FW_INFO].contains(&(addr, len)),
                "memory read {addr:#x}/{len} is not an info block"
            );
        }
    }
    let reads = s.transport().types().iter().filter(|&&t| t == 0x44).count();
    assert_eq!(reads, 2, "both info blocks, nothing else");
}

// ---- AC-V9SK52.4: warnings from the status ----

#[test]
fn sim_firmware_2_0_6_is_outdated() {
    let clock = clock();
    let mut s = logged_session(device(Persona::Bulk, Scenario::Idle, &clock), &clock);
    let st = s.firmware_status().unwrap();
    assert_eq!(st.warnings(), vec![FirmwareWarning::Outdated]);
}

#[test]
fn sim_firmware_version_is_configurable_and_old_firmware_is_too_old() {
    let clock = clock();
    let dev = device(Persona::Bulk, Scenario::Idle, &clock).with_app_version("1.6.0");
    let mut s = logged_session(dev, &clock);
    let st = s.firmware_status().unwrap();
    assert_eq!(st.application.as_ref().unwrap().version(), "1.6.0");
    assert_eq!(st.warnings(), vec![FirmwareWarning::TooOld]);
    // device_info reads the same block.
    assert_eq!(s.device_info().unwrap().firmware_version, "1.6.0");
}

#[test]
fn newest_firmware_without_head_flag_has_no_warning() {
    let clock = clock();
    let dev = device(Persona::Bulk, Scenario::Idle, &clock).with_app_version("2.0.7");
    let mut s = logged_session(dev, &clock);
    assert_eq!(s.firmware_status().unwrap().warnings(), Vec::new());
}

// ---- AC-V9SK52.2 / .3: Session::read_firmware_status_or_unknown ----

/// A logged session on `dev` with CONNECT (request 1) already accepted.
fn connected_logged(dev: SimDevice, clock: &SimClock) -> Session<Logged, SimClock> {
    let mut s = logged_session(dev, clock);
    s.connect().expect("CONNECT");
    s
}

#[test]
fn status_or_unknown_returns_the_full_status_when_everything_answers() {
    let clock = clock();
    let mut s = connected_logged(device(Persona::Bulk, Scenario::Idle, &clock), &clock);
    let st = s
        .read_firmware_status_or_unknown()
        .expect("firmware status");
    assert_eq!(st.bootloader.as_ref().map(|b| b.version()), Some("2.5"));
    assert_eq!(st.application.as_ref().map(|b| b.version()), Some("2.0.6"));
    assert_eq!(st.update_required, Some(false));
    assert_eq!(st.problems, Vec::<FirmwareProblem>::new());
    let connects = s.transport().types().iter().filter(|&&t| t == 0x02).count();
    assert_eq!(connects, 1, "no recovery CONNECT when nothing failed");
}

#[test]
fn status_or_unknown_keeps_the_blocks_and_reconnects_after_a_head_timeout() {
    let clock = clock();
    // Request 1 is CONNECT, 2 and 3 the info blocks, 4 the HEAD.
    let dev =
        device(Persona::Bulk, Scenario::Idle, &clock).with_faults(FaultPlan::new().timeout_at(4));
    let mut s = connected_logged(dev, &clock);
    let st = s
        .read_firmware_status_or_unknown()
        .expect("a HEAD timeout only costs the update check");
    assert_eq!(st.bootloader.as_ref().map(|b| b.version()), Some("2.5"));
    assert_eq!(st.application.as_ref().map(|b| b.version()), Some("2.0.6"));
    assert_eq!(st.update_required, None, "unknown, not false");
    assert_eq!(st.warnings(), vec![FirmwareWarning::Outdated]);
    assert!(
        st.problems
            .iter()
            .any(|p| p.kind == FirmwareProblemKind::Head && p.text.contains("HEAD")),
        "{:?}",
        st.problems
    );
    let types = s.transport().types();
    assert_eq!(&types[..5], &[0x02, 0x44, 0x44, 0x40, 0x02], "{types:02x?}");
    assert_eq!(
        types.iter().filter(|&&t| t == 0x02).count(),
        2,
        "exactly one recovery CONNECT: {types:02x?}"
    );
    s.read_adc().expect("the session works after the recovery");
}

#[test]
fn status_or_unknown_reports_a_timed_out_info_block_and_reconnects() {
    // Request 2 is the bootloader block, request 3 the application block.
    for (n, words) in [(2, ["bootloader", "0x420"]), (3, ["application", "0x4420"])] {
        let clock = clock();
        let dev = device(Persona::Bulk, Scenario::Idle, &clock)
            .with_faults(FaultPlan::new().timeout_at(n));
        let mut s = connected_logged(dev, &clock);
        let st = s
            .read_firmware_status_or_unknown()
            .unwrap_or_else(|e| panic!("request {n}: {e:?}"));
        let missing = if n == 2 {
            &st.bootloader
        } else {
            &st.application
        };
        assert_eq!(*missing, None, "request {n}");
        assert!(
            st.problems
                .iter()
                .any(|p| words.iter().any(|w| p.text.to_lowercase().contains(w))),
            "request {n}: a problem names the block: {:?}",
            st.problems
        );
        let kind = if n == 2 {
            FirmwareProblemKind::BootloaderBlock
        } else {
            FirmwareProblemKind::ApplicationBlock
        };
        assert_eq!(
            st.problems.iter().map(|p| p.kind).collect::<Vec<_>>(),
            vec![kind],
            "request {n}: {:?}",
            st.problems
        );
        let types = s.transport().types();
        let failed = n - 1;
        assert_eq!(types[failed], 0x44, "request {n}: {types:02x?}");
        assert_eq!(
            types.get(failed + 1),
            Some(&0x02),
            "request {n}: CONNECT right after the failed block: {types:02x?}"
        );
        assert_eq!(
            types.iter().filter(|&&t| t == 0x02).count(),
            2,
            "request {n}: {types:02x?}"
        );
        assert_eq!(
            types.iter().filter(|&&t| t == 0x40).count(),
            1,
            "request {n}: HEAD still runs: {types:02x?}"
        );
        s.read_adc()
            .unwrap_or_else(|e| panic!("request {n}: session dead after recovery: {e:?}"));
    }
}
