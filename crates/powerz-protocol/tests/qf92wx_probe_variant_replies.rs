//! FR QF92WX, AC .4: how a variant's replies are classified.
//!
//! - A variant counts as answered when the meter replied to its CONNECT or
//!   its HEAD at all (REJECT included), not only on ACCEPT.
//! - A variant CONNECT reply that is neither ACCEPT nor REJECT is kept raw.
//! - The write-site check lets only CONNECT and HEAD frames through.
//!
//! Over a scripted hand-written fake; every value is synthetic.

mod common;

use std::collections::HashMap;
use std::time::Duration;

use common::*;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::session::{
    ConnectOutcome, HeadOutcome, ProbeFrame, ProbePipe, ProbeVariant, Session, check_probe_frame,
};
use powerz_protocol::transport::{Error as TransportError, Transport};

/// What a variant replies to CONNECT and to HEAD (`None`: times out). The
/// reply id is patched to the request's.
#[derive(Debug, Default, Clone)]
struct Script {
    connect: Option<Vec<u8>>,
    head: Option<Vec<u8>>,
}

/// A pipe whose standard path always times out and whose variants follow
/// their [`Script`] (unscripted variants time out).
#[derive(Debug, Default)]
struct Scripted(HashMap<ProbeVariant, Script>);

impl Transport for Scripted {
    fn transact(&mut self, _request: &[u8], _timeout: Duration) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::Timeout)
    }

    fn read(&mut self, _timeout: Duration) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::Timeout)
    }
}

impl ProbePipe for Scripted {
    fn probe_variant(
        &mut self,
        variant: ProbeVariant,
        frame: &ProbeFrame,
        _timeout: Duration,
    ) -> Result<Vec<u8>, TransportError> {
        let bytes = frame.bytes();
        let script = self.0.get(&variant).cloned().unwrap_or_default();
        let reply = match bytes[0] & 0x7F {
            0x02 => script.connect,
            _ => script.head,
        };
        let mut reply = reply.ok_or(TransportError::Timeout)?;
        if reply.len() > 1 {
            reply[1] = bytes[1];
        }
        Ok(reply)
    }
}

fn run(scripts: Vec<(ProbeVariant, Script)>) -> powerz_protocol::session::ProbeReport {
    let pipe = Scripted(scripts.into_iter().collect());
    let mut s = Session::with_clock(pipe, FakeClock::new(1_790_000_000_000));
    s.probe_with_variants(DeviceMode::LegacyBootloader)
        .expect("probe runs")
        .0
}

fn variant(
    report: &powerz_protocol::session::ProbeReport,
    v: ProbeVariant,
) -> &powerz_protocol::session::VariantOutcome {
    report
        .variants
        .iter()
        .find(|o| o.variant == v)
        .expect("variant ran")
}

#[test]
fn qf92wx_a_variant_whose_connect_is_rejected_counts_as_answered() {
    let report = run(vec![(
        ProbeVariant::NoDrainNoClearHalt,
        Script {
            connect: Some(reject(0)),
            head: None,
        },
    )]);
    let o = variant(&report, ProbeVariant::NoDrainNoClearHalt);
    assert_eq!(o.connect, ConnectOutcome::Rejected);
    assert_eq!(o.head, HeadOutcome::TimedOut);
    assert_eq!(
        report.answered_variant(),
        Some(ProbeVariant::NoDrainNoClearHalt)
    );
}

#[test]
fn qf92wx_a_variant_whose_connect_times_out_but_head_replies_counts_as_answered() {
    let report = run(vec![(
        ProbeVariant::AllInEndpoints,
        Script {
            connect: None,
            head: Some(reject(0)),
        },
    )]);
    let o = variant(&report, ProbeVariant::AllInEndpoints);
    assert_eq!(o.connect, ConnectOutcome::TimedOut);
    assert!(matches!(o.head, HeadOutcome::Rejected(_)), "{:?}", o.head);
    assert_eq!(
        report.answered_variant(),
        Some(ProbeVariant::AllInEndpoints)
    );
}

#[test]
fn qf92wx_the_first_variant_to_reply_wins_even_without_accept() {
    let report = run(vec![
        (
            ProbeVariant::InBeforeOut,
            Script {
                connect: Some(reject(0)),
                head: None,
            },
        ),
        (
            ProbeVariant::AllInEndpoints,
            Script {
                connect: Some(accept(0)),
                head: None,
            },
        ),
    ]);
    assert_eq!(report.answered_variant(), Some(ProbeVariant::InBeforeOut));
}

#[test]
fn qf92wx_a_variant_connect_reply_that_is_neither_accept_nor_reject_is_kept_raw() {
    // Type 0x41 (PUT_DATA): not ACCEPT, not REJECT.
    let report = run(vec![(
        ProbeVariant::InBeforeOut,
        Script {
            connect: Some(vec![0x41, 0x00, 0x00, 0x00]),
            head: None,
        },
    )]);
    let o = variant(&report, ProbeVariant::InBeforeOut);
    match &o.connect {
        ConnectOutcome::Raw(r) => assert_eq!(r[0], 0x41, "{r:02x?}"),
        other => panic!("expected a raw CONNECT reply, got {other:?}"),
    }
    assert_eq!(report.answered_variant(), Some(ProbeVariant::InBeforeOut));
}

#[test]
fn qf92wx_a_variant_connect_reject_stays_rejected_not_raw() {
    let report = run(vec![(
        ProbeVariant::InBeforeOut,
        Script {
            connect: Some(reject(0)),
            head: None,
        },
    )]);
    assert_eq!(
        variant(&report, ProbeVariant::InBeforeOut).connect,
        ConnectOutcome::Rejected
    );
}

#[test]
fn qf92wx_all_silent_variants_answer_none() {
    let report = run(Vec::new());
    assert_eq!(report.variants.len(), 3);
    assert_eq!(report.answered_variant(), None);
}

#[test]
fn qf92wx_write_site_check_passes_connect_and_head_only() {
    assert_eq!(check_probe_frame(ProbeFrame::connect(1).bytes()), Ok(()));
    assert_eq!(check_probe_frame(ProbeFrame::head(2).bytes()), Ok(()));
    // The extend bit is ignored.
    assert_eq!(check_probe_frame(&[0x82, 1, 0, 0]), Ok(()));
    assert_eq!(check_probe_frame(&[0xC0, 1, 0, 0]), Ok(()));
    for ty in [0x09u8, 0x08, 0x42, 0x0A, 0x4C, 0x03] {
        assert_eq!(
            check_probe_frame(&[ty, 1, 0, 0]),
            Err(TransportError::Forbidden { ty }),
            "type {ty:#04x}"
        );
    }
    assert_eq!(
        check_probe_frame(&[]),
        Err(TransportError::Forbidden { ty: 0 })
    );
}

/// A variant sends exactly the standard probe's CONNECT and HEAD frames.
#[test]
fn qf92wx_variant_frames_are_the_standard_connect_and_head() {
    use powerz_protocol::decode::head;
    use powerz_protocol::frame;
    use powerz_protocol::session::ProbeFrame;
    assert_eq!(ProbeFrame::connect(3).bytes(), &frame::connect(3)[..]);
    assert_eq!(ProbeFrame::head(4).bytes(), &head::request(4)[..]);
    assert_eq!(ProbeFrame::head(4).bytes().len(), 20);
}
