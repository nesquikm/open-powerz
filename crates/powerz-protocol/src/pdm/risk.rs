//! Pure risk classifier for outgoing PDM commands.
//!
//! - [`Risk::ReadOnly`]: only reads (settings, capability lists); the
//!   charger's output does not change.
//! - [`Risk::AtMost5V`]: session control, protocol entry, detection, hard
//!   reset and requests that provably stay at or below 5 V.
//! - [`Risk::Above5V`]: anything that may raise VBUS above 5 V, including
//!   every command the classifier cannot prove safe (unknown or raw text,
//!   a request for a slot with no known PDO).

use super::caps::Capabilities;
use super::command::Command;
use crate::pd::{Header, MessageKind, Rdo, Sop, SourcePdo};

/// Highest voltage a command may request without a confirmation, mV.
pub const SAFE_LIMIT_MV: u32 = 5000;

/// Risk of one command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Risk {
    /// Changes nothing on the output.
    ReadOnly,
    /// Can change the output, but not above 5 V.
    AtMost5V,
    /// May raise VBUS above 5 V: needs a confirmation.
    Above5V,
}

/// Classify `command` given the last known `caps`. Pure; never panics.
pub fn classify(command: &Command, caps: &Capabilities) -> Risk {
    match command {
        Command::PdmParm | Command::PdPdo | Command::UfcsPdo => Risk::ReadOnly,
        // Session control, protocol entry and detection, resets: the meter
        // negotiates nothing above the default 5 V. [doc] [inferred]
        Command::PdmOpen
        | Command::PdmClose
        | Command::Reset
        | Command::PdmSet { .. }
        | Command::Entry(_)
        | Command::PdHardReset
        // PD control messages (types 1..=24 in USB PD r3.2) carry no data
        // objects, so no voltage request. [doc] [inferred]
        | Command::PdCmd(1..=24)
        // Stepping down never raises VBUS: a QC 3.0 session is entered at
        // 5 V and only `qc3 inc` / `qc3 volt=` raise it. [inferred]
        | Command::Qc3Dec(_) => Risk::AtMost5V,
        // `volt=` is rendered only together with `cur=`, so judge the
        // voltage that actually reaches the meter.
        Command::PdRequest {
            position,
            voltage_mv,
            current_ma,
        } => pd_request(caps, *position, current_ma.and(*voltage_mv)),
        Command::PdData(bytes) => pd_data(caps, bytes),
        Command::FixedVoltage { volts, .. } => by_voltage(u32::from(*volts) * 1000),
        Command::Qc3Volt { voltage_mv }
        | Command::ScpRequest { voltage_mv, .. }
        | Command::VfcpRequest { voltage_mv, .. }
        | Command::TfcpRequest { voltage_mv, .. }
        | Command::UfcsRequest { voltage_mv, .. } => by_voltage(*voltage_mv),
        // Outside the PD control range: not known to be safe.
        // UFCS control numbers: no trusted table of them, so fail closed.
        Command::PdCmd(_) | Command::UfcsCmd(_) | Command::Qc3Inc(_) | Command::Raw(_) => {
            Risk::Above5V
        }
    }
}

fn by_voltage(mv: u32) -> Risk {
    if mv <= SAFE_LIMIT_MV {
        Risk::AtMost5V
    } else {
        Risk::Above5V
    }
}

/// A request for slot `position`: safe only when the last known PDO in that
/// slot (or, for an APDO, the requested voltage) stays at or below 5 V.
fn pd_request(caps: &Capabilities, position: u8, voltage_mv: Option<u32>) -> Risk {
    let Some(pdo) = caps.pd.as_ref().and_then(|b| b.pdo(position)) else {
        return Risk::Above5V;
    };
    // A request below an APDO's range is raised to its minimum by the
    // charger, so the larger of the two is what VBUS can reach. \[inferred\]
    let in_range = |min_mv: u32| voltage_mv.map_or(Risk::Above5V, |v| by_voltage(v.max(min_mv)));
    match pdo {
        SourcePdo::Fixed(f) => by_voltage(f.voltage_mv),
        SourcePdo::Variable(v) => by_voltage(v.max_voltage_mv),
        SourcePdo::Battery(b) => by_voltage(b.max_voltage_mv),
        SourcePdo::Pps(p) => in_range(p.min_voltage_mv),
        // SPR AVS covers 9-20 V by definition. \[doc\]
        SourcePdo::SprAvs(_) => in_range(9000),
        SourcePdo::EprAvs(a) => in_range(a.min_voltage_mv),
        SourcePdo::Reserved(_) => Risk::Above5V,
    }
}

/// `pd data=`: SOP byte, header u16 LE, data objects u32 LE. \[doc\]
/// Control messages are at most 5 V; Request / EPR_Request go by the RDO;
/// every other message is above 5 V.
fn pd_data(caps: &Capabilities, bytes: &[u8]) -> Risk {
    const REQUEST: u8 = 0x02;
    const EPR_REQUEST: u8 = 0x09;
    let [_sop, lo, hi, rest @ ..] = bytes else {
        return Risk::Above5V;
    };
    let header = Header::decode(u16::from_le_bytes([*lo, *hi]), Sop::Sop);
    match header.message_type.kind() {
        MessageKind::Control => Risk::AtMost5V,
        MessageKind::Data if matches!(header.message_type.code(), REQUEST | EPR_REQUEST) => {
            let [a, b, c, d, ..] = *rest else {
                return Risk::Above5V;
            };
            let raw = u32::from_le_bytes([a, b, c, d]);
            let position = Rdo::decode(raw, None).position();
            let pdo = caps.pd.as_ref().and_then(|p| p.pdo(position));
            let voltage_mv = match Rdo::decode(raw, pdo.as_ref()) {
                Rdo::Pps(r) => Some(r.voltage_mv),
                Rdo::Avs(r) => Some(r.voltage_mv),
                _ => None,
            };
            pd_request(caps, position, voltage_mv)
        }
        _ => Risk::Above5V,
    }
}

#[cfg(test)]
mod tests {
    use super::super::caps::fixtures::{epr_block, spr_block};
    use super::super::caps::{CapBlock, ScpCaps, parse};
    use super::super::command::{FixedProtocol, Protocol};
    use super::*;

    fn caps_with(block: Vec<u8>) -> Capabilities {
        let (b, _) = parse(&block).unwrap().unwrap();
        let mut caps = Capabilities::default();
        caps.update(&b);
        caps
    }

    fn pd_req(position: u8) -> Command {
        Command::PdRequest {
            position,
            voltage_mv: None,
            current_ma: None,
        }
    }

    // ---- AC-PR41FT.4: read-only and ≤ 5 V classes ----

    #[test]
    fn read_only_commands() {
        let none = Capabilities::default();
        for c in [Command::PdmParm, Command::PdPdo, Command::UfcsPdo] {
            assert_eq!(classify(&c, &none), Risk::ReadOnly, "{c:?}");
        }
    }

    #[test]
    fn session_entry_and_detection_are_at_most_5v() {
        let none = Capabilities::default();
        let mut cmds = vec![
            Command::PdmOpen,
            Command::PdmClose,
            Command::Reset,
            Command::PdHardReset,
            Command::PdmSet {
                ty: 0,
                em: 2,
                sink: 0,
            },
        ];
        for p in [
            Protocol::Pd,
            Protocol::Ufcs,
            Protocol::Qc,
            Protocol::Fcp,
            Protocol::Scp,
            Protocol::Afc,
            Protocol::Vfcp,
            Protocol::Sfcp,
            Protocol::Tfcp,
            Protocol::Bc,
            Protocol::Apple,
            Protocol::List,
            Protocol::ListPlus,
        ] {
            cmds.push(Command::Entry(p));
        }
        for c in cmds {
            assert_eq!(classify(&c, &none), Risk::AtMost5V, "{c:?}");
        }
    }

    // ---- AC-PR41FT.4: PD requests use the last known pdo block ----

    #[test]
    fn pd_request_by_slot_uses_the_known_pdo() {
        let spr = caps_with(spr_block());
        assert_eq!(classify(&pd_req(1), &spr), Risk::AtMost5V, "5 V fixed");
        assert_eq!(classify(&pd_req(2), &spr), Risk::Above5V, "9 V fixed");
        assert_eq!(classify(&pd_req(4), &spr), Risk::Above5V, "20 V fixed");
        // Slot 8 is unused in the SPR list: nothing proves it safe.
        assert_eq!(classify(&pd_req(8), &spr), Risk::Above5V);
        let epr = caps_with(epr_block());
        assert_eq!(classify(&pd_req(8), &epr), Risk::Above5V, "28 V fixed");
    }

    #[test]
    fn pd_request_without_known_caps_is_above_5v() {
        assert_eq!(
            classify(&pd_req(1), &Capabilities::default()),
            Risk::Above5V
        );
    }

    #[test]
    fn avs_request_classifies_by_requested_voltage() {
        let epr = caps_with(epr_block());
        let avs = |mv| Command::PdRequest {
            position: 9,
            voltage_mv: Some(mv),
            current_ma: Some(1000),
        };
        assert_eq!(classify(&avs(15000), &epr), Risk::Above5V);
        // An APDO request with no voltage cannot be proven safe.
        assert_eq!(
            classify(
                &Command::PdRequest {
                    position: 9,
                    voltage_mv: None,
                    current_ma: Some(1000)
                },
                &epr
            ),
            Risk::Above5V
        );
    }

    #[test]
    fn apdo_request_below_its_range_is_above_5v() {
        // The charger clamps up to the APDO minimum (15 V on the EPR AVS slot),
        // so a 5 V request there still raises VBUS above 5 V.
        let epr = caps_with(epr_block());
        let below = Command::PdRequest {
            position: 9,
            voltage_mv: Some(5000),
            current_ma: Some(1000),
        };
        assert_eq!(classify(&below, &epr), Risk::Above5V);
        // Same through a raw Request: RDO pos 9, AVS 5000 mV (200 x 25 mV), 1 A.
        let raw = Command::parse("pd data=00821014900190");
        assert_eq!(classify(&raw, &epr), Risk::Above5V);
    }

    #[test]
    fn apdo_request_without_current_is_above_5v() {
        // `pd req=<n>,volt=` is rendered only together with `cur=`, so a
        // voltage with no current never reaches the meter.
        let epr = caps_with(epr_block());
        let no_cur = Command::PdRequest {
            position: 9,
            voltage_mv: Some(5000),
            current_ma: None,
        };
        assert_eq!(classify(&no_cur, &epr), Risk::Above5V);
    }

    #[test]
    fn pd_cmd_is_at_most_5v_only_for_pd_control_message_types() {
        let none = Capabilities::default();
        // Get_Source_Cap (7) and Get_Revision (24) are control messages.
        assert_eq!(classify(&Command::PdCmd(7), &none), Risk::AtMost5V);
        assert_eq!(classify(&Command::PdCmd(24), &none), Risk::AtMost5V);
        // 0 and anything past the control range are not known to be safe.
        assert_eq!(classify(&Command::PdCmd(0), &none), Risk::Above5V);
        assert_eq!(classify(&Command::PdCmd(25), &none), Risk::Above5V);
        assert_eq!(classify(&Command::PdCmd(200), &none), Risk::Above5V);
    }

    #[test]
    fn ufcs_cmd_is_not_proven_safe() {
        // No trusted table of UFCS control numbers: fail closed.
        let none = Capabilities::default();
        assert_eq!(classify(&Command::UfcsCmd(6), &none), Risk::Above5V);
    }

    // ---- AC-PR41FT.4: other protocols by requested voltage ----

    #[test]
    fn legacy_requests_classify_by_voltage() {
        let none = Capabilities::default();
        let fixed = |protocol, volts| Command::FixedVoltage { protocol, volts };
        assert_eq!(
            classify(&fixed(FixedProtocol::Qc, 5), &none),
            Risk::AtMost5V
        );
        assert_eq!(classify(&fixed(FixedProtocol::Qc, 9), &none), Risk::Above5V);
        assert_eq!(
            classify(&fixed(FixedProtocol::Fcp, 12), &none),
            Risk::Above5V
        );
        assert_eq!(
            classify(&fixed(FixedProtocol::Afc, 5), &none),
            Risk::AtMost5V
        );
        assert_eq!(
            classify(&Command::Qc3Volt { voltage_mv: 5000 }, &none),
            Risk::AtMost5V
        );
        assert_eq!(
            classify(&Command::Qc3Volt { voltage_mv: 5200 }, &none),
            Risk::Above5V
        );
        assert_eq!(classify(&Command::Qc3Inc(1), &none), Risk::Above5V);
        let scp = |mv| Command::ScpRequest {
            voltage_mv: mv,
            current_ma: 1000,
        };
        assert_eq!(classify(&scp(5000), &none), Risk::AtMost5V);
        assert_eq!(classify(&scp(9000), &none), Risk::Above5V);
        assert_eq!(
            classify(
                &Command::VfcpRequest {
                    voltage_mv: 9000,
                    current_ma: 1000
                },
                &none
            ),
            Risk::Above5V
        );
        assert_eq!(
            classify(
                &Command::TfcpRequest {
                    voltage_mv: 4500,
                    current_ma: 1000
                },
                &none
            ),
            Risk::AtMost5V
        );
        assert_eq!(
            classify(
                &Command::UfcsRequest {
                    index: 1,
                    voltage_mv: 20000,
                    current_ma: 1000
                },
                &none
            ),
            Risk::Above5V
        );
        // Known caps do not lower the risk of a high request.
        let mut caps = Capabilities::default();
        caps.update(&CapBlock::Scp(ScpCaps {
            max_power_mw: 40_000,
            min_voltage_mv: 3400,
            max_voltage_mv: 20000,
            min_current_ma: 100,
            max_current_ma: 4000,
            voltage_step_mv: 10,
            current_step_ma: 50,
            bound_voltage_mv: 11000,
            bound_current_ma: 5000,
        }));
        assert_eq!(classify(&scp(9000), &caps), Risk::Above5V);
    }

    // ---- AC-PR41FT.4: raw PD messages ----

    #[test]
    fn pd_data_request_uses_the_rdo_position() {
        let spr = caps_with(spr_block());
        // Request, RDO 0x2304B12C: object position 2 (9 V).
        let req9 = Command::PdData(vec![0x00, 0x82, 0x10, 0x2C, 0xB1, 0x04, 0x23]);
        assert_eq!(classify(&req9, &spr), Risk::Above5V);
        // Soft_Reset carries no request.
        let soft_reset = Command::PdData(vec![0x00, 0x8D, 0x00]);
        assert_eq!(classify(&soft_reset, &spr), Risk::AtMost5V);
    }

    // ---- AC-PR41FT.4: unknown or unparsable is above 5 V ----

    #[test]
    fn unknown_and_unparsable_commands_are_above_5v() {
        let spr = caps_with(spr_block());
        for text in ["bogus", "pd req=abc", "PDM PARM", "qc3 volt=lots", ""] {
            let c = Command::parse(text);
            assert_eq!(classify(&c, &spr), Risk::Above5V, "{text:?}");
        }
        assert_eq!(
            classify(&Command::Raw("pd req=7".into()), &spr),
            Risk::Above5V
        );
    }
}
