//! FR XW4QQN against the simulator's old bootloader that numbers its own
//! replies (`SimDevice::with_own_reply_ids`):
//!
//! - AC .5: driven the way v0.13.2 drove it (strict ids, a 500 ms first
//!   wait), it reproduces the field trace frame for frame.
//! - AC .1: driven in old-bootloader mode, the standard path alone gets
//!   CONNECT accepted and HEAD field 0, with the id mismatches noted and no
//!   variant run.
//! - AC .6: its string 2 is the raw `BOOT\0` of the field meter.
//!
//! Field bytes from one trace; every other value is synthetic.

mod common;

use std::time::Duration;

use common::*;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::session::{
    ConnectOutcome, HeadOutcome, IdMismatch, ProbeFrame, ProbePipe, ProbeReport, ProbeVariant,
};
use powerz_protocol::transport::discovery::StringDescriptor;
use powerz_protocol::transport::{Error as TransportError, TimeoutDirection, Transport};

/// One entry of a frame log.
#[derive(Debug, Clone, PartialEq, Eq)]
enum F {
    /// A frame written.
    Out(Vec<u8>),
    /// A frame read.
    In(Vec<u8>),
    /// A transfer that timed out.
    Timeout,
}

/// Wrapper logging every frame on the standard path and in each variant,
/// and every recovery's result.
#[derive(Debug)]
struct Recorded {
    inner: SimDevice,
    log: Vec<F>,
    recovers: Vec<bool>,
}

impl Recorded {
    fn result(&mut self, r: Result<Vec<u8>, TransportError>) -> Result<Vec<u8>, TransportError> {
        match &r {
            Ok(b) => self.log.push(F::In(b.clone())),
            Err(TransportError::Timeout) => self.log.push(F::Timeout),
            Err(_) => {}
        }
        r
    }
}

impl Transport for Recorded {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.log.push(F::Out(request.to_vec()));
        let r = self.inner.transact(request, timeout);
        self.result(r)
    }
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        let r = self.inner.read(timeout);
        self.result(r)
    }
    fn recover(&mut self) -> Result<(), TransportError> {
        let r = self.inner.recover();
        self.recovers.push(r.is_ok());
        r
    }
    fn last_timeout_direction(&self) -> Option<TimeoutDirection> {
        self.inner.last_timeout_direction()
    }
}

impl ProbePipe for Recorded {
    fn probe_variant(
        &mut self,
        variant: ProbeVariant,
        frame: &ProbeFrame,
        timeout: Duration,
    ) -> Result<Vec<u8>, TransportError> {
        self.log.push(F::Out(frame.bytes().to_vec()));
        let r = self.inner.probe_variant(variant, frame, timeout);
        self.result(r)
    }
}

fn own_ids_sim(clock: &SimClock) -> SimDevice {
    device(Persona::Bulk, Scenario::Idle, clock)
        .with_mode(DeviceMode::LegacyBootloader)
        .with_own_reply_ids()
}

/// Probe the own-ids sim, driving the probe as `drive`.
fn run(drive: DeviceMode) -> (ProbeReport, Recorded) {
    let clock = clock();
    let mut s = powerz_protocol::session::Session::with_clock(
        Recorded {
            inner: own_ids_sim(&clock),
            log: Vec::new(),
            recovers: Vec::new(),
        },
        clock,
    );
    let (report, _) = s
        .probe_with_variants(drive)
        .unwrap_or_else(|e| panic!("probe failed: {e}"));
    let rec = std::mem::replace(
        s.transport_mut(),
        Recorded {
            inner: SimDevice::new(Persona::Bulk, Scenario::Idle),
            log: Vec::new(),
            recovers: Vec::new(),
        },
    );
    (report, rec)
}

fn connect(id: u8) -> Vec<u8> {
    vec![0x02, id, 0x00, 0x00]
}

fn head(id: u8) -> Vec<u8> {
    let mut r = vec![0x40, id, 0x00, 0x01];
    r.extend([0u8; 16]);
    r
}

fn accept(id: u8) -> Vec<u8> {
    vec![0x05, id, 0x00, 0x00]
}

fn head_reply(id: u8) -> Vec<u8> {
    let mut r = vec![0xC0, id, 0x01, 0x01];
    r.extend([0u8; 8]);
    r.extend([0xFF; 4]);
    r.extend([0u8; 4]);
    r
}

// ---------------------------------------------------------------- AC-XW4QQN.5

#[test]
fn xw4qqn_strict_drive_replays_the_field_trace_frame_for_frame() {
    // `Bootloader` drives the strict policy of v0.13.2 (exact ids, a 500 ms
    // first CONNECT wait, a recovery, the variants after a silent CONNECT).
    let (_, rec) = run(DeviceMode::Bootloader);
    use F::*;
    let expected = vec![
        Out(connect(1)),
        Timeout,
        Out(head(2)),
        In(accept(0)),
        Timeout,
        Out(connect(3)),
        In(accept(0)),
        Out(head(4)),
        In(head_reply(1)),
        Out(connect(5)),
        In(accept(2)),
        Out(head(6)),
        In(head_reply(3)),
        Out(connect(7)),
        In(accept(4)),
        Out(head(8)),
        In(head_reply(5)),
    ];
    assert_eq!(rec.log, expected);
}

#[test]
fn xw4qqn_strict_drive_recovery_fails_on_the_own_ids_sim() {
    let (_, rec) = run(DeviceMode::Bootloader);
    assert_eq!(rec.recovers, vec![false]);
}

#[test]
fn xw4qqn_strict_drive_reports_the_field_timeouts() {
    let (report, _) = run(DeviceMode::Bootloader);
    assert_eq!(
        (
            report.connect.clone(),
            report.head.clone(),
            report.connect_timeout,
            report.head_timeout
        ),
        (
            ConnectOutcome::TimedOut,
            HeadOutcome::TimedOut,
            Some(TimeoutDirection::In),
            Some(TimeoutDirection::In)
        )
    );
}

// ---------------------------------------------------------------- AC-XW4QQN.1

#[test]
fn xw4qqn_legacy_drive_connect_is_accepted() {
    let (report, _) = run(DeviceMode::LegacyBootloader);
    assert_eq!(report.connect, ConnectOutcome::Accepted, "{report:?}");
}

#[test]
fn xw4qqn_legacy_drive_head_gives_field_0() {
    let (report, _) = run(DeviceMode::LegacyBootloader);
    assert_eq!(report.head, HeadOutcome::Field(0), "{report:?}");
}

#[test]
fn xw4qqn_legacy_drive_runs_no_variant() {
    let (report, rec) = run(DeviceMode::LegacyBootloader);
    assert!(report.variants.is_empty(), "{report:?}");
    use F::*;
    assert_eq!(
        rec.log,
        vec![
            Out(connect(1)),
            In(accept(0)),
            Out(head(2)),
            In(head_reply(1))
        ]
    );
}

#[test]
fn xw4qqn_legacy_drive_notes_the_id_mismatches() {
    let (report, _) = run(DeviceMode::LegacyBootloader);
    assert_eq!(
        (report.connect_ids, report.head_ids),
        (
            Some(IdMismatch { meter: 0, ours: 1 }),
            Some(IdMismatch { meter: 1, ours: 2 })
        )
    );
}

// ---------------------------------------------------------------- AC-XW4QQN.6

#[test]
fn xw4qqn_own_ids_sim_string_2_is_the_raw_field_string() {
    let clock = clock();
    let details = own_ids_sim(&clock).usb_details();
    let s2 = details
        .strings
        .iter()
        .find(|(i, _)| *i == 2)
        .map(|(_, s)| s.clone());
    assert_eq!(s2, Some(StringDescriptor::Text("BOOT\0".to_owned())));
}
