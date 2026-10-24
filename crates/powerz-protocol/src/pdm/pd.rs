//! USB PD triggering on top of the PDM session (FR DGSQ6P).
//!
//! - [`offers`]: the charger's offers from the last `pdo:` block, decoded
//!   with the PD decoder (position, kind, voltage or range, current/power).
//! - [`validate`]: check a [`Request`] against the offers and render the
//!   device command (`pd req=<pos>[,volt=<mV>,cur=<mA>]`); nothing is sent.
//! - [`enter`], [`request`], [`hard_reset`]: the session flows, with the
//!   `req:` / `ready:` replies parsed into [`Reading`]s.
//! - [`PdSettings`]: `pdm parm` / `pdm set` with named values.
//!
//! EPR positions are 8 and above. \[device\] (session 2)

use std::io::{Read, Write};
use std::time::Duration;

use super::caps::Capabilities;
use super::command::{Command, Protocol};
use super::risk::Risk;
use super::session::{HighVoltageConfirmation, PdmSession, REPLY_TIMEOUT};
use crate::pd::SourcePdo;
use crate::session::Clock;

/// How long `entry pd` may take to reach its first `ready:`. \[inferred\]
pub const ENTRY_TIMEOUT: Duration = Duration::from_millis(3000);
/// How long to keep listening after a `ready:` for another `pdo:` block
/// (the EPR list follows the SPR one unprompted). On the 140 W charger the
/// EPR list came about 2.7 s after the first `ready:`; with 1 s the meter
/// was still entering EPR when the next request went out, and ignored it.
/// \[device\]
pub const EPR_SETTLE: Duration = Duration::from_millis(4000);
/// Most `ready:` lines one negotiation (`entry pd`, `pd hrst`) reads.
pub const MAX_READY_LINES: usize = 4;

/// First object position of the EPR part of the source capabilities.
pub const FIRST_EPR_POSITION: u8 = 8;

/// What kind of supply an offer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferKind {
    /// Fixed supply.
    Fixed,
    /// Battery supply.
    Battery,
    /// Variable supply.
    Variable,
    /// SPR PPS APDO.
    Pps,
    /// SPR AVS APDO.
    SprAvs,
    /// EPR AVS APDO.
    EprAvs,
}

impl OfferKind {
    /// Short machine name (`fixed`, `battery`, `variable`, `pps`, `spr-avs`, `epr-avs`).
    pub fn name(self) -> &'static str {
        match self {
            OfferKind::Fixed => "fixed",
            OfferKind::Battery => "battery",
            OfferKind::Variable => "variable",
            OfferKind::Pps => "pps",
            OfferKind::SprAvs => "spr-avs",
            OfferKind::EprAvs => "epr-avs",
        }
    }

    /// Whether requests for this kind carry a voltage (PPS / AVS).
    pub fn is_programmable(self) -> bool {
        matches!(self, OfferKind::Pps | OfferKind::SprAvs | OfferKind::EprAvs)
    }
}

/// One offer of the charger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// 1-based object position (the slot `pd req=` takes).
    pub position: u8,
    /// Supply kind.
    pub kind: OfferKind,
    /// Lowest voltage, mV (equal to the highest for a fixed supply).
    pub min_voltage_mv: u32,
    /// Highest voltage, mV.
    pub max_voltage_mv: u32,
    /// Maximum current, mA (`None` for power-limited offers: battery, EPR AVS).
    pub max_current_ma: Option<u32>,
    /// Maximum power, mW (battery, EPR AVS), else `None`.
    pub max_power_mw: Option<u32>,
    /// The decoded PDO.
    pub pdo: SourcePdo,
}

impl Offer {
    /// The offer for `pdo` at `position`; `None` for a reserved APDO.
    pub fn from_pdo(position: u8, pdo: SourcePdo) -> Option<Offer> {
        // (kind, min mV, max mV, max mA, max mW)
        let (kind, min, max, current, power) = match pdo {
            SourcePdo::Fixed(f) => (
                OfferKind::Fixed,
                f.voltage_mv,
                f.voltage_mv,
                Some(f.max_current_ma),
                None,
            ),
            SourcePdo::Battery(b) => (
                OfferKind::Battery,
                b.min_voltage_mv,
                b.max_voltage_mv,
                None,
                Some(b.power_mw),
            ),
            SourcePdo::Variable(v) => (
                OfferKind::Variable,
                v.min_voltage_mv,
                v.max_voltage_mv,
                Some(v.current_ma),
                None,
            ),
            SourcePdo::Pps(p) => (
                OfferKind::Pps,
                p.min_voltage_mv,
                p.max_voltage_mv,
                Some(p.max_current_ma),
                None,
            ),
            // SPR AVS spans 9-15 V, and up to 20 V when the 20 V current
            // is non-zero (USB PD r3.2 6.4.1.2.4.2). [doc]
            SourcePdo::SprAvs(a) => (
                OfferKind::SprAvs,
                9000,
                if a.max_current_20v_ma > 0 {
                    20_000
                } else {
                    15_000
                },
                Some(a.max_current_15v_ma),
                None,
            ),
            SourcePdo::EprAvs(a) => (
                OfferKind::EprAvs,
                a.min_voltage_mv,
                a.max_voltage_mv,
                None,
                Some(a.pdp_w.saturating_mul(1000)),
            ),
            SourcePdo::Reserved(_) => return None,
        };
        Some(Offer {
            position,
            kind,
            min_voltage_mv: min,
            max_voltage_mv: max,
            max_current_ma: current,
            max_power_mw: power,
            pdo,
        })
    }

    /// Whether this is an EPR offer (position 8 or above).
    pub fn is_epr(&self) -> bool {
        self.position >= FIRST_EPR_POSITION
    }
}

/// Every offer in the last `pdo:` block of `caps`, in position order.
pub fn offers(caps: &Capabilities) -> Vec<Offer> {
    caps.pd
        .as_ref()
        .map(|block| {
            block
                .offers()
                .into_iter()
                .filter_map(|(position, pdo)| Offer::from_pdo(position, pdo))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the offers include EPR positions (the meter entered EPR mode).
pub fn epr_entered(offers: &[Offer]) -> bool {
    offers.iter().any(Offer::is_epr)
}

/// A PD request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// A fixed (or variable / battery) offer by position; `current_ma`
    /// `None` asks for the offer's maximum current.
    Fixed {
        /// Object position.
        position: u8,
        /// Current, mA.
        current_ma: Option<u32>,
    },
    /// A PPS or AVS offer with a voltage and a current.
    Programmable {
        /// Object position.
        position: u8,
        /// Voltage, mV.
        voltage_mv: u32,
        /// Current, mA.
        current_ma: u32,
    },
}

/// A voltage / current pair from a `req:` or `ready:` line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// Voltage, mV.
    pub voltage_mv: u32,
    /// Current, mA.
    pub current_ma: u32,
}

/// Parse `req:<mV>mV,<mA>mA`. \[device\]
pub fn parse_req(line: &str) -> Option<Reading> {
    parse_reading(line, "req:")
}

/// Parse `ready:<mV>mV,<mA>mA`. \[device\]
pub fn parse_ready(line: &str) -> Option<Reading> {
    parse_reading(line, "ready:")
}

/// `<prefix><mV>mV,<mA>mA`, surrounding whitespace ignored.
fn parse_reading(line: &str, prefix: &str) -> Option<Reading> {
    let rest = line.trim().strip_prefix(prefix)?;
    let (v, i) = rest.split_once(',')?;
    Some(Reading {
        voltage_mv: v.trim().strip_suffix("mV")?.parse().ok()?,
        current_ma: i.trim().strip_suffix("mA")?.parse().ok()?,
    })
}

/// The result of one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestOutcome {
    /// The command that was sent.
    pub command: Command,
    /// What the meter asked for (`req:`).
    pub requested: Reading,
    /// VBUS / IBUS the meter measured at PS_RDY (`ready:`); VBUS may still
    /// be ramping then (8554 mV for a 9 V request that settled at 8.87 V).
    /// \[device\]
    pub delivered: Reading,
}

/// Offers received after `entry pd` or a hard reset, and VBUS once ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Negotiation {
    /// The latest offers (EPR list when the meter entered EPR).
    pub offers: Vec<Offer>,
    /// The last `ready:` reading.
    pub vbus: Reading,
}

/// Errors of the PD trigger layer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A PDM session error.
    #[error(transparent)]
    Pdm(#[from] super::Error),
    /// No offer at that position.
    #[error("the charger offers nothing at position {position}")]
    NoOffer {
        /// Requested position.
        position: u8,
    },
    /// An EPR position was requested but the meter did not enter EPR mode.
    #[error("position {position} is EPR, but the charger or cable did not enter EPR mode")]
    EprUnavailable {
        /// Requested position.
        position: u8,
    },
    /// The voltage is outside the offer's range.
    #[error("{voltage_mv} mV is outside {min_mv}-{max_mv} mV offered at position {position}")]
    VoltageOutOfRange {
        /// Requested position.
        position: u8,
        /// Requested voltage, mV.
        voltage_mv: u32,
        /// Lowest offered, mV.
        min_mv: u32,
        /// Highest offered, mV.
        max_mv: u32,
    },
    /// The current is above what the offer allows.
    #[error("{current_ma} mA is above the {max_ma} mA allowed at position {position}")]
    CurrentOutOfRange {
        /// Requested position.
        position: u8,
        /// Requested current, mA.
        current_ma: u32,
        /// Highest allowed at the requested voltage, mA.
        max_ma: u32,
    },
    /// A PPS / AVS offer was requested without a voltage.
    #[error("position {position} is a PPS/AVS offer: give a voltage and a current")]
    NeedsVoltage {
        /// Requested position.
        position: u8,
    },
    /// A voltage was given for an offer that is not PPS / AVS.
    #[error("position {position} is not a PPS/AVS offer: no voltage can be chosen")]
    NotProgrammable {
        /// Requested position.
        position: u8,
    },
    /// The meter answered the request with `req error <n>`: the request
    /// failed. The session stays in step (it is the request's own reply).
    #[error("the request failed: the meter answered {reply:?}")]
    RequestFailed {
        /// The `req error` line.
        reply: String,
    },
    /// `pdm parm` did not have the expected form.
    #[error("unexpected pdm parm reply {got:?}")]
    BadSettings {
        /// The line.
        got: String,
    },
}

/// Check `request` against `offers` and render its command; pure.
pub fn validate(offers: &[Offer], request: &Request) -> Result<Command, Error> {
    let position = match *request {
        Request::Fixed { position, .. } | Request::Programmable { position, .. } => position,
    };
    let Some(offer) = offers.iter().find(|o| o.position == position) else {
        // Without the EPR list the meter has no positions 8 and above.
        if position >= FIRST_EPR_POSITION && !epr_entered(offers) {
            return Err(Error::EprUnavailable { position });
        }
        return Err(Error::NoOffer { position });
    };
    match *request {
        Request::Fixed { current_ma, .. } => {
            if offer.kind.is_programmable() {
                return Err(Error::NeedsVoltage { position });
            }
            if let Some(current_ma) = current_ma {
                check_current(offer, offer.max_voltage_mv, current_ma)?;
            }
            Ok(Command::PdRequest {
                position,
                voltage_mv: None,
                current_ma,
            })
        }
        Request::Programmable {
            voltage_mv,
            current_ma,
            ..
        } => {
            if !offer.kind.is_programmable() {
                return Err(Error::NotProgrammable { position });
            }
            if voltage_mv < offer.min_voltage_mv || voltage_mv > offer.max_voltage_mv {
                return Err(Error::VoltageOutOfRange {
                    position,
                    voltage_mv,
                    min_mv: offer.min_voltage_mv,
                    max_mv: offer.max_voltage_mv,
                });
            }
            check_current(offer, voltage_mv, current_ma)?;
            Ok(Command::PdRequest {
                position,
                voltage_mv: Some(voltage_mv),
                current_ma: Some(current_ma),
            })
        }
    }
}

/// The most current `offer` allows at `voltage_mv`, mA.
fn max_current_ma(offer: &Offer, voltage_mv: u32) -> u32 {
    match offer.pdo {
        // SPR AVS: the 15 V current up to 15 V, the 20 V current above.
        // (USB PD r3.2 6.4.1.2.4.2) [doc]
        SourcePdo::SprAvs(a) if voltage_mv > 15_000 => a.max_current_20v_ma,
        _ => match (offer.max_current_ma, offer.max_power_mw) {
            (Some(ma), _) => ma,
            // Power-limited (battery, EPR AVS): P / V.
            (None, Some(mw)) => {
                let ma = u64::from(mw) * 1000 / u64::from(voltage_mv.max(1));
                u32::try_from(ma).unwrap_or(u32::MAX)
            }
            (None, None) => 0,
        },
    }
}

/// [`Error::CurrentOutOfRange`] when `current_ma` is above what `offer`
/// allows at `voltage_mv`.
fn check_current(offer: &Offer, voltage_mv: u32, current_ma: u32) -> Result<(), Error> {
    let max_ma = max_current_ma(offer, voltage_mv);
    if current_ma > max_ma {
        return Err(Error::CurrentOutOfRange {
            position: offer.position,
            current_ma,
            max_ma,
        });
    }
    Ok(())
}

/// `entry pd`, then collect the offers and the final `ready:`.
pub fn enter<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
) -> Result<Negotiation, Error> {
    session.send(&Command::Entry(Protocol::Pd))?;
    negotiation(session)
}

/// Collect the unprompted `pdo:` blocks and `ready:` lines that follow
/// `entry pd` / `pd hrst`: the SPR list, its `ready:`, then (when the meter
/// enters EPR) the EPR list and a second `ready:`. \[device\] Done once no
/// further `ready:` comes within [`EPR_SETTLE`]. \[inferred\]
fn negotiation<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
) -> Result<Negotiation, Error> {
    let mut vbus = reading(&session.wait_for(&["ready:"], ENTRY_TIMEOUT)?, "ready:")?;
    // SPR then EPR gives two `ready:` lines [device]; a few more are allowed,
    // but a meter that keeps sending them cannot hold the call open. Once
    // the EPR list is in there is nothing more to wait for.
    for _ in 1..MAX_READY_LINES {
        if epr_entered(&offers(session.capabilities())) {
            break;
        }
        match session.wait_for(&["ready:"], EPR_SETTLE) {
            Ok(line) => vbus = reading(&line, "ready:")?,
            Err(super::Error::Timeout { .. }) => break,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(Negotiation {
        offers: offers(session.capabilities()),
        vbus,
    })
}

/// `entry pd` unless PD is the protocol entered last
/// ([`PdmSession::entered`]); the new negotiation when it was sent. Another
/// protocol entered after PD leaves the meter answering `pd req=` with
/// `req error 252`. \[device\] 2026-10-24
pub fn ensure_entered<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
) -> Result<Option<Negotiation>, Error> {
    if session.entered() == Some(Protocol::Pd) {
        return Ok(None);
    }
    enter(session).map(Some)
}

/// Validate, enter PD again when another protocol (or none) was entered
/// after it ([`ensure_entered`]; the request is then checked against the
/// refreshed offers), send (with `confirmation` when above 5 V) and read
/// `req:` / `ready:`.
pub fn request<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    offers: &[Offer],
    request: &Request,
    confirmation: Option<HighVoltageConfirmation>,
) -> Result<RequestOutcome, Error> {
    // Validation first: an invalid request writes nothing.
    validate(offers, request)?;
    let command = match ensure_entered(session)? {
        // The charger may offer less now (SPR only): nothing is written
        // for a position it no longer offers.
        Some(negotiation) => validate(&negotiation.offers, request)?,
        None => validate(offers, request)?,
    };
    send_request(session, command, confirmation)
}

/// Send a validated `pd req=` and read `req:` / `ready:`; no entry.
fn send_request<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    command: Command,
    confirmation: Option<HighVoltageConfirmation>,
) -> Result<RequestOutcome, Error> {
    match confirmation {
        Some(confirmation) if session.risk(&command) == Risk::Above5V => {
            session.send_confirmed(&command, confirmation)?;
        }
        // At or below 5 V needs no confirmation; above 5 V without one is
        // refused by `send` before anything is written.
        _ => session.send(&command)?,
    }
    // `req error <n>` is this request's own reply: it fails the request at
    // once instead of being waited out. [device] `req error 252`
    let line = session.wait_for(&["req:", "req error"], REPLY_TIMEOUT)?;
    if line.starts_with("req error") {
        return Err(Error::RequestFailed {
            reply: line.trim().to_string(),
        });
    }
    let requested = reading(&line, "req:")?;
    let delivered = reading(&session.wait_for(&["ready:"], ENTRY_TIMEOUT)?, "ready:")?;
    Ok(RequestOutcome {
        command,
        requested,
        delivered,
    })
}

/// A `req:` / `ready:` line (by `prefix`), parsed.
fn reading(line: &str, prefix: &str) -> Result<Reading, Error> {
    parse_reading(line, prefix).ok_or_else(|| {
        Error::Pdm(super::Error::UnexpectedReply {
            expected: format!("{prefix}<mV>mV,<mA>mA"),
            got: line.to_string(),
        })
    })
}

/// `pd hrst`, then collect the new offers and the `ready:` at 5 V.
pub fn hard_reset<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
) -> Result<Negotiation, Error> {
    session.send(&Command::PdHardReset)?;
    negotiation(session)
}

/// How [`return_to_5v`] got the charger back to 5 V.
#[derive(Debug)]
pub enum BackTo5V {
    /// `pd req=1` was answered.
    Requested,
    /// `pd req=1` failed with `request_error`; a Hard Reset did it instead.
    HardReset {
        /// Why `pd req=1` failed.
        request_error: Error,
    },
}

/// Neither `pd req=1` nor the Hard Reset fallback worked: VBUS may still be
/// above 5 V.
#[derive(Debug, thiserror::Error)]
#[error("could not return the charger to 5 V ({request}; Hard Reset: {hard_reset})")]
pub struct ReturnTo5VError {
    /// Why `pd req=1` failed.
    pub request: Error,
    /// Why the Hard Reset failed.
    pub hard_reset: Error,
}

/// Back to 5 V: `pd req=1` (a 5 V request needs no confirmation); if that
/// fails, `pd hrst`, which also ends at 5 V.
///
/// A session out of sync trusts no reply, so `pd hrst` is written blind
/// through [`PdmSession::send_safety`] and the result is an error with
/// [`super::Error::OutOfSync`] as `request` (and as `hard_reset` when the
/// write itself worked): VBUS is not confirmed back at 5 V.
pub fn return_to_5v<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    offers: &[Offer],
) -> Result<BackTo5V, ReturnTo5VError> {
    if let Err(out_of_sync) = session.check_in_step() {
        let hard_reset = session
            .send_safety(&Command::PdHardReset)
            .err()
            .unwrap_or_else(|| out_of_sync.clone());
        return Err(ReturnTo5VError {
            request: Error::Pdm(out_of_sync),
            hard_reset: Error::Pdm(hard_reset),
        });
    }
    let to_5v = Request::Fixed {
        position: 1,
        current_ma: None,
    };
    // No `entry pd` on the way back: the request (or the Hard Reset
    // fallback) is sent as it is.
    let sent = validate(offers, &to_5v).and_then(|command| send_request(session, command, None));
    let Err(request_error) = sent else {
        return Ok(BackTo5V::Requested);
    };
    match hard_reset(session) {
        Ok(_) => Ok(BackTo5V::HardReset { request_error }),
        Err(hard_reset) => Err(ReturnTo5VError {
            request: request_error,
            hard_reset,
        }),
    }
}

/// `type=` of `pdm set`. \[doc\] \[static\]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdType {
    /// 0: automatic.
    Auto,
    /// 1: PD 3.0 (SPR).
    Spr,
    /// 2: PD 3.1 (EPR).
    Epr,
    /// 3: proprietary PPS.
    ProprietaryPps,
}

/// `em=` (e-marker emulation) of `pdm set`. \[doc\] \[static\]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emarker {
    /// 0: off.
    Off,
    /// 1: 20 V 5 A (100 W).
    V20A5,
    /// 2: 50 V 240 W EPR.
    Epr240W,
    /// 3: LA135 6.75 A.
    La135,
    /// 4: reserved 1.
    Reserved1,
    /// 5: reserved 2.
    Reserved2,
}

/// `sink=` (Sink_Capabilities emulation) of `pdm set`. \[doc\] \[static\]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkCaps {
    /// 0: 3 A PPS.
    Pps3A,
    /// 1: 5 A PPS.
    Pps5A,
}

macro_rules! named_codes {
    ($ty:ident { $($variant:ident = $code:literal, $name:literal;)+ }) => {
        impl $ty {
            /// Every value, in code order.
            pub fn all() -> &'static [$ty] {
                &[$($ty::$variant),+]
            }
            /// The device code.
            pub fn code(self) -> u8 {
                match self {
                    $($ty::$variant => $code,)+
                }
            }
            /// The value for a device code.
            pub fn from_code(code: u8) -> Option<$ty> {
                Self::all().iter().copied().find(|v| v.code() == code)
            }
            /// The command-line name.
            pub fn name(self) -> &'static str {
                match self {
                    $($ty::$variant => $name,)+
                }
            }
            /// The value for a command-line name.
            pub fn from_name(name: &str) -> Option<$ty> {
                Self::all().iter().copied().find(|v| v.name() == name)
            }
        }
    };
}

named_codes!(PdType {
    Auto = 0, "auto";
    Spr = 1, "spr";
    Epr = 2, "epr";
    ProprietaryPps = 3, "proprietary-pps";
});
named_codes!(Emarker {
    Off = 0, "off";
    V20A5 = 1, "20v-5a";
    Epr240W = 2, "50v-240w-epr";
    La135 = 3, "la135";
    Reserved1 = 4, "reserved-1";
    Reserved2 = 5, "reserved-2";
});
named_codes!(SinkCaps {
    Pps3A = 0, "pps-3a";
    Pps5A = 1, "pps-5a";
});

/// The meter's PD emulation settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdSettings {
    /// PD type.
    pub pd_type: PdType,
    /// e-marker emulation.
    pub emarker: Emarker,
    /// Sink capabilities emulation.
    pub sink: SinkCaps,
}

impl PdSettings {
    /// Parse a `pdm parm` reply line (`type=0,em=2,sink=0`). \[device\]
    pub fn parse(line: &str) -> Result<PdSettings, Error> {
        let bad = || Error::BadSettings {
            got: line.to_string(),
        };
        let (mut ty, mut em, mut sink) = (None, None, None);
        for field in line.trim().split(',') {
            let (key, value) = field.split_once('=').ok_or_else(bad)?;
            let code: u8 = value.trim().parse().map_err(|_| bad())?;
            match key.trim() {
                "type" => ty = Some(PdType::from_code(code).ok_or_else(bad)?),
                "em" => em = Some(Emarker::from_code(code).ok_or_else(bad)?),
                "sink" => sink = Some(SinkCaps::from_code(code).ok_or_else(bad)?),
                _ => {}
            }
        }
        match (ty, em, sink) {
            (Some(pd_type), Some(emarker), Some(sink)) => Ok(PdSettings {
                pd_type,
                emarker,
                sink,
            }),
            _ => Err(bad()),
        }
    }

    /// The `pdm set` command for these settings.
    pub fn to_command(&self) -> Command {
        Command::PdmSet {
            ty: self.pd_type.code(),
            em: self.emarker.code(),
            sink: self.sink.code(),
        }
    }
}

/// `pdm parm`, parsed.
pub fn read_settings<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
) -> Result<PdSettings, Error> {
    session.send(&Command::PdmParm)?;
    PdSettings::parse(&session.wait_for(&["type="], REPLY_TIMEOUT)?)
}

/// `pdm set` with `settings`, then read them back with `pdm parm`.
/// `pdm set` has no reply. \[static\]
pub fn write_settings<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    settings: &PdSettings,
) -> Result<PdSettings, Error> {
    session.send(&settings.to_command())?;
    read_settings(session)
}

#[cfg(test)]
mod tests {
    use super::super::caps::PdoBlock;
    use super::*;

    // ---- AC-DGSQ6P.3: EPR offers arrive late [device] ----

    #[test]
    fn the_epr_wait_covers_the_device_timing() {
        // The EPR list came ~2.7 s after the first `ready:` on the 140 W
        // charger (2026-10-23); a shorter wait sends requests too early.
        assert!(EPR_SETTLE >= Duration::from_millis(2700) + Duration::from_millis(1000));
    }

    /// The 140 W charger's EPR list. \[device\]
    fn epr_caps() -> Capabilities {
        Capabilities {
            pd: Some(PdoBlock {
                marker: b'9',
                slots: vec![
                    0x0981_912C,
                    0x0002_D12C,
                    0x0004_B12C,
                    0x0006_41F4,
                    0,
                    0,
                    0,
                    0x0008_C1F4,
                    0xD230_968C,
                    0,
                    0,
                ],
            }),
            ..Capabilities::default()
        }
    }

    /// An SPR-only charger with a PPS APDO (3.3-21 V 3.25 A) at slot 5.
    fn spr_pps_caps() -> Capabilities {
        Capabilities {
            pd: Some(PdoBlock {
                marker: b'5',
                slots: vec![
                    0x0001_912C,
                    0x0002_D12C,
                    0x0004_B12C,
                    0x0006_4145,
                    0xC1A4_2141,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                ],
            }),
            ..Capabilities::default()
        }
    }

    // ---- AC-DGSQ6P.1: offers decoded and listed ----

    #[test]
    fn epr_offers_have_position_kind_range_and_current() {
        let list = offers(&epr_caps());
        let positions: Vec<u8> = list.iter().map(|o| o.position).collect();
        assert_eq!(positions, [1, 2, 3, 4, 8, 9]);
        let o8 = &list[4];
        assert_eq!(o8.kind, OfferKind::Fixed);
        assert_eq!((o8.min_voltage_mv, o8.max_voltage_mv), (28_000, 28_000));
        assert_eq!(o8.max_current_ma, Some(5000));
        assert!(o8.is_epr());
        let o9 = &list[5];
        assert_eq!(o9.kind, OfferKind::EprAvs);
        assert_eq!((o9.min_voltage_mv, o9.max_voltage_mv), (15_000, 28_000));
        assert_eq!(o9.max_power_mw, Some(140_000));
        assert!(epr_entered(&list));
        assert!(!list[0].is_epr());
        assert_eq!(OfferKind::EprAvs.name(), "epr-avs");
        assert_eq!(OfferKind::Fixed.name(), "fixed");
    }

    #[test]
    fn pps_offer_has_a_range_and_a_current() {
        let list = offers(&spr_pps_caps());
        assert_eq!(list.len(), 5);
        let pps = &list[4];
        assert_eq!(pps.position, 5);
        assert_eq!(pps.kind, OfferKind::Pps);
        assert_eq!((pps.min_voltage_mv, pps.max_voltage_mv), (3300, 21_000));
        assert_eq!(pps.max_current_ma, Some(3250));
        assert!(pps.kind.is_programmable());
        assert!(!OfferKind::Fixed.is_programmable());
        assert!(!epr_entered(&list));
    }

    // ---- AC-DGSQ6P.2: validation and command form ----

    #[test]
    fn fixed_request_renders_pd_req_by_position() {
        let list = offers(&epr_caps());
        let cmd = validate(
            &list,
            &Request::Fixed {
                position: 2,
                current_ma: None,
            },
        )
        .unwrap();
        assert_eq!(cmd.render(), b"pd req=2");
        let cmd = validate(
            &list,
            &Request::Fixed {
                position: 3,
                current_ma: Some(2000),
            },
        )
        .unwrap();
        assert_eq!(cmd.render(), b"pd req=3,cur=2000");
    }

    #[test]
    fn avs_request_renders_volt_and_cur() {
        let cmd = validate(
            &offers(&epr_caps()),
            &Request::Programmable {
                position: 9,
                voltage_mv: 15_000,
                current_ma: 1000,
            },
        )
        .unwrap();
        assert_eq!(cmd.render(), b"pd req=9,volt=15000,cur=1000");
    }

    #[test]
    fn out_of_range_requests_are_rejected() {
        let list = offers(&epr_caps());
        let r = validate(
            &list,
            &Request::Programmable {
                position: 9,
                voltage_mv: 30_000,
                current_ma: 1000,
            },
        );
        assert!(
            matches!(
                r,
                Err(Error::VoltageOutOfRange {
                    position: 9,
                    voltage_mv: 30_000,
                    min_mv: 15_000,
                    max_mv: 28_000
                })
            ),
            "{r:?}"
        );
        // 28 V × 6 A = 168 W > 140 W PDP.
        let r = validate(
            &list,
            &Request::Programmable {
                position: 9,
                voltage_mv: 28_000,
                current_ma: 6000,
            },
        );
        assert!(
            matches!(r, Err(Error::CurrentOutOfRange { position: 9, .. })),
            "{r:?}"
        );
        let r = validate(
            &list,
            &Request::Fixed {
                position: 2,
                current_ma: Some(3500),
            },
        );
        assert!(
            matches!(
                r,
                Err(Error::CurrentOutOfRange {
                    position: 2,
                    current_ma: 3500,
                    max_ma: 3000
                })
            ),
            "{r:?}"
        );
        let r = validate(
            &list,
            &Request::Fixed {
                position: 9,
                current_ma: None,
            },
        );
        assert!(
            matches!(r, Err(Error::NeedsVoltage { position: 9 })),
            "{r:?}"
        );
        let r = validate(
            &list,
            &Request::Programmable {
                position: 2,
                voltage_mv: 9000,
                current_ma: 1000,
            },
        );
        assert!(
            matches!(r, Err(Error::NotProgrammable { position: 2 })),
            "{r:?}"
        );
        let r = validate(
            &list,
            &Request::Fixed {
                position: 5,
                current_ma: None,
            },
        );
        assert!(matches!(r, Err(Error::NoOffer { position: 5 })), "{r:?}");
    }

    #[test]
    fn pps_request_is_checked_against_the_pps_range() {
        let list = offers(&spr_pps_caps());
        let cmd = validate(
            &list,
            &Request::Programmable {
                position: 5,
                voltage_mv: 9000,
                current_ma: 2000,
            },
        )
        .unwrap();
        assert_eq!(cmd.render(), b"pd req=5,volt=9000,cur=2000");
        let r = validate(
            &list,
            &Request::Programmable {
                position: 5,
                voltage_mv: 3000,
                current_ma: 1000,
            },
        );
        assert!(
            matches!(r, Err(Error::VoltageOutOfRange { position: 5, .. })),
            "{r:?}"
        );
        let r = validate(
            &list,
            &Request::Programmable {
                position: 5,
                voltage_mv: 9000,
                current_ma: 3300,
            },
        );
        assert!(
            matches!(
                r,
                Err(Error::CurrentOutOfRange {
                    position: 5,
                    max_ma: 3250,
                    ..
                })
            ),
            "{r:?}"
        );
    }

    #[test]
    fn req_and_ready_lines_parse() {
        assert_eq!(
            parse_req("req:9000mV,3000mA"),
            Some(Reading {
                voltage_mv: 9000,
                current_ma: 3000
            })
        );
        assert_eq!(
            parse_ready("ready:8554mV,0mA"),
            Some(Reading {
                voltage_mv: 8554,
                current_ma: 0
            })
        );
        assert_eq!(parse_req("ready:8554mV,0mA"), None);
        assert_eq!(parse_ready("ready:garbage"), None);
    }

    // ---- AC-DGSQ6P.3: EPR positions need EPR mode ----

    #[test]
    fn epr_position_without_epr_mode_reports_epr_unavailable() {
        // [inferred]: a charger or cable that cannot enter EPR leaves only
        // the SPR `pdo:` block, so positions 8+ are absent.
        let list = offers(&spr_pps_caps());
        let r = validate(
            &list,
            &Request::Fixed {
                position: 8,
                current_ma: None,
            },
        );
        assert!(
            matches!(r, Err(Error::EprUnavailable { position: 8 })),
            "{r:?}"
        );
        // In EPR mode an empty EPR slot is just "no offer".
        let r = validate(
            &offers(&epr_caps()),
            &Request::Fixed {
                position: 10,
                current_ma: None,
            },
        );
        assert!(matches!(r, Err(Error::NoOffer { position: 10 })), "{r:?}");
        let cmd = validate(
            &offers(&epr_caps()),
            &Request::Fixed {
                position: 8,
                current_ma: None,
            },
        )
        .unwrap();
        assert_eq!(cmd.render(), b"pd req=8");
    }

    // ---- AC-DGSQ6P.4: settings with named values ----

    #[test]
    fn settings_parse_and_render_with_names() {
        let s = PdSettings::parse("type=0,em=2,sink=0").unwrap();
        assert_eq!(
            s,
            PdSettings {
                pd_type: PdType::Auto,
                emarker: Emarker::Epr240W,
                sink: SinkCaps::Pps3A,
            }
        );
        let s = PdSettings {
            pd_type: PdType::Spr,
            emarker: Emarker::La135,
            sink: SinkCaps::Pps5A,
        };
        assert_eq!(s.to_command().render(), b"pdm set type=1,em=3,sink=1");
        assert!(matches!(
            PdSettings::parse("bogus"),
            Err(Error::BadSettings { .. })
        ));
        assert!(matches!(
            PdSettings::parse("type=9,em=2,sink=0"),
            Err(Error::BadSettings { .. })
        ));
    }

    #[test]
    fn every_named_value_round_trips() {
        let names: Vec<&str> = PdType::all().iter().map(|t| t.name()).collect();
        assert_eq!(names, ["auto", "spr", "epr", "proprietary-pps"]);
        let names: Vec<&str> = Emarker::all().iter().map(|t| t.name()).collect();
        assert_eq!(
            names,
            [
                "off",
                "20v-5a",
                "50v-240w-epr",
                "la135",
                "reserved-1",
                "reserved-2"
            ]
        );
        let names: Vec<&str> = SinkCaps::all().iter().map(|t| t.name()).collect();
        assert_eq!(names, ["pps-3a", "pps-5a"]);
        for (i, t) in PdType::all().iter().enumerate() {
            assert_eq!(usize::from(t.code()), i);
            assert_eq!(PdType::from_code(t.code()), Some(*t));
            assert_eq!(PdType::from_name(t.name()), Some(*t));
        }
        for (i, t) in Emarker::all().iter().enumerate() {
            assert_eq!(usize::from(t.code()), i);
            assert_eq!(Emarker::from_code(t.code()), Some(*t));
            assert_eq!(Emarker::from_name(t.name()), Some(*t));
        }
        for (i, t) in SinkCaps::all().iter().enumerate() {
            assert_eq!(usize::from(t.code()), i);
            assert_eq!(SinkCaps::from_code(t.code()), Some(*t));
            assert_eq!(SinkCaps::from_name(t.name()), Some(*t));
        }
        assert_eq!(PdType::from_code(4), None);
        assert_eq!(Emarker::from_name("bogus"), None);
    }
}
