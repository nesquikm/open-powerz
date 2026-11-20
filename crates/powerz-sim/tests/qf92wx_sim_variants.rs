//! FR QF92WX against the simulator: the old-bootloader persona's endpoints
//! and string descriptors (AC .1, .2), the direction of its timeouts
//! (AC .3), the read-only probe variants over `ProbePipe`, with the persona
//! told to answer on one of them (AC .4), and a frame log proving every
//! frame sent in every variant is CONNECT (0x02) or HEAD (0x40), with no
//! variant retries in application mode (AC .5).
//!
//! Every identifier is synthetic.

mod common;

use std::time::Duration;

use common::*;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::session::{
    ConnectOutcome, ProbeFrame, ProbePipe, ProbeReport, ProbeVariant, Session,
};
use powerz_protocol::transport::discovery::{EndpointDirection, StringDescriptor};
use powerz_protocol::transport::{Error as TransportError, TimeoutDirection, Transport};

const CONNECT: u8 = 0x02;
const HEAD: u8 = 0x40;

/// Wrapper that logs every frame the probe hands the simulator, on the
/// standard path and in each variant.
#[derive(Debug)]
struct Logged {
    inner: SimDevice,
    sent: Vec<Vec<u8>>,
    variant_sent: Vec<(ProbeVariant, Vec<u8>)>,
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
    fn last_timeout_direction(&self) -> Option<TimeoutDirection> {
        self.inner.last_timeout_direction()
    }
}

impl ProbePipe for Logged {
    fn probe_variant(
        &mut self,
        variant: ProbeVariant,
        frame: &ProbeFrame,
        timeout: Duration,
    ) -> Result<Vec<u8>, TransportError> {
        self.variant_sent.push((variant, frame.bytes().to_vec()));
        self.inner.probe_variant(variant, frame, timeout)
    }
}

fn sim(mode: DeviceMode, clock: &SimClock) -> SimDevice {
    device(Persona::Bulk, Scenario::Idle, clock).with_mode(mode)
}

/// The frames a probe run handed the simulator.
#[derive(Debug)]
struct Log {
    sent: Vec<Vec<u8>>,
    variant_sent: Vec<(ProbeVariant, Vec<u8>)>,
}

/// Run the probe with variants on `dev` (in `mode`); return the report and
/// the frame log.
fn probe(dev: SimDevice, mode: DeviceMode, clock: &SimClock) -> (ProbeReport, Log) {
    let mut s = Session::with_clock(
        Logged {
            inner: dev,
            sent: Vec::new(),
            variant_sent: Vec::new(),
        },
        clock.clone(),
    );
    let (report, _) = s
        .probe_with_variants(mode)
        .unwrap_or_else(|e| panic!("probe failed: {e}"));
    let t = s.transport();
    let log = Log {
        sent: t.sent.clone(),
        variant_sent: t.variant_sent.clone(),
    };
    (report, log)
}

fn old_bootloader(clock: &SimClock) -> SimDevice {
    sim(DeviceMode::LegacyBootloader, clock)
}

// ---------------------------------------------------------------- AC .1 / .2

#[test]
fn qf92wx_sim_old_bootloader_lists_in_and_out_endpoints_on_interface_0() {
    let c = clock();
    let details = old_bootloader(&c).usb_details();
    let eps = details.endpoints_of(0);
    assert!(
        eps.iter().any(|e| e.direction == EndpointDirection::In),
        "an IN endpoint: {eps:?}"
    );
    assert!(
        eps.iter().any(|e| e.direction == EndpointDirection::Out),
        "an OUT endpoint: {eps:?}"
    );
}

#[test]
fn qf92wx_sim_application_mode_lists_endpoints_for_every_interface() {
    let c = clock();
    let details = sim(DeviceMode::Application, &c).usb_details();
    for n in 0..=3u8 {
        assert!(
            !details.endpoints_of(n).is_empty(),
            "interface {n} has endpoints: {details:?}"
        );
    }
}

#[test]
fn qf92wx_sim_strings_cover_indices_1_to_4() {
    let c = clock();
    let details = old_bootloader(&c).usb_details();
    let indices: Vec<u8> = details.strings.iter().map(|(i, _)| *i).collect();
    assert_eq!(indices, vec![1, 2, 3, 4]);
}

#[test]
fn qf92wx_sim_old_bootloader_string_2_is_its_interface_string() {
    let c = clock();
    let details = old_bootloader(&c).usb_details();
    let two = details
        .strings
        .iter()
        .find(|(i, _)| *i == 2)
        .map(|(_, s)| s.clone());
    assert_eq!(
        two,
        Some(StringDescriptor::Text("POWER-Z KM003C".to_owned()))
    );
}

// ---------------------------------------------------------------- AC .3

#[test]
fn qf92wx_sim_old_bootloader_times_out_on_the_in_read() {
    let c = clock();
    let (report, _) = probe(old_bootloader(&c), DeviceMode::LegacyBootloader, &c);
    assert_eq!(report.connect, ConnectOutcome::TimedOut);
    assert_eq!(report.connect_timeout, Some(TimeoutDirection::In));
    assert_eq!(report.head_timeout, Some(TimeoutDirection::In));
}

// ---------------------------------------------------------------- AC .4

#[test]
fn qf92wx_sim_old_bootloader_answers_no_variant_by_default() {
    let c = clock();
    let (report, _) = probe(old_bootloader(&c), DeviceMode::LegacyBootloader, &c);
    assert_eq!(report.variants.len(), 3, "{:?}", report.variants);
    assert_eq!(report.answered_variant(), None);
}

#[test]
fn qf92wx_sim_old_bootloader_answers_on_the_variant_it_is_told() {
    for v in ProbeVariant::ALL {
        let c = clock();
        let dev = old_bootloader(&c).with_probe_answer_on(v);
        let (report, _) = probe(dev, DeviceMode::LegacyBootloader, &c);
        assert_eq!(report.answered_variant(), Some(v), "{report:?}");
    }
}

#[test]
fn qf92wx_sim_answering_variant_accepts_connect() {
    let c = clock();
    let dev = old_bootloader(&c).with_probe_answer_on(ProbeVariant::AllInEndpoints);
    let (report, _) = probe(dev, DeviceMode::LegacyBootloader, &c);
    let outcome = report
        .variants
        .iter()
        .find(|o| o.variant == ProbeVariant::AllInEndpoints)
        .expect("all-in-endpoints ran");
    assert_eq!(outcome.connect, ConnectOutcome::Accepted);
}

#[test]
fn qf92wx_sim_variant_name_round_trips() {
    for v in ProbeVariant::ALL {
        assert_eq!(ProbeVariant::from_name(v.name()), Some(v));
    }
    assert_eq!(ProbeVariant::from_name("bogus"), None);
}

// ---------------------------------------------------------------- AC .5

#[test]
fn qf92wx_sim_frame_log_holds_only_connect_and_head_in_every_variant() {
    let answers: Vec<Option<ProbeVariant>> = std::iter::once(None)
        .chain(ProbeVariant::ALL.into_iter().map(Some))
        .collect();
    for answer in answers {
        let c = clock();
        let dev = match answer {
            Some(v) => old_bootloader(&c).with_probe_answer_on(v),
            None => old_bootloader(&c),
        };
        let (_, log) = probe(dev, DeviceMode::LegacyBootloader, &c);
        assert_eq!(
            log.variant_sent.len(),
            6,
            "{answer:?}: CONNECT and HEAD per variant"
        );
        let types: Vec<u8> = log
            .sent
            .iter()
            .chain(log.variant_sent.iter().map(|(_, f)| f))
            .map(|f| f[0] & 0x7F)
            .collect();
        assert!(
            types.iter().all(|t| *t == CONNECT || *t == HEAD),
            "{answer:?}: frame types sent {types:02x?}"
        );
    }
}

#[test]
fn qf92wx_sim_application_mode_timeout_gets_no_variant_retries() {
    let c = clock();
    let dev = sim(DeviceMode::Application, &c).with_faults(FaultPlan::new().timeout_always());
    let (report, log) = probe(dev, DeviceMode::Application, &c);
    assert_eq!(report.connect, ConnectOutcome::TimedOut);
    assert!(report.variants.is_empty(), "{:?}", report.variants);
    assert!(log.variant_sent.is_empty(), "{:?}", log.variant_sent);
}
