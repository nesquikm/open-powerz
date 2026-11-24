//! The read-only firmware probe (FR V9SK56, AC .6): CONNECT, then HEAD,
//! and nothing else, in application or bootloader mode.

use super::{Clock, Error, Session, is_sync};
use crate::decode::head::{self, HeadReply};
use crate::firmware::DeviceMode;
use crate::frame::msg;
use crate::transport::{self, TimeoutDirection, Transport};

/// How the meter answered CONNECT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectOutcome {
    /// ACCEPT.
    Accepted,
    /// REJECT.
    Rejected,
    /// A reply that is neither ACCEPT nor REJECT, kept as sent. Only a
    /// [`ProbeVariant`]'s CONNECT reports this; the standard path's
    /// CONNECT does not.
    Raw(Vec<u8>),
    /// No answer, even after one recovery and retry.
    TimedOut,
}

/// How the meter answered HEAD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadOutcome {
    /// A HEAD reply with this old-firmware field.
    Field(u32),
    /// A reply that does not decode as HEAD, kept as sent.
    Raw(Vec<u8>),
    /// REJECT, kept as sent.
    Rejected(Vec<u8>),
    /// No answer.
    TimedOut,
}

/// A reply id that differs from the request id it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdMismatch {
    /// The id the meter replied with.
    pub meter: u8,
    /// The id we sent.
    pub ours: u8,
}

impl IdMismatch {
    /// The ids of `reply` and the `request` it answers, when they differ.
    pub(super) fn between(reply: &[u8], request: &[u8]) -> Option<IdMismatch> {
        match (reply.get(1), request.get(1)) {
            (Some(&meter), Some(&ours)) if meter != ours => Some(IdMismatch { meter, ours }),
            _ => None,
        }
    }
}

/// What [`Session::probe`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    /// The answer to CONNECT.
    pub connect: ConnectOutcome,
    /// The answer to HEAD.
    pub head: HeadOutcome,
    /// Which half of the CONNECT exchange timed out, if it did.
    pub connect_timeout: Option<TimeoutDirection>,
    /// Which half of the HEAD exchange timed out, if it did.
    pub head_timeout: Option<TimeoutDirection>,
    /// The read-only variants run after a silent standard path, in order.
    pub variants: Vec<VariantOutcome>,
    /// The ids of the CONNECT exchange, when the meter's differs from ours.
    pub connect_ids: Option<IdMismatch>,
    /// The ids of the HEAD exchange, when the meter's differs from ours.
    pub head_ids: Option<IdMismatch>,
}

impl ProbeReport {
    /// The first variant the meter answered at all, if any: its CONNECT
    /// or its HEAD got a reply (ACCEPT, REJECT or anything else), not a
    /// timeout.
    pub fn answered_variant(&self) -> Option<ProbeVariant> {
        self.variants
            .iter()
            .find(|v| v.answered())
            .map(|v| v.variant)
    }
}

/// A read-only way of running CONNECT and HEAD on a pipe (FR QF92WX).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProbeVariant {
    /// No drain and no clear-halt before the write.
    NoDrainNoClearHalt,
    /// Post the IN read before the OUT write.
    InBeforeOut,
    /// Read every IN endpoint of the vendor interface.
    AllInEndpoints,
}

impl ProbeVariant {
    /// Every variant, in the order the probe runs them.
    pub const ALL: [ProbeVariant; 3] = [
        ProbeVariant::NoDrainNoClearHalt,
        ProbeVariant::InBeforeOut,
        ProbeVariant::AllInEndpoints,
    ];

    /// Kebab-case name.
    pub fn name(self) -> &'static str {
        match self {
            ProbeVariant::NoDrainNoClearHalt => "no-drain-no-clear-halt",
            ProbeVariant::InBeforeOut => "in-before-out",
            ProbeVariant::AllInEndpoints => "all-in-endpoints",
        }
    }

    /// The variant named `name`, if any.
    pub fn from_name(name: &str) -> Option<ProbeVariant> {
        ProbeVariant::ALL.into_iter().find(|v| v.name() == name)
    }
}

/// A frame a variant may send: only CONNECT or HEAD can be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeFrame(Vec<u8>);

impl ProbeFrame {
    /// CONNECT with transaction id `id`.
    pub fn connect(id: u8) -> ProbeFrame {
        ProbeFrame(crate::frame::connect(id).to_vec())
    }

    /// HEAD with transaction id `id`: byte for byte the standard probe's
    /// HEAD (`40 id 00 01` and 16 zero bytes), so a variant differs from the
    /// standard path only in how the pipe is driven.
    pub fn head(id: u8) -> ProbeFrame {
        ProbeFrame(head::request(id).to_vec())
    }

    /// The frame bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

/// The write-site check for a probe frame: `bytes` must start with a
/// CONNECT (0x02) or HEAD (0x40) type byte (extend bit ignored), else
/// [`transport::Error::Forbidden`] and nothing may be written. An empty
/// frame is refused as type 0.
pub fn check_probe_frame(bytes: &[u8]) -> Result<(), transport::Error> {
    match bytes.first().map(|b| b & 0x7F) {
        Some(msg::CONNECT | msg::HEAD) => Ok(()),
        ty => Err(transport::Error::Forbidden {
            ty: ty.unwrap_or(0),
        }),
    }
}

/// A pipe that can run one probe frame the way a [`ProbeVariant`] says.
pub trait ProbePipe: Transport {
    /// Send `frame` the `variant` way and return the reply.
    fn probe_variant(
        &mut self,
        variant: ProbeVariant,
        frame: &ProbeFrame,
        timeout: std::time::Duration,
    ) -> Result<Vec<u8>, transport::Error>;
}

/// How the meter answered one variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantOutcome {
    /// The variant.
    pub variant: ProbeVariant,
    /// The answer to its CONNECT.
    pub connect: ConnectOutcome,
    /// The answer to its HEAD.
    pub head: HeadOutcome,
    /// The ids of its CONNECT, when the meter's differs from ours.
    pub connect_ids: Option<IdMismatch>,
    /// The ids of its HEAD, when the meter's differs from ours.
    pub head_ids: Option<IdMismatch>,
}

impl VariantOutcome {
    /// The meter replied to this variant's CONNECT or HEAD at all.
    pub fn answered(&self) -> bool {
        self.connect != ConnectOutcome::TimedOut || self.head != HeadOutcome::TimedOut
    }
}

impl<T: Transport, C: Clock> Session<T, C> {
    /// Send CONNECT, then (whatever CONNECT gave) one HEAD, and report both
    /// answers. Only CONNECT (0x02) and HEAD (0x40) frames are sent; a
    /// REJECT, a timeout or an undecodable reply is data; any other error
    /// is returned (no HEAD is sent after such a CONNECT failure). A SYNC
    /// reply of any id to either frame is
    /// [`Error::BootloaderMidDownload`]: nothing more is sent.
    pub fn probe(&mut self) -> Result<ProbeReport, Error> {
        self.probe_with_head_reply().map(|(report, _)| report)
    }

    /// [`Session::probe`], also returning the HEAD reply exactly as the
    /// meter sent it (`None` when HEAD timed out), so a caller can show the
    /// raw bytes next to a decoded field.
    pub fn probe_with_head_reply(&mut self) -> Result<(ProbeReport, Option<Vec<u8>>), Error> {
        self.last_ids = None;
        let connect = match self.connect_checked(true) {
            Ok(()) => ConnectOutcome::Accepted,
            Err(Error::Rejected { .. }) => ConnectOutcome::Rejected,
            Err(Error::Transport(transport::Error::Timeout)) => ConnectOutcome::TimedOut,
            Err(e) => return Err(e),
        };
        let connect_timeout = match connect {
            ConnectOutcome::TimedOut => self.transport.last_timeout_direction(),
            _ => None,
        };
        let connect_ids = match connect {
            ConnectOutcome::Accepted => self.last_ids.take(),
            _ => None,
        };
        self.last_ids = None;
        let (head, reply) = match self.head_exchange_taking_sync() {
            Ok(reply) => (head_outcome(reply.clone())?, reply),
            Err(Error::Transport(transport::Error::Timeout)) => {
                return Ok((
                    ProbeReport {
                        connect,
                        head: HeadOutcome::TimedOut,
                        connect_timeout,
                        head_timeout: self.transport.last_timeout_direction(),
                        variants: Vec::new(),
                        connect_ids,
                        head_ids: None,
                    },
                    None,
                ));
            }
            Err(e) => return Err(e),
        };
        // Like the variants: only a HEAD that gave a field counts.
        let head_ids = match head {
            HeadOutcome::Field(_) => self.last_ids.take(),
            _ => None,
        };
        Ok((
            ProbeReport {
                connect,
                head,
                connect_timeout,
                head_timeout: None,
                variants: Vec::new(),
                connect_ids,
                head_ids,
            },
            Some(reply),
        ))
    }
}

impl<T: ProbePipe, C: Clock> Session<T, C> {
    /// [`Session::probe_with_head_reply`] on a meter in `mode`; outside
    /// application mode, a timed-out CONNECT is followed by every
    /// [`ProbeVariant`] in [`ProbeVariant::ALL`] order, each sending one
    /// CONNECT and then one HEAD (FR QF92WX, AC .4). A variant's REJECT or
    /// timeout is data; a SYNC reply is [`Error::BootloaderMidDownload`].
    pub fn probe_with_variants(
        &mut self,
        mode: DeviceMode,
    ) -> Result<(ProbeReport, Option<Vec<u8>>), Error> {
        let policy = self.reply_ids;
        if mode == DeviceMode::LegacyBootloader {
            self.expect_own_reply_ids();
        }
        let probed = self.probe_with_head_reply();
        self.reply_ids = policy;
        let (mut report, reply) = probed?;
        if mode != DeviceMode::Application && report.connect == ConnectOutcome::TimedOut {
            for variant in ProbeVariant::ALL {
                let outcome = self.run_variant(variant)?;
                report.variants.push(outcome);
            }
        }
        Ok((report, reply))
    }

    /// CONNECT, then HEAD, the `variant` way.
    fn run_variant(&mut self, variant: ProbeVariant) -> Result<VariantOutcome, Error> {
        let frame = ProbeFrame::connect(self.counter.next());
        let timeout = transport::default_timeout(frame.bytes());
        let mut connect_ids = None;
        let connect = match self.transport.probe_variant(variant, &frame, timeout) {
            Ok(reply) if is_sync(&reply) => return Err(Error::BootloaderMidDownload),
            Ok(reply) if reply.first().map(|b| b & 0x7F) == Some(msg::ACCEPT) => {
                connect_ids = IdMismatch::between(&reply, frame.bytes());
                ConnectOutcome::Accepted
            }
            Ok(reply) if head::is_reject(&reply) => ConnectOutcome::Rejected,
            // Neither ACCEPT nor REJECT: kept so the output shows it.
            Ok(reply) => ConnectOutcome::Raw(reply),
            Err(transport::Error::Timeout) => ConnectOutcome::TimedOut,
            Err(e) => return Err(e.into()),
        };
        let frame = ProbeFrame::head(self.counter.next());
        let timeout = transport::default_timeout(frame.bytes());
        let mut head_ids = None;
        let head = match self.transport.probe_variant(variant, &frame, timeout) {
            Ok(reply) => {
                let ids = IdMismatch::between(&reply, frame.bytes());
                let outcome = head_outcome(reply)?;
                if matches!(outcome, HeadOutcome::Field(_)) {
                    head_ids = ids;
                }
                outcome
            }
            Err(transport::Error::Timeout) => HeadOutcome::TimedOut,
            Err(e) => return Err(e.into()),
        };
        Ok(VariantOutcome {
            variant,
            connect,
            head,
            connect_ids,
            head_ids,
        })
    }
}

/// Classify a HEAD reply: SYNC is [`Error::BootloaderMidDownload`], then
/// REJECT, a decoded field, or the raw bytes.
fn head_outcome(reply: Vec<u8>) -> Result<HeadOutcome, Error> {
    if is_sync(&reply) {
        return Err(Error::BootloaderMidDownload);
    }
    if head::is_reject(&reply) {
        return Ok(HeadOutcome::Rejected(reply));
    }
    Ok(match HeadReply::decode(&reply) {
        Ok(h) => HeadOutcome::Field(h.old_firmware_field()),
        Err(_) => HeadOutcome::Raw(reply),
    })
}
