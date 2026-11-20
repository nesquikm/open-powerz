//! FR QF92WX: the firmware probe says why an old bootloader stays silent.
//!
//! - AC .1: endpoint descriptors decode into address, direction, transfer
//!   type, max packet size and interval (`EndpointInfo::from_descriptor`).
//! - AC .3: a CONNECT / HEAD timeout carries its direction (OUT write or IN
//!   read), read from `Transport::last_timeout_direction`.
//! - AC .4: outside application mode, a timed-out standard CONNECT is
//!   followed by the read-only variants, each running CONNECT then HEAD over
//!   `ProbePipe`; the report names the first variant that got an answer.
//! - AC .5: the variants send only CONNECT (0x02) and HEAD (0x40) frames,
//!   and application mode gets no variant retries.
//!
//! Over hand-written fakes; every value is synthetic.

mod common;

use std::time::Duration;

use common::*;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::session::{
    ConnectOutcome, HeadOutcome, ProbeFrame, ProbePipe, ProbeVariant, Session,
};
use powerz_protocol::transport::discovery::{EndpointDirection, EndpointInfo, TransferType};
use powerz_protocol::transport::{Error as TransportError, TimeoutDirection, Transport};

const CONNECT: u8 = 0x02;
const HEAD: u8 = 0x40;

/// A HEAD reply with id 0 (as the meter sends it) and `field`.
fn head_reply(field: u32) -> Vec<u8> {
    let mut f = vec![0x40, 0x00, 0x00, 0x01];
    f.extend(field.to_le_bytes());
    f.extend([0u8; 12]);
    f
}

/// A pipe on which the standard path answers or times out (in `dir`), and
/// the variants answer only on `answer_on`.
#[derive(Debug)]
struct Fake {
    standard_answers: bool,
    answer_on: Option<ProbeVariant>,
    dir: Option<TimeoutDirection>,
    sent: Vec<Vec<u8>>,
    variant_sent: Vec<(ProbeVariant, Vec<u8>)>,
}

impl Fake {
    fn silent(dir: TimeoutDirection) -> Fake {
        Fake {
            standard_answers: false,
            answer_on: None,
            dir: Some(dir),
            sent: Vec::new(),
            variant_sent: Vec::new(),
        }
    }

    fn answering_on(variant: ProbeVariant) -> Fake {
        Fake {
            answer_on: Some(variant),
            ..Fake::silent(TimeoutDirection::In)
        }
    }

    fn answering() -> Fake {
        Fake {
            standard_answers: true,
            ..Fake::silent(TimeoutDirection::In)
        }
    }

    fn reply_to(request: &[u8]) -> Vec<u8> {
        match request[0] & 0x7F {
            CONNECT => accept(request[1]),
            _ => head_reply(7),
        }
    }
}

impl Transport for Fake {
    fn transact(&mut self, request: &[u8], _timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.sent.push(request.to_vec());
        if self.standard_answers {
            Ok(Fake::reply_to(request))
        } else {
            Err(TransportError::Timeout)
        }
    }

    fn read(&mut self, _timeout: Duration) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::Timeout)
    }

    fn last_timeout_direction(&self) -> Option<TimeoutDirection> {
        self.dir
    }
}

impl ProbePipe for Fake {
    fn probe_variant(
        &mut self,
        variant: ProbeVariant,
        frame: &ProbeFrame,
        _timeout: Duration,
    ) -> Result<Vec<u8>, TransportError> {
        self.variant_sent.push((variant, frame.bytes().to_vec()));
        if self.answer_on == Some(variant) {
            Ok(Fake::reply_to(frame.bytes()))
        } else {
            Err(TransportError::Timeout)
        }
    }
}

fn session(fake: Fake) -> Session<Fake, FakeClock> {
    Session::with_clock(fake, FakeClock::new(1_790_000_000_000))
}

// ---------------------------------------------------------------- AC .1

#[test]
fn qf92wx_endpoint_in_interrupt_decodes_every_field() {
    assert_eq!(
        EndpointInfo::from_descriptor(0x82, 0x03, 8, 10),
        EndpointInfo {
            address: 0x82,
            direction: EndpointDirection::In,
            transfer: TransferType::Interrupt,
            max_packet_size: 8,
            interval: 10,
        }
    );
}

#[test]
fn qf92wx_endpoint_out_bulk_decodes_every_field() {
    assert_eq!(
        EndpointInfo::from_descriptor(0x01, 0x02, 64, 0),
        EndpointInfo {
            address: 0x01,
            direction: EndpointDirection::Out,
            transfer: TransferType::Bulk,
            max_packet_size: 64,
            interval: 0,
        }
    );
}

#[test]
fn qf92wx_endpoint_transfer_type_reads_only_bits_0_and_1() {
    // 0x0D: isochronous with synchronisation bits set.
    assert_eq!(
        EndpointInfo::from_descriptor(0x83, 0x0D, 192, 1).transfer,
        TransferType::Isochronous
    );
    assert_eq!(
        EndpointInfo::from_descriptor(0x00, 0x00, 64, 0).transfer,
        TransferType::Control
    );
}

// ---------------------------------------------------------------- AC .3

#[test]
fn qf92wx_connect_timeout_reports_the_out_write() {
    let mut s = session(Fake::silent(TimeoutDirection::Out));
    let report = s.probe().expect("a timeout is data");
    assert_eq!(report.connect, ConnectOutcome::TimedOut);
    assert_eq!(report.connect_timeout, Some(TimeoutDirection::Out));
}

#[test]
fn qf92wx_head_timeout_reports_the_in_read() {
    let mut s = session(Fake::silent(TimeoutDirection::In));
    let report = s.probe().expect("a timeout is data");
    assert_eq!(report.head, HeadOutcome::TimedOut);
    assert_eq!(report.head_timeout, Some(TimeoutDirection::In));
}

#[test]
fn qf92wx_answered_probe_reports_no_timeout_direction() {
    let mut s = session(Fake::answering());
    let report = s.probe().expect("probe runs");
    assert_eq!(report.connect, ConnectOutcome::Accepted);
    assert_eq!((report.connect_timeout, report.head_timeout), (None, None));
}

#[test]
fn qf92wx_timeout_direction_labels() {
    assert_eq!(TimeoutDirection::Out.label(), "OUT write");
    assert_eq!(TimeoutDirection::In.label(), "IN read");
    assert_eq!(TimeoutDirection::Out.name(), "out");
    assert_eq!(TimeoutDirection::In.name(), "in");
}

// ---------------------------------------------------------------- AC .4

#[test]
fn qf92wx_variant_names_in_order() {
    let names: Vec<&str> = ProbeVariant::ALL.iter().map(|v| v.name()).collect();
    assert_eq!(
        names,
        vec![
            "no-drain-no-clear-halt",
            "in-before-out",
            "all-in-endpoints"
        ]
    );
}

#[test]
fn qf92wx_silent_old_bootloader_runs_every_variant_and_none_answers() {
    let mut s = session(Fake::silent(TimeoutDirection::In));
    let (report, _) = s
        .probe_with_variants(DeviceMode::LegacyBootloader)
        .expect("probe runs");
    let ran: Vec<ProbeVariant> = report.variants.iter().map(|v| v.variant).collect();
    assert_eq!(
        ran,
        vec![
            ProbeVariant::NoDrainNoClearHalt,
            ProbeVariant::InBeforeOut,
            ProbeVariant::AllInEndpoints
        ]
    );
    assert_eq!(report.answered_variant(), None);
}

#[test]
fn qf92wx_silent_variant_outcomes_are_timeouts() {
    let mut s = session(Fake::silent(TimeoutDirection::In));
    let (report, _) = s
        .probe_with_variants(DeviceMode::LegacyBootloader)
        .expect("probe runs");
    assert_eq!(report.variants.len(), 3);
    for v in &report.variants {
        assert_eq!(v.connect, ConnectOutcome::TimedOut, "{:?}", v.variant);
        assert_eq!(v.head, HeadOutcome::TimedOut, "{:?}", v.variant);
    }
}

#[test]
fn qf92wx_report_names_the_variant_that_answered() {
    let mut s = session(Fake::answering_on(ProbeVariant::InBeforeOut));
    let (report, _) = s
        .probe_with_variants(DeviceMode::LegacyBootloader)
        .expect("probe runs");
    assert_eq!(report.answered_variant(), Some(ProbeVariant::InBeforeOut));
    let answered = report
        .variants
        .iter()
        .find(|v| v.variant == ProbeVariant::InBeforeOut)
        .expect("in-before-out ran");
    assert_eq!(answered.connect, ConnectOutcome::Accepted);
    assert_eq!(answered.head, HeadOutcome::Field(7));
}

#[test]
fn qf92wx_every_variant_sends_connect_then_head() {
    let mut s = session(Fake::silent(TimeoutDirection::In));
    s.probe_with_variants(DeviceMode::LegacyBootloader)
        .expect("probe runs");
    let sent: Vec<(ProbeVariant, u8)> = s
        .transport()
        .variant_sent
        .iter()
        .map(|(v, f)| (*v, f[0] & 0x7F))
        .collect();
    assert_eq!(
        sent,
        vec![
            (ProbeVariant::NoDrainNoClearHalt, CONNECT),
            (ProbeVariant::NoDrainNoClearHalt, HEAD),
            (ProbeVariant::InBeforeOut, CONNECT),
            (ProbeVariant::InBeforeOut, HEAD),
            (ProbeVariant::AllInEndpoints, CONNECT),
            (ProbeVariant::AllInEndpoints, HEAD),
        ]
    );
}

#[test]
fn qf92wx_new_bootloader_that_answers_gets_no_variants() {
    let mut s = session(Fake::answering());
    let (report, _) = s
        .probe_with_variants(DeviceMode::Bootloader)
        .expect("probe runs");
    assert!(report.variants.is_empty(), "{:?}", report.variants);
    assert!(s.transport().variant_sent.is_empty());
}

// ---------------------------------------------------------------- AC .5

#[test]
fn qf92wx_application_mode_gets_no_variant_retries() {
    let mut s = session(Fake::silent(TimeoutDirection::In));
    let (report, _) = s
        .probe_with_variants(DeviceMode::Application)
        .expect("probe runs");
    assert_eq!(report.connect, ConnectOutcome::TimedOut);
    assert!(report.variants.is_empty(), "{:?}", report.variants);
    assert!(
        s.transport().variant_sent.is_empty(),
        "no variant frame in application mode"
    );
}

#[test]
fn qf92wx_variant_frames_are_only_connect_or_head() {
    let mut s = session(Fake::silent(TimeoutDirection::In));
    s.probe_with_variants(DeviceMode::LegacyBootloader)
        .expect("probe runs");
    let t = s.transport();
    assert_eq!(t.variant_sent.len(), 6, "two frames per variant");
    let all: Vec<u8> = t
        .sent
        .iter()
        .chain(t.variant_sent.iter().map(|(_, f)| f))
        .map(|f| f[0] & 0x7F)
        .collect();
    assert!(
        all.iter().all(|ty| *ty == CONNECT || *ty == HEAD),
        "frame types sent: {all:02x?}"
    );
}

#[test]
fn qf92wx_probe_frames_build_only_connect_and_head() {
    assert_eq!(ProbeFrame::connect(9).bytes()[0] & 0x7F, CONNECT);
    assert_eq!(ProbeFrame::connect(9).bytes()[1], 9);
    assert_eq!(ProbeFrame::head(4).bytes()[0] & 0x7F, HEAD);
    assert_eq!(ProbeFrame::head(4).bytes()[1], 4);
}
