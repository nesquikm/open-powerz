//! Legacy and vendor fast-charge triggering on top of the PDM session
//! (FR DGSQ6Q): Quick Charge 2.0 / 3.0, Huawei FCP / SCP, Samsung AFC,
//! vivo VFCP, Spreadtrum SFCP, Transsion TFCP and UFCS.
//!
//! - [`LegacyProtocol`]: the protocols, each entered with `entry <name>`
//!   (QC 2.0 and 3.0 share `entry qc`). \[doc\] \[static\]
//! - [`limits`]: the decoded limits of the last capability block
//!   (`scp:` / `vfcp:` / `tfcp:` / `ufcs:`) for a protocol. \[static\]
//! - [`validate`]: check a [`Request`] (fixed voltages per protocol, the
//!   QC 3.0 range and 200 mV step, the block limits) and render its
//!   command; pure, nothing is sent.
//! - [`enter`], [`request`], [`return_to_5v`]: the session flows. An entry
//!   answered with `timeout` is [`Error::NotSupported`] \[device\]
//!   (session 2); so is a protocol that reports capabilities but sent no
//!   block (`entry vfcp` → `ok\nready\n` only on a charger without VFCP).
//!   \[device\] Reply forms of a supported protocol (`ready`, the blocks)
//!   are \[static\] / \[doc\] only: no charger speaking them has been seen.

use std::io::{Read, Write};
use std::time::Duration;

use super::BlockTag;
use super::caps::Capabilities;
use super::command::{Command, FixedProtocol, Protocol};
use super::reply::ReplyItem;
use super::risk::Risk;
use super::session::{HighVoltageConfirmation, PdmSession, REPLY_TIMEOUT};
use crate::session::Clock;

/// How long an `entry` may take to answer `ready` / `timeout`. \[inferred\]
pub const ENTRY_TIMEOUT: Duration = Duration::from_millis(3000);

/// Lowest QC 3.0 voltage, mV. \[doc\]
pub const QC3_MIN_MV: u32 = 3600;
/// Highest QC 3.0 voltage, mV. \[doc\]
pub const QC3_MAX_MV: u32 = 20_000;
/// QC 3.0 voltage step, mV. \[doc\]
pub const QC3_STEP_MV: u32 = 200;

/// The non-PD protocols this module triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyProtocol {
    /// Quick Charge 2.0 / 3.0 (`entry qc`).
    Qc,
    /// Huawei FCP.
    Fcp,
    /// Samsung AFC.
    Afc,
    /// Spreadtrum SFCP.
    Sfcp,
    /// Huawei SCP.
    Scp,
    /// vivo VFCP.
    Vfcp,
    /// Transsion TFCP.
    Tfcp,
    /// UFCS.
    Ufcs,
}

impl LegacyProtocol {
    /// Every protocol, in the order above.
    pub const ALL: [LegacyProtocol; 8] = [
        LegacyProtocol::Qc,
        LegacyProtocol::Fcp,
        LegacyProtocol::Afc,
        LegacyProtocol::Sfcp,
        LegacyProtocol::Scp,
        LegacyProtocol::Vfcp,
        LegacyProtocol::Tfcp,
        LegacyProtocol::Ufcs,
    ];

    /// Machine name, as in `entry <name>`: `qc`, `fcp`, ...
    pub fn name(self) -> &'static str {
        match self {
            LegacyProtocol::Qc => "qc",
            LegacyProtocol::Fcp => "fcp",
            LegacyProtocol::Afc => "afc",
            LegacyProtocol::Sfcp => "sfcp",
            LegacyProtocol::Scp => "scp",
            LegacyProtocol::Vfcp => "vfcp",
            LegacyProtocol::Tfcp => "tfcp",
            LegacyProtocol::Ufcs => "ufcs",
        }
    }

    /// The `entry` protocol.
    pub fn entry(self) -> Protocol {
        match self {
            LegacyProtocol::Qc => Protocol::Qc,
            LegacyProtocol::Fcp => Protocol::Fcp,
            LegacyProtocol::Afc => Protocol::Afc,
            LegacyProtocol::Sfcp => Protocol::Sfcp,
            LegacyProtocol::Scp => Protocol::Scp,
            LegacyProtocol::Vfcp => Protocol::Vfcp,
            LegacyProtocol::Tfcp => Protocol::Tfcp,
            LegacyProtocol::Ufcs => Protocol::Ufcs,
        }
    }

    /// The capability block the protocol reports after `ready`, if any.
    fn block_tag(self) -> Option<BlockTag> {
        match self {
            LegacyProtocol::Scp => Some(BlockTag::Scp),
            LegacyProtocol::Vfcp => Some(BlockTag::Vfcp),
            LegacyProtocol::Tfcp => Some(BlockTag::Tfcp),
            LegacyProtocol::Ufcs => Some(BlockTag::Ufcs),
            LegacyProtocol::Qc
            | LegacyProtocol::Fcp
            | LegacyProtocol::Afc
            | LegacyProtocol::Sfcp => None,
        }
    }
}

/// Decoded limits of one capability (one UFCS entry, or the whole block of
/// SCP / VFCP / TFCP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// UFCS capability number (1-based); `None` for the other protocols.
    pub index: Option<u8>,
    /// Lowest voltage, mV.
    pub min_voltage_mv: u32,
    /// Highest voltage, mV.
    pub max_voltage_mv: u32,
    /// Lowest current, mA.
    pub min_current_ma: u32,
    /// Highest current, mA.
    pub max_current_ma: u32,
    /// Maximum power, mW, when the block reports it.
    pub max_power_mw: Option<u32>,
}

/// One request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// `qc|fcp|afc|sfcp <n>V`: QC 2.0 allows 5, 9, 12 and 20 V; FCP, AFC
    /// and SFCP 5, 9 and 12 V. \[doc\]
    Fixed {
        /// Protocol.
        protocol: FixedProtocol,
        /// Volts.
        volts: u8,
    },
    /// `qc3 volt=<mV>`: 3600-20000 mV in 200 mV steps. \[doc\]
    Qc3Volt {
        /// Voltage, mV.
        voltage_mv: u32,
    },
    /// `qc3 inc=<n>`: up by `n` steps (always treated as above 5 V).
    Qc3Inc(u32),
    /// `qc3 dec=<n>`: down by `n` steps.
    Qc3Dec(u32),
    /// `scp volt=<mV>,cur=<mA>` inside the `scp:` limits.
    Scp {
        /// Voltage, mV.
        voltage_mv: u32,
        /// Current, mA.
        current_ma: u32,
    },
    /// `vfcp volt=<mV>,cur=<mA>` inside the `vfcp:` limits.
    Vfcp {
        /// Voltage, mV.
        voltage_mv: u32,
        /// Current, mA.
        current_ma: u32,
    },
    /// `tfcp volt=<mV>,cur=<mA>` inside the `tfcp:` limits.
    Tfcp {
        /// Voltage, mV.
        voltage_mv: u32,
        /// Current, mA.
        current_ma: u32,
    },
    /// `ufcs req=<n>,volt=<mV>,cur=<mA>` inside capability `n` (1-based).
    Ufcs {
        /// Capability number.
        index: u8,
        /// Voltage, mV.
        voltage_mv: u32,
        /// Current, mA.
        current_ma: u32,
    },
    /// `ufcs cmd=<n>`: a UFCS control message by its number. \[doc\]
    UfcsCmd(u8),
}

/// A protocol after a successful `entry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entered {
    /// The protocol.
    pub protocol: LegacyProtocol,
    /// Decoded limits from its capability block (empty for protocols that
    /// report none: QC, FCP, AFC, SFCP).
    pub limits: Vec<Limits>,
}

/// The result of one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The command that was sent.
    pub command: Command,
    /// The reply line (`ready...`), if one came. \[static\]
    pub reply: Option<String>,
}

/// Errors of the legacy trigger layer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A PDM session error.
    #[error(transparent)]
    Pdm(#[from] super::Error),
    /// The charger did not answer the protocol (the meter replied `timeout`,
    /// or sent no capability block where one is expected).
    #[error("the charger does not support {protocol:?}")]
    NotSupported {
        /// The protocol.
        protocol: LegacyProtocol,
    },
    /// Not one of the protocol's fixed voltages.
    #[error("{protocol:?} cannot request {volts} V")]
    FixedVoltage {
        /// Protocol.
        protocol: FixedProtocol,
        /// Requested volts.
        volts: u8,
    },
    /// A QC 3.0 voltage outside 3600-20000 mV or off the 200 mV grid.
    #[error("QC 3.0 cannot request {voltage_mv} mV (3600-20000 mV in 200 mV steps)")]
    Qc3Voltage {
        /// Requested voltage, mV.
        voltage_mv: u32,
    },
    /// A QC 3.0 step count that is not usable.
    #[error("QC 3.0 cannot step by {count}")]
    StepCount {
        /// Requested count.
        count: u32,
    },
    /// No capability block is known for the protocol, so no request can be
    /// checked.
    #[error("no {protocol:?} capabilities are known")]
    NoCapabilities {
        /// The protocol.
        protocol: LegacyProtocol,
    },
    /// Voltage × current is above the charger's maximum power.
    #[error("{voltage_mv} mV × {current_ma} mA is above the charger's {max_power_mw} mW")]
    PowerOutOfRange {
        /// Requested voltage, mV.
        voltage_mv: u32,
        /// Requested current, mA.
        current_ma: u32,
        /// The block's maximum power, mW.
        max_power_mw: u32,
    },
    /// No UFCS capability with that number.
    #[error("the charger has no UFCS capability {index}")]
    NoCapability {
        /// Requested capability number.
        index: u8,
    },
    /// The voltage is outside the charger's limits.
    #[error("{voltage_mv} mV is outside {min_mv}-{max_mv} mV")]
    VoltageOutOfRange {
        /// Requested voltage, mV.
        voltage_mv: u32,
        /// Lowest allowed, mV.
        min_mv: u32,
        /// Highest allowed, mV.
        max_mv: u32,
    },
    /// The current is outside the charger's limits.
    #[error("{current_ma} mA is outside {min_ma}-{max_ma} mA")]
    CurrentOutOfRange {
        /// Requested current, mA.
        current_ma: u32,
        /// Lowest allowed, mA.
        min_ma: u32,
        /// Highest allowed, mA.
        max_ma: u32,
    },
}

/// The decoded limits of the last block for `protocol` in `caps`; empty
/// when there is none (or the protocol reports none).
pub fn limits(caps: &Capabilities, protocol: LegacyProtocol) -> Vec<Limits> {
    let one = match protocol {
        LegacyProtocol::Qc | LegacyProtocol::Fcp | LegacyProtocol::Afc | LegacyProtocol::Sfcp => {
            None
        }
        LegacyProtocol::Scp => caps.scp.map(|b| Limits {
            index: None,
            min_voltage_mv: b.min_voltage_mv,
            max_voltage_mv: b.max_voltage_mv,
            min_current_ma: b.min_current_ma,
            max_current_ma: b.max_current_ma,
            max_power_mw: Some(b.max_power_mw),
        }),
        // The block reports two fixed modes (NUM1 / NUM2), no range: the
        // limits span both modes, from no current floor up to the larger
        // current. \[inferred\] from the \[static\] NUM1 / NUM2 fields.
        LegacyProtocol::Vfcp => caps.vfcp.as_ref().map(|b| Limits {
            index: None,
            min_voltage_mv: b.num1_voltage_mv.min(b.num2_voltage_mv),
            max_voltage_mv: b.num1_voltage_mv.max(b.num2_voltage_mv),
            min_current_ma: 0,
            max_current_ma: b.num1_current_ma.max(b.num2_current_ma),
            max_power_mw: Some(b.max_power_mw),
        }),
        LegacyProtocol::Tfcp => caps.tfcp.map(|b| Limits {
            index: None,
            min_voltage_mv: b.min_voltage_mv,
            max_voltage_mv: b.max_voltage_mv,
            min_current_ma: b.min_current_ma,
            max_current_ma: b.max_current_ma,
            max_power_mw: Some(b.max_power_mw),
        }),
        // One entry per capability, numbered by its own index field (bits
        // 63..60, shown by the vendor app as `NUM<n>`); the block reports no
        // power limit. \[static\]
        LegacyProtocol::Ufcs => {
            return caps.ufcs.as_ref().map_or_else(Vec::new, |b| {
                b.entries
                    .iter()
                    .map(|c| Limits {
                        index: Some(c.index),
                        min_voltage_mv: c.min_voltage_mv,
                        max_voltage_mv: c.max_voltage_mv,
                        min_current_ma: c.min_current_ma,
                        max_current_ma: c.max_current_ma,
                        max_power_mw: None,
                    })
                    .collect()
            });
        }
    };
    one.into_iter().collect()
}

/// Check `voltage_mv` / `current_ma` against `limits`.
fn check(limits: &Limits, voltage_mv: u32, current_ma: u32) -> Result<(), Error> {
    if !(limits.min_voltage_mv..=limits.max_voltage_mv).contains(&voltage_mv) {
        return Err(Error::VoltageOutOfRange {
            voltage_mv,
            min_mv: limits.min_voltage_mv,
            max_mv: limits.max_voltage_mv,
        });
    }
    if !(limits.min_current_ma..=limits.max_current_ma).contains(&current_ma) {
        return Err(Error::CurrentOutOfRange {
            current_ma,
            min_ma: limits.min_current_ma,
            max_ma: limits.max_current_ma,
        });
    }
    // Both ranges can hold while their product is above the block's power
    // (and VFCP's two modes are merged into one range). [static] [inferred]
    if let Some(max_power_mw) = limits.max_power_mw {
        // mV x mA is µW; compare without dividing, so no rounding slack.
        let power_uw = u64::from(voltage_mv) * u64::from(current_ma);
        if power_uw > u64::from(max_power_mw) * 1000 {
            return Err(Error::PowerOutOfRange {
                voltage_mv,
                current_ma,
                max_power_mw,
            });
        }
    }
    Ok(())
}

/// Check a volt/cur request of a single-block protocol and render it.
fn checked(
    caps: &Capabilities,
    protocol: LegacyProtocol,
    voltage_mv: u32,
    current_ma: u32,
    command: Command,
) -> Result<Command, Error> {
    let limits = limits(caps, protocol);
    let first = limits.first().ok_or(Error::NoCapabilities { protocol })?;
    check(first, voltage_mv, current_ma)?;
    Ok(command)
}

/// Check `request` against `caps` and render its command; pure.
pub fn validate(caps: &Capabilities, request: &Request) -> Result<Command, Error> {
    match *request {
        Request::Fixed { protocol, volts } => {
            let allowed: &[u8] = match protocol {
                FixedProtocol::Qc => &[5, 9, 12, 20],
                FixedProtocol::Fcp | FixedProtocol::Afc | FixedProtocol::Sfcp => &[5, 9, 12],
            };
            if allowed.contains(&volts) {
                Ok(Command::FixedVoltage { protocol, volts })
            } else {
                Err(Error::FixedVoltage { protocol, volts })
            }
        }
        Request::Qc3Volt { voltage_mv } => {
            if (QC3_MIN_MV..=QC3_MAX_MV).contains(&voltage_mv)
                && (voltage_mv - QC3_MIN_MV).is_multiple_of(QC3_STEP_MV)
            {
                Ok(Command::Qc3Volt { voltage_mv })
            } else {
                Err(Error::Qc3Voltage { voltage_mv })
            }
        }
        // Zero changes nothing; more than the whole 3.6-20 V span cannot be
        // meant. Both are refused rather than sent.
        Request::Qc3Inc(count) | Request::Qc3Dec(count)
            if count == 0 || count > (QC3_MAX_MV - QC3_MIN_MV) / QC3_STEP_MV =>
        {
            Err(Error::StepCount { count })
        }
        Request::Qc3Inc(count) => Ok(Command::Qc3Inc(count)),
        Request::Qc3Dec(count) => Ok(Command::Qc3Dec(count)),
        Request::Scp {
            voltage_mv,
            current_ma,
        } => checked(
            caps,
            LegacyProtocol::Scp,
            voltage_mv,
            current_ma,
            Command::ScpRequest {
                voltage_mv,
                current_ma,
            },
        ),
        Request::Vfcp {
            voltage_mv,
            current_ma,
        } => checked(
            caps,
            LegacyProtocol::Vfcp,
            voltage_mv,
            current_ma,
            Command::VfcpRequest {
                voltage_mv,
                current_ma,
            },
        ),
        Request::Tfcp {
            voltage_mv,
            current_ma,
        } => checked(
            caps,
            LegacyProtocol::Tfcp,
            voltage_mv,
            current_ma,
            Command::TfcpRequest {
                voltage_mv,
                current_ma,
            },
        ),
        Request::Ufcs { .. } if caps.ufcs.is_none() => Err(Error::NoCapabilities {
            protocol: LegacyProtocol::Ufcs,
        }),
        Request::Ufcs {
            index,
            voltage_mv,
            current_ma,
        } => {
            let all = limits(caps, LegacyProtocol::Ufcs);
            let cap = all
                .iter()
                .find(|l| l.index == Some(index))
                .ok_or(Error::NoCapability { index })?;
            check(cap, voltage_mv, current_ma)?;
            Ok(Command::UfcsRequest {
                index,
                voltage_mv,
                current_ma,
            })
        }
        // A control message by its number (UFCS spec table); the session's
        // risk gate judges it. \[doc\]
        Request::UfcsCmd(n) => Ok(Command::UfcsCmd(n)),
    }
}

/// `entry <protocol>`: `ok`, then `ready` (and the capability block for
/// SCP / VFCP / TFCP / UFCS), or [`Error::NotSupported`] on `timeout`.
pub fn enter<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    protocol: LegacyProtocol,
) -> Result<Entered, Error> {
    session.send(&Command::Entry(protocol.entry()))?;
    match session.wait_for(&["ready", "timeout"], ENTRY_TIMEOUT) {
        Ok(line) if line.starts_with("ready") => {}
        Ok(_) | Err(super::Error::Timeout { .. }) => {
            return Err(Error::NotSupported { protocol });
        }
        Err(e) => return Err(e.into()),
    }
    if let Some(tag) = protocol.block_tag() {
        wait_for_block(session, protocol, tag)?;
    }
    Ok(Entered {
        protocol,
        limits: limits(session.capabilities(), protocol),
    })
}

/// Most reply items read while waiting for the capability block after
/// `ready`; bounds a device that keeps sending other lines. \[inferred\]
const MAX_ITEMS_BEFORE_BLOCK: usize = 32;

/// After `ready`: read until the protocol's capability block arrives (the
/// session records it), or [`Error::NotSupported`] when none comes.
/// \[static\] (block after `ready`); \[device\] (`ready` with no block on a
/// charger without the protocol).
fn wait_for_block<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    protocol: LegacyProtocol,
    tag: BlockTag,
) -> Result<(), Error> {
    for _ in 0..MAX_ITEMS_BEFORE_BLOCK {
        match session.next_item(ENTRY_TIMEOUT) {
            Ok(ReplyItem::Block(block)) if block.tag() == tag => return Ok(()),
            Ok(_) => {}
            Err(super::Error::Timeout { .. }) => break,
            Err(e) => return Err(e.into()),
        }
    }
    Err(Error::NotSupported { protocol })
}

/// Validate, send (with `confirmation` when above 5 V) and read the reply.
pub fn request<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    request: &Request,
    confirmation: Option<HighVoltageConfirmation>,
) -> Result<Outcome, Error> {
    // Validation first: an invalid request writes nothing.
    let command = validate(session.capabilities(), request)?;
    match confirmation {
        Some(confirmation) if session.risk(&command) == Risk::Above5V => {
            session.send_confirmed(&command, confirmation)?;
        }
        // At or below 5 V needs no confirmation; above 5 V without one is
        // refused by `send` before anything is written.
        _ => session.send(&command)?,
    }
    // The `ready` reply form is \[static\]; a missing reply is not an error.
    let reply = match session.wait_for(&["ready"], REPLY_TIMEOUT) {
        Ok(line) => Some(line),
        Err(super::Error::Timeout { .. }) => None,
        Err(e) => return Err(e.into()),
    };
    Ok(Outcome { command, reply })
}

/// `reset` and wait for `ok`: the meter leaves the protocol and VBUS goes
/// back to 5 V (\[device\] after PD; \[inferred\] for these protocols).
///
/// A session out of sync trusts no reply: `reset` is written blind through
/// [`PdmSession::send_safety`] and the latched [`super::Error::OutOfSync`]
/// is returned, since VBUS is not confirmed back at 5 V.
pub fn return_to_5v<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
) -> Result<(), Error> {
    if let Err(out_of_sync) = session.check_in_step() {
        session.send_safety(&Command::Reset)?;
        return Err(out_of_sync.into());
    }
    session.send(&Command::Reset)?;
    session.wait_for(&["ok"], REPLY_TIMEOUT)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::caps::fixtures::{scp_block, tfcp_block, ufcs_block, vfcp_block};
    use super::super::caps::parse;
    use super::*;

    fn caps_with(blocks: &[Vec<u8>]) -> Capabilities {
        let mut caps = Capabilities::default();
        for block in blocks {
            let (b, _) = parse(block).unwrap().unwrap();
            caps.update(&b);
        }
        caps
    }

    fn all_caps() -> Capabilities {
        // The vfcp: fixture has no line end; the parser needs one.
        let mut vfcp = vfcp_block();
        vfcp.push(b'\n');
        caps_with(&[scp_block(), tfcp_block(), ufcs_block(), vfcp])
    }

    fn text(c: &Command) -> String {
        String::from_utf8(c.render()).unwrap()
    }

    fn fixed(protocol: FixedProtocol, volts: u8) -> Request {
        Request::Fixed { protocol, volts }
    }

    // ---- AC-DGSQ6Q.1: QC 2.0 fixed voltages, QC 3.0 range, step and counts ----

    #[test]
    fn qc2_requests_5_9_12_and_20_v() {
        let caps = Capabilities::default();
        for v in [5u8, 9, 12, 20] {
            let c = validate(&caps, &fixed(FixedProtocol::Qc, v)).unwrap();
            assert_eq!(text(&c), format!("qc {v}V"));
        }
        for v in [0u8, 3, 7, 15, 21] {
            assert!(
                matches!(
                    validate(&caps, &fixed(FixedProtocol::Qc, v)),
                    Err(Error::FixedVoltage { volts, .. }) if volts == v
                ),
                "{v} V"
            );
        }
    }

    #[test]
    fn qc3_voltage_is_3600_to_20000_mv_in_200_mv_steps() {
        let caps = Capabilities::default();
        for mv in [3600u32, 3800, 5000, 6000, 9000, 12_000, 19_800, 20_000] {
            let c = validate(&caps, &Request::Qc3Volt { voltage_mv: mv }).unwrap();
            assert_eq!(text(&c), format!("qc3 volt={mv}"));
        }
        for mv in [0u32, 3400, 3500, 3700, 6100, 9050, 20_200, 21_000] {
            assert!(
                matches!(
                    validate(&caps, &Request::Qc3Volt { voltage_mv: mv }),
                    Err(Error::Qc3Voltage { voltage_mv }) if voltage_mv == mv
                ),
                "{mv} mV"
            );
        }
    }

    #[test]
    fn qc3_step_counts_beyond_the_whole_span_are_refused() {
        let caps = Capabilities::default();
        let span = (QC3_MAX_MV - QC3_MIN_MV) / QC3_STEP_MV;
        assert!(validate(&caps, &Request::Qc3Inc(span)).is_ok());
        assert!(matches!(
            validate(&caps, &Request::Qc3Inc(span + 1)),
            Err(Error::StepCount { .. })
        ));
        assert!(matches!(
            validate(&caps, &Request::Qc3Dec(u32::MAX)),
            Err(Error::StepCount { .. })
        ));
    }

    #[test]
    fn power_limit_has_no_rounding_slack() {
        let caps = all_caps();
        // SCP 40 W: 10.053 V x 3.979 A = 40.000887 W, under 1 mW over.
        assert!(matches!(
            validate(
                &caps,
                &Request::Scp {
                    voltage_mv: 10_053,
                    current_ma: 3979
                }
            ),
            Err(Error::PowerOutOfRange { .. })
        ));
    }

    #[test]
    fn qc3_steps_up_and_down_by_a_count() {
        let caps = Capabilities::default();
        assert_eq!(
            text(&validate(&caps, &Request::Qc3Inc(2)).unwrap()),
            "qc3 inc=2"
        );
        assert_eq!(
            text(&validate(&caps, &Request::Qc3Dec(3)).unwrap()),
            "qc3 dec=3"
        );
        // Zero steps changes nothing: refused rather than sent.
        assert!(matches!(
            validate(&caps, &Request::Qc3Inc(0)),
            Err(Error::StepCount { count: 0 })
        ));
        assert!(matches!(
            validate(&caps, &Request::Qc3Dec(0)),
            Err(Error::StepCount { count: 0 })
        ));
    }

    // ---- AC-DGSQ6Q.2: FCP, AFC and SFCP request 5, 9 and 12 V ----

    #[test]
    fn fcp_afc_sfcp_request_5_9_and_12_v_only() {
        let caps = Capabilities::default();
        for (p, name) in [
            (FixedProtocol::Fcp, "fcp"),
            (FixedProtocol::Afc, "afc"),
            (FixedProtocol::Sfcp, "sfcp"),
        ] {
            for v in [5u8, 9, 12] {
                let c = validate(&caps, &fixed(p, v)).unwrap();
                assert_eq!(text(&c), format!("{name} {v}V"));
            }
            for v in [20u8, 15, 7, 0] {
                assert!(
                    matches!(
                        validate(&caps, &fixed(p, v)),
                        Err(Error::FixedVoltage { protocol, volts }) if protocol == p && volts == v
                    ),
                    "{name} {v} V"
                );
            }
        }
    }

    // ---- AC-DGSQ6Q.3: SCP / VFCP / TFCP limits decoded and enforced ----

    #[test]
    fn scp_limits_are_decoded_from_the_block() {
        let l = limits(&all_caps(), LegacyProtocol::Scp);
        assert_eq!(
            l,
            [Limits {
                index: None,
                min_voltage_mv: 3400,
                max_voltage_mv: 20_000,
                min_current_ma: 100,
                max_current_ma: 4000,
                max_power_mw: Some(40_000),
            }]
        );
    }

    #[test]
    fn tfcp_limits_are_decoded_from_the_block() {
        let l = limits(&all_caps(), LegacyProtocol::Tfcp);
        assert_eq!(
            l,
            [Limits {
                index: None,
                min_voltage_mv: 5000,
                max_voltage_mv: 20_000,
                min_current_ma: 100,
                max_current_ma: 3000,
                max_power_mw: Some(60_000),
            }]
        );
    }

    #[test]
    fn vfcp_limits_cover_its_two_modes() {
        let l = limits(&all_caps(), LegacyProtocol::Vfcp);
        assert_eq!(l.len(), 1, "{l:?}");
        // Modes 9 V 2 A and 12 V 1.5 A, 18 W.
        assert_eq!(l[0].max_voltage_mv, 12_000);
        assert_eq!(l[0].max_current_ma, 2000);
        assert_eq!(l[0].max_power_mw, Some(18_000));
        assert!(l[0].min_voltage_mv <= 9000);
    }

    #[test]
    fn protocols_without_blocks_have_no_limits() {
        let caps = all_caps();
        for p in [
            LegacyProtocol::Qc,
            LegacyProtocol::Fcp,
            LegacyProtocol::Afc,
            LegacyProtocol::Sfcp,
        ] {
            assert!(limits(&caps, p).is_empty(), "{p:?}");
        }
        for p in [
            LegacyProtocol::Scp,
            LegacyProtocol::Vfcp,
            LegacyProtocol::Tfcp,
            LegacyProtocol::Ufcs,
        ] {
            assert!(limits(&Capabilities::default(), p).is_empty(), "{p:?}");
        }
    }

    #[test]
    fn scp_request_inside_the_limits_renders() {
        let c = validate(
            &all_caps(),
            &Request::Scp {
                voltage_mv: 9000,
                current_ma: 2000,
            },
        )
        .unwrap();
        assert_eq!(text(&c), "scp volt=9000,cur=2000");
    }

    #[test]
    fn requests_above_the_blocks_max_power_are_refused() {
        let caps = all_caps();
        // SCP 40 W: 20 V x 4 A = 80 W is inside both ranges but not the power.
        assert!(matches!(
            validate(
                &caps,
                &Request::Scp {
                    voltage_mv: 20_000,
                    current_ma: 4000
                }
            ),
            Err(Error::PowerOutOfRange { .. })
        ));
        // VFCP 18 W: 12 V x 2 A = 24 W.
        assert!(matches!(
            validate(
                &caps,
                &Request::Vfcp {
                    voltage_mv: 12_000,
                    current_ma: 2000
                }
            ),
            Err(Error::PowerOutOfRange { .. })
        ));
        assert!(
            validate(
                &caps,
                &Request::Vfcp {
                    voltage_mv: 12_000,
                    current_ma: 1500
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn scp_request_outside_the_limits_is_refused() {
        let caps = all_caps();
        let scp = |voltage_mv, current_ma| Request::Scp {
            voltage_mv,
            current_ma,
        };
        assert!(matches!(
            validate(&caps, &scp(21_000, 1000)),
            Err(Error::VoltageOutOfRange {
                voltage_mv: 21_000,
                min_mv: 3400,
                max_mv: 20_000
            })
        ));
        assert!(matches!(
            validate(&caps, &scp(3000, 1000)),
            Err(Error::VoltageOutOfRange {
                voltage_mv: 3000,
                ..
            })
        ));
        assert!(matches!(
            validate(&caps, &scp(9000, 5000)),
            Err(Error::CurrentOutOfRange {
                current_ma: 5000,
                max_ma: 4000,
                ..
            })
        ));
    }

    #[test]
    fn tfcp_and_vfcp_requests_are_checked_against_their_blocks() {
        let caps = all_caps();
        let t = validate(
            &caps,
            &Request::Tfcp {
                voltage_mv: 9000,
                current_ma: 2000,
            },
        )
        .unwrap();
        assert_eq!(text(&t), "tfcp volt=9000,cur=2000");
        assert!(matches!(
            validate(
                &caps,
                &Request::Tfcp {
                    voltage_mv: 4000,
                    current_ma: 1000
                }
            ),
            Err(Error::VoltageOutOfRange {
                voltage_mv: 4000,
                ..
            })
        ));
        assert!(matches!(
            validate(
                &caps,
                &Request::Tfcp {
                    voltage_mv: 9000,
                    current_ma: 3500
                }
            ),
            Err(Error::CurrentOutOfRange {
                current_ma: 3500,
                ..
            })
        ));
        let v = validate(
            &caps,
            &Request::Vfcp {
                voltage_mv: 9000,
                current_ma: 1000,
            },
        )
        .unwrap();
        assert_eq!(text(&v), "vfcp volt=9000,cur=1000");
        assert!(matches!(
            validate(
                &caps,
                &Request::Vfcp {
                    voltage_mv: 20_000,
                    current_ma: 1000
                }
            ),
            Err(Error::VoltageOutOfRange {
                voltage_mv: 20_000,
                ..
            })
        ));
        assert!(matches!(
            validate(
                &caps,
                &Request::Vfcp {
                    voltage_mv: 9000,
                    current_ma: 9000
                }
            ),
            Err(Error::CurrentOutOfRange {
                current_ma: 9000,
                ..
            })
        ));
    }

    #[test]
    fn volt_cur_requests_without_a_block_are_refused() {
        let caps = Capabilities::default();
        let cases = [
            (
                Request::Scp {
                    voltage_mv: 5000,
                    current_ma: 1000,
                },
                LegacyProtocol::Scp,
            ),
            (
                Request::Vfcp {
                    voltage_mv: 5000,
                    current_ma: 1000,
                },
                LegacyProtocol::Vfcp,
            ),
            (
                Request::Tfcp {
                    voltage_mv: 5000,
                    current_ma: 1000,
                },
                LegacyProtocol::Tfcp,
            ),
            (
                Request::Ufcs {
                    index: 1,
                    voltage_mv: 5000,
                    current_ma: 1000,
                },
                LegacyProtocol::Ufcs,
            ),
        ];
        for (req, p) in cases {
            assert!(
                matches!(validate(&caps, &req), Err(Error::NoCapabilities { protocol }) if protocol == p),
                "{req:?}"
            );
        }
    }

    // ---- AC-DGSQ6Q.4: UFCS capabilities by number, control commands ----

    #[test]
    fn ufcs_limits_list_each_capability_by_number() {
        let l = limits(&all_caps(), LegacyProtocol::Ufcs);
        assert_eq!(
            l,
            [Limits {
                index: Some(1),
                min_voltage_mv: 3400,
                max_voltage_mv: 20_000,
                min_current_ma: 100,
                max_current_ma: 5000,
                max_power_mw: None,
            }]
        );
    }

    #[test]
    fn ufcs_request_is_checked_against_its_capability() {
        let caps = all_caps();
        let ufcs = |index, voltage_mv, current_ma| Request::Ufcs {
            index,
            voltage_mv,
            current_ma,
        };
        let c = validate(&caps, &ufcs(1, 9000, 2000)).unwrap();
        assert_eq!(text(&c), "ufcs req=1,volt=9000,cur=2000");
        assert!(matches!(
            validate(&caps, &ufcs(2, 9000, 2000)),
            Err(Error::NoCapability { index: 2 })
        ));
        assert!(matches!(
            validate(&caps, &ufcs(1, 21_000, 2000)),
            Err(Error::VoltageOutOfRange {
                voltage_mv: 21_000,
                ..
            })
        ));
        assert!(matches!(
            validate(&caps, &ufcs(1, 9000, 6000)),
            Err(Error::CurrentOutOfRange {
                current_ma: 6000,
                ..
            })
        ));
    }

    #[test]
    fn ufcs_control_commands_render() {
        let caps = Capabilities::default();
        let c = validate(&caps, &Request::UfcsCmd(6)).unwrap();
        assert_eq!(text(&c), "ufcs cmd=6");
    }

    // ---- AC-DGSQ6Q.5: protocols and their entry commands ----

    #[test]
    fn every_protocol_enters_by_its_name() {
        let want = [
            (LegacyProtocol::Qc, "qc", "entry qc"),
            (LegacyProtocol::Fcp, "fcp", "entry fcp"),
            (LegacyProtocol::Afc, "afc", "entry afc"),
            (LegacyProtocol::Sfcp, "sfcp", "entry sfcp"),
            (LegacyProtocol::Scp, "scp", "entry scp"),
            (LegacyProtocol::Vfcp, "vfcp", "entry vfcp"),
            (LegacyProtocol::Tfcp, "tfcp", "entry tfcp"),
            (LegacyProtocol::Ufcs, "ufcs", "entry ufcs"),
        ];
        for (p, name, entry) in want {
            assert_eq!(p.name(), name);
            assert_eq!(text(&Command::Entry(p.entry())), entry);
        }
    }

    #[test]
    fn not_supported_message_says_so() {
        let msg = Error::NotSupported {
            protocol: LegacyProtocol::Qc,
        }
        .to_string();
        assert!(msg.contains("does not support"), "{msg}");
    }
}
