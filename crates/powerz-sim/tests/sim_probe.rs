//! FR V9SK56 (AC .6) against the simulator: the USB identity of each mode
//! and `Session::probe` in application and bootloader mode, sending only
//! CONNECT and HEAD.
//!
//! Every identifier is synthetic.

mod common;

use std::time::Duration;

use common::*;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::session::{ConnectOutcome, HeadOutcome, Session};
use powerz_protocol::transport::discovery::UsbIdentity;
use powerz_protocol::transport::guard::FORBIDDEN_TYPES;
use powerz_protocol::transport::{Error as TransportError, Transport};

/// Transport wrapper that logs every request.
#[derive(Debug)]
struct Logged {
    inner: SimDevice,
    sent: Vec<Vec<u8>>,
}

impl Logged {
    fn types(&self) -> Vec<u8> {
        self.sent.iter().map(|f| f[0] & 0x7F).collect()
    }
}

impl Transport for Logged {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.sent.push(request.to_vec());
        self.inner.transact(request, timeout)
    }
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.inner.read(timeout)
    }
    fn recover(&mut self) -> Result<(), TransportError> {
        self.inner.recover()
    }
}

fn logged(dev: SimDevice, clock: &SimClock) -> Session<Logged, SimClock> {
    Session::with_clock(
        Logged {
            inner: dev,
            sent: Vec::new(),
        },
        clock.clone(),
    )
}

fn sim(mode: DeviceMode, clock: &SimClock) -> SimDevice {
    device(Persona::Bulk, Scenario::Idle, clock).with_mode(mode)
}

fn class_and_string(id: &UsbIdentity) -> Vec<(u8, u8, Option<&str>)> {
    id.interfaces
        .iter()
        .map(|i| (i.number, i.class, i.string.as_deref()))
        .collect()
}

// ---------------------------------------------------------------- identity

#[test]
fn application_identity_is_the_km003c_with_four_interfaces() {
    let clock = clock();
    let id = sim(DeviceMode::Application, &clock).usb_identity();
    assert_eq!(id.vid, 0x5FC9);
    assert_eq!(id.pid, 0x0063);
    assert_eq!(id.product.as_deref(), Some("POWER-Z KM003C"));
    assert_eq!(
        class_and_string(&id),
        vec![
            (0, 0xFF, Some("APP")),
            (1, 0x02, Some("APP")),
            (2, 0x0A, None),
            (3, 0x03, None),
        ]
    );
    assert_eq!(id.mode(), DeviceMode::Application);
}

#[test]
fn bootloader_identity_has_one_vendor_interface_named_boot() {
    let clock = clock();
    let id = sim(DeviceMode::Bootloader, &clock).usb_identity();
    assert_eq!(id.vid, 0x5FC9);
    assert_eq!(class_and_string(&id), vec![(0, 0xFF, Some("BOOT"))]);
    assert_eq!(id.mode(), DeviceMode::Bootloader);
}

#[test]
fn identity_vendor_string_matches_the_sim_interface_string() {
    let clock = clock();
    for mode in [DeviceMode::Application, DeviceMode::Bootloader] {
        let dev = sim(mode, &clock);
        let id = dev.usb_identity();
        let vendor = id
            .interfaces
            .iter()
            .find(|i| i.class == 0xFF)
            .and_then(|i| i.string.as_deref());
        assert_eq!(vendor, Some(dev.interface_string()), "{mode:?}");
    }
}

// ---------------------------------------------------------------- probe

#[test]
fn probe_in_both_modes_sends_only_connect_and_head_and_reads_field_zero() {
    for mode in [DeviceMode::Application, DeviceMode::Bootloader] {
        let clock = clock();
        let mut s = logged(sim(mode, &clock), &clock);
        let report = s.probe().unwrap_or_else(|e| panic!("{mode:?}: {e}"));
        assert_eq!(report.connect, ConnectOutcome::Accepted, "{mode:?}");
        assert_eq!(report.head, HeadOutcome::Field(0), "{mode:?}");
        assert_eq!(
            s.transport().types(),
            vec![0x02, 0x40],
            "{mode:?}: exactly CONNECT then HEAD"
        );
        for ty in s.transport().types() {
            assert!(!FORBIDDEN_TYPES.contains(&ty), "{mode:?}: {ty:#04x}");
        }
    }
}

#[test]
fn probe_reports_the_configured_head_field() {
    let clock = clock();
    let dev = sim(DeviceMode::Bootloader, &clock).with_head_field(7);
    let mut s = logged(dev, &clock);
    let report = s.probe().expect("probe");
    assert_eq!(report.head, HeadOutcome::Field(7));
}

#[test]
fn probe_of_a_silent_meter_reports_timeouts_and_sends_only_connect_and_head() {
    for mode in [DeviceMode::Application, DeviceMode::Bootloader] {
        let clock = clock();
        let dev = sim(mode, &clock).with_faults(FaultPlan::new().timeout_always());
        let mut s = logged(dev, &clock);
        let report = s.probe().expect("timeouts are data, not errors");
        assert_eq!(report.connect, ConnectOutcome::TimedOut, "{mode:?}");
        assert_eq!(report.head, HeadOutcome::TimedOut, "{mode:?}");
        let types = s.transport().types();
        assert!(
            types.iter().all(|t| *t == 0x02 || *t == 0x40),
            "{mode:?}: {types:02x?}"
        );
        assert_eq!(types.iter().filter(|t| **t == 0x40).count(), 1, "one HEAD");
    }
}
