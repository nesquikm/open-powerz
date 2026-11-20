//! FR JT38C8 (AC .3): the simulator's old-bootloader persona. It reports the
//! field-reported bootloader 2.1 descriptor (one vendor interface, class
//! 0xFF, subclass 0xFF, string `POWER-Z KM003C`, no HID) and its bulk pipe
//! answers neither CONNECT nor HEAD.
//!
//! Every identifier is synthetic.

mod common;

use std::time::Duration;

use common::*;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::session::{ConnectOutcome, HeadOutcome, Session};
use powerz_protocol::transport::{Error as TransportError, Transport};

/// Transport wrapper that logs every request's frame type.
#[derive(Debug)]
struct Logged {
    inner: SimDevice,
    types: Vec<u8>,
}

impl Transport for Logged {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.types.push(request[0] & 0x7F);
        self.inner.transact(request, timeout)
    }
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.inner.read(timeout)
    }
    fn recover(&mut self) -> Result<(), TransportError> {
        self.inner.recover()
    }
}

fn legacy(clock: &SimClock) -> SimDevice {
    device(Persona::Bulk, Scenario::Idle, clock).with_mode(DeviceMode::LegacyBootloader)
}

#[test]
fn jt38c8_the_legacy_bootloader_identity_is_the_2_1_descriptor() {
    let clock = clock();
    let id = legacy(&clock).usb_identity();
    assert_eq!(id.vid, 0x5FC9);
    assert_eq!(id.pid, 0x0063);
    let shape: Vec<(u8, u8, u8, Option<&str>)> = id
        .interfaces
        .iter()
        .map(|i| (i.number, i.class, i.subclass, i.string.as_deref()))
        .collect();
    assert_eq!(shape, vec![(0, 0xFF, 0xFF, Some("POWER-Z KM003C"))]);
}

#[test]
fn jt38c8_the_legacy_bootloader_identity_classifies_as_the_old_bootloader() {
    let clock = clock();
    assert_eq!(
        legacy(&clock).usb_identity().mode(),
        DeviceMode::LegacyBootloader
    );
}

#[test]
fn jt38c8_the_legacy_bootloader_vendor_string_is_the_product_name() {
    let clock = clock();
    assert_eq!(legacy(&clock).interface_string(), "POWER-Z KM003C");
}

#[test]
fn jt38c8_the_legacy_bootloader_answers_neither_connect_nor_head() {
    let clock = clock();
    let mut s = Session::with_clock(
        Logged {
            inner: legacy(&clock),
            types: Vec::new(),
        },
        clock.clone(),
    );
    let report = s.probe().expect("timeouts are data, not errors");
    assert_eq!(report.connect, ConnectOutcome::TimedOut);
    assert_eq!(report.head, HeadOutcome::TimedOut);
    let types = &s.transport().types;
    assert!(
        types.iter().all(|t| *t == 0x02 || *t == 0x40),
        "only CONNECT and HEAD sent: {types:02x?}"
    );
    assert_eq!(types.iter().filter(|t| **t == 0x40).count(), 1, "one HEAD");
}
